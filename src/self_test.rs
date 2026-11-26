//! Known answers of the code, for a program to run before it relies on the crate: a program that
//! starts its work only after [`self_test`] passed knows that the build it runs codes as the
//! references do. Every expected value but one comes from outside this crate, and each check
//! names its source; the one is a property, that a changed shard is told from the right one.

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
const MOST: usize = 17;

/// The checks a run of this build makes: without an allocator six are left out.
const EXPECTED: usize = if cfg!(feature = "alloc") {
    MOST
} else {
    MOST - 6
};

/// The checks of one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfTest {
    checks: [Check; MOST],
    count: usize,
    /// Whether a check came after the run was full, and so was not kept.
    overflowed: bool,
}

impl SelfTest {
    /// Whether the run made every check of this build, no more, and each passed. A program relies
    /// on the crate only then.
    #[must_use]
    pub fn passed(&self) -> bool {
        !self.overflowed && self.count == EXPECTED && self.checks().iter().all(|check| check.passed)
    }

    /// Every check, in the order run.
    #[must_use]
    pub fn checks(&self) -> &[Check] {
        self.checks.get(..self.count).unwrap_or_default()
    }

    fn add(&mut self, name: &'static str, passed: bool) {
        match self.checks.get_mut(self.count) {
            Some(check) => {
                *check = Check { name, passed };
                self.count += 1;
            }
            None => self.overflowed = true,
        }
    }
}

