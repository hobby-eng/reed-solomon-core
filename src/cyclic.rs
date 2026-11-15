//! The cyclic code: a shortened systematic Reed–Solomon code whose shards after the data are the
//! remainder of the data divided by a generator, as QR codes and printed repair words make them.
//! A word of the code is one symbol of each shard, at the same place.

mod decode;

use crate::buffer::Buffer;
use crate::code::{CodeError, in_field, one_length};
use crate::correct::{Change, ChangeKind, Changes, Correction, Mark, NoMemory, Plan};
use crate::correct::{remove_error, roomy, sorted_marks, stripe_bytes, value};
use crate::field::Field;
use alloc::vec::Vec;
use core::ops::Deref;
use decode::{Word, Work};

/// The element `x`: in a field given by a primitive polynomial it generates every other nonzero
/// element, and it is the α of the code.
const ALPHA: u16 = 2;

/// A shortened systematic cyclic Reed–Solomon code: `data` shards, then `parity` more that hold
/// the remainder of the data divided by Π (x + β^(first_root + i)), β = α^root_step, α = x. Each
/// word is put right while `2e + s ≤ parity`. Unlike [`crate::Code`]'s, its shards have fixed
/// places: a lost one is filled where it stands, and none can be added later. The README says
/// which to choose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CyclicCode {
    word: Word,
    data: usize,
    /// The generator, its coefficients from x^parity down to x^0.
    generator: Vec<u16>,
}

impl CyclicCode {
    /// The code of `data` shards and `parity` more over `field`, with the roots β^(first_root + i)
    /// for i below `parity` and β = α^root_step. Fails with [`CodeError::Shape`] for no data or no
    /// parity, for [`Field::Gf65536`], where x does not generate the field, and for words longer
    /// than the order of β, where roots or places would repeat; with [`CodeError::Memory`] when
    /// the generator gets no memory.
    pub fn new(
        field: Field,
        data: usize,
        parity: usize,
        first_root: u32,
        root_step: u32,
    ) -> Result<Self, CodeError> {
        let elements = field.order() - 1;
        let step = usize::try_from(root_step).unwrap_or(usize::MAX) % elements;
        let order = elements / gcd(step, elements);
        let length = data.checked_add(parity).filter(|&length| length <= order);
        let fits = !matches!(field, Field::Gf65536) && data > 0 && parity > 0 && length.is_some();
        if !fits {
            return Err(CodeError::Shape);
        }
        let beta = power(field, ALPHA, step);
        let first = usize::try_from(first_root).unwrap_or(usize::MAX) % order;
        let mut generator = roomy(parity + 1).map_err(|_| CodeError::Memory)?;
        generator.push(1);
        let mut root = power(field, beta, first);
        for _ in 0..parity {
            // Times (x + root): every coefficient takes root times the one before it.
            generator.push(0);
            for at in (1..generator.len()).rev() {
                let before = generator.get(at - 1).copied().unwrap_or(0);
                if let Some(slot) = generator.get_mut(at) {
                    *slot ^= field.multiply(root, before);
                }
            }
            root = field.multiply(root, beta);
        }
        let length = data + parity;
        let word = Word {
            field,
            length,
            parity,
            first,
            beta,
        };
        Ok(Self {
            word,
            data,
            generator,
        })
    }

    /// The field of the symbols.
    #[must_use]
    pub const fn field(&self) -> Field {
        self.word.field
    }

    /// The number of data shards, which come first.
    #[must_use]
    pub const fn data(&self) -> usize {
        self.data
    }

    /// The number of parity shards, which follow the data.
    #[must_use]
    pub const fn parity(&self) -> usize {
        self.word.parity
    }

    /// The number of shards of a word: data and parity.
    #[must_use]
    pub const fn length(&self) -> usize {
        self.word.length
    }

    /// The generator, its coefficients from x^parity, which is 1, down to x^0.
    #[must_use]
    pub fn generator(&self) -> &[u16] {
        &self.generator
    }

