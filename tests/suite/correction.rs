//! Wrong symbols at unknown places: found and corrected within the bound, never called clean
//! beyond it.

use crate::support::{FIELDS, bytes, data, data_in, plain, set, symbols};
use reed_solomon_core::{Code, CodeError, Correction, Field, Redundancy};

/// Shard indexes with gaps and out of order; zero only among the first eight.
const SCATTERED: [u16; 9] = [200, 3, 255, 9, 4, 17, 1, 0, 100];
/// The same without the point zero.
const WITHOUT_ZERO: [u16; 9] = [200, 3, 255, 9, 4, 17, 1, 250, 100];

/// The shards numbered `indexes` of a set of `k` data shards.
fn shards_at(code: &Code, k: usize, length: usize, seed: u64, indexes: &[u16]) -> Vec<Vec<u8>> {
    let data = data_in(code.field(), k, length, seed);
    let data: Vec<&[u8]> = data.iter().map(|bytes| bytes.as_slice()).collect();
    plain(code.encode(&data, indexes).unwrap())
}

/// Runs the correction on `shards` as the shards numbered `indexes`.
fn correct(code: &Code, indexes: &[u16], shards: &mut [Vec<u8>]) -> Correction {
    let mut given: Vec<(u16, &mut [u8])> = indexes
        .iter()
        .copied()
        .zip(shards.iter_mut().map(|bytes| bytes.as_mut_slice()))
        .collect();
    code.correct(&mut given).unwrap()
}

/// A nonzero change for the symbol of `shard` in `column`.
fn error(field: Field, shard: usize, column: usize) -> Vec<u8> {
    let random = bytes((shard * 1000 + column) as u64, field.symbol_bytes());
    let mut change = symbols(field, random);
    if change.iter().all(|&byte| byte == 0) {
        change[field.symbol_bytes() - 1] = 1;
    }
    change
}

/// Changes the symbol of `shard` in `column`.
fn damage(field: Field, shards: &mut [Vec<u8>], shard: usize, column: usize) {
    let at = column * field.symbol_bytes();
    for (byte, change) in shards[shard][at..]
        .iter_mut()
        .zip(error(field, shard, column))
    {
        *byte ^= change;
    }
}

#[test]
fn the_two_reviewed_words_are_corrected() {
    let code = Code::new(Field::Gf256, 3).unwrap();
    // The zero code word with the error 01 in shard 0, the point zero.
    let mut word = vec![vec![0_u8]; 5];
    word[0][0] = 0x01;
    assert_eq!(
        correct(&code, &[0, 1, 2, 3, 4], &mut word),
        Correction::Corrected(vec![1, 0, 0, 0, 0])
    );
    assert_eq!(word, vec![vec![0_u8]; 5]);
    // Errors 01 in shard 0 and 52 in shard 2: two wrong symbols, the most four spare shards find.
    let mut word = vec![vec![0_u8]; 7];
    (word[0][0], word[2][0]) = (0x01, 0x52);
    assert_eq!(
        correct(&code, &[0, 1, 2, 3, 4, 5, 6], &mut word),
        Correction::Corrected(vec![1, 0, 1, 0, 0, 0, 0])
    );
    assert_eq!(word, vec![vec![0_u8]; 7]);
}

#[test]
fn every_placement_of_wrong_symbols_within_the_bound_is_corrected() {
    let in_order: Vec<u16> = (0..9).collect();
    let mut cases = 0;
    for field in FIELDS {
        for indexes in [&in_order[..], &SCATTERED, &WITHOUT_ZERO] {
            for m in 3..=9_usize {
                for k in 1..m {
                    let code = Code::new(field, k).unwrap();
                    let whole = shards_at(&code, k, 3 * field.symbol_bytes(), 5, &indexes[..m]);
                    for mask in 1_u32..1 << m {
                        // Each wrong symbol takes two spare shards.
                        if 2 * mask.count_ones() as usize > m - k {
                            continue;
                        }
                        // Column 1 carries the wrong symbols; columns 0 and 2 stay clean.
                        let mut shards = whole.clone();
                        let mut expected = vec![0; m];
                        for shard in (0..m).filter(|shard| mask >> shard & 1 == 1) {
                            damage(field, &mut shards, shard, 1);
                            expected[shard] = 1;
                        }
                        let found = correct(&code, &indexes[..m], &mut shards);
                        assert_eq!(
                            found,
                            Correction::Corrected(expected),
                            "{k} of {m} {mask:b}"
                        );
                        assert_eq!(shards, whole, "{k} of {m} {mask:b}");
                        cases += 1;
                    }
                }
            }
        }
    }
    assert!(cases > 3000, "{cases}");
}

