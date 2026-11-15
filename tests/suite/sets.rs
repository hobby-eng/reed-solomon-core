//! Sets of shard files: written from data, sorted out of a pile, rebuilt block by block from
//! whichever files hold each block, and made whole again.

use crate::support::{bytes, hex, plain};
use reed_solomon_core::{
    Block, Coverage, DigestCheck, Dispute, Field, FileError, FileSet, Header, Pile, SetError,
    SetFacts, ShardFile, Stretch,
};

/// A set as written: its facts, its data and its files, numbered from 0.
pub(crate) struct Written {
    pub(crate) facts: SetFacts,
    pub(crate) data: Vec<u8>,
    pub(crate) files: Vec<Vec<u8>>,
}

/// A set of `count` files with `data_len` bytes of data.
pub(crate) fn written(
    field: Field,
    id: u8,
    threshold: usize,
    count: u16,
    data_len: usize,
) -> Written {
    let block_len = 4;
    let data = bytes(u64::from(id) * 1000 + data_len as u64, data_len);
    let facts = SetFacts::new(field, &[id], threshold, data_len as u64, block_len, None).unwrap();
    let indexes: Vec<u16> = (0..count).collect();
    let files = plain(facts.files(&data, &indexes).unwrap());
    Written { facts, data, files }
}

/// The one set of the pile of `files`.
pub(crate) fn one_set<'p, 'a>(pile: &'p Pile<'a>) -> &'p FileSet<'a> {
    assert_eq!(pile.sets().len(), 1);
    &pile.sets()[0]
}

/// The files as the slices a pile takes.
pub(crate) fn slices<T: AsRef<[u8]>>(files: &[T]) -> Vec<&[u8]> {
    files.iter().map(AsRef::as_ref).collect()
}

/// Changes one bit in block `block` of `file`, so that its checksum no longer holds.
pub(crate) fn damage(file: &mut [u8], block: usize) {
    let (header, each) = {
        let read = ShardFile::read(file).unwrap();
        let header = read.header();
        let each = usize::try_from(header.set().block_len()).unwrap() + 4;
        (header.as_bytes().len(), each)
    };
    file[header + block * each] ^= 1;
}

/// The shard a file holds: its blocks in order.
pub(crate) fn shard_of(file: &[u8]) -> Vec<u8> {
    let read = ShardFile::read(file).unwrap();
    let blocks = u32::try_from(read.header().set().blocks()).unwrap();
    let mut shard = Vec::new();
    for number in 0..blocks {
        let found = read.block(number);
        assert!(matches!(found, Some(Block::Intact(_))));
        if let Some(Block::Intact(block)) = found {
            shard.extend_from_slice(block);
        }
    }
    shard
}

/// The file of shard `index` with one byte changed in each of `blocks` and every checksum right:
/// a file that reads without a fault and holds other bytes.
pub(crate) fn forged(set: &Written, index: u16, blocks: &[usize]) -> Vec<u8> {
    let mut shard = shard_of(&set.files[usize::from(index)]);
    for &block in blocks {
        shard[block * 4] ^= 0x55;
    }
    Header::new(set.facts, index)
        .unwrap()
        .file(&shard)
        .unwrap()
        .to_vec()
}

/// The stretches of a set as (first block, block after the last, holders), checked against
/// the sums of its coverage.
fn stretches(set: &FileSet<'_>) -> Vec<(u64, u64, usize)> {
    let found: Vec<Stretch> = set.stretches().unwrap().collect();
    let coverage = set.coverage();
    let blocks = |keep: &dyn Fn(usize) -> bool| -> u64 {
        let kept = found.iter().filter(|stretch| keep(stretch.holders));
        kept.map(|stretch| stretch.blocks.end - stretch.blocks.start)
            .sum()
    };
    assert_eq!(blocks(&|_| true), coverage.blocks);
    assert_eq!(blocks(&|holders| holders < coverage.needed), coverage.short);
    assert_eq!(blocks(&|holders| holders == coverage.needed), coverage.bare);
    // One after another, from the first block, and no two neighbours with one number.
    let mut next = 0;
    for pair in found.windows(2) {
        assert_ne!(pair[0].holders, pair[1].holders);
    }
    for stretch in &found {
        assert_eq!(stretch.blocks.start, next);
        assert!(stretch.blocks.end > next);
        next = stretch.blocks.end;
    }
    found
        .iter()
        .map(|stretch| (stretch.blocks.start, stretch.blocks.end, stretch.holders))
        .collect()
}

