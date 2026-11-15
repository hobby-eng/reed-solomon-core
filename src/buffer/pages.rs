//! Pages of their own for a buffer, from the `secret-protection-core` crate: a work area, which
//! that crate sets between guard pages, locks where the system's lock limit holds all of it,
//! keeps out of core dumps and of child processes where the system has a call for that, and
//! wipes before it gives the pages back.

use secret_protection_core::WorkArea;

/// The pages of a buffer and how many of their bytes it has. An empty buffer has no pages.
#[derive(Default)]
pub(super) struct Store {
    area: Option<WorkArea>,
    length: usize,
}

impl Store {
    pub(super) fn zeros(length: usize) -> Option<Self> {
        if length == 0 {
            return Some(Self::default());
        }
        let area = Some(WorkArea::new(length)?);
        Some(Self { area, length })
    }

    pub(super) fn as_slice(&self) -> &[u8] {
        let bytes = self.area.as_ref().map(WorkArea::as_slice);
        bytes
            .and_then(|bytes| bytes.get(..self.length))
            .unwrap_or_default()
    }

    pub(super) fn as_mut_slice(&mut self) -> &mut [u8] {
        let bytes = self.area.as_mut().map(WorkArea::as_mut_slice);
        bytes
            .and_then(|bytes| bytes.get_mut(..self.length))
            .unwrap_or_default()
    }

    pub(super) fn is_locked(&self) -> bool {
        self.area.as_ref().is_none_or(WorkArea::is_locked)
    }

    pub(super) fn is_out_of_dumps(&self) -> bool {
        self.area.as_ref().is_none_or(WorkArea::is_out_of_dumps)
    }

    pub(super) fn is_out_of_children(&self) -> bool {
        self.area.as_ref().is_none_or(WorkArea::is_out_of_children)
    }
}
