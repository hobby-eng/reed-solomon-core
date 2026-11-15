//! The header of a shard file: its bytes, the checks of one copy, and the trailer that names the
//! length of the copy at the end.
//!
//! Every number is big-endian and has a fixed width, and no bit is free, so a header that passes
//! the checks is written back to the same bytes.

use super::FileError;
use super::crc32c::checksum;
use super::facts::{DIGEST, ID_MOST, SetFacts};
use crate::field::Field;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;
#[cfg(feature = "alloc")]
use zeroize::Zeroizing;

/// The mark a header starts with.
const MARK: [u8; 4] = *b"SHRD";

/// The mark the trailer starts with.
const TRAILER_MARK: [u8; 4] = *b"SHRT";

/// The version of the format written and read here.
const VERSION: u8 = 1;

/// The flag of a header that holds the digest of the data. Every other flag is zero.
const FLAG_DIGEST: u8 = 1;

/// Bytes of a header besides the identifier and the digest: the mark, four single bytes, the
/// threshold, the index, the data length, the block length and the checksum.
const FIXED: usize = 32;

/// The longest header: a 32-byte identifier and a digest.
pub(super) const MOST: usize = FIXED + ID_MOST + DIGEST;

/// The shortest header: a one-byte identifier and no digest.
const LEAST: usize = FIXED + 1;

/// Bytes of the trailer: its mark, the length of a header and their checksum.
pub(super) const TRAILER: usize = 12;

/// Bytes of a checksum.
pub(super) const CHECKSUM: usize = 4;

/// [`CHECKSUM`] in the type of a header's length.
const CHECKSUM_BYTE: u8 = 4;

/// The number a header writes for a field. 3 is kept for [`Field::Gf2048`] and is not written:
/// the symbols of that field are not the bytes of a file.
pub(super) const fn field_code(field: Field) -> Option<u8> {
    match field {
        Field::Gf256 => Some(1),
        Field::Gf65536 => Some(2),
        Field::Gf2048 | Field::Binary(_) => None,
    }
}

/// The field a header's number stands for, when this version reads it.
const fn field_of(code: u8) -> Option<Field> {
    match code {
        1 => Some(Field::Gf256),
        2 => Some(Field::Gf65536),
        _ => None,
    }
}

/// The length of a header with an identifier of `id_len` bytes.
pub(super) const fn length(id_len: usize, digest: bool) -> usize {
    FIXED + id_len + if digest { DIGEST } else { 0 }
}

/// The header of one shard file: the facts of its set and the index of its shard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    set: SetFacts,
    index: u16,
    /// The header as written, with zeros after its `len` bytes.
    bytes: [u8; MOST],
    len: u8,
}

impl Header {
    /// The header of shard `index` of `set`.
    ///
    /// # Errors
    ///
    /// [`FileError::Index`] for an index the set's field does not have.
    pub fn new(set: SetFacts, index: u16) -> Result<Self, FileError> {
        let field = set.code().field();
        field.point(index).ok_or(FileError::Index)?;
        let code = field_code(field).ok_or(FileError::Field)?;
        let threshold = u32::try_from(set.code().threshold()).map_err(|_| FileError::Threshold)?;
        let id_len = u8::try_from(set.id().len()).map_err(|_| FileError::Id)?;
        let flags = if set.digest().is_some() {
            FLAG_DIGEST
        } else {
            0
        };
        let fields = MARK
            .into_iter()
            .chain([VERSION, code, flags, id_len])
            .chain(set.id().iter().copied())
            .chain(threshold.to_be_bytes())
            .chain(u32::from(index).to_be_bytes())
            .chain(set.data_len().to_be_bytes())
            .chain(set.block_len().to_be_bytes())
            .chain(set.digest().into_iter().flatten().copied());
        let mut bytes = [0; MOST];
        // At most 92 bytes before the checksum, so the count stays far below its type's end.
        let mut covered = 0_u8;
        for (slot, byte) in bytes.iter_mut().zip(fields) {
            *slot = byte;
            covered += 1;
        }
        let end = usize::from(covered);
        let sum = checksum(&[bytes.get(..end).unwrap_or_default()]);
        for (slot, byte) in bytes.iter_mut().skip(end).zip(sum.to_be_bytes()) {
            *slot = byte;
        }
        Ok(Self {
            set,
            index,
            bytes,
            len: covered + CHECKSUM_BYTE,
        })
    }

