//! The cyclic code: the repair words of MHFE-REPAIR-1 as published, the generators the MHFE
//! specification prints, the QR example of ISO/IEC 18004 (version 1-M, annex I), two sets of other
//! parameters and the refusals, all computed apart from this crate by a reference in Python that
//! follows the text of MHFE-REPAIR-1 and textbook definitions; then words put right within the
//! bound and given up beyond it, and long shards as words side by side.

use crate::support::bytes;
use reed_solomon_core::{ChangeKind, CodeError, Correction, CyclicCode, Field, Mark, Plan};

/// The containers of MHFE-REPAIR-1, as numbers of the BIP39 English list, and their repair
/// words for k = 2, 4, 6 and 8.
pub(crate) const MHFE: [(&[u16], [&[u16]; 4]); 4] = [
    // zero-12
    (
        &[
            0x208, 0x6B5, 0x731, 0x521, 0x3B2, 0x5B9, 0x743, 0x639, 0x5DD, 0x5C7, 0x368, 0x147,
            0x08C, 0x534, 0x03A, 0x4A3, 0x1B6, 0x295, 0x567, 0x352, 0x029, 0x145, 0x125, 0x50E,
        ],
        [
            &[0x3E2, 0x288],
            &[0x627, 0x56F, 0x508, 0x3BF],
            &[0x198, 0x0FC, 0x4DF, 0x715, 0x5F1, 0x169],
            &[0x054, 0x393, 0x79C, 0x486, 0x776, 0x70F, 0x679, 0x5FB],
        ],
    ),
    // same-length-nonzero-12
    (
        &[
            0x371, 0x6CE, 0x220, 0x11D, 0x2DB, 0x73E, 0x12B, 0x7C6, 0x7D5, 0x7AF, 0x6A9, 0x453,
        ],
        [
            &[0x483, 0x5B1],
            &[0x52C, 0x41D, 0x4D6, 0x264],
            &[0x71C, 0x592, 0x5C8, 0x5EA, 0x251, 0x4B7],
            &[0x710, 0x4C0, 0x279, 0x131, 0x1AB, 0x207, 0x78F, 0x372],
        ],
    ),
    // same-length-nonzero-21
    (
        &[
            0x612, 0x328, 0x5EA, 0x667, 0x2C1, 0x2E3, 0x36E, 0x4AC, 0x648, 0x426, 0x784, 0x7B8,
            0x716, 0x519, 0x340, 0x779, 0x07E, 0x0B8, 0x513, 0x0C2, 0x79F,
        ],
        [
            &[0x31D, 0x0C0],
            &[0x629, 0x443, 0x50C, 0x1AA],
            &[0x661, 0x3B5, 0x293, 0x12A, 0x0FD, 0x14F],
            &[0x546, 0x24F, 0x647, 0x492, 0x138, 0x30B, 0x2DC, 0x631],
        ],
    ),
    // abandon-23-art
    (
        &[
            0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000,
            0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x000, 0x066,
        ],
        [
            &[0x154, 0x330],
            &[0x404, 0x7C1, 0x0D0, 0x0FF],
            &[0x14E, 0x74F, 0x40B, 0x624, 0x3A4, 0x303],
            &[0x46C, 0x169, 0x12A, 0x4B5, 0x562, 0x7FC, 0x0A2, 0x096],
        ],
    ),
];
/// The case, k, the damaged word, its unreadable, replaced and filled places, from 0.
pub(crate) type Repair = (
    usize,
    usize,
    &'static [u16],
    &'static [usize],
    &'static [usize],
    &'static [usize],
);
/// The repairs of MHFE-REPAIR-1.
pub(crate) const MHFE_REPAIRS: [Repair; 3] = [
    // two-erasures
    (
        0,
        4,
        &[
            0x208, 0x6B5, 0x7FF, 0x521, 0x3B2, 0x5B9, 0x743, 0x639, 0x5DD, 0x5C7, 0x368, 0x147,
            0x08C, 0x534, 0x03A, 0x4A3, 0x7FF, 0x295, 0x567, 0x352, 0x029, 0x145, 0x125, 0x50E,
            0x627, 0x56F, 0x508, 0x3BF,
        ],
        &[2, 16],
        &[],
        &[2, 16],
    ),
    // one-error-one-erasure
    (
        0,
        4,
        &[
            0x208, 0x6B5, 0x731, 0x521, 0x7FF, 0x5B9, 0x743, 0x639, 0x5DD, 0x7FF, 0x368, 0x147,
            0x08C, 0x534, 0x03A, 0x4A3, 0x1B6, 0x295, 0x567, 0x352, 0x029, 0x145, 0x125, 0x50E,
            0x627, 0x56F, 0x508, 0x3BF,
        ],
        &[4],
        &[9],
        &[4],
    ),
    // two-errors-at-the-bound
    (
        0,
        4,
        &[
            0x208, 0x6B5, 0x7FF, 0x521, 0x3B2, 0x5B9, 0x743, 0x639, 0x5DD, 0x5C7, 0x368, 0x147,
            0x08C, 0x534, 0x03A, 0x4A3, 0x000, 0x295, 0x567, 0x352, 0x029, 0x145, 0x125, 0x50E,
            0x627, 0x56F, 0x508, 0x3BF,
        ],
        &[],
        &[2, 16],
        &[],
    ),
];

