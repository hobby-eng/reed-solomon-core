//! The SHA-256 of the data: computed when a set is described from its data, required of the data
//! its files are written from, and compared with the rebuilt data before the data is given back.
//! Every expected digest was computed apart from this crate, by Python's `hashlib`.

use crate::sets::{one_set, slices};
use crate::support::hex;
use reed_solomon_core::{
    Code, DataDigest, DigestCheck, Field, FileError, Header, Pile, SetError, SetFacts,
};

/// The data of the tests: 26 bytes, so that three shards of nine bytes end in one zero byte.
const TEXT: &[u8] = b"twenty-six bytes of a text";
/// SHA-256 of `TEXT`.
const TEXT_DIGEST: &str = "10A53D32320D75FBD8CEDCBE5890E78A1265AB678F1BFE4DEEC5C13D5764228F";
/// SHA-256 of `TEXT` followed by the zero byte that fills up its last shard: what a digest of
/// the padded shards would be, and must not be.
const PADDED_DIGEST: &str = "585F5DC0EBD9B5B498E32E73D3625258DBD181BE4CC684C66C5658699179220E";
/// Other data of the same length.
const OTHER: &[u8] = b"another text, of 26 bytes!";
/// SHA-256 of `OTHER`.
const OTHER_DIGEST: &str = "4C90749AA2292580883E31C1A5B7EFFBD95F5D8AFD0D5334C9AD51211327FBBD";
/// SHA-256 of no bytes.
const NOTHING_DIGEST: &str = "E3B0C44298FC1C149AFBF4C8996FB92427AE41E4649B934CA495991B7852B855";

fn digest(digits: &str) -> [u8; 32] {
    hex(digits).try_into().unwrap()
}

fn described(data: &[u8], threshold: usize, block_len: u32, digest: DataDigest) -> SetFacts {
    SetFacts::for_data(Field::Gf256, b"digests", threshold, data, block_len, digest).unwrap()
}

#[test]
fn the_digest_is_computed_from_the_data_when_a_set_is_described() {
    let with = described(TEXT, 3, 8, DataDigest::Sha256);
    assert_eq!(with.digest(), Some(&digest(TEXT_DIGEST)));
    assert_eq!(
        with,
        SetFacts::new(
            Field::Gf256,
            b"digests",
            3,
            26,
            8,
            Some(digest(TEXT_DIGEST))
        )
        .unwrap()
    );
    let without = described(TEXT, 3, 8, DataDigest::None);
    assert_eq!(without.digest(), None);
    assert_eq!(
        without,
        SetFacts::new(Field::Gf256, b"digests", 3, 26, 8, None).unwrap()
    );
    assert_eq!(
        described(b"", 2, 4, DataDigest::Sha256).digest(),
        Some(&digest(NOTHING_DIGEST))
    );
    // The checks of the facts come first, as for `SetFacts::new`.
    assert_eq!(
        SetFacts::for_data(Field::Gf2048, b"x", 3, TEXT, 8, DataDigest::Sha256),
        Err(FileError::Field)
    );
    assert_eq!(
        SetFacts::for_data(Field::Gf256, b"", 3, TEXT, 8, DataDigest::Sha256),
        Err(FileError::Id)
    );
}

#[test]
fn files_are_written_only_for_data_that_has_the_digest_the_facts_state() {
    let facts = described(TEXT, 3, 8, DataDigest::Sha256);
    assert_eq!(facts.files(OTHER, &[0, 3]), Err(FileError::Digest));
    // With nothing asked for, the data is still checked.
    assert_eq!(facts.files(OTHER, &[]), Err(FileError::Digest));
    // The length is checked first, then the digest, then the indexes.
    assert_eq!(facts.files(b"short", &[0]), Err(FileError::Data));
    assert_eq!(facts.files(OTHER, &[300]), Err(FileError::Digest));
    assert_eq!(facts.files(TEXT, &[300]), Err(FileError::Index));
    // The files of the right data state the digest in both copies of the header.
    let files = facts.files(TEXT, &[0, 4]).unwrap();
    for file in &files {
        let read = reed_solomon_core::ShardFile::read(file).unwrap();
        assert_eq!(read.header().set().digest(), Some(&digest(TEXT_DIGEST)));
    }
}