/// What a set of a pile comes to: its facts, whether another set has its identifier, the
/// indexes of its files and its data.
type Summary = (SetFacts, bool, Vec<u16>, Option<Vec<u8>>);

/// The sets of a pile, in its order, without the places of their files.
fn summary(pile: &Pile<'_>) -> Vec<Summary> {
    pile.sets()
        .iter()
        .map(|set| {
            let indexes = set.members().iter().map(|member| member.index).collect();
            let data = set.rebuild(None).ok().map(|rebuilt| rebuilt.data.to_vec());
            (*set.facts(), set.id_shared(), indexes, data)
        })
        .collect()
}

/// Every choice of `count` of the numbers below `of`.
fn choices(count: usize, of: usize) -> Vec<Vec<usize>> {
    (0_u32..1 << of)
        .filter(|mask| mask.count_ones() as usize == count)
        .map(|mask| (0..of).filter(|bit| mask >> bit & 1 == 1).collect())
        .collect()
}

/// Shard 2 of the seventeen bytes 00 to 10 with threshold 3: the file of 92 bytes whose
/// checksums and SHA-256 were computed apart from the crate.
#[test]
fn the_writer_cuts_the_data_in_order_and_fills_up_with_zeros() {
    let data: Vec<u8> = (0..=0x10).collect();
    let facts = SetFacts::new(Field::Gf256, &[1], 3, 17, 4, None).unwrap();
    let before = "5348524401010001010000000300000002000000000000001100000004";
    let expected = format!(
        "{before} 6DF8838D 0C0D0E0F EBBF9719 1000 3D1068C0 {before} 6DF8838D 53485254 00000021 62E47E4A"
    );
    let files = facts.files(&data, &[2, 0, 1]).unwrap();
    assert_eq!(*files[0], hex(&expected));
    assert_eq!(shard_of(&files[1]), hex("000102030405"));
    assert_eq!(shard_of(&files[2]), hex("060708090A0B"));

    // Data that fills its shards, data that ends in the middle of one, data that never reaches
    // the last ones, and no data.
    for (data_len, shards) in [(6, ["0102", "0304", "0506"]), (5, ["0102", "0304", "0500"])] {
        let data = hex("010203040506");
        let facts = SetFacts::new(Field::Gf256, &[1], 3, data_len, 4, None).unwrap();
        let files = facts.files(&data[..data_len as usize], &[0, 1, 2]).unwrap();
        for (file, shard) in files.iter().zip(shards) {
            assert_eq!(shard_of(file), hex(shard));
        }
    }
    let facts = SetFacts::new(Field::Gf65536, &[1], 3, 1, 4, None).unwrap();
    let files = facts.files(&[0xAB], &[0, 1, 2]).unwrap();
    assert_eq!(
        files.iter().map(|file| shard_of(file)).collect::<Vec<_>>(),
        [hex("AB00"), hex("0000"), hex("0000")]
    );
    let empty = SetFacts::new(Field::Gf256, &[1], 3, 0, 4, None).unwrap();
    assert_eq!(empty.files(&[], &[0, 5]).unwrap().len(), 2);

    // No file asked for: an empty list that took no memory; the data is still checked first.
    let none = facts.files(&[0xAB], &[]).unwrap();
    assert_eq!((none.len(), none.capacity()), (0, 0));
    assert_eq!(facts.files(&[], &[]), Err(FileError::Data));
    assert_eq!(facts.files(&[], &[0]), Err(FileError::Data));
    assert_eq!(facts.files(&[1, 2], &[0]), Err(FileError::Data));
    assert_eq!(empty.files(&[], &[0, 256]), Err(FileError::Index));
}

