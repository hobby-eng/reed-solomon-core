//! Random piles of damaged and forged shard files against oracles written here: the vote never
//! names a file that holds the true block where the truth is within the stated rule, and names
//! every file that holds another; the steps stay within their counts; a pile answers the same in
//! any order; repaired files read back whole; a digest-bearing rebuild gives the data or refuses.
#![allow(clippy::all)]

use reed_solomon_core::{Block, DataDigest, Field, FileSet, Header, Pile, SetFacts, ShardFile};
use std::collections::BTreeMap;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// The blocks of a file, as the reader finds them: Some(bytes) for an intact block.
fn blocks_of(file: &[u8], count: u64) -> Vec<Option<Vec<u8>>> {
    let Ok(read) = ShardFile::read(file) else {
        return vec![None; count as usize];
    };
    (0..count)
        .map(|b| match read.block(b as u32) {
            Some(Block::Intact(bytes)) => Some(bytes.to_vec()),
            _ => None,
        })
        .collect()
}

/// What a set comes to, with file positions mapped to the original file numbers.
fn summary(set: &FileSet<'_>, origin: &[usize]) -> String {
    let mut members: Vec<(u16, usize, bool, u64, u64)> = set
        .members()
        .iter()
        .map(|m| (m.index, origin[m.position], m.copy, m.survey.intact, m.survey.damaged))
        .collect();
    members.sort();
    let mut disputes: Vec<(u16, u64, u64, Vec<usize>)> = set
        .disputes()
        .iter()
        .map(|d| {
            let mut f: Vec<usize> = d.files.iter().map(|&p| origin[p]).collect();
            f.sort();
            (d.index, d.blocks, d.first, f)
        })
        .collect();
    disputes.sort();
    let rebuilt = set
        .rebuild(None)
        .map(|r| (r.data.to_vec(), r.digest, r.bare, r.steps));
    let vote = set.vote(None).map(|v| {
        let mut rows: Vec<(u16, usize, u64, u64, Vec<u64>)> = v
            .outvoted()
            .iter()
            .map(|o| {
                (o.index, origin[o.position], o.blocks, o.first, v.outvoted_blocks(o.position).collect())
            })
            .collect();
        rows.sort();
        (rows, v.undecided(), v.first_undecided(), v.undecided_blocks().collect::<Vec<_>>(), v.steps())
    });
    format!(
        "{:?} {} {:?} {:?} {:?} {:?} {:?}",
        set.facts(),
        set.id_shared(),
        members,
        disputes,
        set.coverage(),
        rebuilt,
        vote
    )
}

