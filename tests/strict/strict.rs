//! The strict mode: the same answers as the calls of the root types, with the secret only in
//! protected memory, and a refusal where the system does not protect it. Which memory a strict
//! call takes is fixed by its types; a `Vec` given to one does not compile (the module's docs).

use reed_solomon_core::protect::{Secret, WorkArea};
use reed_solomon_core::strict::{self, Held, HeldMut};
use reed_solomon_core::{
    Buffer, ChangeKind, Changes, Code, CodeError, Correction, DataDigest, Field, FileError, Mark,
    Pile, Plan, SetError, SetFacts,
};

/// `count` bytes that follow from `seed` (SplitMix64), the same for every run.
fn bytes(seed: u64, count: usize) -> Vec<u8> {
    let mut state = seed;
    (0..count)
        .map(|_| {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            (z ^ (z >> 31)) as u8
        })
        .collect()
}

/// `bytes` made symbols of `field` by keeping the bits a symbol has.
fn symbols(field: Field, mut bytes: Vec<u8>) -> Vec<u8> {
    let mask = field.order() - 1;
    if field.symbol_bytes() == 1 {
        bytes.iter_mut().for_each(|byte| *byte &= mask as u8);
    } else {
        for pair in bytes.as_chunks_mut::<2>().0 {
            *pair = (u16::from_be_bytes(*pair) & mask as u16).to_be_bytes();
        }
    }
    bytes
}

/// `bytes` in a secret of their own.
fn secret(bytes: &[u8]) -> Secret {
    let mut secret = Secret::with_capacity(bytes.len());
    secret.push(bytes).unwrap();
    secret
}

/// The bytes of buffers, to compare.
fn plain(buffers: &[Buffer]) -> Vec<Vec<u8>> {
    buffers.iter().map(|buffer| buffer.to_vec()).collect()
}

#[test]
fn the_strict_code_gives_what_the_open_one_gives() {
    let (field, k) = (Field::Gf2048, 3_usize);
    let data: Vec<Vec<u8>> = (0..k)
        .map(|i| symbols(field, bytes(i as u64 + 3, 64)))
        .collect();
    let open = Code::new(field, k).unwrap();
    let code = strict::Code::new(field, k).unwrap();
    let held: Vec<Secret> = data.iter().map(|shard| secret(shard)).collect();
    let given: Vec<&dyn Held> = held.iter().map(|shard| shard as &dyn Held).collect();
    let refs: Vec<&[u8]> = data.iter().map(Vec::as_slice).collect();
    let made = code.encode(&given, &[0, 1, 2, 3, 4, 5]).unwrap();
    assert_eq!(
        plain(&made),
        plain(&open.encode(&refs, &[0, 1, 2, 3, 4, 5]).unwrap())
    );
    assert!(
        made.iter()
            .all(|shard| shard.is_locked() && shard.is_out_of_children())
    );
    // Rebuilt from the last three, checked by the rest, put right in place.
    let pairs: Vec<(u16, &dyn Held)> = (3..6)
        .map(|i| (i, &made[usize::from(i)] as &dyn Held))
        .collect();
    let back = code.rebuild(&pairs, &[0, 1, 2]).unwrap();
    assert_eq!(plain(&back), data);
    let all: Vec<(u16, &dyn Held)> = (0..6)
        .zip(made.iter().map(|shard| shard as &dyn Held))
        .collect();
    assert_eq!(
        code.redundancy(&all),
        Ok(reed_solomon_core::Redundancy::Agrees)
    );
    let mut read: Vec<Secret> = made.iter().map(|shard| secret(shard)).collect();
    read[4].as_mut_bytes()[1] ^= 1;
    let mut shards: Vec<(u16, &mut dyn HeldMut)> = (0..6)
        .zip(read.iter_mut().map(|shard| shard as &mut dyn HeldMut))
        .collect();
    assert_eq!(
        code.correct(&mut shards),
        Ok(Correction::Corrected(vec![0, 0, 0, 0, 1, 0]))
    );
    assert_eq!(read[4].as_bytes(), made[4].as_slice());
}

