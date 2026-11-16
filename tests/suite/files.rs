//! The shard file as written: three files whose bytes were computed apart from the crate, files
//! of every shape against a writer of the tests' own, and the values the format refuses.

use crate::support::{RawHeader, bytes, crc32c, hex};
use reed_solomon_core::{
    Block, Condition, Field, FileError, Frame, Header, Length, SetFacts, ShardFile,
};

/// The SHA-256 of no bytes, the published value.
const DIGEST_OF_NOTHING: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// The header of shard 4 of the set these tests share: identifier 01, threshold 3.
fn header(field: Field, data_len: u64, digest: Option<[u8; 32]>) -> Header {
    let set = SetFacts::new(field, &[1], 3, data_len, 4, digest).unwrap();
    Header::new(set, 4).unwrap()
}

#[test]
fn the_tests_own_checksum_gives_the_published_check_value() {
    assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    assert_eq!(crc32c(b""), 0);
}

/// The checksums of this test and the next two were computed apart from the crate, twice: by a
/// bit-by-bit CRC and by a long division, each checked on "123456789" first.
#[test]
fn a_file_of_bytes_is_written_as_computed_apart() {
    let header = header(Field::Gf256, 10, None);
    let before = "5348524401010001010000000300000004000000000000000A00000004";
    assert_eq!(header.as_bytes(), hex(&format!("{before} CD62C865")));
    assert_eq!(header.block_checksum(0, &hex("F4000000")), 0xB356_1B91);
    assert_eq!(header.trailer().to_vec(), hex("53485254 00000021 62E47E4A"));

    let file = header.file(&hex("F4000000")).unwrap();
    let expected =
        format!("{before} CD62C865 F4000000 B3561B91 {before} CD62C865 53485254 00000021 62E47E4A");
    assert_eq!(*file, hex(&expected));
    assert_eq!(file.len(), 86);
}

#[test]
fn a_file_of_pairs_is_written_as_computed_apart() {
    let header = header(Field::Gf65536, 10, None);
    let before = "5348524401020001010000000300000004000000000000000A00000004";
    assert_eq!(header.as_bytes(), hex(&format!("{before} 065EF906")));
    assert_eq!(header.block_checksum(0, &hex("F4000000")), 0xA5C1_E59F);
    assert_eq!(header.trailer().to_vec(), hex("53485254 00000021 62E47E4A"));
    assert_eq!(header.file(&hex("F4000000")).unwrap().len(), 86);
}

/// A set of no data with a digest: the file is the two headers and the trailer, 142 bytes whose
/// SHA-256, computed apart, is
/// 6ab4f2eab5cc519afd673b269f867e7b97a8e2e5cab216ca1568b36a8598f243.
#[test]
fn a_file_of_no_data_with_a_digest_is_written_as_computed_apart() {
    let digest: [u8; 32] = hex(DIGEST_OF_NOTHING).try_into().unwrap();
    let header = header(Field::Gf65536, 0, Some(digest));
    let before =
        format!("53485244010201010100000003000000040000000000000000 00000004 {DIGEST_OF_NOTHING}");
    assert_eq!(header.as_bytes(), hex(&format!("{before} 57E0B8AE")));
    assert_eq!(header.trailer().to_vec(), hex("53485254 00000041 0322ED28"));

    let file = header.file(&[]).unwrap();
    let expected = format!("{before} 57E0B8AE {before} 57E0B8AE 53485254 00000041 0322ED28");
    assert_eq!(*file, hex(&expected));
    assert_eq!(file.len(), 142);

    let read = ShardFile::read(&file).unwrap();
    assert_eq!(read.header().set().digest(), Some(&digest));
    assert_eq!(read.header().set().blocks(), 0);
    assert_eq!(read.block(0), None);
    assert!(read.survey().whole());
}

