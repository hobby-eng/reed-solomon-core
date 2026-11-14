//! Shard files side by side: which of them read, and which sets they belong to.

use super::FileError;
use super::reading::ShardFile;
use super::set::FileSet;
use alloc::vec::Vec;

/// Any number of files looked at together: each one read, and the ones that read sorted into
/// the sets whose facts they state.
///
/// Files belong to one set when they state the same facts: field, threshold, identifier, data
/// length, block length and digest. Nothing is taken from the order of the files: the sets come
/// in an order of their facts, and their files in the order of their shard indexes.
///
/// The work of looking at a pile grows with the number of files and with the bytes that are in
/// them, whatever their headers say: every block that is there is checked once, files of one
/// shard are compared once, and blocks that no file reaches are counted as one stretch. Every
/// list is asked for before it is filled, and the files are sorted in place.
#[derive(Clone, Debug)]
pub struct Pile<'a> {
    readings: Vec<Result<ShardFile<'a>, FileError>>,
    sets: Vec<FileSet<'a>>,
}

impl<'a> Pile<'a> {
    /// Reads `files`, each the bytes of one file, and sorts them into sets.
    ///
    /// # Errors
    ///
    /// [`FileError::Memory`] when this machine has no memory for a list of the files or for the
    /// map of their blocks. A file that does not read is no refusal of the pile: it is an entry
    /// of [`Pile::readings`].
    pub fn new(files: &[&'a [u8]]) -> Result<Self, FileError> {
        let (mut readings, mut read, mut sets) = (Vec::new(), Vec::new(), Vec::new());
        readings
            .try_reserve_exact(files.len())
            .map_err(|_| FileError::Memory)?;
        readings.extend(files.iter().map(|bytes| ShardFile::read(bytes)));
        let sound = readings.iter().filter(|reading| reading.is_ok()).count();
        read.try_reserve_exact(sound)
            .map_err(|_| FileError::Memory)?;
        let placed = readings.iter().enumerate();
        read.extend(
            placed.filter_map(|(position, reading)| reading.ok().map(|file| (position, file))),
        );
        // Sorted in place, by a key that no two files share: the order of the files given plays
        // no part in the result.
        let key = |&(position, file): &(usize, ShardFile<'a>)| {
            (file.header().set().key(), file.header().index(), position)
        };
        read.sort_unstable_by_key(key);
        for group in read.chunk_by(|one, other| one.1.header().set() == other.1.header().set()) {
            if let Some((_, first)) = group.first() {
                sets.try_reserve(1).map_err(|_| FileError::Memory)?;
                sets.push(FileSet::new(*first.header().set(), group)?);
            }
        }
        // Sets of one identifier are neighbours in the order of the facts.
        for at in 0..sets.len() {
            let id = |at: usize| sets.get(at).map(|set: &FileSet<'a>| set.facts().id());
            let before = at.checked_sub(1).and_then(id);
            let shared = id(at) == before || id(at) == id(at + 1);
            if let Some(set) = sets.get_mut(at).filter(|_| shared) {
                set.share_id();
            }
        }
        Ok(Self { readings, sets })
    }

    /// What became of each file, in the order given: the file as read, or why it does not read.
    #[must_use]
    pub fn readings(&self) -> &[Result<ShardFile<'a>, FileError>] {
        &self.readings
    }

    /// The sets found, in an order of their facts that the order of the files does not change.
    #[must_use]
    pub fn sets(&self) -> &[FileSet<'a>] {
        &self.sets
    }
}
