//! A set of shard files written by the crate, then damaged: bytes changed, files cut short,
//! lengthened, left out or put in each other's place. A set that states the SHA-256 of its data
//! must give back that data or refuse; an undamaged set must rebuild and repair byte for byte. What
//! is written into outputs is what comes back in buffers, and a refusal leaves no part of it.
#![no_main]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a fuzz target answers by stopping the run: its unwraps and assertions are its checks"
)]

use libfuzzer_sys::fuzz_target;
use reed_solomon_core::{DataDigest, Field, Pile, SetFacts};
use reed_solomon_core_fuzz::{FILL, rebuild_both, repairs_both};

/// The steps a call may take: far more than these small sets need.
const LIMIT: Option<u128> = Some(1 << 24);

fuzz_target!(|bytes: &[u8]| {
    let mut input = bytes.iter().copied();
    let mut next = || usize::from(input.next().unwrap_or(0));
    let field = [Field::Gf256, Field::Gf65536][next() % 2];
    let threshold = 1 + next() % 4;
    let count = threshold + next() % 4;
    let block = (1 + next() % 8) * field.symbol_bytes();
    let digest = [DataDigest::None, DataDigest::Sha256][next() % 2];
    let data: Vec<u8> = (0..next() % 64).map(|_| next() as u8).collect();
    let facts = SetFacts::for_data(field, b"fuzz", threshold, &data, block as u32, digest).unwrap();
    let indexes: Vec<u16> = (0..count as u16).collect();
    let written: Vec<Vec<u8>> = facts
        .files(&data, &indexes)
        .unwrap()
        .iter()
        .map(|file| file.to_vec())
        .collect();
    let length = usize::try_from(facts.file_len()).unwrap();
    let mut outs = vec![vec![FILL; length]; count];
    let mut views: Vec<&mut [u8]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
    facts.files_into(&data, &indexes, &mut views).unwrap();
    assert_eq!(outs, written);

    let mut files = written.clone();
    let mut damaged = false;
    for _ in 0..next() % 6 {
        let (at, kind, place, value) = (next() % count, next() % 5, next() << 8 | next(), next());
        damaged = true;
        match kind {
            0 if !files[at].is_empty() => {
                let length = files[at].len();
                files[at][place % length] ^= (value as u8).max(1);
            }
            1 => {
                let length = files[at].len();
                files[at].truncate(place % (length + 1));
            }
            2 => files[at].push(value as u8),
            3 => files[at] = written[value % count].clone(),
            _ => files[at].clear(),
        }
    }

    let slices: Vec<&[u8]> = files.iter().map(Vec::as_slice).collect();
    let pile = Pile::new(&slices).unwrap();
    let own = pile.sets().iter().find(|set| set.facts() == &facts);
    if !damaged {
        // Whole files: the set is found, rebuilds, and makes each of its files again.
        let set = own.unwrap();
        assert_eq!(*set.rebuild(LIMIT).unwrap().data, *data);
        for (index, file) in indexes.iter().zip(&written) {
            assert_eq!(*set.repair(*index, LIMIT).unwrap(), **file);
        }
    }
    for set in pile.sets() {
        let rebuilt = rebuild_both(set, LIMIT);
        if let (Ok(rebuilt), true) = (&rebuilt, set.facts() == &facts) {
            if digest == DataDigest::Sha256 {
                assert_eq!(*rebuilt.data, *data);
            }
            assert_eq!(rebuilt.data.len(), data.len());
        }
        let _ = set.vote(LIMIT);
        repairs_both(set, &[0, count as u16, 0], LIMIT);
        if let Ok(left) = set.without(&[0]) {
            let _ = left.rebuild(LIMIT);
        }
    }
});