/// Each word as a shard of one symbol, high byte first in a field of two bytes.
pub(crate) fn shards(field: Field, words: &[u16]) -> Vec<Vec<u8>> {
    let width = field.symbol_bytes();
    words
        .iter()
        .map(|word| word.to_be_bytes()[2 - width..].to_vec())
        .collect()
}

pub(crate) fn views(shards: &[Vec<u8>]) -> Vec<&[u8]> {
    shards.iter().map(Vec::as_slice).collect()
}

/// The words of `code` for these data words: the data, then its parity.
pub(crate) fn word(code: &CyclicCode, data: &[u16]) -> Vec<u16> {
    let given = shards(code.field(), data);
    let parity = code.encode(&views(&given)).unwrap();
    let width = code.field().symbol_bytes();
    let words = parity
        .iter()
        .map(|shard| shard.iter().fold(0, |w, &b| w << 8 | u16::from(b)));
    data.iter()
        .copied()
        .chain(words)
        .inspect(|_| assert!(width > 0))
        .collect()
}

/// The places a plan changes, each with how.
type Kinds = Vec<(usize, ChangeKind)>;

/// Applies the plan `code` makes for `read` with `marks`, and returns the words it leaves.
fn planned(code: &CyclicCode, read: &[u16], marks: &[usize]) -> Option<(Vec<u16>, Kinds)> {
    let mut held = shards(code.field(), read);
    let marks: Vec<Mark> = marks
        .iter()
        .map(|&shard| Mark { shard, symbol: 0 })
        .collect();
    let Plan::Changes(changes) = code.plan(&views(&held), &[], &marks).unwrap() else {
        return None;
    };
    let kinds = changes
        .iter()
        .map(|change| (change.shard, change.kind))
        .collect();
    let mut buffers: Vec<&mut [u8]> = held.iter_mut().map(Vec::as_mut_slice).collect();
    code.apply(&mut buffers, &changes).unwrap();
    let width = code.field().symbol_bytes();
    let words = held
        .iter()
        .map(|shard| shard.iter().fold(0, |w, &b| w << 8 | u16::from(b)));
    Some((words.inspect(|_| assert!(width > 0)).collect(), kinds))
}

#[test]
fn the_repair_words_of_mhfe_are_the_published_ones() {
    for (data, parities) in MHFE {
        for parity in parities {
            let code = CyclicCode::new(Field::Gf2048, data.len(), parity.len(), 1, 1).unwrap();
            assert_eq!(
                &word(&code, data)[data.len()..],
                parity,
                "k = {}",
                parity.len()
            );
        }
    }
}

#[test]
fn the_generators_are_those_the_mhfe_specification_prints() {
    let printed: [&[u16]; 4] = [
        &[1, 6, 8],
        &[1, 30, 216, 960, 1024],
        &[1, 126, 1181, 1719, 2029, 1077, 1034],
        &[1, 510, 1509, 1770, 1837, 850, 1339, 600, 680],
    ];
    for generator in printed {
        let code = CyclicCode::new(Field::Gf2048, 24, generator.len() - 1, 1, 1).unwrap();
        assert_eq!(code.generator(), generator);
    }
}

