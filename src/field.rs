//! The fields a code can work in, their element arithmetic and the scaling of whole shards.

mod gf2048;
mod gf256;
mod gf65536;
mod lanes;

/// The field a code works in. Its elements are the symbols of every shard and the points of the
/// shard indexes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    /// GF(2^8) with the polynomial x^8 + x^4 + x^3 + x^2 + 1 (0x11D): one byte per symbol and up
    /// to 256 shards in a set. The bytes are those of the Backblaze, klauspost and
    /// `reed-solomon-erasure` codes.
    Gf256,
    /// GF(2^16) as the extension GF(2^8)\[x\] / (x^2 + 2x + 128) of the field above: two bytes
    /// `[high, low]` per symbol, standing for `high·x + low`, and up to 65,536 shards in a set.
    /// The bytes are those of `reed-solomon-erasure`'s `galois_16`.
    Gf65536,
    /// GF(2^11) with the polynomial x^11 + x^2 + 1 (0x805): a symbol is a number below 2,048 in
    /// two bytes, the high byte first, and a set has up to 2,048 shards. One symbol is one word
    /// of a list of 2,048 words, so that the shards a code adds are words of the list too. Bytes
    /// that hold a larger number are refused with [`crate::CodeError::Symbol`].
    Gf2048,
}

impl Field {
    /// The number of elements: the most shards a set can have, with the indexes 0 to
    /// `order() - 1`.
    #[must_use]
    pub const fn order(self) -> usize {
        match self {
            Self::Gf256 => 256,
            Self::Gf65536 => 65_536,
            Self::Gf2048 => 2048,
        }
    }

    /// Bytes of one symbol. Every shard is a whole number of symbols long.
    #[must_use]
    pub const fn symbol_bytes(self) -> usize {
        match self {
            Self::Gf256 => 1,
            Self::Gf65536 | Self::Gf2048 => 2,
        }
    }

    /// Whether every symbol of `bytes`, a whole number of symbols, is an element of the field.
    /// The two fields whose symbols fill their bytes take any bytes; a word is below 2,048.
    /// Every symbol is looked at, with no early exit.
    pub(crate) fn canonical(self, bytes: &[u8]) -> bool {
        match self {
            Self::Gf256 | Self::Gf65536 => true,
            Self::Gf2048 => {
                let high = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .fold(0, |high, [first, _]| high | first);
                high & 0xF8 == 0
            }
        }
    }

    /// The point of shard `index`: the index itself as an element. `None` when the field has no
    /// such element.
    pub(crate) const fn point(self, index: u16) -> Option<u16> {
        match self {
            Self::Gf256 if index > 0xFF => None,
            Self::Gf2048 if index >= gf2048::ORDER => None,
            Self::Gf256 | Self::Gf65536 | Self::Gf2048 => Some(index),
        }
    }

    /// The product of two elements.
    pub(crate) const fn multiply(self, a: u16, b: u16) -> u16 {
        match self {
            Self::Gf256 => {
                let ([_, a], [_, b]) = (a.to_be_bytes(), b.to_be_bytes());
                u16::from_be_bytes([0, gf256::multiply(a, b)])
            }
            Self::Gf65536 => gf65536::multiply(a, b),
            Self::Gf2048 => gf2048::multiply(a, b),
        }
    }

    /// The inverse of a nonzero element; `None` for zero.
    pub(crate) const fn inverse(self, a: u16) -> Option<u16> {
        match self {
            Self::Gf256 => {
                let [_, a] = a.to_be_bytes();
                match gf256::inverse(a) {
                    Some(inverse) => Some(u16::from_be_bytes([0, inverse])),
                    None => None,
                }
            }
            Self::Gf65536 => gf65536::inverse(a),
            Self::Gf2048 => gf2048::inverse(a),
        }
    }

