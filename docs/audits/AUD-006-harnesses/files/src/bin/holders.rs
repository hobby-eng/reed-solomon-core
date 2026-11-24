//! The vote over a pile whose holders change at every block, against the same pile whose holders
//! stay: the steps the vote counts and the time it takes.
#![allow(clippy::all)]

use reed_solomon_core::{DataDigest, Field, Pile, SetFacts};
use std::time::Instant;

fn main() {
    let blocks: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(100_000);
    let data: Vec<u8> = (0..blocks).map(|i| (i * 7 + 3) as u8).collect();
    let facts = SetFacts::for_data(Field::Gf256, b"h", 1, &data, 1, DataDigest::None).unwrap();
    let files: Vec<Vec<u8>> = facts
        .files(&data, &[0, 1, 2, 3])
        .unwrap()
        .iter()
        .map(|f| f.to_vec())
        .collect();
    let header = 33;
    let each = 5;

    // Holders that stay: four whole files.
    let still: Vec<&[u8]> = files.iter().map(|f| f.as_slice()).collect();
    // Holders that change: shard 2 damaged in odd blocks, shard 3 in even ones.
    let (mut two, mut three) = (files[2].clone(), files[3].clone());
    for b in 0..blocks {
        let f = if b % 2 == 1 { &mut two } else { &mut three };
        f[header + b * each] ^= 1;
    }
    let changing: Vec<&[u8]> = vec![&files[0], &files[1], &two, &three];
    // The same count of holders per block (three), staying.
    let three_still: Vec<&[u8]> = vec![&files[0], &files[1], &files[2]];
    // Two holders a block, changing: shard 1 damaged in odd blocks, shard 2 in even ones.
    let (mut one_b, mut two_b) = (files[1].clone(), files[2].clone());
    for b in 0..blocks {
        let f = if b % 2 == 1 { &mut one_b } else { &mut two_b };
        f[header + b * each] ^= 1;
    }
    let pairs: Vec<&[u8]> = vec![&files[0], &one_b, &two_b];
    let two_still: Vec<&[u8]> = vec![&files[0], &files[1]];

    for (name, given) in [("4 holders stay", &still), ("3 holders stay", &three_still), ("3 holders change", &changing), ("2 holders stay", &two_still), ("2 holders change", &pairs)] {
        let t = Instant::now();
        let pile = Pile::new(given).unwrap();
        let read = t.elapsed();
        let set = &pile.sets()[0];
        let bound = set.vote_work().unwrap();
        let t = Instant::now();
        let vote = set.vote(None).unwrap();
        let took = t.elapsed();
        println!(
            "{name}: pile {read:?}; vote {took:?}, steps {} (bound {bound}), {:.1} ns per step, outvoted {}, undecided {}",
            vote.steps(),
            took.as_nanos() as f64 / vote.steps() as f64,
            vote.outvoted().len(),
            vote.undecided()
        );
        let t = Instant::now();
        let r = set.rebuild(None).unwrap();
        let took = t.elapsed();
        println!(
            "    rebuild {took:?}, steps {}, {:.1} ns per step",
            r.steps,
            took.as_nanos() as f64 / r.steps as f64
        );
    }
}
