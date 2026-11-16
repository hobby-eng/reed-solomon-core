use reed_solomon_core::{strict, Field};
use reed_solomon_core::protect::Secret;
fn main() {
    let facts = strict::SetFacts::new(Field::Gf256, b"demo", 1, 4, 4, None).unwrap();
    let mut shard = Secret::with_capacity(4);
    assert!(strict::Held::guarded(&shard));
    shard.push(b"demo").unwrap();
    let file = facts.header(0).unwrap().file(&shard).unwrap();
    let view = strict::read(&file).unwrap();
    let header: strict::Header = view.header();
    let metadata: strict::SetFacts = header.set();
    let _: strict::Code = metadata.code();
    let mut guarded = Secret::zeroed(file.len());
    assert!(strict::Held::guarded(&guarded));
    header.file_into(&shard, &mut guarded).unwrap();
    let mut portable = vec![0_u8; file.len()];
    header.as_portable().file_into(shard.as_bytes(), &mut portable).unwrap();
    assert_eq!(&portable[..], guarded.as_bytes());
    assert_eq!(file.as_slice(), guarded.as_bytes());
    println!("strict and opted-in portable writers returned identical public fixture bytes");
}
