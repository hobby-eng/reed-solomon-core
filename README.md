# reed-solomon-core

> [!WARNING]
> **Draft, not ready for production.** reed-solomon-core is an early draft: the interface will still change, the shard file format is a draft, and nobody outside the project has reviewed it. Do not rely on it for data you cannot afford to lose.

[![CI](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/ci.yml)
[![RustSec audit](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/audit.yml/badge.svg)](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/audit.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

reed-solomon-core is a Reed–Solomon library for sets of shards. Data cut into `k` shards becomes a set of up to 2^m shards in a field GF(2^m) the program chooses, from 4 to 65,536, and any `k` of them rebuild it. A lost shard can be made again, a set can be given more shards later, and wrong bytes at unknown places can be found and corrected, together with gaps a reader marks. A shard can be kept in a shard file, which names its set and checks itself block by block. It depends on `sha2` and `zeroize`, and by default on `secret-protection-core`, which keeps data in protected memory pages and brings the standard library. The crate holds no `unsafe` code, and without its default features it works without an allocator.

The [documentation index](docs/README.md) lists the guides; [How it works](docs/TECHNICAL.md) has the mathematics and the guarantees.

Line coverage on Linux x86-64, measured on 2026-11-15 with the default features, is **98.35%** for the unit tests and the tests of the public interface (source fingerprint `5f5a9adb`). [Test scope and per-file results](docs/COVERAGE.md).

## Contents

- [What it is for](#what-it-is-for)
- [What it does](#what-it-does)
- [Two constructions](#two-constructions)
- [Fields](#fields)
- [Shard files](#shard-files)
- [What it does not do](#what-it-does-not-do)
- [Limits](#limits)
- [Use from Rust](#use-from-rust)
- [Self-test](#self-test)
- [License](#license)

## What it is for

The crate keeps data recoverable where parts of it get lost or damaged: shards spread over files, photos, disks or places, and short codes written or printed by hand, such as the words of a recovery phrase, where a word may be unreadable or copied wrong. It is the shared core of programs that need this, and the choices behind it favour checks over speed: every result is checked before it is given back, every bound is stated, and nothing is guessed.

Most Reed–Solomon crates for Rust do one of two things. The erasure coders (`reed-solomon-erasure`, `reed-solomon-simd`, `reed-solomon-novelpoly`, `leopard-codec`) rebuild shards that are known to be missing, many of them very fast, but do not find a shard that is present and wrong. The error correctors (`reed-solomon`, `reed_solomon_rs`) find wrong symbols in one short message of bytes. PAR2 implementations such as `rust-par2` protect existing files with separate recovery files in their own format. reed-solomon-core brings these together and adds what we found in none of them:

| Feature                                                                                                                         | Here | Elsewhere among the crates above            |
| ------------------------------------------------------------------------------------------------------------------------------- | ---- | ------------------------------------------- |
| Missing shards rebuilt, and wrong symbols at unknown places found and corrected, on the same set                                | yes  | one or the other                            |
| Gaps a reader marks inside a shard, one symbol at a time, together with wrong symbols: `2e + s ≤ n − k`                         | yes  | gaps only as whole shards                   |
| A plan of every change (place, what was read, what it becomes) before anything changes, applied after a check                   | yes  | not found                                   |
| A field of 2,048 elements: one symbol is one word of a list of 2,048, such as the BIP39 list                                    | yes  | not found                                   |
| A field of any size from 4 to 65,536 elements, by a standard primitive polynomial or one the program gives and the crate checks | yes  | GF(2^8) and GF(2^16) only                   |
| The bytes of `reed-solomon-erasure` in GF(2^8) and GF(2^16), checked shard by shard                                             | yes  | that crate itself                           |
| A set given more shards later from any `k` at hand, the shards already issued unchanged                                         | yes  | by encoding again with more parity          |
| A shard file that names its set, with a checksum per block tied to its place and the header at both ends                        | yes  | PAR2 has its own format of recovery files   |
| Data rebuilt block by block from whichever files still hold each block, even when no `k` files are whole                        | yes  | PAR2 works by slices of the protected files |
| The files that differ from what the others share named block by block (a vote), and the set rebuilt without them                | yes  | not found                                   |
| The SHA-256 of the data computed when written and compared before the data is given back                                        | yes  | PAR2 compares MD5                           |
| A limit on the work of a call, checked before it starts; a stated bound on working memory; no partial result                    | yes  | not found                                   |
| No allocator needed, work on the caller's buffers; where shards are scaled, no byte of them chooses a branch or a table entry   | yes  | some are `no_std`; the rest not stated      |
| A self-test against answers from outside the crate, for a program to run at start                                               | yes  | not found                                   |
| Every buffer that held data wiped, results in self-wiping buffers, no data in `Debug` texts                                     | yes  | not found                                   |

The comparison was made from the documentation of each crate at its latest version on crates.io in November 2026; "not found" means we did not find it there.

What the others do better. The fast erasure coders work in O(n log n) and with SIMD: for thousands of shards and gigabytes of data they are much faster than this crate, whose work grows with `k` times the number of shards. PAR2 is a format many programs read.

## What it does

A set is written from `k` data shards of equal length. The first `k` shards of the set are the data itself; every further shard is computed from them, by coefficients that depend only on its number and on `k`, not on how many shards the set has. So the shards already handed out never change when more are made.

| Task                                          | Call                                                            | What it needs                      |
| --------------------------------------------- | --------------------------------------------------------------- | ---------------------------------- |
| Write a set                                   | `Code::encode`                                                  | the `k` data shards                |
| Get the data back, or make a lost shard again | `Code::rebuild`                                                 | any `k` shards                     |
| Give a set more shards later                  | `Code::rebuild` with new numbers                                | any `k` shards                     |
| Check that shards belong to one set           | `Code::redundancy`                                              | more than `k` shards               |
| Find and correct wrong bytes                  | `Code::correct`                                                 | more than `k` shards               |
| Plan corrections with marked gaps, then apply | `Code::plan`, then `Code::apply`                                | more than `k` shards               |
| Work on a large file piece by piece           | `Code::sources`, then `Sources::build` and `Sources::agrees`    | the caller's own buffers           |
| Keep a shard in a file that describes itself  | `Header::file`                                                  | one shard and its set              |
| Read such a file, damaged or cut short        | `ShardFile::read`                                               | what is left of the file           |
| Write all the files of a set                  | `SetFacts::files`                                               | the data                           |
| Sort files into sets and rebuild the data     | `Pile::new`, then `FileSet::rebuild`                            | any files, in any order            |
| Make a lost file again                        | `FileSet::repair`                                               | files that hold `k` of every block |
| Make several lost files in one pass           | `FileSet::repairs`                                              | files that hold `k` of every block |
| Write, rebuild or repair files beyond memory  | `SetFacts::files_into`, `FileSet::rebuild_into`, `repairs_into` | mappings of the files and outputs  |
| Tell which files differ from the rest         | `FileSet::vote`, then `FileSet::without`                        | spare files beyond `k`             |
| Add check symbols after data, as QR codes do  | `CyclicCode::encode`                                            | the data                           |
| Put such words right, with marked gaps        | `CyclicCode::plan`, then `CyclicCode::apply`                    | every shard of the word            |

Correction finds wrong symbols without being told where they are. With `m` shards at hand it puts right up to `(m − k) / 2` wrong symbols at every position of the shards; shards that are missing are simply not given. In short, a set of `n` shards survives `e` wrong and `s` missing shards when `2e + s ≤ n − k`. Either every position is corrected or nothing is changed.

A reader often knows that one symbol is unreadable, such as a smudged word on a card, while the rest of its shard is fine. `Code::plan` takes such marks: a marked symbol costs one spare shard where a wrong one costs two, and the plan lists every change, with the place, what was read there and what it becomes, before anything is touched. `Code::apply` makes the changes after checking that the shards still hold what the plan was made for.

## Two constructions

The crate makes Reed–Solomon codes in two ways, and a program chooses one by its type; nothing is guessed from the symbols. Both put a word right while `2e + s ≤` the number of check symbols, take marks of unreadable symbols, and give a plan of every change before anything is written. Their check symbols differ, so a word made one way is checked only the same way.

- **`Code`, by points.** Shard `i` is the value at point `i` of one polynomial, and the data shards are the values at the first `k` points. Any `k` shards rebuild every other one, a lost shard can be made again, and a set can be given more shards later, up to the size of the field, with the shards already handed out unchanged. Choose it to spread data over several carriers (files, photos, places) and rebuild it from whichever are left. Shard files use it.
- **`CyclicCode`, by remainder.** The data stays as it is, and the check symbols after it are the remainder of the data divided by a generator polynomial, as QR codes, CDs and printed repair cards compute them. A word has a fixed length, at most the order of the code's roots. Choose it to add check symbols to data that stays where it is, and whenever a format fixes the code: a QR code is `CyclicCode::new(Field::Gf256, data, check, 0, 1)` with its numbers of data and check codewords, and the repair words of MHFE-REPAIR-1 are `CyclicCode::new(Field::Gf2048, words, repair, 1, 1)` with the numbers of a card's words and of its repair words.

```rust
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
let Plan::Changes(changes) = code.plan(&views, &[Mark { shard: 2, symbol: 0 }])? else {
    return Err("the card is within 2e + s ≤ 4".into());
};
let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(|word| word.as_mut_slice()).collect();
code.apply(&mut buffers, &changes)?;
assert_eq!((read[2], read[10]), (0x220_u16.to_be_bytes(), 0x6A9_u16.to_be_bytes()));
```

The same twelve words get other repair words from `Code` (0x143, 0x384, 0x313, 0x6DE), which a card made by the cyclic code does not check.

## Fields

| Field                 | Symbol                                                    | Most shards | Same bytes as                                                                        |
| --------------------- | --------------------------------------------------------- | ----------- | ------------------------------------------------------------------------------------ |
| `Field::Gf256`        | one byte                                                  | 256         | `reed-solomon-erasure` 6.0.0, a port of Backblaze's and Klaus Post's implementations |
| `Field::Gf65536`      | two bytes                                                 | 65,536      | `galois_16` of `reed-solomon-erasure` 6.0.0                                          |
| `Field::Gf2048`       | a number below 2,048 in two bytes                         | 2,048       | values computed independently, pinned in the tests                                   |
| `Field::binary(m, …)` | a number below 2^m: one byte up to m = 8, two bytes above | 2^m         | values computed independently for every degree, pinned in the tests                  |

The tests compare every shard with that crate for all shapes up to 16 shards in its two fields, for large sets, and for each of the 65,536 shard numbers. `Field::Gf2048` makes one symbol one word of a list of 2,048 words, so the shards a code adds to such a list are words of it too; bytes that hold a larger number are refused.

A program can also choose the size of the field: `Field::binary(m, None)` is GF(2^m) for any degree `m` from 2 to 16 by its standard primitive polynomial, and `Field::binary(m, Some(polynomial))` by a primitive polynomial of its own, which the crate checks. The size limits only how many shards a set can have, 2^m; a larger field makes symbols longer and work slower, so the smallest that is large enough is the best. These fields have no shard file.

## Shard files

A shard file holds one shard and everything needed to use it later: an identifier of the set that the program chooses, `k`, the shard's number, the length of the data and, if the program asks for it, the SHA-256 of the data, which the crate computes when it writes the set and compares when it rebuilds the data. The shard is stored in blocks. Each block has a CRC-32C checksum that also covers the file's header and the block's number, and the header is written a second time at the end of the file.

`Header::file` writes a shard file. `ShardFile::read` reads one that may be damaged. It works from whichever copy of the header holds and tells for every block whether it is intact, damaged or missing: one wrong bit costs one block, and a file that lost its end keeps the blocks that are wholly there. When both copies of the header hold and differ, the file is refused; one is never chosen over the other.

```rust
use reed_solomon_core::{Block, Field, Header, SetFacts, ShardFile};

// Shard 4 of a set of ten bytes with threshold 3, in blocks of four bytes.
let set = SetFacts::new(Field::Gf256, b"backup-1", 3, 10, 4, None)?;
let header = Header::new(set, 4)?;
let mut file = header.file(&[0xF4, 0x00, 0x00, 0x00])?;

// A bit goes wrong in the first header: the copy at the end is believed.
file[9] ^= 0x01;
let read = ShardFile::read(&file)?;
assert_eq!(read.header(), &header);
assert_eq!(read.block(0), Some(Block::Intact(&[0xF4, 0x00, 0x00, 0x00])));
assert!(!read.survey().whole());
```

`SetFacts::files` writes the files of a whole set from its data, and `Pile::new` takes any files in any order and sorts them into the sets they belong to. `FileSet::rebuild` then puts the data together block by block: each block comes from any `k` files that still hold it, so a set survives damage that leaves no `k` files whole. Every spare file is compared with the result, and files that read without a fault but do not belong together are refused and named. `FileSet::repair` makes a lost file again, byte for byte, or a further file for the set.

```rust
use reed_solomon_core::{Field, Pile, SetFacts};

// Five files of a set of twenty-six bytes; any three of them rebuild it.
let data = b"twenty-six bytes of a text";
let facts = SetFacts::new(Field::Gf256, b"backup-1", 3, 26, 8, None)?;
let mut files = facts.files(data, &[0, 1, 2, 3, 4])?;

// One file is lost and another damaged; the rest is given in any order.
let lost = files.remove(1);
files[2][50] ^= 0x01;
let given: Vec<&[u8]> = files.iter().rev().map(|file| file.as_slice()).collect();
let pile = Pile::new(&given)?;
let set = &pile.sets()[0];
assert_eq!(*set.rebuild(None)?.data, *data);

// The lost file is made again from the others, byte for byte.
assert_eq!(set.repair(1, None)?, lost);
```

When files read without a fault and still are not one set, `FileSet::vote` says which of them differ from the rest, block by block, wherever enough files are at hand to tell: two spare files outvote one wrong file, four outvote two. Of two files of the same shard that differ, it names the one the other shards do not confirm. `FileSet::without` is the set without the files a program decides to leave out.

```rust
use reed_solomon_core::{Field, Pile, SetError, SetFacts};

// Five files of a set, and in the place of file 3 the file of another text.
let data = b"twenty-six bytes of a text";
let facts = SetFacts::new(Field::Gf256, b"backup-1", 3, 26, 8, None)?;
let mut files = facts.files(data, &[0, 1, 2, 3, 4])?;
files[3] = facts.files(b"another text, of 26 bytes!", &[3])?.remove(0);
let given: Vec<&[u8]> = files.iter().map(|file| file.as_slice()).collect();
let pile = Pile::new(&given)?;
let set = &pile.sets()[0];
assert!(matches!(set.rebuild(None), Err(SetError::Inconsistent { .. })));

// Four files agree and one does not: the vote names it, and the set without it rebuilds.
let vote = set.vote(None)?;
assert_eq!(vote.outvoted().len(), 1);
assert_eq!(vote.outvoted()[0].position, 3);
assert_eq!(*set.without(&[3])?.rebuild(None)?.data, *data);
```

These calls take a limit on their work, counted in steps by a stated rule and checked before anything is computed; `None` sets no limit.

A file larger than memory is given as a mapping of it, which the crate reads a block at a time. `SetFacts::files_into`, `FileSet::rebuild_into` and `FileSet::repairs_into` write the files of a set, its data and lost files into outputs the program gives, such as mappings of new files of `SetFacts::file_len` and `SetFacts::data_len` bytes, holding one block at a time. An output counts only when the call succeeds: a refusal before the work leaves it as it was, one during the work or by the checks after it leaves zeros in it. The files must not change from `Pile::new` until the last call on their set returns: a block's checksum is checked once, when the pile reads it. A large set should state the SHA-256 of its data, which a rebuild compares.

```rust
use reed_solomon_core::{Field, Pile, SetFacts};

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
set.repairs_into(&[0, 1], &mut [first.as_mut_slice(), second.as_mut_slice()], None)?;
assert_eq!((first.as_slice(), second.as_slice()), (&*files[0], &*files[1]));
```

[How it works](docs/TECHNICAL.md#the-shard-file) has every byte of the format, every rule of reading and the rules for sets.

## What it does not do

reed-solomon-core holds no secret, and its one cryptographic function, SHA-256 from the `sha2` crate, checks data, not people. Its checks say whether shards belong together and whether the data has the digest its files state, never who made them: someone who can change every shard can make a consistent set of other data and state its digest. Beyond the correction limit the answer is either "unresolved" or a different valid set. A program that needs to know it got the original data back compares a digest or an authentication tag it trusts from elsewhere. The checksums of a shard file find accidental damage and are no signature.

Data can be a secret. The crate wipes every buffer it filled with data when it lets go of it, keeps the bytes of data out of every `Debug` text, and multiplies symbols without tables or branches on their values. Every result that holds data comes in a `Buffer`, and so does every working buffer of bytes that follow from data; the outputs of `FileSet::rebuild_into` and `FileSet::repairs_into` are the program's own memory, for the program to protect. With the `protect` feature, which is on by default, a `Buffer` lies in memory pages of its own from the `secret-protection-core` crate, which keeps them out of swap, core dumps and child processes as far as each system allows. The crate passes that crate on as `reed_solomon_core::protect`, so that a program can also close its whole process against a secret leaving it (`protect::Isolation`), which only the program can decide. [How it works](docs/TECHNICAL.md#secrets-in-memory) says what is covered and what is not.

## Limits

- Shards of one set have one length, a whole number of symbols.
- Correction asks for at most `Code::correction_memory` bytes of working memory, under 8 MiB for any set, whatever the length of the shards, and `CyclicCode::correct` for at most 4 MiB of syndromes and a few elements per shard; with `protect` the system maps whole pages for the largest part. A plan of either code holds every change it lists.
- A file is read through the bytes a program gives: one larger than memory is given as a mapping of it, which must not change from `Pile::new` until the last call on its set returns, and the calls that end in `_into` write into mappings as well. Where a system has no mappings, as in a browser, the files and the data have to fit in memory. `Field::Gf2048` has no shard file, because its symbols are not the bytes of a file.
- A rebuild compares the SHA-256 only when the files state one; files written without it rebuild with nothing compared, and with exactly `k` files no shard is compared either. Making one lost file again compares no digest.
- The vote names the files that differ from what most shards share, within a stated bound: a wrong file takes two spare files in every block it is wrong in. It does not say that the files left hold the data once written; a digest the program trusts does.
- The [roadmap](docs/ROADMAP.md) lists the planned work.

## Use from Rust

```rust
use reed_solomon_core::{Code, Correction, Field, Redundancy};

// Three data shards become a set of five; any three rebuild the data.
let code = Code::new(Field::Gf256, 3)?;
let data: [&[u8]; 3] = [b"abcd", b"efgh", b"ijkl"];
let mut set = code.encode(&data, &[0, 1, 2, 3, 4])?;

// Shards 0 and 2 are lost: three others give them back, and a sixth shard as well.
let left = [(4, &set[4][..]), (1, &set[1][..]), (3, &set[3][..])];
let lost = code.rebuild(&left, &[0, 2])?;
assert_eq!((&lost[0][..], &lost[1][..]), (&b"abcd"[..], &b"ijkl"[..]));
let sixth = code.rebuild(&left, &[5])?;
assert_eq!(code.redundancy(&[left[0], left[1], left[2], (5, &sixth[0][..])])?, Redundancy::Agrees);

// One byte goes wrong somewhere: the two spare shards find it and put it right.
set[2][1] ^= 0x40;
let mut given: Vec<(u16, &mut [u8])> = (0..).zip(set.iter_mut().map(|shard| shard.as_mut_slice())).collect();
assert_eq!(code.correct(&mut given)?, Correction::Corrected(vec![0, 0, 1, 0, 0]));
assert_eq!(*set[2], *b"ijkl");
```

The features: `protect`, on by default, keeps the crate's buffers in protected pages and brings the standard library; `alloc` alone keeps them in the allocator's memory, wiped all the same, for a build without an operating system; without either the crate allocates nothing, and `Code::sources`, `Sources::build` and `Sources::agrees` work on buffers the caller provides.

## Self-test

`self_test()` checks the build against answers from outside the crate: published parity bytes, shards computed by hand in each field, the bytes of an independent implementation, two words whose errors must be found, a card with marked gaps whose plan must put it right, a shard file whose checksums were computed apart, a vote among five files whose answer was computed apart, the published SHA-256 of "abc", and files that state a digest their data does not have. A program runs it at start and goes on only when `passed()` is true.

## License

MIT or Apache-2.0, at your option. [Third-party notices](THIRD_PARTY_NOTICES.md) list the crates it uses and the one used by the tests.
