//! Code::correct and Code::plan against a brute-force bounded-distance decoder: every received
//! column (and every set of marks) for small fields and point sets, the point 0 included and left
//! out, shards in scattered order. Expected: the unique code word within 2e + s <= m - k, else
//! Unresolved with nothing changed; Agrees / Unchecked as documented.

use aud006_codes_probe::gf::{Gf, evaluate, interpolate};
use reed_solomon_core::{ChangeKind, Code, Correction, Field, Mark, Plan};
use std::collections::HashMap;

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

/// Every code word of threshold k at `points`, by the oracle: data at points 0..k.
fn codewords(gf: &Gf, k: usize, points: &[u16]) -> Vec<Vec<u16>> {
    let q = gf.q;
    let xs: Vec<u16> = (0..k as u16).collect();
    let mut out = Vec::new();
    let total = q.pow(k as u32);
    for idx in 0..total {
        let mut d = Vec::with_capacity(k);
        let mut v = idx;
        for _ in 0..k {
            d.push((v % q) as u16);
            v /= q;
        }
        let c = interpolate(gf, &xs, &d);
        out.push(points.iter().map(|&p| evaluate(gf, &c, p)).collect());
    }
    out
}

/// The unique code word c with 2·#{i not in S: c_i != y_i} + |S| <= r, if any (brute force).
fn expected<'a>(words: &'a [Vec<u16>], y: &[u16], marked: &[bool], r: usize) -> Option<&'a Vec<u16>> {
    let s = marked.iter().filter(|&&b| b).count();
    if s > r {
        return None;
    }
    let mut found: Option<&Vec<u16>> = None;
    for c in words {
        let e = c.iter().zip(y).zip(marked).filter(|((a, b), m)| !**m && a != b).count();
        if 2 * e + s <= r {
            assert!(found.is_none(), "two code words within the bound: the oracle is wrong");
            found = Some(c);
        }
    }
    found
}

#[derive(Default, Debug)]
struct Tally {
    calls: usize,
    corrected: usize,
    agrees: usize,
    unresolved: usize,
    unchecked: usize,
    changes: usize,
    bad: usize,
}

fn run_correct(code: &Code, points: &[u16], y: &[u16], want: Option<&Vec<u16>>, r: usize, t: &mut Tally, label: &str) {
    let mut bytes: Vec<[u8; 1]> = y.iter().map(|&v| [v as u8]).collect();
    let mut shards: Vec<(u16, &mut [u8])> = points.iter().copied().zip(bytes.iter_mut().map(|b| &mut b[..])).collect();
    let got = code.correct(&mut shards).unwrap();
    drop(shards);
    let after: Vec<u16> = bytes.iter().map(|b| u16::from(b[0])).collect();
    t.calls += 1;
    let ok = match (&got, want) {
        (Correction::Unchecked, _) => {
            t.unchecked += 1;
            r == 0 && after == y
        }
        (Correction::Agrees, Some(c)) => {
            t.agrees += 1;
            c.as_slice() == y && after == y
        }
        (Correction::Corrected(counts), Some(c)) => {
            t.corrected += 1;
            let changed: Vec<usize> = y.iter().zip(c.iter()).map(|(a, b)| usize::from(a != b)).collect();
            after == *c && *counts == changed && c.as_slice() != y
        }
        (Correction::Unresolved { columns: 1 }, None) => {
            t.unresolved += 1;
            r > 0 && after == y
        }
        _ => false,
    };
    if !ok {
        t.bad += 1;
        if t.bad <= 5 {
            println!("  CORRECT MISMATCH {label} points {points:?} y {y:?}: crate {got:?} after {after:?}, expected {want:?}");
        }
    }
}

