//! One word of a cyclic code put right in working room the caller gives, without an allocator: from
//! its syndromes and the places marked unreadable, Berlekamp–Massey gives the locator of the wrong
//! places, a search over the places of the word finds them, and Forney's formula gives the value
//! to add at each.

use super::power;
use crate::code::CodeError;
use crate::field::Field;
use crate::list::List;
use crate::recurrence::berlekamp_massey;
use core::ops::Deref;

/// The element `x`: in a field given by a primitive polynomial it generates every other nonzero
/// element, and it is the α of the code.
const ALPHA: u16 = 2;

/// The lists of a search, each with room for `parity + 1` elements.
const LISTS: usize = 10;

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

impl Word {
    /// The code of `data` and `parity` places over `field` with the roots β^(first_root + i), as
    /// `CyclicCode::new` describes it, or [`CodeError::Shape`].
    pub(super) fn new(
        field: Field,
        data: usize,
        parity: usize,
        first_root: u32,
        root_step: u32,
    ) -> Result<Self, CodeError> {
        let elements = field.order() - 1;
        let step = usize::try_from(root_step).unwrap_or(usize::MAX) % elements;
        let order = elements / gcd(step, elements);
        let length = data.checked_add(parity).filter(|&length| length <= order);
        let fits = !matches!(field, Field::Gf65536) && data > 0 && parity > 0;
        let Some(length) = length.filter(|_| fits) else {
            return Err(CodeError::Shape);
        };
        Ok(Self {
            field,
            length,
            parity,
            first: usize::try_from(first_root).unwrap_or(usize::MAX) % order,
            beta: power(field, ALPHA, step),
        })
    }

    /// Writes the generator Π (x + β^(first + i)) into `out`, which holds exactly `parity + 1`
    /// elements, its coefficients from x^parity, which is 1, down to x^0; else [`CodeError::Length`].
    pub(super) fn generator_into(&self, out: &mut [u16]) -> Result<(), CodeError> {
        if out.len() != self.parity + 1 {
            return Err(CodeError::Length);
        }
        out.fill(0);
        if let Some(top) = out.first_mut() {
            *top = 1;
        }
        let mut root = power(self.field, self.beta, self.first);
        for made in 1..=self.parity {
            // Times (x + root): every coefficient takes root times the one before it.
            for at in (1..=made).rev() {
                let before = out.get(at - 1).copied().unwrap_or(0);
                if let Some(slot) = out.get_mut(at) {
                    *slot ^= self.field.multiply(root, before);
                }
            }
            root = self.field.multiply(root, self.beta);
        }
        Ok(())
    }

    /// The locator of `place`: β^(length − 1 − place).
    pub(super) fn locator(&self, place: usize) -> u16 {
        power(self.field, self.beta, self.length - 1 - place)
    }

    /// The syndromes of the words in the `span` bytes of `shards` from byte `at`, one buffer of
    /// `span` bytes for each: S_l = Σ c_i X_i^(first + l), X_i = β^(length − 1 − i). The shards at
    /// the places `lost`, in rising order, are left out, as if they were zeros.
    pub(super) fn stripe<S: Deref<Target = [u8]>>(
        &self,
        shards: &[S],
        lost: &[usize],
        at: usize,
        syndromes: &mut [u8],
        span: usize,
    ) {
        let field = self.field;
        syndromes.fill(0);
        let mut x = 1;
        for (place, shard) in shards.iter().enumerate().rev() {
            if lost.binary_search(&place).is_err() {
                let source = shard.get(at..at + span).unwrap_or_default();
                let mut factor = power(field, x, self.first);
                for out in syndromes.chunks_exact_mut(span).take(self.parity) {
                    field.add_scaled(out, source, factor);
                    factor = field.multiply(factor, x);
                }
            }
            x = field.multiply(x, self.beta);
        }
    }

    /// The syndromes of the word `symbols` into `work.syndromes`, the symbols at the places
    /// `erased`, in rising order, left out as if they were zeros.
    pub(super) fn syndromes_of(&self, symbols: &[u16], erased: &[usize], work: &mut Work<'_>) {
        let field = self.field;
        work.syndromes.clear();
        work.syndromes.resize(self.parity, 0);
        for (place, symbol) in symbols.iter().enumerate() {
            if erased.binary_search(&place).is_ok() {
                continue;
            }
            let symbol = *symbol;
            let x = self.locator(place);
            let mut factor = field.multiply(symbol, power(field, x, self.first));
            for syndrome in work.syndromes.as_mut_slice() {
                *syndrome ^= factor;
                factor = field.multiply(factor, x);
            }
        }
    }

    /// Finds the errata of the word whose syndromes are in `work.syndromes`, its symbols at the
    /// `erased` places, sorted, taken as zero: leaves the places in `work.places`, ascending, and
    /// the value to add at each in `work.values`. False when the word cannot be put right within
    /// `2e + s ≤ parity`, or when what was found does not leave it without syndromes.
    pub(super) fn errata(&self, work: &mut Work<'_>, erased: &[usize]) -> bool {
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
        let (syndromes, gamma) = (work.syndromes.as_slice(), work.gamma.as_slice());
        product(field, syndromes, gamma, parity, &mut work.forney);
        // The syndromes after the first `rho`, spent on the marked places, give the locator of
        // the wrong ones.
        let sequence = work.forney.as_slice().get(rho..parity).unwrap_or_default();
        for list in [&mut work.locator, &mut work.previous, &mut work.saved] {
            list.clear();
            list.resize(parity - rho + 1, 0);
        }
        let length = berlekamp_massey(
            field,
            sequence,
            work.locator.as_mut_slice(),
            work.previous.as_mut_slice(),
            work.saved.as_mut_slice(),
        );
        work.locator.truncate(length + 1);
        while work.locator.len() > 1 && work.locator.last() == Some(0) {
            work.locator.pop();
        }
        let errors = work.locator.len() - 1;
        if 2 * errors + rho > parity {
            return false;
        }
        let (locator, gamma) = (work.locator.as_slice(), work.gamma.as_slice());
        product(field, locator, gamma, usize::MAX, &mut work.errata);
        let (syndromes, errata) = (work.syndromes.as_slice(), work.errata.as_slice());
        product(field, syndromes, errata, parity, &mut work.evaluator);
        self.search(work) && self.values(work, erased) && self.leaves_no_syndrome(work)
    }

