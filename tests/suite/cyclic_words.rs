//! The cyclic code in memory the caller gives: the same generators, check shards and checks as
//! the code that allocates, one word put right in place as MHFE-REPAIR-1 publishes it and as that
//! code's plans put it right, and the refusals, each before anything is written.

use crate::cyclic::{MHFE, MHFE_REPAIRS, shards, views, word};
use crate::support::{bytes, symbols};
use reed_solomon_core::{CodeError, CyclicCode, CyclicWords, Field, Mark, Plan};

/// Codes of several fields and roots: the field, data, parity, first root and step between roots.
fn shapes() -> Vec<(Field, usize, usize, u32, u32)> {
    vec![
        (Field::Gf2048, 12, 4, 1, 1),
        (Field::Gf2048, 24, 8, 1, 1),
        (Field::Gf256, 16, 10, 0, 1),
        (Field::Gf2048, 5, 4, 5, 3),
        (Field::binary(4, None).unwrap(), 9, 6, 2, 1),
        (Field::binary(13, None).unwrap(), 7, 3, 0, 5),
    ]
}

/// The words of shards of one symbol each.
fn words_of(shards: &[Vec<u8>]) -> Vec<u16> {
    let word = |shard: &Vec<u8>| shard.iter().fold(0, |w, &b| w << 8 | u16::from(b));
    shards.iter().map(word).collect()
}

/// The generator, the check shards and the check are those of `CyclicCode`, whatever room the
/// syndromes get, and that room is wiped.
#[test]
fn the_code_in_given_memory_is_the_code_that_allocates() {
    for (field, data, parity, first, step) in shapes() {
        let code = CyclicCode::new(field, data, parity, first, step).unwrap();
        let mut generator = vec![0; parity + 1];
        let words = CyclicWords::new(field, data, parity, first, step, &mut generator).unwrap();
        assert_eq!(words, code.words());
        assert_eq!(words.generator(), code.generator());
        let shape = (words.field(), words.data(), words.parity(), words.length());
        assert_eq!(shape, (field, data, parity, data + parity));
        let (width, columns) = (field.symbol_bytes(), 37);
        let given: Vec<Vec<u8>> = (0..data)
            .map(|i| symbols(field, bytes(i as u64 + 80, columns * width)))
            .collect();
        let made = code.encode(&views(&given)).unwrap();
        let mut checks = vec![vec![0xEE; columns * width]; parity];
        let mut outs: Vec<&mut [u8]> = checks.iter_mut().map(Vec::as_mut_slice).collect();
        let mut remainder = vec![0; parity];
        words
            .encode(&views(&given), &mut outs, &mut remainder)
            .unwrap();
        let made: Vec<Vec<u8>> = made.iter().map(|shard| shard.to_vec()).collect();
        assert_eq!(checks, made, "{field:?}");
        let whole: Vec<Vec<u8>> = given.into_iter().chain(checks).collect();
        let mut late = whole.clone();
        late[data + parity - 1][columns * width - 1] ^= 1;
        // A word a stripe, five words a stripe and a short last one, every word in one stripe.
        for room in [1, 5, columns + 3] {
            let mut syndromes = vec![0xEE; room * parity * width];
            assert!(words.agrees(&views(&whole), &mut syndromes).unwrap());
            assert!(!words.agrees(&views(&late), &mut syndromes).unwrap());
            assert!(syndromes.iter().all(|&byte| byte == 0));
        }
    }
}

/// The repairs of MHFE-REPAIR-1 as published, each word put right in place, its room wiped.
#[test]
fn the_repairs_of_mhfe_put_one_word_right_in_place() {
    for (case, parity, damaged, unreadable, replaced, filled) in MHFE_REPAIRS {
        let (data, parities) = MHFE[case];
        let mut generator = vec![0; parity + 1];
        let code = CyclicWords::new(Field::Gf2048, data.len(), parity, 1, 1, &mut generator);
        let code = code.unwrap();
        let whole: Vec<u16> = data
            .iter()
            .chain(parities[parity / 2 - 1])
            .copied()
            .collect();
        let mut read = damaged.to_vec();
        let mut room = vec![0xFFFF; CyclicWords::room(parity)];
        let written = code.correct_word(&mut read, unreadable, &mut room);
        assert_eq!(written, Ok(Some(replaced.len() + filled.len())));
        assert_eq!(read, whole);
        assert!(room.iter().all(|&element| element == 0));
    }
}

