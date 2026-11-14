//! Reed–Solomon coding for sets of shards: data cut into `k` shards becomes a set of up to 256,
//! 2,048 or 65,536 shards, depending on the field, any `k` of which rebuild it.
//!
//! Shard `i` is the value at point `i` of the one polynomial of degree below `k` whose values at
//! the points 0 to `k - 1` are the data shards, symbol by symbol. So the first `k` shards are the
//! data itself, how a shard follows from the data depends only on its index and `k`, and a set
//! can be given more shards later without touching the ones already issued. With the field
//! [`Field::Gf256`] the bytes are those of the Backblaze, klauspost and `reed-solomon-erasure`
//! codes.
//!
//! [`Code::sources`] takes any `k` shards of a set; [`Sources::build`] makes any shard from them,
//! which is how a set is written, how one lost shard is made again and how a set grows, and
//! [`Sources::agrees`] checks a further shard against them. Both work stripe by stripe on the
//! caller's buffers, so a large file never has to be in memory at once.
//!
//! [`Code::correct`] finds and puts right wrong symbols at unknown places. [`Code::plan`] does
//! the same with symbols a reader marks as unreadable, and lists every change before anything
//! changes; [`Code::apply`] makes them.
//!
//! A shard can be kept in a shard file: [`Header::file`] writes one, with the facts of its set
//! ([`SetFacts`]) in a header at both ends and a checksum after every block, and
//! [`ShardFile::read`] reads one that is damaged, cut short or followed by other bytes.
//! [`SetFacts::files`] writes the files of a whole set, [`Pile::new`] sorts any files into their
//! sets, and [`FileSet::rebuild`] puts the data together block by block from whichever files
//! still hold each block; [`FileSet::repair`] and [`FileSet::repairs`] make lost files again.
//! Where files read without a fault and are not one set,
//! [`FileSet::vote`] names the ones that differ from the rest, and [`FileSet::without`] is the
//! set without them.
//!
//! A set can state the SHA-256 of its data: [`SetFacts::for_data`] computes it, and
//! [`FileSet::rebuild`] gives the data back only when it has that digest.
//!
//! The crate holds no secret and no `unsafe` code, and allocates nothing without the `alloc`
//! feature; its one dependency, `sha2`, computes the digest. Its checks say whether shards belong
//! together and whether data has the digest its files state, never who made them: a program that
//! needs authenticity checks it itself.
//!
//! # Example
//!
//! ```
//! # fn main() -> Result<(), reed_solomon_core::CodeError> {
//! use reed_solomon_core::{Code, Correction, Field, Redundancy};
//!
//! // Three data shards become a set of five; any three rebuild the data.
//! let code = Code::new(Field::Gf256, 3)?;
//! let data: [&[u8]; 3] = [b"abcd", b"efgh", b"ijkl"];
//! let mut set = code.encode(&data, &[0, 1, 2, 3, 4])?;
//!
//! // Shards 0 and 2 are lost: three others give them back, and a sixth shard as well.
//! let left = [(4, &set[4][..]), (1, &set[1][..]), (3, &set[3][..])];
//! assert_eq!(code.rebuild(&left, &[0, 2])?, [b"abcd", b"ijkl"]);
//! let sixth = code.rebuild(&left, &[5])?;
//! assert_eq!(code.redundancy(&[left[0], left[1], left[2], (5, &sixth[0][..])])?, Redundancy::Agrees);
//!
//! // One byte goes wrong somewhere: the two spare shards find it and put it right.
//! set[2][1] ^= 0x40;
//! let mut given: Vec<(u16, &mut [u8])> = (0..).zip(set.iter_mut().map(Vec::as_mut_slice)).collect();
//! assert_eq!(code.correct(&mut given)?, Correction::Corrected(vec![0, 0, 1, 0, 0]));
//! assert_eq!(set[2], b"ijkl");
//! # Ok(())
//! # }
//! ```
//!
//! A shard file, written and read back after damage:
//!
//! ```
//! # fn main() -> Result<(), reed_solomon_core::FileError> {
//! use reed_solomon_core::{Block, Field, Header, SetFacts, ShardFile};
//!
//! // Shard 4 of a set of ten bytes with threshold 3, in blocks of four bytes.
//! let set = SetFacts::new(Field::Gf256, b"backup-1", 3, 10, 4, None)?;
//! let header = Header::new(set, 4)?;
//! let mut file = header.file(&[0xF4, 0x00, 0x00, 0x00])?;
//!
//! // A bit goes wrong in the first header: the copy at the end is believed.
//! file[9] ^= 0x01;
//! let read = ShardFile::read(&file)?;
//! assert_eq!(read.header(), &header);
//! assert_eq!(read.block(0), Some(Block::Intact(&[0xF4, 0x00, 0x00, 0x00])));
//! assert!(!read.survey().whole());
//! # Ok(())
//! # }
//! ```
//!
//! A set of files, rebuilt after a loss:
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use reed_solomon_core::{DataDigest, DigestCheck, Field, Pile, SetFacts};
//!
//! // Five files of a set of twenty-six bytes, stating its SHA-256; any three rebuild it.
//! let data = b"twenty-six bytes of a text";
//! let facts = SetFacts::for_data(Field::Gf256, b"backup-1", 3, data, 8, DataDigest::Sha256)?;
//! let mut files = facts.files(data, &[0, 1, 2, 3, 4])?;
//!
//! // One file is lost and another damaged; the rest is given in any order.
//! let lost = files.remove(1);
//! files[2][82] ^= 0x01;
//! let given: Vec<&[u8]> = files.iter().rev().map(Vec::as_slice).collect();
//! let pile = Pile::new(&given)?;
//! let set = &pile.sets()[0];
//! let rebuilt = set.rebuild(None)?;
//! assert_eq!(rebuilt.data, data);
//! assert!(matches!(rebuilt.digest, DigestCheck::Matched(_)));
//!
//! // The lost file is made again from the others, byte for byte.
//! assert_eq!(set.repair(1, None)?, lost);
//! # Ok(())
//! # }
//! ```
//!
//! A file that reads without a fault and holds other bytes, named by the others:
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use reed_solomon_core::{Field, Pile, SetError, SetFacts};
//!
//! // Five files of a set, and in the place of file 3 the file of another text.
//! let data = b"twenty-six bytes of a text";
//! let facts = SetFacts::new(Field::Gf256, b"backup-1", 3, 26, 8, None)?;
//! let mut files = facts.files(data, &[0, 1, 2, 3, 4])?;
//! files[3] = facts.files(b"another text, of 26 bytes!", &[3])?.remove(0);
//! let given: Vec<&[u8]> = files.iter().map(Vec::as_slice).collect();
//! let pile = Pile::new(&given)?;
//! let set = &pile.sets()[0];
//! assert!(matches!(set.rebuild(None), Err(SetError::Inconsistent { .. })));
//!
//! // Four files agree and one does not: the vote names it, and the set without it rebuilds.
//! let vote = set.vote(None)?;
//! assert_eq!(vote.outvoted().len(), 1);
//! assert_eq!(vote.outvoted()[0].position, 3);
//! assert_eq!(set.without(&[3])?.rebuild(None)?.data, data);
//! # Ok(())
//! # }
//! ```

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

#[cfg(feature = "alloc")]
extern crate alloc;

mod code;
#[cfg(feature = "alloc")]
mod correct;
mod field;
mod file;
mod self_test;

pub use code::{Code, CodeError, Sources};
pub use field::Field;
pub use file::{
    Block, Condition, DataDigest, FileError, Frame, Header, Length, SetFacts, ShardFile, Survey,
};
pub use self_test::{Check, SelfTest, self_test};

#[cfg(feature = "alloc")]
pub use code::Redundancy;
#[cfg(feature = "alloc")]
pub use correct::{Change, ChangeKind, Correction, Mark, Plan};
#[cfg(feature = "alloc")]
pub use file::{
    Coverage, DigestCheck, Dispute, FileSet, Member, Outvoted, Pile, Rebuilt, SetError, Stretch,
    Stretches, Vote,
};
