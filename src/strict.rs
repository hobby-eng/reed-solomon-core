//! The strict mode, for a secret in the open such as the words of a recovery phrase: its calls
//! take and give the secret only in memory whose protection the crate can check, and refuse with
//! an `Unprotected` error before the first secret byte is written where the system does not
//! protect it. The protected-memory example in docs/API.md repairs a card; docs/TECHNICAL.md says what is covered.

mod codes;
mod files;

pub use codes::{Code, CyclicCode, Sources, values};
pub use files::{FileSet, Header, Pile, SetFacts, ShardFile, read};

use crate::Changes;
use crate::buffer::{Buffer, Shortfall, guarded};
use crate::protect::{Secret, WorkArea};
use alloc::vec::Vec;
use core::cell::Cell;

std::thread_local! {
    /// Whether the call running on this thread is strict.
    #[cfg_attr(
        target_os = "android",
        allow(
            clippy::missing_const_for_thread_local,
            reason = "on Android std expands this const initializer into its lazy form, which \
                      Clippy takes for a missing const"
        )
    )]
    static STRICT: Cell<bool> = const { Cell::new(false) };
}

/// Whether the call running on this thread is strict: then every buffer the crate makes must be
/// protected as that mode requires.
pub(crate) fn required() -> bool {
    STRICT.with(Cell::get)
}

/// Strict for as long as it lives, on this thread; it gives back what was there before.
#[cfg(unix)]
struct Scope(bool);

#[cfg(unix)]
impl Scope {
    fn enter() -> Self {
        Self(STRICT.with(|strict| strict.replace(true)))
    }
}

#[cfg(unix)]
impl Drop for Scope {
    fn drop(&mut self) {
        let before = self.0;
        STRICT.with(|strict| strict.set(before));
    }
}

/// Bytes of the protected stack a strict call runs on.
#[cfg(unix)]
const STACK: usize = 512 * 1024;

/// Runs `work` as a strict call, on a stack of its own that the system locks and keeps out of
/// child processes, and out of dumps where it can, wiped afterwards, so that the arithmetic's and
/// the hash's temporaries never lie in pages the system may swap. Where the system gives no such
/// stack, as Windows and WebAssembly give none, the call refuses.
pub(crate) fn run<R: Send>(work: impl FnOnce() -> R + Send) -> Result<R, Shortfall> {
    #[cfg(unix)]
    {
        use crate::protect::{StackError, StackProperties, run_on_protected_stack};
        let required = StackProperties {
            locked: true,
            out_of_dumps: !cfg!(target_vendor = "apple"),
            out_of_children: true,
        };
        let strict = || {
            let _scope = Scope::enter();
            work()
        };
        match run_on_protected_stack(STACK, required, strict) {
            Ok(run) => Ok(run.value),
            Err(StackError::System(_) | StackError::Size) => Err(Shortfall::Memory),
            Err(_) => Err(Shortfall::Unprotected),
        }
    }
    #[cfg(not(unix))]
    {
        drop(work);
        Err(Shortfall::Unprotected)
    }
}

mod sealed {
    /// Only the crate's own and the protection crate's memory is [`super::Held`].
    pub trait Sealed {}
}

/// Memory whose protection the crate can check, which a strict call takes a secret in: a
/// [`Buffer`] of this crate, and a `Secret` or a `WorkArea` of the protection crate. No other
/// type can be, so ordinary memory such as a `Vec` given to a strict call does not compile.
pub trait Held: sealed::Sealed + Sync {
    /// The bytes.
    fn held(&self) -> &[u8];

    /// Whether the system protects the pages as the strict mode requires on this system: locked,
    /// out of child processes and, where it keeps single pages out of dumps, out of dumps.
    fn guarded(&self) -> bool;
}

/// [`Held`] memory a strict call can write a secret into.
pub trait HeldMut: Held + Send {
    /// The bytes, to change in place.
    fn held_mut(&mut self) -> &mut [u8];
}

