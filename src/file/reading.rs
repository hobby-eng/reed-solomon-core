//! Reading a shard file: which copy of the header to believe, what is left of the file, and its
//! blocks one by one.

use super::FileError;
use super::header::{CHECKSUM, Found, Header, MOST, TRAILER};
use core::convert::Infallible;
use core::fmt;

/// How a piece of a shard file was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Condition {
    /// Its bytes are there and pass their check.
    Intact,
    /// Its bytes are there and do not pass.
    Damaged,
    /// The file ends before its last byte.
    Missing,
}

/// The length of a file against the length its header gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Length {
    /// As long as the header says.
    Exact,
    /// Shorter by this many bytes: the end of the file is lost.
    Short(u64),
    /// Longer by this many bytes, which belong to nothing.
    Long(u64),
}

/// What surrounds the blocks of a shard file, as it was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The header at the start. When it is damaged the file was read by the copy at the end.
    pub lead: Condition,
    /// The copy of the header at the end.
    pub tail: Condition,
    /// The trailer after that copy.
    pub trailer: Condition,
    /// The length of the file.
    pub length: Length,
}

/// One block of a shard, as it was found. Its `Debug` text gives the length of an intact block,
/// not its bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Block<'a> {
    /// The bytes of the block, whose checksum holds.
    Intact(&'a [u8]),
    /// The block is there and its checksum does not hold: its bytes are not used.
    Damaged,
    /// The file ends before the end of the block.
    Missing,
}

impl fmt::Debug for Block<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Intact(bytes) => write!(f, "Intact({} bytes)", bytes.len()),
            Self::Damaged => f.write_str("Damaged"),
            Self::Missing => f.write_str("Missing"),
        }
    }
}

/// A count of a shard file's pieces by their condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Survey {
    /// What surrounds the blocks.
    pub frame: Frame,
    /// Blocks whose checksum holds.
    pub intact: u64,
    /// Blocks that are there and do not pass.
    pub damaged: u64,
    /// Blocks the file ends before.
    pub missing: u64,
}

impl Survey {
    /// Whether the file is exactly what [`Header::file`] writes for its header and the bytes of
    /// its blocks: every piece intact and the length exact.
    #[must_use]
    pub fn whole(&self) -> bool {
        let Frame {
            lead,
            tail,
            trailer,
            length,
        } = self.frame;
        [lead, tail, trailer] == [Condition::Intact; 3]
            && length == Length::Exact
            && self.damaged == 0
            && self.missing == 0
    }
}

/// A shard file that was read: its header and the bytes its blocks are taken from.
///
/// Reading decides which copy of the header holds and looks at no block; [`ShardFile::block`]
/// checks one block when it is asked for, and [`ShardFile::survey`] checks them all. Nothing is
/// allocated, and the work is bounded by the length of the bytes given, whatever the header
/// says. Its `Debug` text gives the header, the frame and the length of the bytes, not the bytes.
#[derive(Clone, Copy)]
pub struct ShardFile<'a> {
    header: Header,
    bytes: &'a [u8],
    frame: Frame,
}

impl fmt::Debug for ShardFile<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShardFile")
            .field("header", &self.header)
            .field("frame", &self.frame)
            .field("length", &self.bytes.len())
            .finish()
    }
}

impl<'a> ShardFile<'a> {
    /// Reads the shard file `bytes`: the whole file, or what is left of it.
    ///
    /// The header at the start and the copy at the end are judged apart. A copy holds when its
    /// checksum and every value pass; the copy at the end counts only where its own sizes put
    /// it. One copy that holds is enough to read the file, and [`ShardFile::frame`] names what
    /// the other was. A file cut short is read by the header at its start and keeps the blocks
    /// that are wholly there; bytes after the end are reported and used for nothing.
    ///
    /// # Errors
    ///
    /// [`FileError::Conflict`] when two headers hold, each in a place that is certain, and
    /// differ: neither is chosen. [`FileError::Unsupported`] when no header holds and one, whose
    /// checksum holds, names a field or a flag this version does not read.
    /// [`FileError::Version`] when no header's checksum holds and one names another version of
    /// the format. [`FileError::Damaged`] when the marks of a shard file are there and no header
    /// holds. [`FileError::Unrecognized`] when the bytes have no mark of a shard file at their
    /// start or their end, which is also what a file that lost both gives.
    pub fn read(bytes: &'a [u8]) -> Result<Self, FileError> {
        let lead = Found::at_start(bytes);
        let last = Found::at_end(bytes);
        let header = believed(&lead, &last)?;
        let frame = frame(&header, bytes, &lead)?;
        Ok(Self {
            header,
            bytes,
            frame,
        })
    }

    /// The header of the file.
    #[must_use]
    pub const fn header(&self) -> &Header {
        &self.header
    }

    /// The condition of the two copies of the header, of the trailer and of the file's length.
    #[must_use]
    pub const fn frame(&self) -> Frame {
        self.frame
    }

