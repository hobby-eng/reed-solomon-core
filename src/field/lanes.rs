//! `out ^= factor · source` over whole shards, eight bytes at a time, by shifts and masks.
//!
//! The factor follows from shard indexes and is public; the bytes of the shards may be anything,
//! a secret included. So the factor picks the masks, and the bytes only pass through `&`, `^`,
//! shifts and one multiplication by a constant: no branch and no table entry depends on them.

use super::Binary;
use super::gf256;
use super::gf65536::{CONSTANT, MIDDLE};

/// The bytes of a word that hold the high and the low byte of a two-byte symbol, the word read
/// big-endian.
const HIGH_LANES: u64 = 0xFF00_FF00_FF00_FF00;
const LOW_LANES: u64 = 0x00FF_00FF_00FF_00FF;
/// The low seven bits and the lowest bit of every byte.
const LOW_SEVEN: u64 = 0x7F7F_7F7F_7F7F_7F7F;
const LANE_ONES: u64 = 0x0101_0101_0101_0101;
/// The polynomial 0x11D without its x^8 term: what a byte's overflowing bit turns into.
const REDUCTION: u64 = 0x1D;

/// For each bit of a factor, the byte lanes whose factor has that bit.
type Masks = [u64; 8];

/// The masks of a word whose high lanes take `high_factor` and low lanes `low_factor`.
fn masks(high_factor: u8, low_factor: u8) -> Masks {
    let mut masks = [0; 8];
    for (bit, mask) in masks.iter_mut().enumerate() {
        if high_factor >> bit & 1 == 1 {
            *mask |= HIGH_LANES;
        }
        if low_factor >> bit & 1 == 1 {
            *mask |= LOW_LANES;
        }
    }
    masks
}

/// Every byte of `word` times two in GF(2^8).
const fn doubled(word: u64) -> u64 {
    ((word & LOW_SEVEN) << 1) ^ (((word >> 7) & LANE_ONES) * REDUCTION)
}

/// Every byte of `word` times its lane's factor: the sum of `word · 2^bit` over the factor's bits.
fn scaled(mut word: u64, masks: &Masks) -> u64 {
    let mut sum = 0;
    for mask in masks {
        sum ^= word & mask;
        word = doubled(word);
    }
    sum
}

/// Adds `product(word)` to `out` for each eight bytes `word` of `source`, read big-endian. The
/// last bytes are padded with zeros, which every product here leaves zero.
fn add_products(out: &mut [u8], source: &[u8], product: impl Fn(u64) -> u64) {
    let (out_words, out_rest) = out.as_chunks_mut::<8>();
    let (source_words, source_rest) = source.as_chunks::<8>();
    for (out, source) in out_words.iter_mut().zip(source_words) {
        *out = (u64::from_be_bytes(*out) ^ product(u64::from_be_bytes(*source))).to_be_bytes();
    }
    let mut last = [0; 8];
    for (padded, byte) in last.iter_mut().zip(source_rest) {
        *padded = *byte;
    }
    let last = product(u64::from_be_bytes(last)).to_be_bytes();
    for (out, byte) in out_rest.iter_mut().zip(last) {
        *out ^= byte;
    }
}

/// `out ^= factor · source` in GF(2^8), byte by byte.
pub(super) fn add_scaled_bytes(out: &mut [u8], source: &[u8], factor: u8) {
    match factor {
        0 => {}
        1 => add_products(out, source, |word| word),
        _ => {
            let masks = masks(factor, factor);
            add_products(out, source, |word| scaled(word, &masks));
        }
    }
}

/// The low eleven bits and the lowest bit of every two-byte lane of a word.
const LOW_ELEVEN: u64 = 0x07FF_07FF_07FF_07FF;
const PAIR_ONES: u64 = 0x0001_0001_0001_0001;
/// The polynomial 0x805 without its x^11 term: what a word symbol's overflowing bit turns into.
const WORD_REDUCTION: u64 = 0x005;

/// Every two-byte lane of `word`, a symbol below 2,048, times two in GF(2^11). The bit that
/// leaves the eleven comes back as 0x005 in its own lane: 5 never carries into the next one.
const fn doubled_pairs(word: u64) -> u64 {
    ((word << 1) & LOW_ELEVEN) ^ (((word >> 10) & PAIR_ONES) * WORD_REDUCTION)
}

