//! The shards of a set as its files hold them: one bit per block of every file, the files of one
//! shard side by side, and the walk over the blocks that finds who holds each.
//!
//! Every loop here is bounded by the blocks that are in the files. A file is looked at for a
//! block only when it reaches the block, so short files beside a long one cost their own blocks
//! and no more, be they files of other shards or further files of the same shard. Every list is
//! asked for before it is filled, and a refusal of the memory is an answer, not an abort.

use super::{Dispute, Member, SetError};
use crate::file::FileError;
use crate::file::reading::ShardFile;
use alloc::vec::Vec;
use core::ops::Range;

/// The memory for a list was refused.
pub(super) struct Full;

impl From<Full> for FileError {
    fn from(_: Full) -> Self {
        Self::Memory
    }
}

impl From<Full> for SetError {
    fn from(_: Full) -> Self {
        Self::Memory
    }
}

/// Room for `more` further items in `list`.
pub(super) fn room<T>(list: &mut Vec<T>, more: usize) -> Result<(), Full> {
    list.try_reserve(more).map_err(|_| Full)
}

/// One bit per block, for the blocks a file reaches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Bits(Vec<u64>);

impl Bits {
    /// Bits in one word.
    const WORD: u32 = u64::BITS;

    /// The same bits in memory of their own.
    fn copy(&self) -> Result<Self, Full> {
        let mut words = Vec::new();
        room(&mut words, self.0.len())?;
        words.extend_from_slice(&self.0);
        Ok(Self(words))
    }

    pub(super) fn put(&mut self, number: u32) -> Result<(), Full> {
        let word = usize::try_from(number / Self::WORD).map_err(|_| Full)?;
        if word >= self.0.len() {
            let more = word + 1 - self.0.len();
            room(&mut self.0, more)?;
            self.0.resize(word + 1, 0);
        }
        if let Some(slot) = self.0.get_mut(word) {
            *slot |= 1 << (number % Self::WORD);
        }
        Ok(())
    }

    fn has(&self, number: u32) -> bool {
        usize::try_from(number / Self::WORD)
            .ok()
            .and_then(|word| self.0.get(word))
            .is_some_and(|slot| (slot >> (number % Self::WORD)) & 1 == 1)
    }

    /// The numbers whose bit is set, in order. Words without a bit are passed over.
    pub(super) fn ones(&self) -> impl Iterator<Item = u64> + '_ {
        let width = u64::from(Self::WORD);
        let words = self.0.iter().zip(0_u64..).filter(|&(&word, _)| word != 0);
        words.flat_map(move |(&word, at)| {
            (0..width)
                .filter(move |bit| (word >> bit) & 1 == 1)
                .map(move |bit| at * width + bit)
        })
    }
}

/// One file that holds a shard.
#[derive(Clone, Debug)]
struct Held<'a> {
    position: usize,
    file: ShardFile<'a>,
    /// The blocks whose checksum holds.
    intact: Bits,
    /// How many blocks are wholly there, from the first one on.
    reach: u64,
}

/// One shard of the set and the files that hold it, the one that reaches furthest first.
#[derive(Clone, Debug)]
pub(super) struct Shard<'a> {
    pub(super) index: u16,
    held: Vec<Held<'a>>,
    /// The blocks on which two of the files, each with a checksum that holds, differ.
    disputed: Bits,
}

impl<'a> Shard<'a> {
    /// The most blocks one of the files reaches.
    fn reach(&self) -> u64 {
        self.held.first().map_or(0, |held| held.reach)
    }

