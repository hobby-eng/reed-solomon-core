//! Every choice of exactly `k` shards rebuilds the data, and makes every other shard.

use crate::support::{FIELDS, data, data_in, pick, set};
use reed_solomon_core::{Code, Field};

/// The numbers of the set bits of `mask`, highest first, so that the sources are not in order.
fn members(mask: u32, n: usize) -> Vec<usize> {
    (0..n).rev().filter(|i| mask >> i & 1 == 1).collect()
}

#[test]
fn each_of_the_130_918_choices_of_k_from_n_up_to_16_rebuilds_the_data() {
    let mut choices = 0;
    for n in 2..=16_usize {
        for k in 2..=n {
            let code = Code::new(Field::Gf256, k).unwrap();
            let data = data(k, 3, (n * 31 + k) as u64);
            let all = set(&code, &data, n);
            let targets: Vec<u16> = (0..k as u16).collect();
            for mask in 0_u32..1 << n {
                if mask.count_ones() as usize != k {
                    continue;
                }
                let chosen = pick(&all, &members(mask, n));
                assert_eq!(
                    code.rebuild(&chosen, &targets).unwrap(),
                    data,
                    "{k} {n} {mask:b}"
                );
                choices += 1;
            }
        }
    }
    assert_eq!(choices, 130_918);
}

#[test]
fn each_choice_of_k_from_n_up_to_10_makes_every_shard_in_every_field() {
    for field in FIELDS {
        for n in 2..=10_usize {
            for k in 1..=n {
                let code = Code::new(field, k).unwrap();
                let data = data_in(field, k, 4, (n * 77 + k) as u64);
                let all = set(&code, &data, n);
                let targets: Vec<u16> = (0..n as u16).collect();
                for mask in 0_u32..1 << n {
                    if mask.count_ones() as usize != k {
                        continue;
                    }
                    // Data and parity shards alike, those among the sources included.
                    let chosen = pick(&all, &members(mask, n));
                    assert_eq!(
                        code.rebuild(&chosen, &targets).unwrap(),
                        all,
                        "{k} {n} {mask:b}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_set_grows_without_touching_its_shards_and_old_and_new_mix() {
    for field in FIELDS {
        let code = Code::new(field, 3).unwrap();
        let data = data_in(field, 3, 34, 7);
        let five = set(&code, &data, 5);
        // Two more shards from three of the old ones, without the data.
        let later = code.rebuild(&pick(&five, &[4, 1, 3]), &[5, 6]).unwrap();
        let seven = set(&code, &data, 7);
        assert_eq!(&seven[..5], &five[..]);
        assert_eq!(&seven[5..], &later[..]);
        // Any three of the seven, old and new mixed, give the data.
        for mask in 0_u32..1 << 7 {
            if mask.count_ones() == 3 {
                let chosen = pick(&seven, &members(mask, 7));
                assert_eq!(code.rebuild(&chosen, &[0, 1, 2]).unwrap(), data, "{mask:b}");
            }
        }
    }
}
