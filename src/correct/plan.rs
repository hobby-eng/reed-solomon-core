//! Correcting with symbols marked unreadable, and listing every change before anything changes.
//! A marked symbol costs one spare shard instead of two, because its place is known: it is taken
//! out of the syndromes, and Berlekamp–Massey finds the remaining wrong symbols as usual. A column
//! is fixed whenever `2e + s ≤ m − k`.

use super::changes::{Changes, FINGERPRINT};
use super::{Decoder, Lists, roomy, stripe_bytes};
use crate::buffer::Shortfall;
use crate::code::{Code, CodeError};
use crate::field::Field;
use alloc::vec::Vec;
use core::ops::Deref;
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

/// A symbol known to be unreadable: whatever its bytes hold, it is a gap at this place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mark {
    /// The place of its shard among the shards given.
    pub shard: usize,
    /// The place of the symbol in the shard, counted in symbols from the first.
    pub symbol: usize,
}

/// How a symbol comes to change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    /// The symbol was marked unreadable and gets the value the others give it, which may be the
    /// value it holds.
    Filled,
    /// The symbol was not marked, and the search found it wrong.
    Replaced,
}

/// One symbol that a [`Plan`] changes: its place and how. What it holds and becomes can be a
/// secret, such as a word of a recovery phrase, so a change does not carry them:
/// [`Changes::values`] lets a program look at them where the list keeps them, and the strict
/// mode's `values` does so on a protected stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Change {
    /// The place of its shard among the shards given.
    pub shard: usize,
    /// The place of the symbol in the shard, counted in symbols from the first.
    pub symbol: usize,
    /// Whether it was marked or found.
    pub kind: ChangeKind,
}

/// A change with what the symbol holds and becomes, as the list keeps it; it never leaves the
/// crate.
#[derive(Clone, Copy)]
pub(crate) struct Record {
    pub(crate) shard: usize,
    pub(crate) symbol: usize,
    pub(crate) was: u16,
    pub(crate) becomes: u16,
    pub(crate) kind: ChangeKind,
}

/// What [`Code::plan`] found. Nothing was changed.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// Exactly `threshold` shards and no mark: there is nothing to compare them with, and nothing
    /// was checked.
    Unchecked,
    /// The shards agree and no symbol was marked.
    Agrees,
    /// Every column can be put right by these changes, in the order of the symbols and, within
    /// one, of the shards. Every marked symbol is among them. The list is bound to the shards it
    /// was made for. Whether the result is the original data is for a digest or an
    /// authentication to say. The list wipes itself when it is dropped.
    Changes(Changes),
    /// In `columns` symbol columns the spare shards cannot put the column right: more wrong and
    /// marked symbols than `2e + s ≤ m − k` allows, or more marks than spare shards.
    Unresolved {
        /// How many symbol columns could not be put right.
        columns: usize,
    },
}