    /// The facts of the set the file belongs to.
    #[must_use]
    pub const fn set(&self) -> &SetFacts {
        &self.set
    }

    /// The index of the file's shard in its set.
    #[must_use]
    pub const fn index(&self) -> u16 {
        self.index
    }

    /// The bytes of the header: a shard file starts with them and holds them again before its
    /// trailer.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or_default()
    }

    /// The trailer: the last twelve bytes of the file, which name the length of the header.
    #[must_use]
    pub fn trailer(&self) -> [u8; TRAILER] {
        trailer(u32::from(self.len))
    }

    /// The checksum written after block `number` of the shard, whose bytes are `block`. It
    /// covers the header's own checksum and the block's number as well, so the bytes of another
    /// file or of another place do not pass unless their checksum happens to be the same.
    #[must_use]
    pub fn block_checksum(&self, number: u32, block: &[u8]) -> u32 {
        let own = self
            .as_bytes()
            .last_chunk::<CHECKSUM>()
            .copied()
            .unwrap_or_default();
        checksum(&[&own, &number.to_be_bytes(), block])
    }

    /// Where block `number` of the shard is in the file: the place of its first byte and the
    /// number of its bytes, its checksum not counted. `None` when the shard has no such block.
    pub(super) fn span(&self, number: u32) -> Option<(u64, u64)> {
        let (number, block) = (u64::from(number), u64::from(self.set.block_len()));
        if number >= self.set.blocks() {
            return None;
        }
        let length = self
            .set
            .shard_len()
            .checked_sub(number.checked_mul(block)?)?
            .min(block);
        // Every block before it is a whole one, followed by its checksum.
        let each = block.checked_add(u64::from(CHECKSUM_BYTE))?;
        let start = u64::from(self.len).checked_add(number.checked_mul(each)?)?;
        Some((start, length))
    }

    /// The whole shard file for `shard`: header, blocks with checksums, the header again and the
    /// trailer, in a [`Zeroizing`] buffer. Fails with [`FileError::Shard`] for a wrong length or
    /// [`FileError::Memory`] when there's no memory.
    #[cfg(feature = "alloc")]
    pub fn file(&self, shard: &[u8]) -> Result<Zeroizing<Vec<u8>>, FileError> {
        if u64::try_from(shard.len()) != Ok(self.set.shard_len()) {
            return Err(FileError::Shard);
        }
        let length = usize::try_from(self.set.file_len()).map_err(|_| FileError::Memory)?;
        // A block longer than this machine addresses is longer than the shard: one block.
        let block = usize::try_from(self.set.block_len()).unwrap_or(usize::MAX);
        // The bytes given do not promise memory for a second copy of them: the memory is asked
        // for once, and a refusal is an answer of its own.
        let mut file = Zeroizing::new(Vec::new());
        file.try_reserve_exact(length)
            .map_err(|_| FileError::Memory)?;
        file.extend_from_slice(self.as_bytes());
        for (number, bytes) in (0..=u32::MAX).zip(shard.chunks(block)) {
            file.extend_from_slice(bytes);
            file.extend_from_slice(&self.block_checksum(number, bytes).to_be_bytes());
        }
        file.extend_from_slice(self.as_bytes());
        file.extend_from_slice(&self.trailer());
        Ok(file)
    }
}

