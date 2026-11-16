//! Writing the shard files of a set from its data, into buffers of the crate or outputs of the
//! program, a block at a time.

use super::FileError;
use super::facts::{SetFacts, sha256};
use super::header::Header;
use super::set::{BySlot, Files, Output, Sizes, fill};
use crate::buffer::Buffer;
use crate::code::COMPARED;
use alloc::vec::Vec;

/// Room for `more` further items in `list`, or the refusal of the memory.
fn room<T>(list: &mut Vec<T>, more: usize) -> Result<(), FileError> {
    list.try_reserve_exact(more).map_err(|_| FileError::Memory)
}

impl SetFacts {
    /// The shard files numbered `indexes` for the set whose data is `data`, in the order asked. The data
    /// is cut into the `k` data shards (the last padded with zeros), so files 0 to `k − 1` hold the data
    /// itself. Copies made along the way are wiped, and each file comes in a [`Buffer`]. If the facts
    /// state a SHA-256, data with another digest gets no files. Fails on wrong data length or digest,
    /// an index outside the field, or no memory. [`SetFacts::files_into`] writes into outputs.
    pub fn files(&self, data: &[u8], indexes: &[u16]) -> Result<Vec<Buffer>, FileError> {
        self.check_data(data)?;
        let headers = self.headers(indexes)?;
        let length = usize::try_from(self.file_len()).map_err(|_| FileError::Memory)?;
        let mut files = Vec::new();
        room(&mut files, indexes.len())?;
        for _ in indexes {
            files.push(Buffer::zeros(length).ok_or(FileError::Memory)?);
        }
        let mut outs = Vec::new();
        room(&mut outs, files.len())?;
        outs.extend(files.iter_mut().map(|file| file.as_mut_slice()));
        self.write(data, indexes, &headers, &mut outs)?;
        Ok(files)
    }

    /// The files of [`SetFacts::files`] written into `outs`, one for each index and each exactly
    /// [`SetFacts::file_len`] bytes long. The crate holds a block of each data shard the data ends
    /// in and the block it makes, so `data` and the outputs can be mappings of files. The outputs
    /// are refused first; a refusal of them, the data, its digest or an index leaves them as they
    /// were, and a later one fills them with zeros.
    pub fn files_into(
        &self,
        data: &[u8],
        indexes: &[u16],
        outs: &mut [&mut [u8]],
    ) -> Result<(), FileError> {
        let needed = self.file_len();
        let fits = outs.len() == indexes.len()
            && outs
                .iter()
                .all(|out| u64::try_from(out.len()) == Ok(needed));
        if !fits {
            return Err(FileError::Output { needed });
        }
        self.check_data(data)?;
        let headers = self.headers(indexes)?;
        let written = self.write(data, indexes, &headers, outs);
        if written.is_err() {
            outs.iter_mut().for_each(|out| out.fill(0));
        }
        written
    }

    /// Whether `data` is as long as the facts say and has the digest they state.
    fn check_data(&self, data: &[u8]) -> Result<(), FileError> {
        if u64::try_from(data.len()) != Ok(self.data_len()) {
            return Err(FileError::Data);
        }
        if self.digest().is_some_and(|stated| *stated != sha256(data)) {
            return Err(FileError::Digest);
        }
        Ok(())
    }

    /// The header of each of `indexes`, in their order; an index outside the field is refused.
    fn headers(&self, indexes: &[u16]) -> Result<Vec<Header>, FileError> {
        let mut headers = Vec::new();
        room(&mut headers, indexes.len())?;
        for &index in indexes {
            headers.push(Header::new(*self, index)?);
        }
        Ok(headers)
    }

