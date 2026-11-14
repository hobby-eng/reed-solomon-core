//! The code: shard `i` of a set is the value at point `i` of the one polynomial of degree below
//! `k` whose values at the points 0 to `k - 1` are the data shards.
//!
//! Every operation is one interpolation. From any `k` shards with the points `x_1 … x_k`, the
//! shard at point `t` is `Σ L_i(t) · shard_i`, with the Lagrange coefficients
//! `L_i(t) = Π_{l≠i} (t − x_l) / (x_i − x_l)`. Taking the data shards as sources writes a set,
//! taking any `k` survivors makes a lost shard again or a new one, and no matrix is inverted.
//! Any `k` shards do, because a polynomial of degree below `k` is fixed by `k` of its values.

#[cfg(feature = "alloc")]
mod owned;

use crate::field::Field;
use core::fmt;
use zeroize::Zeroizing;

#[cfg(feature = "alloc")]
pub use owned::Redundancy;

/// Why a request was refused. No output was written when one is returned: an output buffer or
/// a shard to correct is as it was. Working memory lent for the call, the weights of
/// [`Code::sources`], may hold anything afterwards.
///
/// When a request is wrong in more than one way, the counts, the indexes and the lengths are
/// reported before a symbol that is not an element of the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeError {
    /// The threshold is zero or above the number of shards the field allows.
    Threshold,
    /// A shard index the field does not have.
    Index,
    /// The same shard index twice.
    Repeated,
    /// Not as many shards as the request needs: exactly the threshold for [`Code::sources`] and
    /// for the stripes of a [`Sources`], at least the threshold elsewhere.
    Count,
    /// Shards of different lengths, or a length that is not a whole number of symbols.
    Length,
    /// A working buffer shorter than the threshold.
    Buffer,
    /// A symbol that is not an element of the field: in [`Field::Gf2048`], two bytes that hold
    /// 2,048 or more.
    Symbol,
    /// A symbol marked unreadable that is not among the shards given: a shard past the last one,
    /// or a symbol past the end of the shards.
    Mark,
    /// A change that does not fit the shards: a place outside them, a symbol that no longer holds
    /// what the change says was there, or a new value that is not an element of the field.
    Changes,
}

impl fmt::Display for CodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Threshold => "the threshold is zero or above the field's number of shards",
            Self::Index => "the field has no shard with this index",
            Self::Repeated => "the same shard index was given twice",
            Self::Count => "the number of shards does not fit the threshold",
            Self::Length => "the shards differ in length or are not a whole number of symbols long",
            Self::Buffer => "the working buffer is shorter than the threshold",
            Self::Symbol => "a symbol of a shard is not an element of the field",
            Self::Mark => "a symbol marked unreadable is not among the shards given",
            Self::Changes => "a change does not fit the shards it is applied to",
        })
    }
}

impl core::error::Error for CodeError {}

/// A code with threshold `k`: any `k` shards of a set make every other shard of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Code {
    field: Field,
    threshold: usize,
}

impl Code {
    /// The code of `field` with this threshold, from 1 to the field's number of shards.
    ///
    /// # Errors
    ///
    /// [`CodeError::Threshold`] outside that range.
    pub const fn new(field: Field, threshold: usize) -> Result<Self, CodeError> {
        if threshold == 0 || threshold > field.order() {
            return Err(CodeError::Threshold);
        }
        Ok(Self { field, threshold })
    }

    /// The field of the code.
    #[must_use]
    pub const fn field(&self) -> Field {
        self.field
    }

    /// How many shards rebuild a set.
    #[must_use]
    pub const fn threshold(&self) -> usize {
        self.threshold
    }

    /// The shards numbered `indexes` as the sources of others: exactly `threshold` different
    /// shards of one set. `weights` is working memory of at least `threshold` elements, kept by
    /// the result. The work is `threshold` squared field multiplications, done once for any
    /// number of shards made afterwards.
    ///
    /// # Errors
    ///
    /// [`CodeError::Count`] unless `indexes` holds exactly `threshold` indexes,
    /// [`CodeError::Buffer`] for a `weights` shorter than that, [`CodeError::Index`] for an index
    /// the field does not have and [`CodeError::Repeated`] for one given twice. After a refusal
    /// `weights` may hold anything.
    pub fn sources<'a>(
        &self,
        indexes: &'a [u16],
        weights: &'a mut [u16],
    ) -> Result<Sources<'a>, CodeError> {
        if indexes.len() != self.threshold {
            return Err(CodeError::Count);
        }
        let weights = weights.get_mut(..self.threshold).ok_or(CodeError::Buffer)?;
        for index in indexes {
            self.field.point(*index).ok_or(CodeError::Index)?;
        }
        // The weight of source i is 1 / Π_{l≠i} (x_i − x_l); the product is zero exactly when
        // another source has the same point.
        for (position, (&point, weight)) in indexes.iter().zip(weights.iter_mut()).enumerate() {
            let mut product = 1;
            for (other_position, &other) in indexes.iter().enumerate() {
                if other_position != position {
                    product = self.field.multiply(product, point ^ other);
                }
            }
            *weight = self.field.inverse(product).ok_or(CodeError::Repeated)?;
        }
        Ok(Sources {
            field: self.field,
            indexes,
            weights,
        })
    }
}

/// How a shard is made from the sources.
enum Recipe {
    /// It is the source at this position.
    Copy(usize),
    /// It is a weighted sum of all sources; the value is `Π_l (t − x_l)` for its point `t`.
    Sum { point: u16, product: u16 },
}

