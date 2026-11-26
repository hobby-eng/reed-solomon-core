//! The cyclic code in memory the caller gives, without an allocator: its generator in the caller's
//! slice, the check shards and the check of stripes of shards, and one word put right in place.

use super::decode::{Word, Work};
use crate::code::{CodeError, in_field, one_length};
use crate::field::Field;
use core::ops::Deref;
use zeroize::Zeroize;

/// The cyclic code over memory the caller gives, without an allocator: the code of `CyclicCode`,
/// its generator in a slice of the caller's, with the check shards and the check of stripes of
/// shards, and one word at a time put right in place. A card of words is one word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CyclicWords<'g> {
    word: Word,
    generator: &'g [u16],
}

impl<'g> CyclicWords<'g> {
    /// The code of `data` shards and `parity` more over `field`, with the roots β^(first_root + i)
    /// for i below `parity` and β = α^root_step, as `CyclicCode::new` makes it, its generator
    /// written into `generator` of exactly `parity + 1` elements. Fails with [`CodeError::Shape`]
    /// for a shape that makes no code, then [`CodeError::Length`], before anything is written.
    pub fn new(
        field: Field,
        data: usize,
        parity: usize,
        first_root: u32,
        root_step: u32,
        generator: &'g mut [u16],
    ) -> Result<Self, CodeError> {
        let word = Word::new(field, data, parity, first_root, root_step)?;
        word.generator_into(generator)?;
        Ok(Self { word, generator })
    }

    /// The code whose generator `generator` was written for `word`.
    #[cfg(feature = "alloc")]
    pub(super) const fn of(word: Word, generator: &'g [u16]) -> Self {
        Self { word, generator }
    }

    /// The field of the symbols.
    #[must_use]
    pub const fn field(&self) -> Field {
        self.word.field
    }

    /// The number of data shards, which come first.
    #[must_use]
    pub const fn data(&self) -> usize {
        self.word.length - self.word.parity
    }

    /// The number of parity shards, which follow the data.
    #[must_use]
    pub const fn parity(&self) -> usize {
        self.word.parity
    }

    /// The number of shards of a word: data and parity.
    #[must_use]
    pub const fn length(&self) -> usize {
        self.word.length
    }

    /// The generator, its coefficients from x^parity, which is 1, down to x^0.
    #[must_use]
    pub const fn generator(&self) -> &[u16] {
        self.generator
    }

    /// The elements of working room [`CyclicWords::correct_word`] takes for `parity` parity shards.
    #[must_use]
    pub const fn room(parity: usize) -> usize {
        Work::room(parity)
    }

    /// The parity shards of `data` written into `parity`: `data` shards and `parity` outputs, all of
    /// one length, a whole number of symbols of the field; `remainder` is room of exactly `parity`
    /// elements. Counts, lengths, then symbols are refused, before anything is written.
    pub fn encode(
        &self,
        data: &[&[u8]],
        parity: &mut [&mut [u8]],
        remainder: &mut [u16],
    ) -> Result<(), CodeError> {
        crate::cleared(|| {
            if data.len() != self.data() || parity.len() != self.parity() {
                return Err(CodeError::Count);
            }
            let length = self.measured(data, self.data())?;
            let outs = parity.iter().all(|out| out.len() == length);
            if !outs || remainder.len() != self.parity() {
                return Err(CodeError::Length);
            }
            self.canonical(data, &[])?;
            self.encode_checked(data, parity, remainder);
            Ok(())
        })
    }

    /// The parity of `data` into `parity`, whose counts, lengths and symbols were checked.
    pub(super) fn encode_checked<S: core::ops::DerefMut<Target = [u8]>>(
        &self,
        data: &[&[u8]],
        parity: &mut [S],
        remainder: &mut [u16],
    ) {
        parity.iter_mut().for_each(|out| out.fill(0));
        // x^(parity + t) mod the generator, for t = 0, 1, …: the parity a 1 makes at the data
        // place data − 1 − t. Its coefficients follow from the generator, which is public.
        remainder.copy_from_slice(self.generator.get(1..).unwrap_or_default());
        for source in data.iter().rev() {
            for (shard, &factor) in parity.iter_mut().zip(remainder.iter()) {
                self.field().add_scaled(shard, source, factor);
            }
            self.times_x(remainder);
        }
    }