    /// The files of `indexes` into `outs`, whose lengths, data and headers were checked: the frame
    /// of each, then block by block the data shards' blocks, and the blocks the code makes.
    fn write(
        &self,
        data: &[u8],
        indexes: &[u16],
        headers: &[Header],
        outs: &mut [&mut [u8]],
    ) -> Result<(), FileError> {
        for (header, out) in headers.iter().zip(outs.iter_mut()) {
            header.write_frame(out).ok_or(FileError::Memory)?;
        }
        let mut targets = Vec::new();
        room(&mut targets, indexes.len())?;
        targets.extend_from_slice(indexes);
        targets.sort_unstable();
        targets.dedup();
        let mut slots = Vec::new();
        room(&mut slots, indexes.len())?;
        slots.extend(
            indexes
                .iter()
                .filter_map(|index| targets.binary_search(index).ok()),
        );
        let by_slot = BySlot::new(&slots, targets.len()).map_err(|_| FileError::Memory)?;
        let target_headers = self.headers(&targets)?;
        let mut sink = Files {
            outs,
            headers: &target_headers,
            by_slot: &by_slot,
        };
        let code = self.code();
        let sizes = Sizes {
            threshold: code.threshold(),
            width: code.field().symbol_bytes(),
            shard: usize::try_from(self.shard_len()).map_err(|_| FileError::Memory)?,
            block: self.block_bytes(),
        };
        let mut blocks = DataBlocks::new(data, &sizes)?;
        // A shard beyond the data shards is made from them as sources; for the data shards alone
        // no weights are prepared, and their blocks are handed over as they are.
        let made = targets
            .last()
            .is_some_and(|&last| usize::from(last) >= sizes.threshold);
        let mut weights = Vec::new();
        let mut data_shards = Vec::new();
        if made {
            room(&mut weights, sizes.threshold)?;
            weights.resize(sizes.threshold, 0);
            room(&mut data_shards, sizes.threshold)?;
            data_shards.extend((0..=u16::MAX).take(sizes.threshold));
        }
        let sources = made
            .then(|| code.sources(&data_shards, &mut weights))
            .transpose()
            .map_err(|_| FileError::Index)?;
        let mut scratch = Buffer::zeros(sizes.block.min(sizes.shard)).ok_or(FileError::Memory)?;
        let mut compare = Buffer::zeros(COMPARED).ok_or(FileError::Memory)?;
        let mut out = Output {
            scratch: &mut scratch,
            compare: &mut compare,
            sink: &mut sink,
        };
        for number in 0..self.blocks() {
            let holders = blocks.at(number)?;
            let Some(sources) = &sources else {
                held(number, &holders, &targets, &sizes, &mut out)?;
                continue;
            };
            let mut stripes = Vec::new();
            room(&mut stripes, sizes.threshold)?;
            let made = fill(
                number,
                &holders,
                sources,
                &mut stripes,
                &targets,
                &sizes,
                &mut out,
            );
            made.map_err(|_| FileError::Memory)?;
        }
        Ok(())
    }
}

/// Hands over block `number` of each of `targets`, data shards all, as `holders` hold it.
fn held(
    number: u64,
    holders: &[(u16, &[u8])],
    targets: &[u16],
    sizes: &Sizes,
    out: &mut Output<'_>,
) -> Result<(), FileError> {
    let block = u32::try_from(number).map_err(|_| FileError::Memory)?;
    let start = usize::try_from(number)
        .ok()
        .and_then(|number| number.checked_mul(sizes.block))
        .ok_or(FileError::Memory)?;
    for (slot, &target) in targets.iter().enumerate() {
        let &(_, bytes) = holders.get(usize::from(target)).ok_or(FileError::Index)?;
        out.sink
            .put(slot, block, start, bytes)
            .map_err(|_| FileError::Memory)?;
    }
    Ok(())
}

/// The blocks of the data shards, cut from the data where it holds them whole and copied into
/// zero bytes where it ends in them or never reaches them.
struct DataBlocks<'d> {
    data: &'d [u8],
    threshold: usize,
    shard: usize,
    block: usize,
    /// The first data shard the data does not fill.
    whole: usize,
    /// A block of each shard from `whole` on, one after another.
    tails: Buffer,
}

impl<'d> DataBlocks<'d> {
    fn new(data: &'d [u8], sizes: &Sizes) -> Result<Self, FileError> {
        let block = sizes.block.min(sizes.shard);
        let whole = data
            .len()
            .checked_div(sizes.shard)
            .unwrap_or(0)
            .min(sizes.threshold);
        let length = (sizes.threshold - whole)
            .checked_mul(block)
            .ok_or(FileError::Memory)?;
        Ok(Self {
            data,
            threshold: sizes.threshold,
            shard: sizes.shard,
            block,
            whole,
            tails: Buffer::zeros(length).ok_or(FileError::Memory)?,
        })
    }

    /// Block `number` of every data shard, with its index.
    fn at(&mut self, number: u64) -> Result<Vec<(u16, &[u8])>, FileError> {
        let start = usize::try_from(number)
            .ok()
            .and_then(|number| number.checked_mul(self.block))
            .ok_or(FileError::Memory)?;
        let length = self.block.min(self.shard.saturating_sub(start));
        for (at, tail) in self.tails.chunks_exact_mut(self.block.max(1)).enumerate() {
            let first = (self.whole + at) * self.shard + start;
            let held = self.data.get(first..).unwrap_or_default();
            let held = held.get(..length.min(held.len())).unwrap_or_default();
            let (copy, rest) = tail.split_at_mut(held.len());
            copy.copy_from_slice(held);
            rest.fill(0);
        }
        let mut blocks = Vec::new();
        room(&mut blocks, self.threshold)?;
        for index in 0..self.threshold {
            let bytes = if index < self.whole {
                let first = index * self.shard + start;
                self.data.get(first..first + length)
            } else {
                let at = (index - self.whole) * self.block;
                self.tails.get(at..at + length)
            };
            let index = u16::try_from(index).map_err(|_| FileError::Index)?;
            blocks.push((index, bytes.ok_or(FileError::Memory)?));
        }
        Ok(blocks)
    }
}
