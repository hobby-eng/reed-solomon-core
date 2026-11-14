//! Sets whose files read without a fault and are not one set: the files that differ from what
//! the rest shares are named, block by block, and the set is taken without them.

use crate::sets::{Written, damage, forged, one_set, shard_of, slices, written};
use crate::support::{RawHeader, bytes, hex, plain};
use reed_solomon_core::{
    Code, Coverage, Field, FileSet, Header, Outvoted, Pile, SetError, SetFacts, Stretch, Survey,
    Vote,
};

/// The step rule of the vote, written out apart from the crate: the syndromes of a block of `s`
/// symbols with `m` holders of which `r` are spare.
fn syndromes(m: u128, r: u128, s: u128) -> u128 {
    r * m * (s + 64)
}

/// A column whose holders disagree, held against the holders found so far.
fn column(r: u128) -> u128 {
    r * (r / 2 + 1)
}

/// A column searched for its wrong symbols.
fn search(m: u128, r: u128) -> u128 {
    (4 * m + 64) * (r + 1)
}

/// `d` shards in dispute compared with what `k` holders make of a block of `s` symbols.
fn judging(k: u128, d: u128, s: u128) -> u128 {
    k * k + d * k * (s + 64)
}

/// The file of shard `index` of another set with the same facts: other data.
fn foreign(set: &Written, index: u16, seed: u64) -> Vec<u8> {
    let other = bytes(seed, set.data.len());
    set.facts
        .files(&other, &[index])
        .unwrap()
        .remove(0)
        .to_vec()
}

/// The file of shard `index` with the bytes at the offsets `changes` of the shard changed by
/// the masks beside them, and every checksum right.
fn altered(set: &Written, index: u16, changes: &[(usize, u8)]) -> Vec<u8> {
    let mut shard = shard_of(&set.files[usize::from(index)]);
    for &(at, mask) in changes {
        shard[at] ^= mask;
    }
    Header::new(set.facts, index)
        .unwrap()
        .file(&shard)
        .unwrap()
        .to_vec()
}

/// What the one set among `files` says about its files.
fn vote_of(files: &[Vec<u8>]) -> Vote {
    let given = slices(files);
    let pile = Pile::new(&given).unwrap();
    one_set(&pile).vote(None).unwrap()
}

/// A row of the answer.
fn row(position: usize, index: u16, blocks: u64, first: u64) -> Outvoted {
    Outvoted {
        position,
        index,
        blocks,
        first,
    }
}

/// The steps the rule gives for a vote in which every block is held whole by `m` shards, of
/// which those of `wrong` differ: each entry is the right shard and the bytes a file holds
/// instead. Per block the holders found start empty; a column in which some differ is held
/// against them, and searched when one of those that differ is not among them.
fn steps_by_the_rule(k: usize, m: usize, width: usize, wrong: &[(Vec<u8>, Vec<u8>)]) -> u128 {
    let (voters, spare) = (m as u128, (m - k) as u128);
    let most = (m - k) / 2;
    let length = wrong.first().map_or(0, |(right, _)| right.len());
    let mut steps = voters * voters;
    for start in (0..length).step_by(4) {
        let end = length.min(start + 4);
        steps += syndromes(voters, spare, ((end - start) / width) as u128);
        let mut found = vec![false; wrong.len()];
        for at in (start..end).step_by(width) {
            let differs: Vec<bool> = wrong
                .iter()
                .map(|(right, held)| right[at..at + width] != held[at..at + width])
                .collect();
            if !differs.contains(&true) {
                continue;
            }
            steps += column(spare);
            if differs
                .iter()
                .zip(&found)
                .any(|(&now, &known)| now && !known)
            {
                steps += search(voters, spare);
                for (known, &now) in found.iter_mut().zip(&differs) {
                    *known |= now;
                }
            }
            assert!(found.iter().filter(|&&known| known).count() <= most);
        }
    }
    steps
}

/// Five files of a set of three, one of them written from other data: the rebuild stops, the
/// vote names the file in every block, and the set without it gives the data.
#[test]
fn a_file_of_other_data_is_named_among_five() {
    let set = written(Field::Gf256, 7, 3, 5, 40);
    let mut files = set.files.clone();
    files[1] = foreign(&set, 1, 99);
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    let found = one_set(&pile);
    assert!(matches!(
        found.rebuild(None),
        Err(SetError::Inconsistent { block: 0, .. })
    ));

    let vote = found.vote(None).unwrap();
    assert_eq!(vote.outvoted(), [row(1, 1, 4, 0)]);
    assert_eq!((vote.undecided(), vote.first_undecided()), (0, None));
    assert!(vote.outvoted_blocks(1).eq(0..4));
    assert_eq!(vote.outvoted_blocks(0).count(), 0);
    assert_eq!(vote.undecided_blocks().count(), 0);

    // The steps: the weights of five holders once; per block the two syndromes, every column
    // that differs held against the holder found, and one search, in the first such column.
    let (right, held) = (shard_of(&set.files[1]), shard_of(&files[1]));
    let (mut steps, mut most) = (25, 25);
    for (one, other) in right.chunks(4).zip(held.chunks(4)) {
        let symbols = one.len() as u128;
        let differing = one.iter().zip(other).filter(|(a, b)| a != b).count() as u128;
        assert!(differing > 0);
        steps += syndromes(5, 2, symbols) + differing * column(2) + search(5, 2);
        most += syndromes(5, 2, symbols) + symbols * column(2) + 2 * search(5, 2);
    }
    assert_eq!(vote.steps(), steps);
    assert_eq!(vote.steps(), steps_by_the_rule(3, 5, 1, &[(right, held)]));
    assert_eq!(found.vote_work(), Ok(most));
    assert_eq!(
        found.vote(Some(most - 1)),
        Err(SetError::Work { needed: most })
    );
    assert_eq!(found.vote(Some(most)), Ok(vote));

    let rest = found.without(&[1]).unwrap();
    assert_eq!(rest.coverage().shards, 4);
    let rebuilt = rest.rebuild(None).unwrap();
    assert_eq!((rebuilt.data.to_vec(), rebuilt.bare), (set.data, 0));
    let again = rest.vote(None).unwrap();
    assert!(again.outvoted().is_empty() && again.undecided() == 0);
}

