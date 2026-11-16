//! Which files of a set differ from what its shards agree on, block by block. With `r` spare
//! holders, a word shared by all but `r / 2` of them is the only such word. A word is accepted only
//! when it is sure to beat every other (`2·w + u ≤ r`), which errs on the safe side.

use super::shards::{Bits, Shard, Sweep, reach, room};
use super::{ALLOWANCE, FileSet, SetError, wide};
use crate::buffer::Buffer;
use crate::code::Code;
use crate::correct::Finder;
use alloc::vec::Vec;
use zeroize::Zeroizing;

/// A file that differs from the word its set establishes, in blocks that it holds with a
/// checksum that holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outvoted {
    /// The place of the file among the files given.
    pub position: usize,
    /// The index of its shard.
    pub index: u16,
    /// In how many blocks it differs; [`Vote::outvoted_blocks`] lists them.
    pub blocks: u64,
    /// The first such block.
    pub first: u64,
}

/// What the files of a set say about each other, block by block: the answer of
/// [`FileSet::vote`]. It is read and not changed: a program that shows its rows in another
/// order sorts a copy of them, and asks for the blocks of a file by the file's place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vote {
    outvoted: Vec<Outvoted>,
    undecided: u64,
    first_undecided: Option<u64>,
    steps: u128,
    /// The blocks of each outvoted file, in the order of `outvoted`.
    marks: Vec<Bits>,
    /// The undecided blocks.
    open: Bits,
}

impl Vote {
    /// The files that differ from an established block, in the order of their shard indexes
    /// and then of their places.
    #[must_use]
    pub fn outvoted(&self) -> &[Outvoted] {
        &self.outvoted
    }

    /// How many blocks have holders that are not one set and no established word: nothing says
    /// which of them is wrong there. [`Vote::undecided_blocks`] lists them.
    #[must_use]
    pub const fn undecided(&self) -> u64 {
        self.undecided
    }

    /// The first such block.
    #[must_use]
    pub const fn first_undecided(&self) -> Option<u64> {
        self.first_undecided
    }

    /// The steps the call took: at most what [`FileSet::vote_work`] counts.
    #[must_use]
    pub const fn steps(&self) -> u128 {
        self.steps
    }

    /// The blocks in which the file at the place `position` was outvoted, in order. Nothing for
    /// a file that was not.
    pub fn outvoted_blocks(&self, position: usize) -> impl Iterator<Item = u64> + '_ {
        let rows = self.outvoted.iter().zip(&self.marks);
        let found = rows.filter(move |(row, _)| row.position == position);
        found.flat_map(|(_, marks)| marks.ones())
    }

    /// The undecided blocks, in order.
    pub fn undecided_blocks(&self) -> impl Iterator<Item = u64> + '_ {
        self.open.ones()
    }
}

/// What the parts of the vote on one block cost in steps, for a block with `voters` holders of
/// which `spare` are beyond the threshold.
struct Rates {
    voters: u128,
    spare: u128,
    /// The most holders that can be outvoted: half the spare ones.
    most: u128,
}

impl Rates {
    fn new(voters: usize, spare: usize) -> Self {
        Self {
            voters: wide(voters),
            spare: wide(spare),
            most: wide(spare / 2),
        }
    }

    /// The weights of the holders, once for every run of blocks they all hold.
    const fn weights(&self) -> u128 {
        self.voters * self.voters
    }

    /// The syndromes of a block of `symbols` symbols.
    const fn syndromes(&self, symbols: u128) -> u128 {
        self.spare * self.voters * (symbols + ALLOWANCE)
    }

    /// A column whose syndromes are not all zero, held against the holders found so far.
    const fn column(&self) -> u128 {
        self.spare * (self.most + 1)
    }

    /// A column searched for its wrong symbols.
    const fn search(&self) -> u128 {
        (4 * self.voters + ALLOWANCE) * (self.spare + 1)
    }

    /// The most a block of `symbols` symbols costs beyond its syndromes: every column held
    /// against the holders found, and a search of one column more than holders can be outvoted,
    /// or of every column when the block has fewer. With one spare holder nothing can be found,
    /// and nothing is looked for.
    const fn finding(&self, symbols: u128) -> u128 {
        if self.most == 0 {
            return 0;
        }
        let searches = if symbols > self.most {
            self.most + 1
        } else {
            symbols
        };
        symbols * self.column() + searches * self.search()
    }
}

/// The steps of comparing the files of `disputed` shards with what `threshold` holders make of
/// a block of `symbols` symbols: their sources prepared, and one block made per shard.
fn judging(threshold: usize, disputed: usize, symbols: u128) -> u128 {
    let sources = wide(threshold);
    sources * sources + wide(disputed) * sources * (symbols + ALLOWANCE)
}

