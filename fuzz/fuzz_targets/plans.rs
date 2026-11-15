//! Shards of a set with symbols changed and symbols marked unreadable, in every field. Within the
//! bound `2e + s ≤ m − k` a plan must put every column right, and whatever a plan or a correction
//! changes must leave shards that agree.
#![no_main]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a fuzz target answers by stopping the run: its unwraps and assertions are its checks"
)]

use libfuzzer_sys::fuzz_target;
use reed_solomon_core::{ChangeKind, Code, Correction, Field, Mark, Plan, Redundancy};
use reed_solomon_core_fuzz::{damage, symbol, within};

/// The shards as `(index, bytes)`, numbered from zero.
fn given(shards: &[Vec<u8>]) -> Vec<(u16, &[u8])> {
    (0..).zip(shards.iter().map(Vec::as_slice)).collect()
}

fuzz_target!(|bytes: &[u8]| {
    let mut input = bytes.iter().copied();
    let mut next = || usize::from(input.next().unwrap_or(0));
    let field = match next() % 5 {
        0 => Field::Gf256,
        1 => Field::Gf65536,
        2 => Field::Gf2048,
        _ => Field::binary(2 + (next() % 15) as u8, None).unwrap(),
    };
    let (order, width) = (field.order(), field.symbol_bytes());
    let shards = (1 + next() % 4 + next() % 5).min(order);
    let threshold = (1 + next() % 4).min(shards);
    let symbols = 1 + next() % 4;
    let code = Code::new(field, threshold).unwrap();
    let data: Vec<Vec<u8>> = (0..threshold)
        .map(|_| {
            (0..symbols)
                .flat_map(|_| symbol(&mut next, order, width))
                .collect()
        })
        .collect();
    let slices: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
    let targets: Vec<u16> = (0..shards as u16).collect();
    let set: Vec<Vec<u8>> = code
        .encode(&slices, &targets)
        .unwrap()
        .iter()
        .map(|shard| shard.to_vec())
        .collect();

    let mut read = set.clone();
    let marks = damage(&mut next, &mut read, symbols, order, width);
    let is_marked = |shard: usize, at: usize| marks.contains(&Mark { shard, symbol: at });
    let spare = shards - threshold;
    let within = |with_marks: bool| within(&read, &set, &marks, width, spare, with_marks);

    // The symbol at a place, as a number.
    let held = |shards: &[Vec<u8>], shard: usize, at: usize| {
        let bytes = &shards[shard][at * width..(at + 1) * width];
        bytes
            .iter()
            .fold(0_u16, |value, &byte| value << 8 | u16::from(byte))
    };

    match code.plan(&given(&read), &marks).unwrap() {
        Plan::Changes(changes) => {
            // In the order of the symbols and, within one, of the shards; a change is filled
            // exactly where a mark is, starts from what was read, and every mark is listed.
            let places: Vec<(usize, usize)> = changes.iter().map(|c| (c.symbol, c.shard)).collect();
            assert!(places.windows(2).all(|pair| pair[0] < pair[1]));
            for change in &changes {
                let marked = is_marked(change.shard, change.symbol);
                assert_eq!(matches!(change.kind, ChangeKind::Filled), marked);
                assert_eq!(change.was, held(&read, change.shard, change.symbol));
            }
            assert!(
                marks
                    .iter()
                    .all(|mark| places.contains(&(mark.symbol, mark.shard)))
            );
            let mut out = read.clone();
            let mut buffers: Vec<(u16, &mut [u8])> =
                (0..).zip(out.iter_mut().map(Vec::as_mut_slice)).collect();
            code.apply(&mut buffers, &changes).unwrap();
            if spare > 0 {
                assert_eq!(code.redundancy(&given(&out)), Ok(Redundancy::Agrees));
            }
            if within(true) {
                assert_eq!(out, set);
            }
        }
        Plan::Agrees | Plan::Unchecked => {
            assert!(marks.is_empty());
            if within(true) {
                assert_eq!(read, set);
            }
        }
        Plan::Unresolved { .. } => assert!(!within(true)),
    }

    let mut out = read.clone();
    let mut buffers: Vec<(u16, &mut [u8])> =
        (0..).zip(out.iter_mut().map(Vec::as_mut_slice)).collect();
    match code.correct(&mut buffers).unwrap() {
        Correction::Corrected(_) => {
            assert_eq!(code.redundancy(&given(&out)), Ok(Redundancy::Agrees));
            if within(false) {
                assert_eq!(out, set);
            }
        }
        Correction::Agrees | Correction::Unchecked => {
            assert_eq!(out, read);
            if within(false) {
                assert_eq!(read, set);
            }
        }
        Correction::Unresolved { .. } => {
            assert_eq!(out, read);
            assert!(!within(false));
        }
    }
});
