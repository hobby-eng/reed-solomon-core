//! Symbols marked unreadable and the plan of changes: gaps at known places cost one spare shard,
//! wrong symbols at unknown places two, and nothing changes before `Code::apply`. The words a
//! card adds and the shards of the byte example were computed apart from this crate, by Lagrange
//! interpolation in Python.

use crate::support::{FIELDS, bytes, data_in, plain, set};
use reed_solomon_core::{ChangeKind, Code, CodeError, Correction, Field, Mark, Plan, Redundancy};

/// The twelve words of a card as numbers of the BIP39 English list, and the four words the code
/// adds at the points 12 to 15 (chronic identify give swift).
const CARD: [u16; 12] = [
    0x371, 0x6CE, 0x220, 0x11D, 0x2DB, 0x73E, 0x12B, 0x7C6, 0x7D5, 0x7AF, 0x6A9, 0x453,
];
const CARD_REPAIR: [u16; 4] = [0x143, 0x384, 0x313, 0x6DE];

fn word(value: u16) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

fn mark(shard: usize, symbol: usize) -> Mark {
    Mark { shard, symbol }
}

/// The shards as `(index, bytes)`, numbered from 0.
fn given(shards: &[Vec<u8>]) -> Vec<(u16, &[u8])> {
    (0..)
        .zip(shards.iter().map(|bytes| bytes.as_slice()))
        .collect()
}

/// A change as a test looks at it: its place, what the symbol held and becomes, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seen {
    shard: usize,
    symbol: usize,
    was: u16,
    becomes: u16,
    kind: ChangeKind,
}

/// Every change of `plan` with its values, or `None` for a plan without a list.
fn seen(plan: &Plan) -> Option<Vec<Seen>> {
    let Plan::Changes(changes) = plan else {
        return None;
    };
    let each = |(at, change): (usize, reed_solomon_core::Change)| {
        let (was, becomes) = changes.values(at, |was, becomes| (was, becomes)).unwrap();
        Seen {
            shard: change.shard,
            symbol: change.symbol,
            was,
            becomes,
            kind: change.kind,
        }
    };
    Some(changes.iter().enumerate().map(each).collect())
}

/// The shards after `apply` makes the changes of `plan` in them.
fn applied(code: &Code, shards: &[Vec<u8>], plan: &Plan) -> Vec<Vec<u8>> {
    assert!(matches!(plan, Plan::Changes(_)), "{plan:?}");
    let Plan::Changes(changes) = plan else {
        return shards.to_vec();
    };
    applied_changes(code, shards, changes)
}

/// The shards after `apply` makes `changes` in them.
fn applied_changes(
    code: &Code,
    shards: &[Vec<u8>],
    changes: &reed_solomon_core::Changes,
) -> Vec<Vec<u8>> {
    let mut out = shards.to_vec();
    let mut buffers: Vec<(u16, &mut [u8])> = (0..)
        .zip(out.iter_mut().map(|bytes| bytes.as_mut_slice()))
        .collect();
    code.apply(&mut buffers, changes).unwrap();
    out
}

#[test]
fn the_words_a_card_adds_are_those_computed_apart() {
    let code = Code::new(Field::Gf2048, 12).unwrap();
    let data: Vec<Vec<u8>> = CARD.iter().map(|&w| word(w)).collect();
    let refs: Vec<&[u8]> = data.iter().map(|bytes| bytes.as_slice()).collect();
    let added = code.encode(&refs, &[12, 13, 14, 15]).unwrap();
    let expected: Vec<Vec<u8>> = CARD_REPAIR.iter().map(|&w| word(w)).collect();
    assert_eq!(plain(added), expected);
}