/// The trailer of a file whose header is `header_len` bytes long.
fn trailer(header_len: u32) -> [u8; TRAILER] {
    let mut bytes = [0; TRAILER];
    let named = TRAILER_MARK.into_iter().chain(header_len.to_be_bytes());
    for (slot, byte) in bytes.iter_mut().zip(named) {
        *slot = byte;
    }
    let sum = checksum(&[bytes.get(..TRAILER - CHECKSUM).unwrap_or_default()]);
    for (slot, byte) in bytes
        .iter_mut()
        .skip(TRAILER - CHECKSUM)
        .zip(sum.to_be_bytes())
    {
        *slot = byte;
    }
    bytes
}

/// The length of a header that `bytes` names, when they are a trailer of this version: its mark,
/// a length a header can have, and the checksum of both.
fn trailer_names(bytes: &[u8; TRAILER]) -> Option<usize> {
    let mut reader = Reader(bytes);
    if reader.array() != Some(TRAILER_MARK) {
        return None;
    }
    let named = u32::from_be_bytes(reader.array()?);
    let length = usize::try_from(named).ok()?;
    ((LEAST..=MOST).contains(&length) && trailer(named) == *bytes).then_some(length)
}

/// What a place of a file holds where a copy of the header belongs.
#[derive(Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a finding lives on the stack for one reading, and without an allocator there is \
              nothing to box its header into"
)]
pub(super) enum Found {
    /// A header of this version that passes every check.
    Sound(Header),
    /// This many bytes whose checksum holds by this version's layout and that name a field or a
    /// flag this version does not read. Their sizes cannot be worked out.
    Unsupported(usize),
    /// The mark of a header and the number of another version. Nothing else of it is checked: a
    /// later version may lay its header out differently, and a version byte gone wrong in a file
    /// of this version looks the same.
    Version(u8),
    /// The mark of a header or a trailer, and no header that holds.
    Damaged,
    /// No mark.
    Absent,
}

impl Found {
    /// What `bytes` hold at their start.
    pub(super) fn at_start(bytes: &[u8]) -> Self {
        let mut reader = Reader(bytes);
        if reader.array() != Some(MARK) {
            return Self::Absent;
        }
        // The mark and one byte name a version: nothing after them is read for another one.
        let Some([version]) = reader.array() else {
            return Self::Damaged;
        };
        if version != VERSION {
            return Self::Version(version);
        }
        let Some([field, flags, id_len]) = reader.array() else {
            return Self::Damaged;
        };
        // The identifier's length and the digest flag say where the checksum is.
        let id_len = usize::from(id_len);
        let digest = flags & FLAG_DIGEST != 0;
        if !(1..=ID_MOST).contains(&id_len) {
            return Self::Damaged;
        }
        let Some(copy) = bytes.get(..length(id_len, digest)) else {
            return Self::Damaged;
        };
        let Some((covered, stored)) = copy.split_last_chunk::<CHECKSUM>() else {
            return Self::Damaged;
        };
        if checksum(&[covered]) != u32::from_be_bytes(*stored) {
            return Self::Damaged;
        }
        match field_of(field) {
            Some(field) if flags & !FLAG_DIGEST == 0 => {
                decode(field, id_len, digest, reader).map_or(Self::Damaged, Self::Sound)
            }
            _ => Self::Unsupported(copy.len()),
        }
    }

    /// What the end of a file holds, found through the trailer in its last twelve bytes. A
    /// header counts only where its own sizes put it: its length is the one the trailer names,
    /// and the file is as long as the header says.
    pub(super) fn at_end(file: &[u8]) -> Self {
        let Some((before, last)) = file.split_last_chunk::<TRAILER>() else {
            return Self::Absent;
        };
        let Some(named) = trailer_names(last) else {
            // The mark of a trailer that does not hold is still the mark of a shard file.
            return if last.starts_with(&TRAILER_MARK) {
                Self::Damaged
            } else {
                Self::Absent
            };
        };
        let copy = before
            .len()
            .checked_sub(named)
            .and_then(|start| before.get(start..));
        match copy.map(Self::at_start) {
            Some(Self::Sound(header))
                if header.as_bytes().len() == named
                    && u64::try_from(file.len()) == Ok(header.set.file_len()) =>
            {
                Self::Sound(header)
            }
            Some(Self::Unsupported(length)) if length == named => Self::Unsupported(length),
            Some(Self::Version(version)) => Self::Version(version),
            _ => Self::Damaged,
        }
    }
}