    /// The files that reach block `number`: they come first, the longest first.
    fn reaching(&self, number: u32) -> impl Iterator<Item = &Held<'a>> {
        self.held
            .iter()
            .take_while(move |held| held.reach > u64::from(number))
    }

    /// How many files hold the shard.
    pub(super) fn files(&self) -> usize {
        self.held.len()
    }

    /// The place, among the files given, of the file numbered `slot` among the shard's files.
    pub(super) fn place(&self, slot: usize) -> Option<usize> {
        self.held.get(slot).map(|held| held.position)
    }

    /// The files that hold block `number` with a checksum that holds: the number of each among
    /// the shard's files, and its bytes of the block.
    pub(super) fn holding(&self, number: u32) -> impl Iterator<Item = (usize, &'a [u8])> {
        let reaching = self.reaching(number).enumerate();
        let intact = reaching.filter(move |(_, held)| held.intact.has(number));
        intact.filter_map(move |(slot, held)| Some((slot, held.file.bytes_at(number)?)))
    }

    /// The bytes of block `number`, on which the files of the shard do not differ, when one of
    /// them holds it intact. `looks` grows by the files looked at.
    fn block(&self, number: u32, looks: &mut u64) -> Option<&'a [u8]> {
        for held in self.reaching(number) {
            *looks += 1;
            if held.intact.has(number) {
                return held.file.bytes_at(number);
            }
        }
        None
    }

    /// The blocks on which the files of the shard differ, in order.
    pub(super) fn disputed(&self) -> impl Iterator<Item = u64> + '_ {
        self.disputed.ones()
    }

    /// Puts the files in the order the walks need and compares them block by block: where two
    /// that hold a block differ, the block is marked and every file that holds it took part,
    /// whichever comes first. Returns the dispute, if there is one, and the files looked at.
    fn settle(&mut self) -> Result<(Option<Dispute>, u64), Full> {
        self.held.sort_unstable_by(|one, other| {
            (other.reach, one.position).cmp(&(one.reach, other.position))
        });
        // One file has nothing to be compared with.
        if self.held.len() < 2 {
            return Ok((None, 0));
        }
        let mut took_part = Vec::new();
        room(&mut took_part, self.held.len())?;
        took_part.resize(self.held.len(), false);
        let (mut blocks, mut first, mut looks) = (0_u64, 0, 0);
        for number in 0..=u32::MAX {
            let reaching = self.reaching(number).count();
            if reaching == 0 {
                break;
            }
            looks += wide(reaching);
            let differs = {
                let holding = self.reaching(number).filter(|held| held.intact.has(number));
                let mut bytes = holding.map(|held| held.file.bytes_at(number));
                bytes
                    .next()
                    .is_some_and(|shown| !bytes.all(|other| other == shown))
            };
            if !differs {
                continue;
            }
            for (held, part) in self.held.iter().zip(took_part.iter_mut()).take(reaching) {
                *part |= held.intact.has(number);
            }
            self.disputed.put(number)?;
            if blocks == 0 {
                first = u64::from(number);
            }
            blocks += 1;
        }
        if blocks == 0 {
            return Ok((None, looks));
        }
        let mut files = Vec::new();
        room(&mut files, self.held.len())?;
        let parts = self.held.iter().zip(&took_part);
        files.extend(parts.filter_map(|(held, part)| part.then_some(held.position)));
        files.sort_unstable();
        let dispute = Dispute {
            index: self.index,
            blocks,
            first,
            files,
        };
        Ok((Some(dispute), looks))
    }
}

/// The files of a set sorted into its shards, with what was found on the way.
pub(super) struct Gathered<'a> {
    /// The shards, in the order of their indexes.
    pub(super) shards: Vec<Shard<'a>>,
    /// A row for every file, in the order of the shards.
    pub(super) members: Vec<Member>,
    /// The shards whose files differ.
    pub(super) disputes: Vec<Dispute>,
}

/// Sorts `files`, which state one set's facts and come in the order of their shard indexes,
/// into shards. Every block of every file is checked here, once, and the files of one shard
/// are compared. Returns the files looked at while comparing as well.
pub(super) fn gather<'a>(files: &[(usize, ShardFile<'a>)]) -> Result<(Gathered<'a>, u64), Full> {
    let mut shards: Vec<Shard<'a>> = Vec::new();
    let mut members = Vec::new();
    room(&mut members, files.len())?;
    for &(position, file) in files {
        let mut intact = Bits::default();
        let survey = file.walk(|number| intact.put(number))?;
        let index = file.header().index();
        let held = Held {
            position,
            file,
            intact,
            reach: survey.intact + survey.damaged,
        };
        place(&mut shards, index, held)?;
        members.push(Member {
            position,
            index,
            survey,
            copy: false,
        });
    }
    settle_all(shards, members)
}