#[test]
fn a_card_with_two_gaps_and_a_wrong_word_is_put_right() {
    let code = Code::new(Field::Gf2048, 12).unwrap();
    let card: Vec<Vec<u8>> = CARD.iter().chain(&CARD_REPAIR).map(|&w| word(w)).collect();
    // Words 3 and 8 cannot be read; word 11 was copied as another word.
    let mut read = card.clone();
    read[2] = word(0);
    read[7] = word(0);
    read[10] = word(0x7FF);
    let marks = [mark(7, 0), mark(2, 0)];
    let plan = code.plan(&given(&read), &marks).unwrap();
    let expected = [
        Seen {
            shard: 2,
            symbol: 0,
            was: 0,
            becomes: 0x220,
            kind: ChangeKind::Filled,
        },
        Seen {
            shard: 7,
            symbol: 0,
            was: 0,
            becomes: 0x7C6,
            kind: ChangeKind::Filled,
        },
        Seen {
            shard: 10,
            symbol: 0,
            was: 0x7FF,
            becomes: 0x6A9,
            kind: ChangeKind::Replaced,
        },
    ];
    assert_eq!(seen(&plan), Some(expected.to_vec()));
    assert_eq!(applied(&code, &read, &plan), card);
    // One gap more is beyond the four spare words: 2 · 1 + 3 = 5.
    read[0] = word(0);
    let more = [mark(7, 0), mark(2, 0), mark(0, 0)];
    assert_eq!(
        code.plan(&given(&read), &more),
        Ok(Plan::Unresolved { columns: 1 })
    );
}

#[test]
fn a_wrong_word_below_or_between_the_gaps_keeps_its_place_in_the_list() {
    let code = Code::new(Field::Gf2048, 12).unwrap();
    let card: Vec<Vec<u8>> = CARD.iter().chain(&CARD_REPAIR).map(|&w| word(w)).collect();
    // Words 2 and 5 cannot be read; a word before them, then one between them, was copied wrong.
    for wrong in [0, 3] {
        let mut read = card.clone();
        read[1] = word(0);
        read[4] = word(0);
        read[wrong] = word(0x7FF);
        let change = |shard: usize, kind| Seen {
            shard,
            symbol: 0,
            was: match kind {
                ChangeKind::Filled => 0,
                ChangeKind::Replaced => 0x7FF,
            },
            becomes: CARD[shard],
            kind,
        };
        let mut expected = vec![
            change(1, ChangeKind::Filled),
            change(4, ChangeKind::Filled),
            change(wrong, ChangeKind::Replaced),
        ];
        expected.sort_by_key(|change| change.shard);
        let plan = code.plan(&given(&read), &[mark(4, 0), mark(1, 0)]).unwrap();
        assert_eq!(seen(&plan), Some(expected), "wrong word {wrong}");
        assert_eq!(applied(&code, &read, &plan), card);
    }
}

