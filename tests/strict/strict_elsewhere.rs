//! The strict mode where the system gives no protected stack, as on Windows: a code is made and
//! its public facts read, but every call on the bytes of shards refuses and writes nothing,
//! whether or not the system protected its inputs.

use reed_solomon_core::protect::Secret;
use reed_solomon_core::strict::{self, Held};
use reed_solomon_core::{CodeError, Field};

#[test]
fn strict_calls_on_shard_bytes_refuse_without_a_protected_stack() {
    let code = strict::Code::new(Field::Gf256, 2).unwrap();
    assert_eq!((code.field(), code.threshold()), (Field::Gf256, 2));
    let (mut first, mut second) = (Secret::with_capacity(2), Secret::with_capacity(2));
    first.push(b"ab").unwrap();
    second.push(b"cd").unwrap();
    let data: [&dyn Held; 2] = [&first, &second];
    assert_eq!(code.encode(&data, &[2]), Err(CodeError::Unprotected));
    let mut weights = [0; 2];
    let sources = code.sources(&[0, 1], &mut weights).unwrap();
    let mut out = Secret::zeroed(2);
    let answer = sources.build(2, &data, &mut out);
    assert_eq!(answer, Err(CodeError::Unprotected));
    assert_eq!(out.as_bytes(), [0, 0]);
}
