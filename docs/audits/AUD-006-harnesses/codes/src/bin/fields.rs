//! Field arithmetic through the public API, against tables of logarithms written apart:
//! primitivity check over every candidate polynomial of every degree, every product of the
//! fields up to 2^13 (sampled above), inverses, and the symbol encodings.

use aud006_codes_probe::gf::{Gf, is_primitive, slow_mul};
use reed_solomon_core::{Code, CodeError, Field, FieldError};

fn symbols(field: Field, values: impl Iterator<Item = u16>) -> Vec<u8> {
    let w = field.symbol_bytes();
    values.flat_map(|v| v.to_be_bytes().into_iter().skip(2 - w)).collect()
}
fn values(field: Field, bytes: &[u8]) -> Vec<u16> {
    let w = field.symbol_bytes();
    bytes.chunks(w).map(|c| c.iter().fold(0u16, |v, &b| v << 8 | u16::from(b))).collect()
}

/// Every product a·b for all a, for the b given, through Code::encode with k = 2: the data
/// [all elements] at point 0 and zeros at point 1 make a·(t + 1) at point t.
fn check_products(field: Field, gf: &Gf, bs: &[u16]) -> usize {
    let q = field.order();
    let code = Code::new(field, 2).unwrap();
    let all = symbols(field, (0..q).map(|a| a as u16));
    let zeros = vec![0u8; all.len()];
    let mut bad = 0;
    for chunk in bs.chunks(64) {
        let targets: Vec<u16> = chunk.iter().map(|&b| b ^ 1).collect();
        let made = code.encode(&[&all, &zeros], &targets).unwrap();
        for (&b, shard) in chunk.iter().zip(&made) {
            let got = values(field, shard);
            for a in 0..q {
                let want = gf.mul(a as u16, b);
                if got[a] != want {
                    if bad < 5 {
                        println!("  PRODUCT MISMATCH {field:?} {a:#x}*{b:#x}: crate {:#x} oracle {want:#x}", got[a]);
                    }
                    bad += 1;
                }
            }
        }
    }
    bad
}

/// Inverses: sources at points 0 (value 0) and c (value 1) make t/c at point t; t = 1 gives 1/c.
fn check_inverses(field: Field, gf: &Gf, cs: &[u16]) -> usize {
    let code = Code::new(field, 2).unwrap();
    let w = field.symbol_bytes();
    let zero = vec![0u8; w];
    let one = symbols(field, std::iter::once(1));
    let mut bad = 0;
    for &c in cs {
        if c == 0 {
            continue;
        }
        let got = if c == 1 {
            1
        } else {
            let made = code.rebuild(&[(0, &zero), (c, &one)], &[1]).unwrap();
            values(field, &made[0])[0]
        };
        if got != gf.inv(c) {
            if bad < 5 {
                println!("  INVERSE MISMATCH {field:?} 1/{c:#x}: crate {got:#x} oracle {:#x}", gf.inv(c));
            }
            bad += 1;
        }
    }
    bad
}

