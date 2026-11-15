//! A set found among shard files: which blocks each shard holds, and its data or a lost file made
//! from them block by block. Each block comes from its `k` lowest holders, and every further holder
//! is compared with them, so files that don't belong together are noticed.

mod shards;
mod vote;

use super::FileError;
use super::facts::{SetFacts, sha256};
use super::header::Header;
use super::reading::{ShardFile, Survey};
use crate::buffer::Buffer;
use crate::code::{CodeError, Sources};
use alloc::vec::Vec;
use core::fmt;
use shards::{Full, Gathered, Shard, Sweep, gather, keep, room};

pub use shards::{Stretch, Stretches};
pub use vote::{Outvoted, Vote};

/// Steps charged per source for working out the coefficients of one block.
const ALLOWANCE: u128 = 64;

/// One file of a set, as it was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member {
    /// The place of the file among the files given.
    pub position: usize,
    /// The index of its shard.
    pub index: u16,
    /// Its pieces by their condition.
    pub survey: Survey,
    /// Whether another file of the set holds the same shard.
    pub copy: bool,
}

/// Files of one shard that differ in a block although each has a checksum that holds there. The
/// shard is not used for such a block; nothing says which file is the right one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dispute {
    /// The index of the shard.
    pub index: u16,
    /// How many of its blocks the files differ on; [`FileSet::disputed_blocks`] lists them.
    pub blocks: u64,
    /// The first such block.
    pub first: u64,
    /// The places, among the files given, of the files that took part, in rising order: every
    /// file that holds one of those blocks with a checksum that holds, whichever it agrees
    /// with.
    pub files: Vec<usize>,
}

/// What a set found among files still holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coverage {
    /// Different shards among the files.
    pub shards: usize,
    /// Shards that rebuild the set: its threshold.
    pub needed: usize,
    /// Blocks in every shard of the set.
    pub blocks: u64,
    /// Blocks held by fewer shards than needed. While there is one, the set does not rebuild.
    pub short: u64,
    /// The first such block.
    pub first_short: Option<u64>,
    /// Blocks held by exactly as many shards as needed: they rebuild, and nothing is left to
    /// compare the result with.
    pub bare: u64,
}

/// The data of a set, rebuilt. Its `Debug` text gives the length of the data, not its bytes.
#[derive(PartialEq, Eq)]
pub struct Rebuilt {
    /// The data, in a buffer that wipes itself when it is dropped.
    pub data: Buffer,
    /// Whether the SHA-256 of the data was compared with the one the files state.
    pub digest: DigestCheck,
    /// Blocks that were held by exactly `k` shards: rebuilt with nothing to compare them with.
    /// With zero, every block was confirmed by at least one further shard.
    pub bare: u64,
    /// The steps the call took, as [`FileSet::rebuild_work`] counts them.
    pub steps: u128,
}

impl fmt::Debug for Rebuilt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Rebuilt")
            .field("length", &self.data.len())
            .field("digest", &self.digest)
            .field("bare", &self.bare)
            .field("steps", &self.steps)
            .finish_non_exhaustive()
    }
}

/// What a rebuild found when it compared the digest of the data. Data whose digest differs from
/// the one the files state is not given back: [`SetError::Digest`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DigestCheck {
    /// The files state a SHA-256, and the rebuilt data, exactly as long as the files say, has
    /// it. The data is the data the files were written for, or someone stated the digest of
    /// other data along with it: only a digest trusted from elsewhere tells these apart.
    Matched([u8; 32]),
    /// The files state no digest: nothing was compared. That the data is the data once written
    /// is for the program to check.
    Unchecked,
}

/// Why a set was not rebuilt, or a file of it not made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetError {
    /// Fewer different shards among the files than the set needs. This holds for a set of no
    /// data as well: one file never stands for a set.
    Shards {
        /// Different shards found.
        found: usize,
        /// Shards needed.
        needed: usize,
    },
    /// Some blocks are held by fewer shards than the set needs.
    Blocks {
        /// How many blocks.
        short: u64,
        /// The first of them.
        first: u64,
    },
    /// The shards that hold a block, each with a checksum that holds, are not one set there: a
    /// shard differs from what the `k` lowest make. Which of them is wrong cannot be told from
    /// this: it may be one of the lowest. [`FileSet::vote`] tells, where enough shards hold the
    /// block.
    Inconsistent {
        /// The block.
        block: u64,
        /// The shards the block was made from.
        base: Vec<u16>,
        /// The shards that differ from what those make.
        differing: Vec<u16>,
    },
    /// The bytes after the end of the rebuilt data are not zero: the set was not written by the
    /// rules of the format.
    Padding,
    /// The files agree on data whose SHA-256 is not the one they state. The data is not given
    /// back. Either the data or the stated digest was changed after writing, in every file
    /// alike, or the files were written for other data.
    Digest {
        /// The digest the files state.
        stated: [u8; 32],
        /// The digest of the data they agree on.
        found: [u8; 32],
    },
    /// The work is above the limit given. Nothing was computed.
    Work {
        /// The steps the call needs.
        needed: u128,
    },
    /// This machine has no memory for the result or for a list the call keeps.
    Memory,
    /// The field has no shard with the index asked for.
    Index,
    /// The code refused shards that had passed the checks of their files. That is a fault of
    /// this crate, not of the files.
    Code(CodeError),
}

