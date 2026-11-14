//! What is left of a shard file after damage: one wrong bit anywhere, a lost end, bytes after
//! the end, a lost header, two headers that differ, another version, values out of range.

use crate::support::{RawHeader, bytes, trailer};
use reed_solomon_core::{
    Block, Condition, Field, FileError, Frame, Header, Length, SetFacts, ShardFile, Survey,
};

/// The header of the sample file is 35 bytes long: a three-byte identifier and no digest.
const HEADER: usize = 35;
/// The sample's shard is nine bytes in blocks of four: two whole blocks and one of one byte.
const SHARD: usize = 9;
/// Where the pieces of the sample file start: the lead, three blocks, the copy at the end, the
/// trailer, and the end of the file.
const PIECES: [usize; 7] = [0, 35, 43, 51, 56, 91, 103];

/// The sample's header as raw numbers: shard `index` of a set of bytes with threshold 3.
fn raw(index: u32) -> RawHeader {
    RawHeader {
        version: 1,
        field: 1,
        flags: 0,
        id: vec![1, 2, 3],
        threshold: 3,
        index,
        data_len: 25,
        block_len: 4,
        digest: None,
    }
}

/// The sample file of shard `index`, with its header and its shard.
fn sample(index: u16) -> (Header, Vec<u8>, Vec<u8>) {
    let set = SetFacts::new(Field::Gf256, &[1, 2, 3], 3, 25, 4, None).unwrap();
    let header = Header::new(set, index).unwrap();
    let shard = bytes(u64::from(index), SHARD);
    let file = header.file(&shard).unwrap();
    assert_eq!(file.len(), PIECES[6]);
    assert_eq!(header.as_bytes().len(), HEADER);
    (header, shard, file)
}

/// The conditions of the lead, the three blocks, the copy at the end and the trailer.
fn conditions(file: &ShardFile<'_>) -> [Condition; 6] {
    let block = |number| match file.block(number).unwrap() {
        Block::Intact(_) => Condition::Intact,
        Block::Damaged => Condition::Damaged,
        Block::Missing => Condition::Missing,
    };
    let Frame {
        lead,
        tail,
        trailer,
        ..
    } = file.frame();
    [lead, block(0), block(1), block(2), tail, trailer]
}

/// The survey's counts of intact, damaged and missing blocks.
fn counts(survey: &Survey) -> (u64, u64, u64) {
    (survey.intact, survey.damaged, survey.missing)
}

#[test]
fn one_wrong_bit_anywhere_costs_the_piece_it_is_in_and_nothing_else() {
    let (header, _, file) = sample(4);
    for bit in 0..file.len() * 8 {
        let mut changed = file.clone();
        changed[bit / 8] ^= 1 << (bit % 8);
        let read = ShardFile::read(&changed).unwrap();
        assert_eq!(read.header(), &header, "bit {bit}");
        assert_eq!(read.frame().length, Length::Exact);
        let piece = PIECES.iter().rposition(|&start| start <= bit / 8).unwrap();
        let mut expected = [Condition::Intact; 6];
        expected[piece] = Condition::Damaged;
        assert_eq!(conditions(&read), expected, "bit {bit}");
        let survey = read.survey();
        assert!(!survey.whole(), "bit {bit}");
        let in_block = (1..=3).contains(&piece);
        assert_eq!(
            counts(&survey),
            (3 - u64::from(in_block), u64::from(in_block), 0)
        );
    }
}

