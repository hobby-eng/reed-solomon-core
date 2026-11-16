//! The codes in the strict mode: [`Code`], its [`Sources`], and [`CyclicCode`]. Each call takes
//! the secret only in [`Held`] memory, checks it before anything else, and runs as a strict call.

use super::{
    Held, HeldMut, held, held_mut, held_pairs, held_pairs_mut, one, one_mut, protected, strict,
};
use crate::buffer::Buffer;
use crate::{Changes, CodeError, Correction, Field, Mark, Plan, Redundancy};
use alloc::vec::Vec;

/// [`crate::Code`] in the strict mode: shards only in [`Held`] memory, results and every buffer
/// of the work protected as the mode requires, checked before a byte is written, the work on a
/// protected stack. A refusal is [`CodeError::Unprotected`]; nothing falls back to other memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Code(pub(super) crate::Code);

impl Code {
    /// The code of [`crate::Code::new`].
    pub fn new(field: Field, threshold: usize) -> Result<Self, CodeError> {
        crate::Code::new(field, threshold).map(Self)
    }

    /// The field of the symbols.
    #[must_use]
    pub const fn field(&self) -> Field {
        self.0.field()
    }

    /// How many shards rebuild a set.
    #[must_use]
    pub const fn threshold(&self) -> usize {
        self.0.threshold()
    }

    /// As [`crate::Code::correction_memory`].
    #[must_use]
    pub fn correction_memory(&self, shards: usize) -> usize {
        self.0.correction_memory(shards)
    }

    /// As [`crate::Code::encode`].
    pub fn encode(&self, data: &[&dyn Held], targets: &[u16]) -> Result<Vec<Buffer>, CodeError> {
        let data = held(data)?;
        strict(|| self.0.encode(&data, targets))
    }

    /// As [`crate::Code::rebuild`].
    pub fn rebuild(
        &self,
        shards: &[(u16, &dyn Held)],
        targets: &[u16],
    ) -> Result<Vec<Buffer>, CodeError> {
        let shards = held_pairs(shards)?;
        strict(|| self.0.rebuild(&shards, targets))
    }

    /// As [`crate::Code::redundancy`].
    pub fn redundancy(&self, shards: &[(u16, &dyn Held)]) -> Result<Redundancy, CodeError> {
        let shards = held_pairs(shards)?;
        strict(|| self.0.redundancy(&shards))
    }

    /// As [`crate::Code::correct`], the shards changed in place.
    pub fn correct(&self, shards: &mut [(u16, &mut dyn HeldMut)]) -> Result<Correction, CodeError> {
        let mut shards = held_pairs_mut(shards)?;
        strict(|| self.0.correct(&mut shards))
    }

    /// As [`crate::Code::plan`]: the list lies in protected pages, bound to these shards.
    pub fn plan(&self, shards: &[(u16, &dyn Held)], marks: &[Mark]) -> Result<Plan, CodeError> {
        let shards = held_pairs(shards)?;
        strict(|| self.0.plan(&shards, marks))
    }

    /// As [`crate::Code::apply`]: the shards must be those the plan was made for, and the plan
    /// must lie in protected pages, as a strict plan does.
    pub fn apply(
        &self,
        shards: &mut [(u16, &mut dyn HeldMut)],
        changes: &Changes,
    ) -> Result<(), CodeError> {
        let mut shards = held_pairs_mut(shards)?;
        let changes = protected(changes)?;
        strict(|| self.0.apply(&mut shards, changes))
    }

    /// As [`crate::Code::sources`]: the weights follow from the indexes, which are public.
    pub fn sources<'a>(
        &self,
        indexes: &'a [u16],
        weights: &'a mut [u16],
    ) -> Result<Sources<'a>, CodeError> {
        self.0.sources(indexes, weights).map(Sources)
    }
}

/// [`crate::Sources`] in the strict mode: stripes and outputs only in [`Held`] memory.
#[derive(Debug)]
pub struct Sources<'a>(crate::Sources<'a>);

