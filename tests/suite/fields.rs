//! Fields of a degree the program chooses, GF(2^m) for m from 2 to 16: the standard polynomial of
//! each degree and two others, refusals, and a set written, rebuilt, corrected and planned in
//! every degree. The shards expected were computed apart from this crate, in Python, with tables
//! of logarithms and Lagrange interpolation.

use crate::support::{bytes, plain};
use reed_solomon_core::{
    Code, CodeError, Correction, Field, FieldError, FileError, Mark, Plan, SetFacts,
};

/// Degree, polynomial, and the shards 2 and 3 of the set of two whose shards 0 and 1 are 1 and
/// the largest element, 2^m − 1.
const SHARDS: [(u8, u32, u16, u16); 17] = [
    (2, 0x7, 0x2, 0x0),
    (3, 0xB, 0x6, 0x0),
    (4, 0x13, 0xE, 0x0),
    (5, 0x25, 0x18, 0x6),
    (6, 0x43, 0x3E, 0x0),
    (7, 0x89, 0x74, 0xA),
    (8, 0x11D, 0xE0, 0x1E),
    (9, 0x211, 0x1EC, 0x12),
    (10, 0x409, 0x3F4, 0xA),
    (11, 0x805, 0x7F8, 0x6),
    (12, 0x1053, 0xFAE, 0x50),
    (13, 0x201B, 0x1FE6, 0x18),
    (14, 0x4443, 0x3BBE, 0x440),
    (15, 0x8003, 0x7FFE, 0x0),
    (16, 0x1100B, 0xEFF6, 0x1008),
    (8, 0x187, 0x7A, 0x84),
    (16, 0x1002D, 0xFFD0, 0x2E),
];

/// The standard polynomials of the degrees 2 to 16.
const STANDARD: [u32; 15] = [
    0x7, 0xB, 0x13, 0x25, 0x43, 0x89, 0x11D, 0x211, 0x409, 0x805, 0x1053, 0x201B, 0x4443, 0x8003,
    0x1100B,
];

fn symbol(field: Field, value: u16) -> Vec<u8> {
    match field.symbol_bytes() {
        1 => vec![u8::try_from(value).unwrap()],
        _ => value.to_be_bytes().to_vec(),
    }
}

/// `count` symbols of the field from a fixed generator, each below 2^m.
fn symbols(field: Field, count: usize, seed: u64) -> Vec<u8> {
    let width = field.symbol_bytes();
    let raw = bytes(seed, count * width);
    raw.chunks(width)
        .flat_map(|chunk| {
            let value = chunk.iter().fold(0_u32, |v, &b| v << 8 | u32::from(b));
            let value = u16::try_from(value % u32::try_from(field.order()).unwrap()).unwrap();
            symbol(field, value)
        })
        .collect()
}

#[test]
fn every_degree_writes_the_shards_computed_apart() {
    for (degree, polynomial, two, three) in SHARDS {
        let field = Field::binary(degree, Some(polynomial)).unwrap();
        assert_eq!(field.order(), 1 << degree);
        let top = u16::try_from((1_u32 << degree) - 1).unwrap();
        let code = Code::new(field, 2).unwrap();
        let (one, largest) = (symbol(field, 1), symbol(field, top));
        let shards = plain(code.encode(&[&one, &largest], &[2, 3]).unwrap());
        assert_eq!(
            shards,
            [symbol(field, two), symbol(field, three)],
            "{degree} {polynomial:#x}"
        );
    }
}

#[test]
fn a_degree_alone_takes_its_standard_polynomial_and_named_fields_keep_their_names() {
    for (degree, polynomial) in (2..=16).zip(STANDARD) {
        assert_eq!(
            Field::binary(degree, None),
            Field::binary(degree, Some(polynomial))
        );
    }
    assert_eq!(Field::binary(8, None), Ok(Field::Gf256));
    assert_eq!(Field::binary(11, Some(0x805)), Ok(Field::Gf2048));
    // GF(2^16) of polynomials is another field than the extension of GF(2^8).
    let sixteen = Field::binary(16, None).unwrap();
    assert_ne!(sixteen, Field::Gf65536);
    assert!(matches!(sixteen, Field::Binary(field) if field.polynomial() == 0x1100B));
    assert_eq!(Field::binary(5, None).unwrap().symbol_bytes(), 1);
    assert_eq!(Field::binary(9, None).unwrap().symbol_bytes(), 2);
}