#[test]
fn one_wrong_symbol_too_many_is_never_called_clean() {
    let indexes: Vec<u16> = (0..9).collect();
    for field in FIELDS {
        for m in 2..=9_usize {
            for k in 1..m {
                let code = Code::new(field, k).unwrap();
                let limit = (m - k) / 2;
                let whole = shards_at(&code, k, field.symbol_bytes(), 6, &indexes[..m]);
                for mask in 1_u32..1 << m {
                    if mask.count_ones() as usize != limit + 1 {
                        continue;
                    }
                    let mut shards = whole.clone();
                    for shard in (0..m).filter(|shard| mask >> shard & 1 == 1) {
                        damage(field, &mut shards, shard, 0);
                    }
                    let received = shards.clone();
                    match correct(&code, &indexes[..m], &mut shards) {
                        // Nothing may change when the column is given up.
                        Correction::Unresolved { columns: 1 } => assert_eq!(shards, received),
                        // Another code word may be nearer than the original: it must be a code
                        // word, and no further from what was received than the bound.
                        Correction::Corrected(changed) => {
                            assert!(changed.iter().sum::<usize>() <= limit);
                            let given: Vec<(u16, &[u8])> = indexes
                                .iter()
                                .copied()
                                .zip(shards.iter().map(|bytes| bytes.as_slice()))
                                .collect();
                            assert_eq!(code.redundancy(&given), Ok(Redundancy::Agrees));
                            assert_ne!(shards, whole);
                        }
                        other => panic!("{k} of {m} {mask:b}: {other:?}"),
                    }
                }
            }
        }
    }
}

/// The code word within `limit` symbols of `received`, found without the decoder: every choice
/// of `k` shards is taken as right and the others compared with what it makes.
fn nearest(
    code: &Code,
    indexes: &[u16],
    received: &[Vec<u8>],
    limit: usize,
) -> Option<Vec<Vec<u8>>> {
    let (k, m) = (code.threshold(), indexes.len());
    for mask in 0_u32..1 << m {
        if mask.count_ones() as usize != k {
            continue;
        }
        let chosen: Vec<(u16, &[u8])> = (0..m)
            .filter(|i| mask >> i & 1 == 1)
            .map(|i| (indexes[i], received[i].as_slice()))
            .collect();
        let word = plain(code.rebuild(&chosen, indexes).unwrap());
        let distance = word.iter().zip(received).filter(|(a, b)| a != b).count();
        if distance <= limit {
            return Some(word);
        }
    }
    None
}

#[test]
fn the_decoder_finds_what_a_search_over_every_choice_of_k_shards_finds() {
    let mut outcomes = [0; 3];
    for (round, m) in (0..2000_u64).zip((3..=7_usize).cycle()) {
        let k = 1 + (round as usize / 5) % (m - 1).min(3);
        let code = Code::new(Field::Gf256, k).unwrap();
        let indexes = &SCATTERED[..m];
        let limit = (m - k) / 2;
        let mut received = shards_at(&code, k, 1, round, indexes);
        // From no wrong symbol to one more than the bound, at places the round picks.
        let random = bytes(round, 16);
        for wrong in 0..usize::from(random[0]) % (limit + 2) {
            let shard = usize::from(random[1 + wrong]) % m;
            received[shard][0] ^= random[8 + wrong] | 1;
        }
        let expected = nearest(&code, indexes, &received, limit);
        let mut shards = received.clone();
        match (correct(&code, indexes, &mut shards), expected) {
            (Correction::Agrees, Some(word)) => {
                assert_eq!(word, received);
                outcomes[0] += 1;
            }
            (Correction::Corrected(_), Some(word)) => {
                assert_eq!(shards, word, "round {round}");
                outcomes[1] += 1;
            }
            (Correction::Unresolved { .. }, None) => {
                assert_eq!(shards, received);
                outcomes[2] += 1;
            }
            (found, expected) => panic!("round {round}: {found:?} against {expected:?}"),
        }
    }
    // Each outcome was met often enough to mean something.
    assert!(outcomes.iter().all(|&count| count > 100), "{outcomes:?}");
}