#[test]
fn any_k_files_of_a_set_rebuild_its_data() {
    for field in [Field::Gf256, Field::Gf65536] {
        for (threshold, count) in [(1, 3), (2, 4), (3, 5), (4, 6)] {
            for data_len in [1, 7, 16, 17, 100] {
                let set = written(field, 1, threshold, count, data_len);
                for chosen in choices(threshold, usize::from(count)) {
                    let files: Vec<&[u8]> = chosen.iter().map(|&at| &set.files[at][..]).collect();
                    let pile = Pile::new(&files).unwrap();
                    let found = one_set(&pile);
                    assert_eq!(found.facts(), &set.facts);
                    let rebuilt = found.rebuild(None).unwrap();
                    assert_eq!(*rebuilt.data, set.data);
                    assert_eq!(rebuilt.digest, DigestCheck::Unchecked);
                    // Exactly k files: every block rebuilt with nothing to compare it with.
                    assert_eq!(rebuilt.bare, set.facts.blocks());
                    assert_eq!(found.rebuild_work(), Ok(rebuilt.steps));
                }
                // Every file at once: each block is confirmed by the spare ones.
                let pile = Pile::new(&slices(&set.files)).unwrap();
                let rebuilt = one_set(&pile).rebuild(None).unwrap();
                assert_eq!((rebuilt.data.to_vec(), rebuilt.bare), (set.data, 0));
            }
        }
    }
}

/// Five files, each with another block damaged: no three files are whole, and every block is
/// still held by four.
#[test]
fn every_block_is_taken_from_the_files_that_hold_it() {
    let mut set = written(Field::Gf256, 1, 3, 5, 60);
    assert_eq!(set.facts.blocks(), 5);
    for (at, file) in set.files.iter_mut().enumerate() {
        damage(file, at);
    }
    let pile = Pile::new(&slices(&set.files)).unwrap();
    let found = one_set(&pile);
    for member in found.members() {
        assert_eq!((member.survey.intact, member.survey.damaged), (4, 1));
        assert!(!member.copy);
    }
    let coverage = Coverage {
        shards: 5,
        needed: 3,
        blocks: 5,
        short: 0,
        first_short: None,
        bare: 0,
    };
    assert_eq!(found.coverage(), coverage);
    assert_eq!(stretches(found), [(0, 5, 4)]);
    assert_eq!(*found.rebuild(None).unwrap().data, set.data);
    // The lost file of every shard is made again, byte for byte.
    let whole = written(Field::Gf256, 1, 3, 5, 60);
    for index in 0..5 {
        assert_eq!(
            *found.repair(index, None).unwrap(),
            whole.files[usize::from(index)]
        );
    }
}

#[test]
fn a_block_with_fewer_holders_than_the_threshold_stops_the_rebuild() {
    let mut set = written(Field::Gf256, 1, 3, 5, 60);
    for file in &mut set.files[..3] {
        damage(file, 2);
    }
    let pile = Pile::new(&slices(&set.files)).unwrap();
    let found = one_set(&pile);
    assert_eq!(
        (found.coverage().short, found.coverage().first_short),
        (1, Some(2))
    );
    let short = SetError::Blocks { short: 1, first: 2 };
    assert_eq!(found.rebuild(None), Err(short.clone()));
    assert_eq!(found.repair(0, None), Err(short.clone()));
    assert_eq!(found.rebuild_work(), Err(short));
    // Which parts can still be made: two blocks before the short one and two after it.
    assert_eq!(stretches(found), [(0, 2, 5), (2, 3, 2), (3, 5, 5)]);

    // Every file cut after its second block: the three blocks no file reaches are one stretch.
    let set = written(Field::Gf256, 1, 3, 5, 60);
    let cut: Vec<&[u8]> = set.files.iter().map(|file| &file[..33 + 2 * 8]).collect();
    let pile = Pile::new(&cut).unwrap();
    let coverage = one_set(&pile).coverage();
    assert_eq!(
        (coverage.short, coverage.first_short, coverage.bare),
        (3, Some(2), 0)
    );
    assert_eq!(stretches(one_set(&pile)), [(0, 2, 5), (2, 5, 0)]);

    // The second block damaged in every one of those files as well: it joins the stretch that
    // no file reaches.
    let mut cut: Vec<Vec<u8>> = cut.iter().map(|file| file.to_vec()).collect();
    for file in &mut cut {
        file[33 + 8] ^= 1;
    }
    let pile = Pile::new(&slices(&cut)).unwrap();
    assert_eq!(stretches(one_set(&pile)), [(0, 1, 5), (1, 5, 0)]);

    // A set of no data has no block and no stretch; a whole set is one stretch.
    let empty = written(Field::Gf256, 1, 3, 4, 0);
    let pile = Pile::new(&slices(&empty.files)).unwrap();
    assert_eq!(stretches(one_set(&pile)), []);
    let whole = written(Field::Gf65536, 1, 3, 4, 60);
    let pile = Pile::new(&slices(&whole.files)).unwrap();
    assert_eq!(stretches(one_set(&pile)), [(0, 5, 4)]);
}