/// The shards again without the files at the places `left_out`, which come in rising order.
/// No block is read anew: what each file holds is taken as it was found, and the files left of
/// each shard are compared again. `members` are the rows of all files, in the order of the
/// shards and, within one, of the places.
pub(super) fn keep<'a>(
    shards: &[Shard<'a>],
    members: &[Member],
    left_out: &[usize],
) -> Result<Gathered<'a>, Full> {
    let stays = |position: usize| left_out.binary_search(&position).is_err();
    let mut kept = Vec::new();
    room(&mut kept, members.len())?;
    let files = shards
        .iter()
        .flat_map(|shard| shard.held.iter().map(move |held| (shard.index, held)));
    kept.extend(files.filter(|(_, held)| stays(held.position)));
    // The files in the order of their rows: by shard, then by place.
    kept.sort_unstable_by_key(|&(index, held)| (index, held.position));
    let (mut again, mut rows) = (Vec::new(), Vec::new());
    room(&mut rows, kept.len())?;
    let staying = members.iter().filter(|row| stays(row.position));
    for (&(index, held), row) in kept.iter().zip(staying) {
        let held = Held {
            position: held.position,
            file: held.file,
            intact: held.intact.copy()?,
            reach: held.reach,
        };
        place(&mut again, index, held)?;
        rows.push(Member {
            copy: false,
            ..*row
        });
    }
    Ok(settle_all(again, rows)?.0)
}

/// Adds a file to the shards, which grow in the order of their indexes: to the last shard when
/// it has this index, to a new one otherwise.
fn place<'a>(shards: &mut Vec<Shard<'a>>, index: u16, held: Held<'a>) -> Result<(), Full> {
    match shards.last_mut() {
        Some(shard) if shard.index == index => {
            room(&mut shard.held, 1)?;
            shard.held.push(held);
        }
        _ => {
            let mut first = Vec::new();
            room(&mut first, 1)?;
            first.push(held);
            room(shards, 1)?;
            shards.push(Shard {
                index,
                held: first,
                disputed: Bits::default(),
            });
        }
    }
    Ok(())
}

/// Compares the files of every shard and says in the rows of the files which shards have more
/// than one. Returns the files looked at while comparing as well.
fn settle_all<'a>(
    mut shards: Vec<Shard<'a>>,
    mut members: Vec<Member>,
) -> Result<(Gathered<'a>, u64), Full> {
    // The rows of the files come in the order of the shards, each shard's files together.
    let mut rows = members.iter_mut();
    let (mut disputes, mut looks) = (Vec::new(), 0);
    for shard in &mut shards {
        let copy = shard.held.len() > 1;
        for row in rows.by_ref().take(shard.held.len()) {
            row.copy = copy;
        }
        let (dispute, looked) = shard.settle()?;
        looks += looked;
        if let Some(dispute) = dispute {
            room(&mut disputes, 1)?;
            disputes.push(dispute);
        }
    }
    let gathered = Gathered {
        shards,
        members,
        disputes,
    };
    Ok((gathered, looks))
}

