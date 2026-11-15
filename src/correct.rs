//! Finding and correcting wrong symbols when nobody knows which shards are wrong. The symbols at
//! one offset form a code word; its `m − k` syndromes are zero exactly when the word is valid, and
//! otherwise Berlekamp–Massey finds the errors, up to `(m − k) / 2` per column. The arithmetic
//! never branches on shard bytes or indexes a table by them; search buffers are wiped on drop.

mod changes;
mod plan;

use crate::buffer::Buffer;
use crate::code::{Code, CodeError};
use crate::field::Field;
use alloc::vec::Vec;
use core::ops::Deref;
use zeroize::Zeroize;

pub use changes::{Changes, ChangesIter};
pub use plan::{Change, ChangeKind, Mark, Plan};

/// The system gave no memory for a search.
#[derive(Debug)]
pub(crate) struct NoMemory;

/// Most bytes of syndromes held at a time, whatever the size of the shards.
const SYNDROME_BYTES: usize = 1 << 22;

/// What [`Code::correct`] found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Correction {
    /// Exactly `threshold` shards were given: there is nothing to compare them with, and nothing
    /// was checked.
    Unchecked,
    /// The shards agree; nothing was changed.
    Agrees,
    /// Wrong symbols were found and corrected in place: per shard, in the order given, how many
    /// of its symbols were changed. Whether the result is the original data is for a digest or
    /// an authentication to say.
    Corrected(Vec<usize>),
    /// In `columns` symbol columns the shards disagree in a way the spare shards cannot resolve:
    /// more symbols of the column are wrong than half the number of spare shards, `(m − k) / 2`
    /// rounded down. Nothing was changed.
    Unresolved {
        /// How many symbol columns could not be put right.
        columns: usize,
    },
}

impl Code {
    /// Finds wrong symbols among `shards`, given as `(index, bytes)`, and corrects them in place: up to
    /// `(m − k) / 2` per column, wherever they are. Missing shards are simply not given. Either every
    /// column is fixed or nothing changes. Asks for at most [`Code::correction_memory`] bytes (under
    /// 8 MiB). Fails on too few shards, bad indexes, unequal lengths, a symbol outside the field, or
    /// with [`CodeError::Memory`] when that memory is refused.
    pub fn correct(&self, shards: &mut [(u16, &mut [u8])]) -> Result<Correction, CodeError> {
        let indexes = shards.iter().map(|&(index, _)| index);
        let length = self.check_set(indexes, shards.iter().map(|(_, bytes)| bytes.len()))?;
        self.check_symbols(shards.iter().map(|(_, bytes)| &**bytes))?;
        let spare = shards.len() - self.threshold();
        if spare == 0 {
            return Ok(Correction::Unchecked);
        }
        if length == 0 {
            return Ok(Correction::Agrees);
        }
        let points: Vec<u16> = shards.iter().map(|&(index, _)| index).collect();
        // The memory of the search has a published bound; a refusal of it is an answer.
        let mut decoder =
            Decoder::new(self.field(), points, spare, length).map_err(|_| CodeError::Memory)?;
        // First find every wrong symbol without touching the shards, then correct them. Both
        // passes work in the decoder's own buffers, so the second takes no memory of its own.
        let unresolved = decoder.pass(shards, length, false);
        if unresolved > 0 {
            return Ok(Correction::Unresolved {
                columns: unresolved,
            });
        }
        if decoder.changed.iter().all(|&count| count == 0) {
            return Ok(Correction::Agrees);
        }
        decoder.pass(shards, length, true);
        Ok(Correction::Corrected(core::mem::take(&mut decoder.changed)))
    }

    /// An upper bound, in bytes, of the memory [`Code::correct`] asks for with `shards` shards,
    /// whatever their length: the syndromes of one stripe and a few elements per shard and per
    /// spare shard. With the `protect` feature the system rounds the syndromes up to whole pages.
    /// With exactly `threshold` shards only their indexes are held. Zero for a number of shards
    /// `correct` refuses at once: fewer than the threshold, or more than the field has indexes.
    #[must_use]
    pub fn correction_memory(&self, shards: usize) -> usize {
        if shards < self.threshold() || shards > self.field().order() {
            return 0;
        }
        // The check of the indexes holds them once and lets go before the search begins.
        let check = shards * size_of::<u16>();
        match shards - self.threshold() {
            0 => check,
            spare => check.max(Decoder::memory(self.field(), shards, spare)),
        }
    }
}