    /// The `parity` shards of `data`, in their order: exactly `data` shards of one length, a whole
    /// number of symbols, each an element of the field. Counts and lengths are refused before
    /// symbols, as in [`crate::Code`].
    pub fn encode(&self, data: &[&[u8]]) -> Result<Vec<Buffer>, CodeError> {
        let length = self.measured(data, self.data)?;
        self.canonical(data)?;
        let mut parity = roomy(self.parity()).map_err(|_| CodeError::Memory)?;
        for _ in 0..self.parity() {
            parity.push(Buffer::zeros(length).ok_or(CodeError::Memory)?);
        }
        // x^(parity + t) mod the generator, for t = 0, 1, …: the parity a 1 makes at the data
        // place data − 1 − t. Its coefficients follow from the generator, which is public.
        let mut remainder = roomy(self.parity()).map_err(|_| CodeError::Memory)?;
        remainder.extend(self.generator.iter().skip(1));
        for source in data.iter().rev() {
            for (shard, &factor) in parity.iter_mut().zip(remainder.iter()) {
                self.field().add_scaled(shard, source, factor);
            }
            self.times_x(&mut remainder);
        }
        Ok(parity)
    }

    /// Whether every word of `shards`, all `length` of them in their order, is a word of the code:
    /// it stops at the first stripe of words that holds one that is not. Fails as
    /// [`CyclicCode::correct`] does.
    pub fn agrees(&self, shards: &[&[u8]]) -> Result<bool, CodeError> {
        let columns = self.checked(shards)?;
        let (width, mut syndromes) = self.stripes(columns)?;
        let symbol = self.field().symbol_bytes();
        for start in (0..columns).step_by(width) {
            let span = (start + width).min(columns) * symbol - start * symbol;
            self.stripe(shards, start * symbol, &mut syndromes, span);
            let filled = syndromes.get(..self.parity() * span).unwrap_or_default();
            if filled.iter().any(|&byte| byte != 0) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Corrects wrong symbols in the words of `shards`, all `length` of them in their order, in
    /// place: up to `parity / 2` in each word. Either every word is put right or nothing changes;
    /// one pass counts the changes, a second makes them, and neither keeps a list. Fails on another
    /// number of shards, unequal lengths, a symbol outside the field, or with [`CodeError::Memory`];
    /// never [`Correction::Unchecked`], since a word always has parity.
    pub fn correct(&self, shards: &mut [&mut [u8]]) -> Result<Correction, CodeError> {
        if shards.len() != self.length() {
            return Err(CodeError::Count);
        }
        let mut changed = roomy(shards.len()).map_err(|_| CodeError::Memory)?;
        changed.resize(shards.len(), 0_usize);
        let count = |_: &mut [&mut [u8]], _: usize, _: &[usize], work: &Work| {
            for &place in work.places.iter() {
                if let Some(count) = changed.get_mut(place) {
                    *count += 1;
                }
            }
            Ok(())
        };
        let columns = self.each_word(shards, &[], count)?;
        if columns > 0 {
            return Ok(Correction::Unresolved { columns });
        }
        if changed.iter().all(|&count| count == 0) {
            return Ok(Correction::Agrees);
        }
        let symbol = self.field().symbol_bytes();
        // A word's syndromes are its own: putting it right leaves the rest of its stripe as read.
        let make = |shards: &mut [&mut [u8]], column: usize, _: &[usize], work: &Work| {
            for (&place, &value) in work.places.iter().zip(work.values.iter()) {
                if let Some(shard) = shards.get_mut(place) {
                    remove_error(shard, column * symbol, symbol, value);
                }
            }
            Ok(())
        };
        self.each_word(shards, &[], make)?;
        Ok(Correction::Corrected(changed))
    }

    /// The changes that would put the words of `shards` right, with `marks` unreadable; nothing is
    /// changed until [`CyclicCode::apply`]. A lost shard is given as symbols of the field, such as
    /// zeros, every one of them marked. Every marked symbol is listed, as [`ChangeKind::Filled`], so
    /// the list grows with the marks and the changes. Fails as [`CyclicCode::correct`] does, or
    /// with [`CodeError::Mark`] for a mark outside the shards; never [`Plan::Unchecked`].
    pub fn plan(&self, shards: &[&[u8]], marks: &[Mark]) -> Result<Plan, CodeError> {
        if shards.len() != self.length() {
            return Err(CodeError::Count);
        }
        let mut view = roomy(shards.len()).map_err(|_| CodeError::Memory)?;
        view.extend_from_slice(shards);
        let mut changes = Changes::default();
        let list = |shards: &mut [&[u8]], column: usize, erased: &[usize], work: &Work| {
            self.record(shards, column, erased, work, &mut changes)
        };
        let columns = self.each_word(&mut view, marks, list)?;
        Ok(if columns > 0 {
            Plan::Unresolved { columns }
        } else if changes.is_empty() {
            Plan::Agrees
        } else {
            Plan::Changes(changes)
        })
    }

    /// Makes `changes`, as [`crate::Code::apply`] does: every change is checked first, and if one
    /// does not fit, nothing is changed and it fails with [`CodeError::Changes`]; with
    /// [`CodeError::Memory`] when the list of the shards gets no memory.
    pub fn apply(&self, shards: &mut [&mut [u8]], changes: &Changes) -> Result<(), CodeError> {
        let mut places = roomy(shards.len()).map_err(|_| CodeError::Memory)?;
        places.extend((0..=u16::MAX).zip(shards.iter_mut().map(|shard| &mut **shard)));
        crate::correct::apply_changes(self.field(), &mut places, changes)
    }

    /// The length in bytes of `shards`, which must be `count` shards of one length, a whole
    /// number of symbols.
    fn measured<S: Deref<Target = [u8]>>(
        &self,
        shards: &[S],
        count: usize,
    ) -> Result<usize, CodeError> {
        if shards.len() != count {
            return Err(CodeError::Count);
        }
        one_length(self.field(), shards.iter().map(|shard| shard.len()))
    }

    /// Whether every symbol of `shards` is an element of the field; every shard is looked at.
    fn canonical<S: Deref<Target = [u8]>>(&self, shards: &[S]) -> Result<(), CodeError> {
        in_field(self.field(), shards.iter().map(|shard| &**shard))
    }

    /// The number of words of `shards`, all `length` of them, once their count, lengths and
    /// symbols are checked.
    fn checked<S: Deref<Target = [u8]>>(&self, shards: &[S]) -> Result<usize, CodeError> {
        let length = self.measured(shards, self.length())?;
        self.canonical(shards)?;
        Ok(length / self.field().symbol_bytes())
    }

    /// How many words a stripe takes, and the protected buffer for its syndromes: a few MiB at
    /// most, whatever the length of the shards.
    fn stripes(&self, columns: usize) -> Result<(usize, Buffer), CodeError> {
        let symbol = self.field().symbol_bytes();
        let width = (stripe_bytes(self.field(), self.parity()) / symbol).min(columns.max(1));
        let syndromes = Buffer::zeros(self.parity() * width * symbol).ok_or(CodeError::Memory)?;
        Ok((width, syndromes))
    }

    /// `remainder` times x, modulo the generator: a shift up, and the generator's tail times what
    /// left the top.
    fn times_x(&self, remainder: &mut [u16]) {
        let top = remainder.first().copied().unwrap_or(0);
        remainder.rotate_left(1);
        if let Some(last) = remainder.last_mut() {
            *last = 0;
        }
        for (slot, &g) in remainder.iter_mut().zip(self.generator.iter().skip(1)) {
            *slot ^= self.field().multiply(top, g);
        }
    }

    /// Puts each word of `shards` right in turn, with `marks` unreadable, and hands every word that
    /// changes to `found`: the shards, its column, its marked places and the work that holds what
    /// was found. Returns how many words cannot be put right; those are not handed over. The
    /// syndromes are made a stripe of words at a time.
    fn each_word<S: Deref<Target = [u8]>>(
        &self,
        shards: &mut [S],
        marks: &[Mark],
        mut found: impl FnMut(&mut [S], usize, &[usize], &Work) -> Result<(), CodeError>,
    ) -> Result<usize, CodeError> {
        let length = self.measured(shards, self.length())?;
        let symbol = self.field().symbol_bytes();
        let columns = length / symbol;
        let marked = sorted_marks(marks, self.length(), columns)?;
        self.canonical(shards)?;
        let (width, mut syndromes) = self.stripes(columns)?;
        let mut work = Work::new(self.parity()).map_err(|_| CodeError::Memory)?;
        let mut erased = roomy(self.length()).map_err(|_| CodeError::Memory)?;
        let (mut unresolved, mut next) = (0, 0);
        for start in (0..columns).step_by(width) {
            let end = (start + width).min(columns);
            let span = (end - start) * symbol;
            self.stripe(shards, start * symbol, &mut syndromes, span);
            for column in start..end {
                erased.clear();
                while let Some(mark) = marked.get(next).filter(|mark| mark.symbol == column) {
                    erased.push(mark.shard);
                    next += 1;
                }
                let offset = (column - start) * symbol;
                let words =
                    self.column(shards, &syndromes, span, offset, column, &erased, &mut work);
                match words {
                    None => unresolved += 1,
                    Some(()) if work.places.is_empty() => {}
                    Some(()) => found(shards, column, &erased, &work)?,
                }
            }
        }
        Ok(unresolved)
    }

    /// The syndromes of the words in the `span` bytes of the shards from byte `at`, one buffer of
    /// `span` bytes for each: S_l = Σ c_i X_i^(first + l), X_i = β^(length − 1 − i).
    fn stripe<S: Deref<Target = [u8]>>(
        &self,
        shards: &[S],
        at: usize,
        syndromes: &mut [u8],
        span: usize,
    ) {
        let field = self.field();
        syndromes.fill(0);
        let mut x = 1;
        for shard in shards.iter().rev() {
            let source = shard.get(at..at + span).unwrap_or_default();
            let mut factor = power(field, x, self.word.first);
            for out in syndromes.chunks_exact_mut(span).take(self.parity()) {
                field.add_scaled(out, source, factor);
                factor = field.multiply(factor, x);
            }
            x = field.multiply(x, self.word.beta);
        }
    }

    /// Puts the word at `column` right: its syndromes from the stripe, the marked symbols taken
    /// as zero, then the search. `None` when it cannot be put right.
    #[allow(
        clippy::too_many_arguments,
        reason = "one word's parts, read in one place"
    )]
    fn column<S: Deref<Target = [u8]>>(
        &self,
        shards: &[S],
        syndromes: &[u8],
        span: usize,
        offset: usize,
        column: usize,
        erased: &[usize],
        work: &mut Work,
    ) -> Option<()> {
        let (field, symbol) = (self.field(), self.field().symbol_bytes());
        work.syndromes.clear();
        for stripe in syndromes.chunks_exact(span).take(self.parity()) {
            work.syndromes
                .push(stripe.get(offset..offset + symbol).map_or(0, value));
        }
        // The symbols read at marked places count as zero.
        for &place in erased {
            let read = symbol_at(shards, place, column, symbol);
            let x = self.word.locator(place);
            let mut factor = field.multiply(read, power(field, x, self.word.first));
            for syndrome in work.syndromes.iter_mut() {
                *syndrome ^= factor;
                factor = field.multiply(factor, x);
            }
        }
        if erased.is_empty() && work.syndromes.iter().all(|&s| s == 0) {
            work.places.clear();
            return Some(());
        }
        self.word.errata(work, erased).then_some(())
    }

