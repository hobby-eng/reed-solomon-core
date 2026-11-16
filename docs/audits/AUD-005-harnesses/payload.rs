use reed_solomon_core::{strict, protect::Secret};
fn probe(view: strict::ShardFile<'_>, shard: &Vec<u8>, out: &mut Secret) { let _ = view.header().file_into(shard, out); }
fn main() {}