#[test]
fn the_repairs_of_mhfe_put_the_words_right_as_published() {
    for (case, parity, damaged, unreadable, replaced, filled) in MHFE_REPAIRS {
        let data = MHFE[case].0;
        let code = CyclicCode::new(Field::Gf2048, data.len(), parity, 1, 1).unwrap();
        let (words, kinds) = planned(&code, damaged, unreadable).unwrap();
        assert_eq!(words, word(&code, data));
        let mut expected: Vec<(usize, ChangeKind)> = replaced
            .iter()
            .map(|&place| (place, ChangeKind::Replaced))
            .chain(filled.iter().map(|&place| (place, ChangeKind::Filled)))
            .collect();
        expected.sort_unstable_by_key(|&(place, _)| place);
        assert_eq!(kinds, expected);
    }
}

#[test]
fn the_qr_example_of_iso_18004_gets_its_published_parity() {
    let code = CyclicCode::new(Field::Gf256, 16, 10, 0, 1).unwrap();
    let data = [
        0x10, 0x20, 0x0C, 0x56, 0x61, 0x80, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC,
        0x11,
    ];
    let parity = [0xA5, 0x24, 0xD4, 0xC1, 0xED, 0x36, 0xC7, 0x87, 0x2C, 0x55];
    assert_eq!(&word(&code, &data)[16..], parity);
}

/// GF(2^11), first root 5, step 3: generator, parity, a word with one wrong and two marked symbols,
/// and three wrong symbols given up.
#[test]
fn other_roots_in_the_field_of_words_put_a_word_right_and_give_up_beyond() {
    let code = CyclicCode::new(Field::Gf2048, 5, 4, 5, 3).unwrap();
    assert_eq!(code.generator(), [0x001, 0x69E, 0x2BE, 0x55A, 0x2EB]);
    let whole = [
        0x001, 0x7FF, 0x400, 0x123, 0x000, 0x410, 0x680, 0x321, 0x134,
    ];
    assert_eq!(word(&code, &whole[..5]), whole);
    let damaged = [
        0x001, 0x55A, 0x400, 0x7FF, 0x000, 0x410, 0x000, 0x321, 0x134,
    ];
    let (words, kinds) = planned(&code, &damaged, &[3, 6]).unwrap();
    assert_eq!(words, whole);
    let expected = [
        (1, ChangeKind::Replaced),
        (3, ChangeKind::Filled),
        (6, ChangeKind::Filled),
    ];
    assert_eq!(kinds, expected);
    let mut three = whole;
    for place in [0, 2, 4] {
        three[place] ^= 1;
    }
    let held = shards(Field::Gf2048, &three);
    assert_eq!(
        code.plan(&views(&held), &[], &[]),
        Ok(Plan::Unresolved { columns: 1 })
    );
}

/// GF(2^8), first root 1: the parity of "shards", and two wrong bytes corrected in place.
#[test]
fn a_byte_word_with_two_wrong_bytes_is_corrected_in_place() {
    let code = CyclicCode::new(Field::Gf256, 6, 4, 1, 1).unwrap();
    let data: Vec<u16> = b"shards".iter().map(|&b| u16::from(b)).collect();
    let whole = word(&code, &data);
    assert_eq!(&whole[6..], [0x7D, 0x8E, 0x96, 0x18]);
    let mut read = shards(Field::Gf256, &whole);
    read[0][0] ^= 0x40;
    read[9][0] ^= 0x01;
    let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(Vec::as_mut_slice).collect();
    let mut counts = vec![0; 10];
    (counts[0], counts[9]) = (1, 1);
    assert_eq!(
        code.correct(&mut buffers, &[]),
        Ok(Correction::Corrected(counts))
    );
    assert_eq!(read, shards(Field::Gf256, &whole));
    assert!(code.agrees(&views(&read)).unwrap());
}

#[test]
fn shapes_that_make_no_code_are_refused() {
    let shapes = [
        (Field::Gf2048, 2047, 1, 1, 1),
        (Field::Gf2048, 23, 1, 1, 89),
        (Field::Gf256, 250, 6, 1, 1),
        (Field::Gf256, 5, 0, 1, 1),
        (Field::Gf256, 0, 5, 1, 1),
        (Field::Gf256, 2, 2, 1, 255),
        (Field::Gf65536, 2, 2, 1, 1),
    ];
    for (field, data, parity, first, step) in shapes {
        let refused = CyclicCode::new(field, data, parity, first, step);
        assert_eq!(
            refused,
            Err(CodeError::Shape),
            "{field:?} {data} {parity} {step}"
        );
    }
    // At the bound itself: 2046 + 1 words in GF(2^11) with step 1, 22 + 1 with step 89.
    assert!(CyclicCode::new(Field::Gf2048, 2046, 1, 1, 1).is_ok());
    assert!(CyclicCode::new(Field::Gf2048, 22, 1, 1, 89).is_ok());
}

