//! Words of the cyclic code in every field it takes, any roots, symbols changed or marked and
//! whole shards lost, whatever they hold. Within `2e + s ≤ parity` a plan and a correction give
//! the word back; what they change is a word of the code, a refusal or a word beyond the bound
//! changes nothing, and other bytes in lost shards give the same answers. The code in the caller's
//! memory encodes and checks as the one that allocates, and puts each word right as its plan does.
#![no_main]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a fuzz target answers by stopping the run: its unwraps and assertions are its checks"
)]

use libfuzzer_sys::fuzz_target;
use reed_solomon_core::{
    Change, ChangeKind, Changes, CodeError, Correction, CyclicCode, CyclicWords, Field, Mark, Plan,
};
use reed_solomon_core_fuzz::{damage, symbol, within};

fn views(shards: &[Vec<u8>]) -> Vec<&[u8]> {
    shards.iter().map(Vec::as_slice).collect()
}

/// The word at `column` of `shards`, a symbol of `width` bytes from each.
fn word_at(shards: &[Vec<u8>], column: usize, width: usize) -> Vec<u16> {
    let bytes = |shard: &Vec<u8>| shard[column * width..(column + 1) * width].to_vec();
    let value = |bytes: Vec<u8>| bytes.iter().fold(0, |w, &b| w << 8 | u16::from(b));
    shards.iter().map(|shard| value(bytes(shard))).collect()
}

/// Every word of `shards` put right in place, `erased(column)` its unreadable places: the shards
/// left, the places written, and the words left as they were because no word lies within the
/// bound. The room is wiped after each word.
fn in_place(
    words: &CyclicWords<'_>,
    shards: &[Vec<u8>],
    erased: impl Fn(usize) -> Vec<usize>,
    width: usize,
) -> (Vec<Vec<u8>>, usize, usize) {
    let (mut each, mut written, mut unresolved) = (shards.to_vec(), 0, 0);
    let mut work = vec![0; CyclicWords::room(words.parity())];
    for column in 0..shards[0].len() / width {
        let mut erased = erased(column);
        erased.sort_unstable();
        erased.dedup();
        let mut word = word_at(shards, column, width);
        let before = word.clone();
        match words.correct_word(&mut word, &erased, &mut work).unwrap() {
            Some(count) => written += count,
            None => {
                assert_eq!(word, before);
                unresolved += 1;
            }
        }
        for (shard, value) in each.iter_mut().zip(word) {
            let bytes = value.to_be_bytes();
            shard[column * width..(column + 1) * width].copy_from_slice(&bytes[2 - width..]);
        }
        assert!(work.iter().all(|&element| element == 0));
    }
    (each, written, unresolved)
}

/// The words put right in place are what `plan` makes of `read`: the same shards, as many places
/// written as changes listed, and as many words unresolved.
fn follows(
    code: &CyclicCode,
    plan: &Plan,
    read: &[Vec<u8>],
    (each, written, unresolved): (Vec<Vec<u8>>, usize, usize),
) {
    match plan {
        Plan::Changes(changes) => {
            let mut out = read.to_vec();
            let mut buffers: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
            code.apply(&mut buffers, changes).unwrap();
            assert_eq!((unresolved, written, each), (0, changes.len(), out));
        }
        Plan::Agrees => assert_eq!((unresolved, written, each.as_slice()), (0, 0, read)),
        Plan::Unresolved { columns } => assert_eq!(unresolved, *columns),
        Plan::Unchecked => panic!("a cyclic word always has parity"),
    }
}

