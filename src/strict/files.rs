//! Shard files and sets in the strict mode: data and shards only in [`Held`] memory, files read
//! from it and written into it, the data of a set put together in protected pages. Memory the
//! crate protects ends at the file: saving a file or the data to a disk is the program's own act.

use super::{Held, HeldMut, held, held_mut, one, one_mut, strict};
use crate::Field;
use crate::buffer::Buffer;
use crate::{
    Block, Coverage, DataDigest, Dispute, FileError, Frame, Member, Rebuilt, SetError, Stretches,
    Survey, Vote, Written,
};
use alloc::vec::Vec;

/// [`crate::SetFacts`] in the strict mode, for writing the files of a set: the data only in
/// [`Held`] memory, the files in buffers protected as the mode requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetFacts(crate::SetFacts);

impl SetFacts {
    /// The facts of [`crate::SetFacts::new`].
    pub fn new(
        field: Field,
        id: &[u8],
        threshold: usize,
        data_len: u64,
        block_len: u32,
        digest: Option<[u8; 32]>,
    ) -> Result<Self, FileError> {
        crate::SetFacts::new(field, id, threshold, data_len, block_len, digest).map(Self)
    }

    /// The facts of [`crate::SetFacts::for_data`], the digest computed on a protected stack.
    pub fn for_data(
        field: Field,
        id: &[u8],
        threshold: usize,
        data: &dyn Held,
        block_len: u32,
        digest: DataDigest,
    ) -> Result<Self, FileError> {
        let data = one(data)?;
        let made = || crate::SetFacts::for_data(field, id, threshold, data, block_len, digest);
        strict(made).map(Self)
    }

    /// The code of the set, in the strict mode.
    #[must_use]
    pub const fn code(&self) -> super::Code {
        super::Code(self.0.code())
    }

    /// As [`crate::SetFacts::id`].
    #[must_use]
    pub fn id(&self) -> &[u8] {
        self.0.id()
    }

    /// As [`crate::SetFacts::data_len`].
    #[must_use]
    pub const fn data_len(&self) -> u64 {
        self.0.data_len()
    }

    /// As [`crate::SetFacts::block_len`].
    #[must_use]
    pub const fn block_len(&self) -> u32 {
        self.0.block_len()
    }

    /// As [`crate::SetFacts::digest`]: the digest the files state, which every header shows.
    #[must_use]
    pub const fn digest(&self) -> Option<&[u8; 32]> {
        self.0.digest()
    }

    /// As [`crate::SetFacts::shard_len`].
    #[must_use]
    pub const fn shard_len(&self) -> u64 {
        self.0.shard_len()
    }

    /// As [`crate::SetFacts::blocks`].
    #[must_use]
    pub const fn blocks(&self) -> u64 {
        self.0.blocks()
    }

    /// As [`crate::SetFacts::file_len`].
    #[must_use]
    pub const fn file_len(&self) -> u64 {
        self.0.file_len()
    }

    /// The same facts for the calls of the root types, which take any memory: an explicit step
    /// out of the strict mode, for public data.
    #[must_use]
    pub const fn as_portable(&self) -> &crate::SetFacts {
        &self.0
    }

    /// As [`crate::SetFacts::files`].
    pub fn files(&self, data: &dyn Held, indexes: &[u16]) -> Result<Vec<Buffer>, FileError> {
        let data = one(data)?;
        strict(|| self.0.files(data, indexes))
    }

    /// As [`crate::SetFacts::files_into`], into [`Held`] memory, not a mapping of a file.
    pub fn files_into(
        &self,
        data: &dyn Held,
        indexes: &[u16],
        outs: &mut [&mut dyn HeldMut],
    ) -> Result<(), FileError> {
        let data = one(data)?;
        let mut outs = held_mut(outs)?;
        strict(|| self.0.files_into(data, indexes, &mut outs))
    }

    /// The header of shard `index`, as [`crate::Header::new`] makes it.
    pub fn header(&self, index: u16) -> Result<Header, FileError> {
        crate::Header::new(self.0, index).map(Header)
    }
}

/// [`crate::Header`] in the strict mode: the file of one shard in [`Held`] memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header(crate::Header);