/// The shards present and the working memory of the search.
struct Decoder {
    field: Field,
    /// The point of each shard.
    points: Vec<u16>,
    /// Per shard, 1 / Π_{l≠i} (x_i − x_l) over the shards present.
    weights: Vec<u16>,
    /// The number of syndromes: the shards beyond the threshold.
    spare: usize,
    /// The connection polynomial of the recurrence and Berlekamp–Massey's two copies of it.
    connection: Vec<u16>,
    previous: Vec<u16>,
    saved: Vec<u16>,
    /// The syndromes of the column at hand.
    column: Vec<u16>,
    /// The wrong symbols of the column at hand: the shard's position and the error to remove.
    /// Symbols found by the search come first, marked ones after them. There is room for half
    /// the spare shards for a search alone, for all of them when symbols may be marked.
    errors: Vec<(usize, u16)>,
    /// With marks only: the positions of the shards whose symbol is marked in the column at
    /// hand, `G(X) = Π (X − x)` over their points, lowest coefficient first, and the syndromes
    /// with the marked symbols taken out, `T_j = Σ_i g_i · S_(j+i)`.
    erased: Vec<usize>,
    erasure: Vec<u16>,
    sequence: Vec<u16>,
    /// Π (z − X_e) over the wrong symbols' points, lowest coefficient first, and the running
    /// powers X_e^j: the working memory of `values`.
    product: Vec<u16>,
    powers: Vec<u16>,
    /// The syndromes of the stripe at hand, one after another, and per shard the running factor
    /// `u_i · x_i^j` of the syndrome being summed.
    syndromes: Buffer,
    factors: Vec<u16>,
    /// Wrong symbols found per shard, in the order the shards were given.
    changed: Vec<usize>,
}

impl Decoder {
    /// The decoder of the shards at `points`, `spare` (at least one) of them beyond the
    /// threshold, for shards of `length` bytes. Everything a search needs is allocated here,
    /// and a refusal of the memory is the answer.
    fn new(field: Field, points: Vec<u16>, spare: usize, length: usize) -> Result<Self, NoMemory> {
        Self::with_room(field, points, spare, length, Self::most_errors(spare))
    }

    /// The decoder of [`Decoder::new`] with room for `most` wrong symbols a column, and with the
    /// buffers of marked symbols when `most` is above half the spare shards.
    fn with_room(
        field: Field,
        points: Vec<u16>,
        spare: usize,
        length: usize,
        most: usize,
    ) -> Result<Self, NoMemory> {
        let shards = points.len();
        let marked = most > Self::most_errors(spare);
        let mut weights = roomy(shards)?;
        weights.extend(points.iter().enumerate().map(|(position, &point)| {
            let product = points
                .iter()
                .enumerate()
                .filter(|&(other, _)| other != position)
                .fold(1, |product, (_, &other)| {
                    field.multiply(product, point ^ other)
                });
            // The points differ, so the product is not zero.
            field.inverse(product).unwrap_or(0)
        }));
        Ok(Self {
            field,
            points,
            weights,
            spare,
            connection: zeros(spare + 1)?,
            previous: zeros(spare + 1)?,
            saved: zeros(spare + 1)?,
            column: zeros(spare)?,
            errors: roomy(most)?,
            erased: roomy(if marked { spare } else { 0 })?,
            erasure: roomy(if marked { spare + 1 } else { 0 })?,
            sequence: roomy(if marked { spare } else { 0 })?,
            product: roomy(most + 1)?,
            powers: roomy(most)?,
            syndromes: Buffer::zeros(spare * Self::stripe(field, spare).min(length))
                .ok_or(NoMemory)?,
            factors: zeros(shards)?,
            changed: zeros(shards)?,
        })
    }

    /// The most wrong symbols a column can be cleared of: half the spare shards.
    const fn most_errors(spare: usize) -> usize {
        spare / 2
    }

    /// Bytes of one stripe of all syndromes: within `SYNDROME_BYTES`, but at least one symbol
    /// of each.
    const fn stripe(field: Field, spare: usize) -> usize {
        let symbol = field.symbol_bytes();
        let symbols = SYNDROME_BYTES / spare / symbol;
        (if symbols == 0 { 1 } else { symbols }) * symbol
    }

    /// An upper bound of the bytes this decoder allocates for `shards` shards, of which `spare`
    /// (at least one) are beyond the threshold; its two passes allocate nothing more.
    const fn memory(field: Field, shards: usize, spare: usize) -> usize {
        let element = size_of::<u16>();
        // Points, weights and the factors of a stripe; the count of changes.
        let per_shard = 3 * element + size_of::<usize>();
        // The recurrence and its two copies, and the column's syndromes.
        let per_spare = 3 * (spare + 1) * element + spare * element;
        let per_error = size_of::<(usize, u16)>() + 2 * element;
        shards * per_shard
            + per_spare
            + (Self::most_errors(spare) + 1) * per_error
            + spare * Self::stripe(field, spare)
    }