#[test]
fn a_file_cut_short_keeps_the_blocks_that_are_wholly_there() {
    let (header, shard, file) = sample(4);
    for length in 0..file.len() {
        let cut = &file[..length];
        let read = match ShardFile::read(cut) {
            Ok(read) => read,
            Err(refusal) => {
                // Before the end of the header nothing can be read: without its mark the bytes
                // are not a shard file, with it they are a damaged one.
                assert!(length < HEADER, "length {length}");
                let expected = if length < 4 {
                    FileError::Unrecognized
                } else {
                    FileError::Damaged
                };
                assert_eq!(refusal, expected, "length {length}");
                continue;
            }
        };
        assert!(length >= HEADER, "length {length}");
        assert_eq!(read.header(), &header);
        let lost = u64::try_from(file.len() - length).unwrap();
        assert_eq!(read.frame().length, Length::Short(lost));
        // A piece is there when the file reaches the start of the next one.
        let there = |piece: usize| {
            if length >= PIECES[piece + 1] {
                Condition::Intact
            } else {
                Condition::Missing
            }
        };
        let expected = [
            Condition::Intact,
            there(1),
            there(2),
            there(3),
            there(4),
            there(5),
        ];
        assert_eq!(conditions(&read), expected, "length {length}");
        let intact = (1..=3)
            .filter(|&piece| there(piece) == Condition::Intact)
            .count();
        let intact = u64::try_from(intact).unwrap();
        assert_eq!(counts(&read.survey()), (intact, 0, 3 - intact));
        if length >= PIECES[2] {
            assert_eq!(read.block(0), Some(Block::Intact(&shard[..4])));
        }
    }
}

#[test]
fn bytes_after_the_end_are_counted_and_used_for_nothing() {
    let (header, shard, file) = sample(4);
    let tails: [Vec<u8>; 5] = [
        vec![0],
        vec![0; 12],
        bytes(7, 100),
        trailer(HEADER),
        file.clone(),
    ];
    for tail in tails {
        let long = [&file[..], &tail[..]].concat();
        let read = ShardFile::read(&long).unwrap();
        assert_eq!(read.header(), &header);
        let extra = u64::try_from(tail.len()).unwrap();
        assert_eq!(read.frame().length, Length::Long(extra));
        assert_eq!(conditions(&read), [Condition::Intact; 6]);
        assert_eq!(read.block(2), Some(Block::Intact(&shard[8..])));
        let survey = read.survey();
        assert_eq!(counts(&survey), (3, 0, 0));
        assert!(!survey.whole());
    }
}

#[test]
fn the_copy_at_the_end_stands_in_for_a_lost_header() {
    let (header, shard, file) = sample(4);
    let mut lost = file.clone();
    lost[..HEADER].fill(0);
    let read = ShardFile::read(&lost).unwrap();
    assert_eq!(read.header(), &header);
    let mut expected = [Condition::Intact; 6];
    expected[0] = Condition::Damaged;
    assert_eq!(conditions(&read), expected);
    assert_eq!(read.block(1), Some(Block::Intact(&shard[4..8])));

    // The copy at the end counts only where its own sizes put it: not in a file that lost its
    // last byte or its first, or that has a byte too many.
    assert_eq!(
        ShardFile::read(&lost[..102]).err(),
        Some(FileError::Unrecognized)
    );
    assert_eq!(ShardFile::read(&lost[1..]).err(), Some(FileError::Damaged));
    assert_eq!(ShardFile::read(&file[1..]).err(), Some(FileError::Damaged));
    let long = [&lost[..], &[0]].concat();
    assert_eq!(ShardFile::read(&long).err(), Some(FileError::Unrecognized));

    // The mark of a trailer that does not hold is still a mark: a damaged shard file. Without
    // it nothing says that these bytes ever were one.
    let mut trailer_wrong = lost.clone();
    trailer_wrong[102] ^= 1;
    assert_eq!(
        ShardFile::read(&trailer_wrong).err(),
        Some(FileError::Damaged)
    );
    let mut mark_wrong = lost.clone();
    mark_wrong[91] ^= 1;
    assert_eq!(
        ShardFile::read(&mark_wrong).err(),
        Some(FileError::Unrecognized)
    );

    // With the mark of the lead still there and no copy that holds, the file is a damaged one.
    let mut both = file.clone();
    both[20] ^= 1;
    both[70] ^= 1;
    assert_eq!(ShardFile::read(&both).err(), Some(FileError::Damaged));
    let mut no_trailer = file;
    no_trailer[20] ^= 1;
    no_trailer[100] ^= 1;
    assert_eq!(ShardFile::read(&no_trailer).err(), Some(FileError::Damaged));
}