/// The header whose fields after the first eight bytes `reader` holds, when every value is one
/// the format allows. The checksum was checked by the caller.
fn decode(field: Field, id_len: usize, digest: bool, mut reader: Reader<'_>) -> Option<Header> {
    let id = reader.bytes(id_len)?;
    let threshold = u32::from_be_bytes(reader.array()?);
    let index = u32::from_be_bytes(reader.array()?);
    let data_len = u64::from_be_bytes(reader.array()?);
    let block_len = u32::from_be_bytes(reader.array()?);
    let digest = if digest { Some(reader.array()?) } else { None };
    let threshold = usize::try_from(threshold).ok()?;
    let set = SetFacts::new(field, id, threshold, data_len, block_len, digest).ok()?;
    // The index is checked against the field before it is narrowed: one that does not fit
    // sixteen bits is in no field.
    Header::new(set, u16::try_from(index).ok()?).ok()
}

/// Bytes read from the front, piece by piece.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    /// The next `N` bytes.
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.0.split_first_chunk::<N>()?;
        self.0 = rest;
        Some(*head)
    }

    /// The next `count` bytes.
    fn bytes(&mut self, count: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        Some(head)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(field: Field, digest: Option<[u8; DIGEST]>) -> Header {
        let set = SetFacts::new(field, &[1], 3, 10, 4, digest).unwrap();
        Header::new(set, 4).unwrap()
    }

    #[test]
    fn a_header_is_as_long_as_its_identifier_and_digest_make_it() {
        assert_eq!((LEAST, MOST), (33, 96));
        assert_eq!(header(Field::Gf256, None).as_bytes().len(), 33);
        assert_eq!(header(Field::Gf256, Some([0; DIGEST])).as_bytes().len(), 65);
        let set = SetFacts::new(Field::Gf65536, &[7; 32], 3, 10, 4, Some([0; DIGEST])).unwrap();
        assert_eq!(Header::new(set, 4).unwrap().as_bytes().len(), 96);
    }

    #[test]
    fn a_sound_header_is_read_back_as_itself() {
        for field in [Field::Gf256, Field::Gf65536] {
            for digest in [None, Some([0xA5; DIGEST])] {
                let written = header(field, digest);
                assert_eq!(Found::at_start(written.as_bytes()), Found::Sound(written));
            }
        }
    }

    #[test]
    fn a_trailer_names_only_the_length_of_a_header() {
        assert_eq!(trailer_names(&trailer(33)), Some(33));
        assert_eq!(trailer_names(&trailer(96)), Some(96));
        assert_eq!(trailer_names(&trailer(32)), None);
        assert_eq!(trailer_names(&trailer(97)), None);
        let mut changed = trailer(33);
        changed[7] ^= 1;
        assert_eq!(trailer_names(&changed), None);
    }

    #[test]
    fn a_block_of_the_last_number_has_its_place() {
        // 2^32 blocks of one byte: the last block is number FFFFFFFF.
        let set = SetFacts::new(Field::Gf256, &[1], 1, 1 << 32, 1, None).unwrap();
        let header = Header::new(set, 0).unwrap();
        assert_eq!(header.span(0), Some((33, 1)));
        assert_eq!(
            header.span(u32::MAX),
            Some((33 + 5 * u64::from(u32::MAX), 1))
        );
        // A short last block, and no block after it.
        let set = SetFacts::new(Field::Gf256, &[1], 3, 10, 3, None).unwrap();
        let header = Header::new(set, 0).unwrap();
        assert_eq!(header.span(0), Some((33, 3)));
        assert_eq!(header.span(1), Some((40, 1)));
        assert_eq!(header.span(2), None);
    }
}
