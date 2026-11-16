use reed_solomon_core::{strict, protect::Secret};
fn probe(view: strict::ShardFile<'_>, shard: &Secret, out: &mut Vec<u8>) { let _ = view.header().file_into(shard, out); }
fn main() {}