/// `threshold` shards of one set, chosen as the sources of any other shard of it.
#[derive(Debug)]
pub struct Sources<'a> {
    field: Field,
    indexes: &'a [u16],
    /// Per source, the inverse of the product of its point's differences from the other points.
    weights: &'a [u16],
}

impl Sources<'_> {
    /// How shard `target` follows from the sources.
    fn recipe(&self, target: u16) -> Result<Recipe, CodeError> {
        let point = self.field.point(target).ok_or(CodeError::Index)?;
        if let Some(position) = self.indexes.iter().position(|&index| index == target) {
            return Ok(Recipe::Copy(position));
        }
        let product = self.indexes.iter().fold(1, |product, &index| {
            self.field.multiply(product, point ^ index)
        });
        Ok(Recipe::Sum { point, product })
    }

    /// The Lagrange coefficient of the source at `position`.
    fn coefficient(&self, recipe: &Recipe, position: usize, index: u16, weight: u16) -> u16 {
        match *recipe {
            Recipe::Copy(copied) => u16::from(copied == position),
            // Π_{l≠i} (t − x_l) is the whole product divided by (t − x_i), which is nonzero:
            // the target is none of the sources.
            Recipe::Sum { point, product } => {
                let others = self
                    .field
                    .inverse(point ^ index)
                    .map_or(0, |inverse| self.field.multiply(product, inverse));
                self.field.multiply(others, weight)
            }
        }
    }

    /// Checks that `stripes` are one stretch of `length` bytes of every source.
    fn geometry(&self, stripes: &[&[u8]], length: usize) -> Result<(), CodeError> {
        if stripes.len() != self.indexes.len() {
            return Err(CodeError::Count);
        }
        let whole = length.is_multiple_of(self.field.symbol_bytes());
        if !whole || stripes.iter().any(|stripe| stripe.len() != length) {
            return Err(CodeError::Length);
        }
        Ok(())
    }

    /// Checks that every symbol of the stripes is an element of the field. Every stripe is
    /// looked at whole, whatever the others hold. It comes after the counts, the lengths and
    /// the target's number.
    fn symbols(&self, stripes: &[&[u8]]) -> Result<(), CodeError> {
        let canonical = stripes
            .iter()
            .fold(true, |all, stripe| all & self.field.canonical(stripe));
        if canonical {
            Ok(())
        } else {
            Err(CodeError::Symbol)
        }
    }

    /// Writes into `out` the bytes of shard `target` for one stretch of the set: `stripes` holds
    /// the same stretch of every source, in the order of the indexes given to [`Code::sources`].
    /// A whole shard is one stretch; a large one can be made piece by piece. A target that is
    /// one of the sources is copied.
    ///
    /// # Errors
    ///
    /// [`CodeError::Index`] for a target the field does not have, [`CodeError::Count`] unless
    /// there is one stripe per source, [`CodeError::Length`] unless the stripes and `out` have
    /// one length that is a whole number of symbols, [`CodeError::Symbol`] for a symbol of a
    /// stripe that is not an element of the field. `out` is left as it was; what it held
    /// before is never read.
    pub fn build(&self, target: u16, stripes: &[&[u8]], out: &mut [u8]) -> Result<(), CodeError> {
        self.geometry(stripes, out.len())?;
        let recipe = self.recipe(target)?;
        self.symbols(stripes)?;
        // A shard that is one of the sources is that source: no sum over all of them.
        if let Recipe::Copy(position) = recipe
            && let Some(stripe) = stripes.get(position)
        {
            out.copy_from_slice(stripe);
            return Ok(());
        }
        out.fill(0);
        let sources = self.indexes.iter().zip(self.weights).zip(stripes);
        for (position, ((&index, &weight), stripe)) in sources.enumerate() {
            let coefficient = self.coefficient(&recipe, position, index, weight);
            self.field.add_scaled(out, stripe, coefficient);
        }
        Ok(())
    }

    /// Whether `shard` is what [`Sources::build`] would write for `target` from `stripes`: the
    /// check of a further shard against the sources, without a buffer for the expected bytes.
    ///
    /// # Errors
    ///
    /// As [`Sources::build`], with `shard` in the place of `out`.
    pub fn agrees(&self, target: u16, stripes: &[&[u8]], shard: &[u8]) -> Result<bool, CodeError> {
        /// Bytes compared at a time: a whole number of symbols of either field.
        const STEP: usize = 256;
        self.geometry(stripes, shard.len())?;
        let recipe = self.recipe(target)?;
        self.symbols(stripes)?;
        if !self.field.canonical(shard) {
            return Err(CodeError::Symbol);
        }
        // The expected bytes follow from the data: wiped when the call returns.
        let mut expected = Zeroizing::new([0_u8; STEP]);
        for (number, given) in shard.chunks(STEP).enumerate() {
            let range = number * STEP..number * STEP + given.len();
            let expected = expected.get_mut(..given.len()).ok_or(CodeError::Length)?;
            expected.fill(0);
            let sources = self.indexes.iter().zip(self.weights).zip(stripes);
            for (position, ((&index, &weight), stripe)) in sources.enumerate() {
                let stripe = stripe.get(range.clone()).ok_or(CodeError::Length)?;
                let coefficient = self.coefficient(&recipe, position, index, weight);
                self.field.add_scaled(expected, stripe, coefficient);
            }
            // Every byte of the stretch is looked at, whichever differs.
            let differ = expected
                .iter()
                .zip(given)
                .fold(0, |differ, (made, held)| differ | (made ^ held));
            if differ != 0 {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