    /// Whether every word of `shards`, all `length` of them in their order, is a word of the code.
    /// `syndromes` is room for the syndromes of a stripe of words, at least `parity` symbols, and
    /// the more room, the fewer stripes; it is wiped before the call returns, unless the call is
    /// refused. It stops at the first stripe with a word that is not of the code. Fails as
    /// [`CyclicWords::encode`] does, and with [`CodeError::Length`] for too little room.
    pub fn agrees(&self, shards: &[&[u8]], syndromes: &mut [u8]) -> Result<bool, CodeError> {
        crate::cleared(|| {
            let length = self.measured(shards, self.length())?;
            let symbol = self.field().symbol_bytes();
            let width = syndromes.len() / (self.parity() * symbol);
            if width == 0 {
                return Err(CodeError::Length);
            }
            self.canonical(shards, &[])?;
            let columns = length / symbol;
            let mut agrees = true;
            for start in (0..columns).step_by(width) {
                let span = ((start + width).min(columns) - start) * symbol;
                let room = syndromes
                    .get_mut(..self.parity() * span)
                    .unwrap_or_default();
                self.word.stripe(shards, &[], start * symbol, room, span);
                if room.iter().any(|&byte| byte != 0) {
                    agrees = false;
                    break;
                }
            }
            syndromes.zeroize();
            Ok(agrees)
        })
    }

    /// Puts one word right in place: `word` holds its `length` symbols, and those at the places
    /// `erased`, in rising order, are unreadable and never read. Returns how many places it wrote,
    /// or `None`, the word unchanged, when no word of the code lies within `2e + s ≤ parity`.
    /// `room` holds at least [`CyclicWords::room`] elements and is wiped before the call returns.
    /// Lengths, then marks, then symbols are refused, before anything is written, the room too.
    pub fn correct_word(
        &self,
        word: &mut [u16],
        erased: &[usize],
        room: &mut [u16],
    ) -> Result<Option<usize>, CodeError> {
        crate::cleared(|| {
            if word.len() != self.length() || room.len() < Self::room(self.parity()) {
                return Err(CodeError::Length);
            }
            let rising = erased.windows(2).all(|pair| pair.first() < pair.last());
            if !rising || erased.last().is_some_and(|&last| last >= self.length()) {
                return Err(CodeError::Mark);
            }
            let read = word
                .iter()
                .enumerate()
                .filter(|(place, _)| erased.binary_search(place).is_err());
            let order = self.field().order();
            if read.fold(false, |outside, (_, &symbol)| {
                outside | (usize::from(symbol) >= order)
            }) {
                return Err(CodeError::Symbol);
            }
            let answer = self.put_right(word, erased, room);
            room.zeroize();
            answer
        })
    }

    /// The search of `correct_word` once its word and marks are checked.
    fn put_right(
        &self,
        word: &mut [u16],
        erased: &[usize],
        room: &mut [u16],
    ) -> Result<Option<usize>, CodeError> {
        let mut work = Work::new(self.parity(), room).ok_or(CodeError::Length)?;
        self.word.syndromes_of(word, erased, &mut work);
        let clean = work
            .syndromes
            .as_slice()
            .iter()
            .all(|&syndrome| syndrome == 0);
        if erased.is_empty() && clean {
            return Ok(Some(0));
        }
        if !self.word.errata(&mut work, erased) {
            return Ok(None);
        }
        let mut written = 0;
        for (place, &value) in work.places().zip(work.values.as_slice()) {
            if let Some(symbol) = word.get_mut(place) {
                // A marked symbol is the value itself; a wrong one has the value added.
                let marked = erased.binary_search(&place).is_ok();
                *symbol = if marked { value } else { *symbol ^ value };
                written += 1;
            }
        }
        Ok(Some(written))
    }

    /// The length in bytes of `shards`, which must be `count` shards of one length, a whole
    /// number of symbols.
    pub(super) fn measured<S: Deref<Target = [u8]>>(
        &self,
        shards: &[S],
        count: usize,
    ) -> Result<usize, CodeError> {
        if shards.len() != count {
            return Err(CodeError::Count);
        }
        one_length(self.field(), shards.iter().map(|shard| shard.len()))
    }

    /// Whether every symbol of `shards` but those at the places `lost`, in rising order, is an
    /// element of the field; every such shard is looked at.
    pub(super) fn canonical<S: Deref<Target = [u8]>>(
        &self,
        shards: &[S],
        lost: &[usize],
    ) -> Result<(), CodeError> {
        let read = shards.iter().enumerate();
        let read = read.filter(|(place, _)| lost.binary_search(place).is_err());
        in_field(self.field(), read.map(|(_, shard)| &**shard))
    }

    /// `remainder` times x, modulo the generator: a shift up, and the generator's tail times what
    /// left the top.
    fn times_x(&self, remainder: &mut [u16]) {
        let top = remainder.first().copied().unwrap_or(0);
        remainder.rotate_left(1);
        if let Some(last) = remainder.last_mut() {
            *last = 0;
        }
        for (slot, &g) in remainder.iter_mut().zip(self.generator.iter().skip(1)) {
            *slot ^= self.field().multiply(top, g);
        }
    }
}
