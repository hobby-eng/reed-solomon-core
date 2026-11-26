//! The stack below a call that worked on data, overwritten after it: where the arithmetic, the
//! checksums and the hash left their temporaries, which the caller's own frames would otherwise
//! keep until something writes over them.

use core::cell::Cell;

std::thread_local! {
    /// Whether a call that clears the stack is running on this thread: the calls it makes in turn
    /// leave the clearing to it.
    #[cfg_attr(
        target_os = "android",
        allow(
            clippy::missing_const_for_thread_local,
            reason = "on Android std expands this const initializer into its lazy form, which \
                      Clippy takes for a missing const"
        )
    )]
    static CLEARING: Cell<bool> = const { Cell::new(false) };
}

/// Runs `work`, then overwrites `protect::CLEARED_STACK_BYTES` of the calling thread's stack
/// below the caller's frame, as `protect::with_cleared_stack` does, also when a panic unwinds out
/// of `work` (an aborting one ends the program first); a fixed window, best effort; once,
/// for the outermost such call on the thread. A strict call clears nothing here: the whole stack
/// it runs on is wiped.
pub(crate) fn cleared<T>(work: impl FnOnce() -> T) -> T {
    if crate::strict::required() || CLEARING.with(Cell::get) {
        return work();
    }
    crate::protect::with_cleared_stack(|| {
        let _inside = Inside::enter();
        work()
    })
}

/// Marks the thread as clearing for as long as it lives, through a panic too.
struct Inside;

impl Inside {
    fn enter() -> Self {
        CLEARING.with(|clearing| clearing.set(true));
        Self
    }
}

impl Drop for Inside {
    fn drop(&mut self) {
        CLEARING.with(|clearing| clearing.set(false));
    }
}

#[cfg(test)]
mod tests {
    use super::{CLEARING, cleared};
    use core::cell::Cell;
    #[cfg(target_os = "linux")]
    use core::hint::black_box;

    /// The outermost call marks the thread, the calls inside it leave the mark as it is, and the
    /// mark goes when the outermost call ends, by a panic too.
    #[test]
    fn only_the_outermost_call_clears() {
        assert!(!CLEARING.with(Cell::get));
        let inside = cleared(|| {
            (
                CLEARING.with(Cell::get),
                cleared(|| CLEARING.with(Cell::get)),
            )
        });
        assert_eq!(inside, (true, true));
        assert!(!CLEARING.with(Cell::get));
        let panicked = std::panic::catch_unwind(|| cleared(|| panic!("work that fails")));
        assert!(panicked.is_err());
        assert!(!CLEARING.with(Cell::get));
    }

    /// What the work left on the stack below the caller is gone after the call. Work left
    /// uncleared, with a mark of its own, shows that the reading finds what is there.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_work_leaves_nothing_below_the_caller() {
        /// "RSC_SEE!" and "RSC_STK!", which nothing else on the stack holds.
        const SEEN: u64 = 0x5253_435f_5345_4521;
        const MARK: u64 = 0x5253_435f_5354_4b21;
        crate::protect::clear_stack();
        assert_eq!(work(SEEN), !SEEN);
        assert!(stack_holds(!SEEN));
        assert_eq!(cleared(|| work(MARK)), !MARK);
        assert!(!stack_holds(!MARK));
    }

    /// Fills a frame with `mark` and returns its complement, so that only that frame holds it.
    #[cfg(target_os = "linux")]
    #[inline(never)]
    fn work(mark: u64) -> u64 {
        let mut words = [black_box(mark); 1024];
        black_box(&mut words);
        !words[17]
    }

    /// Whether the cleared window below this frame holds the word whose complement is given,
    /// read through the kernel, as Rust may not read memory no value was written to; the word is
    /// made only after the reading, so that no copy of it lies in the window.
    #[cfg(target_os = "linux")]
    #[inline(never)]
    fn stack_holds(complement: u64) -> bool {
        use std::os::unix::fs::FileExt;
        let frame = 0u64;
        let end = black_box(&raw const frame) as usize;
        let mut below = std::vec![0u8; crate::protect::CLEARED_STACK_BYTES - (16 << 10)];
        let start = (end - below.len()) as u64;
        let memory = std::fs::File::open("/proc/self/mem").unwrap();
        memory.read_exact_at(&mut below, start).unwrap();
        let word = (!black_box(complement)).to_ne_bytes();
        below.windows(word.len()).any(|bytes| bytes == word)
    }
}
