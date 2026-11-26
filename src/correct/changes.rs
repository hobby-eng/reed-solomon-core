//! The list of changes of a plan. What a symbol holds and becomes can be a secret, so the list lies
//! in a [`Buffer`] like every other result that holds data: the fingerprint of the shards the plan
//! was made for, then records of a fixed length. It hands out places, and the values only to a
//! closure that looks at them.

use super::plan::{Change, ChangeKind, Record};
use crate::buffer::{Buffer, Shortfall};
use core::fmt;

/// Bytes of one record: the symbol's place (8), its shard's place (8), what the symbol holds (2)
/// and becomes (2), its kind (1) and three spare bytes, each number low byte first.
const RECORD: usize = 24;

/// The byte of a record that says [`ChangeKind::Replaced`]; [`ChangeKind::Filled`] is zero.
const REPLACED: u8 = 1;

/// The records a list gets room for at first: most of a page, which a buffer in protected pages
/// takes whole in any case.
const FIRST_ROOM: usize = 128;

/// Bytes of the fingerprint of the shards a plan was made for, before the records.
pub(crate) const FINGERPRINT: usize = 32;

/// The changes of a [`crate::Plan`], in the order of the symbols and, within one, of the shards,
/// bound to the shards the plan was made for: `apply` refuses other shards before it writes. They
/// lie in a [`Buffer`], wiped when dropped and, with the `protect` feature, in pages of their own.
/// Its `Debug` text gives their number.
pub struct Changes {
    /// The fingerprint, then the records.
    bytes: Buffer,
    count: usize,
}

impl Changes {
    /// An empty list for the shards whose fingerprint is `fingerprint`.
    pub(crate) fn bound(fingerprint: &[u8; FINGERPRINT]) -> Result<Self, Shortfall> {
        let mut bytes = Buffer::zeros(FINGERPRINT + FIRST_ROOM * RECORD)?;
        if let Some(front) = bytes.get_mut(..FINGERPRINT) {
            front.copy_from_slice(fingerprint);
        }
        Ok(Self { bytes, count: 0 })
    }

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

    /// The place and kind of the change at place `at`, counted from zero; `None` past the last.
    /// What the symbol held and becomes is not read.
    #[must_use]
    pub fn get(&self, at: usize) -> Option<Change> {
        self.slot(at).map(change)
    }

    /// What the symbol of the change at place `at` holds and becomes, given to `look`; `None`
    /// past the last. They can be a secret: a program looks at what it needs and keeps no copy,
    /// or uses the strict mode's `values`, which runs `look` on a protected stack. With `protect`
    /// a fixed window of the stack below is cleared after `look` returns, best effort.
    pub fn values<R>(&self, at: usize, look: impl FnOnce(u16, u16) -> R) -> Option<R> {
        crate::cleared(|| {
            self.record(at)
                .map(|record| look(record.was, record.becomes))
        })
    }

    /// Whether the list lies in locked pages, as [`Buffer::is_locked`] says of a buffer.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.bytes.is_locked()
    }

    /// The places and kinds of the changes, in their order.
    #[must_use]
    pub const fn iter(&self) -> ChangesIter<'_> {
        ChangesIter {
            changes: self,
            next: 0,
        }
    }

    /// Whether the list's pages are protected as the strict mode requires.
    #[cfg(feature = "protect")]
    pub(crate) fn guarded(&self) -> bool {
        self.bytes.guarded()
    }

    /// The fingerprint of the shards the plan was made for.
    pub(crate) fn fingerprint(&self) -> &[u8] {
        self.bytes.get(..FINGERPRINT).unwrap_or_default()
    }

    /// The change at place `at` with its values.
    pub(crate) fn record(&self, at: usize) -> Option<Record> {
        self.slot(at).map(read)
    }

    /// The bytes of the record at place `at`.
    fn slot(&self, at: usize) -> Option<&[u8; RECORD]> {
        let start = at.checked_mul(RECORD)?.checked_add(FINGERPRINT)?;
        let record = self.bytes.get(start..)?.first_chunk::<RECORD>()?;
        (at < self.count).then_some(record)
    }

    /// The fingerprint and the records the list holds.
    fn used(&self) -> &[u8] {
        let end = self
            .count
            .saturating_mul(RECORD)
            .saturating_add(FINGERPRINT);
        self.bytes.get(..end).unwrap_or_default()
    }

    /// Adds `record` after the others. A list that has to grow moves into a larger buffer, and
    /// the old one is wiped as it is dropped; a refusal of the memory leaves the list as it was.
    pub(crate) fn push(&mut self, record: Record) -> Result<(), Shortfall> {
        let room = self.bytes.len().saturating_sub(FINGERPRINT) / RECORD;
        if self.count == room {
            let more = room.saturating_mul(2).max(FIRST_ROOM);
            let bytes = more.saturating_mul(RECORD).saturating_add(FINGERPRINT);
            let mut larger = Buffer::zeros(bytes)?;
            if let Some(front) = larger.get_mut(..self.bytes.len()) {
                front.copy_from_slice(&self.bytes);
            }
            self.bytes = larger;
        }
        let start = self.count * RECORD + FINGERPRINT;
        let slot = self
            .bytes
            .get_mut(start..)
            .and_then(<[u8]>::first_chunk_mut::<RECORD>)
            .ok_or(Shortfall::Memory)?;
        write(slot, record);
        self.count += 1;
        Ok(())
    }
}

/// A place as a record keeps it: a `usize` is at most 64 bits wide.
fn place(place: usize) -> [u8; 8] {
    u64::try_from(place).unwrap_or(u64::MAX).to_le_bytes()
}

