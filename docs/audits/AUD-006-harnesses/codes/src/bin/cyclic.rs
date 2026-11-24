//! The cyclic code against brute force: parity by long division written apart, and every
//! received word (small shapes) with sampled erasure sets, decoded by CyclicWords::correct_word,
//! CyclicCode::plan (marks and lost) + apply and CyclicCode::correct (lost); expected is the unique
//! code word with 2e + s <= parity, else nothing found and nothing changed.

use aud006_codes_probe::gf::{Gf, cyclic_parity};
use reed_solomon_core::{ChangeKind, CodeError, Correction, CyclicCode, CyclicWords, Field, Mark, Plan};

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

fn expected<'a>(words: &'a [Vec<u16>], y: &[u16], erased: &[bool], p: usize) -> Option<&'a Vec<u16>> {
    let s = erased.iter().filter(|&&b| b).count();
    if s > p {
        return None;
    }
    let mut found = None;
    for c in words {
        let e = c.iter().zip(y).zip(erased).filter(|((a, b), m)| !**m && a != b).count();
        if 2 * e + s <= p {
            assert!(found.is_none(), "two words within the bound");
            found = Some(c);
        }
    }
    found
}

#[derive(Default, Debug)]
struct Tally {
    cases: usize,
    fixed: usize,
    clean: usize,
    gave_up: usize,
    bad: usize,
}

fn bytes_of(field: Field, y: &[u16]) -> Vec<Vec<u8>> {
    let w = field.symbol_bytes();
    y.iter().map(|v| v.to_be_bytes()[2 - w..].to_vec()).collect()
}
fn words_of(s: &[Vec<u8>]) -> Vec<u16> {
    s.iter().map(|b| b.iter().fold(0u16, |v, &x| v << 8 | u16::from(x))).collect()
}