impl Code {
    /// Finds the changes that would put `shards` right, with `marks` known to be unreadable, and changes
    /// nothing; [`Code::apply`] makes them. Fixes up to `e` wrong symbols per column while
    /// `2e + s ≤ m − k`. Every marked symbol is listed, as [`ChangeKind::Filled`], even when it already
    /// holds the right value, so a program can show each change first. Fails as [`Code::correct`] does,
    /// or with [`CodeError::Mark`] for a mark outside the shards.
    pub fn plan(&self, shards: &[(u16, &[u8])], marks: &[Mark]) -> Result<Plan, CodeError> {
        let indexes = shards.iter().map(|&(index, _)| index);
        let length = self.check_set(indexes, shards.iter().map(|(_, bytes)| bytes.len()))?;
        let symbols = length / self.field().symbol_bytes();
        let sorted = sorted_marks(marks, shards.len(), symbols)?;
        self.check_symbols(shards.iter().map(|(_, bytes)| *bytes))?;
        let spare = shards.len() - self.threshold();
        if spare == 0 {
            if sorted.is_empty() {
                return Ok(Plan::Unchecked);
            }
            let mut columns = 1;
            for pair in sorted.windows(2) {
                if let [first, second] = pair {
                    columns += usize::from(first.symbol != second.symbol);
                }
            }
            return Ok(Plan::Unresolved { columns });
        }
        if length == 0 {
            return Ok(Plan::Agrees);
        }
        let mut points = roomy(shards.len()).map_err(CodeError::from)?;
        points.extend(shards.iter().map(|&(index, _)| index));
        let mut decoder = Decoder::with_room(self.field(), points, spare, length, spare)
            .map_err(CodeError::from)?;
        let reading = shards.iter().map(|&(index, bytes)| (index, bytes));
        let print = Zeroizing::new(fingerprint(&self.identity(), reading));
        let mut changes = Changes::bound(&print).map_err(CodeError::from)?;
        let unresolved = decoder
            .plan_pass(shards, length, &sorted, &mut changes)
            .map_err(CodeError::from)?;
        if unresolved > 0 {
            return Ok(Plan::Unresolved {
                columns: unresolved,
            });
        }
        if changes.is_empty() {
            return Ok(Plan::Agrees);
        }
        Ok(Plan::Changes(changes))
    }

    /// Makes `changes`, the list [`Code::plan`] found, in these shards. Before anything is
    /// written, the shards must be those the plan was made for, the same indexes in the same
    /// order and every byte as it was, and the code this one; otherwise nothing is changed and
    /// it fails with [`CodeError::Changes`].
    pub fn apply(
        &self,
        shards: &mut [(u16, &mut [u8])],
        changes: &Changes,
    ) -> Result<(), CodeError> {
        let reading = shards.iter().map(|(index, bytes)| (*index, &**bytes));
        let print = Zeroizing::new(fingerprint(&self.identity(), reading));
        apply_changes(self.field(), shards, changes, &print)
    }

    /// What names this code in the fingerprint of a plan.
    fn identity(&self) -> [u8; 14] {
        let mut identity = [0; 14];
        let threshold = u64::try_from(self.threshold())
            .unwrap_or(u64::MAX)
            .to_le_bytes();
        let fields = (*b"P")
            .into_iter()
            .chain(self.field().tag())
            .chain(threshold);
        for (slot, byte) in identity.iter_mut().zip(fields) {
            *slot = byte;
        }
        identity
    }
}

/// The fingerprint of the shards a plan is made for: the SHA-256 of what names the code, then
/// each shard's index or place, length and bytes. It follows from the data, so it lies only in the
/// plan's own buffer, and `apply` compares it before it writes.
pub(crate) fn fingerprint<'a>(
    code: &[u8],
    shards: impl Iterator<Item = (u16, &'a [u8])>,
) -> [u8; FINGERPRINT] {
    let mut hash = Sha256::new();
    hash.update(b"reed-solomon-core plan\0");
    hash.update(code);
    for (index, bytes) in shards {
        hash.update(index.to_le_bytes());
        hash.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_le_bytes());
        hash.update(bytes);
    }
    hash.finalize().into()
}