#[test]
fn nothing_changes_unless_every_column_is_put_right() {
    let code = Code::new(Field::Gf256, 2).unwrap();
    let indexes = [0, 1, 2, 3, 4];
    let whole = shards_at(&code, 2, 4, 8, &indexes);
    let mut shards = whole.clone();
    // Column 0: one wrong symbol, which three spare shards find. Column 2: two, which they do not.
    damage(Field::Gf256, &mut shards, 1, 0);
    damage(Field::Gf256, &mut shards, 0, 2);
    damage(Field::Gf256, &mut shards, 3, 2);
    let received = shards.clone();
    let found = correct(&code, &indexes, &mut shards);
    if found == (Correction::Unresolved { columns: 1 }) {
        assert_eq!(shards, received);
    } else {
        // Two wrong symbols of five may also sit one symbol from another code word.
        assert!(matches!(found, Correction::Corrected(_)), "{found:?}");
    }
    // With the second column clean again, the first is corrected.
    let mut shards = received;
    shards[0][2] = whole[0][2];
    shards[3][2] = whole[3][2];
    assert_eq!(
        correct(&code, &indexes, &mut shards),
        Correction::Corrected(vec![0, 1, 0, 0, 0])
    );
    assert_eq!(shards, whole);
}

#[test]
fn exactly_k_shards_are_not_checked_and_clean_shards_agree() {
    let code = Code::new(Field::Gf65536, 3).unwrap();
    let indexes = [0, 255, 256, 65_535, 4660];
    let whole = shards_at(&code, 3, 8, 3, &indexes);
    let mut shards = whole.clone();
    assert_eq!(correct(&code, &indexes, &mut shards), Correction::Agrees);
    // One wrong symbol in the shard with the highest index of the field.
    damage(Field::Gf65536, &mut shards, 3, 2);
    assert_eq!(
        correct(&code, &indexes, &mut shards),
        Correction::Corrected(vec![0, 0, 0, 1, 0])
    );
    assert_eq!(shards, whole);
    // Three shards of three: any bytes form a set, so nothing can be said.
    damage(Field::Gf65536, &mut shards, 0, 0);
    assert_eq!(
        correct(&code, &indexes[..3], &mut shards[..3]),
        Correction::Unchecked
    );
}

#[test]
fn a_line_with_unreadable_and_misread_symbols_comes_back() {
    // A line of 20 symbols holding 12: 8 spare. Two unreadable and three misread: 2·3 + 2 = 8.
    for field in FIELDS {
        let code = Code::new(field, 12).unwrap();
        let all: Vec<u16> = (0..20).collect();
        let line = shards_at(&code, 12, field.symbol_bytes(), 11, &all);
        let unreadable = [4, 17];
        let readable: Vec<u16> = all
            .iter()
            .copied()
            .filter(|i| !unreadable.contains(i))
            .collect();
        let mut read: Vec<Vec<u8>> = readable
            .iter()
            .map(|&i| line[usize::from(i)].clone())
            .collect();
        for misread in [0, 9, 15] {
            damage(field, &mut read, misread, 0);
        }
        let found = correct(&code, &readable, &mut read);
        assert!(matches!(found, Correction::Corrected(ref c) if c.iter().sum::<usize>() == 3));
        let given: Vec<(u16, &[u8])> = readable
            .iter()
            .copied()
            .zip(read.iter().map(|bytes| bytes.as_slice()))
            .collect();
        assert_eq!(plain(code.rebuild(&given, &all).unwrap()), line);
    }
}

