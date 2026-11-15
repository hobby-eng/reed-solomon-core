//! GF(2^m) for a degree `m` from 2 to 16 that a program chooses, with a primitive polynomial of
//! that degree: an element is a number below 2^m, bit `i` the coefficient of x^i.
//!
//! As in the other fields, a product is `m` steps of add, shift and reduce, each chosen by a mask
//! made from one bit, and an inverse the same chain of squarings and multiplications for every
//! element: no table is indexed by an element and no branch depends on one, except that the
//! inverse of zero is told apart.

use core::fmt;

/// The fewest and the most bits of an element.
pub(super) const DEGREES: core::ops::RangeInclusive<u8> = 2..=16;

/// For each degree from 2 to 16, the primitive polynomial taken when a program names none, its
/// x^m term included: the polynomials Reed–Solomon software commonly takes for these degrees,
/// three or five terms each. The tests check every one of them to be primitive, and so does
/// [`crate::Field::binary`] when a field is chosen.
pub(super) const STANDARD: [u32; 15] = [
    0x7, 0xB, 0x13, 0x25, 0x43, 0x89, 0x11D, 0x211, 0x409, 0x805, 0x1053, 0x201B, 0x4443, 0x8003,
    0x1100B,
];

/// A field GF(2^m) by a primitive polynomial: what [`crate::Field::binary`] makes when the
/// program chooses a field other than the three that have names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Binary {
    degree: u8,
    polynomial: u32,
}

impl Binary {
    /// The field, after the checks of [`crate::Field::binary`].
    pub(super) const fn checked(degree: u8, polynomial: u32) -> Self {
        Self { degree, polynomial }
    }

    /// `m`: the bits of an element.
    #[must_use]
    pub const fn degree(self) -> u8 {
        self.degree
    }

    /// The polynomial, its x^m term included: x^4 + x + 1 is 0x13.
    #[must_use]
    pub const fn polynomial(self) -> u32 {
        self.polynomial
    }

    /// The number of elements, 2^m.
    pub(super) const fn order(self) -> usize {
        1 << self.degree
    }

    /// The product of `a` and `b`, both below 2^m.
    pub(super) const fn multiply(self, a: u16, b: u16) -> u16 {
        let (mut a, b) = (a as u32, b as u32);
        let mut product = 0;
        let mut bit = 0;
        while bit < self.degree as u32 {
            product ^= a & mask(b >> bit);
            a <<= 1;
            a ^= self.polynomial & mask(a >> self.degree);
            bit += 1;
        }
        // The product is below 2^m, at most 2^16.
        product as u16
    }

    /// The inverse of a nonzero `a`; `None` for zero. It is `a^(2^m − 2)`: the product of `a^2`,
    /// `a^4`, …, `a^(2^(m−1))`, the same chain for every `a`.
    pub(super) const fn inverse(self, a: u16) -> Option<u16> {
        let mut square = a;
        let mut product = 1;
        let mut step = 1;
        while step < self.degree {
            square = self.multiply(square, square);
            product = self.multiply(product, square);
            step += 1;
        }
        if a == 0 { None } else { Some(product) }
    }
}

/// All ones when `bit` is 1, all zeros when it is 0.
const fn mask(bit: u32) -> u32 {
    0_u32.wrapping_sub(bit & 1)
}

/// Whether `polynomial` is a primitive polynomial of degree `degree`: x^degree is its highest
/// term, its constant term is 1, and x has the order 2^degree − 1 modulo it, so that the powers
/// of x are every nonzero element. That is at most 65,535 steps, done when a field is chosen.
pub(super) fn primitive(degree: u8, polynomial: u32) -> bool {
    if !DEGREES.contains(&degree) || polynomial >> degree != 1 || polynomial & 1 == 0 {
        return false;
    }
    let order = (1_u32 << degree) - 1;
    let mut power = 1_u32;
    for step in 1..=order {
        power <<= 1;
        if power >> degree & 1 == 1 {
            power ^= polynomial;
        }
        if power == 1 {
            return step == order;
        }
    }
    false
}

/// Why a field was not chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldError {
    /// A degree outside 2 to 16.
    Degree,
    /// A polynomial that is not a primitive polynomial of the degree: another highest term, no
    /// constant term, or a polynomial whose powers of x do not reach every nonzero element.
    Polynomial,
}

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Degree => "the degree of the field is not between 2 and 16",
            Self::Polynomial => "the polynomial is not a primitive polynomial of that degree",
        })
    }
}

impl core::error::Error for FieldError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_standard_polynomial_is_primitive_and_others_are_refused() {
        for (degree, &polynomial) in DEGREES.zip(&STANDARD) {
            assert!(primitive(degree, polynomial), "{degree} {polynomial:#x}");
        }
        // x^4 + x^3 + x^2 + x + 1 is irreducible, but x has the order 5, not 15.
        assert!(!primitive(4, 0x1F));
        // x^4 + x^2 + 1 = (x^2 + x + 1)^2.
        assert!(!primitive(4, 0x15));
        // No constant term, another degree, a degree out of range.
        assert!(!primitive(4, 0x12));
        assert!(!primitive(4, 0x25));
        assert!(!primitive(1, 0x3));
        assert!(!primitive(17, 0x2_0009));
    }
}