/// One wrong symbol in one block of one file: the vote names that file and that block,
/// whether the file is the shard at the point zero, another of the three lowest or a spare one.
/// The rebuild names the spare shards when one of the lowest is wrong; the vote does not.
#[test]
fn one_wrong_symbol_is_found_wherever_its_file_stands() {
    let set = written(Field::Gf256, 8, 3, 5, 40);
    for index in [0_u16, 2, 4] {
        let place = usize::from(index);
        let mut files = set.files.clone();
        files[place] = forged(&set, index, &[1]);
        let vote = vote_of(&files);
        assert_eq!(vote.outvoted(), [row(place, index, 1, 1)]);
        assert_eq!(vote.undecided(), 0);
        assert!(vote.outvoted_blocks(place).eq([1]));
        // Blocks of 4, 4, 4 and 2 symbols; one column and one search in block 1.
        let quiet = 3 * syndromes(5, 2, 4) + syndromes(5, 2, 2);
        assert_eq!(vote.steps(), 25 + quiet + column(2) + search(5, 2));
    }
}

/// What the spare shards can and cannot say: one spare shard shows that the shards differ and
/// names nobody; two outvote one file and not two; four outvote two and not three; and exactly
/// as many files as the threshold have nothing to say.
#[test]
fn the_spare_shards_outvote_half_their_number() {
    // One spare shard.
    let set = written(Field::Gf256, 9, 3, 4, 40);
    let mut files = set.files.clone();
    files[1] = forged(&set, 1, &[2]);
    let vote = vote_of(&files);
    assert!(vote.outvoted().is_empty());
    assert_eq!((vote.undecided(), vote.first_undecided()), (1, Some(2)));
    assert!(vote.undecided_blocks().eq([2]));
    // Nothing is looked for: the weights of four holders and one syndrome a block.
    let quiet = 16 + 3 * syndromes(4, 1, 4) + syndromes(4, 1, 2);
    assert_eq!(vote.steps(), quiet);
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    assert_eq!(one_set(&pile).vote_work(), Ok(quiet));

    // Two spare shards: two files wrong in different blocks are each outvoted in its own,
    // two files of other data are not.
    let set = written(Field::Gf256, 9, 3, 5, 40);
    let mut files = set.files.clone();
    (files[1], files[3]) = (forged(&set, 1, &[0]), forged(&set, 3, &[2]));
    let vote = vote_of(&files);
    assert_eq!(vote.outvoted(), [row(1, 1, 1, 0), row(3, 3, 1, 2)]);
    assert_eq!(vote.undecided(), 0);
    (files[1], files[3]) = (foreign(&set, 1, 41), foreign(&set, 3, 42));
    let vote = vote_of(&files);
    assert!(vote.outvoted().is_empty());
    assert_eq!((vote.undecided(), vote.first_undecided()), (4, Some(0)));
    assert!(vote.undecided_blocks().eq(0..4));

    // Four spare shards: the same two files are outvoted in every block, three are not.
    let set = written(Field::Gf256, 9, 3, 7, 40);
    let mut files = set.files.clone();
    (files[1], files[3]) = (foreign(&set, 1, 41), foreign(&set, 3, 42));
    let vote = vote_of(&files);
    assert_eq!(vote.outvoted(), [row(1, 1, 4, 0), row(3, 3, 4, 0)]);
    assert_eq!(vote.undecided(), 0);
    let wrong = [1, 3].map(|at| (shard_of(&set.files[at]), shard_of(&files[at])));
    assert_eq!(vote.steps(), steps_by_the_rule(3, 7, 1, &wrong));
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    let found = one_set(&pile);
    let rest = found.without(&[3, 1]).unwrap();
    assert_eq!(*rest.rebuild(None).unwrap().data, set.data);
    let most = found.vote_work().unwrap();
    assert!(vote.steps() < most);
    files[5] = foreign(&set, 5, 43);
    let vote = vote_of(&files);
    assert!(vote.outvoted().is_empty());
    assert_eq!(vote.undecided(), 4);
    assert!(vote.steps() <= most);

    // As many files as the threshold, one of them wrong: nothing to compare, nothing to pay.
    let set = written(Field::Gf256, 9, 3, 3, 40);
    let mut files = set.files.clone();
    files[1] = forged(&set, 1, &[2]);
    let vote = vote_of(&files);
    assert!(vote.outvoted().is_empty());
    assert_eq!((vote.undecided(), vote.steps()), (0, 0));
    assert_eq!(vote_of(&files[..2]).steps(), 0);
}

