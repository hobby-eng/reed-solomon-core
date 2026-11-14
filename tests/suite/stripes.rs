//! A shard made stretch by stretch is the shard made at once.

use crate::support::{FIELDS, data, data_in, set};
use reed_solomon_core::{Code, Field};

#[test]
fn a_shard_made_in_stripes_is_the_shard_made_at_once() {
    for field in FIELDS {
        let code = Code::new(field, 4).unwrap();
        let data = data_in(field, 4, 1000, 9);
        let all = set(&code, &data, 7);
        let mut weights = [0; 4];
        let indexes = [6, 0, 5, 2];
        let sources = code.sources(&indexes, &mut weights).unwrap();
        for stripe in [2, 8, 14, 256, 998, 1000] {
            for target in [1, 3, 4, 6] {
                let mut made = vec![0xEE; 1000];
                for start in (0..1000).step_by(stripe) {
                    let end = (start + stripe).min(1000);
                    let stripes: Vec<&[u8]> = indexes
                        .iter()
                        .map(|&i| &all[usize::from(i)][start..end])
                        .collect();
                    sources
                        .build(target, &stripes, &mut made[start..end])
                        .unwrap();
                    let whole = &all[usize::from(target)][start..end];
                    assert!(sources.agrees(target, &stripes, whole).unwrap());
                }
                assert_eq!(
                    made,
                    all[usize::from(target)],
                    "stripe {stripe} target {target}"
                );
            }
        }
    }
}

#[test]
fn a_changed_bit_anywhere_in_a_shard_is_seen() {
    let code = Code::new(Field::Gf256, 2).unwrap();
    let all = set(&code, &data(2, 600, 4), 3);
    let mut weights = [0; 2];
    let sources = code.sources(&[0, 1], &mut weights).unwrap();
    let stripes = [&all[0][..], &all[1][..]];
    assert!(sources.agrees(2, &stripes, &all[2]).unwrap());
    for byte in 0..600 {
        let mut changed = all[2].clone();
        changed[byte] ^= 1 << (byte % 8);
        assert!(
            !sources.agrees(2, &stripes, &changed).unwrap(),
            "byte {byte}"
        );
    }
}
