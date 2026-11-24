//! AUD-006 probe: does the time of checking a clean word depend on its (secret) symbols?
//! A clean word is put right with no change; only the syndromes of its symbols are computed.
//! Interleaved rounds: all-zero words against distinct random words, the named GF(2^11) as the
//! control and a field chosen by degree (`Field::binary(10, None)`) as the case under test.
use reed_solomon_core::{CyclicCode, CyclicWords, Field};
use std::hint::black_box;
use std::time::Instant;

const WORDS: usize = 2048;
const ROUNDS: usize = 41;

/// A word of the code: `data` symbols and the parity the code gives them.
fn codeword(code: &CyclicCode, data: &[u16; 12]) -> [u16; 16] {
    let shards: Vec<[u8; 2]> = data.iter().map(|d| d.to_be_bytes()).collect();
    let refs: Vec<&[u8]> = shards.iter().map(|s| &s[..]).collect();
    let parity = code.encode(&refs).expect("encode");
    let mut word = [0_u16; 16];
    word[..12].copy_from_slice(data);
    for (slot, p) in word[12..].iter_mut().zip(&parity) {
        *slot = u16::from_be_bytes([p[0], p[1]]);
    }
    word
}

/// Nanoseconds per call over one pass of `words`.
fn pass(code: &CyclicWords<'_>, words: &[[u16; 16]], room: &mut [u16]) -> f64 {
    let start = Instant::now();
    for word in words {
        let mut w = black_box(*word);
        let answer = code.correct_word(&mut w, &[], black_box(&mut *room));
        assert_eq!(black_box(answer), Ok(Some(0)));
    }
    start.elapsed().as_nanos() as f64 / words.len() as f64
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() {
    let mut state = 0x5EED_u64;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let binary = Field::binary(10, None).expect("GF(2^10)");
    // A field whose time depends on the symbols fails the run.
    let mut failed = false;
    for (name, field, mask) in [("Gf2048 (named)", Field::Gf2048, 0x7FF_u16), ("Binary GF(2^10)", binary, 0x3FF)] {
        let owned = CyclicCode::new(field, 12, 4, 1, 1).expect("code");
        let zero = codeword(&owned, &[0; 12]);
        let zeros = vec![zero; WORDS];
        let randoms: Vec<[u16; 16]> = (0..WORDS)
            .map(|_| {
                let mut data = [0_u16; 12];
                data.iter_mut().for_each(|d| *d = next() as u16 & mask);
                codeword(&owned, &data)
            })
            .collect();
        let mut generator = [0_u16; 5];
        let code = CyclicWords::new(field, 12, 4, 1, 1, &mut generator).expect("words");
        let mut room = [0_u16; 64];
        // Warm up both.
        pass(&code, &zeros, &mut room);
        pass(&code, &randoms, &mut room);
        let (mut z, mut r, mut ratios) = (Vec::new(), Vec::new(), Vec::new());
        for round in 0..ROUNDS {
            let (tz, tr) = if round % 2 == 0 {
                let tz = pass(&code, &zeros, &mut room);
                (tz, pass(&code, &randoms, &mut room))
            } else {
                let tr = pass(&code, &randoms, &mut room);
                (pass(&code, &zeros, &mut room), tr)
            };
            z.push(tz);
            r.push(tr);
            ratios.push(tr / tz);
        }
        let ratio = median(ratios.clone());
        failed |= !(0.8..=1.25).contains(&ratio);
        println!(
            "{name}: median ns/call all-zero {:.0}, random {:.0}; median per-round ratio random/zero {:.2} (min {:.2}, max {:.2})",
            median(z),
            median(r),
            median(ratios.clone()),
            ratios.iter().copied().fold(f64::MAX, f64::min),
            ratios.iter().copied().fold(0.0, f64::max)
        );
    }
    if failed {
        std::process::exit(1);
    }
}
