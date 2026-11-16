//! Data can be a secret: the `Debug` text of what holds it gives lengths, not bytes, and a change
//! of a plan carries its place, while its values stay in the plan's list. Wiping memory that was
//! given back cannot be seen from safe code; that the results come in a `Buffer` is checked by
//! the types of the calls.

use crate::sets::{one_set, slices};
use reed_solomon_core::{
    Change, ChangeKind, Code, DataDigest, Field, Mark, Pile, Plan, SetFacts, ShardFile,
};

/// Data whose bytes are easy to find in a text: 0xAB is 171.
const SECRET: [u8; 12] = [0xAB; 12];

/// The flags the kernel lists for the mapping that holds `address`.
#[cfg(all(feature = "protect", target_os = "linux"))]
fn flags_at(address: usize) -> Option<String> {
    let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap();
    let mut inside = false;
    for line in smaps.lines() {
        if let Some(flags) = line.strip_prefix("VmFlags:") {
            if inside {
                return Some(flags.to_owned());
            }
            continue;
        }
        let range = line.split_whitespace().next();
        let ends = range
            .and_then(|range| range.split_once('-'))
            .map(|(start, end)| {
                (
                    usize::from_str_radix(start, 16),
                    usize::from_str_radix(end, 16),
                )
            });
        if let Some((Ok(start), Ok(end))) = ends {
            inside = (start..end).contains(&address);
        }
    }
    None
}

/// With the `protect` feature every result lies in pages of its own, which the system keeps out
/// of core dumps (`dd`) and gives a child process as zeros (`wf`), and which are locked (`lo`)
/// exactly when the buffer says so.
#[cfg(all(feature = "protect", target_os = "linux"))]
#[test]
fn with_protect_a_result_lies_in_pages_kept_out_of_core_dumps() {
    let facts =
        SetFacts::for_data(Field::Gf256, b"secret", 3, &SECRET, 4, DataDigest::None).unwrap();
    let files = facts.files(&SECRET, &[0, 1, 2, 3]).unwrap();
    let pile = Pile::new(&slices(&files)).unwrap();
    let rebuilt = one_set(&pile).rebuild(None).unwrap();
    let code = Code::new(Field::Gf256, 2).unwrap();
    let shards = code
        .encode(&[&SECRET[..6], &SECRET[6..]], &[0, 1, 2])
        .unwrap();
    for bytes in files.iter().chain(&shards).chain([&rebuilt.data]) {
        let flags = flags_at(bytes.as_ptr() as usize).unwrap();
        assert!(flags.contains(" dd") && flags.contains(" wf"), "{flags}");
        assert_eq!(flags.contains(" lo"), bytes.is_locked(), "{flags}");
        assert!(bytes.is_out_of_dumps() && bytes.is_out_of_children());
    }
}

/// Without the feature a result lies in the allocator's memory, which nothing protects.
#[cfg(not(feature = "protect"))]
#[test]
fn without_protect_a_result_says_it_is_not_protected() {
    let code = Code::new(Field::Gf256, 2).unwrap();
    let shards = code
        .encode(&[&SECRET[..6], &SECRET[6..]], &[0, 1, 2])
        .unwrap();
    for shard in &shards {
        assert!(!shard.is_locked() && !shard.is_out_of_dumps() && !shard.is_out_of_children());
    }
}

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
fn a_change_shows_its_place_and_kind_and_its_values_stay_in_the_list() {
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
    // The list hides itself whole; a change carries its place and kind only, and the values are
    // looked at where the list keeps them.
    let text = format!("{plan:?}");
    assert_eq!(text, "Changes(Changes(1))");
    let Plan::Changes(changes) = plan else {
        panic!("{text}")
    };
    let change: Change = changes.get(0).unwrap();
    let text = format!("{change:?}");
    shows_no_byte(&text);
    assert_eq!(text, "Change { shard: 1, symbol: 0, kind: Filled }");
    assert_eq!(change.kind, ChangeKind::Filled);
    let values = changes.values(0, |was, becomes| (was, becomes));
    assert_eq!(values, Some((0xAB, 0xAB)));
}
