//! Whole shards in owned memory: the same operations for a caller who has the shards at hand.

use super::{Code, CodeError};
use alloc::vec;
use alloc::vec::Vec;
use zeroize::Zeroizing;

/// What the shards beyond the threshold say about a set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redundancy {
    /// Exactly `threshold` shards were given: any such shards form a set, so there is nothing to
    /// compare them with.
    None,
    /// Every shard beyond the first `threshold` is what those make.
    Agrees,
    /// A shard beyond the first `threshold` differs from what those make: some shard was changed
    /// or belongs to another set.
    Differs,
}

impl Code {
    /// Checks the indexes and lengths of at least `threshold` shards of one set; returns their
    /// common length.
    pub(crate) fn check_set(
        &self,
        indexes: impl Iterator<Item = u16> + Clone,
        mut lengths: impl Iterator<Item = usize>,
    ) -> Result<usize, CodeError> {
        // Counted and looked at one by one first: a number of shards that cannot be a set is
        // refused before any memory is taken for them.
        let (mut count, mut known) = (0_usize, true);
        for index in indexes.clone() {
            count += 1;
            known &= self.field.point(index).is_some();
        }
        if count < self.threshold {
            return Err(CodeError::Count);
        }
        if !known {
            return Err(CodeError::Index);
        }
        // More shards than the field has indexes: one index is there twice.
        if count > self.field.order() {
            return Err(CodeError::Repeated);
        }
        let mut seen: Vec<u16> = indexes.collect();
        seen.sort_unstable();
        if seen.windows(2).any(|pair| pair.first() == pair.last()) {
            return Err(CodeError::Repeated);
        }
        let length = lengths.next().unwrap_or(0);
        let whole = length.is_multiple_of(self.field.symbol_bytes());
        if !whole || lengths.any(|other| other != length) {
            return Err(CodeError::Length);
        }
        Ok(length)
    }

    /// Checks that every symbol of every shard is an element of the field: the shards beyond
    /// the threshold too, and whatever is made of them afterwards.
    pub(crate) fn check_symbols<'a>(
        &self,
        shards: impl Iterator<Item = &'a [u8]>,
    ) -> Result<(), CodeError> {
        if shards.fold(true, |all, bytes| all & self.field.canonical(bytes)) {
            Ok(())
        } else {
            Err(CodeError::Symbol)
        }
    }

    /// The indexes and bytes of the first `threshold` of `shards`, after checking the number,
    /// the indexes and the lengths of every one. Their symbols are checked by the caller, after
    /// the numbers of what it is asked to make.
    fn chosen<'a>(&self, shards: &[(u16, &'a [u8])]) -> Result<Chosen<'a>, CodeError> {
        let indexes = shards.iter().map(|&(index, _)| index);
        let length = self.check_set(indexes, shards.iter().map(|(_, bytes)| bytes.len()))?;
        let (indexes, stripes) = shards.iter().take(self.threshold).copied().unzip();
        Ok(Chosen {
            indexes,
            stripes,
            length,
        })
    }

    /// The shards numbered `targets` of the set whose data shards are `data`: how a set is written.
    /// Each shard comes in a [`Zeroizing`] buffer. Fails when `data` has the wrong count or lengths, a
    /// target is outside the field, or a symbol isn't an element of the field.
    pub fn encode(
        &self,
        data: &[&[u8]],
        targets: &[u16],
    ) -> Result<Vec<Zeroizing<Vec<u8>>>, CodeError> {
        if data.len() != self.threshold {
            return Err(CodeError::Count);
        }
        // The threshold is at most 65,536, so every data index fits.
        let numbered: Vec<(u16, &[u8])> = (0..=u16::MAX).zip(data.iter().copied()).collect();
        self.rebuild(&numbered, targets)
    }

    /// The shards numbered `targets` from any `threshold` or more shards, given as `(index, bytes)`:
    /// how a lost shard comes back, how the data is read and how a set grows. The first `threshold`
    /// shards are the sources; the rest are only checked. Each result comes in a [`Zeroizing`] buffer.
    /// Fails on too few shards, bad or repeated indexes, unequal lengths, or a symbol outside the field.
    pub fn rebuild(
        &self,
        shards: &[(u16, &[u8])],
        targets: &[u16],
    ) -> Result<Vec<Zeroizing<Vec<u8>>>, CodeError> {
        let chosen = self.chosen(shards)?;
        // Every target is checked before the first shard is made.
        for &target in targets {
            self.field.point(target).ok_or(CodeError::Index)?;
        }
        self.check_symbols(shards.iter().map(|&(_, bytes)| bytes))?;
        let mut weights = vec![0; self.threshold];
        let sources = self.sources(&chosen.indexes, &mut weights)?;
        let mut made = Vec::with_capacity(targets.len());
        for &target in targets {
            let mut shard = Zeroizing::new(vec![0; chosen.length]);
            sources.build(target, &chosen.stripes, &mut shard)?;
            made.push(shard);
        }
        Ok(made)
    }

    /// Whether the shards beyond the first `threshold` match what those make: they belong together,
    /// which says nothing about who made them. Fails as [`Code::rebuild`] does.
    pub fn redundancy(&self, shards: &[(u16, &[u8])]) -> Result<Redundancy, CodeError> {
        let chosen = self.chosen(shards)?;
        self.check_symbols(shards.iter().map(|&(_, bytes)| bytes))?;
        let spare = shards.get(self.threshold..).unwrap_or_default();
        if spare.is_empty() {
            return Ok(Redundancy::None);
        }
        let mut weights = vec![0; self.threshold];
        let sources = self.sources(&chosen.indexes, &mut weights)?;
        for &(index, bytes) in spare {
            if !sources.agrees(index, &chosen.stripes, bytes)? {
                return Ok(Redundancy::Differs);
            }
        }
        Ok(Redundancy::Agrees)
    }
}

/// The sources picked from the shards a caller gave.
struct Chosen<'a> {
    indexes: Vec<u16>,
    stripes: Vec<&'a [u8]>,
    length: usize,
}
