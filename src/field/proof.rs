//! A proof by exhaustion that the arithmetic of each field, as implemented, is a field. The
//! product is checked on every pair of elements: it is linear over GF(2) in each factor,
//! commutative, closed, and associative on every triple of basis elements, so on every triple;
//! 1 is its unit and every nonzero element has an inverse. The kernel that scales whole shards
//! gives the same products. Then any `k` values fix a polynomial of degree below `k`.

use super::Field;
use std::format;
use std::string::String;
use std::vec;
use std::vec::Vec;

/// The first law of a field that `field` breaks, or `None`.
fn broken_law(field: Field) -> Option<String> {
    let order = u32::try_from(field.order()).ok()?;
    let bits = order.trailing_zeros();
    let basis: Vec<u16> = (0..bits).map(|bit| 1 << bit).collect();
    let width = field.symbol_bytes();
    // Every element of the field as a symbol of a shard, in order.
    let all: Vec<u8> = (0..order)
        .flat_map(|element| (element as u16).to_be_bytes().into_iter().skip(2 - width))
        .collect();
    let threads = std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).clamp(1, 4));
    let chunk = order.div_ceil(threads as u32);
    // Linear in the second factor, closed, commutative and scaled alike: every pair, a thread
    // per range of a. A thread that panics passes its panic on.
    let pairs = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads as u32)
            .map(|part| {
                let (basis, all) = (&basis, &all);
                scope.spawn(move || {
                    let mut scaled = vec![0; all.len()];
                    let start = part * chunk;
                    let end = (start + chunk).min(order);
                    (start..end).find_map(|a| pair_law(field, a as u16, basis, all, &mut scaled))
                })
            })
            .collect();
        let answers = handles.into_iter().map(|handle| {
            handle
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        });
        answers.collect::<Vec<_>>().into_iter().flatten().next()
    });
    if pairs.is_some() {
        return pairs;
    }
    // Associative on the basis, so on every triple; 1 the unit; every nonzero element inverted.
    for &x in &basis {
        for &y in &basis {
            for &z in &basis {
                let left = field.multiply(field.multiply(x, y), z);
                if left != field.multiply(x, field.multiply(y, z)) {
                    return Some(format!("(x·y)·z ≠ x·(y·z) for {x:#x}, {y:#x}, {z:#x}"));
                }
            }
        }
    }
    let all = || (0..order).map(|element| element as u16);
    if let Some(a) = all().find(|&a| field.multiply(1, a) != a) {
        return Some(format!("1·a ≠ a for {a:#x}"));
    }
    if field.inverse(0).is_some() {
        return Some("zero has an inverse".into());
    }
    let inverted = |a: u16| {
        let inverse = field.inverse(a);
        inverse.is_some_and(|i| u32::from(i) < order && field.multiply(a, i) == 1)
    };
    all()
        .skip(1)
        .find(|&a| !inverted(a))
        .map(|a| format!("no inverse of {a:#x}"))
}

/// The laws on the pairs (a, b) for every b: a·b lies in the field, equals b·a, is the sum of
/// a·e over the basis elements e of b, which b runs through in Gray code, and is what the kernel
/// makes of the symbol b in a shard of `all` elements scaled by a. `scaled` is room for that.
fn pair_law(field: Field, a: u16, basis: &[u16], all: &[u8], scaled: &mut [u8]) -> Option<String> {
    let width = field.symbol_bytes();
    let order = all.len() / width;
    let row: Vec<u16> = basis.iter().map(|&e| field.multiply(a, e)).collect();
    scaled.fill(0);
    field.add_scaled(scaled, all, a);
    let mut sum = 0;
    for step in 0..order {
        let b = (step ^ (step >> 1)) as u16;
        // From one b to the next only the bit at the lowest set bit of `step` changes.
        if step > 0 {
            sum ^= row
                .get(step.trailing_zeros() as usize)
                .copied()
                .unwrap_or(0);
        }
        let product = field.multiply(a, b);
        if product != sum || usize::from(product) >= order {
            return Some(format!("a·b is not linear in b for {a:#x}, {b:#x}"));
        }
        if product != field.multiply(b, a) {
            return Some(format!("a·b ≠ b·a for {a:#x}, {b:#x}"));
        }
        let at = usize::from(b) * width;
        let kernel = scaled.get(at..at + width).unwrap_or_default();
        if kernel
            .iter()
            .fold(0, |value, &byte| value << 8 | u16::from(byte))
            != product
        {
            return Some(format!("the kernel scales {b:#x} by {a:#x} otherwise"));
        }
    }
    None
}

/// The named fields and the field of the standard polynomial of each degree in `degrees`.
fn fields(degrees: core::ops::RangeInclusive<u8>, named: &[Field]) -> Vec<Field> {
    let standard = degrees.map(|degree| Field::binary(degree, None).unwrap());
    named.iter().copied().chain(standard).collect()
}

#[test]
fn fields_of_up_to_4096_elements_are_fields() {
    let other = Field::binary(8, Some(0x187)).unwrap();
    for field in fields(2..=12, &[Field::Gf256, Field::Gf2048, other]) {
        assert_eq!(broken_law(field), None, "{field:?}");
    }
}

#[test]
#[ignore = "every pair of up to 65,536 elements: minutes in a release build"]
fn fields_of_8192_to_65536_elements_are_fields() {
    let other = Field::binary(16, Some(0x1002D)).unwrap();
    for field in fields(13..=16, &[Field::Gf65536, other]) {
        assert_eq!(broken_law(field), None, "{field:?}");
    }
}