/// One file never stands for a set, not even for a set of no data, whose header says it all.
#[test]
fn fewer_different_shards_than_the_threshold_never_rebuild() {
    let empty = written(Field::Gf256, 1, 3, 4, 0);
    let few = SetError::Shards {
        found: 1,
        needed: 3,
    };
    let one: [&[u8]; 1] = [&empty.files[2]];
    let pile = Pile::new(&one).unwrap();
    assert_eq!(one_set(&pile).rebuild(None), Err(few.clone()));
    let pile = Pile::new(&[one[0], one[0], one[0]]).unwrap();
    assert_eq!(one_set(&pile).rebuild(None), Err(few));
    assert_eq!(one_set(&pile).coverage().shards, 1);
    let pile = Pile::new(&slices(&empty.files[1..])).unwrap();
    let rebuilt = one_set(&pile).rebuild(None).unwrap();
    assert_eq!((rebuilt.data.len(), rebuilt.bare, rebuilt.steps), (0, 0, 0));
    assert_eq!(one_set(&pile).rebuild_work(), Ok(0));

    let set = written(Field::Gf256, 1, 3, 4, 20);
    let pile = Pile::new(&slices(&set.files[..2])).unwrap();
    let two = SetError::Shards {
        found: 2,
        needed: 3,
    };
    assert_eq!(one_set(&pile).rebuild(None), Err(two.clone()));
    assert_eq!(one_set(&pile).repair(2, None), Err(two));
}

#[test]
fn the_same_shard_twice_counts_once_and_its_files_fill_each_other_in() {
    let set = written(Field::Gf256, 1, 3, 3, 36);
    // The same file twice, beside two others.
    let files: [&[u8]; 4] = [&set.files[0], &set.files[1], &set.files[0], &set.files[2]];
    let pile = Pile::new(&files).unwrap();
    let found = one_set(&pile);
    assert_eq!(found.coverage().shards, 3);
    let rows: Vec<(usize, u16, bool)> = found
        .members()
        .iter()
        .map(|member| (member.position, member.index, member.copy))
        .collect();
    assert_eq!(
        rows,
        [(0, 0, true), (2, 0, true), (1, 1, false), (3, 2, false)]
    );
    assert!(found.disputes().is_empty());
    assert_eq!(*found.rebuild(None).unwrap().data, set.data);

    // Two files of shard 0, each with another block damaged: together they hold the shard.
    let (mut first, mut second) = (set.files[0].clone(), set.files[0].clone());
    damage(&mut first, 0);
    damage(&mut second, 2);
    let files: [&[u8]; 4] = [&first, &second, &set.files[1], &set.files[2]];
    let pile = Pile::new(&files).unwrap();
    let found = one_set(&pile);
    assert_eq!((found.coverage().short, found.coverage().bare), (0, 3));
    assert!(found.disputes().is_empty());
    assert_eq!(*found.rebuild(None).unwrap().data, set.data);
}

