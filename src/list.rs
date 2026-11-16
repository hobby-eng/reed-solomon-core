//! A list in room the caller gives, without an allocator: the decoders cut their short lists from
//! one room each, a protected buffer where there is one.

use core::ops::{Deref, DerefMut};

/// A list in a slice of room: the room and how much of it is in use. Each decoder gives its lists
/// the room they need by construction, so a push beyond it never comes; it would be dropped.
pub(crate) struct List<'w> {
    slots: &'w mut [u16],
    len: usize,
}

impl<'w> List<'w> {
    pub(crate) const fn new(slots: &'w mut [u16]) -> Self {
        Self { slots, len: 0 }
    }

    pub(crate) const fn clear(&mut self) {
        self.len = 0;
    }

    pub(crate) fn push(&mut self, value: u16) {
        if let Some(slot) = self.slots.get_mut(self.len) {
            *slot = value;
            self.len += 1;
        }
    }

    pub(crate) fn as_slice(&self) -> &[u16] {
        self.slots.get(..self.len).unwrap_or_default()
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u16] {
        self.slots.get_mut(..self.len).unwrap_or_default()
    }

    /// Grows the list to `len` elements of `value`, or cuts it there; never beyond its room.
    pub(crate) fn resize(&mut self, len: usize, value: u16) {
        let len = len.min(self.slots.len());
        if let Some(added) = self.slots.get_mut(self.len..len) {
            added.fill(value);
        }
        self.len = len;
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }

    pub(crate) const fn pop(&mut self) {
        self.len = self.len.saturating_sub(1);
    }

    pub(crate) fn last(&self) -> Option<u16> {
        self.as_slice().last().copied()
    }

    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn reverse(&mut self) {
        self.as_mut_slice().reverse();
    }
}

impl Deref for List<'_> {
    type Target = [u16];

    fn deref(&self) -> &[u16] {
        self.as_slice()
    }
}

impl DerefMut for List<'_> {
    fn deref_mut(&mut self) -> &mut [u16] {
        self.as_mut_slice()
    }
}

impl<'a> IntoIterator for &'a List<'_> {
    type Item = &'a u16;
    type IntoIter = core::slice::Iter<'a, u16>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a> IntoIterator for &'a mut List<'_> {
    type Item = &'a mut u16;
    type IntoIter = core::slice::IterMut<'a, u16>;

    fn into_iter(self) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
    }
}