/// GF(2^11), first root 5, step 3: a word of the code is left as it is, a marked symbol is never
/// read, even outside the field, and a word with no word of the code within the bound, three
/// wrong symbols or five marks, stays as it was.
#[test]
fn a_word_beyond_the_bound_stays_as_it_was() {
    let mut generator = [0; 5];
    let code = CyclicWords::new(Field::Gf2048, 5, 4, 5, 3, &mut generator).unwrap();
    let whole = [
        0x001, 0x7FF, 0x400, 0x123, 0x000, 0x410, 0x680, 0x321, 0x134,
    ];
    let mut room = [0; CyclicWords::room(4)];
    let mut read = whole;
    assert_eq!(code.correct_word(&mut read, &[], &mut room), Ok(Some(0)));
    // A marked place is written even when it held its value.
    assert_eq!(code.correct_word(&mut read, &[4], &mut room), Ok(Some(1)));
    assert_eq!(read, whole);
    (read[3], read[6]) = (0xFFFF, 0x0800);
    assert_eq!(
        code.correct_word(&mut read, &[3, 6], &mut room),
        Ok(Some(2))
    );
    assert_eq!(read, whole);
    let mut three = whole;
    for place in [0, 2, 4] {
        three[place] ^= 1;
    }
    let mut read = three;
    assert_eq!(code.correct_word(&mut read, &[], &mut room), Ok(None));
    let five = [0, 1, 2, 3, 4];
    assert_eq!(code.correct_word(&mut read, &five, &mut room), Ok(None));
    assert_eq!(read, three);
}

/// Random words and errata in several fields and roots: a word put right in place becomes what the
/// plan of `CyclicCode` makes of it, and stays as it was when that plan finds no word. Every fourth
/// word is left whole, so that the rounds meet each of the three answers.
#[test]
fn one_word_in_place_follows_the_plan_of_the_code_that_allocates() {
    let mut draw = bytes(7, 1 << 16).into_iter().cycle();
    let mut next = move |bound: usize| usize::from(draw.next().unwrap()) % bound.max(1);
    let shapes = shapes();
    let mut met = [0; 3];
    for round in 0..600 {
        let (field, data, parity, first, step) = shapes[round % shapes.len()];
        let code = CyclicCode::new(field, data, parity, first, step).unwrap();
        let (length, order) = (data + parity, field.order());
        let given: Vec<u16> = (0..data)
            .map(|_| (next(256) * 257 % order) as u16)
            .collect();
        let mut read = word(&code, &given);
        let (wrong, marked) = match round % 4 {
            0 => (0, 0),
            _ => (next(parity + 2), next(parity + 2)),
        };
        for _ in 0..wrong {
            read[next(length)] = (next(256) * 131 % order) as u16;
        }
        let mut erased: Vec<usize> = (0..marked).map(|_| next(length)).collect();
        erased.sort_unstable();
        erased.dedup();
        let marks: Vec<Mark> = erased
            .iter()
            .map(|&shard| Mark { shard, symbol: 0 })
            .collect();
        let mut held = shards(field, &read);
        let plan = code.plan(&views(&held), &[], &marks).unwrap();
        let mut fixed = read.clone();
        let mut room = vec![0; CyclicWords::room(parity)];
        let written = code.words().correct_word(&mut fixed, &erased, &mut room);
        match plan {
            Plan::Agrees => {
                assert_eq!((written, &fixed), (Ok(Some(0)), &read));
                met[0] += 1;
            }
            Plan::Changes(changes) => {
                assert_eq!(written, Ok(Some(changes.len())), "round {round}");
                met[1] += 1;
                let mut buffers: Vec<&mut [u8]> = held.iter_mut().map(Vec::as_mut_slice).collect();
                code.apply(&mut buffers, &changes).unwrap();
                assert_eq!(fixed, words_of(&held), "round {round}");
            }
            Plan::Unresolved { columns: 1 } => {
                assert_eq!((written, &fixed), (Ok(None), &read), "round {round}");
                met[2] += 1;
            }
            other => panic!("round {round}: {other:?}"),
        }
    }
    assert!(met.iter().all(|&count| count > 20), "{met:?}");
}

