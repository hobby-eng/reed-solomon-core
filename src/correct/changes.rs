//! The list of changes of a plan. What a symbol was and becomes can be a secret, so the list lies
//! in a [`Buffer`] like every other result that holds data, as records of a fixed length.

use super::NoMemory;
use super::plan::{Change, ChangeKind};
use crate::buffer::Buffer;
use alloc::alloc::handle_alloc_error;
use core::alloc::Layout;
use core::fmt;

/// Bytes of one record: the symbol's place (8), its shard's place (8), what the symbol holds (2)
/// and becomes (2), its kind (1) and three spare bytes, each number low byte first.
const RECORD: usize = 24;

/// The byte of a record that says [`ChangeKind::Replaced`]; [`ChangeKind::Filled`] is zero.
const REPLACED: u8 = 1;

/// The records a list gets room for when it first grows: most of a page, which a buffer in
/// protected pages takes whole in any case.
const FIRST_ROOM: usize = 128;

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

    /// Whether the list lies in locked pages, as [`Buffer::is_locked`] says of a buffer.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.records.is_locked()
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
    /// the old one is wiped as it is dropped; a refusal of the memory leaves the list as it was.
    pub(super) fn push(&mut self, change: Change) -> Result<(), NoMemory> {
        let room = self.records.len() / RECORD;
        if self.count == room {
            let more = room.saturating_mul(2).max(FIRST_ROOM);
            let mut larger = Buffer::zeros(more.saturating_mul(RECORD)).ok_or(NoMemory)?;
            if let Some(front) = larger.get_mut(..self.records.len()) {
                front.copy_from_slice(&self.records);
            }
            self.records = larger;
        }
        let record = self
            .records
            .get_mut(self.count * RECORD..)
            .and_then(<[u8]>::first_chunk_mut::<RECORD>)
            .ok_or(NoMemory)?;
        write(record, change);
        self.count += 1;
        Ok(())
    }
}

/// A place as a record keeps it: a `usize` is at most 64 bits wide.
fn place(place: usize) -> [u8; 8] {
    u64::try_from(place).unwrap_or(u64::MAX).to_le_bytes()
}

/// Writes `change` into `record`.
fn write(record: &mut [u8; RECORD], change: Change) {
    let kind = match change.kind {
        ChangeKind::Filled => 0,
        ChangeKind::Replaced => REPLACED,
    };
    let fields = place(change.symbol)
        .into_iter()
        .chain(place(change.shard))
        .chain(change.was.to_le_bytes())
        .chain(change.becomes.to_le_bytes())
        .chain([kind]);
    for (slot, byte) in record.iter_mut().zip(fields) {
        *slot = byte;
    }
}

/// The change that `record` holds.
fn read(record: &[u8; RECORD]) -> Change {
    let (symbol, rest) = record.split_at(8);
    let (shard, rest) = rest.split_at(8);
    let number = |bytes: &[u8]| {
        let wide = bytes
            .iter()
            .rev()
            .fold(0_u64, |wide, &byte| wide << 8 | u64::from(byte));
        usize::try_from(wide).unwrap_or(usize::MAX)
    };
    let value = |at: usize| {
        let low = rest.get(at).copied().unwrap_or(0);
        let high = rest.get(at + 1).copied().unwrap_or(0);
        u16::from_le_bytes([low, high])
    };
    Change {
        shard: number(shard),
        symbol: number(symbol),
        was: value(0),
        becomes: value(2),
        kind: match rest.get(4) {
            Some(&REPLACED) => ChangeKind::Replaced,
            _ => ChangeKind::Filled,
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
    /// A list a program makes itself, for [`crate::Code::apply`] or to compare a plan with. As
    /// a `Vec` does, it stops the program when there is no memory for the list.
    fn from_iter<I: IntoIterator<Item = Change>>(changes: I) -> Self {
        let mut list = Self::default();
        for change in changes {
            if list.push(change).is_err() {
                handle_alloc_error(Layout::new::<Change>());
            }
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
        // An empty list holds no memory, which counts as locked.
        assert!(Changes::default().is_locked());
        // More than the first room, so the list grows on the way, and the largest places: a
        // place beyond every set is kept as it is, so that `apply` refuses it.
        let count = 2 * FIRST_ROOM + 3;
        let made: alloc::vec::Vec<Change> = (0..count)
            .map(|i| match i % 2 {
                0 => change(i, i * 1000, ChangeKind::Filled),
                _ => change(usize::MAX - i, usize::MAX - 2 * i, ChangeKind::Replaced),
            })
            .collect();
        let list: Changes = made.iter().copied().collect();
        assert_eq!(list.len(), count);
        assert!(list.iter().eq(made.iter().copied()));
        assert_eq!(list.iter().len(), count);
        assert_eq!(list.get(count - 1), made.last().copied());
        assert_eq!(list.get(count), None);
        assert_eq!(
            alloc::format!("{list:?}"),
            alloc::format!("Changes({count})")
        );
        // Two places that differ only above sixteen bits are two changes.
        let far: Changes = [change(70_000, 0, ChangeKind::Filled)]
            .into_iter()
            .collect();
        let near: Changes = [change(65_535, 0, ChangeKind::Filled)]
            .into_iter()
            .collect();
        assert!(far != near && far.get(0).map(|change| change.shard) == Some(70_000));
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