/// The most blocks a file of any of `shards` reaches.
pub(super) fn reach(shards: &[Shard<'_>]) -> u64 {
    shards.iter().map(Shard::reach).max().unwrap_or(0)
}

/// The shards that hold each block, block after block from the first.
///
/// Only the shards whose files reach a block are looked at for it, and when one of them ends
/// only the shards still on the list are gone through, never all of them again.
pub(super) struct Sweep<'s, 'a> {
    /// The shards that reach the next block, in the order of their indexes.
    reaching: Vec<&'s Shard<'a>>,
    /// The first block that one of them no longer reaches.
    ends: u64,
    next: u64,
    /// The holders of the block last asked for, with room for every shard.
    holders: Vec<(u16, &'a [u8])>,
    /// The shards whose files differ on that block, with room for every shard.
    in_dispute: Vec<&'s Shard<'a>>,
    /// How many times a shard or one of its files was looked at: what the walk cost.
    looks: u64,
}

impl<'s, 'a> Sweep<'s, 'a> {
    pub(super) fn new(shards: &'s [Shard<'a>]) -> Result<Self, Full> {
        let (mut reaching, mut holders, mut in_dispute) = (Vec::new(), Vec::new(), Vec::new());
        room(&mut reaching, shards.len())?;
        room(&mut holders, shards.len())?;
        room(&mut in_dispute, shards.len())?;
        reaching.extend(shards.iter().filter(|shard| shard.reach() > 0));
        let ends = reaching
            .iter()
            .map(|shard| shard.reach())
            .min()
            .unwrap_or(0);
        Ok(Self {
            reaching,
            ends,
            next: 0,
            holders,
            in_dispute,
            looks: 0,
        })
    }

    /// The shards that hold the next block and their bytes of it, in the order of their
    /// indexes.
    pub(super) fn next_block(&mut self) -> &[(u16, &'a [u8])] {
        self.advance();
        &self.holders
    }

    /// The holders of the next block, as [`Sweep::next_block`] gives them, and the shards whose
    /// files differ on it, in the order of their indexes.
    pub(super) fn next_both(&mut self) -> (&[(u16, &'a [u8])], &[&'s Shard<'a>]) {
        self.advance();
        (&self.holders, &self.in_dispute)
    }

    /// Finds who holds the next block and whose files differ on it.
    fn advance(&mut self) {
        let number = self.next;
        self.next += 1;
        if number >= self.ends {
            self.looks += wide(self.reaching.len());
            self.reaching.retain(|shard| shard.reach() > number);
            let ends = self.reaching.iter().map(|shard| shard.reach()).min();
            self.ends = ends.unwrap_or(u64::MAX);
        }
        self.holders.clear();
        self.in_dispute.clear();
        let number = u32::try_from(number).unwrap_or(u32::MAX);
        for shard in &self.reaching {
            if shard.disputed.has(number) {
                self.in_dispute.push(shard);
            } else if let Some(bytes) = shard.block(number, &mut self.looks) {
                self.holders.push((shard.index, bytes));
            }
        }
    }
}

/// A run of blocks, one after another, that the same number of shards hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stretch {
    /// The blocks: from the first number to before the last.
    pub blocks: Range<u64>,
    /// How many shards hold each of them.
    pub holders: usize,
}

/// The blocks of a set in stretches held by one number of shards, from the first block to the
/// last. The blocks that no file reaches are one stretch, however many they are.
pub struct Stretches<'s, 'a> {
    sweep: Sweep<'s, 'a>,
    /// Blocks that at least one file reaches.
    reached: u64,
    blocks: u64,
    next: u64,
    /// The holders of block `next`, when it was looked at already.
    ahead: Option<usize>,
}

impl<'s, 'a> Stretches<'s, 'a> {
    pub(super) fn new(shards: &'s [Shard<'a>], blocks: u64) -> Result<Self, Full> {
        Ok(Self {
            sweep: Sweep::new(shards)?,
            reached: reach(shards).min(blocks),
            blocks,
            next: 0,
            ahead: None,
        })
    }
}

impl Iterator for Stretches<'_, '_> {
    type Item = Stretch;

    fn next(&mut self) -> Option<Stretch> {
        let start = self.next;
        if start >= self.blocks {
            return None;
        }
        let mut holders = 0;
        if start < self.reached {
            holders = match self.ahead.take() {
                Some(holders) => holders,
                None => self.sweep.next_block().len(),
            };
            self.next += 1;
            while self.next < self.reached {
                let following = self.sweep.next_block().len();
                if following != holders {
                    self.ahead = Some(following);
                    break;
                }
                self.next += 1;
            }
        }
        // Past the longest file nothing is held: one stretch, joined to blocks without a
        // holder just before it.
        if self.next >= self.reached && holders == 0 {
            self.next = self.blocks;
        }
        Some(Stretch {
            blocks: start..self.next,
            holders,
        })
    }
}

/// A count of files in the type of the looks.
fn wide(count: usize) -> u64 {
    u64::try_from(count).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::Field;
    use crate::file::{Pile, SetFacts};

    /// What a walk over every block of `shards` holds in all and what it cost in looks.
    fn walk(shards: &[Shard<'_>], blocks: u64) -> (u64, u64) {
        let mut sweep = Sweep::new(shards).ok().unwrap();
        let held = (0..blocks).map(|_| wide(sweep.next_block().len())).sum();
        (held, sweep.looks)
    }

    #[test]
    fn a_bit_is_kept_for_any_block_number() {
        let mut bits = Bits::default();
        let numbers = [0, 63, 64, 1000, 100_000];
        for number in numbers {
            assert!(!bits.has(number));
            assert!(bits.put(number).is_ok());
            assert!(bits.has(number));
        }
        assert!(!bits.has(1) && !bits.has(65) && !bits.has(99_999) && !bits.has(u32::MAX));
        assert!(bits.ones().eq(numbers.map(u64::from)));
    }

    /// Thirty files of thirty shards that end one block later each, beside one file of four
    /// thousand blocks: looking at every shard for every block would be 124,000 looks. The
    /// walk looks only at the shards that reach a block, and at the shards on its list when
    /// one of them ends: at most twice the blocks that are in the files, and once more every
    /// shard.
    #[test]
    fn the_walk_over_the_blocks_costs_the_blocks_that_are_in_the_files() {
        let facts = SetFacts::new(Field::Gf256, &[1], 2, 8000, 1, None).unwrap();
        assert_eq!(facts.blocks(), 4000);
        let indexes: Vec<u16> = (0..=30).collect();
        let files = facts.files(&[7; 8000], &indexes).unwrap();
        // File `i` keeps its header and `i` blocks of one byte and a checksum; file 0 is whole.
        let cut: Vec<&[u8]> = files
            .iter()
            .enumerate()
            .map(|(at, file)| &file[..if at == 0 { file.len() } else { 33 + 5 * at }])
            .collect();
        let read: Vec<(usize, ShardFile<'_>)> = cut
            .iter()
            .enumerate()
            .map(|(at, file)| (at, ShardFile::read(file).unwrap()))
            .collect();
        let (gathered, comparing) = gather(&read).ok().unwrap();
        // No shard has two files: nothing to compare.
        assert_eq!(comparing, 0);
        let in_files = 4000 + (1..=30).sum::<u64>();
        let (held, looks) = walk(&gathered.shards, facts.blocks());
        assert_eq!(held, in_files);
        assert!(looks <= 2 * in_files + 31, "{looks} looks");
        assert!(looks < 124_000 / 10);
    }

    /// The same with thirty-one files of one shard: thirty that end one block later each and
    /// one of four thousand blocks, in which block 2000 is damaged. Comparing them and finding
    /// the holder of each block costs the blocks that are in the files, not the blocks of the
    /// longest times the files: for the damaged block only the file that reaches it is looked
    /// at.
    #[test]
    fn files_of_one_shard_cost_the_blocks_that_are_in_them() {
        let facts = SetFacts::new(Field::Gf256, &[1], 2, 8000, 1, None).unwrap();
        let mut files = facts.files(&[7; 8000], &[0; 31]).unwrap();
        files[0][33 + 5 * 2000] ^= 1;
        for stepped in [true, false] {
            // Thirty files cut after one block each, or after one block more each.
            let kept = |at: usize| 33 + 5 * if stepped { at } else { 1 };
            let cut: Vec<&[u8]> = files
                .iter()
                .enumerate()
                .map(|(at, file)| &file[..if at == 0 { file.len() } else { kept(at) }])
                .collect();
            let read: Vec<(usize, ShardFile<'_>)> = cut
                .iter()
                .enumerate()
                .map(|(at, file)| (at, ShardFile::read(file).unwrap()))
                .collect();
            let (gathered, comparing) = gather(&read).ok().unwrap();
            assert_eq!(gathered.shards.len(), 1);
            assert!(gathered.disputes.is_empty());
            let short = if stepped { (1..=30).sum::<u64>() } else { 30 };
            // A file counts once for every block it reaches: the comparison goes over the
            // files that reach a block a fixed number of times, and over no other file.
            assert_eq!(comparing, 4000 + short);
            let (held, looks) = walk(&gathered.shards, facts.blocks());
            // The longest file holds every block but one, each found at the first look; the
            // damaged block costs one look as well.
            assert_eq!((held, looks), (3999, 4000));
        }
        // The pile itself agrees: one shard, every file a copy.
        let whole: Vec<&[u8]> = files.iter().map(|bytes| bytes.as_slice()).collect();
        let pile = Pile::new(&whole).unwrap();
        assert!(pile.sets()[0].members().iter().all(|member| member.copy));
    }
}
