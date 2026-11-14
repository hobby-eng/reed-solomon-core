//! The bytes of every shard equal those of `reed-solomon-erasure` 6.0.0, an implementation that
//! shares no code with this crate: it builds a Vandermonde matrix and inverts its top rows.

use crate::support::{data, set};
use reed_solomon_core::{Code, Field};
use reed_solomon_erasure::{galois_8, galois_16};

/// The set of `n` shards as reed-solomon-erasure writes it over GF(2^8).
fn theirs_bytes(data: &[Vec<u8>], n: usize) -> Vec<Vec<u8>> {
    let (k, length) = (data.len(), data[0].len());
    let mut shards = data.to_vec();
    shards.extend((k..n).map(|_| vec![0; length]));
    galois_8::ReedSolomon::new(k, n - k)
        .unwrap()
        .encode(&mut shards)
        .unwrap();
    shards
}

/// The set of `n` shards as reed-solomon-erasure writes it over its GF(2^16).
fn theirs_pairs(data: &[Vec<u8>], n: usize) -> Vec<Vec<u8>> {
    let (k, symbols) = (data.len(), data[0].len() / 2);
    let mut shards: Vec<Vec<[u8; 2]>> = data
        .iter()
        .map(|shard| shard.as_chunks::<2>().0.to_vec())
        .collect();
    shards.extend((k..n).map(|_| vec![[0; 2]; symbols]));
    galois_16::ReedSolomon::new(k, n - k)
        .unwrap()
        .encode(&mut shards)
        .unwrap();
    shards.iter().map(|shard| shard.concat()).collect()
}

#[test]
fn every_shape_up_to_16_equals_reed_solomon_erasure_in_both_fields() {
    for n in 2..=16 {
        for k in 1..n {
            let length = 2 * (1 + (n * 7 + k) % 35);
            let data = data(k, length, (n * 100 + k) as u64);
            let ours = set(&Code::new(Field::Gf256, k).unwrap(), &data, n);
            assert_eq!(ours, theirs_bytes(&data, n), "GF(256) k {k} n {n}");
            let ours = set(&Code::new(Field::Gf65536, k).unwrap(), &data, n);
            assert_eq!(ours, theirs_pairs(&data, n), "GF(65536) k {k} n {n}");
        }
    }
}

#[test]
fn large_sets_of_bytes_equal_reed_solomon_erasure() {
    for (k, n) in [(1, 256), (100, 130), (128, 255), (200, 256), (255, 256)] {
        let data = data(k, 9, (n * 1000 + k) as u64);
        let ours = set(&Code::new(Field::Gf256, k).unwrap(), &data, n);
        assert_eq!(ours, theirs_bytes(&data, n), "k {k} n {n}");
    }
}

#[test]
fn sets_of_pairs_beyond_256_shards_equal_reed_solomon_erasure() {
    for (k, n) in [(250, 260), (255, 257), (256, 258), (300, 304)] {
        let data = data(k, 6, (n * 1000 + k) as u64);
        let ours = set(&Code::new(Field::Gf65536, k).unwrap(), &data, n);
        assert_eq!(ours, theirs_pairs(&data, n), "k {k} n {n}");
    }
}

#[test]
fn every_one_of_the_65536_shards_equals_reed_solomon_erasure() {
    // The whole index range, 255, 256 and 65,535 included, for a threshold of three.
    let k = 3;
    let data = data(k, 2, 65_536);
    let ours = set(&Code::new(Field::Gf65536, k).unwrap(), &data, 65_536);
    assert_eq!(ours, theirs_pairs(&data, 65_536));
}