    /// Goes through the shards stripe by stripe and finds the wrong symbols of every column:
    /// without `apply` it counts them per shard in `self.changed`, with `apply` it corrects
    /// them. Returns how many columns it could not resolve.
    fn pass(&mut self, shards: &mut [(u16, &mut [u8])], length: usize, apply: bool) -> usize {
        let symbol = self.field.symbol_bytes();
        let stripe = Self::stripe(self.field, self.spare);
        // The stripe's two buffers leave the decoder for the pass, so that the search can use
        // the rest of it while they are read; they come back for the next pass.
        let mut syndromes = core::mem::take(&mut self.syndromes);
        let mut factors = core::mem::take(&mut self.factors);
        let mut unresolved = 0;
        let mut start = 0;
        while start < length {
            let size = stripe.min(length - start);
            if self.fill_syndromes(shards, start, size, &mut factors, &mut syndromes) {
                for offset in (0..size).step_by(symbol) {
                    if !self.read_column(&syndromes, size, offset) {
                        continue;
                    }
                    if !self.locate() {
                        unresolved += 1;
                        continue;
                    }
                    for &(position, error) in &self.errors {
                        if apply {
                            remove(shards, position, start + offset, symbol, error);
                        } else if let Some(count) = self.changed.get_mut(position) {
                            *count += 1;
                        }
                    }
                }
            }
            start += size;
        }
        self.syndromes = syndromes;
        self.factors = factors;
        unresolved
    }

    /// Fills `syndromes` with the `spare` syndromes of the stripe of `size` bytes at `start`,
    /// one after another: syndrome j is Σ u_i · x_i^j · shard_i. Only the bytes they take are
    /// written and looked at, however long the buffer is. Returns whether one of them is not
    /// zero: whether the shards disagree somewhere in the stripe.
    fn fill_syndromes<B: Deref<Target = [u8]>>(
        &self,
        shards: &[(u16, B)],
        start: usize,
        size: usize,
        factors: &mut [u16],
        syndromes: &mut [u8],
    ) -> bool {
        let taken = syndromes.len().min(self.spare * size);
        let syndromes = syndromes.get_mut(..taken).unwrap_or_default();
        syndromes.fill(0);
        factors.copy_from_slice(&self.weights);
        for syndrome in syndromes.chunks_mut(size) {
            for (((_, bytes), factor), &point) in
                shards.iter().zip(factors.iter_mut()).zip(&self.points)
            {
                if let Some(stripe) = bytes.get(start..start + size) {
                    self.field.add_scaled(syndrome, stripe, *factor);
                }
                *factor = self.field.multiply(*factor, point);
            }
        }
        syndromes.iter().any(|&byte| byte != 0)
    }

    /// Reads the syndromes of the column at byte `offset` of a stripe of `size` bytes; false when
    /// all are zero, which is a column whose symbols agree.
    fn read_column(&mut self, syndromes: &[u8], size: usize, offset: usize) -> bool {
        let symbol = self.field.symbol_bytes();
        let mut any = false;
        for (value, syndrome) in self.column.iter_mut().zip(syndromes.chunks(size)) {
            let bytes = syndrome.get(offset..offset + symbol).unwrap_or_default();
            *value = bytes
                .iter()
                .fold(0, |value, &byte| value << 8 | u16::from(byte));
            any |= *value != 0;
        }
        any
    }

    /// Finds the wrong symbols of the column whose syndromes are in `self.column` and leaves
    /// them in `self.errors`; false when they cannot be found.
    fn locate(&mut self) -> bool {
        let length = self.recurrence(false);
        self.errors.clear();
        if 2 * length > self.spare {
            return false;
        }
        // The characteristic polynomial X^L · C(1/X) = c_0·X^L + c_1·X^(L-1) + … + c_L has the
        // points of the wrong symbols as its roots; a zero c_L is the root 0, the point of shard 0.
        let field = self.field;
        let connection = self.connection.get(..=length).unwrap_or_default();
        for (position, &point) in self.points.iter().enumerate() {
            let value = connection
                .iter()
                .fold(0, |value, &c| field.multiply(value, point) ^ c);
            if value == 0 {
                self.errors.push((position, 0));
            }
        }
        // Exactly L different roots among the shards present, or the recurrence is not that of
        // L wrong symbols.
        self.errors.len() == length && self.values(length)
    }

