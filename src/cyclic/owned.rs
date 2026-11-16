//! The cyclic code in memory of its own: the check shards in buffers, the check, the correction
//! and the plan of changes for shards of any length, a stripe of words at a time, with marked
//! symbols and whole shards lost.

use super::decode::{Word, Work};
use super::words::CyclicWords;
use crate::buffer::{Buffer, Numbers};
use crate::code::CodeError;
use crate::correct::{ChangeKind, Changes, Correction, Mark, Plan, Record};
use crate::correct::{fingerprint, remove_error, roomy, sorted_marks, stripe_bytes, value};
use crate::field::Field;
use alloc::vec::Vec;
use core::ops::Deref;
use zeroize::Zeroizing;

/// A shortened systematic cyclic Reed–Solomon code: `data` shards, then `parity` more that hold
/// the remainder of the data divided by Π (x + β^(first_root + i)), β = α^root_step, α = x. Each
/// word is put right while `2e + s ≤ parity`. Unlike [`crate::Code`]'s, its shards have fixed
/// places: a lost one is filled where it stands, and none can be added later. The README says
/// which to choose; [`CyclicWords`] is the same code without an allocator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CyclicCode {
    word: Word,
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
        let word = Word::new(field, data, parity, first_root, root_step)?;
        let mut generator = roomy(parity + 1).map_err(|_| CodeError::Memory)?;
        generator.resize(parity + 1, 0);
        word.generator_into(&mut generator)?;
        Ok(Self { word, generator })
    }

    /// The same code over this one's generator, for its calls without an allocator.
    #[must_use]
    pub fn words(&self) -> CyclicWords<'_> {
        CyclicWords::of(self.word.clone(), &self.generator)
    }

    /// The field of the symbols.
    #[must_use]
    pub const fn field(&self) -> Field {
        self.word.field
    }

    /// The number of data shards, which come first.
    #[must_use]
    pub const fn data(&self) -> usize {
        self.word.length - self.word.parity
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
        let words = self.words();
        let length = words.measured(data, self.data())?;
        words.canonical(data, &[])?;
        let mut parity = roomy(self.parity()).map_err(|_| CodeError::Memory)?;
        for _ in 0..self.parity() {
            parity.push(Buffer::zeros(length)?);
        }
        let mut remainder = roomy(self.parity()).map_err(|_| CodeError::Memory)?;
        remainder.resize(self.parity(), 0);
        words.encode_checked(data, &mut parity, &mut remainder);
        Ok(parity)
    }

    /// Whether every word of `shards`, all `length` of them in their order, is a word of the code:
    /// it stops at the first stripe of words that holds one that is not. Fails on another number
    /// of shards, unequal lengths, a symbol outside the field, or without memory.
    pub fn agrees(&self, shards: &[&[u8]]) -> Result<bool, CodeError> {
        let length = self.words().measured(shards, self.length())?;
        let (_, mut syndromes) = self.stripes(length / self.field().symbol_bytes())?;
        self.words().agrees(shards, &mut syndromes)
    }

    /// Corrects the words of `shards`, all `length` of them in their order, in place: the shards at
    /// the places `lost` are written anew, and wrong symbols elsewhere put right, while `2e + s ≤
    /// parity` with `s` the lost shards. Either every word is put right or nothing changes; neither
    /// pass keeps a list. Fails on another number of shards, unequal lengths, a lost place outside
    /// the word, a symbol of a shard not lost outside the field, or without memory.
    pub fn correct(
        &self,
        shards: &mut [&mut [u8]],
        lost: &[usize],
    ) -> Result<Correction, CodeError> {
        if shards.len() != self.length() {
            return Err(CodeError::Count);
        }
        // Counts of wrong symbols follow from the data: wiped unless the answer hands them over.
        let mut changed = Zeroizing::new(roomy(shards.len()).map_err(|_| CodeError::Memory)?);
        changed.resize(shards.len(), 0_usize);
        let count = |_: &mut [&mut [u8]], _: usize, _: &[usize], work: &Work<'_>| {
            for place in work.places() {
                if let Some(count) = changed.get_mut(place) {
                    *count += 1;
                }
            }
            Ok(())
        };
        // Both passes work in the memory the first asks for.
        let mut scratch = None;
        let columns = self.each_word(shards, (lost, &[]), &mut scratch, count)?;
        if columns > 0 {
            return Ok(Correction::Unresolved { columns });
        }
        if changed.iter().all(|&count| count == 0) {
            return Ok(Correction::Agrees);
        }
        let symbol = self.field().symbol_bytes();
        // A word's syndromes are its own: putting it right leaves the rest of its stripe as read.
        let make = |shards: &mut [&mut [u8]], column: usize, erased: &[usize], work: &Work<'_>| {
            for (place, &value) in work.places().zip(work.values.as_slice()) {
                // A lost symbol becomes the value; a wrong one has it added.
                let was = symbol_at(shards, place, column, symbol);
                let marked = erased.binary_search(&place).is_ok();
                let error = if marked { was ^ value } else { value };
                if let Some(shard) = shards.get_mut(place) {
                    remove_error(shard, column * symbol, symbol, error);
                }
            }
            Ok(())
        };
        self.each_word(shards, (lost, &[]), &mut scratch, make)?;
        Ok(Correction::Corrected(core::mem::take(&mut *changed)))
    }

    /// The changes that would put the words of `shards` right, with the shards at the places
    /// `lost` and the symbols `marks` unreadable; nothing is changed until [`CyclicCode::apply`].
    /// Every lost and marked symbol is listed, as [`ChangeKind::Filled`], so the list grows with
    /// them. Fails as [`CyclicCode::correct`] does, or with [`CodeError::Mark`] for a mark outside
    /// the shards; never [`Plan::Unchecked`].
    pub fn plan(
        &self,
        shards: &[&[u8]],
        lost: &[usize],
        marks: &[Mark],
    ) -> Result<Plan, CodeError> {
        if shards.len() != self.length() {
            return Err(CodeError::Count);
        }
        let mut view = roomy(shards.len()).map_err(|_| CodeError::Memory)?;
        view.extend_from_slice(shards);
        let reading = (0..=u16::MAX).zip(shards.iter().copied());
        let print = Zeroizing::new(fingerprint(&self.identity(), reading));
        let mut changes = Changes::bound(&print).map_err(CodeError::from)?;
        let list = |shards: &mut [&[u8]], column: usize, erased: &[usize], work: &Work<'_>| {
            record(shards, column, erased, work, &mut changes, self.field())
        };
        let columns = self.each_word(&mut view, (lost, marks), &mut None, list)?;
        Ok(if columns > 0 {
            Plan::Unresolved { columns }
        } else if changes.is_empty() {
            Plan::Agrees
        } else {
            Plan::Changes(changes)
        })
    }

    /// Makes `changes`, the list [`CyclicCode::plan`] found, as [`crate::Code::apply`] does: the
    /// shards must be those the plan was made for, in the same order with every byte as it was,
    /// and the code this one; otherwise nothing is changed and it fails with
    /// [`CodeError::Changes`]. Fails with [`CodeError::Memory`] when the list of the shards gets
    /// no memory.
    pub fn apply(&self, shards: &mut [&mut [u8]], changes: &Changes) -> Result<(), CodeError> {
        let reading = (0..=u16::MAX).zip(shards.iter().map(|shard| &**shard));
        let print = Zeroizing::new(fingerprint(&self.identity(), reading));
        let mut places = roomy(shards.len()).map_err(|_| CodeError::Memory)?;
        places.extend((0..=u16::MAX).zip(shards.iter_mut().map(|shard| &mut **shard)));
        crate::correct::apply_changes(self.field(), &mut places, changes, &print)
    }

    /// What names this code in the fingerprint of a plan.
    fn identity(&self) -> [u8; 32] {
        let word = &self.word;
        let number = |n: usize| u64::try_from(n).unwrap_or(u64::MAX).to_le_bytes();
        let fields = (*b"C").into_iter().chain(word.field.tag());
        let fields = fields.chain(number(word.length)).chain(number(word.parity));
        let fields = fields
            .chain(number(word.first))
            .chain(word.beta.to_le_bytes());
        let mut identity = [0; 32];
        for (slot, byte) in identity.iter_mut().zip(fields) {
            *slot = byte;
        }
        identity
    }

    /// How many words a stripe takes, and the protected buffer for its syndromes: a few MiB at
    /// most, whatever the length of the shards.
    fn stripes(&self, columns: usize) -> Result<(usize, Buffer), CodeError> {
        let symbol = self.field().symbol_bytes();
        let width = (stripe_bytes(self.field(), self.parity()) / symbol).min(columns.max(1));
        let syndromes = Buffer::zeros(self.parity() * width * symbol)?;
        Ok((width, syndromes))
    }

    /// The memory a pass over `columns` words works in, asked for once for both passes of a
    /// correction.
    fn scratch(&self, columns: usize) -> Result<Scratch, CodeError> {
        let (width, syndromes) = self.stripes(columns)?;
        // The lists of the search follow from the data: numbers in a buffer of their own.
        let room = Numbers::zeros(Work::room(self.parity()))?;
        let (mut erased, mut read) = (Vec::new(), Vec::new());
        for list in [&mut erased, &mut read] {
            list.try_reserve_exact(self.length())
                .map_err(|_| CodeError::Memory)?;
        }
        Ok(Scratch {
            width,
            syndromes,
            room,
            erased,
            read,
        })
    }

    /// Puts each word of `shards` right in turn, with the shards `lost` and the symbols `marks`
    /// unreadable, and hands every word that changes to `found`: the shards, its column, its
    /// unreadable places and the work that holds what was found. Returns how many words cannot be
    /// put right; those are not handed over. The syndromes are made a stripe of words at a time.
    fn each_word<S: Deref<Target = [u8]>>(
        &self,
        shards: &mut [S],
        (lost, marks): (&[usize], &[Mark]),
        scratch: &mut Option<Scratch>,
        mut found: impl FnMut(&mut [S], usize, &[usize], &Work<'_>) -> Result<(), CodeError>,
    ) -> Result<usize, CodeError> {
        let words = self.words();
        let symbol = self.field().symbol_bytes();
        let columns = words.measured(shards, self.length())? / symbol;
        let lost = sorted_places(lost, self.length())?;
        let marked = sorted_marks(marks, self.length(), columns)?;
        words.canonical(shards, &lost)?;
        let scratch = match scratch {
            Some(scratch) => scratch,
            None => scratch.insert(self.scratch(columns)?),
        };
        let Scratch {
            width,
            syndromes,
            room,
            erased,
            read,
        } = scratch;
        let width = *width;
        let mut work = Work::new(self.parity(), room).ok_or(CodeError::Memory)?;
        let (mut unresolved, mut next) = (0, 0);
        for start in (0..columns).step_by(width) {
            let end = (start + width).min(columns);
            let span = (end - start) * symbol;
            self.word.stripe(
                shards,
                &lost,
                start * symbol,
                syndromes.as_mut_slice(),
                span,
            );
            for column in start..end {
                // The marked symbols of shards not lost are read and taken out of the syndromes.
                erased.clear();
                read.clear();
                erased.extend_from_slice(&lost);
                while let Some(mark) = marked.get(next).filter(|mark| mark.symbol == column) {
                    if lost.binary_search(&mark.shard).is_err() {
                        erased.push(mark.shard);
                        read.push(mark.shard);
                    }
                    next += 1;
                }
                erased.sort_unstable();
                let offset = (column - start) * symbol;
                let syndromes = (syndromes.as_slice(), span, offset);
                match self.column(shards, syndromes, column, read, erased, &mut work) {
                    None => unresolved += 1,
                    Some(()) if work.places().next().is_none() => {}
                    Some(()) => found(shards, column, erased, &work)?,
                }
            }
        }
        Ok(unresolved)
    }

    /// Puts the word at `column` right: its syndromes from the stripe `(syndromes, span, offset)`,
    /// the symbols at the places `read` taken out, then the search with every place `erased`
    /// unreadable. `None` when it cannot be put right.
    fn column<S: Deref<Target = [u8]>>(
        &self,
        shards: &[S],
        (syndromes, span, offset): (&[u8], usize, usize),
        column: usize,
        read: &[usize],
        erased: &[usize],
        work: &mut Work<'_>,
    ) -> Option<()> {
        let (field, symbol) = (self.field(), self.field().symbol_bytes());
        work.syndromes.clear();
        for stripe in syndromes.chunks_exact(span).take(self.parity()) {
            work.syndromes
                .push(stripe.get(offset..offset + symbol).map_or(0, value));
        }
        for &place in read {
            let x = self.word.locator(place);
            let symbol = symbol_at(shards, place, column, symbol);
            let mut factor = field.multiply(symbol, super::power(field, x, self.word.first));
            for syndrome in work.syndromes.as_mut_slice() {
                *syndrome ^= factor;
                factor = field.multiply(factor, x);
            }
        }
        let clean = work.syndromes.as_slice().iter().all(|&s| s == 0);
        if erased.is_empty() && clean {
            work.found_nothing();
            return Some(());
        }
        self.word.errata(work, erased).then_some(())
    }
}