fn main() {
    let rounds: u64 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(2000);
    let seed0: u64 = std::env::args().nth(2).and_then(|a| a.parse().ok()).unwrap_or(1);
    let mut stats = BTreeMap::<&str, u64>::new();
    for round in 0..rounds {
        let mut rng = Rng(seed0.wrapping_mul(1_000_003).wrapping_add(round));
        let field = [Field::Gf256, Field::Gf65536][rng.below(2)];
        let width = field.symbol_bytes();
        let k = 1 + rng.below(4);
        let n = k + rng.below(5);
        let block = (1 + rng.below(4)) * width;
        let digest = [DataDigest::None, DataDigest::Sha256][rng.below(2)];
        let data: Vec<u8> = (0..rng.below(40)).map(|_| rng.next() as u8).collect();
        let facts = SetFacts::for_data(field, b"r", k, &data, block as u32, digest).unwrap();
        let indexes: Vec<u16> = (0..n as u16).collect();
        let written: Vec<Vec<u8>> =
            facts.files(&data, &indexes).unwrap().iter().map(|f| f.to_vec()).collect();
        let count = facts.blocks();
        let truth: Vec<Vec<Vec<u8>>> = written
            .iter()
            .map(|f| blocks_of(f, count).into_iter().map(|b| b.unwrap()).collect())
            .collect();
        // Files given: (bytes, shard index it claims).
        let mut files: Vec<(Vec<u8>, u16)> = written.iter().cloned().zip(indexes.iter().copied()).collect();
        for _ in 0..rng.below(5) {
            let at = rng.below(files.len());
            match rng.below(7) {
                // A bit wrong: the checksum catches it.
                0 => {
                    let f = &mut files[at].0;
                    if !f.is_empty() {
                        let p = rng.below(f.len());
                        f[p] ^= 1 << rng.below(8);
                    }
                }
                // Cut short.
                1 => {
                    let l = files[at].0.len();
                    files[at].0.truncate(rng.below(l + 1));
                }
                // Another copy of a file, maybe of a forged one.
                2 => {
                    let c = files[at].clone();
                    files.push(c);
                }
                // A forged file: blocks changed under checksums that hold.
                3 | 4 => {
                    let index = indexes[rng.below(indexes.len())];
                    let mut shard: Vec<u8> = truth[index as usize].concat();
                    if !shard.is_empty() {
                        for _ in 0..=rng.below(2) {
                            let p = rng.below(shard.len());
                            shard[p] ^= (rng.next() as u8) | 1;
                        }
                    }
                    let f = Header::new(facts, index).unwrap().file(&shard).unwrap().to_vec();
                    if rng.below(2) == 0 {
                        files[at] = (f, index);
                    } else {
                        files.push((f, index));
                    }
                }
                // Left out.
                5 => {
                    if files.len() > 1 {
                        files.remove(at);
                    }
                }
                // Bytes after the end.
                _ => {
                    let extra = rng.below(20);
                    for _ in 0..extra {
                        let b = rng.next() as u8;
                        files[at].0.push(b);
                    }
                }
            }
        }
        let given: Vec<&[u8]> = files.iter().map(|(f, _)| f.as_slice()).collect();
        let pile = Pile::new(&given).unwrap();
        let origin: Vec<usize> = (0..given.len()).collect();
        let Some(set) = pile.sets().iter().find(|s| s.facts() == &facts) else {
            *stats.entry("no own set").or_default() += 1;
            continue;
        };

        // Steps within their counts.
        let vote = set.vote(None).unwrap();
        if !set.disputes().is_empty() {
            *stats.entry("with disputes").or_default() += 1;
        }
        if !vote.outvoted().is_empty() {
            *stats.entry("with outvoted").or_default() += 1;
        }
        if vote.undecided() > 0 {
            *stats.entry("with undecided").or_default() += 1;
        }
        if matches!(set.rebuild(None), Err(reed_solomon_core::SetError::Inconsistent { .. })) {
            *stats.entry("inconsistent").or_default() += 1;
        }
        let bound = set.vote_work().unwrap();
        assert!(vote.steps() <= bound, "round {round}: vote {} > bound {bound}", vote.steps());
        if let Ok(r) = set.rebuild(None) {
            assert_eq!(set.rebuild_work(), Ok(r.steps), "round {round}");
            if digest == DataDigest::Sha256 {
                assert_eq!(*r.data, *data, "round {round}: digest-bearing rebuild gave other data");
            }
            if *r.data != *data {
                *stats.entry("rebuild other data").or_default() += 1;
                if r.bare == 0 {
                    println!("round {round}: other data with every block confirmed");
                }
            } else {
                *stats.entry("rebuild ok").or_default() += 1;
            }
        }
        for index in [0u16, (n as u16), 1] {
            if let Ok(made) = set.repair(index, None) {
                let read = ShardFile::read(&made).unwrap();
                assert!(read.survey().whole(), "round {round}");
                assert_eq!(read.header(), &Header::new(facts, index).unwrap());
                assert_eq!(set.repair_work(index).is_ok(), true);
            }
        }

        // The vote against the truth, block by block.
        let per_file: Vec<Vec<Option<Vec<u8>>>> =
            files.iter().map(|(f, _)| blocks_of(f, count)).collect();
        let reads: Vec<Option<u16>> = files
            .iter()
            .map(|(f, _)| {
                ShardFile::read(f)
                    .ok()
                    .filter(|r| r.header().set() == &facts)
                    .map(|r| r.header().index())
            })
            .collect();
        let outvoted: BTreeMap<usize, Vec<u64>> = vote
            .outvoted()
            .iter()
            .map(|o| (o.position, vote.outvoted_blocks(o.position).collect()))
            .collect();
        let undecided: Vec<u64> = vote.undecided_blocks().collect();
        assert_eq!(undecided.len() as u64, vote.undecided());
        for o in vote.outvoted() {
            assert_eq!(outvoted[&o.position].len() as u64, o.blocks);
            assert_eq!(outvoted[&o.position].first().copied(), Some(o.first));
            assert_eq!(reads[o.position], Some(o.index));
        }
        for b in 0..count {
            // Per shard index: its intact copies at b.
            let mut copies: BTreeMap<u16, Vec<(usize, &Vec<u8>)>> = BTreeMap::new();
            for (p, idx) in reads.iter().enumerate() {
                if let (Some(idx), Some(bytes)) = (idx, &per_file[p][b as usize]) {
                    copies.entry(*idx).or_default().push((p, bytes));
                }
            }
            let mut m = 0;
            let mut w = 0;
            let mut u = 0;
            for (idx, list) in &copies {
                let first = list[0].1;
                let disputed = list.iter().any(|(_, x)| *x != first);
                let true_block = &truth[*idx as usize][b as usize];
                if disputed {
                    if !list.iter().any(|(_, x)| *x == true_block) {
                        u += 1;
                    }
                } else {
                    m += 1;
                    if first != true_block {
                        w += 1;
                    }
                }
            }
            let within = m >= k && 2 * w + u <= m - k;
            for (idx, list) in &copies {
                let true_block = &truth[*idx as usize][b as usize];
                for (p, bytes) in list {
                    let named = outvoted.get(p).is_some_and(|bl| bl.contains(&b));
                    let honest = *bytes == true_block;
                    if within && honest && named {
                        panic!("round {round}: honest file {p} outvoted in block {b} (m {m} w {w} u {u} k {k})");
                    }
                    if within && !honest && !named {
                        panic!("round {round}: wrong file {p} not named in block {b} (m {m} w {w} u {u} k {k})");
                    }
                    if !within && named && honest {
                        *stats.entry("honest named outside the rule").or_default() += 1;
                    }
                }
            }
            if within && undecided.contains(&b) {
                panic!("round {round}: block {b} undecided within the rule");
            }
        }

        // The same pile in another order.
        let mut order: Vec<usize> = (0..given.len()).collect();
        for i in (1..order.len()).rev() {
            let j = rng.below(i + 1);
            order.swap(i, j);
        }
        let turned: Vec<&[u8]> = order.iter().map(|&i| given[i]).collect();
        let pile2 = Pile::new(&turned).unwrap();
        assert_eq!(pile.sets().len(), pile2.sets().len());
        for (one, two) in pile.sets().iter().zip(pile2.sets()) {
            assert_eq!(summary(one, &origin), summary(two, &order), "round {round}: order changes a set");
        }

        // The set without the outvoted files.
        let out: Vec<usize> = vote.outvoted().iter().map(|o| o.position).collect();
        let left = set.without(&out).unwrap();
        if let Ok(r) = left.rebuild(None) {
            if digest == DataDigest::Sha256 {
                assert_eq!(*r.data, *data);
            }
            if *r.data == *data {
                *stats.entry("without outvoted ok").or_default() += 1;
            }
        }
        *stats.entry("rounds").or_default() += 1;
    }
    println!("{stats:?}");
}