#[test]
fn a_rebuild_compares_the_digest_of_exactly_the_bytes_of_the_data() {
    // Blocks of eight bytes: each shard of nine bytes ends in a block of one byte.
    let facts = described(TEXT, 3, 8, DataDigest::Sha256);
    let files = facts.files(TEXT, &[0, 1, 2, 3, 4]).unwrap();
    assert_ne!(digest(TEXT_DIGEST), digest(PADDED_DIGEST));
    for chosen in [[0, 1, 2], [2, 3, 4], [0, 3, 4], [1, 2, 4]] {
        let given: Vec<&[u8]> = chosen.iter().map(|&at| files[at].as_slice()).collect();
        let pile = Pile::new(&given).unwrap();
        let rebuilt = one_set(&pile).rebuild(None).unwrap();
        assert_eq!(*rebuilt.data, *TEXT);
        assert_eq!(rebuilt.digest, DigestCheck::Matched(digest(TEXT_DIGEST)));
    }
}

#[test]
fn data_of_no_bytes_rebuilds_with_the_digest_of_nothing() {
    let facts = described(b"", 2, 4, DataDigest::Sha256);
    let files = facts.files(b"", &[0, 1, 2]).unwrap();
    let pile = Pile::new(&slices(&files)).unwrap();
    let rebuilt = one_set(&pile).rebuild(None).unwrap();
    assert!(rebuilt.data.is_empty());
    assert_eq!(rebuilt.digest, DigestCheck::Matched(digest(NOTHING_DIGEST)));
}

#[test]
fn files_without_a_digest_rebuild_and_say_that_nothing_was_compared() {
    let facts = described(TEXT, 3, 8, DataDigest::None);
    let files = facts.files(TEXT, &[0, 1, 2]).unwrap();
    let pile = Pile::new(&slices(&files)).unwrap();
    let rebuilt = one_set(&pile).rebuild(None).unwrap();
    assert_eq!(*rebuilt.data, *TEXT);
    assert_eq!(rebuilt.digest, DigestCheck::Unchecked);
}

/// The five files of `OTHER` under facts that state the digest of `TEXT`: written shard by shard,
/// so that every checksum holds and the shards are one set, as a writer that changed the data
/// in every file alike would leave them.
fn agreeing_on_other_data() -> Vec<Vec<u8>> {
    let facts = SetFacts::new(
        Field::Gf256,
        b"digests",
        3,
        26,
        8,
        Some(digest(TEXT_DIGEST)),
    )
    .unwrap();
    let mut padded = OTHER.to_vec();
    padded.push(0);
    let data: Vec<&[u8]> = padded.chunks(9).collect();
    let shards = Code::new(Field::Gf256, 3)
        .unwrap()
        .encode(&data, &[0, 1, 2, 3, 4])
        .unwrap();
    (0..5_u16)
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
fn files_that_agree_on_data_of_another_digest_are_refused() {
    let files = agreeing_on_other_data();
    let pile = Pile::new(&slices(&files)).unwrap();
    let set = one_set(&pile);
    // Every file reads whole and the shards are one set: nothing but the digest tells.
    assert!(
        pile.readings()
            .iter()
            .all(|reading| reading.as_ref().unwrap().survey().whole())
    );
    let vote = set.vote(None).unwrap();
    assert!(vote.outvoted().is_empty());
    assert_eq!(vote.undecided(), 0);
    assert_eq!(
        set.rebuild(None),
        Err(SetError::Digest {
            stated: digest(TEXT_DIGEST),
            found: digest(OTHER_DIGEST),
        })
    );
    // Making one lost file again does not put the data together: it compares no digest, and the
    // file it makes states the set's digest, which a rebuild then compares.
    let made = set.repair(1, None).unwrap();
    assert_eq!(*made, files[1]);
}