impl Header {
    /// The facts of the set, in the strict mode.
    #[must_use]
    pub const fn set(&self) -> SetFacts {
        SetFacts(*self.0.set())
    }

    /// As [`crate::Header::index`].
    #[must_use]
    pub const fn index(&self) -> u16 {
        self.0.index()
    }

    /// As [`crate::Header::as_bytes`]: the header's bytes, which are public.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// The same header for the calls of the root types, which take any memory: an explicit step
    /// out of the strict mode, for public data.
    #[must_use]
    pub const fn as_portable(&self) -> &crate::Header {
        &self.0
    }

    /// As [`crate::Header::file`].
    pub fn file(&self, shard: &dyn Held) -> Result<Buffer, FileError> {
        let shard = one(shard)?;
        strict(|| self.0.file(shard))
    }

    /// As [`crate::Header::file_into`], into [`Held`] memory.
    pub fn file_into(&self, shard: &dyn Held, out: &mut dyn HeldMut) -> Result<(), FileError> {
        let shard = one(shard)?;
        let out = one_mut(out)?;
        strict(|| self.0.file_into(shard, out))
    }
}

/// [`crate::ShardFile::read`] of a file in [`Held`] memory: what it gives refers to that memory.
pub fn read(file: &dyn Held) -> Result<ShardFile<'_>, FileError> {
    let bytes = one(file)?;
    strict(|| crate::ShardFile::read(bytes)).map(ShardFile)
}

/// [`crate::ShardFile`] in the strict mode: a file read from [`Held`] memory, whose blocks are
/// checked on a protected stack as well, each time they are asked for.
#[derive(Clone, Copy, Debug)]
pub struct ShardFile<'a>(crate::ShardFile<'a>);

impl<'a> ShardFile<'a> {
    /// As [`crate::ShardFile::header`], in the strict mode.
    #[must_use]
    pub const fn header(&self) -> Header {
        Header(*self.0.header())
    }

    /// As [`crate::ShardFile::frame`], which looks at no shard byte.
    #[must_use]
    pub const fn frame(&self) -> Frame {
        self.0.frame()
    }

    /// As [`crate::ShardFile::block`], its checksum computed on a protected stack.
    pub fn block(&self, number: u32) -> Result<Option<Block<'a>>, FileError> {
        strict(|| Ok(self.0.block(number)))
    }

    /// As [`crate::ShardFile::survey`], on a protected stack.
    pub fn survey(&self) -> Result<Survey, FileError> {
        strict(|| Ok(self.0.survey()))
    }
}

/// [`crate::Pile`] in the strict mode: files only in [`Held`] memory, read on a protected stack,
/// sorted into sets that work in the strict mode.
#[derive(Clone, Debug)]
pub struct Pile<'a> {
    readings: Vec<Result<ShardFile<'a>, FileError>>,
    sets: Vec<FileSet<'a>>,
}

impl<'a> Pile<'a> {
    /// As [`crate::Pile::new`].
    pub fn new(files: &[&'a dyn Held]) -> Result<Self, FileError> {
        let files = held(files)?;
        let (open_readings, open) = strict(|| crate::Pile::new(&files))?.into_parts();
        let (mut readings, mut sets) = (Vec::new(), Vec::new());
        readings
            .try_reserve_exact(open_readings.len())
            .map_err(|_| FileError::Memory)?;
        sets.try_reserve_exact(open.len())
            .map_err(|_| FileError::Memory)?;
        readings.extend(open_readings.into_iter().map(|read| read.map(ShardFile)));
        sets.extend(open.into_iter().map(FileSet));
        Ok(Self { readings, sets })
    }

    /// As [`crate::Pile::readings`], each file a strict reading.
    #[must_use]
    pub fn readings(&self) -> &[Result<ShardFile<'a>, FileError>] {
        &self.readings
    }

    /// The sets, as [`crate::Pile::sets`] has them.
    #[must_use]
    pub fn sets(&self) -> &[FileSet<'a>] {
        &self.sets
    }
}

/// [`crate::FileSet`] in the strict mode: the data and the files it makes in buffers protected as
/// the mode requires, or in [`Held`] memory the program gives, never in a mapping of a file.
#[derive(Clone, Debug)]
pub struct FileSet<'a>(crate::FileSet<'a>);