    /// Berlekamp–Massey: the shortest recurrence `Σ_{i=0..=L} c_i · s_{n−i} = 0` (c_0 = 1) that
    /// the syndromes obey, or with `marked` the syndromes with the marked symbols taken out.
    /// Leaves its polynomial in `self.connection` and returns `L`.
    fn recurrence(&mut self, marked: bool) -> usize {
        let field = self.field;
        self.connection.fill(0);
        self.previous.fill(0);
        if let (Some(c), Some(b)) = (self.connection.first_mut(), self.previous.first_mut()) {
            (*c, *b) = (1, 1);
        }
        let sequence: &[u16] = if marked { &self.sequence } else { &self.column };
        let (mut length, mut shift, mut last) = (0, 1, 1);
        for (n, &syndrome) in sequence.iter().enumerate() {
            // The difference between the syndrome and what the recurrence so far predicts.
            let taps = self.connection.iter().skip(1).take(length);
            let earlier = sequence.iter().take(n).rev();
            let discrepancy = taps
                .zip(earlier)
                .fold(syndrome, |sum, (&c, &s)| sum ^ field.multiply(c, s));
            if discrepancy == 0 {
                shift += 1;
                continue;
            }
            let scale = field.multiply(discrepancy, field.inverse(last).unwrap_or(0));
            let grows = 2 * length <= n;
            if grows {
                self.saved.copy_from_slice(&self.connection);
            }
            for (c, &b) in self.connection.iter_mut().skip(shift).zip(&self.previous) {
                *c ^= field.multiply(scale, b);
            }
            if grows {
                core::mem::swap(&mut self.previous, &mut self.saved);
                (length, last, shift) = (n + 1 - length, discrepancy, 1);
            } else {
                shift += 1;
            }
        }
        length
    }

    /// The error value of each wrong symbol, from the first syndromes, then a check of every syndrome
    /// against it; false when they don't fit. Each value takes one synthetic division, so memory grows
    /// with the number of errors and no matrix is built. Marked symbols may turn out to be right.
    fn values(&mut self, found: usize) -> bool {
        let field = self.field;
        let count = self.errors.len();
        self.product.clear();
        self.product.push(1);
        for &(position, _) in &self.errors {
            let point = self.points.get(position).copied().unwrap_or(0);
            // Times (z − X): each coefficient becomes the one below it plus X times itself.
            let mut below = 0;
            for coefficient in &mut self.product {
                let old = *coefficient;
                *coefficient = below ^ field.multiply(point, old);
                below = old;
            }
            self.product.push(below);
        }
        for (position, value) in &mut self.errors {
            let point = self.points.get(*position).copied().unwrap_or(0);
            // q_e from its highest coefficient down, with Σ S_j · q_{e,j} and q_e(X_e) on the way.
            let (mut quotient, mut sum, mut at_point) = (0, 0, 0);
            let syndromes = self.column.iter().take(count).rev();
            for (&coefficient, &syndrome) in self.product.iter().skip(1).rev().zip(syndromes) {
                quotient = coefficient ^ field.multiply(point, quotient);
                sum ^= field.multiply(syndrome, quotient);
                at_point = field.multiply(at_point, point) ^ quotient;
            }
            // The points differ, so q_e(X_e) is not zero.
            *value = field.multiply(sum, field.inverse(at_point).unwrap_or(0));
        }
        // Every syndrome, the first ones included, must be the power sum of the solution.
        self.powers.clear();
        self.powers.resize(count, 1);
        for &syndrome in &self.column {
            let mut sum = 0;
            for (power, &(position, value)) in self.powers.iter_mut().zip(&self.errors) {
                sum ^= field.multiply(*power, value);
                let point = self.points.get(position).copied().unwrap_or(0);
                *power = field.multiply(*power, point);
            }
            if sum != syndrome {
                return false;
            }
        }
        for (at, (position, value)) in self.errors.iter_mut().enumerate() {
            let weight = self.weights.get(*position).copied().unwrap_or(0);
            *value = field.multiply(*value, field.inverse(weight).unwrap_or(0));
            if *value == 0 && at < found {
                return false;
            }
        }
        true
    }
}

impl Drop for Decoder {
    /// The syndromes, the recurrences and the errors follow from the data, and for a marked
    /// symbol the error is its value: all of them are wiped, the stripe's syndromes by their
    /// buffer. The points, the weights and the factors follow from the shard numbers alone.
    fn drop(&mut self) {
        self.column.zeroize();
        self.connection.zeroize();
        self.previous.zeroize();
        self.saved.zeroize();
        self.errors.zeroize();
        self.sequence.zeroize();
        self.product.zeroize();
        self.powers.zeroize();
        self.changed.zeroize();
    }
}

/// What [`Finder::look`] saw in one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Look {
    /// Whether the shards outside [`Finder::found`] share one word in the block. Otherwise no
    /// word is shared by all but as many shards as the spare ones can outvote.
    pub(crate) settled: bool,
    /// The columns looked at whose syndromes are not all zero.
    pub(crate) columns: usize,
    /// The columns searched for their wrong symbols.
    pub(crate) searches: usize,
}

