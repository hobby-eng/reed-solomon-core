//! Differential test of the crate's cyclic code against MHFE's own MHFE-REPAIR-1 code, whose
//! field, parity and decoder (an exhaustive search over error places with erasure decoding) are
//! copied here unchanged in substance from mhfe_core/src/repair/field.rs (vector bounds raised).
//! Random containers of every MHFE length, every card size, damage from none to two beyond
//! 2e + s <= k: both must find the same word, or both none.

use reed_solomon_core::{CyclicCode, CyclicWords, Field};

const FIELD_SIZE: usize = 2048;
const ORDER: usize = FIELD_SIZE - 1;
const POLYNOMIAL: usize = 0x805;
const MAX_PARITY: usize = 8;

struct Mhfe {
    exp: Vec<u16>,
    log: Vec<u16>,
}

impl Mhfe {
    fn new() -> Self {
        let mut exp = vec![0u16; 2 * ORDER];
        let mut log = vec![0u16; FIELD_SIZE];
        let mut value = 1usize;
        let (powers, repeated) = exp.split_at_mut(ORDER);
        for (power, slot) in powers.iter_mut().enumerate() {
            *slot = value as u16;
            log[value] = power as u16;
            value <<= 1;
            if value & FIELD_SIZE != 0 {
                value ^= POLYNOMIAL;
            }
        }
        repeated.copy_from_slice(powers);
        Self { exp, log }
    }
    fn mul(&self, a: u16, b: u16) -> u16 {
        if a == 0 || b == 0 {
            return 0;
        }
        self.exp[usize::from(self.log[usize::from(a)]) + usize::from(self.log[usize::from(b)])]
    }
    fn div(&self, a: u16, b: u16) -> u16 {
        if a == 0 {
            return 0;
        }
        let power = usize::from(self.log[usize::from(a)]) + ORDER - usize::from(self.log[usize::from(b)]);
        self.exp[power]
    }
    fn alpha(&self, power: usize) -> u16 {
        self.exp[power % ORDER]
    }
    fn eval(&self, coefficients: &[u16], x: u16) -> u16 {
        coefficients.iter().fold(0, |sum, &c| self.mul(sum, x) ^ c)
    }
    fn generator(&self, k: usize) -> Vec<u16> {
        let mut g = vec![1u16];
        for power in 1..=k {
            let root = self.alpha(power);
            let mut next = vec![0u16; g.len() + 1];
            for (i, &c) in g.iter().enumerate() {
                next[i] ^= c;
                next[i + 1] ^= self.mul(c, root);
            }
            g = next;
        }
        g
    }
    fn parity(&self, data: &[u16], parity: &mut [u16]) {
        let k = parity.len();
        let g = self.generator(k);
        parity.fill(0);
        for &symbol in data {
            let feedback = symbol ^ parity[0];
            for i in 0..k - 1 {
                parity[i] = parity[i + 1] ^ self.mul(feedback, g[i + 1]);
            }
            parity[k - 1] = self.mul(feedback, g[k]);
        }
    }
    fn syndromes(&self, codeword: &[u16], k: usize) -> [u16; MAX_PARITY] {
        let mut s = [0; MAX_PARITY];
        for (power, syndrome) in (1..=k).zip(&mut s) {
            *syndrome = self.eval(codeword, self.alpha(power));
        }
        s
    }
    fn correct_at(&self, received: &[u16], positions: &[usize], k: usize, corrected: &mut [u16]) -> bool {
        let n = received.len();
        let syndromes = self.syndromes(received, k);
        corrected.copy_from_slice(received);
        if syndromes.iter().all(|&s| s == 0) {
            return true;
        }
        let mut locators = [0u16; MAX_PARITY];
        for (locator, &position) in locators.iter_mut().zip(positions) {
            *locator = self.alpha(n - 1 - position);
        }
        let mut gamma = [0u16; MAX_PARITY + 1];
        gamma[0] = 1;
        for (degree, &locator) in locators[..positions.len()].iter().enumerate() {
            for i in (0..=degree).rev() {
                gamma[i + 1] ^= self.mul(gamma[i], locator);
            }
        }
        let mut omega = [0u16; MAX_PARITY];
        for (i, &syndrome) in syndromes[..k].iter().enumerate() {
            for (j, &c) in gamma.iter().enumerate() {
                if i + j < k {
                    omega[i + j] ^= self.mul(syndrome, c);
                }
            }
        }
        let low_first = |cs: &[u16], x: u16| cs.iter().rev().fold(0, |sum, &c| self.mul(sum, x) ^ c);
        let mut derivative = [0u16; MAX_PARITY];
        for (i, c) in derivative.iter_mut().enumerate().step_by(2) {
            *c = gamma[i + 1];
        }
        for (&position, &locator) in positions.iter().zip(&locators) {
            let inverse = self.div(1, locator);
            let denominator = low_first(&derivative, inverse);
            if denominator == 0 {
                return false;
            }
            corrected[position] ^= self.div(low_first(&omega, inverse), denominator);
        }
        self.syndromes(corrected, k).iter().all(|&s| s == 0)
    }
    fn repair(&self, received: &[u16], unreadable: &[usize], k: usize, corrected: &mut [u16]) -> bool {
        if unreadable.len() > k {
            return false;
        }
        let readable: Vec<usize> = (0..received.len()).filter(|p| !unreadable.contains(p)).collect();
        let mut positions = [0usize; MAX_PARITY];
        positions[..unreadable.len()].copy_from_slice(unreadable);
        for wrong in 0..=(k - unreadable.len()) / 2 {
            let damaged = unreadable.len() + wrong;
            let mut found = false;
            for_each_subset(&readable, wrong, &mut |chosen| {
                if !found {
                    positions[unreadable.len()..damaged].copy_from_slice(chosen);
                    found = self.correct_at(received, &positions[..damaged], k, corrected);
                }
            });
            if found {
                return true;
            }
        }
        false
    }
}