/// Runs the known answers: a few field multiplications' and hashes' worth of work. Without the
/// `alloc` feature it allocates nothing and leaves out the three checks of the error correction,
/// the plan of changes for a card, the vote among the files of a set and a digest that differs.
#[must_use]
pub fn self_test() -> SelfTest {
    let empty = Check {
        name: "",
        passed: false,
    };
    let mut run = SelfTest {
        checks: [empty; MOST],
        count: 0,
        overflowed: false,
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
        "GF(2^4), 0x13, and GF(2^13), 0x201B: the data 1 and 2^m - 1 give shards 2 and 3 as E, 0 and 1FE6, 018 (computed apart)",
        chosen_fields(),
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
    run.add(
        "A cyclic code of GF(2^8), roots α^0 to α^9: the data of the QR example gives A5 24 D4 C1 ED 36 C7 87 2C 55 (ISO/IEC 18004, annex I)",
        cyclic::qr_example(),
    );
    run.add(
        "A cyclic code of GF(2^11), first root 5, step 3: one wrong and two marked words are put right (words computed apart)",
        cyclic::marked_word(),
    );
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

/// Two fields of a degree a program chooses, by their standard polynomials: the set of two whose
/// shards 0 and 1 are 1 and the largest element, with its shards 2 and 3 computed apart from this
/// crate with tables of logarithms.
fn chosen_fields() -> bool {
    let (Ok(four), Ok(thirteen)) = (Field::binary(4, None), Field::binary(13, None)) else {
        return false;
    };
    let small = [[0x01], [0x0F]];
    let large = [[0x00, 0x01], [0x1F, 0xFF]];
    made(four, [0, 1], small, 2) == Some([0x0E])
        && made(four, [0, 1], small, 3) == Some([0x00])
        && made(thirteen, [0, 1], large, 2) == Some([0x1F, 0xE6])
        && made(thirteen, [0, 1], large, 3) == Some([0x00, 0x18])
        // 0x10 is not an element of GF(2^4).
        && made(four, [0, 1], [[0x10], [0x00]], 2).is_none()
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
    // The bytes compared are public here: room on the stack, so that no page can be refused.
    let mut room = [0; crate::code::COMPARED];
    let right = set.agrees_in(4, &stripes, &[0x45], &mut room);
    right == Ok(true) && set.agrees_in(4, &stripes, &[0x44], &mut room) == Ok(false)
}

/// The cyclic code without an allocator, so that a build without one checks it too.
mod cyclic {
    use crate::cyclic::CyclicWords;
    use crate::field::Field;

    /// The parity of the version 1-M example of ISO/IEC 18004, annex I.
    pub(super) fn qr_example() -> bool {
        let data: [u8; 16] = [
            0x10, 0x20, 0x0C, 0x56, 0x61, 0x80, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11, 0xEC, 0x11,
            0xEC, 0x11,
        ];
        let expected = [0xA5, 0x24, 0xD4, 0xC1, 0xED, 0x36, 0xC7, 0x87, 0x2C, 0x55];
        let mut generator = [0; 11];
        let Ok(code) = CyclicWords::new(Field::Gf256, 16, 10, 0, 1, &mut generator) else {
            return false;
        };
        let shards = data.each_ref().map(core::slice::from_ref);
        let mut parity = [[0_u8; 1]; 10];
        let mut outs = parity.each_mut().map(<[u8; 1]>::as_mut_slice);
        let made = code.encode(&shards, &mut outs, &mut [0; 10]).is_ok();
        made && parity
            .iter()
            .zip(expected)
            .all(|(shard, byte)| *shard == [byte])
    }

    /// The word 001 7FF 400 123 000 410 680 321 134, read with word 1 wrong and words 3 and 6
    /// marked: one word replaced and two filled give the word back.
    pub(super) fn marked_word() -> bool {
        let whole: [u16; 9] = [
            0x001, 0x7FF, 0x400, 0x123, 0x000, 0x410, 0x680, 0x321, 0x134,
        ];
        let mut read: [u16; 9] = [
            0x001, 0x55A, 0x400, 0x7FF, 0x000, 0x410, 0x000, 0x321, 0x134,
        ];
        let mut generator = [0; 5];
        let Ok(code) = CyclicWords::new(Field::Gf2048, 5, 4, 5, 3, &mut generator) else {
            return false;
        };
        let mut room = [0; CyclicWords::room(4)];
        let found = code.correct_word(&mut read, &[3, 6], &mut room);
        found == Ok(Some(3)) && read == whole
    }
}

#[cfg(feature = "alloc")]
mod correction {
    use crate::code::Code;
    use crate::correct::Correction;
    use crate::field::Field;

    /// The shards of `word`, numbered from zero: an array, so that a check asks for no memory
    /// of its own and a refusal of memory fails the check instead of ending the program.
    fn numbered<const M: usize, T>(
        word: &mut [T; M],
        bytes: impl Fn(&mut T) -> &mut [u8],
    ) -> [(u16, &mut [u8]); M] {
        let mut index = 0;
        word.each_mut().map(|shard| {
            let numbered = (index, bytes(shard));
            index += 1;
            numbered
        })
    }

    /// Whether `found` corrected exactly `counts` symbols in each shard.
    fn corrected(found: &Correction, counts: &[usize]) -> bool {
        matches!(found, Correction::Corrected(each) if each.as_slice() == counts)
    }

    /// Runs the correction on the all-zero code word of threshold 3 with `errors` added, one
    /// one-byte shard per index; returns what it found and the word afterwards.
    fn run<const M: usize>(errors: [u8; M]) -> Option<(Correction, [u8; M])> {
        let code = Code::new(Field::Gf256, 3).ok()?;
        let mut word = errors;
        let mut shards = numbered(&mut word, core::slice::from_mut);
        let found = code.correct(&mut shards).ok()?;
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
        let mut shards = numbered(&mut word, <[u8; 2]>::as_mut_slice);
        let found = code.correct(&mut shards);
        found.is_ok_and(|found| corrected(&found, &[1, 0, 1, 0, 0, 0, 0])) && word == [[0; 2]; 7]
    }

    /// A card of twelve words and the four words the code adds at the points 12 to 15, computed
    /// apart from this crate by Lagrange interpolation: words 3 and 8 marked unreadable and word
    /// 11 read as 7FF. The plan fills the two and replaces the third, and applied it gives the
    /// card back.
    pub(super) fn card_plan() -> bool {
        use crate::correct::{ChangeKind, Mark, Plan};
        const CARD: [u16; 16] = [
            0x371, 0x6CE, 0x220, 0x11D, 0x2DB, 0x73E, 0x12B, 0x7C6, 0x7D5, 0x7AF, 0x6A9, 0x453,
            0x143, 0x384, 0x313, 0x6DE,
        ];
        // Each shard and its place, what it held and becomes, and how.
        const EXPECTED: [(usize, u16, u16, ChangeKind); 3] = [
            (2, 0, 0x220, ChangeKind::Filled),
            (7, 0, 0x7C6, ChangeKind::Filled),
            (10, 0x7FF, 0x6A9, ChangeKind::Replaced),
        ];
        let Ok(code) = Code::new(Field::Gf2048, 12) else {
            return false;
        };
        let mut read = CARD.map(u16::to_be_bytes);
        let [_, _, third, _, _, _, _, eighth, _, _, eleventh, ..] = &mut read;
        (*third, *eighth, *eleventh) = ([0, 0], [0, 0], [0x07, 0xFF]);
        // Arrays, not lists: a self-test that gets no memory fails, it does not stop the program.
        let mut index = 0_u16;
        let shards = read.each_ref().map(|word| {
            index += 1;
            (index - 1, word.as_slice())
        });
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
        let Ok(Plan::Changes(changes)) = code.plan(&shards, &marks) else {
            return false;
        };
        let listed = (0..EXPECTED.len())
            .zip(EXPECTED)
            .all(|(at, (shard, was, becomes, kind))| {
                let place = changes.get(at).is_some_and(|change| {
                    (change.shard, change.symbol, change.kind) == (shard, 0, kind)
                });
                let values = changes.values(at, |held, new| held == was && new == becomes);
                place && values == Some(true)
            });
        let planned = listed && changes.len() == EXPECTED.len();
        let mut index = 0_u16;
        let mut buffers = read.each_mut().map(|word| {
            index += 1;
            (index - 1, word.as_mut_slice())
        });
        let applied = code.apply(&mut buffers, &changes).is_ok();
        planned && applied && read == CARD.map(u16::to_be_bytes)
    }

    /// The two words whose syndromes were computed apart from this crate (90 00, and 00 49 92
    /// 39): an error in shard 0, the point zero, and one more in shard 2.
    pub(super) fn within_the_bound() -> bool {
        let one = run([0x01, 0, 0, 0, 0]);
        let two = run([0x01, 0, 0x52, 0, 0, 0, 0]);
        one.is_some_and(|(found, word)| corrected(&found, &[1, 0, 0, 0, 0]) && word == [0; 5])
            && two.is_some_and(|(found, word)| {
                corrected(&found, &[1, 0, 1, 0, 0, 0, 0]) && word == [0; 7]
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

#[cfg(test)]
mod tests {
    /// Every check of this build passes; without an allocator six of the seventeen are left out.
    #[test]
    fn every_check_of_this_build_passes() {
        let run = super::self_test();
        assert!(run.passed());
        let count = if cfg!(feature = "alloc") { 17 } else { 11 };
        assert_eq!(run.checks().len(), count);
    }

    use super::{Check, EXPECTED, MOST, SelfTest};

    /// A run of checks with these results.
    fn run(results: &[bool]) -> SelfTest {
        let empty = Check {
            name: "",
            passed: false,
        };
        let mut run = SelfTest {
            checks: [empty; MOST],
            count: 0,
            overflowed: false,
        };
        for &passed in results {
            run.add("a check", passed);
        }
        run
    }

    /// A run passes only when it made exactly the checks of this build and each passed: none
    /// failed, none was missing, and none was left out for want of room.
    #[test]
    fn a_run_passes_only_whole_and_without_a_failure() {
        assert!(run(&[true; EXPECTED]).passed());
        let one_failed: Vec<bool> = (0..EXPECTED).map(|check| check != 3).collect();
        assert!(!run(&one_failed).passed());
        assert!(!run(&[true; EXPECTED - 1]).passed());
        assert!(!run(&[]).passed());
        let one_more = run(&[true; MOST + 1]);
        assert_eq!(one_more.checks().len(), MOST);
        assert!(!one_more.passed());
    }
}
