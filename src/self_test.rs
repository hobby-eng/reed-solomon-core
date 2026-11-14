//! Known answers of the code, for a program to run before it relies on the crate: a program that
//! starts its work only after [`self_test`] passed knows that the build it runs codes as the
//! references do. Every expected value comes from outside this crate, and each check names its
//! source.

use crate::code::Code;
use crate::field::Field;

/// One check: what was checked against which source, and whether the answer was the expected one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Check {
    /// What was checked and where the expected answer comes from.
    pub name: &'static str,
    /// Whether the answer was the expected one.
    pub passed: bool,
}

/// The most checks a run holds.
const MOST: usize = 14;

/// The checks of one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfTest {
    checks: [Check; MOST],
    count: usize,
}

impl SelfTest {
    /// Whether every check passed. A program relies on the crate only then.
    #[must_use]
    pub fn passed(&self) -> bool {
        self.checks().iter().all(|check| check.passed)
    }

    /// Every check, in the order run.
    #[must_use]
    pub fn checks(&self) -> &[Check] {
        self.checks.get(..self.count).unwrap_or_default()
    }

    fn add(&mut self, name: &'static str, passed: bool) {
        if let Some(check) = self.checks.get_mut(self.count) {
            *check = Check { name, passed };
            self.count += 1;
        }
    }
}

/// Runs the known answers: a few field multiplications' and hashes' worth of work. Without the
/// `alloc` feature it allocates nothing and leaves out the three checks of the error correction,
/// the plan of changes for a card, the check of the vote among the files of a set and the check
/// of a digest that differs.
#[must_use]
pub fn self_test() -> SelfTest {
    let empty = Check {
        name: "",
        passed: false,
    };
    let mut run = SelfTest {
        checks: [empty; MOST],
        count: 0,
    };
    run.add(
        "GF(2^8), 0x11D: the data 01 02 03 04 gives the shards 45 5E (reed-solomon-erasure, klauspost)",
        published_parity(),
    );
    run.add(
        "A set grows: the data 46 91 23 gives F4 F3, and the shards 1, 3, 4 give 24 96 (interpolation by hand)",
        growth(),
    );
    run.add(
        "GF(2^16): the data 01 00, 00 00 gives shard 256 as 03 80 (galois_16 of reed-solomon-erasure)",
        pair_field(),
    );
    run.add(
        "GF(2^11), 0x805: the words 001 7FF 400 give the words 3FE and 7E3 (computed apart)",
        word_field(),
    );
    run.add(
        "A changed shard is told from the one its set makes",
        changed_shard(),
    );
    run.add(
        "CRC-32C of \"123456789\" is E3069283 (the published check value)",
        crate::file::checksum_known_answer(),
    );
    run.add(
        "A shard file of 86 bytes has the checksums CD62C865, B3561B91 and 62E47E4A (computed apart), and a changed block is refused",
        crate::file::file_known_answer(),
    );
    run.add(
        "SHA-256 of \"abc\" is BA7816BF…F20015AD (the example of FIPS 180-4)",
        crate::file::digest_known_answer(),
    );
    #[cfg(feature = "alloc")]
    {
        run.add(
            "One wrong symbol of five and two of seven are found and put right (words computed apart)",
            correction::within_the_bound(),
        );
        run.add(
            "Two wrong words of seven are found and put right in GF(2^11) (word computed apart)",
            correction::wrong_words(),
        );
        run.add(
            "Three wrong symbols of seven with no code word within two are given up, nothing changed",
            correction::beyond_the_bound(),
        );
        run.add(
            "A card of 12 + 4 words with two marked gaps and one wrong word gets a plan that puts it right (words computed apart)",
            correction::card_plan(),
        );
        run.add(
            "Of five files of a set of three, the one that holds other bytes is named after 945 steps (computed apart)",
            crate::file::vote_known_answer(),
        );
        run.add(
            "Files that agree on \"abc\" and state the SHA-256 of \"abd\" are refused; stating their own, they rebuild",
            crate::file::digest_refusal_known_answer(),
        );
    }
    run
}