    /// Block `number` of the shard, its checksum checked now; `None` when the shard has no such
    /// block. The bytes come from the block's own place between the two copies of the header and
    /// from nowhere else.
    #[must_use]
    pub fn block(&self, number: u32) -> Option<Block<'a>> {
        let (start, length) = self.header.span(number)?;
        let stored = usize::try_from(length)
            .ok()
            .and_then(|length| length.checked_add(CHECKSUM))
            .and_then(|length| piece(self.bytes, start, length));
        let Some((block, sum)) = stored.and_then(<[u8]>::split_last_chunk::<CHECKSUM>) else {
            return Some(Block::Missing);
        };
        let holds = self.header.block_checksum(number, block) == u32::from_be_bytes(*sum);
        Some(if holds {
            Block::Intact(block)
        } else {
            Block::Damaged
        })
    }

    /// Checks every block that is wholly there and counts the pieces of the file by their
    /// condition: one pass over the bytes given.
    #[must_use]
    pub fn survey(&self) -> Survey {
        match self.walk(|_| Ok::<(), Infallible>(())) {
            Ok(survey) => survey,
            Err(never) => match never {},
        }
    }

    /// The survey of the file, with `intact` called for the number of every block whose
    /// checksum holds, in order; its refusal ends the walk.
    pub(super) fn walk<E>(
        &self,
        mut intact: impl FnMut(u32) -> Result<(), E>,
    ) -> Result<Survey, E> {
        let (mut whole, mut damaged) = (0_u64, 0_u64);
        // The blocks lie one after another: after the first one the file ends before, every
        // later one is missing too, so the pass ends there however many the header counts.
        for number in 0..=u32::MAX {
            match self.block(number) {
                Some(Block::Intact(_)) => {
                    intact(number)?;
                    whole += 1;
                }
                Some(Block::Damaged) => damaged += 1,
                Some(Block::Missing) | None => break,
            }
        }
        let missing = self.header.set().blocks().saturating_sub(whole + damaged);
        Ok(Survey {
            frame: self.frame,
            intact: whole,
            damaged,
            missing,
        })
    }

    /// The bytes in the place of block `number`, unchecked: for a caller that walked the file
    /// and knows the block's checksum to hold.
    #[cfg(feature = "alloc")]
    pub(super) fn bytes_at(&self, number: u32) -> Option<&'a [u8]> {
        let (start, length) = self.header.span(number)?;
        piece(self.bytes, start, usize::try_from(length).ok()?)
    }
}

/// The header to read a file by, from what its start and its end hold.
///
/// Two headers whose places are certain and that differ are never chosen between. The place of
/// the start is certain; a sound header at the end is where its own sizes put it. An unsupported
/// header at the end has sizes nobody can work out, so next to a sound start it decides nothing
/// here: [`frame`] refuses it in the place of the copy, and after the end of the file it is
/// bytes like any others. A copy that only names another version is believed last, the one at
/// the start before the one at the end.
fn believed(lead: &Found, last: &Found) -> Result<Header, FileError> {
    match (lead, last) {
        (Found::Sound(first), Found::Sound(second)) if first != second => Err(FileError::Conflict),
        (Found::Unsupported(_), Found::Sound(_)) => Err(FileError::Conflict),
        (Found::Sound(header), _) | (_, Found::Sound(header)) => Ok(*header),
        (Found::Unsupported(_), _) | (_, Found::Unsupported(_)) => Err(FileError::Unsupported),
        (Found::Version(version), _) | (_, Found::Version(version)) => {
            Err(FileError::Version(*version))
        }
        (Found::Damaged, _) | (_, Found::Damaged) => Err(FileError::Damaged),
        (Found::Absent, Found::Absent) => Err(FileError::Unrecognized),
    }
}

/// What surrounds the blocks of the file `bytes`, read by `header`. The copy at the end and the
/// trailer are looked for where the header's sizes put them, also when bytes follow them.
fn frame(header: &Header, bytes: &[u8], lead: &Found) -> Result<Frame, FileError> {
    let expected = header.set().file_len();
    let given = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let length = match given.checked_sub(expected) {
        Some(0) => Length::Exact,
        Some(extra) => Length::Long(extra),
        None => Length::Short(expected - given),
    };
    let own = header.as_bytes();
    let trailer_at = expected.saturating_sub(wide(TRAILER));
    let tail_at = trailer_at.saturating_sub(wide(own.len()));
    // What stands in the place of the copy is read by its own length, not by this header's:
    // another header there holds by its own checksum, be it longer or shorter than this one.
    let there = from(bytes, tail_at);
    let tail = match Found::at_start(there) {
        Found::Sound(copy) if copy == *header => Condition::Intact,
        // A second header whose checksum holds, in the place of the copy: not this file's.
        Found::Sound(_) | Found::Unsupported(_) => return Err(FileError::Conflict),
        Found::Version(_) | Found::Damaged | Found::Absent if there.len() < own.len() => {
            Condition::Missing
        }
        Found::Version(_) | Found::Damaged | Found::Absent => Condition::Damaged,
    };
    let trailer = match piece(bytes, trailer_at, TRAILER) {
        None => Condition::Missing,
        Some(found) if found == header.trailer() => Condition::Intact,
        Some(_) => Condition::Damaged,
    };
    // A header that does not hold at the start is there all the same: the file was read by the
    // copy at the end, which counts only in a file of the full length.
    let lead = if matches!(lead, Found::Sound(_)) {
        Condition::Intact
    } else {
        Condition::Damaged
    };
    Ok(Frame {
        lead,
        tail,
        trailer,
        length,
    })
}

/// A count as a place in a file.
fn wide(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

/// The bytes of `bytes` from `start` on, as many as the longest header has: what could be a
/// header there.
fn from(bytes: &[u8], start: u64) -> &[u8] {
    let rest = usize::try_from(start)
        .ok()
        .and_then(|start| bytes.get(start..))
        .unwrap_or_default();
    rest.get(..MOST).unwrap_or(rest)
}

/// The `length` bytes of `bytes` from `start`, when they are all there.
fn piece(bytes: &[u8], start: u64, length: usize) -> Option<&[u8]> {
    let start = usize::try_from(start).ok()?;
    bytes.get(start..start.checked_add(length)?)
}