fuzz_target!(|bytes: &[u8]| {
    let mut input = bytes.iter().copied();
    let mut next = || usize::from(input.next().unwrap_or(0));
    let field = match next() % 4 {
        0 => Field::Gf256,
        1 => Field::Gf2048,
        _ => Field::binary(2 + (next() % 15) as u8, None).unwrap(),
    };
    let (order, width) = (field.order(), field.symbol_bytes());
    let (data, parity) = (1 + next() % 8, 1 + next() % 8);
    let first = (next() << 8 | next()) as u32;
    let step = (next() << 8 | next()) as u32;
    // Lengths and roots that make no code are refused, before any shard is looked at.
    let Ok(code) = CyclicCode::new(field, data, parity, first, step) else {
        return;
    };
    let symbols = 1 + next() % 4;
    let given: Vec<Vec<u8>> = (0..data)
        .map(|_| {
            (0..symbols)
                .flat_map(|_| symbol(&mut next, order, width))
                .collect()
        })
        .collect();
    let made = code.encode(&views(&given)).unwrap();
    let word: Vec<Vec<u8>> = given
        .iter()
        .cloned()
        .chain(made.iter().map(|shard| shard.to_vec()))
        .collect();
    assert!(code.agrees(&views(&word)).unwrap());
    let mut generator = vec![0; parity + 1];
    let words = CyclicWords::new(field, data, parity, first, step, &mut generator).unwrap();
    assert_eq!(words, code.words());
    let mut checks = vec![vec![0; symbols * width]; parity];
    let mut outs: Vec<&mut [u8]> = checks.iter_mut().map(Vec::as_mut_slice).collect();
    words
        .encode(&views(&given), &mut outs, &mut vec![0; parity])
        .unwrap();
    assert_eq!(checks, word[data..]);

    let mut read = word.clone();
    let marks = damage(&mut next, &mut read, symbols, order, width);
    let is_marked = |shard: usize, at: usize| marks.contains(&Mark { shard, symbol: at });
    let within_marks = |with_marks: bool| within(&read, &word, &marks, width, parity, with_marks);
    let mut room = vec![0; (1 + next() % symbols) * parity * width];
    let agrees = code.agrees(&views(&read));
    assert_eq!(words.agrees(&views(&read), &mut room), agrees);
    let marked_at = |column: usize| -> Vec<usize> {
        let marked = marks.iter().filter(|mark| mark.symbol == column);
        marked.map(|mark| mark.shard).collect()
    };

    let plan = code.plan(&views(&read), &[], &marks).unwrap();
    follows(
        &code,
        &plan,
        &read,
        in_place(&words, &read, marked_at, width),
    );
    match plan {
        Plan::Changes(changes) => {
            for change in &changes {
                let marked = is_marked(change.shard, change.symbol);
                assert_eq!(matches!(change.kind, ChangeKind::Filled), marked);
            }
            let listed = |mark: &Mark| {
                changes
                    .iter()
                    .any(|c| c.shard == mark.shard && c.symbol == mark.symbol)
            };
            assert!(marks.iter().all(listed));
            let mut out = read.clone();
            let mut buffers: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
            code.apply(&mut buffers, &changes).unwrap();
            assert!(code.agrees(&views(&out)).unwrap());
            if within_marks(true) {
                assert_eq!(out, word);
            }
        }
        Plan::Agrees => {
            assert!(marks.is_empty() && agrees.unwrap());
            if within_marks(true) {
                assert_eq!(read, word);
            }
        }
        Plan::Unresolved { .. } => assert!(!within_marks(true)),
        Plan::Unchecked => panic!("a cyclic word always has parity"),
    }

    let mut out = read.clone();
    let mut buffers: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
    match code.correct(&mut buffers, &[]).unwrap() {
        Correction::Corrected(_) => {
            assert!(code.agrees(&views(&out)).unwrap());
            if within_marks(false) {
                assert_eq!(out, word);
            }
        }
        Correction::Agrees => {
            assert!(out == read && agrees.unwrap());
        }
        Correction::Unresolved { .. } => {
            assert_eq!(out, read);
            assert!(!within_marks(false));
        }
        Correction::Unchecked => panic!("a cyclic word always has parity"),
    }

    // Whole shards lost, in any order and more than once, holding any bytes: their symbols count
    // as marked in every word, and a lost place outside the word is refused with nothing changed.
    let length = data + parity;
    let mut lost: Vec<usize> = (0..next() % (parity + 2))
        .map(|_| next() % length)
        .collect();
    let mut gone = read.clone();
    for &place in &lost {
        gone[place].iter_mut().for_each(|byte| *byte = next() as u8);
    }
    if next() % 16 == 0 {
        lost.push(length + next() % 3);
        let before = gone.clone();
        let refused = code.plan(&views(&gone), &lost, &marks);
        assert_eq!(refused, Err(CodeError::Mark));
        let mut buffers: Vec<&mut [u8]> = gone.iter_mut().map(Vec::as_mut_slice).collect();
        assert_eq!(code.correct(&mut buffers, &lost), Err(CodeError::Mark));
        assert_eq!(gone, before);
        return;
    }
    // The same shards with every byte of the lost ones turned over.
    let mut other = gone.clone();
    for &place in &lost {
        other[place].iter_mut().for_each(|byte| *byte = !*byte);
    }
    let every = |shard: usize| (0..symbols).map(move |symbol| Mark { shard, symbol });
    let lost_marks: Vec<Mark> = lost.iter().flat_map(|&shard| every(shard)).collect();
    let all_marks: Vec<Mark> = lost_marks.iter().chain(&marks).copied().collect();
    let within_lost = |marks: &[Mark]| within(&gone, &word, marks, width, parity, true);
    let erased_at = |column: usize| [lost.clone(), marked_at(column)].concat();

    let plan = code.plan(&views(&gone), &lost, &marks).unwrap();
    follows(
        &code,
        &plan,
        &gone,
        in_place(&words, &gone, erased_at, width),
    );
    match (&plan, &code.plan(&views(&other), &lost, &marks).unwrap()) {
        (Plan::Changes(one), Plan::Changes(two)) => {
            let made = |list: &Changes| {
                let each = |(at, c): (usize, Change)| {
                    let becomes = list.values(at, |_, becomes| becomes);
                    (c.shard, c.symbol, becomes, c.kind)
                };
                list.iter().enumerate().map(each).collect::<Vec<_>>()
            };
            assert_eq!(made(one), made(two));
        }
        (one, two) => assert_eq!(one, two),
    }
    match plan {
        Plan::Changes(changes) => {
            for change in changes.iter().filter(|c| lost.contains(&c.shard)) {
                assert_eq!(change.kind, ChangeKind::Filled);
            }
            let listed = |mark: &Mark| {
                changes
                    .iter()
                    .any(|c| c.shard == mark.shard && c.symbol == mark.symbol)
            };
            assert!(all_marks.iter().all(listed));
            let mut out = gone.clone();
            let mut buffers: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
            code.apply(&mut buffers, &changes).unwrap();
            assert!(code.agrees(&views(&out)).unwrap());
            if within_lost(&all_marks) {
                assert_eq!(out, word);
            }
        }
        Plan::Agrees => assert!(lost.is_empty() && marks.is_empty()),
        Plan::Unresolved { .. } => assert!(!within_lost(&all_marks)),
        Plan::Unchecked => panic!("a cyclic word always has parity"),
    }

    let (mut out, mut turned) = (gone.clone(), other.clone());
    let mut buffers: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
    let answer = code.correct(&mut buffers, &lost).unwrap();
    let mut buffers: Vec<&mut [u8]> = turned.iter_mut().map(Vec::as_mut_slice).collect();
    assert_eq!(code.correct(&mut buffers, &lost).unwrap(), answer);
    match answer {
        Correction::Corrected(_) => {
            assert_eq!(out, turned);
            assert!(code.agrees(&views(&out)).unwrap());
            if within_lost(&lost_marks) {
                assert_eq!(out, word);
            }
        }
        Correction::Agrees => assert!(lost.is_empty() && out == gone),
        Correction::Unresolved { .. } => {
            assert_eq!((&out, &turned), (&gone, &other));
            assert!(!within_lost(&lost_marks));
        }
        Correction::Unchecked => panic!("a cyclic word always has parity"),
    }
});