/// The card of MHFE-REPAIR-1's second container, k = 4: every word in a secret of its own, the
/// repair words in protected buffers, two smudged words filled and one copied wrong replaced.
#[test]
fn a_card_is_put_right_in_protected_memory() {
    let words: [u16; 12] = [
        0x371, 0x6CE, 0x220, 0x11D, 0x2DB, 0x73E, 0x12B, 0x7C6, 0x7D5, 0x7AF, 0x6A9, 0x453,
    ];
    let code = strict::CyclicCode::new(Field::Gf2048, 12, 4, 1, 1).unwrap();
    let card: Vec<Secret> = words
        .iter()
        .map(|word| secret(&word.to_be_bytes()))
        .collect();
    let given: Vec<&dyn Held> = card.iter().map(|word| word as &dyn Held).collect();
    let repair = code.encode(&given).unwrap();
    assert_eq!(*repair[0], 0x52C_u16.to_be_bytes());
    let mut read: Vec<Secret> = card
        .iter()
        .map(|word| secret(word.as_bytes()))
        .chain(repair.iter().map(|word| secret(word)))
        .collect();
    read[2].as_mut_bytes().fill(0);
    read[7].as_mut_bytes().fill(0);
    read[10]
        .as_mut_bytes()
        .copy_from_slice(&0x7FF_u16.to_be_bytes());
    let marks = [
        Mark {
            shard: 2,
            symbol: 0,
        },
        Mark {
            shard: 7,
            symbol: 0,
        },
    ];
    let shown: Vec<&dyn Held> = read.iter().map(|word| word as &dyn Held).collect();
    let Plan::Changes(changes) = code.plan(&shown, &[], &marks).unwrap() else {
        panic!("the card is within 2e + s <= 4")
    };
    assert!(changes.is_locked());
    // Each change gives its place; what a word becomes is looked at where the list keeps it.
    let kinds: Vec<(usize, ChangeKind)> = changes.iter().map(|c| (c.shard, c.kind)).collect();
    let filled = [(2, ChangeKind::Filled), (7, ChangeKind::Filled)];
    assert_eq!(kinds, [filled[0], filled[1], (10, ChangeKind::Replaced)]);
    assert_eq!(
        changes.values(2, |was, becomes| (was, becomes)),
        Some((0x7FF, 0x6A9))
    );
    let mut shards: Vec<&mut dyn HeldMut> =
        read.iter_mut().map(|w| w as &mut dyn HeldMut).collect();
    code.apply(&mut shards, &changes).unwrap();
    assert!(
        read.iter()
            .zip(&words)
            .all(|(word, &w)| word.as_bytes() == w.to_be_bytes())
    );
}

/// A set written from data in a secret, read back from its files in protected buffers, and its
/// data put together in a protected buffer and in a secret of the program's.
#[test]
fn a_set_is_written_and_rebuilt_in_protected_memory() {
    let data = secret(b"abandon abandon abandon abandon about");
    let facts = strict::SetFacts::for_data(Field::Gf256, b"card", 3, &data, 8, DataDigest::Sha256);
    let facts = facts.unwrap();
    let open = SetFacts::for_data(
        Field::Gf256,
        b"card",
        3,
        data.as_bytes(),
        8,
        DataDigest::Sha256,
    );
    let open = open.unwrap();
    assert_eq!(facts.as_portable(), &open);
    assert_eq!(
        (facts.file_len(), facts.digest()),
        (open.file_len(), open.digest())
    );
    let files = facts.files(&data, &[0, 1, 2, 3, 4]).unwrap();
    assert!(files.iter().all(|file| file.is_locked()));
    let held: Vec<&dyn Held> = files.iter().skip(1).map(|file| file as &dyn Held).collect();
    let pile = strict::Pile::new(&held).unwrap();
    let set = &pile.sets()[0];
    let rebuilt = set.rebuild(None).unwrap();
    assert_eq!(rebuilt.data.as_slice(), data.as_bytes());
    assert!(rebuilt.data.is_locked());
    let mut out = Secret::zeroed(data.as_bytes().len());
    set.rebuild_into(&mut out, None).unwrap();
    assert_eq!(out.as_bytes(), data.as_bytes());
    let lost = set.repair(0, None).unwrap();
    assert_eq!(lost.as_slice(), files[0].as_slice());
    // The same files through the open pile give the same data.
    let views: Vec<&[u8]> = files.iter().skip(1).map(Buffer::as_slice).collect();
    let open = Pile::new(&views).unwrap();
    assert_eq!(open.sets()[0].rebuild(None).unwrap().data, rebuilt.data);
    let read = strict::read(&files[2]).unwrap();
    assert_eq!(read.header().index(), 2);
    assert_eq!(read.header().set(), facts);
    // The root calls, for data a program treats as public, only by an explicit step.
    let portable = facts.as_portable().files(data.as_bytes(), &[2]).unwrap();
    assert_eq!(portable[0], files[2]);
    let header = read.header();
    assert_eq!(header.as_portable().as_bytes(), header.as_bytes());
    let open_read = reed_solomon_core::ShardFile::read(&files[2]).unwrap();
    assert_eq!(read.block(0), Ok(open_read.block(0)));
    assert_eq!(read.survey(), Ok(open_read.survey()));
    assert!(read.survey().unwrap().whole());
    let readings = pile.readings();
    assert!(matches!(&readings[0], Ok(first) if first.survey() == Ok(open_read.survey())));
}

