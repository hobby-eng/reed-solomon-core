//! One word of a cyclic code put right: from its syndromes and the places marked unreadable,
//! Berlekamp–Massey gives the locator of the wrong places, a search over the places of the word
//! finds them, and Forney's formula gives the value to add at each.

use super::{NoMemory, power, roomy};
use crate::correct::berlekamp_massey;
use crate::field::Field;
use alloc::vec::Vec;
use zeroize::Zeroizing;

/// What fixes the search of one code: its field and lengths, and β, whose powers are the roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Word {
    pub(super) field: Field,
    /// Places of a word: data and parity.
    pub(super) length: usize,
    /// The parity, which is the number of syndromes.
    pub(super) parity: usize,
    /// The exponent of the first root, below the order of β.
    pub(super) first: usize,
    pub(super) beta: u16,
}

/// The working lists of a search, made once for a call and wiped when it ends: the values in them
/// follow from the data, and for a marked place they are the symbol itself.
pub(super) struct Work {
    /// The syndromes of the word at hand, its marked symbols taken as zero.
    pub(super) syndromes: Zeroizing<Vec<u16>>,
    gamma: Zeroizing<Vec<u16>>,
    forney: Zeroizing<Vec<u16>>,
    locator: Zeroizing<Vec<u16>>,
    previous: Zeroizing<Vec<u16>>,
    saved: Zeroizing<Vec<u16>>,
    errata: Zeroizing<Vec<u16>>,
    evaluator: Zeroizing<Vec<u16>>,
    /// The places found, in their order, and the value to add at each.
    pub(super) places: Zeroizing<Vec<usize>>,
    pub(super) values: Zeroizing<Vec<u16>>,
}

impl Work {
    /// Room for every list of a code with `parity` syndromes: a product of two of them has at
    /// most twice as many terms.
    pub(super) fn new(parity: usize) -> Result<Self, NoMemory> {
        let room = parity
            .checked_mul(2)
            .and_then(|room| room.checked_add(2))
            .ok_or(NoMemory)?;
        let list = || roomy(room).map(Zeroizing::new);
        Ok(Self {
            syndromes: list()?,
            gamma: list()?,
            forney: list()?,
            locator: list()?,
            previous: list()?,
            saved: list()?,
            errata: list()?,
            evaluator: list()?,
            places: roomy(room).map(Zeroizing::new)?,
            values: list()?,
        })
    }
}

impl Word {
    /// The locator of `place`: β^(length − 1 − place).
    pub(super) fn locator(&self, place: usize) -> u16 {
        power(self.field, self.beta, self.length - 1 - place)
    }

    /// Finds the errata of the word whose syndromes are in `work.syndromes`, its symbols at the
    /// `erased` places, sorted, taken as zero: leaves the places in `work.places`, ascending, and
    /// the value to add at each in `work.values`. False when the word cannot be put right within
    /// `2e + s ≤ parity`, or when what was found does not leave it without syndromes.
    pub(super) fn errata(&self, work: &mut Work, erased: &[usize]) -> bool {
        let (field, parity) = (self.field, self.parity);
        let rho = erased.len();
        if rho > parity {
            return false;
        }
        // Γ(x) = Π (1 + X x) over the marked places, and the syndromes it leaves to the errors.
        work.gamma.clear();
        work.gamma.push(1);
        for &place in erased {
            times_linear(field, &mut work.gamma, self.locator(place));
        }
        product(
            field,
            &work.syndromes,
            &work.gamma,
            parity,
            &mut work.forney,
        );
        // The syndromes after the first `rho`, spent on the marked places, give the locator of
        // the wrong ones.
        let sequence = work.forney.get(rho..parity).unwrap_or_default();
        for list in [&mut work.locator, &mut work.previous, &mut work.saved] {
            list.clear();
            list.resize(parity - rho + 1, 0);
        }
        let length = berlekamp_massey(
            field,
            sequence,
            &mut work.locator,
            &mut work.previous,
            &mut work.saved,
        );
        work.locator.truncate(length + 1);
        while work.locator.len() > 1 && work.locator.last() == Some(&0) {
            work.locator.pop();
        }
        let errors = work.locator.len() - 1;
        if 2 * errors + rho > parity {
            return false;
        }
        product(
            field,
            &work.locator,
            &work.gamma,
            usize::MAX,
            &mut work.errata,
        );
        product(
            field,
            &work.syndromes,
            &work.errata,
            parity,
            &mut work.evaluator,
        );
        self.search(work) && self.values(work, erased) && self.leaves_no_syndrome(work)
    }

