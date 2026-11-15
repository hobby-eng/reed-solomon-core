//! The data and lost files written into the program's own output, as into a mapping of a file:
//! the bytes and steps of the calls that return them in a buffer, and an output that holds none
//! of the data after a refusal.

use crate::sets::{damage, four_bytes_and, one_set, slices, written};
use crate::support::{bytes, plain};
use reed_solomon_core::{
    Code, DataDigest, Field, FileError, Header, Pile, SetError, SetFacts, Written,
};

/// What an output holds before a call, so that a test sees whether the call wrote to it.
const FILL: u8 = 0xA5;

/// Rebuilds the set of `files` both ways and checks that both give `data`.
fn both_ways(files: &[&[u8]], data: &[u8]) {
    let pile = Pile::new(files).unwrap();
    let found = one_set(&pile);
    let rebuilt = found.rebuild(None).unwrap();
    let mut out = vec![FILL; rebuilt.data.len()];
    let written = found.rebuild_into(&mut out, None).unwrap();
    assert_eq!(out, data);
    assert_eq!(*rebuilt.data, *data);
    let expected = Written {
        digest: rebuilt.digest,
        bare: rebuilt.bare,
        steps: rebuilt.steps,
    };
    assert_eq!(written, expected);
}

#[test]
fn the_data_written_in_place_is_the_data_a_rebuild_returns() {
    // k = 3, seven files, shards of nine bytes in blocks of four, one byte of them padding.
    let set = written(Field::Gf256, 41, 3, 7, 26);
    let subsets: [&[usize]; 4] = [
        &[0, 1, 2],
        &[3, 4, 5],
        &[0, 2, 4, 5, 6],
        &[1, 2, 3, 4, 5, 6],
    ];
    for chosen in subsets {
        let files: Vec<&[u8]> = chosen.iter().map(|&at| set.files[at].as_slice()).collect();
        both_ways(&files, &set.data);
    }
    // Damaged blocks move the sources of those blocks to other shards.
    let mut damaged = set.files.clone();
    damage(&mut damaged[0], 1);
    damage(&mut damaged[4], 0);
    both_ways(&slices(&damaged[..5]), &set.data);
    // Two-byte symbols, and data of no bytes.
    let pairs = written(Field::Gf65536, 42, 3, 6, 40);
    both_ways(&slices(&pairs.files[2..5]), &pairs.data);
    let empty = written(Field::Gf256, 43, 2, 3, 0);
    both_ways(&slices(&empty.files[1..]), &[]);
}

/// Data of sixteen bytes in two data shards of two blocks, and a spare shard 2 that agrees in
/// block 0 and differs in block 1: a refusal after one block was written.
fn split_in_block_one() -> Vec<Vec<u8>> {
    let facts = SetFacts::new(Field::Gf256, &[44], 2, 16, 4, None).unwrap();
    let data: [&[u8]; 2] = [&[1, 2, 3, 4, 5, 6, 7, 8], &[9, 10, 11, 12, 13, 14, 15, 16]];
    let code = Code::new(Field::Gf256, 2).unwrap();
    let mut shards = plain(code.encode(&data, &[0, 1, 2]).unwrap());
    shards[2][5] ^= 1;
    (0..3_u16)
        .zip(&shards)
        .map(|(index, shard)| {
            Header::new(facts, index)
                .unwrap()
                .file(shard)
                .unwrap()
                .to_vec()
        })
        .collect()
}

#[test]
fn a_refusal_during_the_work_leaves_zeros_in_the_output() {
    let files = split_in_block_one();
    let pile = Pile::new(&slices(&files)).unwrap();
    let found = one_set(&pile);
    let refusal = SetError::Inconsistent {
        block: 1,
        base: vec![0, 1],
        differing: vec![2],
    };
    let mut out = [FILL; 16];
    assert_eq!(found.rebuild_into(&mut out, None), Err(refusal.clone()));
    assert_eq!(out, [0; 16]);
    // Shard 3 is made in block 0 before block 1 refuses: its file holds zeros, frame and all.
    let length = usize::try_from(found.facts().file_len()).unwrap();
    let mut made = vec![FILL; length];
    assert_eq!(
        found.repairs_into(&[3], &mut [made.as_mut_slice()], None),
        Err(refusal)
    );
    assert_eq!(made, vec![0; length]);

    // Padding that is not zero is found once the last block is written: in a shard a file
    // holds, and in one the code makes from the others.
    let held = four_bytes_and([0, 9]);
    let facts = SetFacts::new(Field::Gf256, &[1], 3, 4, 4, None).unwrap();
    let shards: [&[u8]; 3] = [&[1, 2], &[3, 4], &[0, 9]];
    let code = Code::new(Field::Gf256, 3).unwrap();
    let parity = plain(code.encode(&shards, &[3]).unwrap());
    let made = [
        &held[0],
        &held[1],
        &*Header::new(facts, 3).unwrap().file(&parity[0]).unwrap(),
    ];
    for files in [slices(&held), slices(&made)] {
        let pile = Pile::new(&files).unwrap();
        let mut out = [FILL; 4];
        assert_eq!(
            one_set(&pile).rebuild_into(&mut out, None),
            Err(SetError::Padding)
        );
        assert_eq!(out, [0; 4]);
    }
}

