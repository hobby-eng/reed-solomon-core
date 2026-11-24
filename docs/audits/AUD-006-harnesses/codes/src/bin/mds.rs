//! Any k of n rebuild: every subset of every size for GF(4), GF(8), GF(16) with n = the field
//! size, sampled subsets above, and the shards against an interpolation by Gaussian elimination
//! written apart, at scattered points, in every binary degree and the named fields; boundary
//! thresholds k = 1 and k = field size.

use aud006_codes_probe::gf::{Gf, evaluate, interpolate};
use reed_solomon_core::{Code, Field};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn poly_of(field: Field) -> (u32, u32) {
    match field {
        Field::Gf256 => (8, 0x11D),
        Field::Gf2048 => (11, 0x805),
        Field::Binary(b) => (u32::from(b.degree()), b.polynomial()),
        Field::Gf65536 => unreachable!(),
    }
}
fn to_bytes(field: Field, v: &[u16]) -> Vec<u8> {
    let w = field.symbol_bytes();
    v.iter().flat_map(|x| x.to_be_bytes().into_iter().skip(2 - w)).collect()
}
fn to_values(field: Field, b: &[u8]) -> Vec<u16> {
    let w = field.symbol_bytes();
    b.chunks(w).map(|c| c.iter().fold(0u16, |v, &x| v << 8 | u16::from(x))).collect()
}

/// Shards at `points` for the data `data` (k shards of `len` symbols) by the oracle.
fn oracle_set(gf: &Gf, data: &[Vec<u16>], points: &[u16]) -> Vec<Vec<u16>> {
    let k = data.len();
    let len = data[0].len();
    let xs: Vec<u16> = (0..k as u16).collect();
    let mut out = vec![vec![0u16; len]; points.len()];
    for col in 0..len {
        let d: Vec<u16> = data.iter().map(|s| s[col]).collect();
        let coeffs = interpolate(gf, &xs, &d);
        for (o, &p) in out.iter_mut().zip(points) {
            o[col] = evaluate(gf, &coeffs, p);
        }
    }
    out
}

fn exhaustive(field: Field, rng: &mut Rng) -> (usize, usize) {
    let (m, poly) = poly_of(field);
    let gf = Gf::new(m, poly);
    let n = field.order();
    let len = 3;
    let mut checked = 0;
    let mut bad = 0;
    for k in 1..=n {
        let code = Code::new(field, k).unwrap();
        let data: Vec<Vec<u16>> = (0..k).map(|_| (0..len).map(|_| rng.below(n) as u16).collect()).collect();
        let data_bytes: Vec<Vec<u8>> = data.iter().map(|d| to_bytes(field, d)).collect();
        let views: Vec<&[u8]> = data_bytes.iter().map(Vec::as_slice).collect();
        let all_points: Vec<u16> = (0..n as u16).collect();
        let set: Vec<Vec<u8>> = code.encode(&views, &all_points).unwrap().iter().map(|b| b.to_vec()).collect();
        let expected = oracle_set(&gf, &data, &all_points);
        if set.iter().map(|s| to_values(field, s)).collect::<Vec<_>>() != expected {
            println!("  ENCODE MISMATCH {field:?} k={k}");
            bad += 1;
        }
        for mask in 0u64..(1u64 << n) {
            if mask.count_ones() as usize != k {
                continue;
            }
            let mut chosen: Vec<(u16, &[u8])> = (0..n).filter(|i| mask >> i & 1 == 1).map(|i| (i as u16, set[i].as_slice())).collect();
            // A shuffled order of sources.
            for i in (1..chosen.len()).rev() {
                let j = rng.below(i + 1);
                chosen.swap(i, j);
            }
            let made: Vec<Vec<u8>> = code.rebuild(&chosen, &all_points).unwrap().iter().map(|b| b.to_vec()).collect();
            checked += 1;
            if made != set {
                bad += 1;
                if bad < 5 {
                    println!("  REBUILD MISMATCH {field:?} k={k} mask={mask:b}");
                }
            }
        }
    }
    (checked, bad)
}

fn sampled(field: Field, rng: &mut Rng, rounds: usize, max_k: usize, targets_cap: usize, len: usize) -> (usize, usize) {
    let (m, poly) = poly_of(field);
    let gf = Gf::new(m, poly);
    let q = field.order();
    let mut bad = 0;
    for round in 0..rounds {
        let k = match round % 5 {
            0 => 1,
            1 => max_k.min(q),
            _ => 1 + rng.below(max_k.min(q)),
        };
        let code = Code::new(field, k).unwrap();
        // k data shards at points 0..k, then k scattered sources among all points.
        let data: Vec<Vec<u16>> = (0..k).map(|_| (0..len).map(|_| rng.below(q) as u16).collect()).collect();
        let data_bytes: Vec<Vec<u8>> = data.iter().map(|d| to_bytes(field, d)).collect();
        let views: Vec<&[u8]> = data_bytes.iter().map(Vec::as_slice).collect();
        let mut pool: Vec<u16> = (0..q).map(|p| p as u16).collect();
        for i in (1..pool.len()).rev() {
            let j = rng.below(i + 1);
            pool.swap(i, j);
        }
        let sources: Vec<u16> = pool[..k].to_vec();
        let mut targets: Vec<u16> = pool.iter().copied().take(targets_cap).collect();
        targets.extend([0, (q - 1) as u16, (k - 1) as u16]);
        let source_shards: Vec<Vec<u8>> = code.encode(&views, &sources).unwrap().iter().map(|b| b.to_vec()).collect();
        let given: Vec<(u16, &[u8])> = sources.iter().copied().zip(source_shards.iter().map(Vec::as_slice)).collect();
        let made: Vec<Vec<u16>> = code.rebuild(&given, &targets).unwrap().iter().map(|b| to_values(field, b)).collect();
        let expected = if k <= 256 { Some(oracle_set(&gf, &data, &targets)) } else { None };
        // The data comes back from the scattered sources.
        let back: Vec<Vec<u16>> = code.rebuild(&given, &(0..k as u16).collect::<Vec<_>>()).unwrap().iter().map(|b| to_values(field, b)).collect();
        if back != data || expected.as_ref().is_some_and(|e| *e != made) {
            bad += 1;
            if bad < 5 {
                println!("  SAMPLED MISMATCH {field:?} k={k} round {round}");
            }
        }
    }
    (rounds, bad)
}