/// A strict rebuild of data that does not have the stated digest says so without the digest the
/// data has, which follows from the data; an output given is left with zeros, as after any
/// refusal during the work.
#[test]
fn a_strict_rebuild_names_no_digest_of_the_data() {
    let open = SetFacts::for_data(Field::Gf256, b"d", 3, b"abd", 1, DataDigest::Sha256).unwrap();
    let stated = *open.digest().unwrap();
    let facts = strict::SetFacts::new(Field::Gf256, b"d", 3, 3, 1, Some(stated)).unwrap();
    let files: Vec<Buffer> = (0..3_u8)
        .map(|index| {
            let shard = secret(&[b"abc"[usize::from(index)]]);
            facts.header(index.into()).unwrap().file(&shard).unwrap()
        })
        .collect();
    let held: Vec<&dyn Held> = files.iter().map(|file| file as &dyn Held).collect();
    let pile = strict::Pile::new(&held).unwrap();
    let set = &pile.sets()[0];
    assert!(matches!(set.rebuild(None), Err(SetError::DigestDiffers { stated: s }) if s == stated));
    let mut out = Secret::zeroed(3);
    out.as_mut_bytes().fill(0xEE);
    let answer = set.rebuild_into(&mut out, None);
    assert_eq!(answer, Err(SetError::DigestDiffers { stated }));
    assert_eq!(out.as_bytes(), [0, 0, 0]);
    // The open call keeps giving both digests, for data that is no secret.
    let views: Vec<&[u8]> = files.iter().map(Buffer::as_slice).collect();
    let open_pile = Pile::new(&views).unwrap();
    let refused = open_pile.sets()[0].rebuild(None);
    assert!(matches!(refused, Err(SetError::Digest { stated: s, .. }) if s == stated));
}

/// Stripes, checks and plans of the strict mode answer as the open calls on the same bytes; a
/// plan made in it is the open one, bound to the same shards.
#[test]
fn strict_stripes_checks_and_plans_answer_as_the_open_ones() {
    let open = Code::new(Field::Gf256, 2).unwrap();
    let code = strict::Code::new(Field::Gf256, 2).unwrap();
    let data = [secret(b"abcd"), secret(b"efgh")];
    let refs: [&[u8]; 2] = [b"abcd", b"efgh"];
    let made = open.encode(&refs, &[0, 1, 2, 3]).unwrap();
    let mut weights = [0; 2];
    let sources = code.sources(&[0, 1], &mut weights).unwrap();
    let stripes: [&dyn Held; 2] = [&data[0], &data[1]];
    let mut built = Secret::zeroed(4);
    sources.build(3, &stripes, &mut built).unwrap();
    assert_eq!(built.as_bytes(), made[3].as_slice());
    assert_eq!(sources.agrees(3, &stripes, &built), Ok(true));
    // A gap marked in shard 0: the strict plan is the open one, and it puts the shard right.
    let mut read: Vec<Secret> = made.iter().map(|shard| secret(shard)).collect();
    read[0].as_mut_bytes()[2] = 0;
    let marks = [Mark {
        shard: 0,
        symbol: 2,
    }];
    let views: Vec<(u16, &[u8])> = (0..).zip(read.iter().map(Secret::as_bytes)).collect();
    let shown: Vec<(u16, &dyn Held)> = (0..).zip(read.iter().map(|s| s as &dyn Held)).collect();
    let plan = code.plan(&shown, &marks).unwrap();
    assert_eq!(plan, open.plan(&views, &marks).unwrap());
    let Plan::Changes(changes) = plan else {
        panic!("one gap, two spare shards")
    };
    let mut shards: Vec<(u16, &mut dyn HeldMut)> = (0..)
        .zip(read.iter_mut().map(|s| s as &mut dyn HeldMut))
        .collect();
    code.apply(&mut shards, &changes).unwrap();
    assert_eq!(read[0].as_bytes(), b"abcd");
    // A word of the cyclic code checked, then put right in place.
    let cyclic = strict::CyclicCode::new(Field::Gf2048, 3, 2, 1, 1).unwrap();
    let words = [0x123_u16, 0x456, 0x789].map(|word| secret(&word.to_be_bytes()));
    let given: Vec<&dyn Held> = words.iter().map(|word| word as &dyn Held).collect();
    let check = cyclic.encode(&given).unwrap();
    let mut word: Vec<Secret> = words
        .iter()
        .map(|symbol| secret(symbol.as_bytes()))
        .chain(check.iter().map(|symbol| secret(symbol)))
        .collect();
    let all: Vec<&dyn Held> = word.iter().map(|symbol| symbol as &dyn Held).collect();
    assert_eq!(cyclic.agrees(&all), Ok(true));
    word[1].as_mut_bytes()[1] ^= 1;
    let mut shards: Vec<&mut dyn HeldMut> =
        word.iter_mut().map(|w| w as &mut dyn HeldMut).collect();
    let answer = cyclic.correct(&mut shards, &[]);
    assert_eq!(answer, Ok(Correction::Corrected(vec![0, 1, 0, 0, 0])));
    assert_eq!(word[1].as_bytes(), 0x456_u16.to_be_bytes());
}