#[test]
fn files_of_one_shard_that_differ_are_not_used_for_the_block_they_differ_on() {
    let set = written(Field::Gf256, 1, 3, 4, 36);
    let other = forged(&set, 0, &[1]);
    let disputed = Dispute {
        index: 0,
        blocks: 1,
        first: 1,
        files: vec![1, 4],
    };
    // With three other shards the set rebuilds without shard 0 in block 1, and the dispute
    // stays in the report.
    let files: [&[u8]; 5] = [
        &set.files[1],
        &other,
        &set.files[2],
        &set.files[3],
        &set.files[0],
    ];
    let pile = Pile::new(&files).unwrap();
    let found = one_set(&pile);
    assert_eq!(found.disputes(), core::slice::from_ref(&disputed));
    let rebuilt = found.rebuild(None).unwrap();
    assert_eq!((rebuilt.data.to_vec(), rebuilt.bare), (set.data.clone(), 1));
    // With two other shards the disputed block has two holders.
    let files: [&[u8]; 5] = [
        &set.files[1],
        &other,
        &set.files[2],
        &set.files[1],
        &set.files[0],
    ];
    let pile = Pile::new(&files).unwrap();
    let found = one_set(&pile);
    assert_eq!(found.disputes(), [disputed]);
    assert_eq!(
        found.rebuild(None),
        Err(SetError::Blocks { short: 1, first: 1 })
    );
}

/// Three files of shard 0: two the same and one with other bytes in two blocks. Whichever comes
/// first, all three took part, the same blocks are named, and the set rebuilds from the other
/// shards.
#[test]
fn the_report_of_files_that_differ_does_not_depend_on_their_order() {
    let set = written(Field::Gf256, 1, 3, 4, 36);
    let other = forged(&set, 0, &[1, 2]);
    let copies: [&[u8]; 3] = [&set.files[0], &set.files[0], &other];
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for order in orders {
        // The three files of shard 0 in this order, then the other shards.
        let mut files: Vec<&[u8]> = order.iter().map(|&copy| copies[copy]).collect();
        files.extend(set.files[1..].iter().map(|bytes| bytes.as_slice()));
        let pile = Pile::new(&files).unwrap();
        let found = one_set(&pile);
        assert_eq!(found.disputes().len(), 1);
        let dispute = &found.disputes()[0];
        assert_eq!((dispute.index, dispute.blocks, dispute.first), (0, 2, 1));
        // The places lead back to the same three files.
        let mut took_part: Vec<usize> = dispute.files.iter().map(|&place| order[place]).collect();
        took_part.sort_unstable();
        assert_eq!(took_part, [0, 1, 2], "order {order:?}");
        assert!(found.disputed_blocks(0).eq([1, 2]));
        assert_eq!(found.disputed_blocks(1).count(), 0);
        assert_eq!(found.disputed_blocks(77).count(), 0);
        // Block 0 has four holders; blocks 1 and 2 the three other shards.
        assert_eq!(stretches(found), [(0, 1, 4), (1, 3, 3)]);
        assert_eq!(*found.rebuild(None).unwrap().data, set.data);
    }
}

/// An index the field does not have is refused first, by the count as by the call, whatever
/// else is wrong with the set.
#[test]
fn an_index_the_field_does_not_have_is_refused_before_anything_else() {
    let whole = written(Field::Gf256, 1, 3, 4, 20);
    let empty = written(Field::Gf256, 2, 3, 4, 0);
    let piles = [
        Pile::new(&slices(&whole.files)).unwrap(),
        Pile::new(&slices(&whole.files[..2])).unwrap(),
        Pile::new(&slices(&empty.files[..1])).unwrap(),
    ];
    for pile in &piles {
        let found = one_set(pile);
        assert_eq!(found.repair_work(256), Err(SetError::Index));
        assert_eq!(found.repair(256, None), Err(SetError::Index));
        assert_eq!(found.repair(256, Some(0)), Err(SetError::Index));
        assert_eq!(found.repair_work(255).is_ok(), found.rebuild_work().is_ok());
    }
    let pairs = written(Field::Gf65536, 1, 3, 4, 20);
    let pile = Pile::new(&slices(&pairs.files)).unwrap();
    assert!(one_set(&pile).repair_work(65_535).is_ok());
    assert!(one_set(&pile).repair(256, None).is_ok());
}

