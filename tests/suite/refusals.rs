//! Wrong requests are refused before anything is written.

use crate::support::{FIELDS, data, data_in, pick, set};
use reed_solomon_core::{COMPARED, Code, CodeError, Field, Redundancy};

#[test]
fn thresholds_outside_the_field_are_refused() {
    assert_eq!(Code::new(Field::Gf256, 0), Err(CodeError::Threshold));
    assert_eq!(Code::new(Field::Gf256, 257), Err(CodeError::Threshold));
    assert_eq!(Code::new(Field::Gf65536, 65_537), Err(CodeError::Threshold));
    assert_eq!(Code::new(Field::Gf256, 256).unwrap().threshold(), 256);
    assert_eq!(
        Code::new(Field::Gf65536, 65_536).unwrap().field(),
        Field::Gf65536
    );
}

#[test]
fn wrong_sources_are_refused() {
    let code = Code::new(Field::Gf256, 3).unwrap();
    let mut weights = [0; 3];
    assert_eq!(
        code.sources(&[0, 1], &mut weights).err(),
        Some(CodeError::Count)
    );
    assert_eq!(
        code.sources(&[0, 1, 2, 3], &mut weights).err(),
        Some(CodeError::Count)
    );
    assert_eq!(
        code.sources(&[0, 1, 2], &mut weights[..2]).err(),
        Some(CodeError::Buffer)
    );
    assert_eq!(
        code.sources(&[0, 1, 256], &mut weights).err(),
        Some(CodeError::Index)
    );
    assert_eq!(
        code.sources(&[7, 1, 7], &mut weights).err(),
        Some(CodeError::Repeated)
    );
    // The same index list is fine in the larger field.
    let code = Code::new(Field::Gf65536, 3).unwrap();
    assert!(code.sources(&[0, 1, 256], &mut weights).is_ok());
}

#[test]
fn a_refused_shard_leaves_the_output_as_it_was() {
    let code = Code::new(Field::Gf256, 2).unwrap();
    let mut weights = [0; 2];
    let sources = code.sources(&[0, 1], &mut weights).unwrap();
    let (a, b, short) = ([1_u8, 2, 3, 4], [5_u8, 6, 7, 8], [9_u8, 9]);
    let mut out = [0xEE; 4];
    assert_eq!(
        sources.build(256, &[&a, &b], &mut out),
        Err(CodeError::Index)
    );
    assert_eq!(sources.build(2, &[&a], &mut out), Err(CodeError::Count));
    assert_eq!(
        sources.build(2, &[&a, &short], &mut out),
        Err(CodeError::Length)
    );
    assert_eq!(
        sources.build(2, &[&a, &b], &mut out[..3]),
        Err(CodeError::Length)
    );
    assert_eq!(out, [0xEE; 4]);
    assert_eq!(
        sources.agrees(2, &[&a, &short], &out),
        Err(CodeError::Length)
    );
    // Two-byte symbols: an odd length is not a whole number of them.
    let code = Code::new(Field::Gf65536, 2).unwrap();
    let sources = code.sources(&[0, 1], &mut weights).unwrap();
    let mut out = [0xEE; 3];
    assert_eq!(
        sources.build(2, &[&a[..3], &b[..3]], &mut out),
        Err(CodeError::Length)
    );
    assert_eq!(out, [0xEE; 3]);
}

#[test]
fn wrong_sets_of_whole_shards_are_refused() {
    let code = Code::new(Field::Gf256, 3).unwrap();
    let all = set(&code, &data(3, 5, 1), 5);
    assert_eq!(
        code.rebuild(&pick(&all, &[0, 4]), &[1]),
        Err(CodeError::Count)
    );
    // A repeated or unknown index is refused even beyond the shards that are used.
    let mut repeated = pick(&all, &[0, 1, 2, 3]);
    repeated.push((3, &all[3]));
    assert_eq!(code.rebuild(&repeated, &[4]), Err(CodeError::Repeated));
    let mut unknown = pick(&all, &[0, 1, 2]);
    unknown.push((256, &all[3]));
    assert_eq!(code.rebuild(&unknown, &[4]), Err(CodeError::Index));
    let mut uneven = pick(&all, &[0, 1, 2]);
    uneven.push((3, &all[3][..4]));
    assert_eq!(code.rebuild(&uneven, &[4]), Err(CodeError::Length));
    assert_eq!(
        code.rebuild(&pick(&all, &[0, 1, 2]), &[256]),
        Err(CodeError::Index)
    );
    let data: [&[u8]; 2] = [&[1], &[2]];
    assert_eq!(code.encode(&data, &[3]), Err(CodeError::Count));
}