/// The strict writers of files, the repair of several files, the vote and a set without one of
/// its files answer as the open calls do.
#[test]
fn strict_writers_repairs_and_votes_answer_as_the_open_ones() {
    let data = secret(b"twenty-six bytes of a text");
    let digest = DataDigest::Sha256;
    let facts = strict::SetFacts::for_data(Field::Gf256, b"set", 3, &data, 8, digest).unwrap();
    let files = facts.files(&data, &[0, 1, 2, 3, 4]).unwrap();
    let length = usize::try_from(facts.file_len()).unwrap();
    let mut outs = [Secret::zeroed(length), Secret::zeroed(length)];
    let mut views: Vec<&mut dyn HeldMut> = outs.iter_mut().map(|o| o as &mut dyn HeldMut).collect();
    facts.files_into(&data, &[3, 4], &mut views).unwrap();
    assert_eq!(
        [outs[0].as_bytes(), outs[1].as_bytes()],
        [&*files[3], &*files[4]]
    );
    // One file from its shard, into a buffer and into a secret.
    let pieces: Vec<&[u8]> = b"twenty-six bytes of a text\0".chunks(9).collect();
    let shard = Code::new(Field::Gf256, 3).unwrap().encode(&pieces, &[4]);
    let shard = secret(&shard.unwrap()[0]);
    let header = facts.header(4).unwrap();
    assert_eq!(header.file(&shard).unwrap(), files[4]);
    let mut one = Secret::zeroed(length);
    header.file_into(&shard, &mut one).unwrap();
    assert_eq!(one.as_bytes(), &*files[4]);
    // Files 1 to 4: two files made again, into buffers and into secrets.
    let held: Vec<&dyn Held> = files.iter().skip(1).map(|file| file as &dyn Held).collect();
    let pile = strict::Pile::new(&held).unwrap();
    let set = &pile.sets()[0];
    assert_eq!(set.repairs(&[0, 1], None).unwrap(), files[..2]);
    let mut views: Vec<&mut dyn HeldMut> = outs.iter_mut().map(|o| o as &mut dyn HeldMut).collect();
    set.repairs_into(&[0, 1], &mut views, None).unwrap();
    assert_eq!(
        [outs[0].as_bytes(), outs[1].as_bytes()],
        [&*files[0], &*files[1]]
    );
    // The four agree: the vote names none, and the set without file 1 still rebuilds.
    let vote = set.vote(None).unwrap();
    assert!(vote.outvoted().is_empty() && vote.undecided() == 0);
    let rebuilt = set.without(&[0]).unwrap().rebuild(None).unwrap();
    assert_eq!(rebuilt.data.as_slice(), data.as_bytes());
    // A second copy of file 3: the set without file 1 still holds both, which agree.
    let copy = secret(&files[3]);
    let mut held: Vec<&dyn Held> = files.iter().skip(1).map(|file| file as &dyn Held).collect();
    held.push(&copy);
    let pile = strict::Pile::new(&held).unwrap();
    let fewer = pile.sets()[0].without(&[0]).unwrap();
    assert!(fewer.disputes().is_empty() && fewer.members().len() == 4);
    let rebuilt = fewer.rebuild(None).unwrap();
    assert_eq!(rebuilt.data.as_slice(), data.as_bytes());
}

