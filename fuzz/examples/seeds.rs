//! Writes whole sets of shard files as the first inputs of the `files` target: a byte that says
//! how many files follow, then the files, which are of one length.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a helper that writes the first inputs: it stops at a fault"
)]

use reed_solomon_core::{DataDigest, Field, SetFacts};

fn main() {
    let folder = std::env::args()
        .nth(1)
        .expect("the corpus folder of the `files` target");
    let sets = [
        (Field::Gf256, 2, 4_u16, 4, DataDigest::None),
        (Field::Gf256, 3, 4, 8, DataDigest::Sha256),
        (Field::Gf65536, 2, 3, 4, DataDigest::Sha256),
        (Field::Gf256, 1, 2, 2, DataDigest::None),
    ];
    for (number, (field, threshold, count, block, digest)) in sets.into_iter().enumerate() {
        let data: Vec<u8> = (0..23).map(|i| (i * 7 + number) as u8).collect();
        let facts = SetFacts::for_data(field, b"seed", threshold, &data, block, digest).unwrap();
        let indexes: Vec<u16> = (0..count).collect();
        let mut seed = vec![(count - 1) as u8];
        for file in &facts.files(&data, &indexes).unwrap() {
            seed.extend_from_slice(file);
        }
        std::fs::write(format!("{folder}/set-{number}"), seed).unwrap();
    }
}