fn run_plan(code: &Code, points: &[u16], y: &[u16], marked: &[bool], want: Option<&Vec<u16>>, r: usize, t: &mut Tally, label: &str) {
    let bytes: Vec<[u8; 1]> = y.iter().map(|&v| [v as u8]).collect();
    let shards: Vec<(u16, &[u8])> = points.iter().copied().zip(bytes.iter().map(|b| &b[..])).collect();
    let marks: Vec<Mark> = marked.iter().enumerate().filter(|(_, m)| **m).map(|(shard, _)| Mark { shard, symbol: 0 }).collect();
    let got = code.plan(&shards, &marks).unwrap();
    t.calls += 1;
    let s = marks.len();
    let ok = match (&got, want) {
        (Plan::Unchecked, _) => {
            t.unchecked += 1;
            r == 0 && s == 0
        }
        (Plan::Agrees, Some(c)) => {
            t.agrees += 1;
            s == 0 && c.as_slice() == y
        }
        (Plan::Changes(changes), Some(c)) => {
            t.changes += 1;
            let mut after: Vec<[u8; 1]> = bytes.clone();
            let mut b: Vec<(u16, &mut [u8])> = points.iter().copied().zip(after.iter_mut().map(|x| &mut x[..])).collect();
            let applied = code.apply(&mut b, changes).is_ok();
            drop(b);
            let after: Vec<u16> = after.iter().map(|x| u16::from(x[0])).collect();
            // Listed: every mark as Filled, every other changed place as Replaced, in shard order.
            let mut want_list = Vec::new();
            for i in 0..y.len() {
                if marked[i] {
                    want_list.push((i, ChangeKind::Filled, y[i], c[i]));
                } else if c[i] != y[i] {
                    want_list.push((i, ChangeKind::Replaced, y[i], c[i]));
                }
            }
            let listed: Vec<(usize, ChangeKind, u16, u16)> = changes
                .iter()
                .enumerate()
                .map(|(at, ch)| {
                    let (was, becomes) = changes.values(at, |a, b| (a, b)).unwrap();
                    assert_eq!(ch.symbol, 0);
                    (ch.shard, ch.kind, was, becomes)
                })
                .collect();
            applied && after == *c && listed == want_list
        }
        (Plan::Unresolved { columns: 1 }, None) => {
            t.unresolved += 1;
            true
        }
        _ => false,
    };
    if !ok {
        t.bad += 1;
        if t.bad <= 5 {
            println!("  PLAN MISMATCH {label} points {points:?} y {y:?} marks {marked:?}: crate {got:?}, expected {want:?}");
        }
    }
}

fn field_of(m: u8) -> (Field, Gf) {
    let field = Field::binary(m, None).unwrap();
    let poly = match field {
        Field::Gf256 => 0x11D,
        Field::Binary(b) => b.polynomial(),
        _ => unreachable!(),
    };
    (field, Gf::new(u32::from(m), poly))
}