/// Three shards of a set of two, one of them in two files that differ in block 1. The two
/// other shards make the block; the file that agrees with them confirms it, and the other one
/// is outvoted. Without that file the set is whole again.
#[test]
fn files_of_one_shard_that_differ_are_judged_by_the_other_shards() {
    let set = written(Field::Gf256, 10, 2, 3, 30);
    let mut files = set.files.clone();
    files.push(forged(&set, 0, &[1]));
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    let found = one_set(&pile);
    assert_eq!(found.disputes().len(), 1);
    let vote = found.vote(None).unwrap();
    assert_eq!(vote.outvoted(), [row(3, 0, 1, 1)]);
    assert_eq!(vote.undecided(), 0);
    // Blocks of 4, 4, 4 and 3 symbols. Three holders and one syndrome in the blocks 0, 2 and
    // 3, whose holders are the same; two holders and the shard in dispute in block 1.
    let quiet = 9 + 2 * syndromes(3, 1, 4) + syndromes(3, 1, 3);
    assert_eq!(vote.steps(), quiet + judging(2, 1, 4));
    assert_eq!(found.vote_work(), Ok(vote.steps()));

    // A third file of the shard whose block 1 is damaged holds nothing there: it is no witness
    // and it is not named.
    let mut more = files.clone();
    more.push(set.files[0].clone());
    damage(&mut more[4], 1);
    let vote = vote_of(&more);
    assert_eq!(vote.outvoted(), [row(3, 0, 1, 1)]);

    let rest = found.without(&[3]).unwrap();
    assert!(rest.disputes().is_empty());
    assert_eq!(rest.members().len(), 3);
    assert!(rest.members().iter().all(|member| !member.copy));
    let rebuilt = rest.rebuild(None).unwrap();
    assert_eq!((rebuilt.data.to_vec(), rebuilt.bare), (set.data.clone(), 0));

    // Without the file that agreed, the other one is the shard: it differs from the rest, and
    // one spare shard cannot say who is wrong.
    let rest = found.without(&[0]).unwrap();
    assert!(rest.disputes().is_empty());
    assert!(matches!(
        rest.rebuild(None),
        Err(SetError::Inconsistent { block: 1, .. })
    ));
    let vote = rest.vote(None).unwrap();
    assert!(vote.outvoted().is_empty());
    assert!(vote.undecided_blocks().eq([1]));
}

/// A shard in dispute of which no file agrees with the others takes one spare shard: with none
/// the dispute stays open, with one both files are outvoted. Two shards in dispute beside
/// exactly `k` holders are settled only when each has a file that agrees.
#[test]
fn a_dispute_that_no_file_confirms_takes_a_spare_shard() {
    let set = written(Field::Gf256, 11, 2, 4, 30);
    let one = altered(&set, 0, &[(4, 0x55)]);
    let other = altered(&set, 0, &[(5, 0x33)]);

    // The shards 1 and 2 beside two wrong files of shard 0: nothing is established.
    let files = [
        one.clone(),
        other.clone(),
        set.files[1].clone(),
        set.files[2].clone(),
    ];
    let vote = vote_of(&files);
    assert!(vote.outvoted().is_empty());
    assert_eq!(vote.undecided(), 0);
    let quiet = 9 + 2 * syndromes(3, 1, 4) + syndromes(3, 1, 3);
    assert_eq!(vote.steps(), quiet + judging(2, 1, 4));

    // With shard 3 as well, the three holders agree and have one spare among them.
    let mut files = files.to_vec();
    files.push(set.files[3].clone());
    let vote = vote_of(&files);
    assert_eq!(vote.outvoted(), [row(0, 0, 1, 1), row(1, 0, 1, 1)]);
    assert_eq!(vote.undecided(), 0);

    // Two shards in dispute beside the shards 2 and 3. Shard 0 has a file that agrees, shard 1
    // has none: another word could have as many witnesses, so nobody is outvoted.
    let mine = altered(&set, 1, &[(4, 0x0F)]);
    let yours = altered(&set, 1, &[(6, 0xF0)]);
    let files = [
        set.files[0].clone(),
        one.clone(),
        mine.clone(),
        yours,
        set.files[2].clone(),
        set.files[3].clone(),
    ];
    let vote = vote_of(&files);
    assert!(vote.outvoted().is_empty());
    assert_eq!(vote.undecided(), 0);

    // Each of the two has a file that agrees: both other files are outvoted.
    let mut files = files.to_vec();
    files[3] = set.files[1].clone();
    let vote = vote_of(&files);
    assert_eq!(vote.outvoted(), [row(1, 0, 1, 1), row(2, 1, 1, 1)]);
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    let rest = one_set(&pile).without(&[1, 2]).unwrap();
    assert!(rest.disputes().is_empty());
    assert_eq!(*rest.rebuild(None).unwrap().data, set.data);
}

/// A holder that differs takes two spare shards and a dispute that no file confirms takes one
/// more: with two spare shards the block is undecided, with three all three files are named.
#[test]
fn a_wrong_holder_and_an_open_dispute_add_up() {
    let set = written(Field::Gf256, 12, 3, 7, 40);
    let mut files = vec![
        altered(&set, 0, &[(4, 0x55)]),
        altered(&set, 0, &[(5, 0x33)]),
    ];
    files.extend(set.files[1..5].iter().cloned());
    files.push(forged(&set, 5, &[1]));
    let vote = vote_of(&files);
    assert!(vote.outvoted().is_empty());
    assert!(vote.undecided_blocks().eq([1]));

    files.push(set.files[6].clone());
    let vote = vote_of(&files);
    assert_eq!(
        vote.outvoted(),
        [row(0, 0, 1, 1), row(1, 0, 1, 1), row(6, 5, 1, 1)]
    );
    assert_eq!(vote.undecided(), 0);
    // Seven holders in the blocks 0, 2 and 3. In block 1 six of them, three spare, one found,
    // and shard 0 made from three of the others. The holders change twice.
    let quiet = 2 * syndromes(7, 4, 4) + syndromes(7, 4, 2);
    let block = syndromes(6, 3, 4) + column(3) + search(6, 3) + judging(3, 1, 4);
    let steps = 49 + 36 + 49 + quiet + block;
    assert_eq!(vote.steps(), steps);

    // The answer does not depend on the order of the files.
    let rows = |files: &[Vec<u8>]| -> Vec<(u16, u64, u64, Vec<u8>)> {
        let vote = vote_of(files);
        assert_eq!(vote.steps(), steps);
        let named = vote.outvoted().iter();
        let mut rows: Vec<_> = named
            .map(|row| {
                (
                    row.index,
                    row.blocks,
                    row.first,
                    files[row.position].clone(),
                )
            })
            .collect();
        rows.sort();
        rows
    };
    let expected = rows(&files);
    assert_eq!(expected.len(), 3);
    files.reverse();
    assert_eq!(rows(&files), expected);
    files.rotate_left(3);
    assert_eq!(rows(&files), expected);
    files.swap(0, 5);
    assert_eq!(rows(&files), expected);
}

