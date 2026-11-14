//! Correcting with symbols marked unreadable, and saying what would change before anything does.
//!
//! A symbol marked unreadable is a gap at a known place: it costs one spare shard of its column,
//! where a wrong symbol at an unknown place costs two. Its contribution is taken out of the
//! syndromes: with `G(X) = Π (X − x)` over the points of the marked symbols, the sums
//! `T_j = Σ_i g_i · S_(j+i)` for `j` below `m − k − s` are the power sums of the wrong symbols
//! alone, each scaled by `G` at its point, which is not zero. Berlekamp–Massey finds those
//! symbols in them as without marks, and the error of every symbol of the column, marked or
//! found, follows from the syndromes by the one solver of [`Code::correct`]. A column is put
//! right whenever `2e + s ≤ m − k`.

use super::{Decoder, roomy};
use crate::code::{Code, CodeError};
use alloc::alloc::handle_alloc_error;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::ops::Deref;

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

/// One symbol that a [`Plan`] changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Change {
    /// The place of its shard among the shards given.
    pub shard: usize,
    /// The place of the symbol in the shard, counted in symbols from the first.
    pub symbol: usize,
    /// What the symbol holds now.
    pub was: u16,
    /// What it becomes.
    pub becomes: u16,
    /// Whether it was marked or found.
    pub kind: ChangeKind,
}

/// What [`Code::plan`] found. Nothing was changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    /// Exactly `threshold` shards and no mark: there is nothing to compare them with, and nothing
    /// was checked.
    Unchecked,
    /// The shards agree and no symbol was marked.
    Agrees,
    /// Every column can be put right by these changes, in the order of the symbols and, within
    /// one, of the shards. Every marked symbol is among them. Whether the result is the original
    /// data is for a digest or an authentication to say.
    Changes(Vec<Change>),
    /// In `columns` symbol columns the spare shards cannot put the column right: more wrong and
    /// marked symbols than `2e + s ≤ m − k` allows, or more marks than spare shards.
    Unresolved {
        /// How many symbol columns could not be put right.
        columns: usize,
    },
}

impl Code {
    /// Finds what would put `shards`, given as `(index, bytes)`, right, with the symbols of
    /// `marks` known to be unreadable, and changes nothing. With `m` shards, the threshold `k`
    /// and `s` marked symbols in a column, up to `e` wrong symbols are found there wherever they
    /// are, as long as `2e + s ≤ m − k`. A shard that is missing altogether is simply not given,
    /// as for [`Code::correct`]; a mark is for a shard that is there and has a gap. A mark given
    /// twice counts once. [`Code::apply`] makes the changes.
    ///
    /// A marked symbol may hold any element of the field: its value is not used. Every marked
    /// symbol appears among the changes, as [`ChangeKind::Filled`], also when it already holds
    /// the right value; a program shows the person what each place becomes before it applies.
    ///
    /// The working memory is that of [`Code::correct`] with room for `m − k` symbols of a column
    /// instead of half as many, a copy of the marks and the changes found: at most the marks and
    /// `(m − k) / 2` symbols for every column.
    ///
    /// # Errors
    ///
    /// As [`Code::correct`], and [`CodeError::Mark`] for a mark outside the shards, reported
    /// after the counts, the indexes and the lengths and before a symbol that is not an element
    /// of the field.
    pub fn plan(&self, shards: &[(u16, &[u8])], marks: &[Mark]) -> Result<Plan, CodeError> {
        let indexes = shards.iter().map(|&(index, _)| index);
        let length = self.check_set(indexes, shards.iter().map(|(_, bytes)| bytes.len()))?;
        let symbol = self.field().symbol_bytes();
        let symbols = length / symbol;
        if marks
            .iter()
            .any(|mark| mark.shard >= shards.len() || mark.symbol >= symbols)
        {
            return Err(CodeError::Mark);
        }
        self.check_symbols(shards.iter().map(|(_, bytes)| *bytes))?;
        let Ok(mut sorted) = roomy(marks.len()) else {
            handle_alloc_error(Layout::array::<Mark>(marks.len()).unwrap_or(Layout::new::<Mark>()))
        };
        sorted.extend_from_slice(marks);
        sorted.sort_unstable_by_key(|mark| (mark.symbol, mark.shard));
        sorted.dedup();
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
        let points: Vec<u16> = shards.iter().map(|&(index, _)| index).collect();
        let Ok(mut decoder) = Decoder::with_room(self.field(), points, spare, length, spare) else {
            handle_alloc_error(Layout::new::<Decoder>())
        };
        let mut changes = Vec::new();
        let unresolved = decoder.plan_pass(shards, length, &sorted, &mut changes);
        if unresolved > 0 {
            return Ok(Plan::Unresolved {
                columns: unresolved,
            });
        }
        if changes.is_empty() {
            return Ok(Plan::Agrees);
        }
        changes.sort_unstable_by_key(|change| (change.symbol, change.shard));
        Ok(Plan::Changes(changes))
    }