/// Two blocks, the second a short one: shard 2 of the seventeen bytes 00 to 10 with threshold 3
/// is the six bytes 0C 0D 0E 0F 10 00, whose last byte fills up the data. The SHA-256 of the 92
/// bytes, computed apart, is d9b848c338aceb80dea245162680e96715d7488dc1f1223349c7e024eadea6f7.
#[test]
fn a_file_of_two_blocks_is_written_as_computed_apart() {
    let set = SetFacts::new(Field::Gf256, &[1], 3, 17, 4, None).unwrap();
    let header = Header::new(set, 2).unwrap();
    let before = "5348524401010001010000000300000002000000000000001100000004";
    assert_eq!(header.as_bytes(), hex(&format!("{before} 6DF8838D")));
    assert_eq!(header.block_checksum(0, &hex("0C0D0E0F")), 0xEBBF_9719);
    assert_eq!(header.block_checksum(1, &hex("1000")), 0x3D10_68C0);

    let file = header.file(&hex("0C0D0E0F1000")).unwrap();
    let expected = format!(
        "{before} 6DF8838D 0C0D0E0F EBBF9719 1000 3D1068C0 {before} 6DF8838D 53485254 00000021 62E47E4A"
    );
    assert_eq!(*file, hex(&expected));
    assert_eq!(file.len(), 92);

    let read = ShardFile::read(&file).unwrap();
    assert_eq!(read.block(0), Some(Block::Intact(&hex("0C0D0E0F"))));
    assert_eq!(read.block(1), Some(Block::Intact(&hex("1000"))));
    assert_eq!(read.block(2), None);
}

/// Every shape is written by the crate and by the tests' own writer, which shares no code with
/// it; the two files are the same bytes, and the file reads back whole, block by block.
#[test]
fn a_file_of_any_shape_is_what_a_writer_written_apart_writes_and_reads_back_whole() {
    let shapes: [(Field, u8, [u32; 4]); 2] = [
        (Field::Gf256, 1, [1, 3, 64, 4096]),
        (Field::Gf65536, 2, [2, 6, 64, 4096]),
    ];
    let ids: [&[u8]; 2] = [&[9], &[0xAB; 32]];
    let mut seed = 0;
    for (field, code, block_lens) in shapes {
        for (threshold, index) in [(1_u32, 0_u16), (3, 4), (16, 255)] {
            let k = u64::from(threshold);
            for data_len in [0, 1, k - 1, k, k + 1, 100, 1000] {
                for block_len in block_lens {
                    for digest in [None, Some([0x5A; 32])] {
                        for id in ids {
                            seed += 1;
                            let set = SetFacts::new(
                                field,
                                id,
                                usize::try_from(threshold).unwrap(),
                                data_len,
                                block_len,
                                digest,
                            )
                            .unwrap();
                            let header = Header::new(set, index).unwrap();
                            let shard = bytes(seed, usize::try_from(set.shard_len()).unwrap());
                            let raw = RawHeader {
                                version: 1,
                                field: code,
                                flags: u8::from(digest.is_some()),
                                id: id.to_vec(),
                                threshold,
                                index: u32::from(index),
                                data_len,
                                block_len,
                                digest,
                            };
                            let file = header.file(&shard).unwrap();
                            assert_eq!(*file, raw.file(&shard));
                            assert_eq!(u64::try_from(file.len()).unwrap(), set.file_len());
                            reads_back_whole(&file, &header, &shard);
                        }
                    }
                }
            }
        }
    }
}

/// `file` reads as `header` with every piece intact, its blocks in order are `shard`, and it is
/// written again to the same bytes.
fn reads_back_whole(file: &[u8], header: &Header, shard: &[u8]) {
    let read = ShardFile::read(file).unwrap();
    assert_eq!(read.header(), header);
    let survey = read.survey();
    assert!(survey.whole());
    assert_eq!(survey.intact, header.set().blocks());
    let blocks = u32::try_from(header.set().blocks()).unwrap();
    let mut joined = Vec::new();
    for number in 0..blocks {
        let found = read.block(number);
        assert!(
            matches!(found, Some(Block::Intact(_))),
            "block {number}: {found:?}"
        );
        if let Some(Block::Intact(block)) = found {
            joined.extend_from_slice(block);
        }
    }
    assert_eq!(joined, shard);
    assert_eq!(read.block(blocks), None);
    assert_eq!(*read.header().file(&joined).unwrap(), *file);
}

