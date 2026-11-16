//! The shortest linear recurrence that a sequence of field elements obeys, which both decoders use
//! to find the locator of the wrong symbols of a column or a word. It needs no allocator.

use crate::field::Field;

/// Berlekamp–Massey: the shortest recurrence `Σ_{i=0..=L} c_i · s_{n−i} = 0` (c_0 = 1) that
/// `sequence` obeys. Leaves its polynomial in `connection` and returns `L`. `previous` and `saved`
/// are working room as long as `connection`, which is longer than `sequence`.
pub(crate) fn berlekamp_massey(
    field: Field,
    sequence: &[u16],
    connection: &mut [u16],
    previous: &mut [u16],
    saved: &mut [u16],
) -> usize {
    connection.fill(0);
    previous.fill(0);
    if let (Some(c), Some(b)) = (connection.first_mut(), previous.first_mut()) {
        (*c, *b) = (1, 1);
    }
    let (mut length, mut shift, mut last) = (0, 1, 1);
    for (n, &syndrome) in sequence.iter().enumerate() {
        // The difference between the syndrome and what the recurrence so far predicts.
        let taps = connection.iter().skip(1).take(length);
        let earlier = sequence.iter().take(n).rev();
        let discrepancy = taps
            .zip(earlier)
            .fold(syndrome, |sum, (&c, &s)| sum ^ field.multiply(c, s));
        if discrepancy == 0 {
            shift += 1;
            continue;
        }
        let scale = field.multiply(discrepancy, field.inverse(last).unwrap_or(0));
        let grows = 2 * length <= n;
        if grows {
            saved.copy_from_slice(connection);
        }
        for (c, &b) in connection.iter_mut().skip(shift).zip(previous.iter()) {
            *c ^= field.multiply(scale, b);
        }
        if grows {
            previous.copy_from_slice(saved);
            (length, last, shift) = (n + 1 - length, discrepancy, 1);
        } else {
            shift += 1;
        }
    }
    length
}
