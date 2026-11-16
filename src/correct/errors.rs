//! The wrong symbols of a column as a decoder finds them: each shard's position and the error to
//! remove, kept as numbers in the decoder's protected room, since the errors follow from the data.

use crate::list::List;

/// Pairs of a position and an error, two numbers apiece. A position is below 65,536, the most
/// shards a field has.
pub(super) struct Errors<'w>(List<'w>);

impl<'w> Errors<'w> {
    /// Pairs in `list`, which has room for two numbers per pair.
    pub(super) const fn new(list: List<'w>) -> Self {
        Self(list)
    }

    pub(super) const fn clear(&mut self) {
        self.0.clear();
    }

    pub(super) const fn len(&self) -> usize {
        self.0.len() / 2
    }

    pub(super) fn push(&mut self, position: usize, error: u16) {
        self.0.push(u16::try_from(position).unwrap_or(u16::MAX));
        self.0.push(error);
    }

    /// The pairs, in their order.
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, u16)> + '_ {
        let pairs = self.0.as_chunks::<2>().0.iter();
        pairs.map(|&[position, error]| (usize::from(position), error))
    }

    /// Each position with its error, to change.
    pub(super) fn iter_mut(&mut self) -> impl Iterator<Item = (usize, &mut u16)> + '_ {
        let pairs = self.0.as_chunks_mut::<2>().0.iter_mut();
        pairs.map(|[position, error]| (usize::from(*position), error))
    }

    /// The first `at` pairs and the rest.
    pub(super) fn split(&self, at: usize) -> (&[[u16; 2]], &[[u16; 2]]) {
        let pairs = self.0.as_chunks::<2>().0;
        pairs.split_at_checked(at).unwrap_or_default()
    }
}