#[allow(clippy::too_many_arguments)]
fn check(code: &CyclicCode, words: &[Vec<u16>], y: &[u16], erased: &[bool], placeholder: u16, rng: &mut Rng, t: &mut Tally, label: &str) {
    let field = code.field();
    let n = code.length();
    let p = code.parity();
    let order = field.order() as u16;
    let want = expected(words, y, erased, p);
    let places: Vec<usize> = (0..n).filter(|&i| erased[i]).collect();
    let s = places.len();
    t.cases += 1;
    let mut problems = Vec::new();
    // (a) CyclicWords::correct_word, erased places holding a placeholder (maybe outside the field).
    let mut w: Vec<u16> = y.to_vec();
    for &i in &places {
        w[i] = placeholder;
    }
    let before = w.clone();
    let mut room = vec![0xABCDu16; CyclicWords::room(p)];
    let got = code.words().correct_word(&mut w, &places, &mut room);
    match (got, want) {
        (Ok(Some(written)), Some(c)) => {
            let replaced = (0..n).filter(|&i| !erased[i] && y[i] != c[i]).count();
            if w != *c || written != s + replaced {
                problems.push(format!("correct_word wrote {written} -> {w:?}"));
            }
        }
        (Ok(None), None) => {
            if w != before {
                problems.push("correct_word changed a word it gave up".into());
            }
        }
        (got, _) => problems.push(format!("correct_word {got:?}")),
    }
    if room.iter().any(|&e| e != 0) {
        problems.push("room not wiped".into());
    }
    // (b) CyclicCode::plan with marks; a marked symbol holds a field element.
    let fill = (placeholder % order).max(0);
    let mut held_vals = y.to_vec();
    for &i in &places {
        held_vals[i] = fill;
    }
    let held = bytes_of(field, &held_vals);
    let views: Vec<&[u8]> = held.iter().map(Vec::as_slice).collect();
    let marks: Vec<Mark> = places.iter().map(|&shard| Mark { shard, symbol: 0 }).collect();
    let lost_variant = rng.below(2) == 0;
    let plan = if lost_variant { code.plan(&views, &places, &[]) } else { code.plan(&views, &[], &marks) };
    match (plan, want) {
        (Ok(Plan::Changes(changes)), Some(c)) => {
            let mut out = held.clone();
            let mut b: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
            let applied = code.apply(&mut b, &changes);
            drop(b);
            let listed: Vec<(usize, ChangeKind)> = changes.iter().map(|ch| (ch.shard, ch.kind)).collect();
            let want_list: Vec<(usize, ChangeKind)> = (0..n)
                .filter_map(|i| {
                    if erased[i] {
                        Some((i, ChangeKind::Filled))
                    } else if held_vals[i] != c[i] {
                        Some((i, ChangeKind::Replaced))
                    } else {
                        None
                    }
                })
                .collect();
            if applied.is_err() || words_of(&out) != *c || listed != want_list {
                problems.push(format!("plan(lost={lost_variant}) listed {listed:?} applied {applied:?}"));
            }
        }
        (Ok(Plan::Agrees), Some(c)) => {
            if s != 0 || c.as_slice() != y {
                problems.push("plan Agrees".into());
            }
        }
        (Ok(Plan::Unresolved { columns: 1 }), None) => {}
        (got, _) => problems.push(format!("plan(lost={lost_variant}) {got:?}")),
    }
    // (c) CyclicCode::correct with the erased places as lost shards holding anything.
    let mut lost_vals = y.to_vec();
    for &i in &places {
        lost_vals[i] = placeholder;
    }
    let mut held = bytes_of(field, &lost_vals);
    // A placeholder of two bytes in a one-byte field cannot be: keep its low byte.
    let original = held.clone();
    let mut b: Vec<&mut [u8]> = held.iter_mut().map(Vec::as_mut_slice).collect();
    let got = code.correct(&mut b, &places);
    drop(b);
    match (got, want) {
        (Ok(Correction::Corrected(counts)), Some(c)) => {
            let expect: Vec<usize> = (0..n).map(|i| usize::from(erased[i] || y[i] != c[i])).collect();
            if words_of(&held) != *c || counts != expect {
                problems.push(format!("correct Corrected({counts:?})"));
            }
        }
        (Ok(Correction::Agrees), Some(c)) => {
            if s != 0 || c.as_slice() != y || held != original {
                problems.push("correct Agrees".into());
            }
            t.clean += 1;
        }
        (Ok(Correction::Unresolved { columns: 1 }), None) => {
            if held != original {
                problems.push("correct changed shards it gave up".into());
            }
            t.gave_up += 1;
        }
        (Err(CodeError::Symbol), _) => problems.push("correct refused a symbol".into()),
        (got, _) => problems.push(format!("correct {got:?}")),
    }
    if want.is_some() && !(want.unwrap().as_slice() == y && s == 0) {
        t.fixed += 1;
    }
    if !problems.is_empty() {
        t.bad += 1;
        if t.bad <= 8 {
            println!("  MISMATCH {label} y={y:?} erased={places:?} placeholder={placeholder:#x} expected={want:?}: {problems:?}");
        }
    }
}