/// Makes `changes` in `shards`, given by place, once `fingerprint`, that of these shards, is the
/// one the plan was made for: then every change is checked as well, and if one does not fit,
/// nothing is changed. Both codes apply their plans this way.
pub(crate) fn apply_changes(
    field: Field,
    shards: &mut [(u16, &mut [u8])],
    changes: &Changes,
    fingerprint: &[u8; FINGERPRINT],
) -> Result<(), CodeError> {
    let bound = changes.fingerprint();
    let differ = bound
        .iter()
        .zip(fingerprint)
        .fold(0, |differ, (own, given)| differ | (own ^ given));
    if bound.len() != FINGERPRINT || differ != 0 {
        return Err(CodeError::Changes);
    }
    let symbol = field.symbol_bytes();
    let order = field.order();
    let fits = (0..changes.len()).all(|at| {
        changes.record(at).is_some_and(|record| {
            let held = place(shards, record.shard, record.symbol, symbol).map(value);
            held == Some(record.was) && usize::from(record.becomes) < order
        })
    });
    if !fits {
        return Err(CodeError::Changes);
    }
    for record in (0..changes.len()).filter_map(|at| changes.record(at)) {
        let start = record.symbol * symbol;
        let bytes = shards
            .get_mut(record.shard)
            .and_then(|(_, bytes)| bytes.get_mut(start..start + symbol));
        let new = record.becomes.to_be_bytes();
        for (byte, &b) in bytes
            .into_iter()
            .flatten()
            .zip(new.iter().skip(new.len() - symbol))
        {
            *byte = b;
        }
    }
    Ok(())
}

/// The bytes of the symbol at place `at` of the shard at place `shard`, when the shards have it.
fn place<'s>(
    shards: &'s [(u16, &mut [u8])],
    shard: usize,
    at: usize,
    symbol: usize,
) -> Option<&'s [u8]> {
    let start = at.checked_mul(symbol)?;
    let (_, bytes) = shards.get(shard)?;
    bytes.get(start..start.checked_add(symbol)?)
}

/// `marks` sorted by symbol and shard, each once, once each is known to lie inside `shards`
/// shards of `symbols` symbols: [`CodeError::Mark`] for one outside.
pub(crate) fn sorted_marks(
    marks: &[Mark],
    shards: usize,
    symbols: usize,
) -> Result<Vec<Mark>, CodeError> {
    let outside = |mark: &Mark| mark.shard >= shards || mark.symbol >= symbols;
    if marks.iter().any(outside) {
        return Err(CodeError::Mark);
    }
    let mut sorted = roomy(marks.len()).map_err(|_| CodeError::Memory)?;
    sorted.extend_from_slice(marks);
    sorted.sort_unstable_by_key(|mark| (mark.symbol, mark.shard));
    sorted.dedup();
    Ok(sorted)
}

/// The value of a symbol's bytes, high byte first.
pub(crate) fn value(bytes: &[u8]) -> u16 {
    bytes
        .iter()
        .fold(0, |value, &byte| value << 8 | u16::from(byte))
}

impl Decoder {
    /// Finds the wrong symbols of the column whose syndromes are in `lists.column`, with the
    /// symbols of the shards in `self.erased` marked, and leaves in `lists.errors` first the wrong
    /// symbols found, then the marked ones, each with its error. False when the column cannot be
    /// put right.
    fn locate_marked(&mut self, lists: &mut Lists<'_>) -> bool {
        let field = self.field;
        let marked = self.erased.len();
        if marked > self.spare {
            return false;
        }
        // G(X) = Π (X − x) over the marked points: times (X − x), each coefficient becomes the
        // one below it plus x times itself.
        self.erasure.clear();
        self.erasure.push(1);
        for &position in &self.erased {
            let point = self.points.get(position).copied().unwrap_or(0);
            let mut below = 0;
            for coefficient in &mut self.erasure {
                let old = *coefficient;
                *coefficient = below ^ field.multiply(point, old);
                below = old;
            }
            self.erasure.push(below);
        }
        // The syndromes with the marked symbols taken out.
        lists.sequence.clear();
        for j in 0..self.spare - marked {
            let terms = self.erasure.iter().zip(lists.column.iter().skip(j));
            let sum = terms.fold(0, |sum, (&g, &s)| sum ^ field.multiply(g, s));
            lists.sequence.push(sum);
        }
        let length = self.recurrence(lists, true);
        lists.errors.clear();
        if 2 * length > self.spare - marked {
            return false;
        }
        // The roots of X^L · C(1/X) among the shards not marked, as in `locate`.
        let connection = lists.connection.get(..=length).unwrap_or_default();
        for (position, &point) in self.points.iter().enumerate() {
            if self.erased.contains(&position) {
                continue;
            }
            let value = connection
                .iter()
                .fold(0, |value, &c| field.multiply(value, point) ^ c);
            if value == 0 {
                lists.errors.push(position, 0);
            }
        }
        if lists.errors.len() != length {
            return false;
        }
        for &position in &self.erased {
            lists.errors.push(position, 0);
        }
        self.values(lists, length)
    }