/// Finds, block by block, the shards that differ from the word the others share; nothing is
/// corrected. A column that fits the shards already found needs no new search, so a shard that is
/// wrong everywhere costs one search per block.
pub(crate) struct Finder {
    decoder: Decoder,
    /// The positions of the shards found to differ in the block at hand.
    found: Vec<usize>,
    /// Π (1 − X·z) over their points, lowest coefficient first: the recurrence that the
    /// syndromes of a column obey when its wrong symbols are all theirs.
    fit: Vec<u16>,
}

impl Finder {
    /// The finder of the shards at `points`, `spare` (at least one) of them beyond the
    /// threshold, for blocks of at most `length` bytes.
    pub(crate) fn new(
        field: Field,
        points: &[u16],
        spare: usize,
        length: usize,
    ) -> Result<Self, NoMemory> {
        let mut own = roomy(points.len())?;
        own.extend_from_slice(points);
        let most = Decoder::most_errors(spare);
        Ok(Self {
            decoder: Decoder::new(field, own, spare, length)?,
            found: roomy(most)?,
            fit: roomy(most + 1)?,
        })
    }

    /// The points of the shards this finder was made for, in their order.
    pub(crate) fn points(&self) -> &[u16] {
        &self.decoder.points
    }

    /// The positions of the shards that differ in the block last looked at, when it settled.
    pub(crate) fn found(&self) -> &[usize] {
        &self.found
    }

    /// The bytes kept for the syndromes of a stripe.
    #[cfg(test)]
    pub(crate) fn syndrome_bytes(&self) -> usize {
        self.decoder.syndromes.len()
    }

    /// Looks at one block: `shards` holds it for every shard of the finder, in their order, each
    /// no longer than the finder was made for.
    pub(crate) fn look(&mut self, shards: &[(u16, &[u8])]) -> Look {
        let symbol = self.decoder.field.symbol_bytes();
        let stripe = Decoder::stripe(self.decoder.field, self.decoder.spare);
        let length = shards.first().map_or(0, |(_, bytes)| bytes.len());
        let mut syndromes = core::mem::take(&mut self.decoder.syndromes);
        let mut factors = core::mem::take(&mut self.decoder.factors);
        self.found.clear();
        self.fit.clear();
        self.fit.push(1);
        let mut look = Look {
            settled: true,
            columns: 0,
            searches: 0,
        };
        let mut start = 0;
        'block: while start < length {
            let size = stripe.min(length - start);
            let differ =
                self.decoder
                    .fill_syndromes(shards, start, size, &mut factors, &mut syndromes);
            if differ {
                for offset in (0..size).step_by(symbol) {
                    if !self.decoder.read_column(&syndromes, size, offset) {
                        continue;
                    }
                    // One spare shard shows that the shards differ and cannot say which.
                    if Decoder::most_errors(self.decoder.spare) == 0 {
                        look.settled = false;
                        break 'block;
                    }
                    look.columns += 1;
                    if self.fits() {
                        continue;
                    }
                    look.searches += 1;
                    if !self.decoder.locate() || !self.admit() {
                        look.settled = false;
                        break 'block;
                    }
                }
            }
            start += size;
        }
        self.decoder.syndromes = syndromes;
        self.decoder.factors = factors;
        look
    }

    /// Whether the syndromes of the column at hand obey the recurrence of the shards found so
    /// far, `Σ c_i · s_{n−i} = 0` from the first syndrome it reaches to the last: then they are
    /// the power sums of wrong symbols of those shards alone.
    fn fits(&self) -> bool {
        let field = self.decoder.field;
        let column = &self.decoder.column;
        (self.found.len()..column.len()).all(|n| {
            let earlier = column.iter().take(n + 1).rev();
            let sum = self.fit.iter().zip(earlier);
            sum.fold(0, |sum, (&c, &s)| sum ^ field.multiply(c, s)) == 0
        })
    }

    /// Takes the shards of the wrong symbols just found into the shards found; false when that
    /// makes more of them than the spare shards can outvote.
    fn admit(&mut self) -> bool {
        let field = self.decoder.field;
        let most = Decoder::most_errors(self.decoder.spare);
        for &(position, _) in &self.decoder.errors {
            if self.found.contains(&position) {
                continue;
            }
            if self.found.len() == most {
                return false;
            }
            self.found.push(position);
            let point = self.decoder.points.get(position).copied().unwrap_or(0);
            // Times (1 − X·z): each coefficient takes X times the one below it.
            self.fit.push(0);
            let mut below = 0;
            for coefficient in &mut self.fit {
                let old = *coefficient;
                *coefficient = old ^ field.multiply(point, below);
                below = old;
            }
        }
        true
    }
}

