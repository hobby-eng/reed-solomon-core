//! What a shard file says about its set, and the sizes that follow from it.

use super::FileError;
use super::header::{self, TRAILER};
use crate::code::Code;
use crate::field::Field;
use sha2::{Digest as _, Sha256};

/// The longest identifier of a set, in bytes.
pub(super) const ID_MOST: usize = 32;

/// Bytes of the digest of the data.
pub(super) const DIGEST: usize = 32;

/// The most blocks a shard has: a block's number is written in four bytes.
const BLOCKS_MOST: u128 = 1 << 32;

/// The SHA-256 of `data`, by the `sha2` crate.
pub(super) fn sha256(data: &[u8]) -> [u8; DIGEST] {
    Sha256::digest(data).into()
}

/// Whether the files of a set state the SHA-256 of the data. The crate computes it from the data
/// when [`SetFacts::for_data`] describes the set, and compares it with the rebuilt data before
/// it gives the data back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DataDigest {
    /// The files state no digest. A rebuild then says that nothing was compared.
    None,
    /// The files state the SHA-256 of the data: 32 more bytes in each copy of the header.
    Sha256,
}

/// What every shard file of a set says about the set: the code, the set's identifier, the length
/// of the data, the length of a block and, when the program gave one, the digest of the data.
/// Files that say the same belong to one set; the lengths of a shard and of a file follow from
/// it and are the same for every file of the set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SetFacts {
    code: Code,
    /// The identifier, with zeros after its `id_len` bytes.
    id: [u8; ID_MOST],
    id_len: u8,
    data_len: u64,
    block_len: u32,
    digest: Option<[u8; DIGEST]>,
    sizes: Sizes,
}

/// The sizes that follow from the facts, worked out once when they are checked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Sizes {
    shard: u64,
    blocks: u64,
    file: u64,
}

impl SetFacts {
    /// The facts of a set: its field and threshold, its identifier `id` of 1 to 32 bytes, the
    /// length of its data in bytes, the length of a block in bytes and, optionally, the SHA-256
    /// of the data.
    ///
    /// The identifier tells the sets of one program apart: the program chooses it, and two sets
    /// never share one. A block is the unit of damage: a wrong byte costs the block it is in, and
    /// each block costs four bytes of checksum.
    ///
    /// The digest is the one the files will state: [`SetFacts::files`] writes them only for data
    /// that has it, and a rebuild gives the data back only when it has it. A program that holds
    /// the data describes the set with [`SetFacts::for_data`], which computes the digest; this
    /// call is for facts known otherwise, such as those of a header that was read.
    ///
    /// # Errors
    ///
    /// [`FileError::Field`] for [`Field::Gf2048`], whose symbols are not the bytes of a file;
    /// [`FileError::Id`] for an identifier that is empty or longer than 32 bytes;
    /// [`FileError::Threshold`] for a threshold the field does not allow; [`FileError::Block`]
    /// for a block length of zero or of half a symbol; [`FileError::Blocks`] when a shard would
    /// have more than 2^32 blocks; [`FileError::Size`] when a file would be longer than 2^64 − 1
    /// bytes.
    pub fn new(
        field: Field,
        id: &[u8],
        threshold: usize,
        data_len: u64,
        block_len: u32,
        digest: Option<[u8; DIGEST]>,
    ) -> Result<Self, FileError> {
        if header::field_code(field).is_none() {
            return Err(FileError::Field);
        }
        let id_len = u8::try_from(id.len()).map_err(|_| FileError::Id)?;
        if id.is_empty() || id.len() > ID_MOST {
            return Err(FileError::Id);
        }
        let mut owned = [0; ID_MOST];
        for (slot, byte) in owned.iter_mut().zip(id) {
            *slot = *byte;
        }
        let code = Code::new(field, threshold).map_err(|_| FileError::Threshold)?;
        let header_len = header::length(id.len(), digest.is_some());
        let sizes = Sizes::of(&code, data_len, block_len, header_len)?;
        Ok(Self {
            code,
            id: owned,
            id_len,
            data_len,
            block_len,
            digest,
            sizes,
        })
    }

    /// The facts of the set whose data is `data`, as [`SetFacts::new`] takes them, with the length
    /// of the data and, for [`DataDigest::Sha256`], its SHA-256 computed from `data` itself.
    ///
    /// # Errors
    ///
    /// As [`SetFacts::new`], whose checks come first; the digest is computed only for facts that
    /// pass them.
    pub fn for_data(
        field: Field,
        id: &[u8],
        threshold: usize,
        data: &[u8],
        block_len: u32,
        digest: DataDigest,
    ) -> Result<Self, FileError> {
        let data_len = u64::try_from(data.len()).map_err(|_| FileError::Size)?;
        let facts = Self::new(field, id, threshold, data_len, block_len, None)?;
        match digest {
            DataDigest::None => Ok(facts),
            DataDigest::Sha256 => Self::new(
                field,
                id,
                threshold,
                data_len,
                block_len,
                Some(sha256(data)),
            ),
        }
    }

    /// The code of the set: its field and its threshold.
    #[must_use]
    pub const fn code(&self) -> Code {
        self.code
    }

    /// The identifier of the set.
    #[must_use]
    pub fn id(&self) -> &[u8] {
        self.id.get(..usize::from(self.id_len)).unwrap_or_default()
    }