/// Shard `target` of the set whose shards `indexes` are `sources`, by the stripe interface.
fn made<const K: usize, const S: usize>(
    field: Field,
    indexes: [u16; K],
    sources: [[u8; S]; K],
    target: u16,
) -> Option<[u8; S]> {
    let code = Code::new(field, K).ok()?;
    let mut weights = [0; K];
    let set = code.sources(&indexes, &mut weights).ok()?;
    let stripes = sources.each_ref().map(<[u8; S]>::as_slice);
    let mut out = [0; S];
    set.build(target, &stripes, &mut out).ok()?;
    Some(out)
}

fn published_parity() -> bool {
    let data = [[1], [2], [3], [4]];
    made(Field::Gf256, [0, 1, 2, 3], data, 4) == Some([0x45])
        && made(Field::Gf256, [0, 1, 2, 3], data, 5) == Some([0x5E])
}

fn growth() -> bool {
    let data = [[0x46], [0x91], [0x23]];
    let later = [[0x91], [0xF4], [0xF3]];
    made(Field::Gf256, [0, 1, 2], data, 3) == Some([0xF4])
        && made(Field::Gf256, [0, 1, 2], data, 4) == Some([0xF3])
        && made(Field::Gf256, [1, 3, 4], later, 5) == Some([0x24])
        && made(Field::Gf256, [1, 3, 4], later, 6) == Some([0x96])
        && made(Field::Gf256, [1, 3, 4], later, 0) == Some([0x46])
}

/// With the data `a` at point 0 and zero at point 1, the shard at point `t` is `(t + 1) · a`.
/// For `t = a = 01 00` that is `01 00` squared, which is `02 80` in this field, plus `01 00`.
fn pair_field() -> bool {
    made(Field::Gf65536, [0, 1], [[1, 0], [0, 0]], 256) == Some([0x03, 0x80])
}

fn word_field() -> bool {
    let data = [[0x00, 0x01], [0x07, 0xFF], [0x04, 0x00]];
    made(Field::Gf2048, [0, 1, 2], data, 3) == Some([0x03, 0xFE])
        && made(Field::Gf2048, [0, 1, 2], data, 4) == Some([0x07, 0xE3])
        // A symbol of 2,048 is not a word.
        && made(Field::Gf2048, [0, 1, 2], [[0x08, 0x00], [0, 0], [0, 0]], 3).is_none()
}

fn changed_shard() -> bool {
    let Ok(code) = Code::new(Field::Gf256, 4) else {
        return false;
    };
    let mut weights = [0; 4];
    let Ok(set) = code.sources(&[0, 1, 2, 3], &mut weights) else {
        return false;
    };
    let stripes: [&[u8]; 4] = [&[1], &[2], &[3], &[4]];
    set.agrees(4, &stripes, &[0x45]) == Ok(true) && set.agrees(4, &stripes, &[0x44]) == Ok(false)
}

#[cfg(feature = "alloc")]
mod correction {
    use crate::code::Code;
    use crate::correct::Correction;
    use crate::field::Field;
    use alloc::vec::Vec;

    /// Runs the correction on the all-zero code word of threshold 3 with `errors` added, one
    /// one-byte shard per index; returns what it found and the word afterwards.
    fn run<const M: usize>(errors: [u8; M]) -> Option<(Correction, [u8; M])> {
        let code = Code::new(Field::Gf256, 3).ok()?;
        let mut word = errors;
        let mut shards: Vec<(u16, &mut [u8])> = (0..)
            .zip(word.iter_mut().map(core::slice::from_mut))
            .collect();
        let found = code.correct(&mut shards).ok()?;
        drop(shards);
        Some((found, word))
    }

