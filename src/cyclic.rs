//! The cyclic code: a shortened systematic Reed–Solomon code whose shards after the data are the
//! remainder of the data divided by a generator, as QR codes and printed repair words make them.
//! A word of the code is one symbol of each shard, at the same place. [`CyclicWords`] works in
//! memory the caller gives, without an allocator; `CyclicCode` holds its own.

mod decode;
#[cfg(feature = "alloc")]
mod owned;
mod words;

#[cfg(feature = "alloc")]
pub use owned::CyclicCode;
pub use words::CyclicWords;

use crate::field::Field;

/// `base` to the power `exponent`, by squares. Exponents here are public: places and roots.
pub(crate) fn power(field: Field, base: u16, exponent: usize) -> u16 {
    let (mut result, mut square, mut rest) = (1, base, exponent);
    while rest > 0 {
        if rest & 1 == 1 {
            result = field.multiply(result, square);
        }
        square = field.multiply(square, square);
        rest >>= 1;
    }
    result
}