impl fmt::Display for SetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shards { found, needed } => {
                write!(
                    f,
                    "the files hold {found} different shards of the {needed} the set needs"
                )
            }
            Self::Blocks { short, first } => write!(
                f,
                "{short} blocks, the first numbered {first}, are held by fewer shards than the set needs"
            ),
            Self::Inconsistent { block, .. } => {
                write!(f, "the shards that hold block {block} are not one set")
            }
            Self::Padding => f.write_str("the bytes after the end of the data are not zero"),
            Self::Digest { .. } => {
                f.write_str("the rebuilt data does not have the SHA-256 the files state")
            }
            Self::Work { needed } => write!(f, "the call needs {needed} steps, above its limit"),
            Self::Memory => f.write_str("this machine has no memory for the result"),
            Self::Index => f.write_str("the field has no shard with this index"),
            Self::Code(refusal) => write!(f, "the code refused checked shards: {refusal}"),
        }
    }
}

impl core::error::Error for SetError {}

impl From<CodeError> for SetError {
    fn from(refusal: CodeError) -> Self {
        Self::Code(refusal)
    }
}

/// The sizes of a set in the type of an index into memory.
struct Sizes {
    threshold: usize,
    /// Bytes of a symbol.
    width: usize,
    shard: usize,
    block: usize,
}

/// A set as it was found among files: the files that state its facts, shard by shard.
#[derive(Clone, Debug)]
pub struct FileSet<'a> {
    facts: SetFacts,
    id_shared: bool,
    shards: Vec<Shard<'a>>,
    members: Vec<Member>,
    disputes: Vec<Dispute>,
    coverage: Coverage,
}

