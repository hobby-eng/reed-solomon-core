//! Words of the cyclic code in every field it takes, with any roots, with symbols changed and
//! symbols marked unreadable. Within the bound `2e + s ≤ parity` a plan and a correction give the
//! word back; whatever they change leaves words of the code, and a refusal changes nothing.
#![no_main]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a fuzz target answers by stopping the run: its unwraps and assertions are its checks"
)]

use libfuzzer_sys::fuzz_target;
use reed_solomon_core::{ChangeKind, Correction, CyclicCode, Field, Mark, Plan};
use reed_solomon_core_fuzz::{damage, symbol, within};

fn views(shards: &[Vec<u8>]) -> Vec<&[u8]> {
    shards.iter().map(Vec::as_slice).collect()
}

fuzz_target!(|bytes: &[u8]| {
    let mut input = bytes.iter().copied();
    let mut next = || usize::from(input.next().unwrap_or(0));
    let field = match next() % 4 {
        0 => Field::Gf256,
        1 => Field::Gf2048,
        _ => Field::binary(2 + (next() % 15) as u8, None).unwrap(),
    };
    let (order, width) = (field.order(), field.symbol_bytes());
    let (data, parity) = (1 + next() % 8, 1 + next() % 8);
    let first = (next() << 8 | next()) as u32;
    let step = (next() << 8 | next()) as u32;
    // Lengths and roots that make no code are refused, before any shard is looked at.
    let Ok(code) = CyclicCode::new(field, data, parity, first, step) else {
        return;
    };
    let symbols = 1 + next() % 4;
    let given: Vec<Vec<u8>> = (0..data)
        .map(|_| {
            (0..symbols)
                .flat_map(|_| symbol(&mut next, order, width))
                .collect()
        })
        .collect();
    let made = code.encode(&views(&given)).unwrap();
    let word: Vec<Vec<u8>> = given
        .iter()
        .cloned()
        .chain(made.iter().map(|shard| shard.to_vec()))
        .collect();
    assert!(code.agrees(&views(&word)).unwrap());

    let mut read = word.clone();
    let marks = damage(&mut next, &mut read, symbols, order, width);
    let is_marked = |shard: usize, at: usize| marks.contains(&Mark { shard, symbol: at });
    let within = |with_marks: bool| within(&read, &word, &marks, width, parity, with_marks);

    match code.plan(&views(&read), &marks).unwrap() {
        Plan::Changes(changes) => {
            for change in &changes {
                let marked = is_marked(change.shard, change.symbol);
                assert_eq!(matches!(change.kind, ChangeKind::Filled), marked);
            }
            let listed = |mark: &Mark| {
                changes
                    .iter()
                    .any(|c| c.shard == mark.shard && c.symbol == mark.symbol)
            };
            assert!(marks.iter().all(listed));
            let mut out = read.clone();
            let mut buffers: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
            code.apply(&mut buffers, &changes).unwrap();
            assert!(code.agrees(&views(&out)).unwrap());
            if within(true) {
                assert_eq!(out, word);
            }
        }
        Plan::Agrees => {
            assert!(marks.is_empty() && code.agrees(&views(&read)).unwrap());
            if within(true) {
                assert_eq!(read, word);
            }
        }
        Plan::Unresolved { .. } => assert!(!within(true)),
        Plan::Unchecked => panic!("a cyclic word always has parity"),
    }

    let mut out = read.clone();
    let mut buffers: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
    match code.correct(&mut buffers).unwrap() {
        Correction::Corrected(_) => {
            assert!(code.agrees(&views(&out)).unwrap());
            if within(false) {
                assert_eq!(out, word);
            }
        }
        Correction::Agrees => {
            assert!(out == read && code.agrees(&views(&read)).unwrap());
        }
        Correction::Unresolved { .. } => {
            assert_eq!(out, read);
            assert!(!within(false));
        }
        Correction::Unchecked => panic!("a cyclic word always has parity"),
    }
});