    /// Adds the changes of the word at `column`, as `work` found them, in the order of the places.
    fn record<S: Deref<Target = [u8]>>(
        &self,
        shards: &[S],
        column: usize,
        erased: &[usize],
        work: &Work,
        changes: &mut Changes,
    ) -> Result<(), CodeError> {
        let symbol = self.field().symbol_bytes();
        for (&place, &value) in work.places.iter().zip(work.values.iter()) {
            let was = symbol_at(shards, place, column, symbol);
            let marked = erased.contains(&place);
            let (becomes, kind) = match marked {
                true => (value, ChangeKind::Filled),
                false => (was ^ value, ChangeKind::Replaced),
            };
            let change = Change {
                shard: place,
                symbol: column,
                was,
                becomes,
                kind,
            };
            changes.push(change).map_err(|_| CodeError::Memory)?;
        }
        Ok(())
    }
}

/// The symbol at `column` of the shard at `place`, high byte first.
fn symbol_at<S: Deref<Target = [u8]>>(
    shards: &[S],
    place: usize,
    column: usize,
    symbol: usize,
) -> u16 {
    let start = column * symbol;
    let bytes = shards
        .get(place)
        .and_then(|shard| shard.get(start..start + symbol));
    bytes.map_or(0, value)
}

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

/// The greatest common divisor; gcd(0, n) is n.
const fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