/// Says in the test log why a check the system cannot show was left out.
#[expect(
    clippy::print_stderr,
    reason = "a check left out says so in the test log"
)]
fn skip(why: &str) {
    eprintln!("skipped: {why}");
}

/// `length` bytes of 0xEE in a secret: an output that a refusal must leave as it was.
fn filled(length: usize) -> Secret {
    let mut secret = Secret::zeroed(length);
    secret.as_mut_bytes().fill(0xEE);
    secret
}

/// Inputs in protected memory for every strict call, made while the lock limit has room.
struct Ready {
    code: strict::Code,
    shards: [Secret; 3],
    piece: Secret,
    plan: Changes,
    cyclic: strict::CyclicCode,
    word: [Secret; 4],
    facts: strict::SetFacts,
    data: Secret,
    shard: Secret,
    out: Secret,
    whole: Secret,
}

/// The inputs, or `None` if the plan of one gap with one spare shard lists no change.
fn ready() -> Option<Ready> {
    let refs: [&[u8]; 2] = [b"ab", b"cd"];
    let made = Code::new(Field::Gf256, 2)
        .unwrap()
        .encode(&refs, &[0, 1, 2]);
    let made = made.unwrap();
    let shards = [secret(&made[0]), secret(&made[1]), secret(&made[2])];
    let code = strict::Code::new(Field::Gf256, 2).unwrap();
    let shown: Vec<(u16, &dyn Held)> = (0..).zip(shards.iter().map(|s| s as &dyn Held)).collect();
    let mark = Mark {
        shard: 0,
        symbol: 0,
    };
    let Ok(Plan::Changes(plan)) = code.plan(&shown, &[mark]) else {
        return None;
    };
    let cyclic = strict::CyclicCode::new(Field::Gf2048, 2, 2, 1, 1).unwrap();
    let [first, second] = [0x123_u16, 0x456].map(|word| secret(&word.to_be_bytes()));
    let check = cyclic.encode(&[&first, &second]).unwrap();
    let word = [first, second, secret(&check[0]), secret(&check[1])];
    let data = secret(b"twenty-six bytes of a text");
    let digest = DataDigest::Sha256;
    let facts = strict::SetFacts::for_data(Field::Gf256, b"set", 3, &data, 8, digest).unwrap();
    let pieces: Vec<&[u8]> = b"twenty-six bytes of a text\0".chunks(9).collect();
    let shard = Code::new(Field::Gf256, 3).unwrap().encode(&pieces, &[3]);
    let length = usize::try_from(facts.file_len()).unwrap();
    Some(Ready {
        code,
        shards,
        piece: filled(2),
        plan,
        cyclic,
        word,
        facts,
        data,
        shard: secret(&shard.unwrap()[0]),
        out: filled(length),
        whole: filled(26),
    })
}