    /// Goes through the shards stripe by stripe and finds, for every column that disagrees or has
    /// a mark, the changes that put it right, appending them to `changes`. `marks` is sorted by
    /// symbol and holds each mark once. Returns how many columns it could not resolve.
    fn plan_pass<B: Deref<Target = [u8]>>(
        &mut self,
        shards: &[(u16, B)],
        length: usize,
        marks: &[Mark],
        changes: &mut Changes,
    ) -> Result<usize, Shortfall> {
        let symbol = self.field.symbol_bytes();
        let stripe = stripe_bytes(self.field, self.spare);
        let mut syndromes = core::mem::take(&mut self.syndromes);
        let mut factors = core::mem::take(&mut self.factors);
        let mut room = core::mem::take(&mut self.room);
        let mut lists = Lists::cut(&mut room, self.spare, self.most);
        let mut marks = marks.iter().peekable();
        let mut unresolved = 0;
        let mut start = 0;
        while start < length {
            let size = stripe.min(length - start);
            let differ = self.fill_syndromes(shards, start, size, &mut factors, &mut syndromes);
            let end = (start + size) / symbol;
            if !differ && marks.peek().is_none_or(|mark| mark.symbol >= end) {
                start += size;
                continue;
            }
            for offset in (0..size).step_by(symbol) {
                let at = (start + offset) / symbol;
                self.erased.clear();
                let mut over = false;
                while let Some(mark) = marks.next_if(|mark| mark.symbol == at) {
                    if self.erased.len() < self.spare {
                        self.erased.push(mark.shard);
                    } else {
                        over = true;
                    }
                }
                let marked = over || !self.erased.is_empty();
                let disagrees =
                    (differ || marked) && self.read_column(&mut lists, &syndromes, size, offset);
                if !disagrees && !marked {
                    continue;
                }
                let found = if over {
                    None
                } else if marked {
                    self.locate_marked(&mut lists)
                        .then(|| lists.errors.len() - self.erased.len())
                } else {
                    self.locate(&mut lists).then_some(lists.errors.len())
                };
                let Some(found) = found else {
                    unresolved += 1;
                    continue;
                };
                // The symbols found and the marked ones each come in the order of the shards:
                // the two runs are merged, so that the whole list is in that order.
                let (found, marked) = lists.errors.split(found);
                let (mut found, mut marked) = (found.iter().peekable(), marked.iter().peekable());
                loop {
                    let first = match (found.peek(), marked.peek()) {
                        (Some(replaced), Some(filled)) => replaced[0] < filled[0],
                        (Some(_), None) => true,
                        (None, Some(_)) => false,
                        (None, None) => break,
                    };
                    let (next, kind) = match first {
                        true => (found.next(), ChangeKind::Replaced),
                        false => (marked.next(), ChangeKind::Filled),
                    };
                    let Some(&[position, error]) = next else {
                        break;
                    };
                    let position = usize::from(position);
                    let begin = start + offset;
                    let bytes = shards
                        .get(position)
                        .and_then(|(_, bytes)| bytes.get(begin..begin + symbol))
                        .unwrap_or_default();
                    let was = value(bytes);
                    changes.push(Record {
                        shard: position,
                        symbol: at,
                        was,
                        becomes: was ^ error,
                        kind,
                    })?;
                }
            }
            start += size;
        }
        self.syndromes = syndromes;
        self.factors = factors;
        self.room = room;
        Ok(unresolved)
    }
}