#[test]
fn a_refusal_before_the_work_leaves_the_output_as_it_was() {
    let set = written(Field::Gf256, 45, 3, 7, 24);
    let pile = Pile::new(&slices(&set.files[2..6])).unwrap();
    let found = one_set(&pile);
    let needed = found.rebuild_work().unwrap();
    let untouched = |out: &[u8]| out.iter().all(|&byte| byte == FILL);

    // An output of another length is refused first, whatever else is wrong.
    for length in [23, 25, 0] {
        let mut out = vec![FILL; length];
        assert_eq!(
            found.rebuild_into(&mut out, Some(0)),
            Err(SetError::Output { needed: 24 })
        );
        assert!(untouched(&out));
    }
    let mut out = vec![FILL; 24];
    assert_eq!(
        found.rebuild_into(&mut out, Some(needed - 1)),
        Err(SetError::Work { needed })
    );
    assert!(untouched(&out));
    let few = Pile::new(&slices(&set.files[..2])).unwrap();
    assert!(matches!(
        one_set(&few).rebuild_into(&mut out, None),
        Err(SetError::Shards {
            found: 2,
            needed: 3
        })
    ));
    assert!(untouched(&out));
    assert_eq!(
        found.rebuild_into(&mut out, Some(needed)).unwrap().steps,
        needed
    );
    assert_eq!(out, set.data);
}

#[test]
fn lost_files_written_in_place_are_the_files_repairs_returns() {
    let set = written(Field::Gf256, 46, 3, 7, 24);
    let left: Vec<&[u8]> = [0, 2, 5, 6]
        .iter()
        .map(|&at| set.files[at].as_slice())
        .collect();
    let pile = Pile::new(&left).unwrap();
    let found = one_set(&pile);
    // A repeated index, and one that a file at hand holds.
    let indexes = [4, 1, 3, 1, 0];
    let length = set.files[0].len();
    let mut outs = vec![vec![FILL; length]; indexes.len()];
    let mut views: Vec<&mut [u8]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
    let steps = found.repairs_into(&indexes, &mut views, None).unwrap();
    assert_eq!(found.repairs_work(&indexes), Ok(steps));
    for (&index, out) in indexes.iter().zip(&outs) {
        assert_eq!(*out, set.files[usize::from(index)]);
    }
    assert_eq!(plain(found.repairs(&indexes, None).unwrap()), outs);
    assert_eq!(found.repairs_into(&[], &mut [], None), Ok(0));
}

#[test]
fn outputs_for_lost_files_are_checked_before_any_work() {
    let set = written(Field::Gf256, 47, 3, 7, 24);
    let left: Vec<&[u8]> = [0, 2, 5, 6]
        .iter()
        .map(|&at| set.files[at].as_slice())
        .collect();
    let pile = Pile::new(&left).unwrap();
    let found = one_set(&pile);
    let length = set.files[0].len();
    let needed = u64::try_from(length).unwrap();
    let mut outs = vec![vec![FILL; length]; 3];
    let untouched = |outs: &[Vec<u8>]| outs.iter().flatten().all(|&byte| byte == FILL);
    {
        let mut views: Vec<&mut [u8]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
        // One output too few, then one too many.
        let refusal = Err(SetError::Output { needed });
        assert_eq!(
            found.repairs_into(&[4, 1, 3], &mut views[..2], None),
            refusal
        );
        assert_eq!(found.repairs_into(&[4, 1], &mut views, None), refusal);
        // A number outside the field, then a limit below the work.
        assert_eq!(
            found.repairs_into(&[4, 1, 300], &mut views, None),
            Err(SetError::Index)
        );
        let work = found.repairs_work(&[4, 1, 3]).unwrap();
        assert_eq!(
            found.repairs_into(&[4, 1, 3], &mut views, Some(work - 1)),
            Err(SetError::Work { needed: work })
        );
    }
    assert!(untouched(&outs));
    // Too few files, then a block that too few hold.
    {
        let mut views: Vec<&mut [u8]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
        let few = Pile::new(&left[..2]).unwrap();
        assert!(matches!(
            one_set(&few).repairs_into(&[4, 1, 3], &mut views, None),
            Err(SetError::Shards { .. })
        ));
        let mut damaged: Vec<Vec<u8>> = left.iter().map(|file| file.to_vec()).collect();
        damage(&mut damaged[0], 1);
        damage(&mut damaged[1], 1);
        let short = Pile::new(&slices(&damaged)).unwrap();
        assert!(matches!(
            one_set(&short).repairs_into(&[4, 1, 3], &mut views, None),
            Err(SetError::Blocks { .. })
        ));
    }
    assert!(untouched(&outs));
    // One output a byte short.
    outs[1].pop();
    let mut views: Vec<&mut [u8]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
    assert_eq!(
        found.repairs_into(&[4, 1, 3], &mut views, None),
        Err(SetError::Output { needed })
    );
    drop(views);
    assert!(untouched(&outs));
}