fn for_each_subset(items: &[usize], size: usize, visit: &mut dyn FnMut(&[usize])) {
    fn walk(items: &[usize], chosen: &mut [usize], filled: usize, start: usize, visit: &mut dyn FnMut(&[usize])) {
        if filled == chosen.len() {
            visit(chosen);
            return;
        }
        for i in start..items.len() {
            chosen[filled] = items[i];
            walk(items, chosen, filled + 1, i + 1, visit);
        }
    }
    let mut chosen = [0; MAX_PARITY];
    walk(items, &mut chosen[..size], 0, 0, visit);
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

fn main() {
    let mhfe = Mhfe::new();
    let mut rng = Rng(0xC0FF_EE00_1234_ABCD);
    let (mut same_word, mut both_none, mut parity_same, mut bad) = (0usize, 0usize, 0usize, 0usize);
    let mut beyond_found = 0usize;
    let rounds = 24_000;
    for round in 0..rounds {
        let n = [12, 15, 18, 21, 24][round % 5];
        let k = [2, 4, 6, 8][(round / 5) % 4];
        let data: Vec<u16> = (0..n).map(|_| rng.below(FIELD_SIZE) as u16).collect();
        let mut theirs = vec![0u16; k];
        mhfe.parity(&data, &mut theirs);
        let code = CyclicCode::new(Field::Gf2048, n, k, 1, 1).unwrap();
        let shards: Vec<[u8; 2]> = data.iter().map(|w| w.to_be_bytes()).collect();
        let views: Vec<&[u8]> = shards.iter().map(|s| s.as_slice()).collect();
        let ours: Vec<u16> = code.encode(&views).unwrap().iter().map(|b| u16::from_be_bytes([b[0], b[1]])).collect();
        if ours != theirs {
            bad += 1;
            println!("PARITY DIFFERS n={n} k={k}");
            continue;
        }
        parity_same += 1;
        let codeword: Vec<u16> = data.iter().chain(&ours).copied().collect();
        let len = n + k;
        // Damage: s unreadable, e wrong, 2e + s from 0 to k + 2 (beyond for the last two).
        let budget = rng.below(k + 3);
        let s = rng.below(budget.min(k) + 1);
        let e = (budget - s.min(budget)).div_ceil(2);
        let mut places: Vec<usize> = (0..len).collect();
        for i in (1..len).rev() {
            let j = rng.below(i + 1);
            places.swap(i, j);
        }
        let mut unreadable: Vec<usize> = places[..s].to_vec();
        unreadable.sort_unstable();
        let wrong = &places[s..(s + e).min(len)];
        let mut received = codeword.clone();
        for &p in &unreadable {
            received[p] = 0; // MHFE reads an unreadable token as 0 here
        }
        for &p in wrong {
            received[p] ^= 1 + rng.below(FIELD_SIZE - 1) as u16;
        }
        let mut theirs_fixed = vec![0u16; len];
        let theirs_ok = mhfe.repair(&received, &unreadable, k, &mut theirs_fixed);
        let mut ours_fixed = received.clone();
        for &p in &unreadable {
            ours_fixed[p] = 0xFFFF; // the crate never reads an erased place
        }
        let mut room = vec![0u16; CyclicWords::room(k)];
        let ours_answer = code.words().correct_word(&mut ours_fixed, &unreadable, &mut room).unwrap();
        let within = 2 * wrong.len() + s <= k;
        match (theirs_ok, ours_answer) {
            (true, Some(_)) if ours_fixed == theirs_fixed => {
                same_word += 1;
                if !within {
                    beyond_found += 1;
                }
                if within && ours_fixed != codeword {
                    bad += 1;
                    println!("WITHIN BUT NOT THE ORIGINAL n={n} k={k}");
                }
            }
            (false, None) => {
                both_none += 1;
                if within {
                    bad += 1;
                    println!("WITHIN BUT BOTH GAVE UP n={n} k={k}");
                }
            }
            (t, o) => {
                bad += 1;
                if bad <= 10 {
                    println!("DIFFER n={n} k={k} s={s} e={} within={within}: mhfe {t} crate {o:?}", wrong.len());
                }
            }
        }
    }
    println!(
        "{rounds} random cases: parity equal {parity_same}; same word {same_word} (of which beyond the bound {beyond_found}); both none {both_none}; differences {bad}"
    );
    if bad > 0 {
        aud006_codes_probe::fail();
    }
    aud006_codes_probe::finish();
}
