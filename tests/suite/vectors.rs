//! Known answers from outside this crate.

use crate::support::plain;
use reed_solomon_core::{Code, Field};

#[test]
fn four_one_byte_shards_give_the_published_parity() {
    // 01 02 03 04 with two parity shards: 45 5E, the parity of reed-solomon-erasure 6.0.0 and of
    // klauspost's Go code for the same shape.
    let code = Code::new(Field::Gf256, 4).unwrap();
    let data: [&[u8]; 4] = [&[1], &[2], &[3], &[4]];
    assert_eq!(
        plain(code.encode(&data, &[4, 5]).unwrap()),
        [[0x45], [0x5E]]
    );
}

#[test]
fn a_set_of_three_grows_as_lagrange_interpolation_by_hand_says() {
    // Computed apart from this crate by interpolation over GF(2^8) with 0x11D: the data 46 91 23
    // gives the five shards 46 91 23 F4 F3, and the shards 1, 3 and 4 give the shards 5 and 6
    // as 24 and 96.
    let code = Code::new(Field::Gf256, 3).unwrap();
    let data: [&[u8]; 3] = [&[0x46], &[0x91], &[0x23]];
    let five = plain(code.encode(&data, &[0, 1, 2, 3, 4]).unwrap());
    assert_eq!(five.concat(), [0x46, 0x91, 0x23, 0xF4, 0xF3]);
    let old = [(1, &five[1][..]), (3, &five[3][..]), (4, &five[4][..])];
    assert_eq!(
        plain(code.rebuild(&old, &[5, 6]).unwrap()).concat(),
        [0x24, 0x96]
    );
}

#[test]
fn the_data_shards_are_the_data_itself() {
    for field in [Field::Gf256, Field::Gf65536, Field::Gf2048] {
        let code = Code::new(field, 3).unwrap();
        let data: [&[u8]; 3] = [&[1, 2], &[3, 4], &[5, 6]];
        let first = plain(code.encode(&data, &[0, 1, 2]).unwrap());
        assert_eq!(first, data);
    }
}

#[test]
fn three_words_give_the_two_words_computed_apart() {
    // GF(2^11) with 0x805, computed apart from this crate: the polynomial through 001, 7FF and
    // 400 at the points 0, 1 and 2 has the coefficients 001, 0A9, 757 and the values 3FE and 7E3
    // at the points 3 and 4. A word is two bytes, the high one first.
    let code = Code::new(Field::Gf2048, 3).unwrap();
    let data: [&[u8]; 3] = [&[0x00, 0x01], &[0x07, 0xFF], &[0x04, 0x00]];
    let more = plain(code.encode(&data, &[3, 4]).unwrap());
    assert_eq!(more, [[0x03, 0xFE], [0x07, 0xE3]]);
    // The first and the last word of a list of 2,048 are 00 00 and 07 FF; both are symbols.
    let ends: [&[u8]; 3] = [&[0x00, 0x00], &[0x07, 0xFF], &[0x00, 0x00]];
    assert!(code.encode(&ends, &[3]).is_ok());
}
