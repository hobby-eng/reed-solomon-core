//! GF(2^8) with the polynomial 0x11D. No tables and no branches on an element, because errors and
//! gaps can follow from the data; only zero is told apart, as having no inverse. Whole shards go
//! through `lanes`.

/// The reduction polynomial, its x^8 term included.
const POLYNOMIAL: u16 = 0x11D;

/// All ones when `bit` is 1, all zeros when it is 0.
const fn mask(bit: u16) -> u16 {
    0_u16.wrapping_sub(bit & 1)
}

/// The product of `a` and `b`.
pub(super) const fn multiply(a: u8, b: u8) -> u8 {
    let (mut a, b) = (a as u16, b as u16);
    let mut product = 0;
    let mut bit = 0;
    while bit < 8 {
        product ^= a & mask(b >> bit);
        a <<= 1;
        a ^= POLYNOMIAL & mask(a >> 8);
        bit += 1;
    }
    let [_, low] = product.to_be_bytes();
    low
}

/// The inverse of a nonzero `a`; `None` for zero. It is `a^254`: the product of `a^2`, `a^4`, …,
/// `a^128`, the same seven squarings and multiplications for every `a`.
pub(super) const fn inverse(a: u8) -> Option<u8> {
    let mut square = a;
    let mut product = 1;
    let mut step = 1;
    while step < 8 {
        square = multiply(square, square);
        product = multiply(product, square);
        step += 1;
    }
    if a == 0 { None } else { Some(product) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables of logarithms and powers of 2, built apart from the code above: the other way
    /// to multiply, which the code is held against.
    fn tables() -> ([u8; 510], [u8; 256]) {
        let (mut exp, mut log) = ([0_u8; 510], [0_u8; 256]);
        let mut value: u16 = 1;
        for i in 0..255 {
            exp[i] = value as u8;
            exp[i + 255] = value as u8;
            log[usize::from(value)] = i as u8;
            value <<= 1;
            if value & 0x100 != 0 {
                value ^= POLYNOMIAL;
            }
        }
        (exp, log)
    }

    #[test]
    fn products_are_those_of_the_tables_of_logarithms() {
        // 2 · 0x80 = x^8 = x^4 + x^3 + x^2 + 1 = 0x1D under 0x11D.
        assert_eq!(multiply(2, 0x80), 0x1D);
        let (exp, log) = tables();
        for a in 0..=255_u8 {
            for b in 0..=255_u8 {
                let expected = if a == 0 || b == 0 {
                    0
                } else {
                    exp[usize::from(log[usize::from(a)]) + usize::from(log[usize::from(b)])]
                };
                assert_eq!(multiply(a, b), expected, "{a} · {b}");
            }
        }
    }

    #[test]
    fn every_nonzero_element_has_its_inverse() {
        assert_eq!(inverse(0), None);
        let (exp, log) = tables();
        for a in 1..=255_u8 {
            let inverse = inverse(a).unwrap();
            assert_eq!(multiply(a, inverse), 1, "{a}");
            assert_eq!(inverse, exp[255 - usize::from(log[usize::from(a)])]);
        }
    }
}
