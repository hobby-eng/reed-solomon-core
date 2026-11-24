//! Checks that products and inverses take the same time whatever their elements, in each kind of
//! field: the named GF(2^8), GF(2^11) and GF(2^16), and a field of `Field::binary`. The source has
//! no branch on an element, but only the compiled code shows whether the optimiser kept it so.
//!
//! Two measurements, each on inputs that differ only in their values, timed in turns:
//! - words: `CyclicWords::correct_word` on clean words of zeros against clean random words, so that
//!   every symbol goes through the products of the syndromes (the fields of the cyclic code);
//! - corrections: `Code::correct` on sets with one wrong symbol in every column, on the same
//!   shard, whose errors are 1 against random ones, so that the search and the values of the
//!   errors take products and inverses of values that follow from them (every field).
//!
//! The median of the ratios of the two times must lie between 0.8 and 1.25; otherwise the program
//! ends with exit code 1. Run it with the buffers in the allocator's memory, so that no system call
//! of protected pages hides the arithmetic:
//! `cargo run --release --no-default-features --features alloc --example timing`.
#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a measuring program: it prints its table and stops at a fault"
)]

use reed_solomon_core::{Code, Correction, CyclicCode, CyclicWords, Field};
use std::hint::black_box;
use std::time::Instant;

/// Rounds, each timing both kinds of input once.
const ROUNDS: usize = 31;
/// Cyclic words: data and parity symbols, and words per pass.
const DATA: usize = 12;
const PARITY: usize = 4;
const WORDS: usize = 2048;
/// Corrections: data shards, all shards, symbols per shard, the wrong shard, sets per pass.
const THRESHOLD: usize = 8;
const SHARDS: u16 = 12;
const COLUMNS: usize = 64;
const WRONG: usize = 9;
const SETS: usize = 16;

/// A small generator for public test values.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// An element of `field` from 1 up.
    fn element(&mut self, field: Field) -> u16 {
        let top = u64::try_from(field.order() - 1).unwrap();
        u16::try_from(1 + self.next() % top).unwrap()
    }
}

