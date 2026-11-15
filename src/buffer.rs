//! The memory the crate holds data in: every owned result, and every working buffer of bytes that
//! follow from data. With the `protect` feature it is pages of their own from the
//! `secret-protection-core` crate; without it, the allocator's memory. Either is wiped when let go.

#[cfg(not(feature = "protect"))]
mod heap;
#[cfg(feature = "protect")]
mod pages;

#[cfg(not(feature = "protect"))]
use heap::Store;
#[cfg(feature = "protect")]
use pages::Store;

use core::fmt;
use core::ops::{Deref, DerefMut};

/// Bytes of data the crate made, wiped when dropped: a shard, a shard file, rebuilt data. With the
/// `protect` feature they lie in pages of their own, which the system is asked to lock
/// ([`Buffer::is_locked`] says whether it did) and to keep out of core dumps and child processes,
/// as far as it has a call for each. Its `Debug` text gives the length only, and two buffers
/// are compared without stopping at the first difference.
#[derive(Default)]
pub struct Buffer(Store);

impl Buffer {
    /// `length` zero bytes, or `None` when the system gives no memory for them.
    pub(crate) fn zeros(length: usize) -> Option<Self> {
        Store::zeros(length).map(Self)
    }

    /// Shortens the buffer to `length` bytes; a longer `length` changes nothing.
    pub(crate) fn truncate(&mut self, length: usize) {
        self.0.truncate(length);
    }

    /// The bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    /// The bytes, to change in place.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.0.as_mut_slice()
    }

    /// Whether the system keeps the buffer's pages in memory, out of swap; an image written when
    /// the machine hibernates still holds them. Without the `protect` feature this, like the two
    /// answers below, holds for an empty buffer only.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.0.is_locked()
    }

    /// Whether the system leaves the buffer's pages out of the dumps it writes of the process.
    #[must_use]
    pub fn is_out_of_dumps(&self) -> bool {
        self.0.is_out_of_dumps()
    }

    /// Whether a child process gets none of the bytes: no mapping of the pages, or zeros in
    /// their place. A child of `fork` must leave its parent's buffers alone: there a buffer says
    /// it is not protected, dropping it touches nothing, and reading it ends the child.
    #[must_use]
    pub fn is_out_of_children(&self) -> bool {
        self.0.is_out_of_children()
    }
}

impl Deref for Buffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl DerefMut for Buffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Buffer({} bytes)", self.len())
    }
}

impl PartialEq for Buffer {
    /// Every byte is looked at, whichever differs.
    fn eq(&self, other: &Self) -> bool {
        let differ = self
            .iter()
            .zip(other.iter())
            .fold(0, |differ, (own, others)| differ | (own ^ others));
        self.len() == other.len() && differ == 0
    }
}

impl Eq for Buffer {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_holds_zeros_and_shortens() {
        let mut buffer = Buffer::zeros(5).unwrap();
        assert_eq!(*buffer, [0; 5]);
        buffer.copy_from_slice(&[1, 2, 3, 4, 5]);
        buffer.truncate(9);
        assert_eq!(buffer.len(), 5);
        buffer.truncate(2);
        assert_eq!(*buffer, [1, 2]);
        assert_eq!(alloc::format!("{buffer:?}"), "Buffer(2 bytes)");
    }

    #[test]
    #[cfg_attr(miri, ignore = "Miri stops at an allocation that a system refuses")]
    fn a_buffer_beyond_the_system_is_refused() {
        assert!(Buffer::zeros(usize::MAX).is_none());
        assert!(Buffer::zeros(isize::MAX as usize).is_none());
    }

    #[test]
    fn an_empty_buffer_takes_no_memory_and_equals_another() {
        let (empty, other) = (Buffer::default(), Buffer::zeros(0).unwrap());
        assert!(empty.is_empty() && empty.is_locked());
        assert!(empty.is_out_of_dumps() && empty.is_out_of_children());
        assert_eq!(empty, other);
        assert_ne!(empty, Buffer::zeros(1).unwrap());
    }

    #[test]
    fn buffers_of_one_length_differ_by_any_byte() {
        let (mut first, second) = (Buffer::zeros(4).unwrap(), Buffer::zeros(4).unwrap());
        assert_eq!(first, second);
        first.as_mut_slice()[3] = 1;
        assert_ne!(first, second);
    }
}