/// Every file reads without a fault, and one holds other bytes: the spare shards show that the
/// files are not one set, and nothing shows which file it is.
#[test]
fn shards_that_are_not_one_set_are_refused_and_named() {
    let set = written(Field::Gf256, 1, 3, 5, 36);
    let rebuild = |files: &[&[u8]]| {
        let pile = Pile::new(files).unwrap();
        one_set(&pile).rebuild(None)
    };
    let sound = slices(&set.files);

    // A spare shard is the wrong one: it differs from what the three lowest make.
    let wrong = forged(&set, 4, &[1]);
    let mut files = sound.clone();
    files[4] = &wrong;
    let spare = SetError::Inconsistent {
        block: 1,
        base: vec![0, 1, 2],
        differing: vec![4],
    };
    assert_eq!(rebuild(&files), Err(spare.clone()));
    files.reverse();
    assert_eq!(rebuild(&files), Err(spare));

    // One of the three lowest is the wrong one: both spare shards differ from what they make.
    let wrong = forged(&set, 1, &[2]);
    let mut files = sound.clone();
    files[1] = &wrong;
    let base = SetError::Inconsistent {
        block: 2,
        base: vec![0, 1, 2],
        differing: vec![3, 4],
    };
    assert_eq!(rebuild(&files), Err(base.clone()));
    files.rotate_left(2);
    assert_eq!(rebuild(&files), Err(base));
    let pile = Pile::new(&files).unwrap();
    assert!(matches!(
        one_set(&pile).repair(0, None),
        Err(SetError::Inconsistent { .. })
    ));

    // With exactly three files nothing is left to compare: other data comes back, marked as
    // unconfirmed in every block. This is what the digest of the data is for.
    let three: [&[u8]; 3] = [sound[0], &wrong, sound[2]];
    let rebuilt = rebuild(&three).unwrap();
    assert_ne!(*rebuilt.data, set.data);
    assert_eq!(rebuilt.bare, 3);
}

/// The files 0, 1 and 2 of four bytes of data, 1 to 4, in three shards of two, with `last` as
/// shard 2, which the data never reaches: its bytes are padding only.
pub(crate) fn four_bytes_and(last: [u8; 2]) -> Vec<Vec<u8>> {
    let facts = SetFacts::new(Field::Gf256, &[1], 3, 4, 4, None).unwrap();
    let file = |index: u16, shard: &[u8]| Header::new(facts, index).unwrap().file(shard).unwrap();
    plain([file(0, &[1, 2]), file(1, &[3, 4]), file(2, &last)])
}

/// Four bytes of data in three shards of two: shard 2 is zero bytes only.
#[test]
fn bytes_after_the_data_that_are_not_zero_are_refused() {
    let files = four_bytes_and([0, 0]);
    let pile = Pile::new(&slices(&files)).unwrap();
    assert_eq!(*one_set(&pile).rebuild(None).unwrap().data, [1, 2, 3, 4]);

    let files = four_bytes_and([0, 9]);
    let pile = Pile::new(&slices(&files)).unwrap();
    assert_eq!(one_set(&pile).rebuild(None), Err(SetError::Padding));
    // Making one shard does not put the data together and does not look at those bytes.
    assert_eq!(*one_set(&pile).repair(0, None).unwrap(), files[0]);
}