    /// Makes `changes`, as [`Code::plan`] found them for these shards, in the same order. Every
    /// change is checked before the first is made: its place must be in the shards, the symbol
    /// there must still hold what the change says was there, and the new value must be an
    /// element of the field.
    ///
    /// # Errors
    ///
    /// [`CodeError::Changes`] when a change does not fit; nothing is changed then.
    pub fn apply(
        &self,
        shards: &mut [(u16, &mut [u8])],
        changes: &[Change],
    ) -> Result<(), CodeError> {
        let symbol = self.field().symbol_bytes();
        let order = self.field().order();
        let fits = changes.iter().all(|change| {
            let held = place(shards, change, symbol).map(value);
            held == Some(change.was) && usize::from(change.becomes) < order
        });
        if !fits {
            return Err(CodeError::Changes);
        }
        for change in changes {
            let start = change.symbol * symbol;
            let bytes = shards
                .get_mut(change.shard)
                .and_then(|(_, bytes)| bytes.get_mut(start..start + symbol));
            let new = change.becomes.to_be_bytes();
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
}

/// The bytes of the symbol a change names, when the shards have it.
fn place<'s>(shards: &'s [(u16, &mut [u8])], change: &Change, symbol: usize) -> Option<&'s [u8]> {
    let start = change.symbol.checked_mul(symbol)?;
    let (_, bytes) = shards.get(change.shard)?;
    bytes.get(start..start.checked_add(symbol)?)
}

/// The value of a symbol's bytes, high byte first.
fn value(bytes: &[u8]) -> u16 {
    bytes
        .iter()
        .fold(0, |value, &byte| value << 8 | u16::from(byte))
}

impl Decoder {
    /// Finds the wrong symbols of the column whose syndromes are in `self.column`, with the
    /// symbols of the shards in `self.erased` marked, and leaves in `self.errors` first the wrong
    /// symbols found, then the marked ones, each with its error. False when the column cannot be
    /// put right.
    fn locate_marked(&mut self) -> bool {
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
        self.sequence.clear();
        for j in 0..self.spare - marked {
            let terms = self.erasure.iter().zip(self.column.iter().skip(j));
            let sum = terms.fold(0, |sum, (&g, &s)| sum ^ field.multiply(g, s));
            self.sequence.push(sum);
        }
        let length = self.recurrence(true);
        self.errors.clear();
        if 2 * length > self.spare - marked {
            return false;
        }
        // The roots of X^L · C(1/X) among the shards not marked, as in `locate`.
        let connection = self.connection.get(..=length).unwrap_or_default();
        for (position, &point) in self.points.iter().enumerate() {
            if self.erased.contains(&position) {
                continue;
            }
            let value = connection
                .iter()
                .fold(0, |value, &c| field.multiply(value, point) ^ c);
            if value == 0 {
                self.errors.push((position, 0));
            }
        }
        if self.errors.len() != length {
            return false;
        }
        self.errors
            .extend(self.erased.iter().map(|&position| (position, 0)));
        self.values(length)
    }

    /// Goes through the shards stripe by stripe and finds, for every column that disagrees or has
    /// a mark, the changes that put it right, appending them to `changes`. `marks` is sorted by
    /// symbol and holds each mark once. Returns how many columns it could not resolve.
    fn plan_pass<B: Deref<Target = [u8]>>(
        &mut self,
        shards: &[(u16, B)],
        length: usize,
        marks: &[Mark],
        changes: &mut Vec<Change>,
    ) -> usize {
        let symbol = self.field.symbol_bytes();
        let stripe = Self::stripe(self.field, self.spare);
        let mut syndromes = core::mem::take(&mut self.syndromes);
        let mut factors = core::mem::take(&mut self.factors);
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
                let disagrees = (differ || marked) && self.read_column(&syndromes, size, offset);
                if !disagrees && !marked {
                    continue;
                }
                let found = if over {
                    None
                } else if marked {
                    self.locate_marked()
                        .then(|| self.errors.len() - self.erased.len())
                } else {
                    self.locate().then_some(self.errors.len())
                };
                let Some(found) = found else {
                    unresolved += 1;
                    continue;
                };
                if changes.try_reserve(self.errors.len()).is_err() {
                    handle_alloc_error(Layout::new::<Change>());
                }
                for (count, &(position, error)) in self.errors.iter().enumerate() {
                    let begin = start + offset;
                    let bytes = shards
                        .get(position)
                        .and_then(|(_, bytes)| bytes.get(begin..begin + symbol))
                        .unwrap_or_default();
                    let was = value(bytes);
                    let kind = if count < found {
                        ChangeKind::Replaced
                    } else {
                        ChangeKind::Filled
                    };
                    changes.push(Change {
                        shard: position,
                        symbol: at,
                        was,
                        becomes: was ^ error,
                        kind,
                    });
                }
            }
            start += size;
        }
        self.syndromes = syndromes;
        self.factors = factors;
        unresolved
    }
}