impl Sources<'_> {
    /// As [`crate::Sources::build`].
    pub fn build(
        &self,
        target: u16,
        stripes: &[&dyn Held],
        out: &mut dyn HeldMut,
    ) -> Result<(), CodeError> {
        let stripes = held(stripes)?;
        let out = one_mut(out)?;
        strict(|| self.0.build(target, &stripes, out))
    }

    /// As [`crate::Sources::agrees`]: the bytes compared lie in a protected buffer.
    pub fn agrees(
        &self,
        target: u16,
        stripes: &[&dyn Held],
        shard: &dyn Held,
    ) -> Result<bool, CodeError> {
        let stripes = held(stripes)?;
        let shard = one(shard)?;
        strict(|| self.0.agrees(target, &stripes, shard))
    }
}

/// [`crate::CyclicCode`] in the strict mode, as [`Code`] is for the code by points. A card of
/// words is one word of it: one shard of one symbol for each word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CyclicCode(crate::CyclicCode);

impl CyclicCode {
    /// The code of [`crate::CyclicCode::new`].
    pub fn new(
        field: Field,
        data: usize,
        parity: usize,
        first_root: u32,
        root_step: u32,
    ) -> Result<Self, CodeError> {
        crate::CyclicCode::new(field, data, parity, first_root, root_step).map(Self)
    }

    /// The field of the symbols.
    #[must_use]
    pub const fn field(&self) -> Field {
        self.0.field()
    }

    /// The number of data shards, which come first.
    #[must_use]
    pub const fn data(&self) -> usize {
        self.0.data()
    }

    /// The number of parity shards, which follow the data.
    #[must_use]
    pub const fn parity(&self) -> usize {
        self.0.parity()
    }

    /// The number of shards of a word: data and parity.
    #[must_use]
    pub const fn length(&self) -> usize {
        self.0.length()
    }

    /// The generator, its coefficients from x^parity, which is 1, down to x^0.
    #[must_use]
    pub fn generator(&self) -> &[u16] {
        self.0.generator()
    }

    /// As [`crate::CyclicCode::encode`].
    pub fn encode(&self, data: &[&dyn Held]) -> Result<Vec<Buffer>, CodeError> {
        let data = held(data)?;
        strict(|| self.0.encode(&data))
    }

    /// As [`crate::CyclicCode::agrees`].
    pub fn agrees(&self, shards: &[&dyn Held]) -> Result<bool, CodeError> {
        let shards = held(shards)?;
        strict(|| self.0.agrees(&shards))
    }

    /// As [`crate::CyclicCode::correct`], the shards changed in place.
    pub fn correct(
        &self,
        shards: &mut [&mut dyn HeldMut],
        lost: &[usize],
    ) -> Result<Correction, CodeError> {
        let mut shards = held_mut(shards)?;
        strict(|| self.0.correct(&mut shards, lost))
    }

    /// As [`crate::CyclicCode::plan`]: the list lies in protected pages, bound to these shards.
    pub fn plan(
        &self,
        shards: &[&dyn Held],
        lost: &[usize],
        marks: &[Mark],
    ) -> Result<Plan, CodeError> {
        let shards = held(shards)?;
        strict(|| self.0.plan(&shards, lost, marks))
    }

    /// As [`crate::CyclicCode::apply`]: the shards must be those the plan was made for, and the
    /// plan must lie in protected pages, as a strict plan does.
    pub fn apply(
        &self,
        shards: &mut [&mut dyn HeldMut],
        changes: &Changes,
    ) -> Result<(), CodeError> {
        let mut shards = held_mut(shards)?;
        let changes = protected(changes)?;
        strict(|| self.0.apply(&mut shards, changes))
    }
}

/// What the change at place `at` of a plan held and becomes, given to `look` on the stack of a
/// strict call, so that a program can move them into protected memory of its own with no copy on
/// its stack; `None` past the last change. The plan must lie in protected pages.
pub fn values<R: Send>(
    changes: &Changes,
    at: usize,
    look: impl FnOnce(u16, u16) -> R + Send,
) -> Result<Option<R>, CodeError> {
    let changes = protected(changes)?;
    strict(|| Ok(changes.values(at, look)))
}