#[test]
fn the_files_of_a_set_written_in_place_are_those_it_returns() {
    // Data that fills its shards, data that ends in the first shard, and no data at all.
    let shapes = [
        (Field::Gf256, 3, 26, 4),
        (Field::Gf65536, 2, 40, 8),
        (Field::Gf256, 4, 3, 4),
    ];
    for (field, threshold, length, block) in shapes.into_iter().chain([(Field::Gf256, 2, 0, 4)]) {
        let data = bytes(length as u64 + 50, length);
        let digest = DataDigest::Sha256;
        let facts =
            SetFacts::for_data(field, b"in place", threshold, &data, block, digest).unwrap();
        let length = usize::try_from(facts.file_len()).unwrap();
        // All, a repeated index, the data shards alone, the made ones alone, and none.
        let made = u16::try_from(threshold).unwrap();
        let lists: [&[u16]; 5] = [
            &[0, 1, 2, 3, 4],
            &[made, 0, made],
            &[0, 1],
            &[made + 1],
            &[],
        ];
        for indexes in lists {
            let expected = plain(facts.files(&data, indexes).unwrap());
            let mut outs = vec![vec![FILL; length]; indexes.len()];
            let mut views: Vec<&mut [u8]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
            facts.files_into(&data, indexes, &mut views).unwrap();
            assert_eq!(outs, expected, "{field:?} {threshold} {indexes:?}");
        }
    }
}

#[test]
fn outputs_for_the_files_of_a_set_are_checked_first() {
    let data = b"twenty-six bytes of a text";
    let digest = DataDigest::Sha256;
    let facts = SetFacts::for_data(Field::Gf256, b"in place", 3, data, 8, digest).unwrap();
    let needed = facts.file_len();
    let mut outs = vec![vec![FILL; usize::try_from(needed).unwrap()]; 2];
    let mut views: Vec<&mut [u8]> = outs.iter_mut().map(Vec::as_mut_slice).collect();
    let output = Err(FileError::Output { needed });
    assert_eq!(facts.files_into(b"short", &[0, 1, 2], &mut views), output);
    assert_eq!(
        facts.files_into(&data[1..], &[0, 1], &mut views),
        Err(FileError::Data)
    );
    let other = b"another text, of 26 bytes!";
    assert_eq!(
        facts.files_into(other, &[0, 1], &mut views),
        Err(FileError::Digest)
    );
    assert_eq!(
        facts.files_into(data, &[0, 300], &mut views),
        Err(FileError::Index)
    );
    drop(views);
    assert!(outs.iter().flatten().all(|&byte| byte == FILL));
}

#[test]
fn a_header_writes_its_file_in_place() {
    let facts = SetFacts::new(Field::Gf256, &[7], 2, 10, 4, None).unwrap();
    let header = Header::new(facts, 1).unwrap();
    let shard = [1, 2, 3, 4, 5];
    let file = header.file(&shard).unwrap();
    let mut out = vec![FILL; file.len()];
    header.file_into(&shard, &mut out).unwrap();
    assert_eq!(out, *file);
    // Another length of the output is refused first, then one of the shard; neither writes.
    let mut out = vec![FILL; file.len() + 1];
    let needed = facts.file_len();
    assert_eq!(
        header.file_into(&[1], &mut out),
        Err(FileError::Output { needed })
    );
    let mut out = vec![FILL; file.len()];
    assert_eq!(header.file_into(&[1], &mut out), Err(FileError::Shard));
    assert!(out.iter().all(|&byte| byte == FILL));
}