    /// The places whose locator's inverse is a root of the errata locator; as many as its degree,
    /// all inside the word, or the word is not put right.
    fn search(&self, work: &mut Work) -> bool {
        work.places.clear();
        let mut x = 1;
        for place in (0..self.length).rev() {
            let inverse = self.field.inverse(x).unwrap_or(0);
            if evaluate(self.field, &work.errata, inverse) == 0 {
                work.places.push(place);
            }
            x = self.field.multiply(x, self.beta);
        }
        work.places.reverse();
        work.places.len() + 1 == work.errata.len()
    }

    /// Forney's value at each place found: X^(1 − first) Ω(X⁻¹) / Ψ'(X⁻¹). A place not marked
    /// gets a value that is not zero, or the search went astray.
    fn values(&self, work: &mut Work, erased: &[usize]) -> bool {
        let field = self.field;
        let elements = field.order() - 1;
        let lift = (1 + elements - self.first % elements) % elements;
        work.values.clear();
        for &place in work.places.iter() {
            let x = self.locator(place);
            let inverse = field.inverse(x).unwrap_or(0);
            let numerator = field.multiply(
                power(field, x, lift),
                evaluate(field, &work.evaluator, inverse),
            );
            let Some(over) = field.inverse(derivative(field, &work.errata, inverse)) else {
                return false;
            };
            let value = field.multiply(numerator, over);
            if value == 0 && erased.binary_search(&place).is_err() {
                return false;
            }
            work.values.push(value);
        }
        true
    }

    /// Whether the word with the values added has no syndrome left.
    fn leaves_no_syndrome(&self, work: &Work) -> bool {
        let field = self.field;
        (0..self.parity).all(|at| {
            let start = work.syndromes.get(at).copied().unwrap_or(0);
            let left =
                work.places
                    .iter()
                    .zip(work.values.iter())
                    .fold(start, |left, (&place, &value)| {
                        let x = self.locator(place);
                        left ^ field.multiply(value, power(field, x, self.first + at))
                    });
            left == 0
        })
    }
}

/// `p` times (1 + x·root), lowest coefficient first, in place.
fn times_linear(field: Field, p: &mut Vec<u16>, root: u16) {
    p.push(0);
    for at in (1..p.len()).rev() {
        let below = p.get(at - 1).copied().unwrap_or(0);
        if let Some(slot) = p.get_mut(at) {
            *slot ^= field.multiply(root, below);
        }
    }
}

/// The product of `a` and `b`, lowest coefficient first, cut to its first `limit` coefficients.
fn product(field: Field, a: &[u16], b: &[u16], limit: usize, out: &mut Vec<u16>) {
    out.clear();
    let length = (a.len() + b.len()).saturating_sub(1).min(limit);
    out.resize(length, 0);
    for (i, &x) in a.iter().enumerate() {
        for (slot, &y) in out.iter_mut().skip(i).zip(b) {
            *slot ^= field.multiply(x, y);
        }
    }
}

/// `p`, lowest coefficient first, at `x`.
fn evaluate(field: Field, p: &[u16], x: u16) -> u16 {
    p.iter().rev().fold(0, |sum, &c| field.multiply(sum, x) ^ c)
}

/// The formal derivative of `p` at `x`: in characteristic 2 only its odd terms are left, each
/// down one degree.
fn derivative(field: Field, p: &[u16], x: u16) -> u16 {
    let square = field.multiply(x, x);
    let odd = p.iter().skip(1).step_by(2).rev();
    odd.fold(0, |sum, &c| field.multiply(sum, square) ^ c)
}