impl<'a> FileSet<'a> {
    /// As [`crate::FileSet::facts`], in the strict mode.
    #[must_use]
    pub const fn facts(&self) -> SetFacts {
        SetFacts(*self.0.facts())
    }

    /// As [`crate::FileSet::id_shared`].
    #[must_use]
    pub const fn id_shared(&self) -> bool {
        self.0.id_shared()
    }

    /// As [`crate::FileSet::members`].
    #[must_use]
    pub fn members(&self) -> &[Member] {
        self.0.members()
    }

    /// As [`crate::FileSet::disputes`].
    #[must_use]
    pub fn disputes(&self) -> &[Dispute] {
        self.0.disputes()
    }

    /// As [`crate::FileSet::coverage`].
    #[must_use]
    pub const fn coverage(&self) -> Coverage {
        self.0.coverage()
    }

    /// As [`crate::FileSet::stretches`], which reads no data.
    pub fn stretches(&self) -> Result<Stretches<'_, 'a>, FileError> {
        self.0.stretches()
    }

    /// As [`crate::FileSet::disputed_blocks`].
    pub fn disputed_blocks(&self, index: u16) -> impl Iterator<Item = u64> + '_ {
        self.0.disputed_blocks(index)
    }

    /// As [`crate::FileSet::rebuild_work`].
    pub fn rebuild_work(&self) -> Result<u128, SetError> {
        self.0.rebuild_work()
    }

    /// As [`crate::FileSet::repair_work`].
    pub fn repair_work(&self, index: u16) -> Result<u128, SetError> {
        self.0.repair_work(index)
    }

    /// As [`crate::FileSet::repairs_work`].
    pub fn repairs_work(&self, indexes: &[u16]) -> Result<u128, SetError> {
        self.0.repairs_work(indexes)
    }

    /// As [`crate::FileSet::vote_work`].
    pub fn vote_work(&self) -> Result<u128, SetError> {
        self.0.vote_work()
    }

    /// As [`crate::FileSet::rebuild`]: the data in a protected buffer. Data of another digest is
    /// [`SetError::DigestDiffers`], which does not give the digest the data has.
    pub fn rebuild(&self, limit: Option<u128>) -> Result<Rebuilt, SetError> {
        strict(|| self.0.rebuild(limit).map_err(unseen))
    }

    /// As [`crate::FileSet::rebuild_into`], into [`Held`] memory.
    pub fn rebuild_into(
        &self,
        out: &mut dyn HeldMut,
        limit: Option<u128>,
    ) -> Result<Written, SetError> {
        let out = one_mut(out)?;
        strict(|| self.0.rebuild_into(out, limit).map_err(unseen))
    }

    /// As [`crate::FileSet::repair`].
    pub fn repair(&self, index: u16, limit: Option<u128>) -> Result<Buffer, SetError> {
        strict(|| self.0.repair(index, limit))
    }

    /// As [`crate::FileSet::repairs`].
    pub fn repairs(&self, indexes: &[u16], limit: Option<u128>) -> Result<Vec<Buffer>, SetError> {
        strict(|| self.0.repairs(indexes, limit))
    }

    /// As [`crate::FileSet::repairs_into`], into [`Held`] memory.
    pub fn repairs_into(
        &self,
        indexes: &[u16],
        outs: &mut [&mut dyn HeldMut],
        limit: Option<u128>,
    ) -> Result<u128, SetError> {
        let mut outs = held_mut(outs)?;
        strict(|| self.0.repairs_into(indexes, &mut outs, limit))
    }

    /// As [`crate::FileSet::vote`], whose blocks lie in protected pages.
    pub fn vote(&self, limit: Option<u128>) -> Result<Vote, SetError> {
        strict(|| self.0.vote(limit))
    }

    /// As [`crate::FileSet::without`], on a protected stack: the files left of one shard are
    /// compared anew.
    pub fn without(&self, files: &[usize]) -> Result<Self, FileError> {
        strict(|| self.0.without(files)).map(Self)
    }
}

/// `error` with no digest of the data in it: that follows from the data.
fn unseen(error: SetError) -> SetError {
    match error {
        SetError::Digest { stated, .. } => SetError::DigestDiffers { stated },
        other => other,
    }
}