fn main() {
    // 1. Primitivity: every candidate of every degree, against an independent test.
    let mut accepted_total = 0;
    let mut disagreements = 0;
    for m in 2u8..=16 {
        let mut accepted = 0;
        for low in 0u32..(1 << m) {
            let poly = (1u32 << m) | low;
            let ours = Field::binary(m, Some(poly));
            let theirs = is_primitive(u32::from(m), poly);
            match (&ours, theirs) {
                (Ok(field), true) => {
                    accepted += 1;
                    let named = match field {
                        Field::Gf256 => m == 8 && poly == 0x11D,
                        Field::Gf2048 => m == 11 && poly == 0x805,
                        Field::Binary(b) => b.degree() == m && b.polynomial() == poly && !((m, poly) == (8, 0x11D) || (m, poly) == (11, 0x805)),
                        Field::Gf65536 => false,
                    };
                    if !named || field.order() != 1 << m || field.symbol_bytes() != if m <= 8 { 1 } else { 2 } {
                        println!("  SHAPE {m} {poly:#x}: {ours:?}");
                        disagreements += 1;
                    }
                }
                (Err(FieldError::Polynomial), false) => {}
                _ => {
                    println!("  PRIMITIVE DISAGREE m={m} poly={poly:#x}: crate {ours:?} independent {theirs}");
                    disagreements += 1;
                }
            }
        }
        // Known counts of primitive polynomials: phi(2^m - 1) / m.
        let n = (1u64 << m) - 1;
        let phi = (1..=n).filter(|&i| gcd(i, n) == 1).count() as u64;
        println!("degree {m}: {accepted} accepted, phi(2^m-1)/m = {}", phi / u64::from(m));
        accepted_total += accepted;
        // Wrong degree: x^(m+1) term, a polynomial of lower degree, no constant term, high bits.
        for poly in [(1u32 << (m + 1)) | 0x3, (1u32 << (m - 1)) | 1, 1u32 << m, u32::MAX, (1u32 << m) | (1 << 20)] {
            if Field::binary(m, Some(poly)) != Err(FieldError::Polynomial) {
                println!("  WRONG-DEGREE ACCEPTED m={m} {poly:#x}: {:?}", Field::binary(m, Some(poly)));
                disagreements += 1;
            }
        }
    }
    for m in [0u8, 1, 17, 32, 255] {
        assert_eq!(Field::binary(m, None), Err(FieldError::Degree), "{m}");
        assert_eq!(Field::binary(m, Some(0x7)), Err(FieldError::Degree), "{m}");
    }
    println!("primitivity: {accepted_total} accepted in all, {disagreements} disagreements");

    // 2. Products and inverses, every pair up to 2^13 elements, sampled above.
    let mut fields: Vec<(Field, Gf)> = Vec::new();
    for m in 2u8..=16 {
        let field = Field::binary(m, None).unwrap();
        let poly = match field {
            Field::Gf256 => 0x11D,
            Field::Gf2048 => 0x805,
            Field::Binary(b) => b.polynomial(),
            Field::Gf65536 => unreachable!(),
        };
        fields.push((field, Gf::new(u32::from(m), poly)));
    }
    // Some other primitive polynomials: the largest of each degree up to 16.
    for m in 2u8..=16 {
        let poly = ((1u32 << m)..(2u32 << m)).rev().find(|&p| is_primitive(u32::from(m), p)).unwrap();
        fields.push((Field::binary(m, Some(poly)).unwrap(), Gf::new(u32::from(m), poly)));
    }
    let mut state = 0x1234_5678_9ABC_DEF0_u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for (field, gf) in &fields {
        let q = field.order();
        let bs: Vec<u16> = if q <= 1 << 13 {
            (0..q).map(|b| b as u16).collect()
        } else {
            let mut v: Vec<u16> = vec![0, 1, 2, 3, (q - 1) as u16, (q / 2) as u16, (q / 2 + 1) as u16];
            v.extend((0..300).map(|_| (next() % q as u64) as u16));
            v
        };
        let bad_products = check_products(*field, gf, &bs);
        let cs: Vec<u16> = if q <= 1 << 16 { (0..q).map(|c| c as u16).collect() } else { unreachable!() };
        let bad_inverses = check_inverses(*field, gf, &cs);
        // A third route for a few products: carry-less product and long division.
        let mut bad_slow = 0;
        for _ in 0..2000 {
            let (a, b) = ((next() % q as u64) as u16, (next() % q as u64) as u16);
            if slow_mul(gf.m, gf.poly, a, b) != gf.mul(a, b) {
                bad_slow += 1;
            }
        }
        println!(
            "{field:?}: products checked for {} factors x {q}: {bad_products} wrong; inverses of all {q}: {bad_inverses} wrong; oracle self-check {bad_slow}",
            bs.len()
        );
    }

    // 3. GF(2^16) as the extension of GF(2^8): [high, low] = high·x + low, x^2 = 2x + 128.
    let g8 = Gf::new(8, 0x11D);
    let ext = |a: u16, b: u16| -> u16 {
        let (ah, al, bh, bl) = ((a >> 8) as u16, a & 0xFF, (b >> 8) as u16, b & 0xFF);
        let top = g8.mul(ah, bh);
        let high = g8.mul(ah, bl) ^ g8.mul(al, bh) ^ g8.mul(2, top);
        let low = g8.mul(al, bl) ^ g8.mul(128, top);
        (high << 8) | low
    };
    let code = Code::new(Field::Gf65536, 2).unwrap();
    let all: Vec<u8> = (0..=u16::MAX).flat_map(u16::to_be_bytes).collect();
    let zeros = vec![0u8; all.len()];
    let mut bad = 0;
    let mut bs: Vec<u16> = vec![0, 1, 2, 0x80, 0x100, 0x101, 0x280, 0x8000, 0xFFFF, 0x00FF, 0xFF00];
    bs.extend((0..200).map(|_| next() as u16));
    for chunk in bs.chunks(32) {
        let targets: Vec<u16> = chunk.iter().map(|&b| b ^ 1).collect();
        let made = code.encode(&[&all, &zeros], &targets).unwrap();
        for (&b, shard) in chunk.iter().zip(&made) {
            for (a, pair) in (0..=u16::MAX).zip(shard.as_chunks::<2>().0) {
                if u16::from_be_bytes(*pair) != ext(a, b) {
                    bad += 1;
                }
            }
        }
    }
    println!("Gf65536: {} factors x 65536 products against the extension formula: {bad} wrong", bs.len());

    // 4. Symbol encodings: every two-byte value in GF(2^11), every value in each binary field.
    let code = Code::new(Field::Gf2048, 2).unwrap();
    let mut wrongly = 0;
    for v in 0..=u16::MAX {
        let s = v.to_be_bytes();
        let r = code.encode(&[&s, &[0, 0]], &[2]);
        let ok = v < 2048;
        if r.is_ok() != ok || (!ok && r != Err(CodeError::Symbol)) {
            wrongly += 1;
        }
    }
    println!("Gf2048 encode over all 65536 two-byte values: {wrongly} wrongly answered (Ok iff value < 2048, else Symbol)");
    for m in 2u8..=16 {
        let field = Field::binary(m, None).unwrap();
        let code = Code::new(field, 2).unwrap();
        let w = field.symbol_bytes();
        let mut wrongly = 0;
        let top: u32 = if w == 1 { 256 } else { 65536 };
        for v in 0..top {
            let s = (v as u16).to_be_bytes();
            let s = &s[2 - w..];
            let zero = vec![0u8; w];
            let r = code.encode(&[s, &zero], &[2]);
            let ok = v < (1 << m);
            if r.is_ok() != ok || (!ok && r != Err(CodeError::Symbol)) {
                wrongly += 1;
            }
        }
        // Odd length in a two-byte field, empty shards.
        let odd = if w == 2 { code.encode(&[&[0u8], &[0u8]], &[2]) == Err(CodeError::Length) } else { true };
        let empty = code.encode(&[&[], &[]], &[2, 3]).map(|v| v.iter().all(|b| b.is_empty()) && v.len() == 2) == Ok(true);
        println!("degree {m}: {wrongly} values wrongly answered over {top}; odd length refused {odd}; empty shards {empty}");
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}
