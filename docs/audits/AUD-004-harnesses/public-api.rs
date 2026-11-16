use reed_solomon_core::{strict, Field};
use reed_solomon_core::protect::Secret;

fn main() {
    let facts = strict::SetFacts::new(Field::Gf256, b"demo", 1, 4, 4, None).unwrap();
    let mut public = Secret::with_capacity(4);
    assert!(strict::Held::guarded(&public));
    public.push(b"demo").unwrap();
    let file = facts.header(0).unwrap().file(&public).unwrap();
    let reader = strict::read(&file).unwrap();
    let mut destination = vec![0u8; file.len()];
    reader.header().file_into(b"demo", &mut destination).unwrap();
    assert_eq!(destination, file.as_slice());
    let from_facts = facts.facts().files(b"demo", &[0]).unwrap();
    assert_eq!(from_facts[0].as_slice(), destination);
    println!("Both public API chains accept ordinary slice storage; outputs match the strict writer.");
}