    /// `out ^= factor · source`, symbol by symbol. The slices have one length, a whole number of
    /// symbols. The bytes of `source` and `out` choose no branch and no table entry here: only
    /// `factor` does.
    pub(crate) fn add_scaled(self, out: &mut [u8], source: &[u8], factor: u16) {
        match self {
            Self::Gf256 => {
                let [_, factor] = factor.to_be_bytes();
                lanes::add_scaled_bytes(out, source, factor);
            }
            Self::Gf65536 => lanes::add_scaled_pairs(out, source, factor),
            Self::Gf2048 => lanes::add_scaled_words(out, source, factor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_has_as_many_points_as_elements() {
        assert_eq!(Field::Gf256.point(255), Some(255));
        assert_eq!(Field::Gf256.point(256), None);
        assert_eq!(Field::Gf65536.point(65_535), Some(65_535));
        assert_eq!(Field::Gf2048.point(2047), Some(2047));
        assert_eq!(Field::Gf2048.point(2048), None);
        assert_eq!(
            [Field::Gf256, Field::Gf65536, Field::Gf2048].map(Field::order),
            [256, 65_536, 2048]
        );
        assert_eq!(
            [Field::Gf256, Field::Gf65536, Field::Gf2048].map(Field::symbol_bytes),
            [1, 2, 2]
        );
    }

    #[test]
    fn a_word_is_a_symbol_below_2048_and_other_bytes_are_any() {
        let any = [0xFF, 0xFF, 0x08, 0x00];
        assert!(Field::Gf256.canonical(&any) && Field::Gf65536.canonical(&any));
        assert!(Field::Gf2048.canonical(&[0x00, 0x00, 0x07, 0xFF]));
        // 2,048 itself, and each of the five bits above the eleven, in either symbol.
        for high in [0x08, 0x10, 0x20, 0x40, 0x80] {
            assert!(!Field::Gf2048.canonical(&[high, 0x00, 0x00, 0x00]));
            assert!(!Field::Gf2048.canonical(&[0x07, 0xFF, high, 0x00]));
        }
    }

    #[test]
    fn whole_shards_of_words_scale_as_their_symbols_do() {
        // Every symbol once, high byte first, onto an accumulator that is not zero.
        let all: Vec<u8> = (0..2048_u16).flat_map(u16::to_be_bytes).collect();
        let before: Vec<u8> = (0..2048_u16)
            .flat_map(|i| (i.wrapping_mul(0x35) & 0x7FF).to_be_bytes())
            .collect();
        for factor in [0, 1, 2, 3, 0x005, 0x400, 0x401, 0x555, 0x7FE, 0x7FF, 0x123] {
            let mut out = before.clone();
            Field::Gf2048.add_scaled(&mut out, &all, factor);
            let pairs = out.as_chunks::<2>().0.iter().zip(before.as_chunks::<2>().0);
            for (symbol, (pair, old)) in (0..2048_u16).zip(pairs) {
                let product = Field::Gf2048.multiply(factor, symbol);
                assert_eq!(
                    u16::from_be_bytes(*pair),
                    product ^ u16::from_be_bytes(*old),
                    "{factor:#05x} · {symbol:#05x}"
                );
            }
            // Stripes that end inside a word: one, two and three symbols.
            for length in [2, 4, 6] {
                let source = &all[4094 - length..4094];
                let mut short = before[..length].to_vec();
                Field::Gf2048.add_scaled(&mut short, source, factor);
                for ((pair, source), old) in short
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .zip(source.as_chunks::<2>().0)
                    .zip(before.as_chunks::<2>().0)
                {
                    let product = Field::Gf2048.multiply(factor, u16::from_be_bytes(*source));
                    assert_eq!(
                        u16::from_be_bytes(*pair),
                        product ^ u16::from_be_bytes(*old)
                    );
                }
            }
        }
    }

    #[test]
    fn whole_shards_scale_as_their_symbols_do() {
        // Every factor on every byte, over lengths that end inside, at and after a word.
        let all: Vec<u8> = (0..=255).collect();
        for factor in 0..=255_u16 {
            for length in [0, 1, 7, 8, 9, 16, 17, 256] {
                let mut out = vec![0x5A; length];
                Field::Gf256.add_scaled(&mut out, &all[..length], factor);
                for (position, &byte) in out.iter().enumerate() {
                    let product = Field::Gf256.multiply(factor, u16::from(all[position]));
                    assert_eq!(u16::from(byte ^ 0x5A), product, "{factor} at {position}");
                }
            }
        }
    }

    #[test]
    fn whole_shards_of_pairs_scale_as_their_symbols_do() {
        // Every symbol once, as [high, low], under factors that reach every term of the product.
        let all: Vec<u8> = (0..=u16::MAX).flat_map(u16::to_be_bytes).collect();
        let factors = [
            0, 1, 2, 0x80, 0xFF, 0x0100, 0x0101, 0x0280, 0x8000, 0x8001, 0xFF00, 0xFFFF, 0x1234,
            0xBEEF,
        ];
        for factor in factors {
            let mut out = vec![0; all.len()];
            Field::Gf65536.add_scaled(&mut out, &all, factor);
            for (symbol, pair) in (0..=u16::MAX).zip(out.as_chunks::<2>().0) {
                let product = Field::Gf65536.multiply(factor, symbol);
                assert_eq!(
                    *pair,
                    product.to_be_bytes(),
                    "{factor:#06x} · {symbol:#06x}"
                );
            }
            // A stripe that ends inside a word.
            let mut short = vec![0xA5; 6];
            let source = &all[1000..1006];
            Field::Gf65536.add_scaled(&mut short, source, factor);
            for (pair, source) in short
                .as_chunks::<2>()
                .0
                .iter()
                .zip(source.as_chunks::<2>().0)
            {
                let product = Field::Gf65536.multiply(factor, u16::from_be_bytes(*source));
                assert_eq!(*pair, (product ^ 0xA5A5).to_be_bytes());
            }
        }
    }
}
