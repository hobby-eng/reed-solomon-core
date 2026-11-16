//! The tests of the public interface: known answers, an independent implementation, every choice
//! of shards, refusals, stripes, shard files written, read and damaged, and sets put together
//! from their files, with the files that differ from the rest named and the digest of the data
//! compared.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index fails the test, as it should"
)]

mod correction;
mod cyclic;
mod cyclic_words;
mod damage;
mod digests;
mod examples;
mod fields;
mod files;
mod in_place;
mod oracle;
mod plans;
mod refusals;
mod repairs;
mod secrets;
mod self_test;
mod sets;
mod stripes;
mod subsets;
mod support;
mod vectors;
mod votes;