/// Writes `record` into `slot`.
fn write(slot: &mut [u8; RECORD], record: Record) {
    let kind = match record.kind {
        ChangeKind::Filled => 0,
        ChangeKind::Replaced => REPLACED,
    };
    let fields = place(record.symbol)
        .into_iter()
        .chain(place(record.shard))
        .chain(record.was.to_le_bytes())
        .chain(record.becomes.to_le_bytes())
        .chain([kind]);
    for (byte, field) in slot.iter_mut().zip(fields) {
        *byte = field;
    }
}

/// The place and kind that `slot` holds, from their bytes alone: the two values are not read.
fn change(slot: &[u8; RECORD]) -> Change {
    let number = |bytes: Option<&[u8]>| {
        let wide = bytes
            .unwrap_or_default()
            .iter()
            .rev()
            .fold(0_u64, |wide, &byte| wide << 8 | u64::from(byte));
        usize::try_from(wide).unwrap_or(usize::MAX)
    };
    Change {
        shard: number(slot.get(8..16)),
        symbol: number(slot.get(..8)),
        kind: match slot.get(20) {
            Some(&REPLACED) => ChangeKind::Replaced,
            _ => ChangeKind::Filled,
        },
    }
}

/// The record that `slot` holds, its values included.
fn read(slot: &[u8; RECORD]) -> Record {
    let Change {
        shard,
        symbol,
        kind,
    } = change(slot);
    let value = |at: usize| {
        let low = slot.get(at).copied().unwrap_or(0);
        let high = slot.get(at + 1).copied().unwrap_or(0);
        u16::from_le_bytes([low, high])
    };
    Record {
        shard,
        symbol,
        was: value(16),
        becomes: value(18),
        kind,
    }
}

impl PartialEq for Changes {
    /// The fingerprints and every record are looked at, whichever differs; room not yet used
    /// does not count; lists of different lengths are unequal, their common part read all the
    /// same. The values are read too, on the caller's stack; with `protect` on a native system a
    /// fixed window of the stack below is cleared after it, best effort (docs/TECHNICAL.md,
    /// Secrets in memory); WebAssembly clears nothing. Lists equal in every change but made for
    /// other shards differ, by their fingerprints, which only `==` compares; `get` and the strict
    /// mode's `values` show that the changes are the same, not the plans.
    fn eq(&self, other: &Self) -> bool {
        crate::cleared(|| {
            let differ = self
                .used()
                .iter()
                .zip(other.used())
                .fold(0, |differ, (own, others)| differ | (own ^ others));
            self.count == other.count && self.used().len() == other.used().len() && differ == 0
        })
    }
}

impl Eq for Changes {}

impl fmt::Debug for Changes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Changes({})", self.count)
    }
}

impl<'a> IntoIterator for &'a Changes {
    type Item = Change;
    type IntoIter = ChangesIter<'a>;

    fn into_iter(self) -> ChangesIter<'a> {
        self.iter()
    }
}

/// The places and kinds of the changes of a list, one after another.
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

    fn record(shard: usize, symbol: usize, kind: ChangeKind) -> Record {
        Record {
            shard,
            symbol,
            was: 0x07FF,
            becomes: 0xA5C3,
            kind,
        }
    }

    fn list(fingerprint: u8, records: &[Record]) -> Changes {
        let mut list = Changes::bound(&[fingerprint; FINGERPRINT]).unwrap();
        for &record in records {
            list.push(record).unwrap();
        }
        list
    }

    #[test]
    fn a_list_gives_back_every_change_it_took_in_their_order() {
        // More than the first room, so the list grows on the way, and the largest places: a
        // place beyond every set is kept as it is, so that `apply` refuses it.
        let count = 2 * FIRST_ROOM + 3;
        let made: alloc::vec::Vec<Record> = (0..count)
            .map(|i| match i % 2 {
                0 => record(i, i * 1000, ChangeKind::Filled),
                _ => record(usize::MAX - i, usize::MAX - 2 * i, ChangeKind::Replaced),
            })
            .collect();
        let list = list(7, &made);
        assert_eq!((list.len(), list.iter().len()), (count, count));
        for (at, (change, record)) in list.iter().zip(&made).enumerate() {
            assert_eq!(
                (change.shard, change.symbol, change.kind),
                (record.shard, record.symbol, record.kind)
            );
            let values = list.values(at, |was, becomes| (was, becomes));
            assert_eq!(values, Some((0x07FF, 0xA5C3)));
        }
        assert_eq!(
            (list.get(count), list.values(count, |_, _| ())),
            (None, None)
        );
        assert_eq!(list.fingerprint(), [7; FINGERPRINT]);
        assert_eq!(
            alloc::format!("{list:?}"),
            alloc::format!("Changes({count})")
        );
        // Two places that differ only above sixteen bits are two changes.
        let far = self::list(0, &[record(70_000, 0, ChangeKind::Filled)]);
        let near = self::list(0, &[record(65_535, 0, ChangeKind::Filled)]);
        assert!(far != near && far.get(0).map(|change| change.shard) == Some(70_000));
    }

    #[test]
    fn lists_are_equal_when_their_fingerprints_and_changes_are() {
        let one = [record(1, 2, ChangeKind::Filled)];
        let other = [record(1, 2, ChangeKind::Replaced)];
        assert!(list(1, &one) == list(1, &one));
        assert!(list(1, &one) != list(1, &other) && list(1, &one) != list(2, &one));
        assert!(list(1, &[]).is_empty() && list(1, &[]) != list(1, &one));
    }
}