fn main() {
    let mut rng = Rng(0x0123_4567_89AB_CDEF);
    // (degree, polynomial or None, data, parity, first, step)
    let mut shapes: Vec<(u8, usize, usize, u32, u32)> = Vec::new();
    for (data, parity) in [(1, 1), (1, 2), (2, 1)] {
        for first in [0, 1, 2, 3, 7] {
            for step in [1, 2] {
                shapes.push((2, data, parity, first, step));
            }
        }
    }
    for (data, parity) in [(1, 4), (2, 3), (3, 2), (1, 3), (2, 2), (3, 3), (2, 5), (4, 3), (1, 6)] {
        for (first, step) in [(0, 1), (1, 1), (5, 3), (6, 2), (100, 6), (3, 4), (u32::MAX, 5)] {
            shapes.push((3, data, parity, first, step));
        }
    }
    for (data, parity) in [(2, 3), (1, 4), (3, 2), (2, 2)] {
        for (first, step) in [(0, 1), (1, 1), (4, 3), (14, 7), (2, 5), (u32::MAX, 2)] {
            shapes.push((4, data, parity, first, step));
        }
    }
    let mut total = Tally::default();
    let mut shapes_run = 0;
    let mut refused = 0;
    for (m, data, parity, first, step) in shapes {
        let field = Field::binary(m, None).unwrap();
        let poly = match field {
            Field::Binary(b) => b.polynomial(),
            _ => unreachable!(),
        };
        let gf = Gf::new(u32::from(m), poly);
        let q = field.order();
        let elements = q - 1;
        let s = (step as usize) % elements;
        let beta_order = elements / gcd(s, elements);
        let code = match CyclicCode::new(field, data, parity, first, step) {
            Ok(code) => code,
            Err(e) => {
                let should = data + parity > beta_order;
                if !(should && e == CodeError::Shape) {
                    println!("  UNEXPECTED REFUSAL {m} {data} {parity} {first} {step}: {e:?}");
                    total.bad += 1;
                }
                refused += 1;
                continue;
            }
        };
        if data + parity > beta_order {
            println!("  ACCEPTED A WORD LONGER THAN THE ORDER OF BETA {m} {data} {parity} {first} {step}");
            total.bad += 1;
        }
        shapes_run += 1;
        let n = data + parity;
        // Every code word, by long division; the crate's parity must match for every data.
        let mut words = Vec::new();
        for idx in 0..q.pow(data as u32) {
            let mut d = Vec::new();
            let mut v = idx;
            for _ in 0..data {
                d.push((v % q) as u16);
                v /= q;
            }
            let (g, par) = cyclic_parity(&gf, &d, parity, u64::from(first), u64::from(step));
            if idx == 0 && code.generator() != g.as_slice() {
                println!("  GENERATOR {m} {data} {parity} {first} {step}: crate {:?} oracle {g:?}", code.generator());
                total.bad += 1;
            }
            let shards = bytes_of(field, &d);
            let views: Vec<&[u8]> = shards.iter().map(Vec::as_slice).collect();
            let made: Vec<u16> = code.encode(&views).unwrap().iter().map(|b| b.iter().fold(0u16, |x, &y| x << 8 | u16::from(y))).collect();
            if made != par {
                println!("  PARITY {m} {data} {parity} {first} {step} data {d:?}: crate {made:?} oracle {par:?}");
                total.bad += 1;
            }
            let mut c = d.clone();
            c.extend(par);
            words.push(c);
        }
        let all = q.pow(n as u32);
        let exhaustive = all <= 40_000;
        let rounds = if exhaustive { all } else { 40_000 };
        let label = format!("GF(2^{m}) d={data} p={parity} first={first} step={step}");
        let mut t = Tally::default();
        for round in 0..rounds {
            let y: Vec<u16> = if exhaustive {
                let mut v = round;
                (0..n).map(|_| { let x = (v % q) as u16; v /= q; x }).collect()
            } else {
                let mut y = words[rng.below(words.len())].clone();
                for _ in 0..rng.below(parity / 2 + 3) {
                    let i = rng.below(n);
                    y[i] = rng.below(q) as u16;
                }
                y
            };
            // No erasure, every single erasure set when small, and a random one.
            let mut sets: Vec<Vec<bool>> = vec![vec![false; n]];
            if n <= 5 && exhaustive && round % 3 == 0 {
                for sm in 1u32..(1 << n) {
                    sets.push((0..n).map(|i| sm >> i & 1 == 1).collect());
                }
            } else {
                sets.push((0..n).map(|_| rng.below(3) == 0).collect());
            }
            for erased in sets {
                let placeholder = match rng.below(3) {
                    0 => 0xFFFF,
                    1 => q as u16, // the first value outside the field
                    _ => rng.below(q) as u16,
                };
                // One-byte fields: a shard byte cannot hold 0xFFFF; correct() takes the bytes as given.
                let placeholder = if field.symbol_bytes() == 1 && placeholder > 0xFF { 0xFF } else { placeholder };
                check(&code, &words, &y, &erased, placeholder, &mut rng, &mut t, &label);
            }
        }
        if t.bad > 0 {
            println!("{label}: {t:?}");
        }
        total.cases += t.cases;
        total.fixed += t.fixed;
        total.clean += t.clean;
        total.gave_up += t.gave_up;
        total.bad += t.bad;
    }
    println!("cyclic shapes run {shapes_run}, refused {refused}: {total:?}");
    if total.bad > 0 {
        aud006_codes_probe::fail();
    }
    aud006_codes_probe::finish();
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}
