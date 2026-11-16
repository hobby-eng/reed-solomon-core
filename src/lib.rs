//! Reed–Solomon coding for sets of shards: data cut into `k` shards becomes up to 2^m shards in a
//! field GF(2^m) ([`Field`]), and any `k` of them rebuild it. It can also find and fix wrong
//! symbols, keep shards in files that survive damage, and check data against its SHA-256. The
//! crate holds no `unsafe` code; examples are in the README, tests/suite/examples.rs and
//! tests/strict/example.rs.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

#[cfg(feature = "alloc")]
extern crate alloc;
#[cfg(feature = "protect")]
extern crate std;

#[cfg(feature = "alloc")]
mod buffer;
mod code;
#[cfg(feature = "alloc")]
mod correct;
mod cyclic;
mod field;
mod file;
mod list;
mod recurrence;
mod self_test;
#[cfg(feature = "protect")]
pub mod strict;

pub use code::{COMPARED, Code, CodeError, Sources};
pub use field::{Binary, Field, FieldError};
pub use file::{
    Block, Condition, DataDigest, FileError, Frame, Header, Length, SetFacts, ShardFile, Survey,
};
pub use self_test::{Check, SelfTest, self_test};
pub use zeroize::Zeroize;

#[cfg(feature = "alloc")]
pub use buffer::Buffer;
#[cfg(feature = "alloc")]
pub use code::Redundancy;
#[cfg(feature = "alloc")]
pub use correct::{Change, ChangeKind, Changes, ChangesIter, Correction, Mark, Plan};
#[cfg(feature = "alloc")]
pub use cyclic::CyclicCode;
pub use cyclic::CyclicWords;
#[cfg(feature = "alloc")]
pub use file::{
    Coverage, DigestCheck, Dispute, FileSet, Member, Outvoted, Pile, Rebuilt, SetError, Stretch,
    Stretches, Vote, Written,
};
/// The `secret-protection-core` crate, whose pages hold this crate's buffers: for a program that
/// also isolates its process (`protect::Isolation`) or keeps buffers of its own in such pages.
#[cfg(feature = "protect")]
pub use secret_protection_core as protect;