#[test]
fn a_wholly_replaced_long_shard_is_put_right() {
    // Longer than the syndrome memory of two spare shards, so several stripes.
    let length = (1 << 21) + 10;
    let code = Code::new(Field::Gf256, 3).unwrap();
    let whole = set(&code, &data(3, length, 12), 5);
    let mut shards = whole.clone();
    shards[1] = bytes(99, length);
    let wrong = shards[1]
        .iter()
        .zip(&whole[1])
        .filter(|(a, b)| a != b)
        .count();
    let found = correct(&code, &[0, 1, 2, 3, 4], &mut shards);
    assert_eq!(found, Correction::Corrected(vec![0, wrong, 0, 0, 0]));
    assert_eq!(shards, whole);
}

#[test]
fn two_thousand_wrong_symbols_in_one_column_are_found() {
    // 4,200 one-symbol shards of a set of 104: 4,096 spare, so 2,048 wrong symbols at most.
    let (k, m) = (104, 4200);
    let code = Code::new(Field::Gf65536, k).unwrap();
    let indexes: Vec<u16> = (0..m as u16).collect();
    let whole = shards_at(&code, k, 2, 13, &indexes);
    let mut shards = whole.clone();
    let mut expected = vec![0; m];
    // Every second shard up to the bound, shard 0 among them.
    for shard in (0..m).step_by(2).take(2048) {
        damage(Field::Gf65536, &mut shards, shard, 0);
        expected[shard] = 1;
    }
    assert_eq!(
        correct(&code, &indexes, &mut shards),
        Correction::Corrected(expected)
    );
    assert_eq!(shards, whole);
    // One more wrong symbol is beyond the bound: given up, or another code word, never the original.
    for shard in (0..m).step_by(2).take(2049) {
        damage(Field::Gf65536, &mut shards, shard, 0);
    }
    let received = shards.clone();
    match correct(&code, &indexes, &mut shards) {
        Correction::Unresolved { columns: 1 } => assert_eq!(shards, received),
        Correction::Corrected(_) => assert_ne!(shards, whole),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_memory_bound_is_defined_for_every_number_of_shards() {
    for (field, k) in [(Field::Gf256, 8), (Field::Gf65536, 3)] {
        let code = Code::new(field, k).unwrap();
        // Too few shards, and more than the field has indexes: refused before memory is taken.
        for shards in [0, k - 1, field.order() + 1, usize::MAX] {
            assert_eq!(code.correction_memory(shards), 0, "{field:?} {shards}");
        }
        // Exactly the threshold: only the indexes are held while they are checked.
        assert_eq!(code.correction_memory(k), k * size_of::<u16>());
        // With spare shards the bound holds the search as well, and stays under 8 MiB.
        for shards in [k + 1, k + 2, field.order()] {
            let bound = code.correction_memory(shards);
            assert!(bound > shards * size_of::<u16>() && bound < 8 << 20);
        }
    }
}

#[test]
fn the_numbers_of_shards_without_a_bound_are_the_ones_correct_refuses() {
    let code = Code::new(Field::Gf256, 8).unwrap();
    let run = |indexes: &[u16]| {
        let mut bytes = vec![[0_u8; 1]; indexes.len()];
        let mut shards: Vec<(u16, &mut [u8])> = indexes
            .iter()
            .copied()
            .zip(bytes.iter_mut().map(|b| &mut b[..]))
            .collect();
        code.correct(&mut shards)
    };
    let all: Vec<u16> = (0..256).collect();
    // Fewer than the threshold.
    assert_eq!(run(&all[..7]), Err(CodeError::Count));
    // Exactly the threshold: nothing to compare with.
    assert_eq!(run(&all[..8]), Ok(Correction::Unchecked));
    // Every index of the field: a set, and all zero bytes agree.
    assert_eq!(run(&all), Ok(Correction::Agrees));
    // One shard more than the field has indexes: an index is there twice, or is not the field's.
    let mut repeated = all.clone();
    repeated.push(0);
    assert_eq!(run(&repeated), Err(CodeError::Repeated));
    let mut unknown = all;
    unknown.push(256);
    assert_eq!(run(&unknown), Err(CodeError::Index));
}

#[test]
fn the_reviewed_words_of_the_word_field_are_corrected() {
    // The zero code word of threshold 3 over GF(2^11) with the errors whose syndromes were
    // computed apart: one in shard 0; two, in shards 0 and 2; and two whose first syndrome is
    // zero.
    let code = Code::new(Field::Gf2048, 3).unwrap();
    for (errors, expected) in [
        (&[0x001_u16, 0, 0, 0, 0][..], &[1, 0, 0, 0, 0][..]),
        (&[0x001, 0, 0x052, 0, 0, 0, 0], &[1, 0, 1, 0, 0, 0, 0]),
        (&[0x001, 0, 0x2AA, 0, 0, 0, 0], &[1, 0, 1, 0, 0, 0, 0]),
    ] {
        let indexes: Vec<u16> = (0..errors.len() as u16).collect();
        let mut shards: Vec<Vec<u8>> = errors.iter().map(|e| e.to_be_bytes().to_vec()).collect();
        let found = correct(&code, &indexes, &mut shards);
        assert_eq!(found, Correction::Corrected(expected.to_vec()));
        assert!(shards.iter().all(|shard| shard == &[0, 0]));
    }
}

#[test]
fn a_phrase_of_words_with_repair_words_survives_lost_and_wrong_words() {
    // Twenty-four words and six repair words, as numbers 0 to 2,047: two lost and two wrong
    // is 2 · 2 + 2 = 6, the most six repair words put right.
    let code = Code::new(Field::Gf2048, 24).unwrap();
    let all: Vec<u16> = (0..30).collect();
    let phrase = shards_at(&code, 24, 2, 12, &all);
    assert!(
        phrase
            .iter()
            .all(|word| u16::from_be_bytes([word[0], word[1]]) < 2048)
    );
    let lost = [3, 27];
    let readable: Vec<u16> = all.iter().copied().filter(|i| !lost.contains(i)).collect();
    let mut read: Vec<Vec<u8>> = readable
        .iter()
        .map(|&i| phrase[usize::from(i)].clone())
        .collect();
    for wrong in [0, 20] {
        damage(Field::Gf2048, &mut read, wrong, 0);
    }
    let found = correct(&code, &readable, &mut read);
    assert!(matches!(found, Correction::Corrected(ref c) if c.iter().sum::<usize>() == 2));
    let given: Vec<(u16, &[u8])> = readable
        .iter()
        .copied()
        .zip(read.iter().map(|bytes| bytes.as_slice()))
        .collect();
    assert_eq!(plain(code.rebuild(&given, &all).unwrap()), phrase);
}

#[test]
fn beyond_the_bound_another_valid_set_may_be_the_answer() {
    // GF(2^8), threshold 3, seven shards: the values x · (x + 1) at the points 0 to 6 are a code
    // word, zero at the points 0 and 1 and nowhere else, five symbols from the all-zero word.
    let code = Code::new(Field::Gf256, 3).unwrap();
    let indexes: Vec<u16> = (0..7).collect();
    let data: [&[u8]; 3] = [&[0], &[0], &[6]];
    let word = plain(code.encode(&data, &indexes).unwrap());
    assert_eq!(word.iter().filter(|shard| shard[0] != 0).count(), 5);
    // Given as it is, it agrees: nothing tells it from an all-zero set that suffered five errors.
    let mut shards = word.clone();
    assert_eq!(correct(&code, &indexes, &mut shards), Correction::Agrees);
    assert_eq!(shards, word);
    // The all-zero word with three wrong symbols, those of shards 4, 5 and 6 of that word: one
    // more than four spare shards put right. It lies two symbols from the other word, and that
    // is the answer.
    let mut shards: Vec<Vec<u8>> = vec![vec![0]; 7];
    for wrong in [4, 5, 6] {
        shards[wrong] = word[wrong].clone();
    }
    let found = correct(&code, &indexes, &mut shards);
    assert_eq!(found, Correction::Corrected(vec![0, 0, 1, 1, 0, 0, 0]));
    assert_eq!(shards, word);
    // With exactly the threshold nothing is checked at all.
    let mut three: Vec<Vec<u8>> = vec![vec![1], vec![2], vec![3]];
    assert_eq!(
        correct(&code, &indexes[..3], &mut three),
        Correction::Unchecked
    );
}
