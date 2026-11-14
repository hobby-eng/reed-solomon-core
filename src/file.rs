//! The shard file: one shard of a set together with what a program needs to find its set again
//! and to tell which of its bytes survived.
//!
//! A file is the header, the shard in blocks with a checksum each, the header a second time and
//! a trailer that names the header's length:
//!
//! ```text
//! header | block 0, checksum | block 1, checksum | … | header | trailer
//! ```
//!
//! The header ([`Header`]) holds the facts of the set ([`SetFacts`]) and the index of the shard,
//! under a checksum of its own. Each block's checksum also covers the header's checksum and the
//! block's number, so a block is tied to its file and its place. [`ShardFile::read`] takes a file
//! that may be damaged, cut short or followed by other bytes and says what is left of it.
//! `docs/TECHNICAL.md` has every byte and every rule.
//!
//! The checksums are CRC-32C: they find accidental damage. They are no signature, and someone
//! who changes a file on purpose can make every one of them fit.

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
    Stretches, Vote,
};

/// Why a shard file was not described, written or read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileError {
    /// The field's symbols are not the bytes of a file: [`crate::Field::Gf2048`].
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
    /// A file would be longer than 2^64 − 1 bytes.
    Size,
    /// The bytes given for a shard are not as long as the set's shards are.
    Shard,
    /// The data given is not as long as the facts of the set say.
    Data,
    /// The data given does not have the SHA-256 that the facts of the set state.
    Digest,
    /// This machine has no memory for the file: it is longer than the machine addresses, or the
    /// memory was not given. The format allows the file; nothing was written.
    Memory,
    /// The bytes have no mark of a shard file, at the start or at the end.
    Unrecognized,
    /// The marks of a shard file are there, and neither copy of the header passes its checks.
    Damaged,
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
            Self::Field => "a shard file holds bytes, and the symbols of this field are not bytes",
            Self::Id => "the set identifier is empty or longer than 32 bytes",
            Self::Threshold => "the threshold is zero or above the field's number of shards",
            Self::Index => "the field has no shard with this index",
            Self::Block => "the block length is zero or not a whole number of symbols",
            Self::Blocks => "a shard would have more than 2^32 blocks",
            Self::Size => "a shard file would be longer than 2^64 - 1 bytes",
            Self::Shard => "the bytes are not as long as a shard of this set",
            Self::Data => "the data is not as long as the facts of the set say",
            Self::Digest => "the data does not have the SHA-256 the facts of the set state",
            Self::Memory => "this machine has no memory for the shard file",
            Self::Unrecognized => "the bytes are not a shard file",
            Self::Damaged => "the header of the shard file is damaged in both copies",
            Self::Version(version) => {
                return write!(
                    f,
                    "the file names version {version} of the shard file format"
                );
            }
            Self::Unsupported => "the shard file uses a field or an option this version lacks",
            Self::Conflict => "the two headers of the shard file both hold and differ",
        };
        f.write_str(text)
    }
}

impl core::error::Error for FileError {}

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
    let written = SetFacts::new(crate::Field::Gf256, &[1], 3, 10, 4, None)
        .and_then(|set| Header::new(set, 4))
        .is_ok_and(|header| {
            header
                .as_bytes()
                .iter()
                .chain(&shard)
                .chain(&header.block_checksum(0, &shard).to_be_bytes())
                .chain(header.as_bytes())
                .chain(&header.trailer())
                .eq(&FILE)
        });
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
    use alloc::vec::Vec;
    let field = crate::Field::Gf256;
    let rebuilt = |facts: SetFacts| -> Option<Result<Rebuilt, SetError>> {
        let files: Vec<crate::Zeroizing<Vec<u8>>> = (0..3_u8)
            .map(|index| {
                let shard = [b"abc".get(usize::from(index)).copied()?];
                Header::new(facts, index.into()).ok()?.file(&shard).ok()
            })
            .collect::<Option<_>>()?;
        let given: Vec<&[u8]> = files.iter().map(|file| file.as_slice()).collect();
        let pile = Pile::new(&given).ok()?;
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
                *done.data == b"abc" && done.digest == DigestCheck::Matched(expected)
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
    use alloc::vec::Vec;
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
    let given: Vec<&[u8]> = files.iter().map(|file| file.as_slice()).collect();
    let vote = Pile::new(&given)
        .ok()
        .and_then(|pile| pile.sets().first()?.vote(None).ok());
    vote.is_some_and(|vote| {
        vote.outvoted() == [named] && vote.undecided() == 0 && vote.steps() == 945
    })
}