#[test]
fn a_pile_sorts_its_files_into_sets_whatever_their_order() {
    let first = written(Field::Gf256, 1, 2, 3, 20);
    let second = written(Field::Gf65536, 2, 2, 3, 20);
    // The identifier of the first set with other facts: one of the two is not what it says.
    let twin = written(Field::Gf256, 1, 2, 3, 24);
    let mut broken = first.files[0].clone();
    broken[..40].fill(0);
    broken[95..].fill(0);
    let files: [&[u8]; 9] = [
        &second.files[1],
        b"not a shard file",
        &first.files[2],
        &twin.files[0],
        &second.files[0],
        &broken,
        &first.files[1],
        &twin.files[2],
        &[],
    ];
    let pile = Pile::new(&files).unwrap();
    let unread: Vec<(usize, FileError)> = pile
        .readings()
        .iter()
        .enumerate()
        .filter_map(|(at, reading)| reading.err().map(|refusal| (at, refusal)))
        .collect();
    // The file with its ends wiped still shows the mark of its trailer: a damaged shard file.
    assert_eq!(
        unread,
        [
            (1, FileError::Unrecognized),
            (5, FileError::Damaged),
            (8, FileError::Unrecognized)
        ]
    );
    let found = summary(&pile);
    assert_eq!(found.len(), 3);
    for (facts, shared, indexes, data) in &found {
        let (written, expected) = if facts == &first.facts {
            (&first, vec![1, 2])
        } else if facts == &twin.facts {
            (&twin, vec![0, 2])
        } else {
            (&second, vec![0, 1])
        };
        assert_eq!(facts, &written.facts);
        assert_eq!(*shared, facts.id() == [1]);
        assert_eq!((indexes, data), (&expected, &Some(written.data.clone())));
    }
    let positions: Vec<Vec<usize>> = pile
        .sets()
        .iter()
        .map(|set| set.members().iter().map(|member| member.position).collect())
        .collect();
    assert!(positions.contains(&vec![6, 2]));
    assert!(positions.contains(&vec![4, 0]));

    // The same files in other orders give the same sets in the same order.
    let mut turned = files;
    turned.reverse();
    assert_eq!(summary(&Pile::new(&turned).unwrap()), found);
    turned.rotate_left(4);
    turned.swap(0, 3);
    assert_eq!(summary(&Pile::new(&turned).unwrap()), found);
}

/// The steps follow the rule by hand: nine to prepare three sources, and for every block made
/// or compared three sources times its four symbols plus sixty-four.
#[test]
fn the_steps_are_counted_by_the_rule_and_the_limit_is_checked_first() {
    const PREPARE: u128 = 9;
    const BLOCK: u128 = 3 * (4 + 64);
    let set = written(Field::Gf256, 1, 3, 5, 24);
    assert_eq!(set.facts.blocks(), 2);
    let steps = |files: &[usize]| {
        let files: Vec<&[u8]> = files.iter().map(|&at| &set.files[at][..]).collect();
        let pile = Pile::new(&files).unwrap();
        let found = one_set(&pile);
        let rebuilt = found.rebuild(None).unwrap();
        assert_eq!(*rebuilt.data, set.data);
        assert_eq!(found.rebuild_work(), Ok(rebuilt.steps));
        rebuilt.steps
    };
    // The data files alone: nothing is made and nothing compared; the sources are prepared.
    assert_eq!(steps(&[0, 1, 2]), PREPARE);
    // One data shard is made in each of the two blocks.
    assert_eq!(steps(&[1, 2, 3]), PREPARE + 2 * BLOCK);
    // Two spare shards are compared in each block.
    assert_eq!(steps(&[0, 1, 2, 3, 4]), PREPARE + 4 * BLOCK);

    let pile = Pile::new(&slices(&set.files[..4])).unwrap();
    let found = one_set(&pile);
    // Shard 3 is at hand: compared once, then copied. Shard 4 is compared with nothing: made.
    assert_eq!(found.repair_work(3), Ok(PREPARE + 2 * BLOCK));
    assert_eq!(found.repair_work(4), Ok(PREPARE + 4 * BLOCK));
    assert_eq!(found.repair_work(0), Ok(PREPARE + 2 * BLOCK));

    let needed = PREPARE + 2 * BLOCK;
    assert_eq!(found.rebuild_work(), Ok(needed));
    assert_eq!(
        found.rebuild(Some(needed - 1)),
        Err(SetError::Work { needed })
    );
    assert_eq!(
        found.repair(3, Some(needed - 1)),
        Err(SetError::Work { needed })
    );
    assert_eq!(found.rebuild(Some(0)), Err(SetError::Work { needed }));
    assert_eq!(found.rebuild(Some(needed)).unwrap().steps, needed);
    assert_eq!(*found.repair(3, Some(needed)).unwrap(), set.files[3]);

    // Two-byte symbols: two sources, and one spare shard compared in two blocks of two symbols.
    let pairs = written(Field::Gf65536, 1, 2, 3, 16);
    let pile = Pile::new(&slices(&pairs.files)).unwrap();
    assert_eq!(one_set(&pile).rebuild_work(), Ok(4 + 2 * 2 * (2 + 64)));
}