fn main() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for m in [2u8, 3, 4] {
        let field = Field::binary(m, None).unwrap();
        let (checked, bad) = exhaustive(field, &mut rng);
        if bad > 0 {
            aud006_codes_probe::fail();
        }
        println!("{field:?} n = {}: {checked} subsets of every size rebuilt every shard, {bad} wrong", field.order());
    }
    for m in 5u8..=16 {
        let field = Field::binary(m, None).unwrap();
        let (rounds, bad) = sampled(field, &mut rng, if m <= 10 { 400 } else { 60 }, 64.min(field.order()), 200, 3);
        if bad > 0 {
            aud006_codes_probe::fail();
        }
        println!("{field:?}: {rounds} sampled scattered choices, k up to {}, {bad} wrong", 64.min(field.order()));
    }
    for (field, rounds, max_k, cap) in [(Field::Gf256, 300, 256, 256), (Field::Gf2048, 60, 2048, 2048), (Field::Gf2048, 200, 40, 300)] {
        let (r, bad) = sampled(field, &mut rng, rounds, max_k, cap, 2);
        if bad > 0 {
            aud006_codes_probe::fail();
        }
        println!("{field:?}: {r} sampled scattered choices, k up to {max_k}, {bad} wrong");
    }
    // GF(65536): scattered points over the whole index range, small and middle thresholds.
    let code = Code::new(Field::Gf65536, 300).unwrap();
    let data: Vec<Vec<u8>> = (0..300).map(|_| (0..8).map(|_| rng.next() as u8).collect()).collect();
    let views: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
    let mut bad = 0;
    for round in 0..20 {
        let mut pool: Vec<u16> = (0..=u16::MAX).collect();
        for i in (1..pool.len()).rev() {
            let j = rng.below(i + 1);
            pool.swap(i, j);
        }
        let sources = &pool[..300];
        let shards: Vec<Vec<u8>> = code.encode(&views, sources).unwrap().iter().map(|b| b.to_vec()).collect();
        let given: Vec<(u16, &[u8])> = sources.iter().copied().zip(shards.iter().map(Vec::as_slice)).collect();
        let back: Vec<Vec<u8>> = code.rebuild(&given, &(0..300).collect::<Vec<u16>>()).unwrap().iter().map(|b| b.to_vec()).collect();
        let others = &pool[300..1300];
        let a: Vec<Vec<u8>> = code.rebuild(&given, others).unwrap().iter().map(|b| b.to_vec()).collect();
        let b: Vec<Vec<u8>> = code.encode(&views, others).unwrap().iter().map(|b| b.to_vec()).collect();
        if back != data || a != b {
            bad += 1;
            println!("  GF65536 MISMATCH round {round}");
        }
    }
    if bad > 0 {
        aud006_codes_probe::fail();
    }
    println!("Gf65536 k = 300: 20 scattered choices over all 65536 indexes, 1000 other shards each: {bad} wrong");
    // Threshold equal to the field size: every shard is a source, every target a copy.
    for field in [Field::Gf256, Field::Gf2048, Field::binary(4, None).unwrap()] {
        let q = field.order();
        let code = Code::new(field, q).unwrap();
        let data: Vec<Vec<u8>> = (0..q).map(|i| to_bytes(field, &[(i % q) as u16, ((i * 7) % q) as u16])).collect();
        let views: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
        let all: Vec<u16> = (0..q as u16).collect();
        let set: Vec<Vec<u8>> = code.encode(&views, &all).unwrap().iter().map(|b| b.to_vec()).collect();
        let mut order: Vec<usize> = (0..q).collect();
        for i in (1..q).rev() {
            let j = rng.below(i + 1);
            order.swap(i, j);
        }
        let given: Vec<(u16, &[u8])> = order.iter().map(|&i| (i as u16, set[i].as_slice())).collect();
        let back: Vec<Vec<u8>> = code.rebuild(&given, &all).unwrap().iter().map(|b| b.to_vec()).collect();
        if set != data || back != data {
            aud006_codes_probe::fail();
        }
        println!("{field:?} k = n = {q}: encode is the data {}, shuffled rebuild {}", set == data, back == data);
        // One shard more than the field has: refused.
        let mut more = given.clone();
        more.push((0, set[0].as_slice()));
        let refused = code.rebuild(&more, &[0]).err();
        println!("  one more shard than indexes: {refused:?}");
        if refused.is_none() {
            aud006_codes_probe::fail();
        }
    }
    aud006_codes_probe::finish();
}