fn main() {
    let mut rng = Rng(0xDEAD_BEEF_1234_5678);
    // 1. Exhaustive: GF(4) every point set of size >= 2 in two orders, every k, every column,
    //    every mark set; GF(8) chosen point sets, every column for correct, every mark set for plan
    //    while the work stays small.
    let mut tc = Tally::default();
    let mut tp = Tally::default();
    let (f4, g4) = field_of(2);
    for mask in 0u32..16 {
        if mask.count_ones() < 2 {
            continue;
        }
        let mut points: Vec<u16> = (0..4u16).filter(|i| mask >> i & 1 == 1).collect();
        for order in 0..2 {
            if order == 1 {
                points.reverse();
            }
            let m = points.len();
            for k in 1..=m {
                let code = Code::new(f4, k).unwrap();
                let words = codewords(&g4, k, &points);
                let r = m - k;
                for yi in 0..4usize.pow(m as u32) {
                    let y: Vec<u16> = (0..m).map(|i| ((yi >> (2 * i)) & 3) as u16).collect();
                    let none = vec![false; m];
                    run_correct(&code, &points, &y, expected(&words, &y, &none, r), r, &mut tc, "GF4");
                    for sm in 0u32..(1 << m) {
                        let marked: Vec<bool> = (0..m).map(|i| sm >> i & 1 == 1).collect();
                        run_plan(&code, &points, &y, &marked, expected(&words, &y, &marked, r), r, &mut tp, "GF4");
                    }
                }
            }
        }
    }
    println!("GF(4) exhaustive correct: {tc:?}");
    println!("GF(4) exhaustive plan:    {tp:?}");
    let (f8, g8) = field_of(3);
    let sets: [&[u16]; 5] = [&[0, 1, 2, 3, 4], &[7, 3, 0, 5, 1, 6], &[1, 2, 3, 4, 5, 6, 7], &[6, 0, 4], &[5, 2, 7, 1]];
    let mut tc = Tally::default();
    let mut tp = Tally::default();
    for points in sets {
        let m = points.len();
        for k in 1..=m {
            let code = Code::new(f8, k).unwrap();
            let r = m - k;
            let t = r / 2;
            let small = 8usize.pow(k as u32) <= 4096;
            let words = if small { codewords(&g8, k, points) } else { Vec::new() };
            let oracle = |y: &[u16], marked: &[bool]| -> Option<Vec<u16>> {
                if small { expected(&words, y, marked, r).cloned() } else { isd(&g8, points, k, y, marked, r) }
            };
            // correct: every column when 8^m <= 2^18 and the words are few, else sampled columns.
            let total = 8usize.pow(m as u32);
            let exhaustive = small && total <= 1 << 18;
            let table = if exhaustive { Some(radius_table(&words, m, t, 8)) } else { None };
            let rounds = if exhaustive { total } else { 30_000 };
            for round in 0..rounds {
                let y: Vec<u16> = if exhaustive {
                    (0..m).map(|i| ((round >> (3 * i)) & 7) as u16).collect()
                } else {
                    near(&g8, &mut rng, points, k, t + 2)
                };
                let want = match &table {
                    Some(tb) => tb.get(&y).map(|&ci| words[ci].clone()),
                    None => oracle(&y, &vec![false; m]),
                };
                run_correct(&code, points, &y, want.as_ref(), r, &mut tc, "GF8");
            }
            // plan: sampled columns, random mark sets, and every mark set for a few columns.
            for round in 0..20_000 {
                let y = near(&g8, &mut rng, points, k, t + 2);
                let marked: Vec<bool> = if round < 64 {
                    (0..m).map(|i| (round >> i) & 1 == 1).collect()
                } else {
                    (0..m).map(|_| rng.below(3) == 0).collect()
                };
                let want = oracle(&y, &marked);
                run_plan(&code, points, &y, &marked, want.as_ref(), r, &mut tp, "GF8");
            }
        }
    }
    println!("GF(8) correct: {tc:?}");
    println!("GF(8) plan:    {tp:?}");
    // GF(16): all sixteen points in scattered order, several thresholds.
    let (f16, g16) = field_of(4);
    let mut tc = Tally::default();
    let mut tp = Tally::default();
    let mut points: Vec<u16> = (0..16).collect();
    for i in (1..16).rev() {
        let j = rng.below(i + 1);
        points.swap(i, j);
    }
    for k in [1usize, 2, 3, 9, 12, 14, 15, 16] {
        let code = Code::new(f16, k).unwrap();
        let m = 16;
        let r = m - k;
        let t = r / 2;
        let small = k <= 3;
        let words = if small { codewords(&g16, k, &points) } else { Vec::new() };
        for _ in 0..3000 {
            let y = near(&g16, &mut rng, &points, k, t + 2);
            let none = vec![false; m];
            let want = if small { expected(&words, &y, &none, r).cloned() } else { isd(&g16, &points, k, &y, &none, r) };
            run_correct(&code, &points, &y, want.as_ref(), r, &mut tc, "GF16");
            let marked: Vec<bool> = (0..m).map(|_| rng.below(4) == 0).collect();
            let want = if small { expected(&words, &y, &marked, r).cloned() } else { isd(&g16, &points, k, &y, &marked, r) };
            run_plan(&code, &points, &y, &marked, want.as_ref(), r, &mut tp, "GF16");
        }
    }
    println!("GF(16) correct: {tc:?}");
    println!("GF(16) plan:    {tp:?}");
}