/// A holder that differs is no source of the block that the files of a shard in dispute are
/// compared with: here it is the lowest holder, and the file of shard 0 that agrees with the
/// others is still found to agree.
#[test]
fn the_files_of_a_dispute_are_judged_by_the_holders_that_agree() {
    let set = written(Field::Gf256, 22, 2, 5, 30);
    let files = [
        set.files[0].clone(),
        forged(&set, 0, &[1]),
        altered(&set, 1, &[(5, 0x77)]),
        set.files[2].clone(),
        set.files[3].clone(),
        set.files[4].clone(),
    ];
    let vote = vote_of(&files);
    assert_eq!(vote.outvoted(), [row(1, 0, 1, 1), row(2, 1, 1, 1)]);
    assert_eq!(vote.undecided(), 0);
    // Five holders in the blocks 0, 2 and 3; in block 1 four, of which one is found, and
    // shard 0 made from two of the others.
    let quiet = 2 * syndromes(5, 3, 4) + syndromes(5, 3, 3);
    let block = syndromes(4, 2, 4) + column(2) + search(4, 2) + judging(2, 1, 4);
    assert_eq!(vote.steps(), 25 + 16 + 25 + quiet + block);
}

/// Two files of the same wrong shard that agree with each other: both are named, in the order
/// of their places.
#[test]
fn every_file_of_an_outvoted_shard_is_named() {
    let set = written(Field::Gf256, 13, 3, 5, 40);
    let mut files = set.files.clone();
    files[4] = forged(&set, 4, &[0, 3]);
    files.insert(1, files[4].clone());
    let vote = vote_of(&files);
    assert_eq!(vote.outvoted(), [row(1, 4, 2, 0), row(5, 4, 2, 0)]);
    assert!(vote.outvoted_blocks(1).eq([0, 3]));
    assert!(vote.outvoted_blocks(5).eq([0, 3]));
    // A third file of that shard whose block 0 is damaged is named where it holds a block.
    let mut more = files.clone();
    more.push(files[1].clone());
    damage(&mut more[6], 0);
    let vote = vote_of(&more);
    assert_eq!(
        vote.outvoted(),
        [row(1, 4, 2, 0), row(5, 4, 2, 0), row(6, 4, 1, 3)]
    );
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    // Leaving one of the two out leaves the shard as wrong as before.
    let rest = one_set(&pile).without(&[1]).unwrap();
    assert_eq!(rest.vote(None).unwrap().outvoted(), [row(5, 4, 2, 0)]);
    let rest = one_set(&pile).without(&[5, 1]).unwrap();
    assert_eq!(*rest.rebuild(None).unwrap().data, set.data);
}

/// The rows of the answer shown in another order: a program sorts its own copy of them, and
/// the blocks of a file are asked for by the file's place, so each file keeps its own blocks.
/// Here one file differs in the blocks 0 and 65 and another in block 1.
#[test]
fn the_blocks_of_a_file_are_found_by_its_place_however_the_rows_are_shown() {
    let block_len = 2;
    let data = bytes(23, 420);
    let facts = SetFacts::new(Field::Gf256, &[23], 3, 420, block_len, None).unwrap();
    let files = plain(facts.files(&data, &[0, 1, 2, 3, 4, 5, 6]).unwrap());
    let set = Written { facts, data, files };
    assert_eq!(set.facts.blocks(), 70);
    let mut files = set.files.clone();
    files[0] = altered(&set, 0, &[(0, 0x11), (130, 0x22)]);
    files[1] = altered(&set, 1, &[(2, 0x44)]);
    let vote = vote_of(&files);
    assert_eq!(vote.outvoted(), [row(0, 0, 2, 0), row(1, 1, 1, 1)]);
    let blocks_of = |place: usize| -> Vec<u64> { vote.outvoted_blocks(place).collect() };
    let mut shown = vote.outvoted().to_vec();
    for turn in 0..3 {
        match turn {
            0 => shown.reverse(),
            1 => shown.sort_by_key(|row| row.blocks),
            _ => shown.sort_by_key(|row| row.position),
        }
        for row in &shown {
            let expected: &[u64] = if row.position == 0 { &[0, 65] } else { &[1] };
            assert_eq!(blocks_of(row.position), expected);
            assert_eq!(
                (row.blocks, row.first),
                (expected.len() as u64, expected[0])
            );
        }
    }
    // The answer itself is as it was.
    assert_eq!(vote.outvoted(), [row(0, 0, 2, 0), row(1, 1, 1, 1)]);
    assert_eq!(blocks_of(2), [0_u64; 0]);
}

/// Files that end early or have a damaged block: every block is voted on by the shards that
/// hold it. A file that is wrong where five hold the block is named there, and where only four
/// hold it the block stays undecided. The weights are paid again whenever the holders change.
#[test]
fn every_block_is_voted_on_by_the_shards_that_hold_it() {
    let set = written(Field::Gf256, 14, 3, 5, 40);
    let mut files = set.files.clone();
    files[1] = forged(&set, 1, &[0, 2]);
    // File 4 keeps its header and two blocks of four bytes with their checksums.
    files[4].truncate(33 + 2 * 8);
    let vote = vote_of(&files);
    assert_eq!(vote.outvoted(), [row(1, 1, 1, 0)]);
    assert_eq!((vote.undecided(), vote.first_undecided()), (1, Some(2)));
    let five = 25 + 2 * syndromes(5, 2, 4) + column(2) + search(5, 2);
    let four = 16 + syndromes(4, 1, 4) + syndromes(4, 1, 2);
    assert_eq!(vote.steps(), five + four);

    // Holders that change and come back: five, four, five and five.
    let mut files = set.files.clone();
    damage(&mut files[4], 1);
    let vote = vote_of(&files);
    assert!(vote.outvoted().is_empty() && vote.undecided() == 0);
    let five = 2 * syndromes(5, 2, 4) + syndromes(5, 2, 2);
    assert_eq!(vote.steps(), 25 + 16 + 25 + five + syndromes(4, 1, 4));
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    // The count adds, for the three blocks with two spare shards, every one of their ten
    // columns and two searches each.
    let more = 10 * column(2) + 6 * search(5, 2);
    assert_eq!(one_set(&pile).vote_work(), Ok(vote.steps() + more));
}

