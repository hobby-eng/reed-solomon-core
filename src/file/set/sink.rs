//! Where the blocks a set makes go: each is handed over once, whole, with its number and its place
//! in its target's shard. The data goes to the program's own output, shard files are written in
//! place.

use super::SetError;
use crate::file::header::Header;

/// What takes the blocks a set makes.
pub(in crate::file) trait Sink {
    /// Takes `bytes`, block `number` of the target at `slot`, which starts at byte `start` of its
    /// shard.
    fn put(&mut self, slot: usize, number: u32, start: usize, bytes: &[u8])
    -> Result<(), SetError>;
}

/// The data: the data shards one after another, cut at the length of `out`. The bytes beyond are
/// padding, which must be zero: they are looked at, never kept.
pub(super) struct Data<'o> {
    pub(super) out: &'o mut [u8],
    pub(super) shard: usize,
    /// Every padding byte, or-ed together.
    pub(super) padding: u8,
}

impl Sink for Data<'_> {
    fn put(&mut self, slot: usize, _: u32, start: usize, bytes: &[u8]) -> Result<(), SetError> {
        let at = slot
            .checked_mul(self.shard)
            .and_then(|first| first.checked_add(start));
        let at = at.ok_or(SetError::Memory)?;
        let inside = self.out.len().saturating_sub(at).min(bytes.len());
        let (data, padding) = bytes.split_at(inside);
        if !data.is_empty() {
            let place = self.out.get_mut(at..at + inside).ok_or(SetError::Memory)?;
            place.copy_from_slice(data);
        }
        self.padding |= padding.iter().fold(0, |any, &byte| any | byte);
        Ok(())
    }
}

/// Shard files written in place: the outputs, the header of each target, and which outputs hold
/// each target.
pub(in crate::file) struct Files<'s, 'o> {
    pub(in crate::file) outs: &'s mut [&'o mut [u8]],
    pub(in crate::file) headers: &'s [Header],
    pub(in crate::file) by_slot: &'s BySlot,
}

impl Sink for Files<'_, '_> {
    fn put(&mut self, slot: usize, number: u32, _: usize, bytes: &[u8]) -> Result<(), SetError> {
        let header = self.headers.get(slot).ok_or(SetError::Memory)?;
        for &position in self.by_slot.of(slot) {
            let out = self.outs.get_mut(position).ok_or(SetError::Memory)?;
            header
                .write_block(out, number, bytes)
                .ok_or(SetError::Memory)?;
        }
        Ok(())
    }
}

/// The positions of outputs grouped by the slot of the target each holds, so that a block is
/// written to its outputs without a look at the others.
pub(in crate::file) struct BySlot {
    /// The positions, sorted by slot.
    order: alloc::vec::Vec<usize>,
    /// Where the positions of each slot begin in `order`, and after the last slot its end.
    starts: alloc::vec::Vec<usize>,
}

impl BySlot {
    /// The positions of `slots`, each the slot of one output, for `count` slots.
    pub(in crate::file) fn new(slots: &[usize], count: usize) -> Result<Self, SetError> {
        let (mut order, mut starts) = (alloc::vec::Vec::new(), alloc::vec::Vec::new());
        super::room(&mut order, slots.len())?;
        super::room(&mut starts, count.saturating_add(1))?;
        order.extend(0..slots.len());
        order.sort_by_key(|&position| slots.get(position).copied());
        let slot_at = |position: &usize| slots.get(*position).copied().unwrap_or(usize::MAX);
        starts.extend((0..=count).map(|slot| order.partition_point(|p| slot_at(p) < slot)));
        Ok(Self { order, starts })
    }

    /// The positions of the outputs that hold the target at `slot`.
    fn of(&self, slot: usize) -> &[usize] {
        let range = self.starts.get(slot).zip(self.starts.get(slot + 1));
        range
            .and_then(|(&start, &end)| self.order.get(start..end))
            .unwrap_or_default()
    }
}
