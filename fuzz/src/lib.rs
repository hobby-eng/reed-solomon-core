//! What the fuzz targets share: the calls that write in place against those into buffers, with the
//! state they must leave their outputs in after a refusal, and damage done to words of a code, with
//! the bound within which the code puts them right.
#![allow(
    clippy::panic,
    reason = "a fuzz target answers by stopping the run: its assertions are its checks"
)]

use reed_solomon_core::{FileSet, Mark, Rebuilt, SetError};

/// What an output given to be written in place holds before a call, so that a check sees whether
/// the call wrote to it.
pub const FILL: u8 = 0xA5;

/// Checks the outputs after a call that writes in place answered `answer`: a refusal before the
/// work leaves every output as it was, one during the work or after it leaves zeros, and one of
/// memory may come at either time.
pub fn left_as_promised<T>(answer: &Result<T, SetError>, outputs: &[&[u8]]) {
    let Err(error) = answer else {
        return;
    };
    let all = |byte: u8| outputs.iter().all(|out| out.iter().all(|&b| b == byte));
    let held = match error {
        SetError::Shards { .. }
        | SetError::Blocks { .. }
        | SetError::Work { .. }
        | SetError::Index
        | SetError::Output { .. } => all(FILL),
        SetError::Inconsistent { .. }
        | SetError::Padding
        | SetError::Digest { .. }
        | SetError::DigestDiffers { .. }
        | SetError::Code(_) => all(0),
        SetError::Memory | SetError::Unprotected => all(FILL) || all(0),
    };
    assert!(held, "{error:?} left its outputs otherwise");
}

/// The bytes of a symbol of a field of `order` elements, `width` bytes long, from two of `next`.
pub fn symbol(next: &mut dyn FnMut() -> usize, order: usize, width: usize) -> Vec<u8> {
    let value = ((next() << 8 | next()) % order) as u16;
    value.to_be_bytes()[2 - width..].to_vec()
}

/// Gives up to seven symbols of `shards`, each `symbols` symbols long, other values, and marks
/// about half of those places unreadable: the marks.
pub fn damage(
    next: &mut dyn FnMut() -> usize,
    shards: &mut [Vec<u8>],
    symbols: usize,
    order: usize,
    width: usize,
) -> Vec<Mark> {
    let mut marks = Vec::new();
    for _ in 0..next() % 8 {
        let (shard, at) = (next() % shards.len(), next() % symbols);
        let value = symbol(next, order, width);
        shards[shard][at * width..(at + 1) * width].copy_from_slice(&value);
        if next().is_multiple_of(2) {
            marks.push(Mark { shard, symbol: at });
        }
    }
    marks
}

/// Whether every column of `read` differs from `whole` in so few places that `spare` symbols put
/// it right, `2e + s ≤ spare`: the marked places count as `s` with `with_marks`, as wrong without.
pub fn within(
    read: &[Vec<u8>],
    whole: &[Vec<u8>],
    marks: &[Mark],
    width: usize,
    spare: usize,
    with_marks: bool,
) -> bool {
    let symbols = whole.first().map_or(0, |shard| shard.len() / width);
    (0..symbols).all(|at| {
        let range = at * width..(at + 1) * width;
        let marked = |shard: usize| with_marks && marks.contains(&Mark { shard, symbol: at });
        let shards = 0..whole.len();
        let wrong = shards
            .clone()
            .filter(|&shard| read[shard][range.clone()] != whole[shard][range.clone()])
            .filter(|&shard| !marked(shard))
            .count();
        2 * wrong + shards.filter(|&shard| marked(shard)).count() <= spare
    })
}

/// The most bytes of an output the targets give: a header may claim any length.
const MOST: u64 = 1 << 16;

/// Rebuilds `set` into a buffer and into an output: both give the same answer, and the output
/// holds what the answer promises. Returns the answer of the buffer.
pub fn rebuild_both(set: &FileSet<'_>, limit: Option<u128>) -> Result<Rebuilt, SetError> {
    let rebuilt = set.rebuild(limit);
    let length = set.facts().data_len();
    if length > MOST {
        return rebuilt;
    }
    let mut out = vec![FILL; length as usize];
    let written = set.rebuild_into(&mut out, limit);
    match (&rebuilt, &written) {
        (Ok(rebuilt), Ok(written)) => {
            assert_eq!(*rebuilt.data, *out);
            let found = (written.digest, written.bare, written.steps);
            assert_eq!((rebuilt.digest, rebuilt.bare, rebuilt.steps), found);
        }
        (Err(one), Err(other)) => assert_eq!(one, other),
        (one, other) => panic!("{one:?} in a buffer, {other:?} in place"),
    }
    left_as_promised(&written, &[&out]);
    rebuilt
}

/// Makes the files of `asked` from `set` into buffers and into outputs: both give the same
/// answer, and the outputs hold what the answer promises.
pub fn repairs_both(set: &FileSet<'_>, asked: &[u16], limit: Option<u128>) {
    let made = set.repairs(asked, limit);
    let length = set.facts().file_len();
    if length > MOST {
        return;
    }
    let mut outs = vec![vec![FILL; length as usize]; asked.len()];
    let mut views: Vec<&mut [u8]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
    let into = set.repairs_into(asked, &mut views, limit);
    match (&made, &into) {
        (Ok(files), Ok(_)) => {
            let files: Vec<Vec<u8>> = files.iter().map(|file| file.to_vec()).collect();
            assert_eq!(files, outs);
        }
        (Err(one), Err(other)) => assert_eq!(one, other),
        (one, other) => panic!("{one:?} in buffers, {other:?} in place"),
    }
    let outs: Vec<&[u8]> = outs.iter().map(Vec::as_slice).collect();
    left_as_promised(&into, &outs);
}