/// A set over the field of two-byte symbols: one wrong byte is one wrong symbol.
#[test]
fn a_set_of_two_byte_symbols_votes_as_well() {
    let set = written(Field::Gf65536, 15, 3, 5, 40);
    let mut files = set.files.clone();
    files[3] = forged(&set, 3, &[0]);
    let vote = vote_of(&files);
    assert_eq!(vote.outvoted(), [row(3, 3, 1, 0)]);
    // Blocks of 2, 2, 2 and 1 symbols.
    let quiet = 3 * syndromes(5, 2, 2) + syndromes(5, 2, 1);
    assert_eq!(vote.steps(), 25 + quiet + column(2) + search(5, 2));
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    let rest = one_set(&pile).without(&[3]).unwrap();
    assert_eq!(*rest.rebuild(None).unwrap().data, set.data);
}

/// The set without some of its files: nothing changes for places it does not have, the files
/// of another set stay where they are, and what is said of the files left is said anew.
#[test]
fn a_set_without_some_files_is_the_set_of_the_others() {
    let set = written(Field::Gf256, 16, 2, 4, 30);
    let other = written(Field::Gf256, 17, 2, 3, 30);
    // Places 0 to 4 are files of the first set, shard 0 twice; 5 to 7 are the other set.
    let mut files = vec![set.files[0].clone()];
    files.extend(set.files.iter().cloned());
    files.extend(other.files.iter().cloned());
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    assert_eq!(pile.sets().len(), 2);
    let found = &pile.sets()[0];
    assert_eq!(found.facts(), &set.facts);
    let places = |found: &FileSet<'_>| -> Vec<(usize, u16, bool)> {
        let rows = found.members().iter();
        rows.map(|member| (member.position, member.index, member.copy))
            .collect()
    };
    let all = [
        (0, 0, true),
        (1, 0, true),
        (2, 1, false),
        (3, 2, false),
        (4, 3, false),
    ];
    assert_eq!(places(found), all);

    for none in [&[][..], &[5, 6, 7, 99, usize::MAX][..]] {
        let same = found.without(none).unwrap();
        assert_eq!(places(&same), all);
        assert_eq!(same.coverage(), found.coverage());
        assert_eq!(same.disputes(), found.disputes());
        assert_eq!(same.facts(), found.facts());
        assert_eq!(*same.rebuild(None).unwrap().data, set.data);
    }
    // One of two files of shard 0 gone: the other is no copy any more.
    let rest = found.without(&[1]).unwrap();
    assert_eq!(places(&rest)[0], (0, 0, false));
    assert_eq!(rest.coverage().shards, 4);
    // Both gone, in any order and named twice: three shards are left, and then one.
    let rest = found.without(&[1, 0, 1]).unwrap();
    assert_eq!(places(&rest), [(2, 1, false), (3, 2, false), (4, 3, false)]);
    assert_eq!(*rest.rebuild(None).unwrap().data, set.data);
    let rest = rest.without(&[2, 4]).unwrap();
    assert_eq!(places(&rest), [(3, 2, false)]);
    assert_eq!(
        rest.rebuild(None),
        Err(SetError::Shards {
            found: 1,
            needed: 2
        })
    );
    assert!(
        found
            .without(&[0, 1, 2, 3, 4])
            .unwrap()
            .members()
            .is_empty()
    );
}

/// Headers that state four thousand million blocks over files of three: the count and the vote
/// go through the blocks that are in the files and no further.
#[test]
fn a_vote_costs_the_blocks_that_are_in_the_files() {
    let code = Code::new(Field::Gf256, 2).unwrap();
    let (first, second): (&[u8], &[u8]) = (&[1, 2, 3], &[4, 5, 6]);
    let mut third = code.encode(&[first, second], &[2]).unwrap().remove(0);
    third[1] ^= 0x20;
    let file = |index: u32, shard: &[u8]| {
        let header = RawHeader {
            version: 1,
            field: 1,
            flags: 0,
            id: vec![5],
            threshold: 2,
            index,
            data_len: 1 << 33,
            block_len: 1,
            digest: None,
        };
        // The header and three blocks of one byte with a checksum each; the rest is missing.
        let mut file = header.file(shard);
        file.truncate(33 + 3 * 5);
        file
    };
    let files = [file(0, first), file(1, second), file(2, &third)];
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    let found = one_set(&pile);
    assert_eq!(found.facts().blocks(), 1 << 32);
    let steps = 9 + 3 * syndromes(3, 1, 1);
    assert_eq!(found.vote_work(), Ok(steps));
    let vote = found.vote(Some(steps)).unwrap();
    assert!(vote.outvoted().is_empty());
    assert!(vote.undecided_blocks().eq([1]));
    assert_eq!(vote.steps(), steps);
}

/// A set of no data has no block to vote on.
#[test]
fn a_set_of_no_data_has_nothing_to_vote_on() {
    let set = written(Field::Gf256, 18, 2, 4, 0);
    let vote = vote_of(&set.files);
    assert!(vote.outvoted().is_empty());
    assert_eq!((vote.undecided(), vote.steps()), (0, 0));
    let given = slices(&set.files);
    let pile = Pile::new(&given).unwrap();
    assert_eq!(one_set(&pile).vote(Some(0)), Ok(vote));
    assert!(one_set(&pile).without(&[0]).is_ok());
}

