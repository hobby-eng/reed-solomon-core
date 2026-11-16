//! The shard file: one shard plus what a program needs to find its set and tell which bytes
//! survived. It is the header, the blocks with a checksum each, the header again and a trailer;
//! docs/TECHNICAL.md has every byte. The checksums (CRC-32C) catch accidents, not tampering.

mod crc32c;
mod facts;
mod header;
#[cfg(feature = "alloc")]
mod pile;
mod reading;
#[cfg(feature = "alloc")]
mod set;
#[cfg(feature = "alloc")]
mod writing;

use core::fmt;

pub use facts::{DataDigest, SetFacts};
pub use header::Header;
pub use reading::{Block, Condition, Frame, Length, ShardFile, Survey};

#[cfg(feature = "alloc")]
pub use pile::Pile;
#[cfg(feature = "alloc")]
pub use set::{
    Coverage, DigestCheck, Dispute, FileSet, Member, Outvoted, Rebuilt, SetError, Stretch,
    Stretches, Vote, Written,
};

/// Why a shard file was not described, written or read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileError {
    /// The field has no shard file: version 1 names only [`crate::Field::Gf256`] and
    /// [`crate::Field::Gf65536`], so a field of [`crate::Field::binary`] has none, even one whose
    /// symbols are single bytes.
    Field,
    /// A set identifier that is empty or longer than 32 bytes.
    Id,
    /// A threshold that is zero or above the number of shards the field allows.
    Threshold,
    /// A shard index the field does not have.
    Index,
    /// A block length of zero, or one that is not a whole number of symbols.
    Block,
    /// A shard would have more than 2^32 blocks: the set needs longer blocks.
    Blocks,
    /// A file would be longer than 2^64 − 1 bytes, or data of 2^61 bytes or more would state a
    /// SHA-256, which takes fewer than 2^64 bits.
    Size,
    /// The bytes given for a shard are not as long as the set's shards are.
    Shard,
    /// The data given is not as long as the facts of the set say.
    Data,
    /// The data given does not have the SHA-256 that the facts of the set state.
    Digest,
    /// This machine has no memory for the file: it is longer than the machine addresses, or the
    /// memory was not given. The format allows the file. A call that returns a buffer wrote
    /// nothing; one that writes in place says what its outputs hold after a refusal.
    Memory,
    /// In the strict mode, memory the file or its data would pass through is not protected as
    /// that mode requires. A call refused before its work wrote nothing.
    Unprotected,
    /// The bytes have no mark of a shard file, at the start or at the end.
    Unrecognized,
    /// The marks of a shard file are there, and neither copy of the header passes its checks.
    Damaged,
    /// The outputs given to be written in place are not one for each file, each as long as a
    /// file of the set.
    Output {
        /// The bytes each output must have.
        needed: u64,
    },
    /// No header's checksum holds, and one names this other version of the format: the mark and
    /// the byte after it. Nothing else of such a file is checked: a later version may lay its
    /// header out differently, and a file of this version whose version byte went wrong in the
    /// one copy left reads the same way.
    Version(u8),
    /// No header holds, and one whose checksum holds names a field or a flag this version does
    /// not read.
    Unsupported,
    /// Two headers hold, each in a place that is certain, and differ; or a second header whose
    /// checksum holds sits where the copy of the first belongs. Neither is chosen.
    Conflict,
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Field => "the shard file names only GF(2^8) by 0x11D and GF(2^16) built over it",
            Self::Id => "the set identifier is empty or longer than 32 bytes",
            Self::Threshold => "the threshold is zero or above the field's number of shards",
            Self::Index => "the field has no shard with this index",
            Self::Block => "the block length is zero or not a whole number of symbols",
            Self::Blocks => "a shard would have more than 2^32 blocks",
            Self::Size => "a shard file or the data with its SHA-256 would be too long",
            Self::Shard => "the bytes are not as long as a shard of this set",
            Self::Data => "the data is not as long as the facts of the set say",
            Self::Digest => "the data does not have the SHA-256 the facts of the set state",
            Self::Memory => "this machine has no memory for the shard file",
            Self::Unprotected => "the strict mode found memory the system does not protect",
            Self::Unrecognized => "the bytes are not a shard file",
            Self::Damaged => "the header of the shard file is damaged in both copies",
            Self::Version(version) => {
                return write!(
                    f,
                    "the file names version {version} of the shard file format"
                );
            }
            Self::Output { needed } => {
                return write!(f, "the outputs are not one of {needed} bytes for each file");
            }
            Self::Unsupported => "the shard file uses a field or an option this version lacks",
            Self::Conflict => "the two headers of the shard file both hold and differ",
        };
        f.write_str(text)
    }
}

impl core::error::Error for FileError {}

#[cfg(feature = "alloc")]
impl From<crate::buffer::Shortfall> for FileError {
    fn from(shortfall: crate::buffer::Shortfall) -> Self {
        match shortfall {
            crate::buffer::Shortfall::Memory => Self::Memory,
            crate::buffer::Shortfall::Unprotected => Self::Unprotected,
        }
    }
}

/// The published check value of CRC-32C, for the self-test.
pub(crate) fn checksum_known_answer() -> bool {
    crc32c::checksum(&[b"123456789"]) == 0xE306_9283
}