impl<'a> FileSet<'a> {
    /// The set of `files`, which state `facts` and come in the order of their shard indexes,
    /// each with its place among the files given. Every block of every file is checked here,
    /// once.
    pub(super) fn new(
        facts: SetFacts,
        files: &[(usize, ShardFile<'a>)],
    ) -> Result<Self, FileError> {
        let (gathered, _) = gather(files)?;
        let Gathered {
            shards,
            members,
            disputes,
        } = gathered;
        let coverage = cover(&facts, &shards)?;
        Ok(Self {
            facts,
            id_shared: false,
            shards,
            members,
            disputes,
            coverage,
        })
    }

    /// Says that another set among the files has the same identifier and other facts.
    pub(super) fn share_id(&mut self) {
        self.id_shared = true;
    }

    /// The facts every file of the set states.
    #[must_use]
    pub const fn facts(&self) -> &SetFacts {
        &self.facts
    }

    /// Whether another set among the files has the same identifier and other facts. Two sets
    /// never share an identifier by the rules, so one of them is not what it says; nothing here
    /// tells which, and each is rebuilt only when asked for by itself.
    #[must_use]
    pub const fn id_shared(&self) -> bool {
        self.id_shared
    }

    /// The files of the set in the order of their shard indexes, each as it was found.
    #[must_use]
    pub fn members(&self) -> &[Member] {
        &self.members
    }

    /// The shards whose files differ in a block that each holds intact.
    #[must_use]
    pub fn disputes(&self) -> &[Dispute] {
        &self.disputes
    }

    /// How many shards hold the blocks of the set.
    #[must_use]
    pub const fn coverage(&self) -> Coverage {
        self.coverage
    }

    /// The blocks of the set in stretches held by the same number of shards: what rebuilds, what spare
    /// shards confirm, what is lost. Fails only without memory.
    pub fn stretches(&self) -> Result<Stretches<'_, 'a>, FileError> {
        Ok(Stretches::new(&self.shards, self.facts.blocks())?)
    }

    /// The blocks of shard `index` on which its files differ, in order: every one of them, where
    /// [`FileSet::disputes`] has their number and the first. Nothing for a shard without a
    /// dispute.
    pub fn disputed_blocks(&self, index: u16) -> impl Iterator<Item = u64> + '_ {
        let found = self
            .shards
            .binary_search_by_key(&index, |shard| shard.index);
        let shard = found.ok().and_then(|at| self.shards.get(at));
        shard.into_iter().flat_map(Shard::disputed)
    }

    /// The steps [`FileSet::rebuild`] needs, to set its limit by. A step counts the work of making
    /// shards only, not time, reading or memory. The count says what the call costs, not that it will
    /// succeed. Fails when the files can't rebuild the set, or without memory.
    pub fn rebuild_work(&self) -> Result<u128, SetError> {
        self.ready()?;
        self.work(&self.data_shards()?)
    }

    /// The steps [`FileSet::repair`] needs for shard `index`, counted as [`FileSet::rebuild_work`]
    /// counts. Fails for an index outside the field first, otherwise as that call.
    pub fn repair_work(&self, index: u16) -> Result<u128, SetError> {
        self.target(index)?;
        self.work(&[index])
    }

    /// The data of the set, from whichever files hold each block. It is returned only when every block
    /// was made and every spare shard agreed, and, if the files state a SHA-256, only when the data has
    /// it ([`DigestCheck::Matched`]). Without a digest, whether this is the original is for the program
    /// to check. Fails when shards or blocks are missing, above `limit`, on disagreement, on non-zero
    /// padding, on a digest mismatch, or without memory.
    pub fn rebuild(&self, limit: Option<u128>) -> Result<Rebuilt, SetError> {
        self.ready()?;
        let (mut data, steps) = self.make(&self.data_shards()?, limit)?;
        let length = usize::try_from(self.facts.data_len()).map_err(|_| SetError::Memory)?;
        let padding = data.get(length..).ok_or(SetError::Memory)?;
        if padding.iter().any(|&byte| byte != 0) {
            return Err(SetError::Padding);
        }
        data.truncate(length);
        let digest = match self.facts.digest() {
            None => DigestCheck::Unchecked,
            Some(&stated) => {
                let found = sha256(&data);
                if found != stated {
                    return Err(SetError::Digest { stated, found });
                }
                DigestCheck::Matched(found)
            }
        };
        Ok(Rebuilt {
            data,
            digest,
            bare: self.coverage.bare,
            steps,
        })
    }

    /// The file of shard `index`, made from the files at hand: a lost file byte for byte, or a new one.
    /// Spare shards are still compared, but the data isn't assembled or its digest checked; a later
    /// rebuild checks that. Fails as [`FileSet::rebuild`], or for an index outside the field.
    pub fn repair(&self, index: u16, limit: Option<u128>) -> Result<Buffer, SetError> {
        let header = self.target(index)?;
        let (shard, _) = self.make(&[index], limit)?;
        header.file(&shard).map_err(|_| SetError::Memory)
    }

    /// The steps [`FileSet::repairs`] needs for `indexes`. All of them are made in one pass, so each
    /// block's sources are prepared once; a repeated index counts once.
    pub fn repairs_work(&self, indexes: &[u16]) -> Result<u128, SetError> {
        let targets = self.targets(indexes)?;
        if targets.is_empty() {
            return Ok(0);
        }
        self.work(&targets)
    }

    /// The files of shards `indexes`, in the order asked, made in one pass: each block is read and
    /// compared once for all of them. A repeated index gives the same file twice. Fails as
    /// [`FileSet::repair`].
    pub fn repairs(&self, indexes: &[u16], limit: Option<u128>) -> Result<Vec<Buffer>, SetError> {
        let targets = self.targets(indexes)?;
        let mut files = Vec::new();
        room(&mut files, indexes.len())?;
        if targets.is_empty() {
            return Ok(files);
        }
        let (shards, _) = self.make(&targets, limit)?;
        let shard_len = usize::try_from(self.facts.shard_len()).map_err(|_| SetError::Memory)?;
        for &index in indexes {
            let place = targets.binary_search(&index).map_err(|_| SetError::Index)?;
            let start = place.checked_mul(shard_len).ok_or(SetError::Memory)?;
            let shard = shards
                .get(start..start + shard_len)
                .ok_or(SetError::Memory)?;
            let file = self
                .target(index)?
                .file(shard)
                .map_err(|_| SetError::Memory)?;
            files.push(file);
        }
        Ok(files)
    }

    /// The different indexes of `indexes` in rising order, each checked as [`FileSet::target`]
    /// checks one.
    fn targets(&self, indexes: &[u16]) -> Result<Vec<u16>, SetError> {
        for &index in indexes {
            self.target(index)?;
        }
        let mut targets = Vec::new();
        room(&mut targets, indexes.len())?;
        targets.extend_from_slice(indexes);
        targets.sort_unstable();
        targets.dedup();
        Ok(targets)
    }

    /// The set without the files at `files`, for a program that distrusts them, as after
    /// [`FileSet::vote`]. Files are dropped whole, so a file wrong in one block can't silently feed
    /// another. Fails only without memory.
    pub fn without(&self, files: &[usize]) -> Result<Self, FileError> {
        let mut left_out = Vec::new();
        room(&mut left_out, files.len())?;
        left_out.extend_from_slice(files);
        left_out.sort_unstable();
        let Gathered {
            shards,
            members,
            disputes,
        } = keep(&self.shards, &self.members, &left_out)?;
        let coverage = cover(&self.facts, &shards)?;
        Ok(Self {
            facts: self.facts,
            id_shared: self.id_shared,
            shards,
            members,
            disputes,
            coverage,
        })
    }

    /// The header of shard `index` of the set: the one check of an index asked for, made
    /// before a count as before the work.
    fn target(&self, index: u16) -> Result<Header, SetError> {
        Header::new(self.facts, index).map_err(|_| SetError::Index)
    }

    /// The indexes of the data shards, 0 to `k - 1`. They are listed only for a set that has
    /// its `k` shards: a header alone, whatever threshold it names, gets no list.
    fn data_shards(&self) -> Result<Vec<u16>, Full> {
        let threshold = self.facts.code().threshold();
        let mut indexes = Vec::new();
        room(&mut indexes, threshold)?;
        indexes.extend((0..=u16::MAX).take(threshold));
        Ok(indexes)
    }

    /// Why the set does not rebuild, if it does not.
    fn ready(&self) -> Result<(), SetError> {
        let Coverage {
            shards,
            needed,
            short,
            first_short,
            ..
        } = self.coverage;
        if shards < needed {
            return Err(SetError::Shards {
                found: shards,
                needed,
            });
        }
        match first_short {
            Some(first) => Err(SetError::Blocks { short, first }),
            None => Ok(()),
        }
    }

    /// The steps it takes to make the shards `targets`, given in rising order.
    fn work(&self, targets: &[u16]) -> Result<u128, SetError> {
        self.ready()?;
        let needed = self.facts.code().threshold();
        let width = self.facts.code().field().symbol_bytes();
        let sources = wide(needed);
        let mut lowest = Vec::new();
        room(&mut lowest, needed)?;
        let mut sweep = Sweep::new(&self.shards)?;
        let mut steps = 0;
        for number in 0..self.facts.blocks() {
            let holders = sweep.next_block();
            let indexes = holders.iter().map(|&(index, _)| index);
            if number == 0 || !indexes.clone().take(needed).eq(lowest.iter().copied()) {
                steps += sources * sources;
                lowest.clear();
                lowest.extend(indexes.clone().take(needed));
            }
            let held = indexes
                .filter(|index| targets.binary_search(index).is_ok())
                .count();
            let made = targets.len().saturating_sub(held);
            let spare = holders.len().saturating_sub(needed);
            let symbols = holders.first().map_or(0, |(_, bytes)| bytes.len()) / width;
            steps += wide(made + spare) * sources * (wide(symbols) + ALLOWANCE);
        }
        Ok(steps)
    }

    /// The shards `targets`, given in rising order, one after another in one buffer, and the
    /// steps taken: every block from the `k` lowest shards that hold it, every further holder
    /// compared with them.
    fn make(&self, targets: &[u16], limit: Option<u128>) -> Result<(Buffer, u128), SetError> {
        let needed = self.work(targets)?;
        if limit.is_some_and(|limit| needed > limit) {
            return Err(SetError::Work { needed });
        }
        let code = self.facts.code();
        let threshold = code.threshold();
        let sizes = Sizes {
            threshold,
            width: code.field().symbol_bytes(),
            shard: usize::try_from(self.facts.shard_len()).map_err(|_| SetError::Memory)?,
            // A block longer than this machine addresses is longer than the shard.
            block: usize::try_from(self.facts.block_len()).unwrap_or(usize::MAX),
        };
        let length = sizes
            .shard
            .checked_mul(targets.len())
            .ok_or(SetError::Memory)?;
        let mut out = Buffer::zeros(length).ok_or(SetError::Memory)?;
        let (mut stripes, mut lowest, mut weights) = (Vec::new(), Vec::new(), Vec::new());
        room(&mut stripes, threshold)?;
        room(&mut lowest, threshold)?;
        room(&mut weights, threshold)?;
        weights.resize(threshold, 0);

        let blocks = self.facts.blocks();
        let mut sweep = Sweep::new(&self.shards)?;
        let (mut number, mut steps) = (0, 0);
        let mut holders: &[(u16, &[u8])] = &[];
        if blocks > 0 {
            holders = sweep.next_block();
        }
        while number < blocks {
            // The weights of these sources serve every following block with the same ones.
            lowest.clear();
            lowest.extend(holders.iter().take(threshold).map(|&(index, _)| index));
            let sources = code.sources(&lowest, &mut weights)?;
            steps += wide(threshold) * wide(threshold);
            loop {
                steps += fill(
                    number,
                    holders,
                    &sources,
                    &mut stripes,
                    targets,
                    &sizes,
                    &mut out,
                )?;
                number += 1;
                if number >= blocks {
                    break;
                }
                holders = sweep.next_block();
                let indexes = holders.iter().take(threshold).map(|&(index, _)| index);
                if !indexes.eq(lowest.iter().copied()) {
                    break;
                }
            }
        }
        Ok((out, steps))
    }
}

/// Makes block `number` of every target into its place in `out`, from the holders of the block:
/// the `k` lowest are the sources, and every further one is compared with what they make.
/// Returns the steps taken. `stripes` has room for the sources.
fn fill<'a>(
    number: u64,
    holders: &[(u16, &'a [u8])],
    sources: &Sources<'_>,
    stripes: &mut Vec<&'a [u8]>,
    targets: &[u16],
    sizes: &Sizes,
    out: &mut [u8],
) -> Result<u128, SetError> {
    let (lowest, spare) = holders
        .split_at_checked(sizes.threshold)
        .unwrap_or((holders, &[]));
    stripes.clear();
    stripes.extend(lowest.iter().map(|&(_, bytes)| bytes));
    let mut differing = Vec::new();
    // Blocks compared and blocks made: each costs the same.
    let mut worked = spare.len();
    for &(index, bytes) in spare {
        if !sources.agrees(index, stripes, bytes)? {
            room(&mut differing, 1)?;
            differing.push(index);
        }
    }
    if !differing.is_empty() {
        let mut base = Vec::new();
        room(&mut base, lowest.len())?;
        base.extend(lowest.iter().map(|&(index, _)| index));
        return Err(SetError::Inconsistent {
            block: number,
            base,
            differing,
        });
    }
    let length = stripes.first().map_or(0, |stripe| stripe.len());
    // The block's place inside a shard; the shards of the targets lie one after another.
    let within = usize::try_from(number)
        .ok()
        .and_then(|number| number.checked_mul(sizes.block));
    for (slot, &target) in targets.iter().enumerate() {
        let place = slot
            .checked_mul(sizes.shard)
            .zip(within)
            .and_then(|(shard, block)| shard.checked_add(block))
            .and_then(|start| out.get_mut(start..start.checked_add(length)?))
            .ok_or(SetError::Memory)?;
        // A target that a file holds is copied: it is a source, or it was just compared.
        match holders.binary_search_by_key(&target, |&(index, _)| index) {
            Ok(at) => {
                if let Some(&(_, bytes)) = holders.get(at) {
                    place.copy_from_slice(bytes);
                }
            }
            Err(_) => {
                sources.build(target, stripes, place)?;
                worked += 1;
            }
        }
    }
    let symbols = length / sizes.width;
    Ok(wide(worked) * wide(sizes.threshold) * (wide(symbols) + ALLOWANCE))
}

/// How many shards hold the blocks of a set, summed up from its stretches.
fn cover(facts: &SetFacts, shards: &[Shard<'_>]) -> Result<Coverage, Full> {
    let needed = facts.code().threshold();
    let blocks = facts.blocks();
    let (mut short, mut first_short, mut bare) = (0, None, 0);
    for stretch in Stretches::new(shards, blocks)? {
        let count = stretch.blocks.end - stretch.blocks.start;
        if stretch.holders < needed {
            short += count;
            first_short.get_or_insert(stretch.blocks.start);
        } else if stretch.holders == needed {
            bare += count;
        }
    }
    Ok(Coverage {
        shards: shards.len(),
        needed,
        blocks,
        short,
        first_short,
        bare,
    })
}

/// A count in the type of the steps.
fn wide(count: usize) -> u128 {
    u128::try_from(count).unwrap_or(u128::MAX)
}
