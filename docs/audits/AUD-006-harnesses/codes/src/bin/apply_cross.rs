//! A plan is bound to its code and shards: applied by another code (other type, field, threshold,
//! first root, step) or to other shards, it changes nothing and fails with Changes.

use reed_solomon_core::{Code, CodeError, CyclicCode, Field, Plan};

fn main() {
    // A cyclic word of GF(2^11) with one wrong symbol, and its plan.
    let a = CyclicCode::new(Field::Gf2048, 3, 2, 1, 1).unwrap();
    let d: [[u8; 2]; 3] = [[0, 1], [0, 2], [0, 3]];
    let views: Vec<&[u8]> = d.iter().map(|w| w.as_slice()).collect();
    let parity = a.encode(&views).unwrap();
    let mut word: Vec<Vec<u8>> = vec![d[0].to_vec(), d[1].to_vec(), d[2].to_vec(), parity[0].to_vec(), parity[1].to_vec()];
    word[1][1] ^= 0x10;
    let views: Vec<&[u8]> = word.iter().map(Vec::as_slice).collect();
    let Plan::Changes(changes) = a.plan(&views, &[], &[]).unwrap() else { panic!("within the bound") };
    let others = [
        ("first root 2", CyclicCode::new(Field::Gf2048, 3, 2, 2, 1).unwrap()),
        ("step 3", CyclicCode::new(Field::Gf2048, 3, 2, 1, 3).unwrap()),
        ("other polynomial of degree 11", CyclicCode::new(Field::binary(11, Some(0xFE9)).unwrap(), 3, 2, 1, 1).unwrap()),
        ("first root 1 + 2047", CyclicCode::new(Field::Gf2048, 3, 2, 2048, 1).unwrap()),
    ];
    for (label, other) in others {
        let mut out = word.clone();
        let mut b: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
        let got = other.apply(&mut b, &changes);
        drop(b);
        println!("cyclic plan applied by a code with {label}: {got:?}, shards unchanged {}", out == word);
        // First root 2048 is first root 1 in GF(2^11): the same code, whose plan applies.
        let same_code = label == "first root 1 + 2047";
        if got.is_ok() != same_code || (!same_code && (got != Err(CodeError::Changes) || out != word)) {
            aud006_codes_probe::fail();
        }
    }
    // The same code, the same shards: applies.
    let mut out = word.clone();
    let mut b: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
    let own = a.apply(&mut b, &changes);
    println!("cyclic plan applied by its own code: {own:?}");
    if own.is_err() {
        aud006_codes_probe::fail();
    }
    // An evaluation code's plan applied by a cyclic code of the same shards, and the other way.
    let code = Code::new(Field::Gf2048, 3).unwrap();
    let set = code.encode(&[&[0, 1], &[0, 2], &[0, 3]], &[0, 1, 2, 3, 4]).unwrap();
    let mut held: Vec<Vec<u8>> = set.iter().map(|b| b.to_vec()).collect();
    held[2][1] ^= 1;
    let given: Vec<(u16, &[u8])> = (0..5).zip(held.iter().map(Vec::as_slice)).collect();
    let Plan::Changes(point_changes) = code.plan(&given, &[]).unwrap() else { panic!("within the bound") };
    let mut out = held.clone();
    let mut b: Vec<&mut [u8]> = out.iter_mut().map(Vec::as_mut_slice).collect();
    let crossed = a.apply(&mut b, &point_changes);
    println!("point plan applied by a cyclic code: {crossed:?}");
    if crossed != Err(CodeError::Changes) {
        aud006_codes_probe::fail();
    }
    let mut b: Vec<(u16, &mut [u8])> = (0..5).zip(out.iter_mut().map(Vec::as_mut_slice)).collect();
    let crossed = code.apply(&mut b, &changes);
    println!("cyclic plan applied by a point code: {crossed:?}");
    if crossed != Err(CodeError::Changes) {
        aud006_codes_probe::fail();
    }
    let other = Code::new(Field::binary(11, Some(0xFE9)).unwrap(), 3).unwrap();
    let crossed = other.apply(&mut b, &point_changes);
    println!("point plan applied by a point code of another degree-11 field: {crossed:?}");
    if crossed != Err(CodeError::Changes) {
        aud006_codes_probe::fail();
    }
    let shifted = (1..6).zip(held.iter_mut().map(Vec::as_mut_slice)).collect::<Vec<(u16, &mut [u8])>>();
    let mut shifted = shifted;
    println!("point plan applied to the same bytes under other indexes: {:?}", code.apply(&mut shifted, &point_changes));
    assert!(matches!(code.apply(&mut shifted, &point_changes), Err(CodeError::Changes)));
    aud006_codes_probe::finish();
}