/// One block of two symbols, the votes worked out apart from this crate: the data 01 04, 00 07,
/// 09 16 with threshold 3 gives the shards below; a file changed in both columns is found by one
/// search, two files changed in different columns by two, and the same two among five files
/// are beyond what two spare shards outvote, although each column alone could be put right.
#[test]
fn the_votes_computed_apart_from_this_crate_are_found() {
    let facts = SetFacts::new(Field::Gf256, &[1], 3, 6, 2, None).unwrap();
    let shards = ["0104", "0007", "0916", "0815", "3970", "3873", "3162"].map(hex);
    let written = facts.files(&hex("0104 0007 0916"), &[0, 1, 2, 3, 4, 5, 6]);
    let written = plain(written.unwrap());
    assert!(written.iter().map(|file| shard_of(file)).eq(shards.clone()));
    let file = |index: u16, shard: &str| -> Vec<u8> {
        let header = Header::new(facts, index).unwrap();
        header.file(&hex(shard)).unwrap().to_vec()
    };
    // The files given, then the rows expected, the undecided blocks, the steps and the count.
    type Case = (Vec<Vec<u8>>, Vec<Outvoted>, Vec<u64>, u128, u128);
    let mut cases: Vec<Case> = Vec::new();
    let mut files = written[..5].to_vec();
    files[0] = file(0, "0006");
    cases.push((files, vec![row(0, 0, 1, 0)], vec![], 945, 1197));
    let mut files = written.clone();
    (files[0], files[6]) = (file(0, "0004"), file(6, "3160"));
    cases.push((
        files,
        vec![row(0, 0, 1, 0), row(6, 6, 1, 0)],
        vec![],
        2841,
        2841,
    ));
    let mut files = written[..5].to_vec();
    (files[0], files[1]) = (file(0, "0004"), file(1, "0005"));
    cases.push((files, vec![], vec![0], 1197, 1197));
    let mut files = written[..4].to_vec();
    files[0] = file(0, "0004");
    cases.push((files, vec![], vec![0], 280, 280));
    for (files, outvoted, undecided, steps, most) in cases {
        let given = slices(&files);
        let pile = Pile::new(&given).unwrap();
        let found = one_set(&pile);
        let vote = found.vote(None).unwrap();
        assert_eq!(vote.outvoted(), outvoted);
        assert!(vote.undecided_blocks().eq(undecided.iter().copied()));
        assert_eq!(vote.undecided(), undecided.len() as u64);
        assert_eq!((vote.steps(), found.vote_work()), (steps, Ok(most)));
        assert_eq!(
            found.vote(Some(most - 1)),
            Err(SetError::Work { needed: most })
        );
        assert_eq!(found.vote(Some(most)), Ok(vote));
    }

    // Two shards of a set of two and two files of the third: the one that agrees with them
    // stays, the other one is named, and the block is confirmed once it is gone.
    let pairs = SetFacts::new(Field::Gf256, &[1], 2, 4, 2, None).unwrap();
    let file = |index: u16, shard: &str| -> Vec<u8> {
        let header = Header::new(pairs, index).unwrap();
        header.file(&hex(shard)).unwrap().to_vec()
    };
    let files = [
        file(0, "1122"),
        file(1, "3344"),
        file(2, "55EE"),
        file(2, "54EE"),
    ];
    assert_eq!(*pairs.files(&hex("1122 3344"), &[2]).unwrap()[0], files[2]);
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    let found = one_set(&pile);
    let rebuilt = found.rebuild(None).unwrap();
    assert_eq!((rebuilt.data.to_vec(), rebuilt.bare), (hex("1122 3344"), 1));
    let vote = found.vote(None).unwrap();
    assert_eq!(vote.outvoted(), [row(3, 2, 1, 0)]);
    assert_eq!((vote.undecided(), vote.steps()), (0, 136));
    assert_eq!(found.vote_work(), Ok(136));
    assert_eq!(found.vote(Some(135)), Err(SetError::Work { needed: 136 }));
    let rest = found.without(&[3]).unwrap();
    assert_eq!(rest.rebuild(None).unwrap().bare, 0);
}

/// What a set comes to, with the places of its files as they were in a first list of files.
#[derive(Debug, PartialEq)]
struct Shape {
    members: Vec<(usize, u16, Survey, bool)>,
    disputes: Vec<(u16, u64, u64, Vec<usize>)>,
    coverage: Coverage,
    stretches: Vec<Stretch>,
    id_shared: bool,
    rebuilt: Result<Vec<u8>, SetError>,
    outvoted: Vec<Outvoted>,
    undecided: Vec<u64>,
    steps: u128,
}

/// The shape of a set whose file at the place `p` stood at the place `origin(p)` at first.
fn shape(set: &FileSet<'_>, origin: &dyn Fn(usize) -> usize) -> Shape {
    let vote = set.vote(None).unwrap();
    let members = set.members().iter();
    let disputes = set.disputes().iter();
    let outvoted = vote.outvoted().iter();
    Shape {
        members: members
            .map(|row| (origin(row.position), row.index, row.survey, row.copy))
            .collect(),
        disputes: disputes
            .map(|row| {
                let files = row.files.iter().map(|&place| origin(place)).collect();
                (row.index, row.blocks, row.first, files)
            })
            .collect(),
        coverage: set.coverage(),
        stretches: set.stretches().unwrap().collect(),
        id_shared: set.id_shared(),
        rebuilt: set.rebuild(None).map(|rebuilt| rebuilt.data.to_vec()),
        outvoted: outvoted
            .map(|row| Outvoted {
                position: origin(row.position),
                ..*row
            })
            .collect(),
        undecided: vote.undecided_blocks().collect(),
        steps: vote.steps(),
    }
}