#[test]
fn two_headers_that_hold_and_differ_are_a_conflict() {
    let (_, _, four) = sample(4);
    let (_, _, five) = sample(5);
    // The start of one file and the end of another of the same set.
    let mut mixed = four.clone();
    mixed[PIECES[4]..].copy_from_slice(&five[PIECES[4]..]);
    assert_eq!(ShardFile::read(&mixed).err(), Some(FileError::Conflict));
    // The same with the trailer damaged: the other header still sits in the place of the copy.
    mixed[100] ^= 1;
    assert_eq!(ShardFile::read(&mixed).err(), Some(FileError::Conflict));
    // The other way round as well: neither order of the two is preferred.
    let mut mixed = five;
    mixed[PIECES[4]..].copy_from_slice(&four[PIECES[4]..]);
    assert_eq!(ShardFile::read(&mixed).err(), Some(FileError::Conflict));

    // A whole file followed by the header and the trailer of a longer one, 150 bytes in all,
    // which is the length the second header gives: both hold, each in its place.
    let longer = RawHeader {
        data_len: 192,
        block_len: 64,
        ..raw(4)
    };
    assert_eq!(longer.file(&[0; 64]).len(), 150);
    let joined = [&four[..], &longer.bytes()[..], &trailer(HEADER)[..]].concat();
    assert_eq!(joined.len(), 150);
    assert_eq!(ShardFile::read(&joined).err(), Some(FileError::Conflict));
}

/// A file that lost its trailer, with another header after its blocks: the place of the copy is
/// known from the header at the start, and what stands there is read by its own length.
#[test]
fn another_header_in_the_place_of_the_copy_is_a_conflict_whatever_its_length() {
    let shard = bytes(3, 8);
    let header = |id: &[u8]| {
        let set = SetFacts::new(Field::Gf256, id, 3, 24, 4, None).unwrap();
        Header::new(set, 4).unwrap()
    };
    let (short, long, other) = (header(&[1]), header(&[1, 2]), header(&[9]));
    assert_eq!((short.as_bytes().len(), long.as_bytes().len()), (33, 34));
    // The header, the two blocks with their checksums, and then the other header.
    let with = |first: &Header, second: &[u8]| {
        let file = first.file(&shard).unwrap();
        let blocks = first.as_bytes().len() + 16;
        [&file[..blocks], second].concat()
    };
    // Cut by a byte, the other header holds nothing. Where the bytes left still fill the place
    // of the copy, the copy is damaged; where they do not, it is missing. The blocks stay.
    let cases = [
        (&short, &other, Condition::Missing),
        (&short, &long, Condition::Damaged),
        (&long, &short, Condition::Missing),
    ];
    for (first, second, cut_short) in cases {
        let mixed = with(first, second.as_bytes());
        assert_eq!(ShardFile::read(&mixed).err(), Some(FileError::Conflict));
        let read = ShardFile::read(&mixed[..mixed.len() - 1]).unwrap();
        assert_eq!(read.header(), first);
        assert_eq!(read.frame().tail, cut_short);
        assert_eq!(counts(&read.survey()), (2, 0, 0));
    }
    // A header of an unread field whose checksum holds, shorter and longer than the first.
    for id in [vec![7], vec![7; 5]] {
        let unread = RawHeader {
            field: 3,
            id,
            data_len: 24,
            ..raw(4)
        };
        let mixed = with(&long, &unread.bytes());
        assert_eq!(ShardFile::read(&mixed).err(), Some(FileError::Conflict));
    }
    // The file's own header there is its copy, with the trailer missing.
    let own = with(&long, long.as_bytes());
    let read = ShardFile::read(&own).unwrap();
    assert_eq!(read.frame().tail, Condition::Intact);
    assert_eq!(read.frame().trailer, Condition::Missing);
}