/// Shard 0 lacks its middle block: the sources are 0, 1, 2, then 1, 2, 3, then 0, 1, 2 again,
/// and each change is paid for, the return to the first ones too.
#[test]
fn sources_that_change_and_come_back_are_prepared_each_time() {
    let mut set = written(Field::Gf256, 1, 3, 4, 36);
    assert_eq!(set.facts.blocks(), 3);
    damage(&mut set.files[0], 1);
    let pile = Pile::new(&slices(&set.files)).unwrap();
    let found = one_set(&pile);
    let rebuilt = found.rebuild(None).unwrap();
    assert_eq!(*rebuilt.data, set.data);
    // Three preparations; a spare shard compared in the outer blocks, shard 0 made in the
    // middle one.
    assert_eq!(rebuilt.steps, 3 * 9 + 3 * 3 * (4 + 64));
    assert_eq!(found.rebuild_work(), Ok(rebuilt.steps));
    assert_eq!(rebuilt.bare, 1);
}

#[test]
fn a_lost_file_is_made_again_and_a_set_is_given_more_files() {
    for field in [Field::Gf256, Field::Gf65536] {
        let set = written(field, 1, 3, 6, 50);
        for lost in 0..6 {
            let left: Vec<&[u8]> = (0..6)
                .filter(|&at| at != lost)
                .map(|at| &set.files[at][..])
                .collect();
            let pile = Pile::new(&left).unwrap();
            let again = one_set(&pile)
                .repair(u16::try_from(lost).unwrap(), None)
                .unwrap();
            assert_eq!(*again, set.files[lost]);
        }
        // Files the set never had are the ones its writer would have written.
        let pile = Pile::new(&slices(&set.files[3..])).unwrap();
        let found = one_set(&pile);
        for index in [6, 7, 200, 255] {
            let made = found.repair(index, None).unwrap();
            assert_eq!(vec![made], set.facts.files(&set.data, &[index]).unwrap());
        }
    }
    // A file of a new index from the data files: made in four blocks of four symbols and one
    // of one.
    let data_files = written(Field::Gf256, 1, 3, 3, 50);
    let pile = Pile::new(&slices(&data_files.files)).unwrap();
    assert_eq!(one_set(&pile).repair(256, None), Err(SetError::Index));
    assert_eq!(
        one_set(&pile).repair_work(255),
        Ok(9 + 4 * 3 * (4 + 64) + 3 * (1 + 64))
    );
}

/// Twenty thousand files that end after their header beside one file of two hundred thousand
/// blocks: looked at block by block against every file this would be four thousand million
/// looks; only the files that reach a block are looked at for it.
#[test]
fn a_pile_of_short_files_beside_a_long_one_stays_cheap() {
    let facts = SetFacts::new(Field::Gf65536, &[1], 2, 800_000, 2, None).unwrap();
    assert_eq!(facts.blocks(), 200_000);
    let long = Header::new(facts, 0)
        .unwrap()
        .file(&bytes(1, 400_000))
        .unwrap();
    let headers: Vec<Header> = (1..=20_000)
        .map(|index| Header::new(facts, index).unwrap())
        .collect();
    let mut files: Vec<&[u8]> = headers.iter().map(Header::as_bytes).collect();
    files.push(&long);
    let pile = Pile::new(&files).unwrap();
    let coverage = one_set(&pile).coverage();
    assert_eq!(
        (coverage.shards, coverage.short, coverage.first_short),
        (20_001, 200_000, Some(0))
    );
    assert_eq!(stretches(one_set(&pile)), [(0, 200_000, 1)]);
}