/// A `Vec` given to a strict call does not compile; the item is there for this doctest alone.
#[cfg(doctest)]
#[doc = "```compile_fail,E0277
let code = reed_solomon_core::strict::Code::new(reed_solomon_core::Field::Gf256, 2).unwrap();
let _ = code.encode(&[&vec![1_u8], &vec![2_u8]], &[2]);
```"]
pub struct NoVec;

/// The header of a strict reading writes a file only into memory the crate can check: a `Vec` as
/// the output does not compile. The item is there for this doctest alone.
#[cfg(doctest)]
#[doc = "```compile_fail,E0277
let file = reed_solomon_core::protect::Secret::zeroed(1);
let header = reed_solomon_core::strict::read(&file).unwrap().header();
let _ = header.file_into(&file, &mut vec![0_u8; 128]);
```"]
pub struct NoVecFile;

/// The facts of a strict set write files only from memory the crate can check: a slice as the
/// data does not compile. The item is there for this doctest alone.
#[cfg(doctest)]
#[doc = "```compile_fail,E0277
let pile = reed_solomon_core::strict::Pile::new(&[]).unwrap();
let _ = pile.sets()[0].facts().files(&[0_u8], &[0]);
```"]
pub struct NoSliceData;

impl sealed::Sealed for Buffer {}

impl Held for Buffer {
    fn held(&self) -> &[u8] {
        self.as_slice()
    }

    fn guarded(&self) -> bool {
        Self::guarded(self)
    }
}

impl HeldMut for Buffer {
    fn held_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl sealed::Sealed for Secret {}

impl Held for Secret {
    fn held(&self) -> &[u8] {
        self.as_bytes()
    }

    fn guarded(&self) -> bool {
        guarded(
            self.is_locked(),
            self.is_out_of_dumps(),
            self.is_out_of_children(),
        )
    }
}

impl HeldMut for Secret {
    fn held_mut(&mut self) -> &mut [u8] {
        self.as_mut_bytes()
    }
}

impl sealed::Sealed for WorkArea {}

impl Held for WorkArea {
    fn held(&self) -> &[u8] {
        self.as_slice()
    }

    fn guarded(&self) -> bool {
        guarded(
            self.is_locked(),
            self.is_out_of_dumps(),
            self.is_out_of_children(),
        )
    }
}

impl HeldMut for WorkArea {
    fn held_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

/// Runs `work` as a strict call, refusing as [`run`] does in the call's own error.
fn strict<T: Send, E: From<Shortfall> + Send>(
    work: impl FnOnce() -> Result<T, E> + Send,
) -> Result<T, E> {
    run(work).map_err(E::from)?
}

/// Refuses unless every one of `guards` is protected as the strict mode requires.
fn all_guarded(mut guards: impl Iterator<Item = bool>) -> Result<(), Shortfall> {
    if guards.all(|guarded| guarded) {
        Ok(())
    } else {
        Err(Shortfall::Unprotected)
    }
}

/// An empty list with room for `count` elements.
fn listed<T>(count: usize) -> Result<Vec<T>, Shortfall> {
    let mut list = Vec::new();
    list.try_reserve_exact(count)
        .map_err(|_| Shortfall::Memory)?;
    Ok(list)
}

/// The bytes of `inputs`, once every one is protected as the strict mode requires.
fn held<'a>(inputs: &[&'a dyn Held]) -> Result<Vec<&'a [u8]>, Shortfall> {
    all_guarded(inputs.iter().map(|input| input.guarded()))?;
    let mut bytes = listed(inputs.len())?;
    bytes.extend(inputs.iter().map(|input| input.held()));
    Ok(bytes)
}

/// The indexes and bytes of `shards`, once every one is protected.
fn held_pairs<'a>(shards: &[(u16, &'a dyn Held)]) -> Result<Vec<(u16, &'a [u8])>, Shortfall> {
    all_guarded(shards.iter().map(|(_, shard)| shard.guarded()))?;
    let mut bytes = listed(shards.len())?;
    bytes.extend(shards.iter().map(|&(index, shard)| (index, shard.held())));
    Ok(bytes)
}

/// The bytes of `outs`, to write into, once every one is protected.
fn held_mut<'a>(outs: &'a mut [&mut dyn HeldMut]) -> Result<Vec<&'a mut [u8]>, Shortfall> {
    all_guarded(outs.iter().map(|out| out.guarded()))?;
    let mut bytes = listed(outs.len())?;
    bytes.extend(outs.iter_mut().map(|out| out.held_mut()));
    Ok(bytes)
}

/// The indexes and bytes of `shards`, to change in place, once every one is protected.
fn held_pairs_mut<'a>(
    shards: &'a mut [(u16, &mut dyn HeldMut)],
) -> Result<Vec<(u16, &'a mut [u8])>, Shortfall> {
    all_guarded(shards.iter().map(|(_, shard)| shard.guarded()))?;
    let mut bytes = listed(shards.len())?;
    bytes.extend(
        shards
            .iter_mut()
            .map(|(index, shard)| (*index, shard.held_mut())),
    );
    Ok(bytes)
}

/// The bytes of `input`, once it is protected.
fn one(input: &dyn Held) -> Result<&[u8], Shortfall> {
    all_guarded([input.guarded()].into_iter())?;
    Ok(input.held())
}

/// The bytes of `out`, to write into, once it is protected.
fn one_mut(out: &mut dyn HeldMut) -> Result<&mut [u8], Shortfall> {
    all_guarded([out.guarded()].into_iter())?;
    Ok(out.held_mut())
}

/// The list of a plan, once it is protected: what it read and writes is the secret too.
fn protected(changes: &Changes) -> Result<&Changes, Shortfall> {
    all_guarded([changes.guarded()].into_iter())?;
    Ok(changes)
}