#[test]
fn spare_shards_show_a_change_and_exactly_k_show_nothing() {
    for field in FIELDS {
        let code = Code::new(field, 3).unwrap();
        let all = set(&code, &data_in(field, 3, 600, 2), 6);
        assert_eq!(
            code.redundancy(&pick(&all, &[5, 0, 3])),
            Ok(Redundancy::None)
        );
        assert_eq!(
            code.redundancy(&pick(&all, &[5, 0, 3, 1, 4, 2])),
            Ok(Redundancy::Agrees)
        );
        // One changed bit in any shard, among the sources or the spare ones.
        for shard in 0..6 {
            for byte in [0, 255, 256, 599] {
                let mut changed = all.clone();
                // The lowest bit of a byte keeps a word below 2,048.
                changed[shard][byte] ^= 0x01;
                let found = code.redundancy(&pick(&changed, &[5, 0, 3, 1, 4, 2]));
                assert_eq!(found, Ok(Redundancy::Differs), "shard {shard} byte {byte}");
            }
        }
    }
}

#[test]
fn a_symbol_that_is_no_word_is_refused_wherever_it_is_given() {
    let code = Code::new(Field::Gf2048, 2).unwrap();
    let good: [&[u8]; 2] = [&[0x00, 0x01], &[0x07, 0xFF]];
    let set = code.encode(&good, &[0, 1, 2, 3]).unwrap();
    // 2,048 itself, as data.
    let bad: [&[u8]; 2] = [&[0x08, 0x00], &[0x00, 0x00]];
    assert_eq!(code.encode(&bad, &[2]), Err(CodeError::Symbol));
    // Among the data with nothing asked for, and in a shard beyond the threshold.
    assert_eq!(code.encode(&bad, &[]), Err(CodeError::Symbol));
    let spare = [(0, &set[0][..]), (1, &set[1][..]), (2, &[0xF0, 0x00][..])];
    assert_eq!(code.rebuild(&spare, &[3]), Err(CodeError::Symbol));
    assert_eq!(code.redundancy(&spare), Err(CodeError::Symbol));
    // With exactly the threshold, before "nothing to compare with" and "unchecked".
    let exact = [(0, &[0x08, 0x00][..]), (1, &set[1][..])];
    assert_eq!(code.redundancy(&exact), Err(CodeError::Symbol));
    let mut first = [0x08, 0x00];
    let mut second = [0x00, 0x00];
    let mut given: Vec<(u16, &mut [u8])> = vec![(0, &mut first[..]), (1, &mut second[..])];
    assert_eq!(code.correct(&mut given), Err(CodeError::Symbol));
    // On the stripe interface: a source, and the shard compared; the output is left as it was.
    let mut weights = [0; 2];
    let sources = code.sources(&[0, 1], &mut weights).unwrap();
    let mut out = [0xAA, 0xBB];
    assert_eq!(sources.build(2, &bad, &mut out), Err(CodeError::Symbol));
    assert_eq!(sources.build(0, &bad, &mut out), Err(CodeError::Symbol));
    assert_eq!(out, [0xAA, 0xBB]);
    assert_eq!(
        sources.agrees(2, &good, &[0x08, 0x00]),
        Err(CodeError::Symbol)
    );
    // What the output held before is not a symbol given: any bytes do.
    assert_eq!(sources.build(2, &good, &mut out), Ok(()));
    assert_eq!(out, set[2][..]);
    // The other fields take any bytes.
    for field in [Field::Gf256, Field::Gf65536] {
        let code = Code::new(field, 2).unwrap();
        let any: [&[u8]; 2] = [&[0xFF, 0xFF], &[0x08, 0x00]];
        assert!(code.encode(&any, &[2]).is_ok());
    }
}

