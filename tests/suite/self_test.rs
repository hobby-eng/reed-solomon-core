//! The self-test passes, names its sources, and its one pinned answer for the field of pairs is
//! what the independent implementation gives.

use reed_solomon_core::self_test;
use reed_solomon_erasure::galois_16;

#[test]
fn every_check_passes_and_is_named() {
    let run = self_test();
    assert!(run.passed());
    // Five checks of the code, two of the shard file, one of SHA-256, three of the error
    // correction, one plan of changes for marked gaps, one of the vote among the files of a set
    // and one of a digest that differs.
    assert_eq!(run.checks().len(), 14);
    for check in run.checks() {
        assert!(check.passed, "{}", check.name);
        assert!(check.name.len() > 20, "{}", check.name);
    }
}

#[test]
fn the_pinned_shard_of_pairs_is_the_one_reed_solomon_erasure_writes() {
    // Two data shards 01 00 and 00 00, and the shard numbered 256: the last of 257.
    let mut shards = vec![vec![[0_u8; 2]]; 257];
    shards[0][0] = [1, 0];
    galois_16::ReedSolomon::new(2, 255)
        .unwrap()
        .encode(&mut shards)
        .unwrap();
    assert_eq!(shards[256][0], [0x03, 0x80]);
}