/// The symbols of a block as its holders hold it.
fn symbols_of(holders: &[(u16, &[u8])], width: usize) -> u128 {
    wide(holders.first().map_or(0, |(_, bytes)| bytes.len()) / width)
}

impl FileSet<'_> {
    /// An upper bound on the steps [`FileSet::vote`] can take, to set its limit by. It assumes every
    /// column disagrees and the most searches per block; reading files and asking for memory are not
    /// counted. Fails only when there's no memory for the count.
    pub fn vote_work(&self) -> Result<u128, SetError> {
        let code = self.facts.code();
        let (threshold, width) = (code.threshold(), code.field().symbol_bytes());
        let mut voters = Vec::new();
        room(&mut voters, self.shards.len())?;
        let mut sweep = Sweep::new(&self.shards)?;
        let mut steps = 0;
        for _ in 0..reach(&self.shards).min(self.facts.blocks()) {
            let (holders, in_dispute) = sweep.next_both();
            let Some(spare) = holders.len().checked_sub(threshold) else {
                continue;
            };
            let symbols = symbols_of(holders, width);
            if spare > 0 {
                let rates = Rates::new(holders.len(), spare);
                let indexes = holders.iter().map(|&(index, _)| index);
                if !indexes.clone().eq(voters.iter().copied()) {
                    steps += rates.weights();
                    voters.clear();
                    voters.extend(indexes);
                }
                steps += rates.syndromes(symbols) + rates.finding(symbols);
            }
            if !in_dispute.is_empty() {
                steps += judging(threshold, in_dispute.len(), symbols);
            }
        }
        Ok(steps)
    }

    /// Names the files that differ from what the other shards agree on, block by block; nothing is
    /// rebuilt, and [`FileSet::without`] gives the set without them. Worth asking after
    /// [`SetError::Inconsistent`]. A block's word is accepted when the dissenters fit within the spare
    /// shards (`2·w + u ≤ r`); otherwise the block is undecided. Fails above `limit` or without memory.
    pub fn vote(&self, limit: Option<u128>) -> Result<Vote, SetError> {
        let needed = self.vote_work()?;
        if limit.is_some_and(|limit| needed > limit) {
            return Err(SetError::Work { needed });
        }
        let mut poll = Poll::new(self)?;
        let mut sweep = Sweep::new(&self.shards)?;
        for number in 0..reach(&self.shards).min(self.facts.blocks()) {
            let (holders, in_dispute) = sweep.next_both();
            let number = u32::try_from(number).unwrap_or(u32::MAX);
            poll.block(number, holders, in_dispute)?;
        }
        poll.finish()
    }
}

/// The blocks in which one file was outvoted.
#[derive(Default)]
struct Mark {
    blocks: u64,
    first: u64,
    bits: Bits,
}

/// A vote in progress: the search kept from block to block, the lists of the comparison with
/// shards in dispute, and what was found so far.
struct Poll<'s, 'a> {
    code: Code,
    shards: &'s [Shard<'a>],
    /// The most bytes a block of the set can have: the block length, or the whole shard when
    /// that is shorter. What a header allows beyond it is never in a file.
    block: usize,
    /// The search for the holders of the block before, kept while the holders stay the same.
    finder: Option<Finder>,
    /// The indexes of the holders the search was made for.
    voters: Vec<u16>,
    /// The positions, among the holders of the block at hand, of those that differ, in rising
    /// order.
    wrong: Zeroizing<Vec<usize>>,
    /// The holders a block of a shard in dispute is made from: their indexes, weights and bytes.
    lowest: Vec<u16>,
    weights: Vec<u16>,
    stripes: Vec<&'a [u8]>,
    /// The block of a shard in dispute, as those holders make it, with room for the longest.
    made: Buffer,
    /// The files of shards in dispute that differ from it in the block at hand: the shard's
    /// place among the shards, and the file's number among its files.
    differing: Zeroizing<Vec<(usize, usize)>>,
    /// Per shard, where the marks of its files begin.
    firsts: Vec<usize>,
    /// Per file, in the order of the shards and of their files, the blocks it was outvoted in.
    marks: Vec<Mark>,
    open: Bits,
    undecided: u64,
    first_undecided: Option<u64>,
    steps: u128,
}