/// Every column within `t` of a code word, with the word's place.
fn radius_table(words: &[Vec<u16>], m: usize, t: usize, q: u16) -> HashMap<Vec<u16>, usize> {
    let mut table: HashMap<Vec<u16>, usize> = HashMap::new();
    for (ci, c) in words.iter().enumerate() {
        let mut stack = vec![(0usize, 0usize, c.clone())];
        while let Some((pos, w, y)) = stack.pop() {
            if pos == m {
                assert!(table.insert(y, ci).is_none());
                continue;
            }
            stack.push((pos + 1, w, y.clone()));
            if w < t {
                for e in 1..q {
                    let mut z = y.clone();
                    z[pos] ^= e;
                    stack.push((pos + 1, w + 1, z));
                }
            }
        }
    }
    table
}

/// A random code word at `points` with up to `most` random symbols changed.
fn near(gf: &Gf, rng: &mut Rng, points: &[u16], k: usize, most: usize) -> Vec<u16> {
    let xs: Vec<u16> = (0..k as u16).collect();
    let d: Vec<u16> = (0..k).map(|_| rng.below(gf.q) as u16).collect();
    let c = interpolate(gf, &xs, &d);
    let mut y: Vec<u16> = points.iter().map(|&p| evaluate(gf, &c, p)).collect();
    for _ in 0..rng.below(most + 1) {
        let i = rng.below(y.len());
        y[i] = rng.below(gf.q) as u16;
    }
    y
}

/// Information-set decoding: the unique code word with 2·#{unmarked differing} + s <= r, by
/// trying every set E of at most (r - s)/2 unmarked places as the errors and interpolating
/// through k other unmarked places.
fn isd(gf: &Gf, points: &[u16], k: usize, y: &[u16], marked: &[bool], r: usize) -> Option<Vec<u16>> {
    let s = marked.iter().filter(|&&b| b).count();
    if s > r {
        return None;
    }
    let emax = (r - s) / 2;
    let unmarked: Vec<usize> = (0..y.len()).filter(|&i| !marked[i]).collect();
    let mut chosen: Vec<usize> = Vec::new();
    fn walk(gf: &Gf, points: &[u16], k: usize, y: &[u16], marked: &[bool], r: usize, s: usize, unmarked: &[usize], start: usize, emax: usize, chosen: &mut Vec<usize>) -> Option<Vec<u16>> {
        // try the current error set
        let info: Vec<usize> = unmarked.iter().copied().filter(|i| !chosen.contains(i)).take(k).collect();
        if info.len() == k {
            let xs: Vec<u16> = info.iter().map(|&i| points[i]).collect();
            let ds: Vec<u16> = info.iter().map(|&i| y[i]).collect();
            let c = interpolate(gf, &xs, &ds);
            let word: Vec<u16> = points.iter().map(|&p| evaluate(gf, &c, p)).collect();
            let e = (0..y.len()).filter(|&i| !marked[i] && word[i] != y[i]).count();
            if 2 * e + s <= r {
                return Some(word);
            }
        }
        if chosen.len() == emax {
            return None;
        }
        for at in start..unmarked.len() {
            chosen.push(unmarked[at]);
            let found = walk(gf, points, k, y, marked, r, s, unmarked, at + 1, emax, chosen);
            chosen.pop();
            if found.is_some() {
                return found;
            }
        }
        None
    }
    walk(gf, points, k, y, marked, r, s, &unmarked, 0, emax, &mut chosen)
}
