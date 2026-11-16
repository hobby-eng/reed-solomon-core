use reed_solomon_core::strict;
fn probe(view: strict::ShardFile<'_>, bytes: &Vec<u8>) { let _ = view.header().set().files(bytes, &[0]); }
fn main() {}