/// Checks that the set of `facts` among `files` without the files at `left_out` is what the
/// files left give when they are read anew, place for place.
fn left_is_as_read_anew(files: &[Vec<u8>], facts: &SetFacts, left_out: &[usize]) {
    let given = slices(files);
    let pile = Pile::new(&given).unwrap();
    let whole = pile.sets().iter().find(|set| set.facts() == facts).unwrap();
    let rest = whole.without(left_out).unwrap();

    let kept: Vec<usize> = (0..files.len())
        .filter(|place| !left_out.contains(place))
        .collect();
    let anew: Vec<&[u8]> = kept.iter().map(|&place| files[place].as_slice()).collect();
    let pile = Pile::new(&anew).unwrap();
    match pile.sets().iter().find(|set| set.facts() == facts) {
        Some(read) => assert_eq!(
            shape(&rest, &|place| place),
            shape(read, &|place| kept[place])
        ),
        None => assert!(rest.members().is_empty() && rest.coverage().shards == 0),
    }
    // Leaving out one file after another comes to leaving them all out at once.
    let mut stepwise = whole.without(&[]).unwrap();
    for &place in left_out {
        stepwise = stepwise.without(&[place]).unwrap();
    }
    assert_eq!(
        shape(&stepwise, &|place| place),
        shape(&rest, &|place| place)
    );
}

/// The set without some files is the set that the other files give when they are read anew:
/// their places, their copies, their disputes, the coverage, the data and the vote.
#[test]
fn a_set_without_files_is_what_the_others_give_when_read_anew() {
    // Seven shards with two wrong files of shard 0 and a wrong file of shard 5, beside a set
    // of another data length that has the same identifier.
    let set = written(Field::Gf256, 19, 3, 7, 40);
    let twin = written(Field::Gf256, 19, 3, 4, 44);
    let mut files = vec![
        altered(&set, 0, &[(4, 0x55)]),
        altered(&set, 0, &[(5, 0x33)]),
    ];
    files.extend(set.files[1..5].iter().cloned());
    files.push(forged(&set, 5, &[1]));
    files.push(set.files[6].clone());
    files.push(set.files[0].clone());
    files.extend(twin.files.iter().cloned());
    for left_out in [
        &[][..],
        &[0],
        &[1, 0],
        &[6],
        &[0, 1, 6],
        &[8, 0, 1],
        &[2, 3, 4, 5],
        &[7, 6, 5, 4, 3],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8],
    ] {
        left_is_as_read_anew(&files, &set.facts, left_out);
    }
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    assert!(pile.sets().iter().all(FileSet::id_shared));
    assert!(pile.sets()[0].without(&[0]).unwrap().id_shared());

    // Files cut short and a damaged block, with a copy that fills in.
    let set = written(Field::Gf256, 20, 2, 4, 30);
    let mut files = set.files.clone();
    files[0].truncate(33 + 8);
    files[2].truncate(33 + 3 * 8);
    damage(&mut files[3], 1);
    files.push(set.files[0].clone());
    files.push(forged(&set, 1, &[0, 3]));
    for left_out in [&[][..], &[4], &[0], &[5], &[1], &[1, 4], &[0, 2, 3]] {
        left_is_as_read_anew(&files, &set.facts, left_out);
    }
}

/// A file that the vote names in one block and that another block needs: the set without it
/// says which blocks are short, and the set with it is still there.
#[test]
fn leaving_out_a_file_that_another_block_needs_shows_in_the_coverage() {
    let set = written(Field::Gf256, 21, 3, 5, 40);
    let mut files = set.files.clone();
    files[4] = forged(&set, 4, &[0]);
    // The files 0 and 1 end after three blocks: the last block rests on the files 2, 3 and 4.
    files[0].truncate(33 + 3 * 8);
    files[1].truncate(33 + 3 * 8);
    let given = slices(&files);
    let pile = Pile::new(&given).unwrap();
    let found = one_set(&pile);
    let vote = found.vote(None).unwrap();
    assert_eq!(vote.outvoted(), [row(4, 4, 1, 0)]);
    assert_eq!(vote.undecided(), 0);
    assert_eq!((found.coverage().short, found.coverage().bare), (0, 1));

    let rest = found.without(&[4]).unwrap();
    let coverage = rest.coverage();
    assert_eq!((coverage.short, coverage.first_short), (1, Some(3)));
    assert_eq!(
        rest.rebuild(None),
        Err(SetError::Blocks { short: 1, first: 3 })
    );
    // The set as it was found is as it was.
    assert_eq!(found.coverage().short, 0);
    assert!(matches!(
        found.rebuild(None),
        Err(SetError::Inconsistent { block: 0, .. })
    ));
}

/// Numbers from a fixed generator (SplitMix64).
struct Dice(u64);

impl Dice {
    fn next(&mut self, below: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) % below as u64) as usize
    }
}

/// One file of a random pile: its shard index and the four bytes of its shard, two blocks of
/// two symbols. Its place in the pile is its place in the list.
type Held = (u16, [u8; 4]);

