//! AUD-006 probe: the GF(2^11) and custom binary-field decoders as a program built with LTO has them.
use reed_solomon_core::{CyclicWords, Field};

/// One MHFE-REPAIR-1 style word put right: 12 data words, 4 repair words, word 2 marked.
#[inline(never)]
fn repair_named(word: &mut [u16; 16], room: &mut [u16]) -> Option<usize> {
    let mut generator = [0_u16; 5];
    let code = CyclicWords::new(Field::Gf2048, 12, 4, 1, 1, &mut generator).ok()?;
    code.correct_word(word, &[2], room).ok().flatten()
}

/// The same in a field chosen by degree, GF(2^10) by its standard polynomial: a `Binary` field.
#[inline(never)]
fn repair_binary(field: Field, word: &mut [u16; 16], room: &mut [u16]) -> Option<usize> {
    let mut generator = [0_u16; 5];
    let code = CyclicWords::new(field, 12, 4, 1, 1, &mut generator).ok()?;
    code.correct_word(word, &[2], room).ok().flatten()
}

fn main() {
    let seed: u16 = std::env::args().count() as u16;
    let mut word = [0_u16; 16];
    for (i, w) in word.iter_mut().enumerate() {
        *w = (seed.wrapping_mul(97).wrapping_add(i as u16 * 131)) & 0x7FF;
    }
    let mut room = [0_u16; 64];
    let named = repair_named(&mut word, &mut room);
    let field = Field::binary(10, None).ok();
    word.iter_mut().for_each(|w| *w &= 0x3FF);
    let binary = field.and_then(|field| repair_binary(field, &mut word, &mut room));
    println!("{named:?} {binary:?} {:?}", field.map(|f| f == Field::Gf2048));
}