/// With nothing more the system will lock, every strict call refuses, before its work, and leaves
/// the outputs of the program as they were: each runs as a strict call, on a stack of its own.
fn every_call_refuses(ready: &mut Ready, pile: &strict::Pile<'_>, files: &[Buffer]) {
    let set = &pile.sets()[0];
    let shown: Vec<(u16, &dyn Held)> = (0..)
        .zip(ready.shards.iter().map(|s| s as &dyn Held))
        .collect();
    let pair = [shown[0].1, shown[1].1];
    assert_eq!(ready.code.encode(&pair, &[2]), Err(CodeError::Unprotected));
    assert_eq!(
        ready.code.rebuild(&shown[..2], &[2]),
        Err(CodeError::Unprotected)
    );
    assert_eq!(ready.code.redundancy(&shown), Err(CodeError::Unprotected));
    assert_eq!(ready.code.plan(&shown, &[]), Err(CodeError::Unprotected));
    let mut weights = [0; 2];
    let sources = ready.code.sources(&[0, 1], &mut weights).unwrap();
    assert_eq!(
        sources.agrees(2, &pair, shown[2].1),
        Err(CodeError::Unprotected)
    );
    assert_eq!(
        sources.build(2, &pair, &mut ready.piece),
        Err(CodeError::Unprotected)
    );
    assert_eq!(
        strict::values(&ready.plan, 0, |was, _| was),
        Err(CodeError::Unprotected)
    );
    let mut pairs: Vec<(u16, &mut dyn HeldMut)> = (0..)
        .zip(ready.shards.iter_mut().map(|s| s as &mut dyn HeldMut))
        .collect();
    assert_eq!(ready.code.correct(&mut pairs), Err(CodeError::Unprotected));
    assert_eq!(
        ready.code.apply(&mut pairs, &ready.plan),
        Err(CodeError::Unprotected)
    );
    let word: Vec<&dyn Held> = ready.word.iter().map(|w| w as &dyn Held).collect();
    assert_eq!(ready.cyclic.encode(&word[..2]), Err(CodeError::Unprotected));
    assert_eq!(ready.cyclic.agrees(&word), Err(CodeError::Unprotected));
    assert_eq!(
        ready.cyclic.plan(&word, &[], &[]),
        Err(CodeError::Unprotected)
    );
    let mut symbols: Vec<&mut dyn HeldMut> = ready
        .word
        .iter_mut()
        .map(|w| w as &mut dyn HeldMut)
        .collect();
    assert_eq!(
        ready.cyclic.correct(&mut symbols, &[]),
        Err(CodeError::Unprotected)
    );
    assert_eq!(
        ready.cyclic.apply(&mut symbols, &ready.plan),
        Err(CodeError::Unprotected)
    );
    // Files and sets.
    let digest = DataDigest::Sha256;
    let facts = strict::SetFacts::for_data(Field::Gf256, b"set", 3, &ready.data, 8, digest);
    assert_eq!(facts, Err(FileError::Unprotected));
    assert_eq!(
        ready.facts.files(&ready.data, &[0]),
        Err(FileError::Unprotected)
    );
    let header = ready.facts.header(3).unwrap();
    assert_eq!(header.file(&ready.shard), Err(FileError::Unprotected));
    assert!(matches!(
        strict::read(&files[0]),
        Err(FileError::Unprotected)
    ));
    let held: Vec<&dyn Held> = files.iter().map(|file| file as &dyn Held).collect();
    assert!(matches!(
        strict::Pile::new(&held),
        Err(FileError::Unprotected)
    ));
    assert!(matches!(set.rebuild(None), Err(SetError::Unprotected)));
    assert_eq!(set.repair(0, None), Err(SetError::Unprotected));
    assert_eq!(set.repairs(&[0], None), Err(SetError::Unprotected));
    assert!(matches!(set.vote(None), Err(SetError::Unprotected)));
    assert!(matches!(set.without(&[0]), Err(FileError::Unprotected)));
    let reading = pile.readings()[0].as_ref().unwrap();
    assert_eq!(reading.block(0), Err(FileError::Unprotected));
    assert_eq!(reading.survey(), Err(FileError::Unprotected));
    let answer = header.file_into(&ready.shard, &mut ready.out);
    assert_eq!(answer, Err(FileError::Unprotected));
    let mut outs: [&mut dyn HeldMut; 1] = [&mut ready.out];
    let answer = ready.facts.files_into(&ready.data, &[0], &mut outs);
    assert_eq!(answer, Err(FileError::Unprotected));
    assert_eq!(
        set.repairs_into(&[0], &mut outs, None),
        Err(SetError::Unprotected)
    );
    let answer = set.rebuild_into(&mut ready.whole, None);
    assert_eq!(answer, Err(SetError::Unprotected));
    let untouched = |secret: &Secret| secret.as_bytes().iter().all(|&byte| byte == 0xEE);
    assert!(untouched(&ready.out) && untouched(&ready.whole) && untouched(&ready.piece));
}