impl<'s, 'a> Poll<'s, 'a> {
    fn new(set: &'s FileSet<'a>) -> Result<Self, SetError> {
        let shards = &set.shards;
        let longest = u64::from(set.facts.block_len()).min(set.facts.shard_len());
        let (mut voters, mut wrong) = (Vec::new(), Zeroizing::new(Vec::new()));
        let (mut firsts, mut marks) = (Vec::new(), Vec::new());
        room(&mut voters, shards.len())?;
        room(&mut wrong, shards.len() / 2)?;
        room(&mut firsts, shards.len())?;
        room(&mut marks, set.members.len())?;
        for shard in shards {
            firsts.push(marks.len());
            marks.extend((0..shard.files()).map(|_| Mark::default()));
        }
        Ok(Self {
            code: set.facts.code(),
            shards,
            // A block longer than this machine addresses is not in a file.
            block: usize::try_from(longest).unwrap_or(usize::MAX),
            finder: None,
            voters,
            wrong,
            lowest: Vec::new(),
            weights: Vec::new(),
            stripes: Vec::new(),
            made: Buffer::default(),
            differing: Zeroizing::new(Vec::new()),
            firsts,
            marks,
            open: Bits::default(),
            undecided: 0,
            first_undecided: None,
            steps: 0,
        })
    }

