//! The vote against a search over every choice of holders: random piles of small sets, each
//! block judged by trying every choice of `k` holders, which must give what `FileSet::vote` gives.

use crate::sets::{one_set, shard_of, slices};
use reed_solomon_core::{Code, Field, Header, Pile, SetFacts};

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
