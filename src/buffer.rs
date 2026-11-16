//! The memory the crate holds data in: every owned result, and every working buffer of bytes that
//! follow from data. With the `protect` feature it is pages of their own from the
//! `secret-protection-core` crate; without it, the allocator's memory. Either is wiped when let go.

#[cfg(not(feature = "protect"))]
mod heap;
mod numbers;
#[cfg(feature = "protect")]
mod pages;

pub(crate) use numbers::Numbers;

#[cfg(not(feature = "protect"))]
use heap::Store;
#[cfg(feature = "protect")]
use pages::Store;

#[cfg(feature = "protect")]
use crate::strict::required;

/// Without protected pages there is no strict mode to ask for them.
#[cfg(not(feature = "protect"))]
const fn required() -> bool {
    false
}

/// Why the crate made no buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shortfall {
    /// The system gave no memory.
    Memory,
    /// In the strict mode, the system did not protect the pages as that mode requires.
    Unprotected,
}

/// Whether pages are protected as the strict mode requires on this system: locked, out of child
/// processes and, where the system keeps single pages out of dumps, out of dumps. Apple systems
/// keep no single page out of a dump; there the process's own limit on dumps has to.
pub(crate) const fn guarded(locked: bool, out_of_dumps: bool, out_of_children: bool) -> bool {
    let dumps = cfg!(target_vendor = "apple") || out_of_dumps;
    locked && dumps && out_of_children
}

use core::fmt;
use core::ops::{Deref, DerefMut};

/// Bytes of data the crate made, wiped when dropped: a shard, a shard file, rebuilt data. With the
/// `protect` feature they lie in pages of their own (the allocator's in WebAssembly) that the
/// system is asked to lock and keep out of dumps and child processes; the methods below say what
/// it did, and a strict call refuses pages that lack what it requires. `Debug` gives the length
/// only, and two buffers are compared without stopping at a difference.
#[derive(Default)]
pub struct Buffer(Store);

impl Buffer {
    /// `length` zero bytes. In the strict mode the pages must be protected as it requires, which
    /// is checked before a byte is written; otherwise the buffer goes, wiped, and is refused.
    pub(crate) fn zeros(length: usize) -> Result<Self, Shortfall> {
        let buffer = Store::zeros(length).map(Self).ok_or(Shortfall::Memory)?;
        if required() && !buffer.guarded() {
            return Err(Shortfall::Unprotected);
        }
        Ok(buffer)
    }

    /// Whether its pages are protected as the strict mode requires on this system.
    pub(crate) fn guarded(&self) -> bool {
        guarded(
            self.is_locked(),
            self.is_out_of_dumps(),
            self.is_out_of_children(),
        )
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
    /// it is not protected, dropping it touches nothing, and asking it for its bytes ends the
    /// child. A slice taken before the `fork` is not checked again.
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
    fn a_buffer_holds_zeros_and_what_is_written() {
        let mut buffer = Buffer::zeros(5).unwrap();
        assert_eq!(*buffer, [0; 5]);
        buffer.copy_from_slice(&[1, 2, 3, 4, 5]);
        assert_eq!(*buffer, [1, 2, 3, 4, 5]);
        assert_eq!(alloc::format!("{buffer:?}"), "Buffer(5 bytes)");
    }

    #[test]
    #[cfg_attr(miri, ignore = "Miri stops at an allocation that a system refuses")]
    fn a_buffer_beyond_the_system_is_refused() {
        assert_eq!(Buffer::zeros(usize::MAX), Err(Shortfall::Memory));
        assert_eq!(Buffer::zeros(isize::MAX as usize), Err(Shortfall::Memory));
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