    /// The vote on block `number`, with its holders and the shards whose files differ on it.
    fn block(
        &mut self,
        number: u32,
        holders: &[(u16, &'a [u8])],
        in_dispute: &[&'s Shard<'a>],
    ) -> Result<(), SetError> {
        let threshold = self.code.threshold();
        let Some(spare) = holders.len().checked_sub(threshold) else {
            return Ok(());
        };
        let symbols = symbols_of(holders, self.code.field().symbol_bytes());
        self.wrong.clear();
        if spare > 0 && !self.look(holders, spare, symbols)? {
            return self.leave_open(number);
        }
        self.differing.clear();
        let mut unconfirmed = 0;
        if !in_dispute.is_empty() {
            self.steps += judging(threshold, in_dispute.len(), symbols);
            unconfirmed = self.judge(number, holders, in_dispute)?;
        }
        if 2 * self.wrong.len() + unconfirmed > spare {
            // The holders agree and a dispute stays as it is, or they differ and nothing
            // says which of them is wrong.
            return match self.wrong.is_empty() {
                true => Ok(()),
                false => self.leave_open(number),
            };
        }
        let (wrong, differing) = (
            core::mem::take(&mut self.wrong),
            core::mem::take(&mut self.differing),
        );
        for &position in wrong.iter() {
            let index = holders.get(position).map(|&(index, _)| index);
            let found = index.and_then(|index| self.shard(index));
            if let Some((at, shard)) = found {
                for (slot, _) in shard.holding(number) {
                    self.mark(at, slot, number)?;
                }
            }
        }
        for &(at, slot) in differing.iter() {
            self.mark(at, slot, number)?;
        }
        (self.wrong, self.differing) = (wrong, differing);
        Ok(())
    }

    /// Looks for the holders that differ from the word the others share in a block of
    /// `symbols` symbols and leaves their positions in `self.wrong`; false when the holders
    /// settle on no word.
    fn look(
        &mut self,
        holders: &[(u16, &'a [u8])],
        spare: usize,
        symbols: u128,
    ) -> Result<bool, SetError> {
        let rates = Rates::new(holders.len(), spare);
        let indexes = holders.iter().map(|&(index, _)| index);
        let kept = self.finder.as_ref();
        if !kept.is_some_and(|finder| finder.points().iter().copied().eq(indexes.clone())) {
            // The search of the holders before goes before the next one is asked for.
            self.finder = None;
            self.voters.clear();
            self.voters.extend(indexes);
            let field = self.code.field();
            let finder = Finder::new(field, &self.voters, spare, self.block);
            self.finder = Some(finder.map_err(|_| SetError::Memory)?);
            self.steps += rates.weights();
        }
        let finder = self.finder.as_mut().ok_or(SetError::Memory)?;
        let look = finder.look(holders);
        self.steps += rates.syndromes(symbols)
            + wide(look.columns) * rates.column()
            + wide(look.searches) * rates.search();
        if look.settled {
            self.wrong.extend_from_slice(finder.found());
            self.wrong.sort_unstable();
        }
        Ok(look.settled)
    }

    /// Compares the files of the shards in dispute on block `number` with what the holders
    /// that agree make of it. Returns how many of those shards have no file that agrees; the
    /// files that differ are left in `self.differing`.
    fn judge(
        &mut self,
        number: u32,
        holders: &[(u16, &'a [u8])],
        in_dispute: &[&'s Shard<'a>],
    ) -> Result<usize, SetError> {
        let threshold = self.code.threshold();
        self.lowest.clear();
        self.stripes.clear();
        room(&mut self.lowest, threshold)?;
        room(&mut self.stripes, threshold)?;
        let more = threshold.saturating_sub(self.weights.len());
        room(&mut self.weights, more)?;
        self.weights.resize(threshold, 0);
        let agreeing = holders
            .iter()
            .enumerate()
            .filter(|(position, _)| self.wrong.binary_search(position).is_err());
        for (_, &(index, bytes)) in agreeing.take(threshold) {
            self.lowest.push(index);
            self.stripes.push(bytes);
        }
        let length = self.stripes.first().map_or(0, |stripe| stripe.len());
        if self.made.len() < length {
            self.made = Buffer::zeros(length).ok_or(SetError::Memory)?;
        }
        let made = self.made.get_mut(..length).unwrap_or_default();
        let sources = self.code.sources(&self.lowest, &mut self.weights)?;
        let mut unconfirmed = 0;
        for shard in in_dispute {
            sources.build(shard.index, &self.stripes, made)?;
            let at = self
                .shards
                .binary_search_by_key(&shard.index, |shard| shard.index);
            let mut confirmed = false;
            for (slot, bytes) in shard.holding(number) {
                if *bytes == *made {
                    confirmed = true;
                } else if let Ok(at) = at {
                    room(&mut self.differing, 1)?;
                    self.differing.push((at, slot));
                }
            }
            unconfirmed += usize::from(!confirmed);
        }
        Ok(unconfirmed)
    }

    /// The shard with this index and its place among the shards.
    fn shard(&self, index: u16) -> Option<(usize, &'s Shard<'a>)> {
        let found = self
            .shards
            .binary_search_by_key(&index, |shard| shard.index);
        let at = found.ok()?;
        Some((at, self.shards.get(at)?))
    }

    /// Notes that the file numbered `slot` of the shard at the place `at` was outvoted in block
    /// `number`.
    fn mark(&mut self, at: usize, slot: usize, number: u32) -> Result<(), SetError> {
        let first = self.firsts.get(at).copied().unwrap_or(usize::MAX);
        let place = first.checked_add(slot);
        if let Some(mark) = place.and_then(|place| self.marks.get_mut(place)) {
            mark.bits.put(number)?;
            if mark.blocks == 0 {
                mark.first = u64::from(number);
            }
            mark.blocks += 1;
        }
        Ok(())
    }

    /// Notes that nothing says which holder of block `number` is wrong.
    fn leave_open(&mut self, number: u32) -> Result<(), SetError> {
        self.open.put(number)?;
        self.first_undecided.get_or_insert(u64::from(number));
        self.undecided += 1;
        Ok(())
    }

    /// The answer: the outvoted files in the order of their shards and places.
    fn finish(self) -> Result<Vote, SetError> {
        let count = self.marks.iter().filter(|mark| mark.blocks > 0).count();
        let (mut rows, mut outvoted, mut bits) = (Vec::new(), Vec::new(), Vec::new());
        room(&mut rows, count)?;
        room(&mut outvoted, count)?;
        room(&mut bits, count)?;
        let mut marks = self.marks.into_iter();
        for shard in self.shards {
            for (slot, mark) in (0..shard.files()).zip(marks.by_ref()) {
                if let Some(position) = shard.place(slot).filter(|_| mark.blocks > 0) {
                    let row = Outvoted {
                        position,
                        index: shard.index,
                        blocks: mark.blocks,
                        first: mark.first,
                    };
                    rows.push((row, mark.bits));
                }
            }
        }
        rows.sort_unstable_by_key(|(row, _)| (row.index, row.position));
        for (row, marks) in rows {
            outvoted.push(row);
            bits.push(marks);
        }
        Ok(Vote {
            outvoted,
            undecided: self.undecided,
            first_undecided: self.first_undecided,
            steps: self.steps,
            marks: bits,
            open: self.open,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::Field;
    use crate::file::{Header, Pile, SetFacts};

    /// Headers that allow blocks of two million bytes over shards of two bytes: the search is
    /// sized by the bytes a block of the set can have, four bytes of syndromes here, and the
    /// answer is that of the same set with blocks of two bytes.
    #[test]
    fn the_search_is_sized_by_the_bytes_a_block_can_have() {
        for block_len in [2, 1 << 21] {
            let facts = SetFacts::new(Field::Gf256, &[1], 3, 6, block_len, None).unwrap();
            let data = [0x01, 0x04, 0x00, 0x07, 0x09, 0x16];
            let mut files = facts.files(&data, &[0, 1, 2, 3, 4]).unwrap();
            files[0] = Header::new(facts, 0).unwrap().file(&[0x00, 0x06]).unwrap();
            let given: Vec<&[u8]> = files.iter().map(|bytes| bytes.as_slice()).collect();
            let pile = Pile::new(&given).unwrap();
            let set = &pile.sets()[0];
            let mut poll = Poll::new(set).unwrap();
            let mut sweep = Sweep::new(&set.shards).ok().unwrap();
            let (holders, in_dispute) = sweep.next_both();
            poll.block(0, holders, in_dispute).unwrap();
            assert_eq!(poll.finder.as_ref().unwrap().syndrome_bytes(), 4);
            let vote = set.vote(None).unwrap();
            assert_eq!((vote.outvoted().len(), vote.steps()), (1, 945));
        }
    }
}