#[test]
fn gaps_inside_long_shards_cost_only_their_symbols() {
    // Seven shards of four bytes with k = 3, the further ones computed apart.
    let set: Vec<Vec<u8>> = [
        [0x61, 0x62, 0x63, 0x64],
        [0x65, 0x66, 0x67, 0x68],
        [0x69, 0x6A, 0x6B, 0x6C],
        [0x6D, 0x6E, 0x6F, 0x60],
        [0x71, 0x72, 0x73, 0x34],
        [0x75, 0x76, 0x77, 0x38],
        [0x79, 0x7A, 0x7B, 0x3C],
    ]
    .iter()
    .map(|shard| shard.to_vec())
    .collect();
    let code = Code::new(Field::Gf256, 3).unwrap();
    let refs: Vec<&[u8]> = set.iter().take(3).map(|bytes| bytes.as_slice()).collect();
    assert_eq!(
        plain(code.encode(&refs, &[0, 1, 2, 3, 4, 5, 6]).unwrap()),
        set
    );
    let mut read = set.clone();
    read[1][2] = 0;
    read[4][2] = 0;
    read[5][2] ^= 0x5A;
    read[6][0] ^= 0x01;
    // Column 2 has two gaps and a wrong symbol, column 0 a wrong symbol: within four spare.
    let plan = code.plan(&given(&read), &[mark(1, 2), mark(4, 2)]).unwrap();
    let expected = [
        Seen {
            shard: 6,
            symbol: 0,
            was: 0x78,
            becomes: 0x79,
            kind: ChangeKind::Replaced,
        },
        Seen {
            shard: 1,
            symbol: 2,
            was: 0x00,
            becomes: 0x67,
            kind: ChangeKind::Filled,
        },
        Seen {
            shard: 4,
            symbol: 2,
            was: 0x00,
            becomes: 0x73,
            kind: ChangeKind::Filled,
        },
        Seen {
            shard: 5,
            symbol: 2,
            was: 0x2D,
            becomes: 0x77,
            kind: ChangeKind::Replaced,
        },
    ];
    assert_eq!(seen(&plan), Some(expected.to_vec()));
    assert_eq!(applied(&code, &read, &plan), set);
    // Leaving the two shards out instead costs every other column the same two spare shards,
    // and the correction alone still puts this right; with a second wrong symbol in column 0
    // only the marks do.
    read[3][0] ^= 0x10;
    let mut without: Vec<(u16, Vec<u8>)> = (0..7)
        .filter(|i| *i != 1 && *i != 4)
        .map(|i| (i, read[usize::from(i)].clone()))
        .collect();
    let mut buffers: Vec<(u16, &mut [u8])> = without
        .iter_mut()
        .map(|(i, s)| (*i, s.as_mut_slice()))
        .collect();
    assert!(matches!(
        code.correct(&mut buffers),
        Ok(reed_solomon_core::Correction::Unresolved { .. })
    ));
    let plan = code.plan(&given(&read), &[mark(1, 2), mark(4, 2)]).unwrap();
    assert_eq!(applied(&code, &read, &plan), set);
}

#[test]
fn a_marked_symbol_that_holds_the_right_value_is_still_listed() {
    let code = Code::new(Field::Gf256, 2).unwrap();
    let set = set(&code, &data_in(Field::Gf256, 2, 3, 7), 4);
    let plan = code.plan(&given(&set), &[mark(3, 1), mark(3, 1)]).unwrap();
    let held = set[3][1].into();
    let expected = Seen {
        shard: 3,
        symbol: 1,
        was: held,
        becomes: held,
        kind: ChangeKind::Filled,
    };
    assert_eq!(seen(&plan), Some(vec![expected]));
    // Without marks the same shards agree; with exactly k shards nothing is checked.
    assert_eq!(code.plan(&given(&set), &[]), Ok(Plan::Agrees));
    assert_eq!(code.plan(&given(&set[..2]), &[]), Ok(Plan::Unchecked));
    assert_eq!(
        code.plan(&given(&set[..2]), &[mark(0, 0), mark(1, 0), mark(0, 2)]),
        Ok(Plan::Unresolved { columns: 2 })
    );
}

/// A generator of small numbers for the random cases.
struct Draw(Vec<u8>, usize);

impl Draw {
    fn below(&mut self, bound: usize) -> usize {
        let pick = usize::from(self.0[self.1 % self.0.len()]) % bound;
        self.1 += 1;
        pick
    }
}

/// `count` different places below `bound`, avoiding `taken`.
fn places(draw: &mut Draw, bound: usize, count: usize, taken: &[usize]) -> Vec<usize> {
    let mut out = Vec::new();
    while out.len() < count {
        let place = draw.below(bound);
        if !out.contains(&place) && !taken.contains(&place) {
            out.push(place);
        }
    }
    out
}

/// Puts a value other than `symbol` in its place: XOR with a nonzero element of the field.
fn spoil(field: Field, shard: &mut [u8], symbol: usize, draw: &mut Draw) {
    let width = field.symbol_bytes();
    let error = loop {
        let value = u16::try_from(draw.below(field.order().min(65_536))).unwrap();
        if value != 0 {
            break value;
        }
    };
    let bytes = error.to_be_bytes();
    for (byte, e) in shard[symbol * width..][..width]
        .iter_mut()
        .zip(&bytes[2 - width..])
    {
        *byte ^= e;
    }
}

