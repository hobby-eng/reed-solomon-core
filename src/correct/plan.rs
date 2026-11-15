//! Correcting with symbols marked unreadable, and listing every change before anything changes.
//! A marked symbol costs one spare shard instead of two, because its place is known: it is taken
//! out of the syndromes, and Berlekamp–Massey finds the remaining wrong symbols as usual. A column
//! is fixed whenever `2e + s ≤ m − k`.

use super::changes::Changes;
use super::{Decoder, roomy};
use crate::code::{Code, CodeError};
use alloc::alloc::handle_alloc_error;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::fmt;
use core::ops::Deref;
use zeroize::Zeroize;

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

/// One symbol that a [`Plan`] changes. What it was and becomes can be a secret, such as a word of
/// a recovery phrase: they are left out of its `Debug` text, and [`Zeroize`] wipes them.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
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

impl fmt::Debug for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Change")
            .field("shard", &self.shard)
            .field("symbol", &self.symbol)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Zeroize for Change {
    fn zeroize(&mut self) {
        self.shard.zeroize();
        self.symbol.zeroize();
        self.was.zeroize();
        self.becomes.zeroize();
    }
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
    /// one, of the shards. Every marked symbol is among them. Whether the result is the original
    /// data is for a digest or an authentication to say. The list wipes itself when it is
    /// dropped.
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
        let mut changes = Changes::default();
        let unresolved = decoder.plan_pass(shards, length, &sorted, &mut changes);
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

    /// Makes `changes`, as [`Code::plan`] found them for these shards: a [`Changes`] by reference, or
    /// changes a program lists itself. Every change is checked first (its place, the value still
    /// there, the new value), and if one doesn't fit, nothing is changed and it fails with
    /// [`CodeError::Changes`].
    pub fn apply<I>(&self, shards: &mut [(u16, &mut [u8])], changes: I) -> Result<(), CodeError>
    where
        I: IntoIterator<Item = Change>,
        I::IntoIter: Clone,
    {
        let changes = changes.into_iter();
        let symbol = self.field().symbol_bytes();
        let order = self.field().order();
        let fits = changes.clone().all(|change| {
            let held = place(shards, &change, symbol).map(value);
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
        changes: &mut Changes,
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
                // The symbols found and the marked ones each come in the order of the shards:
                // the two runs are merged, so that the whole list is in that order.
                let (found, marked) = self.errors.split_at_checked(found).unwrap_or_default();
                let (mut found, mut marked) = (found.iter().peekable(), marked.iter().peekable());
                loop {
                    let first = match (found.peek(), marked.peek()) {
                        (Some(replaced), Some(filled)) => replaced.0 < filled.0,
                        (Some(_), None) => true,
                        (None, Some(_)) => false,
                        (None, None) => break,
                    };
                    let (next, kind) = match first {
                        true => (found.next(), ChangeKind::Replaced),
                        false => (marked.next(), ChangeKind::Filled),
                    };
                    let Some(&(position, error)) = next else {
                        break;
                    };
                    let begin = start + offset;
                    let bytes = shards
                        .get(position)
                        .and_then(|(_, bytes)| bytes.get(begin..begin + symbol))
                        .unwrap_or_default();
                    let was = value(bytes);
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
