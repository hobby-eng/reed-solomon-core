//! The examples of the crate's documentation, as tests.

use reed_solomon_core::*;

#[test]
fn a_set_is_written_rebuilt_grown_and_corrected() -> Result<(), Box<dyn std::error::Error>> {
    // Three data shards become a set of five; any three rebuild the data.
    let code = Code::new(Field::Gf256, 3)?;
    let data: [&[u8]; 3] = [b"abcd", b"efgh", b"ijkl"];
    let mut set = code.encode(&data, &[0, 1, 2, 3, 4])?;

    // Shards 0 and 2 are lost: three others give them back, and a sixth shard as well.
    let left = [(4, &set[4][..]), (1, &set[1][..]), (3, &set[3][..])];
    let lost = code.rebuild(&left, &[0, 2])?;
    assert_eq!((&lost[0][..], &lost[1][..]), (&b"abcd"[..], &b"ijkl"[..]));
    let sixth = code.rebuild(&left, &[5])?;
    assert_eq!(
        code.redundancy(&[left[0], left[1], left[2], (5, &sixth[0][..])])?,
        Redundancy::Agrees
    );

    // One byte goes wrong somewhere: the two spare shards find it and put it right.
    set[2][1] ^= 0x40;
    let mut given: Vec<(u16, &mut [u8])> = (0..)
        .zip(set.iter_mut().map(|bytes| bytes.as_mut_slice()))
        .collect();
    assert_eq!(
        code.correct(&mut given)?,
        Correction::Corrected(vec![0, 0, 1, 0, 0])
    );
    assert_eq!(*set[2], *b"ijkl");
    Ok(())
}

#[test]
fn a_damaged_shard_file_is_read_by_its_other_header() -> Result<(), Box<dyn std::error::Error>> {
    // Shard 4 of a set of ten bytes with threshold 3, in blocks of four bytes.
    let set = SetFacts::new(Field::Gf256, b"backup-1", 3, 10, 4, None)?;
    let header = Header::new(set, 4)?;
    let mut file = header.file(&[0xF4, 0x00, 0x00, 0x00])?;

    // A bit goes wrong in the first header: the copy at the end is believed.
    file[9] ^= 0x01;
    let read = ShardFile::read(&file)?;
    assert_eq!(read.header(), &header);
    assert_eq!(
        read.block(0),
        Some(Block::Intact(&[0xF4, 0x00, 0x00, 0x00]))
    );
    assert!(!read.survey().whole());
    Ok(())
}

#[test]
fn a_set_of_files_is_rebuilt_and_a_lost_file_repaired() -> Result<(), Box<dyn std::error::Error>> {
    // Five files of a set of twenty-six bytes, stating its SHA-256; any three rebuild it.
    let data = b"twenty-six bytes of a text";
    let facts = SetFacts::for_data(Field::Gf256, b"backup-1", 3, data, 8, DataDigest::Sha256)?;
    let mut files = facts.files(data, &[0, 1, 2, 3, 4])?;

    // One file is lost and another damaged; the rest is given in any order.
    let lost = files.remove(1);
    files[2][82] ^= 0x01;
    let given: Vec<&[u8]> = files.iter().rev().map(|bytes| bytes.as_slice()).collect();
    let pile = Pile::new(&given)?;
    let set = &pile.sets()[0];
    let rebuilt = set.rebuild(None)?;
    assert_eq!(*rebuilt.data, *data);
    assert!(matches!(rebuilt.digest, DigestCheck::Matched(_)));

    // The lost file is made again from the others, byte for byte.
    assert_eq!(set.repair(1, None)?, lost);
    Ok(())
}

