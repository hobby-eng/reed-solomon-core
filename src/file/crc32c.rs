//! CRC-32C (Castagnoli): polynomial 0x1EDC6F41, input and output reflected, initial value and
//! final exclusive-or FFFFFFFF. The checksum of "123456789" is E3069283.
//!
//! One bit at a time, with a mask in place of a branch and no table: the bytes of a shard choose
//! no branch and no table entry here either. It detects accidental damage; someone who changes a
//! file on purpose can make it fit.

/// The polynomial, reflected.
const POLYNOMIAL: u32 = 0x82F6_3B78;

/// A checksum in progress.
#[derive(Clone, Copy)]
pub(super) struct Crc32c(u32);

impl Crc32c {
    pub(super) const fn new() -> Self {
        Self(u32::MAX)
    }

    pub(super) fn update(&mut self, bytes: &[u8]) {
        let mut state = self.0;
        for &byte in bytes {
            state ^= u32::from(byte);
            for _ in 0..8 {
                // The low bit as a mask: all ones or all zeros.
                state = (state >> 1) ^ (POLYNOMIAL & 0_u32.wrapping_sub(state & 1));
            }
        }
        self.0 = state;
    }

    pub(super) const fn finish(self) -> u32 {
        !self.0
    }
}

/// The checksum of `parts`, one after another.
pub(super) fn checksum(parts: &[&[u8]]) -> u32 {
    let mut crc = Crc32c::new();
    for part in parts {
        crc.update(part);
    }
    crc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_value_is_the_published_one() {
        assert_eq!(checksum(&[b"123456789"]), 0xE306_9283);
        assert_eq!(checksum(&[b"1234", b"", b"56789"]), 0xE306_9283);
        assert_eq!(checksum(&[]), 0);
    }
}
