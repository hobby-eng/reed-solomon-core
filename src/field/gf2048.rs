//! GF(2^11) with the polynomial 0x805, one symbol per word of a 2,048-word list. No tables and no
//! branches on an element, since an element can be a word of a recovery phrase; only zero is told
//! apart, as having no inverse.

/// The reduction polynomial, its x^11 term included.
const POLYNOMIAL: u16 = 0x805;
/// The number of elements, and the bit a doubled element overflows into.
pub(super) const ORDER: u16 = 0x800;

/// All ones when `bit` is 1, all zeros when it is 0.
const fn mask(bit: u16) -> u16 {
    0_u16.wrapping_sub(bit & 1)
}

/// The product of `a` and `b`, both below 2,048.
pub(super) const fn multiply(a: u16, b: u16) -> u16 {
    let (mut a, mut product) = (a, 0);
    let mut bit = 0;
    while bit < 11 {
        product ^= a & mask(b >> bit);
        a <<= 1;
        a ^= POLYNOMIAL & mask(a >> 11);
        bit += 1;
    }
    product
}

/// The inverse of a nonzero `a`; `None` for zero.
///
/// The nonzero elements form a group of 2,047, so the inverse is the 2,046th power, and 2,046 is
/// 2 + 4 + … + 1,024: the product of the first ten repeated squares, the same chain for every
/// `a`.
pub(super) const fn inverse(a: u16) -> Option<u16> {
    let mut square = multiply(a, a);
    let mut product = square;
    let mut step = 2;
    while step <= 10 {
        square = multiply(square, square);
        product = multiply(product, square);
        step += 1;
    }
    if a == 0 { None } else { Some(product) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The product as a polynomial product over GF(2) followed by long division by 0x805.
    fn slow(a: u16, b: u16) -> u16 {
        let mut wide: u32 = 0;
        for bit in 0..11 {
            if b >> bit & 1 == 1 {
                wide ^= u32::from(a) << bit;
            }
        }
        for bit in (11..=20).rev() {
            if wide >> bit & 1 == 1 {
                wide ^= u32::from(POLYNOMIAL) << (bit - 11);
            }
        }
        wide as u16
    }

    fn power(base: u16, exponent: u32) -> u16 {
        (0..exponent).fold(1, |power, _| multiply(power, base))
    }

    #[test]
    fn products_and_an_inverse_equal_those_computed_apart_from_this_crate() {
        // x^11 = x^2 + 1.
        assert_eq!(multiply(0x400, 0x002), 0x005);
        assert_eq!(multiply(0x7FF, 0x7FF), 0x552);
        assert_eq!(multiply(0x123, 0x456), 0x2C8);
        assert_eq!(inverse(0x123), Some(0x0A5));
    }

    #[test]
    fn the_products_are_those_of_polynomial_division() {
        for a in (0..2048).step_by(7).chain([1, 2, 0x400, 0x7FF]) {
            for b in 0..2048 {
                assert_eq!(multiply(a, b), slow(a, b), "{a:#x} · {b:#x}");
            }
        }
    }

    #[test]
    fn the_element_two_runs_through_every_nonzero_element() {
        // 2,047 = 23 · 89: the order of 2 divides neither, so it is 2,047 itself.
        assert_eq!(power(2, 23), 0x022);
        assert_eq!(power(2, 89), 0x142);
        assert_eq!(power(2, 2047), 1);
        let mut seen = [false; 2048];
        let mut value = 1;
        for _ in 0..2047 {
            assert!(!seen[usize::from(value)], "{value:#x} twice");
            seen[usize::from(value)] = true;
            value = multiply(value, 2);
        }
    }

    #[test]
    fn every_nonzero_element_has_its_inverse() {
        assert_eq!(inverse(0), None);
        for a in 1..2048 {
            let inverse = inverse(a).unwrap();
            assert!(inverse < ORDER);
            assert_eq!(slow(a, inverse), 1, "{a:#x}");
        }
    }
}