#[test]
fn another_version_is_named_and_believed_last() {
    let (header, _, file) = sample(4);
    // Version 2 in both copies, with the checksums as they were and with fitting ones.
    let mut both = file.clone();
    both[4] = 2;
    both[PIECES[4] + 4] = 2;
    assert_eq!(ShardFile::read(&both).err(), Some(FileError::Version(2)));
    let later = RawHeader {
        version: 2,
        ..raw(4)
    };
    let written = later.file(&bytes(4, SHARD));
    assert_eq!(ShardFile::read(&written).err(), Some(FileError::Version(2)));

    // One copy of this version that holds is believed before a version byte.
    let mut lead = file.clone();
    lead[4] = 2;
    let read = ShardFile::read(&lead).unwrap();
    assert_eq!(read.header(), &header);
    assert_eq!(read.frame().lead, Condition::Damaged);
    let mut tail = file.clone();
    tail[PIECES[4] + 4] = 9;
    let read = ShardFile::read(&tail).unwrap();
    assert_eq!(read.header(), &header);
    assert_eq!(read.frame().tail, Condition::Damaged);

    // Two different numbers: the one at the start is named.
    let mut two = file.clone();
    two[4] = 2;
    two[PIECES[4] + 4] = 3;
    assert_eq!(ShardFile::read(&two).err(), Some(FileError::Version(2)));
    // The mark and the byte after it are enough to name a version; the mark alone is not.
    assert_eq!(
        ShardFile::read(&two[..5]).err(),
        Some(FileError::Version(2))
    );
    assert_eq!(ShardFile::read(&two[..4]).err(), Some(FileError::Damaged));
    // A version byte, the rest of the header lost with the end of the file.
    assert_eq!(
        ShardFile::read(&two[..8]).err(),
        Some(FileError::Version(2))
    );
}

#[test]
fn a_field_or_a_flag_this_version_does_not_read_is_unsupported_when_its_checksum_holds() {
    let shard = bytes(4, SHARD);
    let (header, _, file) = sample(4);
    // The field of words, a field with no meaning, and a flag with none.
    let unread = [
        RawHeader { field: 3, ..raw(4) },
        RawHeader { field: 0, ..raw(4) },
        RawHeader { flags: 2, ..raw(4) },
        RawHeader {
            flags: 0x80,
            ..raw(4)
        },
    ];
    for raw in unread {
        let written = raw.file(&shard);
        assert_eq!(
            ShardFile::read(&written).err(),
            Some(FileError::Unsupported)
        );
        // Its lead alone says the same; with a copy of this version at the end, both hold and
        // differ.
        assert_eq!(
            ShardFile::read(&written[..HEADER]).err(),
            Some(FileError::Unsupported)
        );
        let mut mixed = file.clone();
        mixed[..HEADER].copy_from_slice(&written[..HEADER]);
        assert_eq!(ShardFile::read(&mixed).err(), Some(FileError::Conflict));
        let mut mixed = file.clone();
        mixed[PIECES[4]..PIECES[5]].copy_from_slice(&written[..HEADER]);
        assert_eq!(ShardFile::read(&mixed).err(), Some(FileError::Conflict));
        // The same header and a trailer after the end of a whole file: its sizes cannot be
        // worked out, so its place is not known to be one, and it is bytes like any others.
        let long = [&file[..], &written[..HEADER], &trailer(HEADER)[..]].concat();
        let read = ShardFile::read(&long).unwrap();
        assert_eq!(read.header(), &header);
        assert_eq!(read.frame().length, Length::Long(47));
        assert_eq!(conditions(&read), [Condition::Intact; 6]);
    }
    // Two unread headers that differ, at the start and at the end: unsupported, whichever the
    // one at the end is.
    let words = RawHeader { field: 3, ..raw(4) }.file(&shard);
    let flag = RawHeader { flags: 2, ..raw(4) }.file(&shard);
    let mut two = words;
    two[PIECES[4]..].copy_from_slice(&flag[PIECES[4]..]);
    assert_eq!(ShardFile::read(&two).err(), Some(FileError::Unsupported));
    // The same bytes without a fitting checksum are damage, and the other copy is believed.
    let mut flagged = file.clone();
    flagged[6] = 2;
    let read = ShardFile::read(&flagged).unwrap();
    assert_eq!(read.header(), &header);
    assert_eq!(read.frame().lead, Condition::Damaged);
}