/// Memory the system does not protect is refused, whether a caller gives it or the call would
/// make it, before a byte is written; on Linux, in a process of its own whose lock limit runs out.
#[test]
fn strict_calls_refuse_memory_the_system_does_not_protect() {
    if !cfg!(target_os = "linux") {
        return skip("the lock limit is lowered on Linux only");
    }
    if std::env::var_os("STRICT_CHILD").is_none() {
        // The child's soft lock limit is lowered to 4 MiB where the hard one allows it.
        let name = "strict::strict_calls_refuse_memory_the_system_does_not_protect";
        let status = std::process::Command::new("sh")
            .args(["-c", "ulimit -S -l 4096 2>/dev/null; exec \"$0\" \"$@\""])
            .arg(std::env::current_exe().unwrap())
            .args([name, "--exact", "--test-threads=1", "--nocapture"])
            .env("STRICT_CHILD", "1")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let code = strict::Code::new(Field::Gf256, 2).unwrap();
    // Two data shards and every input, locked while the limit has room.
    let data = [secret(&[7; 256 * 1024]), secret(&[9; 256 * 1024])];
    let small = [secret(&[1; 16]), secret(&[2; 16])];
    let mut ready = ready().unwrap();
    let files = ready.facts.files(&ready.data, &[0, 1, 2, 3]).unwrap();
    let held: Vec<&dyn Held> = files.iter().map(|file| file as &dyn Held).collect();
    let pile = strict::Pile::new(&held).unwrap();
    // A set of one shard of 192 KiB in one block: a rebuild holds a block of 192 KiB.
    let big = secret(&vec![5; 192 * 1024]);
    let facts = strict::SetFacts::new(Field::Gf256, b"big", 1, 192 * 1024, 192 * 1024, None);
    let big_files = facts.unwrap().files(&big, &[0]).unwrap();
    drop(big);
    let big_held: [&dyn Held; 1] = [&big_files[0]];
    let big_pile = strict::Pile::new(&big_held).unwrap();
    let mut big_out = filled(192 * 1024);
    if !data.iter().chain(&small).all(Held::guarded) {
        return skip("the lock limit holds less than the data shards");
    }
    // Pages of 64 KiB, then of 4 KiB, until one is not locked: then the limit is spent.
    let (mut fillers, mut pages) = (Vec::new(), Vec::new());
    for (size, kept) in [(64 * 1024, &mut fillers), (4096, &mut pages)] {
        loop {
            let filler = WorkArea::new(size).unwrap();
            if !filler.is_locked() {
                break;
            }
            kept.push(filler);
            if kept.len() == 1024 {
                return skip("the process locks memory without a limit");
            }
        }
    }
    every_call_refuses(&mut ready, &pile, &files);
    // A buffer given by the caller that is not locked is refused, and so is a plan of the root
    // calls made now, in pages that are not locked either.
    let loose = WorkArea::new(64 * 1024).unwrap();
    assert!(!Held::guarded(&loose));
    // The guard check in the protected-memory example refuses the buffer before it receives a secret.
    assert!(!Held::guarded(&Secret::with_capacity(2)));
    let given: [&dyn Held; 2] = [&data[0], &loose];
    assert_eq!(code.encode(&given, &[2]), Err(CodeError::Unprotected));
    let views: Vec<(u16, &[u8])> = (0..)
        .zip(ready.shards.iter().map(Secret::as_bytes))
        .collect();
    let mark = Mark {
        shard: 0,
        symbol: 0,
    };
    let Ok(Plan::Changes(open_plan)) = Code::new(Field::Gf256, 2).unwrap().plan(&views, &[mark])
    else {
        panic!("one gap, one spare shard")
    };
    // Room for the protected stack of a call (512 KiB), not for its result of 256 KiB: the call
    // refuses its own buffer. A plan in pages that are not locked is refused where one that is
    // locked is read.
    fillers.truncate(fillers.len().saturating_sub(10));
    let few: [&dyn Held; 2] = [&small[0], &small[1]];
    assert_eq!(code.encode(&few, &[2]).unwrap().len(), 1);
    // The stack fits, the block of 192 KiB does not: the rebuild refuses before it writes.
    let answer = big_pile.sets()[0].rebuild_into(&mut big_out, None);
    assert_eq!(answer, Err(SetError::Unprotected));
    assert!(big_out.as_bytes().iter().all(|&byte| byte == 0xEE));
    let mut pairs: Vec<(u16, &mut dyn HeldMut)> = (0..)
        .zip(ready.shards.iter_mut().map(|s| s as &mut dyn HeldMut))
        .collect();
    assert!(!open_plan.is_locked());
    assert_eq!(
        code.apply(&mut pairs, &open_plan),
        Err(CodeError::Unprotected)
    );
    assert_eq!(
        strict::values(&open_plan, 0, |was, _| was),
        Err(CodeError::Unprotected)
    );
    assert_eq!(strict::values(&ready.plan, 0, |was, _| was), Ok(Some(0x61)));
    let given: [&dyn Held; 2] = [&data[0], &data[1]];
    assert_eq!(code.encode(&given, &[2]), Err(CodeError::Unprotected));
    // With the room back, the same call is made.
    fillers.clear();
    assert_eq!(code.encode(&given, &[2]).unwrap().len(), 1);
}

/// What a program shows before the work: the code, the facts of a set, the header of a file, its
/// members and the work a call will take. Every strict answer is the open one, and memory of a
/// `WorkArea` serves as input and output of a strict call.
#[test]
fn strict_facts_and_counts_answer_as_the_open_ones() {
    let code = strict::Code::new(Field::Gf2048, 3).unwrap();
    let open = Code::new(Field::Gf2048, 3).unwrap();
    assert_eq!(
        (code.field(), code.threshold()),
        (open.field(), open.threshold())
    );
    assert_eq!(code.correction_memory(7), open.correction_memory(7));
    let cyclic = strict::CyclicCode::new(Field::Gf2048, 12, 4, 1, 1).unwrap();
    let open_cyclic = reed_solomon_core::CyclicCode::new(Field::Gf2048, 12, 4, 1, 1).unwrap();
    let shape = (
        cyclic.field(),
        cyclic.data(),
        cyclic.parity(),
        cyclic.length(),
    );
    assert_eq!(shape, (Field::Gf2048, 12, 4, 16));
    assert_eq!(cyclic.generator(), open_cyclic.generator());
    // A set of five files, one of them twice.
    let data = secret(b"twenty-six bytes of a text");
    let digest = DataDigest::Sha256;
    let facts = strict::SetFacts::for_data(Field::Gf256, b"facts", 3, &data, 8, digest).unwrap();
    let portable = *facts.as_portable();
    assert_eq!(facts.code().threshold(), portable.code().threshold());
    assert_eq!(
        (facts.id(), facts.data_len()),
        (portable.id(), portable.data_len())
    );
    assert_eq!(
        (facts.block_len(), facts.digest()),
        (portable.block_len(), portable.digest())
    );
    assert_eq!(
        (facts.shard_len(), facts.blocks()),
        (portable.shard_len(), portable.blocks())
    );
    let header = facts.header(4).unwrap();
    assert_eq!((header.set(), header.index()), (facts, 4));
    let mut files = facts.files(&data, &[0, 1, 2, 3, 4]).unwrap();
    files.push(facts.files(&data, &[4]).unwrap().remove(0));
    let held: Vec<&dyn Held> = files.iter().map(|file| file as &dyn Held).collect();
    let views: Vec<&[u8]> = files.iter().map(Buffer::as_slice).collect();
    let (pile, open_pile) = (
        strict::Pile::new(&held).unwrap(),
        Pile::new(&views).unwrap(),
    );
    let (set, open_set) = (&pile.sets()[0], &open_pile.sets()[0]);
    assert_eq!(set.facts(), facts);
    assert_eq!(
        (set.id_shared(), set.members()),
        (open_set.id_shared(), open_set.members())
    );
    assert_eq!(
        (set.disputes(), set.coverage()),
        (open_set.disputes(), open_set.coverage())
    );
    let stretches: Vec<_> = set.stretches().unwrap().collect();
    assert_eq!(stretches, open_set.stretches().unwrap().collect::<Vec<_>>());
    assert_eq!(set.disputed_blocks(4).count(), 0);
    assert_eq!(set.rebuild_work(), open_set.rebuild_work());
    assert_eq!(set.repair_work(1), open_set.repair_work(1));
    assert_eq!(set.repairs_work(&[1, 2]), open_set.repairs_work(&[1, 2]));
    assert_eq!(set.vote_work(), open_set.vote_work());
    // A shard built from data in work areas into a work area.
    let mut weights = [0; 3];
    let sources = code.sources(&[0, 1, 2], &mut weights).unwrap();
    let areas = [0x011_u16, 0x222, 0x433].map(|word| {
        let mut area = WorkArea::new(2).unwrap();
        area.as_mut_slice().copy_from_slice(&word.to_be_bytes());
        area
    });
    let [first, second, third] = &areas;
    let stripes: [&dyn Held; 3] = [first, second, third];
    let mut out = WorkArea::new(2).unwrap();
    sources.build(5, &stripes, &mut out).unwrap();
    let refs: [&[u8]; 3] = [first.as_slice(), second.as_slice(), third.as_slice()];
    assert_eq!(
        out.as_slice(),
        open.encode(&refs, &[5]).unwrap()[0].as_slice()
    );
}