/// The bytes of one symbol of `field`: one or two, the high byte first.
fn bytes(field: Field, value: u16) -> Vec<u8> {
    value.to_be_bytes()[2 - field.symbol_bytes()..].to_vec()
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// The median ratio of `others`' times to `fixed`'s, timed in turns after one pass of each.
fn ratio(mut fixed: impl FnMut() -> f64, mut others: impl FnMut() -> f64) -> f64 {
    let _ = (fixed(), others());
    let ratios = (0..ROUNDS)
        .map(|round| {
            if round % 2 == 0 {
                let base = fixed();
                others() / base
            } else {
                let other = others();
                other / fixed()
            }
        })
        .collect();
    median(ratios)
}

/// A clean word of the cyclic code with `data` as its data symbols.
fn word(code: &CyclicCode, field: Field, data: &[u16]) -> Vec<u16> {
    let shards: Vec<Vec<u8>> = data.iter().map(|&d| bytes(field, d)).collect();
    let given: Vec<&[u8]> = shards.iter().map(Vec::as_slice).collect();
    let parity = code.encode(&given).unwrap();
    let width = field.symbol_bytes();
    let mut word = data.to_vec();
    word.extend(parity.iter().map(|p| {
        let mut value = [0_u8; 2];
        value[2 - width..].copy_from_slice(p.as_slice());
        u16::from_be_bytes(value)
    }));
    word
}

/// The seconds `correct_word` takes over `set`, every word clean.
fn check_words(code: &CyclicWords<'_>, set: &[Vec<u16>], room: &mut [u16]) -> f64 {
    let start = Instant::now();
    for word in set {
        let mut copy = black_box(word.clone());
        let answer = code.correct_word(&mut copy, &[], black_box(&mut *room));
        assert_eq!(
            black_box(answer),
            Ok(Some(0)),
            "a clean word needs no change"
        );
    }
    start.elapsed().as_secs_f64()
}

/// The ratio of the time to check random clean words to that of words of zeros.
fn words(field: Field, rng: &mut Rng) -> f64 {
    let owned = CyclicCode::new(field, DATA, PARITY, 1, 1).unwrap();
    let zeros = vec![word(&owned, field, &[0; DATA]); WORDS];
    let randoms: Vec<Vec<u16>> = (0..WORDS)
        .map(|_| {
            let data: Vec<u16> = (0..DATA).map(|_| rng.element(field)).collect();
            word(&owned, field, &data)
        })
        .collect();
    let mut generator = [0_u16; PARITY + 1];
    let code = CyclicWords::new(field, DATA, PARITY, 1, 1, &mut generator).unwrap();
    let (mut room, mut other_room) = (
        vec![0_u16; CyclicWords::room(PARITY)],
        vec![0_u16; CyclicWords::room(PARITY)],
    );
    ratio(
        || check_words(&code, &zeros, &mut room),
        || check_words(&code, &randoms, &mut other_room),
    )
}

/// The seconds `correct` takes over copies of `sets`, one wrong symbol in every column.
fn correct_sets(code: &Code, sets: &[Vec<Vec<u8>>], work: &mut [Vec<u8>]) -> f64 {
    let mut fixed = vec![0; usize::from(SHARDS)];
    fixed[WRONG] = COLUMNS;
    let start = Instant::now();
    for set in sets {
        for (copy, shard) in work.iter_mut().zip(set) {
            copy.copy_from_slice(shard);
        }
        let mut shards: Vec<(u16, &mut [u8])> =
            (0..).zip(work.iter_mut().map(Vec::as_mut_slice)).collect();
        let answer = code.correct(black_box(&mut shards)).unwrap();
        assert_eq!(
            answer,
            Correction::Corrected(fixed.clone()),
            "every column is put right"
        );
    }
    start.elapsed().as_secs_f64()
}

/// The ratio of the time to correct random errors to that of errors of 1, one in every column of
/// a set of zeros, on the same shard.
fn corrections(field: Field, rng: &mut Rng) -> f64 {
    let code = Code::new(field, THRESHOLD).unwrap();
    let width = field.symbol_bytes();
    let length = COLUMNS * width;
    let zeros = vec![vec![0_u8; length]; usize::from(SHARDS)];
    let damaged = |error: &mut dyn FnMut() -> u16| {
        let mut set = zeros.clone();
        for column in 0..COLUMNS {
            set[WRONG][column * width..(column + 1) * width]
                .copy_from_slice(&bytes(field, error()));
        }
        set
    };
    let ones: Vec<Vec<Vec<u8>>> = (0..SETS).map(|_| damaged(&mut || 1)).collect();
    let randoms: Vec<Vec<Vec<u8>>> = (0..SETS)
        .map(|_| damaged(&mut || rng.element(field)))
        .collect();
    let (mut work, mut other_work) = (zeros.clone(), zeros.clone());
    ratio(
        || correct_sets(&code, &ones, &mut work),
        || correct_sets(&code, &randoms, &mut other_work),
    )
}

fn main() {
    let mut rng = Rng(0x5EED_1234_ABCD_0001);
    let binary = Field::binary(10, None).unwrap();
    let mut passed = true;
    let mut report = |name: &str, what: &str, value: f64| {
        let fine = (0.8..=1.25).contains(&value);
        passed &= fine;
        let verdict = if fine { "ok" } else { "DEPENDS ON THE VALUES" };
        println!("{name}, {what}: median ratio random/fixed {value:.2} {verdict}");
    };
    for (name, field) in [
        ("GF(2^8)", Field::Gf256),
        ("GF(2^11)", Field::Gf2048),
        ("GF(2^10) of Field::binary", binary),
    ] {
        report(name, "words", words(field, &mut rng));
    }
    for (name, field) in [
        ("GF(2^8)", Field::Gf256),
        ("GF(2^11)", Field::Gf2048),
        ("GF(2^16)", Field::Gf65536),
        ("GF(2^10) of Field::binary", binary),
    ] {
        report(name, "corrections", corrections(field, &mut rng));
    }
    if !passed {
        std::process::exit(1);
    }
}
