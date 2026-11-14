//! GF(2^8) with the polynomial x^8 + x^4 + x^3 + x^2 + 1 (0x11D) and the generator 2.
//!
//! The tables serve elements that follow from shard indexes or from the differences between
//! shards (syndromes); the bytes of a shard go through `lanes`, which uses no table.

/// The reduction polynomial, its x^8 term included.
const POLYNOMIAL: u16 = 0x11D;
/// The order of the multiplicative group.
const GROUP: usize = 255;

/// `EXP[i]` is 2^i for i in 0..510, twice round the group so that the sum of two logarithms needs
/// no reduction; `LOG[a]` is the logarithm of a nonzero `a`.
const TABLES: ([u8; 2 * GROUP], [u8; 256]) = tables();

#[expect(
    clippy::indexing_slicing,
    reason = "i stays below 255 and value below 256, the sizes of the tables"
)]
const fn tables() -> ([u8; 2 * GROUP], [u8; 256]) {
    let mut exp = [0_u8; 2 * GROUP];
    let mut log = [0_u8; 256];
    let mut value: u16 = 1;
    let mut i = 0;
    while i < GROUP {
        let [_, byte] = value.to_be_bytes();
        exp[i] = byte;
        exp[i + GROUP] = byte;
        // i is below 255.
        log[value as usize] = i as u8;
        value <<= 1;
        if value & 0x100 != 0 {
            value ^= POLYNOMIAL;
        }
        i += 1;
    }
    (exp, log)
}

/// The product of `a` and `b`.
#[expect(
    clippy::indexing_slicing,
    reason = "a logarithm is below 255, so the sum of two is below 510, the table's size"
)]
pub(super) const fn multiply(a: u8, b: u8) -> u8 {
    if a == 0 || b == 0 {
        return 0;
    }
    let (exp, log) = (&TABLES.0, &TABLES.1);
    exp[log[a as usize] as usize + log[b as usize] as usize]
}

/// The inverse of a nonzero `a`; `None` for zero.
#[expect(
    clippy::indexing_slicing,
    reason = "a logarithm is below 255, so 255 minus it is a valid exponent"
)]
pub(super) const fn inverse(a: u8) -> Option<u8> {
    if a == 0 {
        return None;
    }
    let (exp, log) = (&TABLES.0, &TABLES.1);
    Some(exp[GROUP - log[a as usize] as usize])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The product by shifts and reduction, without the tables.
    fn slow(a: u8, b: u8) -> u8 {
        let (mut a, mut product) = (u16::from(a), 0_u16);
        for bit in 0..8 {
            if b >> bit & 1 == 1 {
                product ^= a;
            }
            a <<= 1;
            if a & 0x100 != 0 {
                a ^= POLYNOMIAL;
            }
        }
        product as u8
    }

    #[test]
    fn the_tables_multiply_as_shift_and_reduce_does() {
        // 2 · 0x80 = x^8 = x^4 + x^3 + x^2 + 1 = 0x1D under 0x11D.
        assert_eq!(multiply(2, 0x80), 0x1D);
        for a in 0..=255 {
            for b in 0..=255 {
                assert_eq!(multiply(a, b), slow(a, b), "{a} · {b}");
            }
        }
    }

    #[test]
    fn every_nonzero_element_has_its_inverse() {
        assert_eq!(inverse(0), None);
        for a in 1..=255 {
            let inverse = inverse(a).unwrap();
            assert_eq!(slow(a, inverse), 1, "{a}");
        }
    }
}