    /// The all-zero code word of threshold 3 over GF(2^11) with the errors 001 in shard 0 and
    /// 2AA in shard 2, whose syndromes 000 276 4EC 1DD were computed apart from this crate: the
    /// first syndrome is zero, and both errors must still be found.
    pub(super) fn wrong_words() -> bool {
        let Ok(code) = Code::new(Field::Gf2048, 3) else {
            return false;
        };
        let mut word = [[0_u8; 2]; 7];
        let [first, _, third, ..] = &mut word;
        *first = [0x00, 0x01];
        *third = [0x02, 0xAA];
        let mut shards: Vec<(u16, &mut [u8])> = (0..)
            .zip(word.iter_mut().map(<[u8; 2]>::as_mut_slice))
            .collect();
        let found = code.correct(&mut shards);
        drop(shards);
        found == Ok(Correction::Corrected([1, 0, 1, 0, 0, 0, 0].to_vec())) && word == [[0; 2]; 7]
    }

    /// A card of twelve words and the four words the code adds at the points 12 to 15, computed
    /// apart from this crate by Lagrange interpolation: words 3 and 8 marked unreadable and word
    /// 11 read as 7FF. The plan fills the two and replaces the third, and applied it gives the
    /// card back.
    pub(super) fn card_plan() -> bool {
        use crate::correct::{Change, ChangeKind, Mark, Plan};
        const CARD: [u16; 16] = [
            0x371, 0x6CE, 0x220, 0x11D, 0x2DB, 0x73E, 0x12B, 0x7C6, 0x7D5, 0x7AF, 0x6A9, 0x453,
            0x143, 0x384, 0x313, 0x6DE,
        ];
        let Ok(code) = Code::new(Field::Gf2048, 12) else {
            return false;
        };
        let mut read = CARD.map(u16::to_be_bytes);
        let [_, _, third, _, _, _, _, eighth, _, _, eleventh, ..] = &mut read;
        (*third, *eighth, *eleventh) = ([0, 0], [0, 0], [0x07, 0xFF]);
        let shards: Vec<(u16, &[u8])> = (0..).zip(read.iter().map(<[u8; 2]>::as_slice)).collect();
        let marks = [
            Mark {
                shard: 2,
                symbol: 0,
            },
            Mark {
                shard: 7,
                symbol: 0,
            },
        ];
        let change = |shard, was, becomes, kind| Change {
            shard,
            symbol: 0,
            was,
            becomes,
            kind,
        };
        let expected = [
            change(2, 0, 0x220, ChangeKind::Filled),
            change(7, 0, 0x7C6, ChangeKind::Filled),
            change(10, 0x7FF, 0x6A9, ChangeKind::Replaced),
        ];
        let planned = code.plan(&shards, &marks) == Ok(Plan::Changes(expected.to_vec()));
        drop(shards);
        let mut buffers: Vec<(u16, &mut [u8])> = (0..)
            .zip(read.iter_mut().map(<[u8; 2]>::as_mut_slice))
            .collect();
        let applied = code.apply(&mut buffers, &expected).is_ok();
        drop(buffers);
        planned && applied && read == CARD.map(u16::to_be_bytes)
    }

    /// The two words whose syndromes were computed apart from this crate (90 00, and 00 49 92
    /// 39): an error in shard 0, the point zero, and one more in shard 2.
    pub(super) fn within_the_bound() -> bool {
        let one = run([0x01, 0, 0, 0, 0]);
        let two = run([0x01, 0, 0x52, 0, 0, 0, 0]);
        one.is_some_and(|(found, word)| {
            found == Correction::Corrected([1, 0, 0, 0, 0].to_vec()) && word == [0; 5]
        }) && two.is_some_and(|(found, word)| {
            found == Correction::Corrected([1, 0, 1, 0, 0, 0, 0].to_vec()) && word == [0; 7]
        })
    }

    /// Four spare shards put two wrong symbols right, not three. For the word 01 02 03 00 00 00
    /// 00 no code word lies within two symbols: every one of the 35 choices of three shards was
    /// interpolated apart from this crate. So the one right answer is to give the column up and
    /// to change nothing.
    pub(super) fn beyond_the_bound() -> bool {
        let errors = [0x01, 0x02, 0x03, 0, 0, 0, 0];
        run(errors) == Some((Correction::Unresolved { columns: 1 }, errors))
    }
}