#[test]
fn within_the_bound_the_plan_gives_back_the_set_in_every_field() {
    let mut draw = Draw(bytes(41, 4096), 0);
    for field in FIELDS {
        for (k, n) in [(1, 4), (2, 6), (3, 7), (4, 9)] {
            let code = Code::new(field, k).unwrap();
            let width = field.symbol_bytes();
            let symbols = 3;
            let set = set(&code, &data_in(field, k, symbols * width, n as u64), n);
            let spare = n - k;
            for _ in 0..60 {
                let mut read = set.clone();
                let mut marks = Vec::new();
                let mut wrong = Vec::new();
                for symbol in 0..symbols {
                    let gaps = draw.below(spare + 1);
                    let errors = draw.below((spare - gaps) / 2 + 1);
                    let at_gaps = places(&mut draw, n, gaps, &[]);
                    let at_errors = places(&mut draw, n, errors, &at_gaps);
                    for &shard in &at_gaps {
                        marks.push(mark(shard, symbol));
                        // A gap may hold anything of the field, here zero.
                        read[shard][symbol * width..][..width].fill(0);
                    }
                    for &shard in &at_errors {
                        spoil(field, &mut read[shard], symbol, &mut draw);
                        wrong.push((shard, symbol));
                    }
                }
                let plan = code.plan(&given(&read), &marks).unwrap();
                if marks.is_empty() && wrong.is_empty() {
                    assert_eq!(plan, Plan::Agrees);
                    continue;
                }
                let Plan::Changes(changes) = plan else {
                    panic!("{field:?} {plan:?}")
                };
                assert_eq!(
                    applied_changes(&code, &read, &changes),
                    set,
                    "{field:?} k={k} n={n}"
                );
                let mut replaced: Vec<(usize, usize)> = changes
                    .iter()
                    .filter(|c| c.kind == ChangeKind::Replaced)
                    .map(|c| (c.shard, c.symbol))
                    .collect();
                replaced.sort_unstable();
                wrong.sort_unstable();
                assert_eq!(replaced, wrong);
                let filled = changes
                    .iter()
                    .filter(|c| c.kind == ChangeKind::Filled)
                    .count();
                assert_eq!(filled, marks.len());
            }
        }
    }
}

#[test]
fn beyond_the_bound_the_answer_is_unresolved_or_another_set() {
    let mut draw = Draw(bytes(43, 4096), 0);
    for field in FIELDS {
        let (k, n) = (3, 8);
        let code = Code::new(field, k).unwrap();
        let width = field.symbol_bytes();
        let set = set(&code, &data_in(field, k, width, 5), n);
        for _ in 0..200 {
            let mut read = set.clone();
            // 2e + s is one or two beyond the five spare shards.
            let gaps = draw.below(6);
            let errors = (5 + 1 + draw.below(2) - gaps).div_ceil(2);
            let at_gaps = places(&mut draw, n, gaps, &[]);
            let at_errors = places(&mut draw, n, errors.min(n - gaps), &at_gaps);
            let marks: Vec<Mark> = at_gaps.iter().map(|&s| mark(s, 0)).collect();
            for &shard in &at_errors {
                spoil(field, &mut read[shard], 0, &mut draw);
            }
            match code.plan(&given(&read), &marks).unwrap() {
                Plan::Unresolved { columns } => assert_eq!(columns, 1),
                Plan::Changes(changes) => {
                    let out = applied_changes(&code, &read, &changes);
                    let shards: Vec<(u16, &[u8])> = given(&out);
                    assert_eq!(code.redundancy(&shards), Ok(Redundancy::Agrees));
                    // Another set, then, but never farther than the bound reaches: every mark
                    // filled and at most (5 − s) / 2 other symbols replaced.
                    let kind = |kind| changes.iter().filter(|c| c.kind == kind).count();
                    assert_eq!(kind(ChangeKind::Filled), gaps);
                    assert!(2 * kind(ChangeKind::Replaced) <= 5 - gaps, "{changes:?}");
                }
                other => panic!("{field:?} {other:?}"),
            }
        }
    }
}