/// What the rule says about block `block` of the pile `held`, found by trying every choice of
/// `k` holders: the places of the outvoted files, and whether the block is undecided.
fn by_every_choice(code: &Code, held: &[Held], block: usize) -> (Vec<usize>, bool) {
    let k = code.threshold();
    let bytes = |file: &Held| -> [u8; 2] { [file.1[2 * block], file.1[2 * block + 1]] };
    let mut indexes: Vec<u16> = held.iter().map(|file| file.0).collect();
    indexes.sort_unstable();
    indexes.dedup();
    // A shard holds the block when its files have the same bytes of it.
    let files_of = |index: u16| {
        held.iter()
            .enumerate()
            .filter(move |(_, file)| file.0 == index)
    };
    let agrees = |index: u16| {
        let mut blocks = files_of(index).map(|(_, file)| bytes(file));
        let first = blocks.next().unwrap();
        blocks.all(|other| other == first).then_some(first)
    };
    let holders: Vec<(u16, [u8; 2])> = indexes
        .iter()
        .filter_map(|&index| Some((index, agrees(index)?)))
        .collect();
    let in_dispute: Vec<u16> = indexes
        .iter()
        .copied()
        .filter(|&index| agrees(index).is_none())
        .collect();
    let Some(spare) = holders.len().checked_sub(k) else {
        return (Vec::new(), false);
    };
    // The block of shard `index` by the word of the holders at the places `chosen`.
    let made = |chosen: &[usize], index: u16| -> [u8; 2] {
        let sources: Vec<(u16, &[u8])> = chosen
            .iter()
            .map(|&at| (holders[at].0, &holders[at].1[..]))
            .collect();
        let shard = code.rebuild(&sources, &[index]).unwrap().remove(0);
        [shard[0], shard[1]]
    };
    // Every choice of `k` holders gives a word; the one that all holders but at most half the
    // spare ones share is the candidate, and it is the only one.
    let mut candidate: Option<(Vec<usize>, Vec<usize>)> = None;
    for mask in 0_u32..1 << holders.len() {
        if mask.count_ones() as usize != k {
            continue;
        }
        let chosen: Vec<usize> = (0..holders.len())
            .filter(|at| mask >> at & 1 == 1)
            .collect();
        let differing: Vec<usize> = (0..holders.len())
            .filter(|&at| made(&chosen, holders[at].0) != holders[at].1)
            .collect();
        if differing.len() <= spare / 2 {
            if let Some((_, known)) = &candidate {
                assert_eq!(known, &differing);
            }
            candidate = Some((chosen, differing));
        }
    }
    let holders_agree = candidate
        .as_ref()
        .is_some_and(|(_, differing)| differing.is_empty());
    let Some((chosen, differing)) = candidate else {
        return (Vec::new(), true);
    };
    // The files of a shard in dispute that differ from the word; a shard none of whose files
    // carries it takes one spare shard.
    let mut outvoted = Vec::new();
    let mut unconfirmed = 0;
    for &index in &in_dispute {
        let word = made(&chosen, index);
        let wrong = files_of(index).filter(|(_, file)| bytes(file) != word);
        let before = outvoted.len();
        outvoted.extend(wrong.map(|(place, _)| place));
        unconfirmed += usize::from(outvoted.len() - before == files_of(index).count());
    }
    if 2 * differing.len() + unconfirmed > spare {
        return (Vec::new(), !holders_agree);
    }
    for &at in &differing {
        outvoted.extend(files_of(holders[at].0).map(|(place, _)| place));
    }
    outvoted.sort_unstable();
    (outvoted, false)
}

/// Thousands of small piles with wrong files and files of one shard that differ, put together
/// at random: the vote names, block by block, the files that a search over every choice of `k`
/// holders names, and leaves undecided what that search leaves undecided.
#[test]
fn the_vote_is_what_a_search_over_every_choice_of_holders_gives() {
    let mut dice = Dice(2026);
    let (mut named, mut open, mut disputes) = (0, 0, 0);
    for trial in 0..4000_u32 {
        let k = 1 + dice.next(3);
        let count = k + dice.next(6);
        let data: Vec<u8> = (0..4 * k).map(|_| dice.next(256) as u8).collect();
        let id = trial.to_be_bytes();
        let facts = SetFacts::new(Field::Gf256, &id, k, 4 * k as u64, 2, None).unwrap();
        let code = Code::new(Field::Gf256, k).unwrap();
        let indexes: Vec<u16> = (0..count as u16).collect();
        let right = facts.files(&data, &indexes).unwrap();
        // Every shard in one to three files, most of them right; a wrong file has one byte
        // changed in one or both of its blocks.
        let mut held: Vec<Held> = Vec::new();
        for index in indexes {
            if dice.next(8) == 0 {
                continue;
            }
            let shard = shard_of(&right[usize::from(index)]);
            let copies = 1 + usize::from(dice.next(4) == 0) + usize::from(dice.next(8) == 0);
            for _ in 0..copies {
                let mut bytes = [shard[0], shard[1], shard[2], shard[3]];
                for block in 0..2 {
                    if dice.next(6) == 0 {
                        bytes[2 * block + dice.next(2)] ^= 1 + dice.next(255) as u8;
                    }
                }
                held.push((index, bytes));
            }
        }
        // The files in an order of their own.
        for at in (1..held.len()).rev() {
            held.swap(at, dice.next(at + 1));
        }
        let files: Vec<Vec<u8>> = held
            .iter()
            .map(|(index, bytes)| {
                Header::new(facts, *index)
                    .unwrap()
                    .file(bytes)
                    .unwrap()
                    .to_vec()
            })
            .collect();
        if files.is_empty() {
            continue;
        }
        let given = slices(&files);
        let pile = Pile::new(&given).unwrap();
        let found = one_set(&pile);
        let vote = found.vote(None).unwrap();
        assert!(vote.steps() <= found.vote_work().unwrap(), "trial {trial}");
        disputes += found.disputes().len();

        for block in 0..2 {
            let (outvoted, undecided) = by_every_choice(&code, &held, block);
            let said: Vec<usize> = (0..held.len())
                .filter(|&place| vote.outvoted_blocks(place).any(|b| b == block as u64))
                .collect();
            assert_eq!(said, outvoted, "trial {trial}, block {block}: {held:?}");
            let left = vote.undecided_blocks().any(|b| b == block as u64);
            assert_eq!(left, undecided, "trial {trial}, block {block}: {held:?}");
            named += outvoted.len();
            open += usize::from(undecided);
        }
        // The rows say what the lists say.
        for row in vote.outvoted() {
            let blocks: Vec<u64> = vote.outvoted_blocks(row.position).collect();
            assert_eq!((row.blocks, row.first), (blocks.len() as u64, blocks[0]));
            assert_eq!(row.index, held[row.position].0);
        }
        let rows = vote.outvoted().iter().map(|row| (row.index, row.position));
        assert!(rows.is_sorted());
        assert_eq!(vote.undecided(), vote.undecided_blocks().count() as u64);
        assert_eq!(vote.first_undecided(), vote.undecided_blocks().next());
    }
    // The piles were of every kind: files named, blocks left open, shards in dispute.
    assert!(
        named > 500 && open > 200 && disputes > 500,
        "{named} {open} {disputes}"
    );
}