/// Shards of many symbols are words side by side: each is put right on its own, and the work
/// passes stripes of words as long shards need.
#[test]
fn long_shards_are_words_side_by_side() {
    for field in [
        Field::Gf256,
        Field::Gf2048,
        Field::binary(4, None).unwrap(),
        Field::binary(16, None).unwrap(),
    ] {
        let (data, parity) = (5, 4);
        let code = CyclicCode::new(field, data, parity, 0, 1).unwrap();
        let width = field.symbol_bytes();
        let symbols = 300;
        let given: Vec<Vec<u8>> = (0..data)
            .map(|i| crate::support::symbols(field, bytes(i as u64 + 7, symbols * width)))
            .collect();
        let made = code.encode(&views(&given)).unwrap();
        let whole: Vec<Vec<u8>> = given
            .iter()
            .cloned()
            .chain(made.iter().map(|shard| shard.to_vec()))
            .collect();
        assert!(code.agrees(&views(&whole)).unwrap());
        // In word j, two wrong symbols at places j and j + 3 of the nine, both within the bound.
        let mut read = whole.clone();
        for column in 0..symbols {
            for place in [column % 9, (column + 3) % 9] {
                read[place][column * width + width - 1] ^= 1;
            }
        }
        assert!(!code.agrees(&views(&read)).unwrap());
        let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(Vec::as_mut_slice).collect();
        assert!(matches!(
            code.correct(&mut buffers, &[]),
            Ok(Correction::Corrected(_))
        ));
        assert_eq!(read, whole, "{field:?}");
    }
}

/// Random words and errata in several fields and roots: within 2e + s ≤ parity a plan gives the
/// word back; beyond it the answer is unresolved or another word of the code, and nothing breaks.
#[test]
fn within_the_bound_a_plan_gives_the_word_back() {
    let fields = [
        Field::Gf256,
        Field::Gf2048,
        Field::binary(5, None).unwrap(),
        Field::binary(13, None).unwrap(),
    ];
    let mut draw = bytes(99, 1 << 16).into_iter().cycle();
    let mut next = move |bound: usize| usize::from(draw.next().unwrap()) % bound.max(1);
    for (round, field) in fields.into_iter().cycle().take(400).enumerate() {
        let order = field.order();
        let (data, parity) = (1 + next(10), 1 + next(8));
        let (first, step) = (next(5) as u32, [1, 1, 2, 3][round % 4]);
        let Ok(code) = CyclicCode::new(field, data, parity, first, step) else {
            continue;
        };
        let words: Vec<u16> = (0..data)
            .map(|_| (next(256) * 257 % order) as u16)
            .collect();
        let whole = word(&code, &words);
        let length = data + parity;
        let marks: Vec<usize> = (0..next(parity + 2)).map(|_| next(length)).collect();
        let mut unique = marks.clone();
        unique.sort_unstable();
        unique.dedup();
        let wrong = next(parity / 2 + 2);
        let mut read = whole.clone();
        for &place in &unique {
            read[place] = (next(256) % order) as u16;
        }
        for _ in 0..wrong {
            let place = next(length);
            if unique.contains(&place) {
                continue;
            }
            read[place] = ((usize::from(read[place]) + 1 + next(order - 1)) % order) as u16;
        }
        let errors = (0..length)
            .filter(|&p| !unique.contains(&p) && read[p] != whole[p])
            .count();
        let within = 2 * errors + unique.len() <= parity;
        match planned(&code, &read, &marks) {
            Some((back, _)) if within => assert_eq!(back, whole, "{field:?} round {round}"),
            Some((back, _)) => assert!(code.agrees(&views(&shards(field, &back))).unwrap()),
            None => assert!(
                !within || errors + unique.len() == 0,
                "{field:?} round {round}"
            ),
        }
    }
}