/// `count` elements of zero, or the refusal of the memory.
fn zeros<T: Clone + Default>(count: usize) -> Result<Vec<T>, NoMemory> {
    let mut list = roomy(count)?;
    list.resize(count, T::default());
    Ok(list)
}

/// An empty list with room for `count` elements, or the refusal of the memory.
fn roomy<T>(count: usize) -> Result<Vec<T>, NoMemory> {
    let mut list = Vec::new();
    list.try_reserve_exact(count).map_err(|_| NoMemory)?;
    Ok(list)
}

/// Removes `error` from the symbol at byte `at` of the shard at `position`.
fn remove(shards: &mut [(u16, &mut [u8])], position: usize, at: usize, symbol: usize, error: u16) {
    let bytes = shards
        .get_mut(position)
        .and_then(|(_, bytes)| bytes.get_mut(at..at + symbol));
    // The last `symbol` bytes of the error, high byte first.
    let error = error.to_be_bytes();
    let error = error.iter().skip(error.len() - symbol);
    for (byte, e) in bytes.into_iter().flatten().zip(error) {
        *byte ^= e;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// The syndromes of one column of one-byte shards at the points 0, 1, 2, ….
    fn syndromes_of(word: &[u8], threshold: usize) -> Vec<u8> {
        let points: Vec<u16> = (0..word.len() as u16).collect();
        let spare = word.len() - threshold;
        let decoder = Decoder::new(Field::Gf256, points, spare, 1).unwrap();
        let mut bytes: Vec<[u8; 1]> = word.iter().map(|&byte| [byte]).collect();
        let shards: Vec<(u16, &mut [u8])> =
            (0..).zip(bytes.iter_mut().map(|b| &mut b[..])).collect();
        let mut syndromes = vec![0; spare];
        let mut factors = vec![0; word.len()];
        decoder.fill_syndromes(&shards, 0, 1, &mut factors, &mut syndromes);
        syndromes
    }

    #[test]
    fn the_syndromes_equal_those_computed_apart_from_this_crate() {
        // Both words are the zero code word of threshold 3 with errors added; the syndromes were
        // computed independently by shifts and polynomial reduction with 0x11D.
        // An error 01 in shard 0, the point zero: only the first syndrome sees it.
        assert_eq!(syndromes_of(&[0x01, 0, 0, 0, 0], 3), [0x90, 0x00]);
        // Errors 01 in shard 0 and 52 in shard 2: the first syndrome is zero.
        assert_eq!(
            syndromes_of(&[0x01, 0, 0x52, 0, 0, 0, 0], 3),
            [0x00, 0x49, 0x92, 0x39]
        );
    }

    /// Every byte a decoder holds, counted by the capacities of its buffers. The two passes of
    /// `Code::correct` allocate nothing besides.
    fn held(decoder: &Decoder) -> usize {
        let elements = decoder.points.capacity()
            + decoder.weights.capacity()
            + decoder.connection.capacity()
            + decoder.previous.capacity()
            + decoder.saved.capacity()
            + decoder.column.capacity()
            + decoder.product.capacity()
            + decoder.powers.capacity()
            + decoder.factors.capacity();
        elements * size_of::<u16>()
            + decoder.errors.capacity() * size_of::<(usize, u16)>()
            + decoder.syndromes.len()
            + decoder.changed.capacity() * size_of::<usize>()
    }

    /// The syndromes of one column of word symbols at the points 0, 1, 2, ….
    fn word_syndromes(word: &[u16], threshold: usize) -> Vec<u16> {
        let points: Vec<u16> = (0..word.len() as u16).collect();
        let spare = word.len() - threshold;
        let decoder = Decoder::new(Field::Gf2048, points, spare, 2).unwrap();
        let mut bytes: Vec<[u8; 2]> = word.iter().map(|symbol| symbol.to_be_bytes()).collect();
        let shards: Vec<(u16, &mut [u8])> =
            (0..).zip(bytes.iter_mut().map(|b| &mut b[..])).collect();
        let mut syndromes = vec![0; 2 * spare];
        let mut factors = vec![0; word.len()];
        decoder.fill_syndromes(&shards, 0, 2, &mut factors, &mut syndromes);
        syndromes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect()
    }

    #[test]
    fn the_syndromes_of_words_equal_those_computed_apart_from_this_crate() {
        // The zero code word of threshold 3 over GF(2^11) with errors added; the syndromes were
        // computed independently by polynomial products reduced by 0x805.
        assert_eq!(word_syndromes(&[0x001, 0, 0, 0, 0], 3), [0x4FD, 0x000]);
        assert_eq!(
            word_syndromes(&[0x001, 0, 0x052, 0, 0, 0, 0], 3),
            [0x1B5, 0x11C, 0x238, 0x470]
        );
        // Two errors whose first syndrome is zero.
        assert_eq!(
            word_syndromes(&[0x001, 0, 0x2AA, 0, 0, 0, 0], 3),
            [0x000, 0x276, 0x4EC, 0x1DD]
        );
    }

    #[test]
    fn the_working_memory_stays_within_its_published_bound() {
        for (field, shards, threshold) in [
            (Field::Gf256, 256, 1),
            (Field::Gf256, 200, 100),
            (Field::Gf256, 10, 8),
            (Field::Gf65536, 5000, 1000),
            (Field::Gf65536, 3000, 2999),
            (Field::Gf2048, 2048, 24),
            (Field::Gf2048, 30, 24),
        ] {
            let spare = shards - threshold;
            let points: Vec<u16> = (0..shards as u16).collect();
            // Shards longer than any stripe: the decoder at its largest.
            let decoder = Decoder::new(field, points, spare, usize::MAX).unwrap();
            let used = held(&decoder);
            let bound = Code::new(field, threshold)
                .unwrap()
                .correction_memory(shards);
            assert!(
                used <= bound,
                "{field:?} {shards} {threshold}: {used} above {bound}"
            );
        }
        // The largest sets either field allows stay under 8 MiB.
        for (field, threshold) in [
            (Field::Gf256, 1),
            (Field::Gf65536, 1),
            (Field::Gf65536, 32_768),
            (Field::Gf2048, 1),
        ] {
            let code = Code::new(field, threshold).unwrap();
            assert!(code.correction_memory(field.order()) < 8 << 20);
        }
    }

    #[test]
    fn a_pass_leaves_the_decoder_with_the_buffers_it_started_with() {
        // Ten shards of 2 MiB with threshold 8, one symbol wrong: the case a measuring allocator
        // showed above the bound when the second pass took buffers of its own.
        let code = Code::new(Field::Gf256, 8).unwrap();
        let data: Vec<Vec<u8>> = (0..8_u8).map(|i| vec![i; 1 << 21]).collect();
        let data: Vec<&[u8]> = data.iter().map(|bytes| bytes.as_slice()).collect();
        let targets: Vec<u16> = (0..10).collect();
        let mut set = code.encode(&data, &targets).unwrap();
        set[3][12345] ^= 0x40;
        let mut shards: Vec<(u16, &mut [u8])> = (0..)
            .zip(set.iter_mut().map(|bytes| bytes.as_mut_slice()))
            .collect();
        let mut decoder = Decoder::new(Field::Gf256, targets.clone(), 2, 1 << 21).unwrap();
        let before = held(&decoder);
        assert_eq!(decoder.pass(&mut shards, 1 << 21, false), 0);
        assert_eq!(decoder.changed[3], 1);
        assert_eq!(decoder.pass(&mut shards, 1 << 21, true), 0);
        assert_eq!(held(&decoder), before);
        assert!(before <= code.correction_memory(10));
        drop(shards);
        assert_eq!(set[3][12345], 3);
    }

    /// A set of `count` shards of `columns` symbols with threshold `threshold`.
    fn a_set(field: Field, threshold: usize, count: usize, columns: usize) -> Vec<Vec<u8>> {
        let symbol = field.symbol_bytes();
        let data: Vec<Vec<u8>> = (0..threshold)
            .map(|shard| {
                let bytes = (0..columns * symbol).map(|at| (shard * 37 + at * 11 + 5) as u8);
                // The high byte of a word stays below eight.
                bytes
                    .enumerate()
                    .map(|(at, byte)| match (symbol, at % 2) {
                        (2, 0) => byte & 0x07,
                        _ => byte,
                    })
                    .collect()
            })
            .collect();
        let data: Vec<&[u8]> = data.iter().map(|bytes| bytes.as_slice()).collect();
        let targets: Vec<u16> = (0..count as u16).collect();
        Code::new(field, threshold)
            .unwrap()
            .encode(&data, &targets)
            .unwrap()
            .into_iter()
            .map(|shard| shard.to_vec())
            .collect()
    }

    /// Every choice of shards of small sets, each wrong in its own columns of a block of five:
    /// up to half the spare shards are named exactly, by one search for every column that
    /// brings a shard not yet found, and one shard more is never settled. The finder is the
    /// same for every choice, so nothing of one block may reach the next.
    #[test]
    fn the_finder_names_the_shards_that_differ_and_no_others() {
        let columns = 5;
        let sets = [
            (3, 4),
            (1, 3),
            (2, 4),
            (3, 5),
            (3, 6),
            (2, 6),
            (3, 7),
            (1, 5),
            (4, 9),
        ];
        for field in [Field::Gf256, Field::Gf65536, Field::Gf2048] {
            let symbol = field.symbol_bytes();
            for (threshold, count) in sets {
                let set = a_set(field, threshold, count, columns);
                let (spare, points) = (count - threshold, (0..count as u16).collect::<Vec<_>>());
                let most = spare / 2;
                let mut finder = Finder::new(field, &points, spare, columns * symbol).unwrap();
                for mask in 0_u32..1 << count {
                    let wrong: Vec<usize> = (0..count).filter(|bit| mask >> bit & 1 == 1).collect();
                    if wrong.len() > most + 1 {
                        continue;
                    }
                    // The shard that is wrong as the `n`th is wrong in the columns `c` with
                    // `c + n` not 2 modulo 3: no two neighbours in the same ones.
                    let in_column = |n: usize, c: usize| (c + n) % 3 != 2;
                    let mut held = set.clone();
                    for (n, &shard) in wrong.iter().enumerate() {
                        for c in (0..columns).filter(|&c| in_column(n, c)) {
                            held[shard][c * symbol + symbol - 1] ^= 1 + n as u8;
                        }
                    }
                    let shards: Vec<(u16, &[u8])> = (0..)
                        .zip(held.iter().map(|bytes| bytes.as_slice()))
                        .collect();
                    let look = finder.look(&shards);
                    let case = format!("{field:?} {threshold} of {count}, wrong {wrong:?}");
                    if wrong.len() > most {
                        assert!(!look.settled, "{case}");
                        continue;
                    }
                    let mut found = finder.found().to_vec();
                    found.sort_unstable();
                    assert!(look.settled, "{case}");
                    assert_eq!(found, wrong, "{case}");
                    // By the rule: a column with a wrong symbol is looked at, and searched
                    // when it has one of a shard not found before.
                    let mut known = vec![false; wrong.len()];
                    let (mut looked, mut searched) = (0, 0);
                    for c in 0..columns {
                        let here: Vec<usize> =
                            (0..wrong.len()).filter(|&n| in_column(n, c)).collect();
                        looked += usize::from(!here.is_empty());
                        if here.iter().any(|&n| !known[n]) {
                            searched += 1;
                            here.iter().for_each(|&n| known[n] = true);
                        }
                    }
                    assert_eq!((look.columns, look.searches), (looked, searched), "{case}");
                }
            }
        }
    }

    /// A block longer than one stretch of syndromes: a shard that is wrong in the first and in
    /// the last stretch is found once and recognized again, and two shards that are wrong in
    /// different stretches are more than two spare shards settle.
    #[test]
    fn the_finder_keeps_what_it_found_over_the_stretches_of_a_block() {
        let length = (SYNDROME_BYTES / 2) * 2 + 3;
        let code = Code::new(Field::Gf256, 3).unwrap();
        let data: Vec<Vec<u8>> = (0..3_u8).map(|i| vec![i + 1; length]).collect();
        let data: Vec<&[u8]> = data.iter().map(|bytes| bytes.as_slice()).collect();
        let points: Vec<u16> = (0..5).collect();
        let mut set = code.encode(&data, &points).unwrap();
        let mut finder = Finder::new(Field::Gf256, &points, 2, length).unwrap();
        let look = |finder: &mut Finder, set: &[Buffer]| {
            let shards: Vec<(u16, &[u8])> = (0..)
                .zip(set.iter().map(|bytes| bytes.as_slice()))
                .collect();
            finder.look(&shards)
        };
        assert_eq!(
            look(&mut finder, &set),
            Look {
                settled: true,
                columns: 0,
                searches: 0
            }
        );
        assert!(finder.found().is_empty());
        set[4][5] ^= 0x10;
        set[4][length - 1] ^= 0x01;
        assert_eq!(
            look(&mut finder, &set),
            Look {
                settled: true,
                columns: 2,
                searches: 1
            }
        );
        assert_eq!(finder.found(), [4]);
        set[4][length - 1] ^= 0x01;
        set[0][length - 1] ^= 0x01;
        assert_eq!(
            look(&mut finder, &set),
            Look {
                settled: false,
                columns: 2,
                searches: 2
            }
        );
    }

    #[test]
    fn a_code_word_has_no_syndromes() {
        let code = Code::new(Field::Gf256, 3).unwrap();
        let data: [&[u8]; 3] = [&[0x46], &[0x91], &[0x23]];
        let word: Vec<u8> = code
            .encode(&data, &[0, 1, 2, 3, 4, 5, 6])
            .unwrap()
            .iter()
            .flat_map(|shard| shard.iter().copied())
            .collect();
        assert_eq!(syndromes_of(&word, 3), [0; 4]);
    }
}