#[test]
fn a_header_with_a_value_out_of_range_does_not_hold_whatever_its_checksum() {
    let shard = bytes(4, SHARD);
    let wrong = [
        RawHeader {
            threshold: 0,
            ..raw(4)
        },
        RawHeader {
            threshold: 257,
            ..raw(4)
        },
        RawHeader {
            index: 256,
            ..raw(4)
        },
        RawHeader {
            index: 65_536,
            field: 2,
            data_len: 50,
            ..raw(4)
        },
        RawHeader {
            block_len: 0,
            ..raw(4)
        },
        RawHeader {
            block_len: 3,
            field: 2,
            ..raw(4)
        },
        RawHeader {
            id: Vec::new(),
            ..raw(4)
        },
        RawHeader {
            id: vec![1; 33],
            ..raw(4)
        },
        // More than 2^32 blocks.
        RawHeader {
            data_len: 3 * (1 << 32) + 1,
            block_len: 1,
            ..raw(4)
        },
    ];
    for raw in wrong {
        let written = raw.file(&shard);
        assert_eq!(ShardFile::read(&written).err(), Some(FileError::Damaged));
    }
}

#[test]
fn a_block_is_tied_to_its_place_and_to_its_file() {
    let (_, shard, file) = sample(4);
    // Blocks 0 and 1, each with its checksum, change places.
    let mut swapped = file.clone();
    swapped[PIECES[1]..PIECES[2]].copy_from_slice(&file[PIECES[2]..PIECES[3]]);
    swapped[PIECES[2]..PIECES[3]].copy_from_slice(&file[PIECES[1]..PIECES[2]]);
    let read = ShardFile::read(&swapped).unwrap();
    assert_eq!(counts(&read.survey()), (1, 2, 0));
    assert_eq!(read.block(2), Some(Block::Intact(&shard[8..])));

    // Block 0 of shard 5 in the place of block 0 of shard 4.
    let (_, _, five) = sample(5);
    let mut foreign = file.clone();
    foreign[PIECES[1]..PIECES[2]].copy_from_slice(&five[PIECES[1]..PIECES[2]]);
    let read = ShardFile::read(&foreign).unwrap();
    assert_eq!(read.block(0), Some(Block::Damaged));
    assert_eq!(read.block(1), Some(Block::Intact(&shard[4..8])));

    // The same bytes under the same number in a file of another set.
    let other = SetFacts::new(Field::Gf256, &[1, 2, 4], 3, 25, 4, None).unwrap();
    let other = Header::new(other, 4).unwrap().file(&shard).unwrap();
    let mut foreign = file;
    foreign[PIECES[1]..PIECES[2]].copy_from_slice(&other[PIECES[1]..PIECES[2]]);
    assert_eq!(
        ShardFile::read(&foreign).unwrap().block(0),
        Some(Block::Damaged)
    );
}

/// Whatever happens to a file, what is read from it is the original header or a refusal, and a
/// block called intact holds the original bytes.
#[test]
fn damage_of_any_kind_never_passes_other_bytes_as_intact() {
    let set = SetFacts::new(Field::Gf65536, &[5; 20], 4, 1200, 16, Some([3; 32])).unwrap();
    let header = Header::new(set, 300).unwrap();
    let shard = bytes(1, 300);
    let file = header.file(&shard).unwrap();
    let mut random = bytes(99, 60_000).into_iter();
    let mut next = |below: usize| {
        let pair = [random.next().unwrap(), random.next().unwrap()];
        usize::from(u16::from_be_bytes(pair)) % below
    };
    let (mut read_back, mut refused) = (0, 0);
    for _ in 0..3000 {
        let mut changed = file.clone();
        for _ in 0..=next(3) {
            let (at, to) = (next(changed.len()), next(changed.len()));
            let (low, high) = (at.min(to), at.max(to));
            match next(6) {
                0 => changed[at] ^= 1 << next(8),
                1 => changed[low..high].fill(0),
                2 => changed.truncate(high.max(1)),
                3 => changed.extend(bytes(7, low % 64)),
                4 => {
                    let to = next(changed.len() - (high - low) + 1);
                    changed.copy_within(low..high, to);
                }
                _ => changed[at] = 0xFF,
            }
        }
        let Ok(read) = ShardFile::read(&changed) else {
            refused += 1;
            continue;
        };
        read_back += 1;
        assert_eq!(read.header(), &header);
        for (number, original) in (0..).zip(shard.chunks(16)) {
            if let Some(Block::Intact(block)) = read.block(number) {
                assert_eq!(block, original);
            }
        }
    }
    // Both outcomes are met many times.
    assert!(
        read_back > 500 && refused > 100,
        "{read_back} read, {refused} refused"
    );
}
