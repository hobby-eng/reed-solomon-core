//! The list of changes of a plan. What a symbol was and becomes can be a secret, so the list lies
//! in a [`Buffer`] like every other result that holds data, as records of a fixed length.

use super::plan::{Change, ChangeKind};
use crate::buffer::Buffer;
use core::fmt;

/// Bytes of one record: the symbol's place (8), its shard's place (2), what the symbol holds (2)
/// and becomes (2), its kind (1) and one spare byte, each number low byte first.
const RECORD: usize = 16;

/// The byte of a record that says [`ChangeKind::Replaced`]; [`ChangeKind::Filled`] is zero.
const REPLACED: u8 = 1;

/// The fewest records a list gets room for when it first grows.
const FIRST_ROOM: usize = 4;

/// The changes of a [`crate::Plan`], in the order of the symbols and, within one, of the shards.
/// They lie in a [`Buffer`]: wiped when dropped and, with the `protect` feature, in pages of their
/// own. The list gives a [`Change`] when asked for one; its `Debug` text gives their number.
#[derive(Default)]
pub struct Changes {
    records: Buffer,
    count: usize,
}

impl Changes {
    /// How many changes there are.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether there is no change.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The change at place `at`, counted from zero; `None` past the last.
    #[must_use]
    pub fn get(&self, at: usize) -> Option<Change> {
        let start = at.checked_mul(RECORD)?;
        let record = self.records.get(start..)?.first_chunk::<RECORD>()?;
        (at < self.count).then(|| read(record))
    }

    /// The changes in their order.
    #[must_use]
    pub const fn iter(&self) -> ChangesIter<'_> {
        ChangesIter {
            changes: self,
            next: 0,
        }
    }

    /// The bytes of the records the list holds.
    fn used(&self) -> &[u8] {
        let end = self.count.saturating_mul(RECORD);
        self.records.get(..end).unwrap_or_default()
    }

    /// Adds `change` after the others. A list that has to grow moves into a larger buffer, and
    /// the old one is wiped as it is dropped; a refusal of the memory stops the program, as in
    /// [`crate::Code::correct`].
    pub(super) fn push(&mut self, change: Change) {
        let room = self.records.len() / RECORD;
        if self.count == room {
            let more = room.saturating_mul(2).max(FIRST_ROOM);
            let mut larger = Buffer::zeros_or_stop(more.saturating_mul(RECORD));
            if let Some(front) = larger.get_mut(..self.records.len()) {
                front.copy_from_slice(&self.records);
            }
            self.records = larger;
        }
        let record = self
            .records
            .get_mut(self.count * RECORD..)
            .and_then(<[u8]>::first_chunk_mut::<RECORD>);
        if let Some(record) = record {
            write(record, change);
            self.count += 1;
        }
    }
}

/// Writes `change` into `record`.
fn write(record: &mut [u8; RECORD], change: Change) {
    // A set has at most 65,536 shards and a shard at most 2^64 symbols, so both places fit.
    let symbol = u64::try_from(change.symbol).unwrap_or(u64::MAX);
    let shard = u16::try_from(change.shard).unwrap_or(u16::MAX);
    let kind = match change.kind {
        ChangeKind::Filled => 0,
        ChangeKind::Replaced => REPLACED,
    };
    let fields = symbol
        .to_le_bytes()
        .into_iter()
        .chain(shard.to_le_bytes())
        .chain(change.was.to_le_bytes())
        .chain(change.becomes.to_le_bytes())
        .chain([kind, 0]);
    for (slot, byte) in record.iter_mut().zip(fields) {
        *slot = byte;
    }
}

/// The change that `record` holds.
fn read(record: &[u8; RECORD]) -> Change {
    let [
        s0,
        s1,
        s2,
        s3,
        s4,
        s5,
        s6,
        s7,
        h0,
        h1,
        w0,
        w1,
        b0,
        b1,
        kind,
        _,
    ] = *record;
    let symbol = u64::from_le_bytes([s0, s1, s2, s3, s4, s5, s6, s7]);
    Change {
        shard: usize::from(u16::from_le_bytes([h0, h1])),
        symbol: usize::try_from(symbol).unwrap_or(usize::MAX),
        was: u16::from_le_bytes([w0, w1]),
        becomes: u16::from_le_bytes([b0, b1]),
        kind: if kind == REPLACED {
            ChangeKind::Replaced
        } else {
            ChangeKind::Filled
        },
    }
}

impl PartialEq for Changes {
    /// Every record is looked at, whichever differs; room not yet used does not count.
    fn eq(&self, other: &Self) -> bool {
        let differ = self
            .used()
            .iter()
            .zip(other.used())
            .fold(0, |differ, (own, others)| differ | (own ^ others));
        self.count == other.count && differ == 0
    }
}

impl Eq for Changes {}

impl fmt::Debug for Changes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Changes({})", self.count)
    }
}

impl FromIterator<Change> for Changes {
    /// A list a program makes itself, for [`crate::Code::apply`] or to compare a plan with.
    fn from_iter<I: IntoIterator<Item = Change>>(changes: I) -> Self {
        let mut list = Self::default();
        for change in changes {
            list.push(change);
        }
        list
    }
}

impl<'a> IntoIterator for &'a Changes {
    type Item = Change;
    type IntoIter = ChangesIter<'a>;

    fn into_iter(self) -> ChangesIter<'a> {
        self.iter()
    }
}

/// The changes of a list, one after another.
#[derive(Clone, Debug)]
pub struct ChangesIter<'a> {
    changes: &'a Changes,
    next: usize,
}

impl Iterator for ChangesIter<'_> {
    type Item = Change;

    fn next(&mut self) -> Option<Change> {
        let change = self.changes.get(self.next)?;
        self.next += 1;
        Some(change)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.changes.count.saturating_sub(self.next);
        (left, Some(left))
    }
}

impl ExactSizeIterator for ChangesIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(shard: usize, symbol: usize, kind: ChangeKind) -> Change {
        Change {
            shard,
            symbol,
            was: 0x07FF,
            becomes: 0xA5C3,
            kind,
        }
    }

    #[test]
    fn a_list_gives_back_every_change_it_took_in_their_order() {
        // More than the first room, so the list grows on the way, and the largest places.
        let made: alloc::vec::Vec<Change> = (0..11)
            .map(|i| match i % 2 {
                0 => change(i, i * 1000, ChangeKind::Filled),
                _ => change(65_535 - i, usize::MAX - i, ChangeKind::Replaced),
            })
            .collect();
        let list: Changes = made.iter().copied().collect();
        assert_eq!(list.len(), 11);
        assert!(list.iter().eq(made.iter().copied()));
        assert_eq!(list.iter().len(), 11);
        assert_eq!(list.get(10), made.last().copied());
        assert_eq!(list.get(11), None);
        assert_eq!(alloc::format!("{list:?}"), "Changes(11)");
    }

    #[test]
    fn lists_are_equal_when_their_changes_are() {
        let list: Changes = [change(1, 2, ChangeKind::Filled)].into_iter().collect();
        let same: Changes = [change(1, 2, ChangeKind::Filled)].into_iter().collect();
        let other: Changes = [change(1, 2, ChangeKind::Replaced)].into_iter().collect();
        assert!(list == same && list != other && list != Changes::default());
        assert!(Changes::default().is_empty());
    }
}
