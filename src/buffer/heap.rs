//! The allocator's memory for a buffer, wiped when it is dropped, its unused room included.

use alloc::vec::Vec;
use zeroize::Zeroizing;

/// The bytes of a buffer.
#[derive(Default)]
pub(super) struct Store(Zeroizing<Vec<u8>>);

impl Store {
    pub(super) fn zeros(length: usize) -> Option<Self> {
        let mut bytes = Zeroizing::new(Vec::new());
        bytes.try_reserve_exact(length).ok()?;
        bytes.resize(length, 0);
        Some(Self(bytes))
    }

    pub(super) fn truncate(&mut self, length: usize) {
        self.0.truncate(length);
    }

    pub(super) fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub(super) fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.0
    }

    /// Nothing keeps the allocator's memory out of swap, of dumps or of a child process: each
    /// answer holds only for no bytes at all.
    pub(super) fn is_locked(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn is_out_of_dumps(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn is_out_of_children(&self) -> bool {
        self.0.is_empty()
    }
}