#[test]
fn refusals_come_in_their_order_and_change_nothing() {
    let code = CyclicCode::new(Field::Gf2048, 3, 2, 1, 1).unwrap();
    let whole = shards(Field::Gf2048, &word(&code, &[1, 2, 3]));
    assert_eq!(code.encode(&views(&whole[..2])), Err(CodeError::Count));
    assert_eq!(
        code.plan(&views(&whole[..4]), &[], &[]),
        Err(CodeError::Count)
    );
    let mut short = whole.clone();
    short[2].pop();
    assert_eq!(code.agrees(&views(&short)), Err(CodeError::Length));
    // A mark outside the words is refused before a symbol outside the field.
    let mut wide = whole.clone();
    wide[1] = vec![0x08, 0x00];
    assert_eq!(
        code.plan(
            &views(&wide),
            &[],
            &[Mark {
                shard: 5,
                symbol: 0
            }]
        ),
        Err(CodeError::Mark)
    );
    assert_eq!(
        code.plan(
            &views(&wide),
            &[],
            &[Mark {
                shard: 0,
                symbol: 1
            }]
        ),
        Err(CodeError::Mark)
    );
    assert_eq!(code.plan(&views(&wide), &[], &[]), Err(CodeError::Symbol));
    assert_eq!(code.encode(&views(&wide[..3])), Err(CodeError::Symbol));
    // A change that no longer fits leaves every shard as it was.
    let mut read = whole.clone();
    read[0][1] ^= 1;
    let Plan::Changes(changes) = code.plan(&views(&read), &[], &[]).unwrap() else {
        panic!("one wrong word is within the bound");
    };
    read[0][1] ^= 2;
    let before = read.clone();
    let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(Vec::as_mut_slice).collect();
    assert_eq!(code.apply(&mut buffers, &changes), Err(CodeError::Changes));
    assert_eq!(read, before);
}

/// A word whose errata, as found, would leave a syndrome is not put right. The error x + α² on the
/// two parity places of a code of parity 2 with first root 1 has the syndromes (6, 0); the
/// locator Berlekamp–Massey makes cancels back to 1, and only the check of what is left refuses.
#[test]
fn a_word_left_with_a_syndrome_is_not_put_right() {
    let code = CyclicCode::new(Field::Gf256, 6, 2, 1, 1).unwrap();
    let whole = shards(Field::Gf256, &word(&code, &[1, 2, 3, 4, 5, 6]));
    let mut read = whole.clone();
    read[6][0] ^= 0x01;
    read[7][0] ^= 0x04;
    assert!(!code.agrees(&views(&read)).unwrap());
    assert!(matches!(
        code.plan(&views(&read), &[], &[]),
        Ok(Plan::Unresolved { columns: 1 })
    ));
    let before = read.clone();
    let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(Vec::as_mut_slice).collect();
    assert_eq!(
        code.correct(&mut buffers, &[]),
        Ok(Correction::Unresolved { columns: 1 })
    );
    assert_eq!(read, before);
    assert!(code.agrees(&views(&whole)).unwrap());
}

/// Words in three stripes: with 2 parity shards of bytes a stripe takes 2^21 words. Lost symbols,
/// given as zeros and marked, lie on both sides of each border and at both ends; wrong ones lie
/// one word further in.
#[test]
fn words_in_several_stripes_are_put_right_across_their_borders() {
    let code = CyclicCode::new(Field::Gf256, 2, 2, 0, 1).unwrap();
    let stripe = 1 << 21;
    let columns = 2 * stripe + 100;
    let given: Vec<Vec<u8>> = (0..2).map(|i| bytes(i + 40, columns)).collect();
    let made = code.encode(&views(&given)).unwrap();
    let whole: Vec<Vec<u8>> = given
        .iter()
        .cloned()
        .chain(made.iter().map(|shard| shard.to_vec()))
        .collect();
    let lost = [
        0,
        stripe - 1,
        stripe,
        2 * stripe - 1,
        2 * stripe,
        columns - 1,
    ];
    let wrong = [1, stripe + 1, 2 * stripe + 1, columns - 2];
    let mut read = whole.clone();
    let mut marks = Vec::new();
    for (at, &column) in lost.iter().enumerate() {
        read[at % 4][column] = 0;
        marks.push(Mark {
            shard: at % 4,
            symbol: column,
        });
    }
    for (at, &column) in wrong.iter().enumerate() {
        read[(at + 1) % 4][column] ^= 0x5A;
    }
    // Only the last stripe holds a wrong word: the check reads up to it.
    let mut late = whole.clone();
    late[3][columns - 1] ^= 1;
    assert!(code.agrees(&views(&whole)).unwrap());
    assert!(!code.agrees(&views(&late)).unwrap());
    let Plan::Changes(changes) = code.plan(&views(&read), &[], &marks).unwrap() else {
        panic!("every word is within the bound");
    };
    let mut found: Vec<(usize, usize, ChangeKind)> = changes
        .iter()
        .map(|change| (change.symbol, change.shard, change.kind))
        .collect();
    let mut expected: Vec<(usize, usize, ChangeKind)> = lost
        .iter()
        .enumerate()
        .map(|(at, &column)| (column, at % 4, ChangeKind::Filled))
        .chain(
            wrong
                .iter()
                .enumerate()
                .map(|(at, &column)| (column, (at + 1) % 4, ChangeKind::Replaced)),
        )
        .collect();
    found.sort_unstable_by_key(|&(column, shard, _)| (column, shard));
    expected.sort_unstable_by_key(|&(column, shard, _)| (column, shard));
    assert_eq!(found, expected);
    let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(Vec::as_mut_slice).collect();
    code.apply(&mut buffers, &changes).unwrap();
    assert_eq!(read, whole);
    // Without marks, the wrong words alone are corrected in place.
    let mut damaged = whole.clone();
    for (at, &column) in wrong.iter().enumerate() {
        damaged[(at + 1) % 4][column] ^= 0x5A;
    }
    let mut buffers: Vec<&mut [u8]> = damaged.iter_mut().map(Vec::as_mut_slice).collect();
    assert_eq!(
        code.correct(&mut buffers, &[]),
        Ok(Correction::Corrected(vec![1, 1, 1, 1]))
    );
    assert_eq!(damaged, whole);
}