/// The memory a pass over the words works in: the syndromes of a stripe of `width` words, the
/// room of the search, and the places of a word that are unreadable and that are read.
struct Scratch {
    width: usize,
    syndromes: Buffer,
    room: Numbers,
    erased: Vec<usize>,
    read: Vec<usize>,
}

/// `lost` in rising order, each once; a place outside a word of `length` is refused. A flag per
/// place sorts them, so a long list with repeats takes no more memory than the shards.
fn sorted_places(lost: &[usize], length: usize) -> Result<Vec<usize>, CodeError> {
    if lost.iter().any(|&place| place >= length) {
        return Err(CodeError::Mark);
    }
    let mut flags = roomy(length).map_err(|_| CodeError::Memory)?;
    flags.resize(length, false);
    for &place in lost {
        if let Some(flag) = flags.get_mut(place) {
            *flag = true;
        }
    }
    let count = flags.iter().filter(|&&flag| flag).count();
    let mut sorted = roomy(count).map_err(|_| CodeError::Memory)?;
    let places = flags.iter().enumerate().filter(|&(_, &flag)| flag);
    sorted.extend(places.map(|(place, _)| place));
    Ok(sorted)
}

/// Adds the changes of the word at `column`, as `work` found them, in the order of the places.
fn record<S: Deref<Target = [u8]>>(
    shards: &[S],
    column: usize,
    erased: &[usize],
    work: &Work<'_>,
    changes: &mut Changes,
    field: Field,
) -> Result<(), CodeError> {
    let symbol = field.symbol_bytes();
    for (place, &value) in work.places().zip(work.values.as_slice()) {
        let was = symbol_at(shards, place, column, symbol);
        let (becomes, kind) = match erased.binary_search(&place).is_ok() {
            true => (value, ChangeKind::Filled),
            false => (was ^ value, ChangeKind::Replaced),
        };
        let record = Record {
            shard: place,
            symbol: column,
            was,
            becomes,
            kind,
        };
        changes.push(record).map_err(CodeError::from)?;
    }
    Ok(())
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
