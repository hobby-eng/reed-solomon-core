use reed_solomon_core::strict;
fn probe(view: &strict::FileSet<'_>, bytes: &Vec<u8>) { let _ = view.facts().files(bytes, &[0]); }
fn main() {}