/// Whole shards named lost are written anew whatever they hold, symbols outside the field too,
/// with one wrong symbol in some words: 2e + s ≤ parity. More lost shards than parity change
/// nothing.
#[test]
fn shards_named_lost_are_written_anew_whatever_they_hold() {
    let field = Field::Gf2048;
    let code = CyclicCode::new(field, 6, 4, 1, 1).unwrap();
    let columns = 50;
    let given: Vec<Vec<u8>> = (0..6)
        .map(|i| crate::support::symbols(field, bytes(i + 60, 2 * columns)))
        .collect();
    let made = code.encode(&views(&given)).unwrap();
    let whole: Vec<Vec<u8>> = given
        .iter()
        .cloned()
        .chain(made.iter().map(|shard| shard.to_vec()))
        .collect();
    let mut read = whole.clone();
    read[2].fill(0xFF);
    read[7].fill(0xFF);
    // One wrong symbol in every third word, in shards 0, 4 and 9 by turns: 2 · 1 + 2 ≤ 4.
    let mut counts = vec![0; 10];
    (counts[2], counts[7]) = (columns, columns);
    for column in (0..columns).step_by(3) {
        let place = [0, 4, 9][column / 3 % 3];
        read[place][2 * column + 1] ^= 1;
        counts[place] += 1;
    }
    let before = read.clone();
    // Each place counts once, in any order, and a mark on a lost shard adds nothing.
    let plan = code.plan(&views(&read), &[2, 7], &[]).unwrap();
    let mark = Mark {
        shard: 2,
        symbol: 0,
    };
    let again = code.plan(&views(&read), &[7, 2, 7], &[mark]);
    assert_eq!(again.as_ref(), Ok(&plan));
    let Plan::Changes(changes) = plan else {
        panic!("every word is within the bound");
    };
    assert_eq!(changes.len(), counts.iter().sum::<usize>());
    for change in &changes {
        let lost = [2, 7].contains(&change.shard);
        assert_eq!(change.kind == ChangeKind::Filled, lost);
    }
    let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(Vec::as_mut_slice).collect();
    code.apply(&mut buffers, &changes).unwrap();
    assert_eq!(read, whole);
    let mut read = before.clone();
    let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(Vec::as_mut_slice).collect();
    assert_eq!(
        code.correct(&mut buffers, &[7, 2]),
        Ok(Correction::Corrected(counts))
    );
    assert_eq!(read, whole);
    // A place outside the word, and five lost shards for four check shards.
    let mut read = before.clone();
    let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(Vec::as_mut_slice).collect();
    assert_eq!(code.correct(&mut buffers, &[2, 10]), Err(CodeError::Mark));
    let five = [0, 1, 2, 3, 7];
    assert_eq!(
        code.correct(&mut buffers, &five),
        Ok(Correction::Unresolved { columns })
    );
    assert_eq!(read, before);
    assert_eq!(
        code.plan(&views(&read), &five, &[]),
        Ok(Plan::Unresolved { columns })
    );
    // A lost shard still has the length of the others: it is written in place.
    read[2].truncate(2 * columns - 2);
    assert_eq!(code.plan(&views(&read), &[2], &[]), Err(CodeError::Length));
}