#[test]
fn shards_of_no_bytes_agree_and_hold_no_symbol_to_mark() {
    let code = Code::new(Field::Gf256, 2).unwrap();
    let empty = vec![Vec::new(); 4];
    assert_eq!(code.plan(&given(&empty), &[]), Ok(Plan::Agrees));
    assert_eq!(
        code.plan(&given(&empty), &[mark(0, 0)]),
        Err(CodeError::Mark)
    );
    let mut out = empty.clone();
    let mut buffers: Vec<(u16, &mut [u8])> = (0..)
        .zip(out.iter_mut().map(|bytes| bytes.as_mut_slice()))
        .collect();
    assert_eq!(code.correct(&mut buffers), Ok(Correction::Agrees));
}

#[test]
fn a_column_with_more_marks_than_spare_shards_is_unresolved() {
    // k = 2 of four shards: two spare. Three marks in column 1, one in column 0.
    let code = Code::new(Field::Gf256, 2).unwrap();
    let set = set(&code, &data_in(Field::Gf256, 2, 3, 11), 4);
    let marks = [mark(0, 1), mark(1, 1), mark(3, 1), mark(2, 0)];
    assert_eq!(
        code.plan(&given(&set), &marks),
        Ok(Plan::Unresolved { columns: 1 })
    );
    // Two marks in column 1 are as many as the spare shards: filled with nothing to confirm.
    let plan = code
        .plan(&given(&set), &[mark(0, 1), mark(3, 1), mark(2, 0)])
        .unwrap();
    let changes = seen(&plan).unwrap();
    assert_eq!(changes.len(), 3);
    assert!(
        changes
            .iter()
            .all(|c| c.kind == ChangeKind::Filled && c.was == c.becomes)
    );
}

#[test]
fn marks_are_checked_after_the_lengths_and_before_the_symbols() {
    let code = Code::new(Field::Gf2048, 2).unwrap();
    let set = set(&code, &data_in(Field::Gf2048, 2, 4, 3), 4);
    assert_eq!(code.plan(&given(&set), &[mark(4, 0)]), Err(CodeError::Mark));
    assert_eq!(code.plan(&given(&set), &[mark(0, 2)]), Err(CodeError::Mark));
    let mut uneven = set.clone();
    uneven[1].push(0);
    uneven[1].push(0);
    assert_eq!(
        code.plan(&given(&uneven), &[mark(9, 0)]),
        Err(CodeError::Length)
    );
    let mut not_a_word = set.clone();
    not_a_word[3][0] = 0xFF;
    assert_eq!(
        code.plan(&given(&not_a_word), &[mark(9, 0)]),
        Err(CodeError::Mark)
    );
    assert_eq!(
        code.plan(&given(&not_a_word), &[mark(3, 0)]),
        Err(CodeError::Symbol)
    );
}

#[test]
fn apply_refuses_shards_other_than_those_the_plan_was_made_for() {
    let code = Code::new(Field::Gf2048, 2).unwrap();
    let set = set(&code, &data_in(Field::Gf2048, 2, 4, 9), 4);
    let mut read = set.clone();
    read[0][1] ^= 1;
    let plan = code.plan(&given(&read), &[]).unwrap();
    let Plan::Changes(changes) = &plan else {
        panic!("{plan:?}")
    };
    // Another symbol changed after the plan, two shards swapped, and another code: nothing is
    // written. The shards as read are put right.
    let mut stale = read.clone();
    stale[2][3] ^= 1;
    let mut swapped = read.clone();
    swapped.swap(1, 2);
    let other = Code::new(Field::Gf2048, 3).unwrap();
    for (code, shards) in [(&code, &stale), (&code, &swapped), (&other, &read)] {
        let mut out = shards.clone();
        let mut buffers: Vec<(u16, &mut [u8])> = (0..)
            .zip(out.iter_mut().map(|bytes| bytes.as_mut_slice()))
            .collect();
        assert_eq!(code.apply(&mut buffers, changes), Err(CodeError::Changes));
        assert_eq!(&out, shards);
    }
    assert_eq!(applied(&code, &read, &plan), set);
}
