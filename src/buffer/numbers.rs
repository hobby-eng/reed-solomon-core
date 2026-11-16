//! Room for numbers of two bytes in a [`Buffer`]: the decoders cut their short lists from it, so
//! that the lists lie in the same pages as the data they follow from, and are wiped with them.

use super::{Buffer, Shortfall};
use core::ops::{Deref, DerefMut};

/// `count` numbers in a buffer of their own, seen through `bytemuck`, which needs no `unsafe`
/// code of this crate's own.
#[derive(Default)]
pub(crate) struct Numbers {
    buffer: Buffer,
    count: usize,
}

impl Numbers {
    /// `count` zeros, refused as [`Buffer::zeros`] refuses, and when the numbers would not fit.
    pub(crate) fn zeros(count: usize) -> Result<Self, Shortfall> {
        let buffer = Buffer::zeros(Self::bytes(count))?;
        let numbers = Self { buffer, count };
        // `align_to` may leave numbers out of its middle; then the room is refused, not short.
        if numbers.all().len() < count {
            return Err(Shortfall::Memory);
        }
        Ok(numbers)
    }

    /// The bytes [`Numbers::zeros`] asks for: one more than the numbers take, so that they can
    /// start on an even address whatever the memory.
    pub(crate) const fn bytes(count: usize) -> usize {
        match count {
            0 => 0,
            _ => count.saturating_mul(2).saturating_add(1),
        }
    }

    /// The buffer's bytes from their first even address, as numbers.
    fn all(&self) -> &[u16] {
        bytemuck::pod_align_to::<u8, u16>(self.buffer.as_slice()).1
    }

    fn all_mut(&mut self) -> &mut [u16] {
        bytemuck::pod_align_to_mut::<u8, u16>(self.buffer.as_mut_slice()).1
    }
}

impl Deref for Numbers {
    type Target = [u16];

    fn deref(&self) -> &[u16] {
        self.all().get(..self.count).unwrap_or_default()
    }
}

impl DerefMut for Numbers {
    fn deref_mut(&mut self) -> &mut [u16] {
        let count = self.count;
        self.all_mut().get_mut(..count).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::Numbers;

    #[test]
    fn the_room_holds_its_numbers_from_an_even_address() {
        let mut room = Numbers::zeros(3).unwrap();
        assert_eq!(*room, [0, 0, 0]);
        room.copy_from_slice(&[7, 8, 9]);
        assert_eq!(*room, [7, 8, 9]);
        assert_eq!(room.as_ptr().align_offset(2), 0);
        assert_eq!((Numbers::bytes(3), Numbers::bytes(0)), (7, 0));
        assert!(Numbers::zeros(0).unwrap().is_empty());
    }
}
