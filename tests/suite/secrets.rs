//! Data can be a secret: the `Debug` text of what holds it gives lengths, not bytes, and a change
//! of a plan is wiped by `Zeroize`. Wiping memory that was given back cannot be seen from safe
//! code; that the buffers come in `Zeroizing` is checked by the types of the calls.

use crate::sets::{one_set, slices};
use reed_solomon_core::{
    Change, ChangeKind, Code, DataDigest, Field, Mark, Pile, Plan, SetFacts, ShardFile, Zeroize,
};

/// Data whose bytes are easy to find in a text: 0xAB is 171.
const SECRET: [u8; 12] = [0xAB; 12];

fn shows_no_byte(text: &str) {
    assert!(!text.contains("171"), "{text}");
    assert!(!text.to_lowercase().contains("ab, ab"), "{text}");
}

#[test]
fn the_debug_text_of_rebuilt_data_gives_its_length_only() {
    let facts =
        SetFacts::for_data(Field::Gf256, b"secret", 3, &SECRET, 4, DataDigest::None).unwrap();
    let files = facts.files(&SECRET, &[0, 1, 2, 3]).unwrap();
    let pile = Pile::new(&slices(&files)).unwrap();
    let rebuilt = one_set(&pile).rebuild(None).unwrap();
    assert_eq!(*rebuilt.data, SECRET);
    let text = format!("{rebuilt:?}");
    shows_no_byte(&text);
    assert!(text.contains("length: 12"), "{text}");
    // The pile, its sets, a file read and a block of it show no byte either.
    shows_no_byte(&format!("{pile:?}"));
    let read = ShardFile::read(&files[0]).unwrap();
    shows_no_byte(&format!("{read:?}"));
    let block = read.block(0).unwrap();
    assert_eq!(format!("{block:?}"), "Intact(4 bytes)");
}

#[test]
fn a_change_shows_its_place_and_kind_and_is_wiped() {
    let code = Code::new(Field::Gf2048, 2).unwrap();
    let data: [&[u8]; 2] = [&[0x00, 0xAB], &[0x00, 0xAB]];
    let set = code.encode(&data, &[0, 1, 2, 3]).unwrap();
    let given: Vec<(u16, &[u8])> = (0..)
        .zip(set.iter().map(|shard| shard.as_slice()))
        .collect();
    let plan = code
        .plan(
            &given,
            &[Mark {
                shard: 1,
                symbol: 0,
            }],
        )
        .unwrap();
    // The list hides itself whole; a change shows its place and kind.
    let text = format!("{plan:?}");
    assert_eq!(text, "Changes(Zeroizing { .. })");
    let Plan::Changes(changes) = plan else {
        panic!("{text}")
    };
    let mut change: Change = changes[0];
    let text = format!("{change:?}");
    shows_no_byte(&text);
    assert_eq!(text, "Change { shard: 1, symbol: 0, kind: Filled, .. }");
    assert_eq!(
        (change.was, change.becomes, change.kind),
        (0xAB, 0xAB, ChangeKind::Filled)
    );
    change.zeroize();
    assert_eq!(
        (change.shard, change.symbol, change.was, change.becomes),
        (0, 0, 0, 0)
    );
}