/// A shard file of 86 bytes whose three checksums were computed apart from this crate, by a
/// bit-by-bit CRC and by a long division, for the self-test. The file must be written to these
/// bytes and read back whole, and with one bit of its block changed the block must be refused.
pub(crate) fn file_known_answer() -> bool {
    const FILE: [u8; 86] = [
        0x53, 0x48, 0x52, 0x44, 0x01, 0x01, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00,
        0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x04, 0xCD,
        0x62, 0xC8, 0x65, 0xF4, 0x00, 0x00, 0x00, 0xB3, 0x56, 0x1B, 0x91, 0x53, 0x48, 0x52, 0x44,
        0x01, 0x01, 0x00, 0x01, 0x01, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x0A, 0x00, 0x00, 0x00, 0x04, 0xCD, 0x62, 0xC8, 0x65, 0x53,
        0x48, 0x52, 0x54, 0x00, 0x00, 0x00, 0x21, 0x62, 0xE4, 0x7E, 0x4A,
    ];
    /// The place of the block's first byte.
    const BLOCK_AT: usize = 33;
    let shard = [0xF4, 0x00, 0x00, 0x00];
    // The writer itself writes the file, into an array: every byte is compared.
    let mut file = [0; 86];
    let written = SetFacts::new(crate::Field::Gf256, &[1], 3, 10, 4, None)
        .and_then(|set| Header::new(set, 4))
        .and_then(|header| header.file_into(&shard, &mut file))
        .is_ok_and(|()| file == FILE);
    let read = ShardFile::read(&FILE)
        .is_ok_and(|file| file.survey().whole() && file.block(0) == Some(Block::Intact(&shard)));
    let mut changed = FILE;
    if let Some(byte) = changed.get_mut(BLOCK_AT) {
        *byte ^= 0x01;
    }
    let refused = ShardFile::read(&changed)
        .is_ok_and(|file| file.block(0) == Some(Block::Damaged) && !file.survey().whole());
    written && read && refused
}

/// The SHA-256 of "abc", the example of FIPS 180-4, for the self-test.
pub(crate) fn digest_known_answer() -> bool {
    const ABC: [u8; 32] = [
        0xBA, 0x78, 0x16, 0xBF, 0x8F, 0x01, 0xCF, 0xEA, 0x41, 0x41, 0x40, 0xDE, 0x5D, 0xAE, 0x22,
        0x23, 0xB0, 0x03, 0x61, 0xA3, 0x96, 0x17, 0x7A, 0x9C, 0xB4, 0x10, 0xFF, 0x61, 0xF2, 0x00,
        0x15, 0xAD,
    ];
    facts::sha256(b"abc") == ABC
}

/// Three files of a set of three whose data is "abc" and which state the digest of "abd", for
/// the self-test: they read and agree, and the rebuild refuses their data. The same set stating
/// its own digest rebuilds and says the digest matched.
#[cfg(feature = "alloc")]
pub(crate) fn digest_refusal_known_answer() -> bool {
    let field = crate::Field::Gf256;
    let rebuilt = |facts: SetFacts| -> Option<Result<Rebuilt, SetError>> {
        let file =
            |index: u8, symbol: u8| Header::new(facts, index.into()).ok()?.file(&[symbol]).ok();
        let [a, b, c] = [file(0, b'a')?, file(1, b'b')?, file(2, b'c')?];
        let pile = Pile::new(&[a.as_slice(), b.as_slice(), c.as_slice()]).ok()?;
        Some(pile.sets().first()?.rebuild(None))
    };
    let other = SetFacts::for_data(field, &[1], 3, b"abd", 1, DataDigest::Sha256);
    let own = SetFacts::for_data(field, &[1], 3, b"abc", 1, DataDigest::Sha256);
    let (Ok(other), Ok(own)) = (other, own) else {
        return false;
    };
    let refused = matches!(rebuilt(other), Some(Err(SetError::Digest { .. })));
    let matched = own.digest().is_some_and(|&expected| {
        rebuilt(own).is_some_and(|result| {
            result.is_ok_and(|done| {
                *done.data == *b"abc" && done.digest == DigestCheck::Matched(expected)
            })
        })
    });
    refused && matched
}

/// Five files of a set of three, the first of which holds other bytes under checksums that
/// hold, for the self-test: the vote worked out apart from this crate names that file in its
/// one block and takes 945 steps.
#[cfg(feature = "alloc")]
pub(crate) fn vote_known_answer() -> bool {
    let named = Outvoted {
        position: 0,
        index: 0,
        blocks: 1,
        first: 0,
    };
    let data = [0x01, 0x04, 0x00, 0x07, 0x09, 0x16];
    let Ok(facts) = SetFacts::new(crate::Field::Gf256, &[1], 3, 6, 2, None) else {
        return false;
    };
    let other = Header::new(facts, 0).and_then(|header| header.file(&[0x00, 0x06]));
    let (Ok(mut files), Ok(other)) = (facts.files(&data, &[0, 1, 2, 3, 4]), other) else {
        return false;
    };
    if let Some(first) = files.first_mut() {
        *first = other;
    }
    let [a, b, c, d, e] = files.as_slice() else {
        return false;
    };
    let given = [a, b, c, d, e].map(|file| file.as_slice());
    let vote = Pile::new(&given)
        .ok()
        .and_then(|pile| pile.sets().first()?.vote(None).ok());
    vote.is_some_and(|vote| {
        vote.outvoted() == [named] && vote.undecided() == 0 && vote.steps() == 945
    })
}
