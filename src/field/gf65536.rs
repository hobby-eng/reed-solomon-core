//! GF(2^16) as the extension GF(2^8)\[x\] / (x^2 + 2x + 128): the element `high·x + low` is the
//! two bytes `[high, low]`, here a `u16` with `high` as its upper byte. It is the field of
//! `reed-solomon-erasure`'s `galois_16`.

use super::gf256;

/// The modulus is x^2 + MIDDLE·x + CONSTANT, so x^2 = MIDDLE·x + CONSTANT.
pub(super) const MIDDLE: u8 = 2;
pub(super) const CONSTANT: u8 = 128;

/// The product of `a` and `b`.
pub(super) const fn multiply(a: u16, b: u16) -> u16 {
    let ([ah, al], [bh, bl]) = (a.to_be_bytes(), b.to_be_bytes());
    // The x^2 coefficient, folded back by the modulus.
    let top = gf256::multiply(ah, bh);
    let high = gf256::multiply(ah, bl) ^ gf256::multiply(al, bh) ^ gf256::multiply(MIDDLE, top);
    let low = gf256::multiply(al, bl) ^ gf256::multiply(CONSTANT, top);
    u16::from_be_bytes([high, low])
}

/// The inverse of a nonzero `a`; `None` for zero.
///
/// The other root of the modulus is `x + MIDDLE`, so the conjugate of `high·x + low` is
/// `high·x + (MIDDLE·high + low)`, and their product, the norm
/// `low^2 + MIDDLE·high·low + CONSTANT·high^2`, lies in GF(2^8): the inverse is the conjugate
/// divided by the norm.
pub(super) const fn inverse(a: u16) -> Option<u16> {
    let [high, low] = a.to_be_bytes();
    let norm = gf256::multiply(low, low)
        ^ gf256::multiply(MIDDLE, gf256::multiply(high, low))
        ^ gf256::multiply(CONSTANT, gf256::multiply(high, high));
    match gf256::inverse(norm) {
        Some(scale) => Some(u16::from_be_bytes([
            gf256::multiply(high, scale),
            gf256::multiply(gf256::multiply(MIDDLE, high) ^ low, scale),
        ])),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_times_x_is_the_modulus_folded_back() {
        // [01,00]·[01,00] = x^2 = 2x + 128 = [02,80].
        assert_eq!(multiply(0x0100, 0x0100), 0x0280);
        // The subfield multiplies as GF(2^8) does: 2 · 0x80 = 0x1D.
        assert_eq!(multiply(0x0002, 0x0080), 0x001D);
        assert_eq!(multiply(0x1234, 1), 0x1234);
        assert_eq!(multiply(0x1234, 0), 0);
    }

    #[test]
    fn every_nonzero_element_has_its_inverse() {
        // So the modulus is irreducible: a reducible one would give zero divisors.
        assert_eq!(inverse(0), None);
        for a in 1..=u16::MAX {
            let inverse = inverse(a).unwrap();
            assert_eq!(multiply(a, inverse), 1, "{a:#06x}");
        }
    }

    #[test]
    fn products_equal_those_of_reed_solomon_erasure() {
        use reed_solomon_erasure::Field;
        use reed_solomon_erasure::galois_16::Field as Theirs;
        // Pairs from a fixed generator (SplitMix64), and the corners.
        let mut state = 7_u64;
        let mut next = || {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            (z ^ (z >> 31)) as u16
        };
        let mut pairs: Vec<(u16, u16)> = (0..200_000).map(|_| (next(), next())).collect();
        pairs.extend([
            (0xFFFF, 0xFFFF),
            (0x0100, 0x0100),
            (0x00FF, 0xFF00),
            (1, 0xFFFF),
        ]);
        for (a, b) in pairs {
            let theirs = Theirs::mul(a.to_be_bytes(), b.to_be_bytes());
            assert_eq!(multiply(a, b).to_be_bytes(), theirs, "{a:#06x} · {b:#06x}");
        }
    }

    #[test]
    fn products_distribute_and_associate() {
        let samples = [
            1, 2, 0x80, 0x0100, 0x0280, 0x8001, 0xBEEF, 0xFFFF, 0x1234, 0x00FF,
        ];
        for a in samples {
            for b in samples {
                assert_eq!(multiply(a, b), multiply(b, a));
                for c in samples {
                    assert_eq!(multiply(a, b ^ c), multiply(a, b) ^ multiply(a, c));
                    assert_eq!(multiply(multiply(a, b), c), multiply(a, multiply(b, c)));
                }
            }
        }
    }
}