#[test]
fn facts_the_format_does_not_allow_are_refused() {
    let facts = |field, id: &[u8], threshold, block_len| {
        SetFacts::new(field, id, threshold, 10, block_len, None).map(|_| ())
    };
    assert_eq!(facts(Field::Gf2048, &[1], 3, 4), Err(FileError::Field));
    assert_eq!(facts(Field::Gf256, &[], 3, 4), Err(FileError::Id));
    assert_eq!(facts(Field::Gf256, &[0; 33], 3, 4), Err(FileError::Id));
    assert_eq!(facts(Field::Gf256, &[0; 32], 3, 4), Ok(()));
    assert_eq!(facts(Field::Gf256, &[1], 0, 4), Err(FileError::Threshold));
    assert_eq!(facts(Field::Gf256, &[1], 256, 4), Ok(()));
    assert_eq!(facts(Field::Gf256, &[1], 257, 4), Err(FileError::Threshold));
    assert_eq!(facts(Field::Gf65536, &[1], 65_536, 4), Ok(()));
    assert_eq!(
        facts(Field::Gf65536, &[1], 65_537, 4),
        Err(FileError::Threshold)
    );
    assert_eq!(facts(Field::Gf256, &[1], 3, 0), Err(FileError::Block));
    assert_eq!(facts(Field::Gf65536, &[1], 3, 0), Err(FileError::Block));
    assert_eq!(facts(Field::Gf65536, &[1], 3, 3), Err(FileError::Block));
    assert_eq!(facts(Field::Gf256, &[1], 3, u32::MAX), Ok(()));
    assert_eq!(
        facts(Field::Gf65536, &[1], 3, u32::MAX),
        Err(FileError::Block)
    );
}

#[test]
fn an_index_outside_the_field_and_a_shard_of_another_length_are_refused() {
    let bytes = SetFacts::new(Field::Gf256, &[1], 3, 10, 4, None).unwrap();
    assert!(Header::new(bytes, 255).is_ok());
    assert_eq!(Header::new(bytes, 256), Err(FileError::Index));
    let pairs = SetFacts::new(Field::Gf65536, &[1], 3, 10, 4, None).unwrap();
    assert!(Header::new(pairs, 65_535).is_ok());

    let header = Header::new(bytes, 4).unwrap();
    assert_eq!(header.file(&[0; 3]), Err(FileError::Shard));
    assert_eq!(header.file(&[0; 5]), Err(FileError::Shard));
    assert_eq!(header.file(&[]), Err(FileError::Shard));
}

/// A block's number is written in four bytes, so a shard has at most 2^32 blocks, the last
/// numbered FFFFFFFF.
#[test]
fn a_shard_has_at_most_two_to_the_32_blocks() {
    let blocks = |field, threshold, data_len, block_len| {
        SetFacts::new(field, &[1], threshold, data_len, block_len, None).map(|set| set.blocks())
    };
    let most = 1_u64 << 32;
    assert_eq!(blocks(Field::Gf256, 1, most, 1), Ok(most));
    assert_eq!(blocks(Field::Gf256, 1, most + 1, 1), Err(FileError::Blocks));
    assert_eq!(blocks(Field::Gf256, 3, 3 * most, 1), Ok(most));
    assert_eq!(
        blocks(Field::Gf256, 3, 3 * most + 1, 1),
        Err(FileError::Blocks)
    );
    assert_eq!(blocks(Field::Gf65536, 1, 2 * most, 2), Ok(most));
    assert_eq!(
        blocks(Field::Gf65536, 1, 2 * most + 1, 2),
        Err(FileError::Blocks)
    );
    // The longest data there is, in the longest blocks: 2^32 + 1 of them.
    assert_eq!(
        blocks(Field::Gf256, 1, u64::MAX, u32::MAX),
        Err(FileError::Blocks)
    );
    assert_eq!(
        blocks(Field::Gf65536, 1, u64::MAX, u32::MAX - 1),
        Err(FileError::Blocks)
    );
}