/// `out ^= factor · source` in GF(2^11), two bytes per symbol, the high byte first, every
/// symbol below 2,048.
pub(super) fn add_scaled_words(out: &mut [u8], source: &[u8], factor: u16) {
    match factor {
        0 => {}
        1 => add_products(out, source, |word| word),
        _ => {
            // For each bit of the factor: every lane, or none.
            let mut masks = [0_u64; 11];
            for (bit, mask) in masks.iter_mut().enumerate() {
                if factor >> bit & 1 == 1 {
                    *mask = u64::MAX;
                }
            }
            add_products(out, source, |mut word| {
                let mut sum = 0;
                for mask in &masks {
                    sum ^= word & mask;
                    word = doubled_pairs(word);
                }
                sum
            });
        }
    }
}

/// `out ^= factor · source` in GF(2^m) by its polynomial: one byte per symbol up to `m` = 8,
/// two bytes above, every symbol below 2^m.
///
/// Every lane of a word is doubled at once: its low `m − 1` bits move up by one, and its bit
/// `m − 1`, brought down to the lowest bit of the lane, comes back times the polynomial without
/// its x^m term, a number below 2^m that stays in its lane.
pub(super) fn add_scaled_binary(out: &mut [u8], source: &[u8], factor: u16, field: Binary) {
    let degree = field.degree();
    let ones = if degree <= 8 { LANE_ONES } else { PAIR_ONES };
    let below_top = ones * ((1_u64 << (degree - 1)) - 1);
    let reduction = u64::from(field.polynomial()) & ((1_u64 << degree) - 1);
    let shift = u32::from(degree) - 1;
    match factor {
        0 => {}
        1 => add_products(out, source, |word| word),
        _ => {
            // For each bit of the factor: every lane, or none.
            let mut all = [0_u64; 16];
            for (bit, mask) in all.iter_mut().enumerate() {
                if factor >> bit & 1 == 1 {
                    *mask = u64::MAX;
                }
            }
            let masks = all.get(..usize::from(degree)).unwrap_or_default();
            add_products(out, source, |mut word| {
                let mut sum = 0;
                for mask in masks {
                    sum ^= word & mask;
                    word = ((word & below_top) << 1) ^ (((word >> shift) & ones) * reduction);
                }
                sum
            });
        }
    }
}

/// `out ^= factor · source` in GF(2^16), two bytes `[high, low]` per symbol.
///
/// With the factor `fh·x + fl`, the symbol `dh·x + dl` and x^2 = MIDDLE·x + CONSTANT:
/// the product's high byte is `dh·(fl + MIDDLE·fh) + dl·fh` and its low byte
/// `dh·(CONSTANT·fh) + dl·fl`. The first terms of each come from the word as it is, the second
/// from the word with the bytes of every symbol swapped.
pub(super) fn add_scaled_pairs(out: &mut [u8], source: &[u8], factor: u16) {
    let [fh, fl] = factor.to_be_bytes();
    match factor {
        0 => {}
        1 => add_products(out, source, |word| word),
        _ => {
            let straight = masks(fl ^ gf256::multiply(MIDDLE, fh), fl);
            let crossed = masks(fh, gf256::multiply(CONSTANT, fh));
            add_products(out, source, |word| {
                let swapped = ((word & HIGH_LANES) >> 8) | ((word & LOW_LANES) << 8);
                scaled(word, &straight) ^ scaled(swapped, &crossed)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_doubles_byte_by_byte() {
        let word = u64::from_be_bytes([0x00, 0x01, 0x7F, 0x80, 0xFF, 0x1D, 0x8E, 0x40]);
        let expected = [0x00, 0x02, 0xFE, 0x1D, 0xE3, 0x3A, 0x01, 0x80];
        assert_eq!(doubled(word).to_be_bytes(), expected);
        assert_eq!(HIGH_LANES | LOW_LANES, u64::MAX);
    }

    #[test]
    fn a_word_of_four_symbols_doubles_lane_by_lane() {
        // Four different symbols, the first lane included: 0x400 overflows to 0x005, 0x7FF to
        // 0x7FB, and no lane reaches its neighbour.
        let word = u64::from_be_bytes([0x04, 0x00, 0x07, 0xFF, 0x00, 0x01, 0x03, 0xFF]);
        let expected = [0x00, 0x05, 0x07, 0xFB, 0x00, 0x02, 0x07, 0xFE];
        assert_eq!(doubled_pairs(word).to_be_bytes(), expected);
    }
}