/// A call of `encode` to refuse: the data, how many outputs and their length, the length of the
/// remainder, and the refusal.
type Refused<'a> = (&'a [Vec<u8>], usize, usize, usize, CodeError);

/// Counts, lengths, marks and symbols are refused in that order, before anything is written.
#[test]
fn refusals_come_before_anything_is_written() {
    let mut generator = [7; 3];
    let shape = CyclicWords::new(Field::Gf65536, 2, 2, 1, 1, &mut generator);
    assert_eq!(shape, Err(CodeError::Shape));
    for mut wrong in [vec![0; 2], vec![0; 4]] {
        let short = CyclicWords::new(Field::Gf2048, 3, 2, 1, 1, &mut wrong);
        assert_eq!(short, Err(CodeError::Length));
    }
    assert_eq!(generator, [7; 3]);
    let code = CyclicWords::new(Field::Gf2048, 3, 2, 1, 1, &mut generator).unwrap();
    let data = vec![vec![0x00, 0x01, 0x07, 0xFF]; 3];
    let mut outside = data.clone();
    outside[1][2] = 0x08;
    let mut checks = vec![vec![0xEE; 4]; 3];
    let mut odd = data.clone();
    odd[0].pop();
    let calls: [Refused<'_>; 7] = [
        (&data[..2], 2, 4, 2, CodeError::Count),
        (&data, 3, 4, 2, CodeError::Count),
        (&odd, 2, 4, 2, CodeError::Length),
        (&data, 2, 2, 2, CodeError::Length),
        (&data, 2, 4, 3, CodeError::Length),
        (&outside, 2, 4, 3, CodeError::Length),
        (&outside, 2, 4, 2, CodeError::Symbol),
    ];
    for (given, count, length, remainder, refusal) in calls {
        let mut outs: Vec<&mut [u8]> = checks
            .iter_mut()
            .take(count)
            .map(|shard| &mut shard[..length])
            .collect();
        let answer = code.encode(&views(given), &mut outs, &mut vec![0; remainder]);
        assert_eq!(answer, Err(refusal));
    }
    assert!(checks.iter().flatten().all(|&byte| byte == 0xEE));
    // The check: the count of shards, then their lengths and the room, then the symbols.
    let whole = [data.clone(), vec![vec![0; 4]; 2]].concat();
    let (mut short, mut room) = ([0xEE; 3], [0xEE; 4]);
    assert_eq!(
        code.agrees(&views(&whole[..4]), &mut room),
        Err(CodeError::Count)
    );
    let mut wide = whole.clone();
    wide[1] = outside[1].clone();
    assert_eq!(
        code.agrees(&views(&wide), &mut short),
        Err(CodeError::Length)
    );
    assert_eq!(
        code.agrees(&views(&wide), &mut room),
        Err(CodeError::Symbol)
    );
    assert_eq!((short, room), ([0xEE; 3], [0xEE; 4]));
    // One word: its length and the room, then the marks, then the symbols read.
    let mut word = [1, 2, 3, 0x800, 5];
    let enough = CyclicWords::room(2);
    let calls: [(usize, &[usize], usize, CodeError); 6] = [
        (4, &[], enough, CodeError::Length),
        (5, &[3], enough - 1, CodeError::Length),
        (5, &[3, 3], enough, CodeError::Mark),
        (5, &[3, 1], enough, CodeError::Mark),
        (5, &[5], enough, CodeError::Mark),
        (5, &[2], enough, CodeError::Symbol),
    ];
    for (length, erased, room, refusal) in calls {
        let mut room = vec![0xEEEE; room];
        let answer = code.correct_word(&mut word[..length], erased, &mut room);
        assert_eq!(answer, Err(refusal));
        assert!(room.iter().all(|&element| element == 0xEEEE));
    }
    assert_eq!(word, [1, 2, 3, 0x800, 5]);
}