/// 2^64 − 2^32 bytes in blocks of 2^32 − 1 are exactly 2^32 blocks; with their checksums the
/// file would pass 2^64 bytes.
#[test]
fn a_file_is_at_most_two_to_the_64_less_one_bytes_long() {
    let facts = |data_len| SetFacts::new(Field::Gf256, &[1], 1, data_len, u32::MAX, None);
    assert_eq!(
        facts(u64::MAX - u64::from(u32::MAX)).map(|_| ()),
        Err(FileError::Size)
    );
    let long = facts(u64::MAX - (1 << 35)).unwrap();
    assert_eq!(long.shard_len(), u64::MAX - (1 << 35));
    assert_eq!(long.blocks(), (1 << 32) - 7);
    assert_eq!(long.file_len(), long.shard_len() + 4 * long.blocks() + 78);
}

/// SHA-256 takes fewer than 2^64 bits: facts that state a digest stop below 2^61 bytes of data,
/// and facts that state none go on. Nothing of the data is made.
#[test]
fn a_digest_is_stated_only_for_less_than_two_to_the_61_bytes() {
    let facts = |data_len, digest| SetFacts::new(Field::Gf256, &[1], 1, data_len, 1 << 30, digest);
    assert!(facts((1 << 61) - 1, Some([0; 32])).is_ok());
    assert_eq!(
        facts(1 << 61, Some([0; 32])).map(|_| ()),
        Err(FileError::Size)
    );
    assert!(facts(1 << 61, None).is_ok());
}

/// A header that counts 2^32 blocks in a file of 43 bytes: the survey ends at the first block
/// the file does not hold.
#[test]
fn a_header_that_counts_billions_of_blocks_costs_a_short_file_no_work() {
    let set = SetFacts::new(Field::Gf256, &[1], 1, 1 << 32, 1, None).unwrap();
    let header = Header::new(set, 0).unwrap();
    let mut file = header.as_bytes().to_vec();
    for number in 0..2 {
        file.push(7);
        file.extend(header.block_checksum(number, &[7]).to_be_bytes());
    }
    let read = ShardFile::read(&file).unwrap();
    let survey = read.survey();
    assert_eq!(
        (survey.intact, survey.damaged, survey.missing),
        (2, 0, (1 << 32) - 2)
    );
    let lost = set.file_len() - u64::try_from(file.len()).unwrap();
    let frame = Frame {
        lead: Condition::Intact,
        tail: Condition::Missing,
        trailer: Condition::Missing,
        length: Length::Short(lost),
    };
    assert_eq!(survey.frame, frame);
    assert_eq!(read.block(1), Some(Block::Intact(&[7])));
    assert_eq!(read.block(u32::MAX), Some(Block::Missing));
}

#[test]
fn every_refusal_says_what_was_wrong() {
    let refusals = [
        FileError::Field,
        FileError::Id,
        FileError::Threshold,
        FileError::Index,
        FileError::Block,
        FileError::Blocks,
        FileError::Size,
        FileError::Shard,
        FileError::Data,
        FileError::Memory,
        FileError::Unrecognized,
        FileError::Damaged,
        FileError::Version(2),
        FileError::Unsupported,
        FileError::Conflict,
    ];
    let texts: Vec<String> = refusals.iter().map(ToString::to_string).collect();
    for (number, text) in texts.iter().enumerate() {
        assert!(text.len() > 20, "{text}");
        assert!(!texts[..number].contains(text), "{text}");
    }
    assert!(FileError::Version(7).to_string().contains("version 7"));
}
