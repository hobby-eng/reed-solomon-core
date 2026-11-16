//! The code: shard `i` is the value at point `i` of the one polynomial of degree below `k` that goes
//! through the data shards. Every operation is one Lagrange interpolation from any `k` shards, so
//! no matrix is ever inverted.

#[cfg(feature = "alloc")]
mod owned;

use crate::field::Field;
use core::fmt;
#[cfg(not(feature = "protect"))]
use zeroize::Zeroizing;

#[cfg(feature = "alloc")]
pub use owned::Redundancy;

/// Why a request was refused. Nothing was written then; lent working memory may hold anything.
/// Counts, indexes and lengths are reported before a symbol outside the field.
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
    /// A working buffer shorter than the threshold, or a room shorter than [`COMPARED`].
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
    /// The system gave no memory for a shard to return, for a search, for a list of changes or,
    /// with the `protect` feature, for the bytes [`Sources::agrees`] compares.
    Memory,
    /// The lengths, field or roots of a cyclic code ([`crate::CyclicWords`]) make no code: no data
    /// or no parity, a field in which x is no generator, or a word longer than the order of its
    /// roots.
    Shape,
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
            Self::Memory => "the system gave no memory for the call",
            Self::Shape => "the lengths, field or roots make no cyclic code",
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

    /// Takes the shards numbered `indexes` (exactly `threshold` different ones) as sources for others.
    /// `weights` is working memory of at least `threshold` elements; the cost, `threshold²`
    /// multiplications, is paid once for any number of shards made later. Fails on a wrong count, a
    /// short buffer, or a bad or repeated index.
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

    /// Writes into `out` shard `target` for one stretch of the set, from the same stretch of every
    /// source, in the order given to [`Code::sources`]. A large shard can be made piece by piece. Fails
    /// on a bad target, a wrong stripe count, unequal lengths or a symbol outside the field; `out` is
    /// then left as it was.
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

    /// Whether `shard` is what [`Sources::build`] would write for `target`, made [`COMPARED`]
    /// bytes at a time. Fails as [`Sources::build`] does, then, with the `protect` feature, with
    /// [`CodeError::Memory`] when the system gives no page for those bytes;
    /// [`Sources::agrees_in`] takes them from the caller instead.
    pub fn agrees(&self, target: u16, stripes: &[&[u8]], shard: &[u8]) -> Result<bool, CodeError> {
        let recipe = self.comparable(target, stripes, shard)?;
        // The expected bytes follow from the data: with `protect` in pages of their own, without
        // it on the stack; wiped when the call returns either way.
        #[cfg(feature = "protect")]
        let mut room = crate::buffer::Buffer::zeros(COMPARED).ok_or(CodeError::Memory)?;
        #[cfg(not(feature = "protect"))]
        let mut room = Zeroizing::new([0_u8; COMPARED]);
        self.compare(&recipe, stripes, shard, room.as_mut_slice())
    }

    /// [`Sources::agrees`] with the expected bytes made in `room`, at least [`COMPARED`] bytes,
    /// which a caller comparing many stretches keeps for all of them and wipes, since they follow
    /// from the data. Fails as [`Sources::build`] does, and with [`CodeError::Buffer`] for a
    /// shorter room after the counts and lengths.
    pub fn agrees_in(
        &self,
        target: u16,
        stripes: &[&[u8]],
        shard: &[u8],
        room: &mut [u8],
    ) -> Result<bool, CodeError> {
        self.geometry(stripes, shard.len())?;
        let room = room.get_mut(..COMPARED).ok_or(CodeError::Buffer)?;
        let recipe = self.comparable(target, stripes, shard)?;
        self.compare(&recipe, stripes, shard, room)
    }

    /// What [`Sources::agrees`] refuses, before any memory is asked for: the recipe of `target`.
    fn comparable(
        &self,
        target: u16,
        stripes: &[&[u8]],
        shard: &[u8],
    ) -> Result<Recipe, CodeError> {
        self.geometry(stripes, shard.len())?;
        let recipe = self.recipe(target)?;
        self.symbols(stripes)?;
        if self.field.canonical(shard) {
            Ok(recipe)
        } else {
            Err(CodeError::Symbol)
        }
    }

    /// Compares `shard` with what `recipe` makes, a stretch of `room`, at least [`COMPARED`]
    /// bytes, at a time.
    fn compare(
        &self,
        recipe: &Recipe,
        stripes: &[&[u8]],
        shard: &[u8],
        room: &mut [u8],
    ) -> Result<bool, CodeError> {
        const STEP: usize = COMPARED;
        for (number, given) in shard.chunks(STEP).enumerate() {
            let range = number * STEP..number * STEP + given.len();
            let expected = room.get_mut(..given.len()).ok_or(CodeError::Length)?;
            expected.fill(0);
            let sources = self.indexes.iter().zip(self.weights).zip(stripes);
            for (position, ((&index, &weight), stripe)) in sources.enumerate() {
                let stripe = stripe.get(range.clone()).ok_or(CodeError::Length)?;
                let coefficient = self.coefficient(recipe, position, index, weight);
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

/// Bytes [`Sources::agrees`] compares at a time, and the least room [`Sources::agrees_in`] takes:
/// a whole number of symbols of every field.
pub const COMPARED: usize = 256;

/// The one length of shards whose lengths are `lengths`, a whole number of symbols of `field`;
/// [`CodeError::Length`] otherwise.
pub(crate) fn one_length(
    field: Field,
    mut lengths: impl Iterator<Item = usize>,
) -> Result<usize, CodeError> {
    let length = lengths.next().unwrap_or(0);
    let whole = length.is_multiple_of(field.symbol_bytes());
    if !whole || lengths.any(|other| other != length) {
        return Err(CodeError::Length);
    }
    Ok(length)
}

/// Whether every symbol of `shards` is an element of `field`, [`CodeError::Symbol`] if not. Every
/// shard is looked at to its end, wherever the first wrong symbol lies.
pub(crate) fn in_field<'a>(
    field: Field,
    shards: impl Iterator<Item = &'a [u8]>,
) -> Result<(), CodeError> {
    if shards.fold(true, |all, bytes| all & field.canonical(bytes)) {
        Ok(())
    } else {
        Err(CodeError::Symbol)
    }
}