#[test]
fn a_file_of_other_bytes_is_named_by_the_vote() -> Result<(), Box<dyn std::error::Error>> {
    // Five files of a set, and in the place of file 3 the file of another text.
    let data = b"twenty-six bytes of a text";
    let facts = SetFacts::new(Field::Gf256, b"backup-1", 3, 26, 8, None)?;
    let mut files = facts.files(data, &[0, 1, 2, 3, 4])?;
    files[3] = facts.files(b"another text, of 26 bytes!", &[3])?.remove(0);
    let given: Vec<&[u8]> = files.iter().map(|bytes| bytes.as_slice()).collect();
    let pile = Pile::new(&given)?;
    let set = &pile.sets()[0];
    assert!(matches!(
        set.rebuild(None),
        Err(SetError::Inconsistent { .. })
    ));

    // Four files agree and one does not: the vote names it, and the set without it rebuilds.
    let vote = set.vote(None)?;
    assert_eq!(vote.outvoted().len(), 1);
    assert_eq!(vote.outvoted()[0].position, 3);
    assert_eq!(*set.without(&[3])?.rebuild(None)?.data, *data);
    Ok(())
}

#[test]
fn a_set_too_large_for_memory_is_rebuilt_into_outputs() -> Result<(), Box<dyn std::error::Error>> {
    // The files and the outputs can be mappings of files on disk; here they are vectors.
    let data = b"twenty-six bytes of a text";
    let facts = SetFacts::new(Field::Gf256, b"backup-1", 3, 26, 8, None)?;
    let files = facts.files(data, &[0, 1, 2, 3, 4])?;
    let given: Vec<&[u8]> = files[2..].iter().map(|file| file.as_slice()).collect();
    let pile = Pile::new(&given)?;
    let set = &pile.sets()[0];

    // The data goes straight into the output, a block at a time.
    let mut out = vec![0; usize::try_from(facts.data_len())?];
    set.rebuild_into(&mut out, None)?;
    assert_eq!(out, data);

    // Two lost files in one pass, each into an output of its own.
    let length = usize::try_from(facts.file_len())?;
    let (mut first, mut second) = (vec![0; length], vec![0; length]);
    set.repairs_into(
        &[0, 1],
        &mut [first.as_mut_slice(), second.as_mut_slice()],
        None,
    )?;
    assert_eq!(
        (first.as_slice(), second.as_slice()),
        (&*files[0], &*files[1])
    );
    Ok(())
}

#[test]
fn a_card_gets_its_repair_words_by_remainder_and_is_put_right()
-> Result<(), Box<dyn std::error::Error>> {
    use reed_solomon_core::{CyclicCode, Field, Mark, Plan};

    // The twelve words of a card, as numbers of a list of 2,048 words, and four repair words after
    // them, as the printed cards of MHFE-REPAIR-1 compute them: GF(2^11), first root α, step 1.
    let code = CyclicCode::new(Field::Gf2048, 12, 4, 1, 1)?;
    let card: [u16; 12] = [
        0x371, 0x6CE, 0x220, 0x11D, 0x2DB, 0x73E, 0x12B, 0x7C6, 0x7D5, 0x7AF, 0x6A9, 0x453,
    ];
    let words: Vec<[u8; 2]> = card.iter().map(|word| word.to_be_bytes()).collect();
    let data: Vec<&[u8]> = words.iter().map(|word| word.as_slice()).collect();
    let repair = code.encode(&data)?;
    assert_eq!(*repair[0], 0x52C_u16.to_be_bytes());

    // Read back with word 3 smudged and word 11 copied wrong: one mark, one word found.
    let mut read: Vec<[u8; 2]> = words.clone();
    read.extend(repair.iter().map(|word| [word[0], word[1]]));
    read[2] = [0, 0];
    read[10] = 0x7FF_u16.to_be_bytes();
    let views: Vec<&[u8]> = read.iter().map(|word| word.as_slice()).collect();
    let Plan::Changes(changes) = code.plan(
        &views,
        &[],
        &[Mark {
            shard: 2,
            symbol: 0,
        }],
    )?
    else {
        return Err("the card is within 2e + s ≤ 4".into());
    };
    let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(|word| word.as_mut_slice()).collect();
    code.apply(&mut buffers, &changes)?;
    assert_eq!(
        (read[2], read[10]),
        (0x220_u16.to_be_bytes(), 0x6A9_u16.to_be_bytes())
    );
    Ok(())
}