#[test]
fn a_degree_or_a_polynomial_that_makes_no_field_is_refused() {
    assert_eq!(Field::binary(1, None), Err(FieldError::Degree));
    assert_eq!(Field::binary(17, None), Err(FieldError::Degree));
    assert_eq!(Field::binary(0, Some(0x3)), Err(FieldError::Degree));
    // Irreducible, but x has the order 5 of 15.
    assert_eq!(Field::binary(4, Some(0x1F)), Err(FieldError::Polynomial));
    // (x^2 + x + 1)^2.
    assert_eq!(Field::binary(4, Some(0x15)), Err(FieldError::Polynomial));
    // A polynomial of another degree, and one without its constant term.
    assert_eq!(Field::binary(4, Some(0x25)), Err(FieldError::Polynomial));
    assert_eq!(Field::binary(4, Some(0x12)), Err(FieldError::Polynomial));
}

#[test]
fn every_degree_rebuilds_corrects_and_plans_a_set() {
    for degree in 2..=16 {
        let field = Field::binary(degree, None).unwrap();
        let width = field.symbol_bytes();
        let n = field.order().min(6);
        let code = Code::new(field, 2).unwrap();
        let data = [
            symbols(field, 5, u64::from(degree)),
            symbols(field, 5, u64::from(degree) + 100),
        ];
        let targets: Vec<u16> = (0..u16::try_from(n).unwrap()).collect();
        let set = plain(code.encode(&[&data[0], &data[1]], &targets).unwrap());
        // Every two shards give the data back.
        for first in 0..n {
            for second in first + 1..n {
                let chosen = [
                    (targets[first], set[first].as_slice()),
                    (targets[second], set[second].as_slice()),
                ];
                let back = plain(code.rebuild(&chosen, &[0, 1]).unwrap());
                assert_eq!(back, data, "{degree} {first} {second}");
            }
        }
        // One wrong symbol is found and put right; with a mark, a second gap costs one more.
        let mut read = set.clone();
        read[n - 1][width] ^= 1;
        let mut given: Vec<(u16, &mut [u8])> = targets
            .iter()
            .copied()
            .zip(read.iter_mut().map(|shard| shard.as_mut_slice()))
            .collect();
        let found = code.correct(&mut given).unwrap();
        assert!(
            matches!(found, Correction::Corrected(_)),
            "{degree} {found:?}"
        );
        assert_eq!(read, set);
        let shards: Vec<(u16, &[u8])> = targets
            .iter()
            .copied()
            .zip(set.iter().map(|shard| shard.as_slice()))
            .collect();
        let plan = code
            .plan(
                &shards,
                &[Mark {
                    shard: 0,
                    symbol: 1,
                }],
            )
            .unwrap();
        assert!(matches!(plan, Plan::Changes(ref changes) if changes.len() == 1));
    }
}

#[test]
fn symbols_and_numbers_beyond_the_field_are_refused() {
    let five = Field::binary(5, None).unwrap();
    let code = Code::new(five, 2).unwrap();
    assert_eq!(
        code.encode(&[&[0x1F], &[0x20]], &[2]),
        Err(CodeError::Symbol)
    );
    assert_eq!(
        code.encode(&[&[0x1F], &[0x1E]], &[32]),
        Err(CodeError::Index)
    );
    assert!(code.encode(&[&[0x1F], &[0x1E]], &[31]).is_ok());
    assert_eq!(Code::new(five, 33), Err(CodeError::Threshold));
    let twelve = Field::binary(12, None).unwrap();
    let code = Code::new(twelve, 2).unwrap();
    assert_eq!(
        code.encode(&[&[0x0F, 0xFF], &[0x10, 0x00]], &[2]),
        Err(CodeError::Symbol)
    );
    // A field whose symbols fill their bytes takes every byte.
    let other = Field::binary(8, Some(0x187)).unwrap();
    assert!(
        Code::new(other, 2)
            .unwrap()
            .encode(&[&[0xFF], &[0x80]], &[2])
            .is_ok()
    );
    // These fields have no shard file.
    assert_eq!(
        SetFacts::new(five, b"x", 2, 10, 4, None),
        Err(FileError::Field)
    );
}