#[test]
fn the_field_of_words_has_2048_shards_and_no_more() {
    assert_eq!(Code::new(Field::Gf2048, 2049), Err(CodeError::Threshold));
    assert_eq!(Code::new(Field::Gf2048, 2048).unwrap().threshold(), 2048);
    let code = Code::new(Field::Gf2048, 2).unwrap();
    let data: [&[u8]; 2] = [&[0x00, 0x01], &[0x00, 0x02]];
    assert!(code.encode(&data, &[2047]).is_ok());
    assert_eq!(code.encode(&data, &[2048]), Err(CodeError::Index));
    // The memory bound: nothing below the threshold or above the field, the indexes at exactly
    // the threshold, and under 8 MiB for the whole field.
    for shards in [0, 1, 2049, usize::MAX] {
        assert_eq!(code.correction_memory(shards), 0, "{shards}");
    }
    assert_eq!(code.correction_memory(2), 2 * size_of::<u16>());
    let whole = code.correction_memory(2048);
    assert!(whole > 2048 * size_of::<u16>() && whole < 8 << 20);
}

#[test]
fn a_wrong_number_is_reported_before_a_wrong_symbol() {
    // A target the field does not have, together with a symbol that is no word: the numbers
    // come first in every call.
    let code = Code::new(Field::Gf2048, 2).unwrap();
    let bad: [&[u8]; 2] = [&[0x08, 0x00], &[0x00, 0x00]];
    assert_eq!(code.encode(&bad, &[2048]), Err(CodeError::Index));
    let given = [(0, bad[0]), (1, bad[1])];
    assert_eq!(code.rebuild(&given, &[2, 2048]), Err(CodeError::Index));
    let mut weights = [0; 2];
    let sources = code.sources(&[0, 1], &mut weights).unwrap();
    let mut out = [0xAA, 0xBB];
    assert_eq!(sources.build(2048, &bad, &mut out), Err(CodeError::Index));
    assert_eq!(out, [0xAA, 0xBB]);
    assert_eq!(
        sources.agrees(2048, &bad, &[0x08, 0x00]),
        Err(CodeError::Index)
    );
    // And the lengths: a stripe of another length with a symbol that is no word.
    let uneven: [&[u8]; 2] = [&[0x08, 0x00], &[0x00, 0x00, 0x00, 0x00]];
    assert_eq!(sources.build(2, &uneven, &mut out), Err(CodeError::Length));
    assert_eq!(code.encode(&uneven, &[2]), Err(CodeError::Length));
}

/// `Sources::agrees_in` refuses as `agrees` does, with a room shorter than `COMPARED` after the
/// counts and lengths, and answers as `agrees` in any room long enough.
#[test]
fn a_comparison_in_the_callers_room_answers_as_agrees() {
    let code = Code::new(Field::Gf2048, 2).unwrap();
    let mut weights = [0; 2];
    let sources = code.sources(&[0, 1], &mut weights).unwrap();
    let good: [&[u8]; 2] = [&[0x01, 0x23], &[0x04, 0x56]];
    let mut made = [0; 2];
    sources.build(2, &good, &mut made).unwrap();
    let mut room = vec![0xEE; COMPARED + 44];
    for length in [COMPARED, COMPARED + 44] {
        let room = &mut room[..length];
        assert_eq!(sources.agrees_in(2, &good, &made, room), Ok(true));
        let other = [0x00, 0x01];
        let answer = sources.agrees_in(2, &good, &other, room);
        assert_eq!(
            (answer, sources.agrees(2, &good, &other)),
            (Ok(false), Ok(false))
        );
    }
    let mut short = [0; COMPARED - 1];
    assert_eq!(
        sources.agrees_in(2, &good[..1], &made, &mut short),
        Err(CodeError::Count)
    );
    assert_eq!(
        sources.agrees_in(2, &good, &made[..1], &mut short),
        Err(CodeError::Length)
    );
    assert_eq!(
        sources.agrees_in(2048, &good, &made, &mut short),
        Err(CodeError::Buffer)
    );
    assert_eq!(
        sources.agrees_in(2, &good, &made, &mut []),
        Err(CodeError::Buffer)
    );
    assert_eq!(
        sources.agrees_in(2048, &good, &made, &mut room),
        Err(CodeError::Index)
    );
    assert_eq!(
        sources.agrees_in(2, &good, &[0x08, 0x00], &mut room),
        Err(CodeError::Symbol)
    );
}