    /// The places whose locator's inverse is a root of the errata locator; as many as its degree,
    /// all inside the word, or the word is not put right.
    fn search(&self, work: &mut Work<'_>) -> bool {
        work.places.clear();
        let mut x = 1;
        for place in (0..self.length).rev() {
            let inverse = self.field.inverse(x).unwrap_or(0);
            if evaluate(self.field, work.errata.as_slice(), inverse) == 0 {
                // A place of a word is below its length, at most 65,535.
                work.places.push(u16::try_from(place).unwrap_or(u16::MAX));
            }
            x = self.field.multiply(x, self.beta);
        }
        work.places.reverse();
        work.places.len() + 1 == work.errata.len()
    }

    /// Forney's value at each place found: X^(1 − first) Ω(X⁻¹) / Ψ'(X⁻¹). A place not marked
    /// gets a value that is not zero, or the search went astray.
    fn values(&self, work: &mut Work<'_>, erased: &[usize]) -> bool {
        let field = self.field;
        let elements = field.order() - 1;
        let lift = (1 + elements - self.first % elements) % elements;
        work.values.clear();
        for at in 0..work.places.len() {
            let place = work
                .places
                .as_slice()
                .get(at)
                .map_or(0, |&place| usize::from(place));
            let x = self.locator(place);
            let inverse = field.inverse(x).unwrap_or(0);
            let numerator = field.multiply(
                power(field, x, lift),
                evaluate(field, work.evaluator.as_slice(), inverse),
            );
            let slope = derivative(field, work.errata.as_slice(), inverse);
            let Some(over) = field.inverse(slope) else {
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
    fn leaves_no_syndrome(&self, work: &Work<'_>) -> bool {
        let field = self.field;
        (0..self.parity).all(|at| {
            let start = work.syndromes.as_slice().get(at).copied().unwrap_or(0);
            let found = work.places().zip(work.values.as_slice().iter());
            let left = found.fold(start, |left, (place, &value)| {
                let x = self.locator(place);
                left ^ field.multiply(value, power(field, x, self.first + at))
            });
            left == 0
        })
    }
}

/// The lists of a search, cut from one slice of working room: the values in them follow from the
/// data, and for a marked place they are the symbol itself, so the room is wiped after a call.
pub(super) struct Work<'w> {
    /// The syndromes of the word at hand, its marked symbols taken as zero.
    pub(super) syndromes: List<'w>,
    gamma: List<'w>,
    forney: List<'w>,
    locator: List<'w>,
    previous: List<'w>,
    saved: List<'w>,
    errata: List<'w>,
    evaluator: List<'w>,
    /// The places found, in their order, and the value to add at each.
    places: List<'w>,
    pub(super) values: List<'w>,
}

impl<'w> Work<'w> {
    /// Elements of room for a code with `parity` syndromes. No list holds more than `parity + 1`:
    /// the errata locator has `e + s + 1` terms with `2e + s ≤ parity`, has at most `e + s` roots,
    /// and the other products are cut to `parity` terms.
    pub(super) const fn room(parity: usize) -> usize {
        parity.saturating_add(1).saturating_mul(LISTS)
    }

    /// The lists, cut from `room`, which holds at least [`Work::room`] elements; `None` if not.
    pub(super) fn new(parity: usize, room: &'w mut [u16]) -> Option<Self> {
        let each = parity.checked_add(1)?;
        let room = room.get_mut(..each.checked_mul(LISTS)?)?;
        let mut lists = room.chunks_exact_mut(each).map(List::new);
        Some(Self {
            syndromes: lists.next()?,
            gamma: lists.next()?,
            forney: lists.next()?,
            locator: lists.next()?,
            previous: lists.next()?,
            saved: lists.next()?,
            errata: lists.next()?,
            evaluator: lists.next()?,
            places: lists.next()?,
            values: lists.next()?,
        })
    }

    /// Forgets the places found: a word with nothing to change.
    #[cfg(feature = "alloc")]
    pub(super) const fn found_nothing(&mut self) {
        self.places.clear();
        self.values.clear();
    }

    /// The places found, in rising order.
    pub(super) fn places(&self) -> impl Iterator<Item = usize> + '_ {
        self.places
            .as_slice()
            .iter()
            .map(|&place| usize::from(place))
    }
}

/// `p` times (1 + x·root), lowest coefficient first, in place.
fn times_linear(field: Field, p: &mut List<'_>, root: u16) {
    p.push(0);
    let p = p.as_mut_slice();
    for at in (1..p.len()).rev() {
        let below = p.get(at - 1).copied().unwrap_or(0);
        if let Some(slot) = p.get_mut(at) {
            *slot ^= field.multiply(root, below);
        }
    }
}

/// The product of `a` and `b`, lowest coefficient first, cut to its first `limit` coefficients.
fn product(field: Field, a: &[u16], b: &[u16], limit: usize, out: &mut List<'_>) {
    out.clear();
    out.resize((a.len() + b.len()).saturating_sub(1).min(limit), 0);
    let out = out.as_mut_slice();
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

/// The greatest common divisor; gcd(0, n) is n.
const fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
