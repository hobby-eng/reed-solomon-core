//! Writing the shard files of a set from its data.

use super::FileError;
use super::facts::{SetFacts, sha256};
use super::header::Header;
use alloc::vec::Vec;
use zeroize::Zeroizing;

/// Room for `more` further items in `list`, or the refusal of the memory.
fn room<T>(list: &mut Vec<T>, more: usize) -> Result<(), FileError> {
    list.try_reserve_exact(more).map_err(|_| FileError::Memory)
}

impl SetFacts {
    /// The shard files numbered `indexes` for the set whose data is `data`, in the order asked. The data
    /// is cut into the `k` data shards (the last padded with zeros), so files 0 to `k − 1` hold the data
    /// itself. Copies made along the way are wiped, and each file comes in a [`Zeroizing`] buffer. If
    /// the facts state a SHA-256, data with another digest gets no files. Fails on wrong data length or
    /// digest, an index outside the field, or no memory.
    pub fn files(
        &self,
        data: &[u8],
        indexes: &[u16],
    ) -> Result<Vec<Zeroizing<Vec<u8>>>, FileError> {
        if u64::try_from(data.len()) != Ok(self.data_len()) {
            return Err(FileError::Data);
        }
        if self.digest().is_some_and(|stated| *stated != sha256(data)) {
            return Err(FileError::Digest);
        }
        if indexes.is_empty() {
            return Ok(Vec::new());
        }
        let mut headers = Vec::new();
        room(&mut headers, indexes.len())?;
        for &index in indexes {
            headers.push(Header::new(*self, index)?);
        }
        let code = self.code();
        let threshold = code.threshold();
        let shard_len = usize::try_from(self.shard_len()).map_err(|_| FileError::Memory)?;
        // The shards that the data fills are the data itself; the ones it ends in, or never
        // reaches, are copied into zero bytes.
        let whole = data
            .len()
            .checked_div(shard_len)
            .unwrap_or(0)
            .min(threshold);
        let (filled, rest) = data
            .split_at_checked(whole * shard_len)
            .unwrap_or((data, &[]));
        let padded_len = (threshold - whole)
            .checked_mul(shard_len)
            .ok_or(FileError::Memory)?;
        let (mut padded, mut shards) = (Zeroizing::new(Vec::new()), Vec::new());
        room(&mut *padded, padded_len)?;
        padded.extend_from_slice(rest);
        padded.resize(padded_len, 0);
        room(&mut shards, threshold)?;
        if shard_len == 0 {
            shards.extend(core::iter::repeat_n(&[][..], threshold));
        } else {
            let cut = filled.chunks_exact(shard_len);
            shards.extend(cut.chain(padded.chunks_exact(shard_len)));
        }

        // Only a shard beyond the data shards is made by the code, one at a time, from the data
        // shards as its sources. Without one, no sources are prepared.
        let made = headers
            .iter()
            .any(|header| usize::from(header.index()) >= threshold);
        let (mut data_indexes, mut weights) = (Vec::new(), Vec::new());
        let (mut shard, mut files) = (Zeroizing::new(Vec::new()), Vec::new());
        if made {
            room(&mut data_indexes, threshold)?;
            data_indexes.extend((0..=u16::MAX).take(threshold));
            room(&mut weights, threshold)?;
            weights.resize(threshold, 0);
            room(&mut *shard, shard_len)?;
            shard.resize(shard_len, 0);
        }
        // The data shards have one length and every index was checked with its header, so the
        // code has nothing left to refuse but an index.
        let sources = match made {
            true => Some(code.sources(&data_indexes, &mut weights)),
            false => None,
        }
        .transpose()
        .map_err(|_| FileError::Index)?;
        room(&mut files, headers.len())?;
        for header in &headers {
            let file = match (shards.get(usize::from(header.index())), &sources) {
                (Some(data_shard), _) => header.file(data_shard)?,
                (None, Some(sources)) => {
                    sources
                        .build(header.index(), &shards, &mut shard)
                        .map_err(|_| FileError::Index)?;
                    header.file(&shard)?
                }
                (None, None) => return Err(FileError::Index),
            };
            files.push(file);
        }
        Ok(files)
    }
}