    /// The length of the data in bytes.
    #[must_use]
    pub const fn data_len(&self) -> u64 {
        self.data_len
    }

    /// The length of a block in bytes; the last block of a shard may be shorter.
    #[must_use]
    pub const fn block_len(&self) -> u32 {
        self.block_len
    }

    /// The SHA-256 of the data that the files of the set state, if they state one. It says
    /// whether the data came back unchanged, not who wrote the set: whoever changes the data can
    /// state its new digest. Only a digest the program trusts from elsewhere tells that.
    #[must_use]
    pub const fn digest(&self) -> Option<&[u8; DIGEST]> {
        self.digest.as_ref()
    }

    /// The length of every shard of the set in bytes: the data length divided by the threshold,
    /// rounded up to a whole number of symbols.
    #[must_use]
    pub const fn shard_len(&self) -> u64 {
        self.sizes.shard
    }

    /// How many blocks every shard of the set has: none for empty data, at most 2^32.
    #[must_use]
    pub const fn blocks(&self) -> u64 {
        self.sizes.blocks
    }

    /// The length of every shard file of the set in bytes.
    #[must_use]
    pub const fn file_len(&self) -> u64 {
        self.sizes.file
    }
}

#[cfg(feature = "alloc")]
impl SetFacts {
    /// The facts as a value to sort sets by: sets of one identifier come together, and the
    /// order of the files they were found in plays no part.
    pub(super) fn key(&self) -> impl Ord + use<> {
        let field = header::field_code(self.code.field());
        let threshold = self.code.threshold();
        (
            self.id,
            self.id_len,
            field,
            threshold,
            self.data_len,
            self.block_len,
            self.digest,
        )
    }
}

impl Sizes {
    /// The sizes of a set, each worked out in 128 bits and checked before it is narrowed. The
    /// divisions round up through the quotient and the remainder, with no sum that could pass
    /// the end of its type.
    fn of(
        code: &Code,
        data_len: u64,
        block_len: u32,
        header_len: usize,
    ) -> Result<Self, FileError> {
        let width = wide(code.field().symbol_bytes());
        let block = u128::from(block_len);
        if block == 0 || !block.is_multiple_of(width) {
            return Err(FileError::Block);
        }
        let shard = ceiling(u128::from(data_len), wide(code.threshold()))
            .ok_or(FileError::Threshold)?
            .next_multiple_of(width);
        let blocks = ceiling(shard, block).ok_or(FileError::Block)?;
        if blocks > BLOCKS_MOST {
            return Err(FileError::Blocks);
        }
        // Two copies of the header, the blocks with a checksum each, and the trailer.
        let file = 2 * wide(header_len) + shard + 4 * blocks + wide(TRAILER);
        Ok(Self {
            shard: u64::try_from(shard).map_err(|_| FileError::Size)?,
            blocks: u64::try_from(blocks).map_err(|_| FileError::Size)?,
            file: u64::try_from(file).map_err(|_| FileError::Size)?,
        })
    }
}

/// A count as 128 bits: every count here is far below that.
fn wide(count: usize) -> u128 {
    u128::try_from(count).unwrap_or(u128::MAX)
}

/// `value / by`, rounded up; `None` when `by` is zero.
fn ceiling(value: u128, by: u128) -> Option<u128> {
    let whole = value.checked_div(by)?;
    let rest = value.checked_rem(by)?;
    Some(whole + u128::from(rest != 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(field: Field, threshold: usize, data_len: u64, block_len: u32) -> (u64, u64, u64) {
        let facts = SetFacts::new(field, &[1], threshold, data_len, block_len, None).unwrap();
        (facts.shard_len(), facts.blocks(), facts.file_len())
    }

    /// Worked out by hand: a header of 33 bytes twice, a trailer of 12, four bytes per block.
    #[test]
    fn the_sizes_follow_from_the_facts() {
        assert_eq!(sizes(Field::Gf256, 3, 10, 4), (4, 1, 86));
        assert_eq!(sizes(Field::Gf65536, 3, 10, 4), (4, 1, 86));
        assert_eq!(sizes(Field::Gf256, 3, 0, 4), (0, 0, 78));
        assert_eq!(sizes(Field::Gf256, 3, 1, 4), (1, 1, 83));
        assert_eq!(sizes(Field::Gf65536, 3, 1, 4), (2, 1, 84));
        assert_eq!(sizes(Field::Gf256, 3, 9, 1), (3, 3, 93));
        assert_eq!(sizes(Field::Gf256, 3, 10, 3), (4, 2, 90));
        assert_eq!(sizes(Field::Gf65536, 2, 13, 2), (8, 4, 102));
        assert_eq!(
            sizes(Field::Gf256, 1, 1 << 32, 1),
            (1 << 32, 1 << 32, 78 + 5 * (1 << 32))
        );
    }

    #[test]
    fn the_rounding_passes_no_end_of_its_type() {
        assert_eq!(ceiling(u128::from(u64::MAX), 1), Some(u128::from(u64::MAX)));
        assert_eq!(ceiling(u128::from(u64::MAX), 2), Some(1 << 63));
        assert_eq!(ceiling(0, 7), Some(0));
        assert_eq!(ceiling(7, 0), None);
    }
}
