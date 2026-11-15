# reed-solomon-core

> [!WARNING]
> **Draft, not ready for production.** reed-solomon-core is an early draft: the interface will still change, the shard file format is a draft, and nobody outside the project has reviewed it. Do not rely on it for data you cannot afford to lose.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

reed-solomon-core is a Reed–Solomon library for sets of shards. Data cut into `k` shards becomes a set of up to 2^m shards in a field GF(2^m) the program chooses, from 4 to 65,536, and any `k` of them rebuild it. A lost shard can be made again, a set can be given more shards later, and wrong bytes at unknown places can be found and corrected, together with gaps a reader marks. A shard can be kept in a shard file, which names its set and checks itself block by block. The crate has one dependency, `sha2`, no `unsafe` code, and works without an allocator.

The [documentation index](docs/README.md) lists the guides; [How it works](docs/TECHNICAL.md) has the mathematics and the guarantees.

## Contents

- [What it is for](#what-it-is-for)
- [What it does](#what-it-does)
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

What the others do better. The fast erasure coders work in O(n log n) and with SIMD: for thousands of shards and gigabytes of data they are much faster than this crate, whose work grows with `k` times the number of shards. PAR2 is a format many programs read. Here, files and data are still held in memory as a whole; the [roadmap](docs/ROADMAP.md) lists reading them piece by piece, a second construction compatible with printed repair words, and fields of a size the program chooses.

## What it does

A set is written from `k` data shards of equal length. The first `k` shards of the set are the data itself; every further shard is computed from them, by coefficients that depend only on its number and on `k`, not on how many shards the set has. So the shards already handed out never change when more are made.

| Task                                          | Call                                                         | What it needs                      |
| --------------------------------------------- | ------------------------------------------------------------ | ---------------------------------- |
| Write a set                                   | `Code::encode`                                               | the `k` data shards                |
| Get the data back, or make a lost shard again | `Code::rebuild`                                              | any `k` shards                     |
| Give a set more shards later                  | `Code::rebuild` with new numbers                             | any `k` shards                     |
| Check that shards belong to one set           | `Code::redundancy`                                           | more than `k` shards               |
| Find and correct wrong bytes                  | `Code::correct`                                              | more than `k` shards               |
| Plan corrections with marked gaps, then apply | `Code::plan`, then `Code::apply`                             | more than `k` shards               |
| Work on a large file piece by piece           | `Code::sources`, then `Sources::build` and `Sources::agrees` | the caller's own buffers           |
| Keep a shard in a file that describes itself  | `Header::file`                                               | one shard and its set              |
| Read such a file, damaged or cut short        | `ShardFile::read`                                            | what is left of the file           |
| Write all the files of a set                  | `SetFacts::files`                                            | the data                           |
| Sort files into sets and rebuild the data     | `Pile::new`, then `FileSet::rebuild`                         | any files, in any order            |
| Make a lost file again                        | `FileSet::repair`                                            | files that hold `k` of every block |
| Make several lost files in one pass           | `FileSet::repairs`                                           | files that hold `k` of every block |
| Tell which files differ from the rest         | `FileSet::vote`, then `FileSet::without`                     | spare files beyond `k`             |

Correction finds wrong symbols without being told where they are. With `m` shards at hand it puts right up to `(m − k) / 2` wrong symbols at every position of the shards; shards that are missing are simply not given. In short, a set of `n` shards survives `e` wrong and `s` missing shards when `2e + s ≤ n − k`. Either every position is corrected or nothing is changed.

A reader often knows that one symbol is unreadable, such as a smudged word on a card, while the rest of its shard is fine. `Code::plan` takes such marks: a marked symbol costs one spare shard where a wrong one costs two, and the plan lists every change, with the place, what was read there and what it becomes, before anything is touched. `Code::apply` makes the changes after checking that the shards still hold what the plan was made for.

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
assert_eq!(*set.rebuild(None)?.data, data);

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
assert_eq!(*set.without(&[3])?.rebuild(None)?.data, data);
```

These calls take a limit on their work, counted in steps by a stated rule and checked before anything is computed; `None` sets no limit.

[How it works](docs/TECHNICAL.md#the-shard-file) has every byte of the format, every rule of reading and the rules for sets.

## What it does not do

reed-solomon-core holds no secret, and its one cryptographic function, SHA-256 from the `sha2` crate, checks data, not people. Its checks say whether shards belong together and whether the data has the digest its files state, never who made them: someone who can change every shard can make a consistent set of other data and state its digest. Beyond the correction limit the answer is either "unresolved" or a different valid set. A program that needs to know it got the original data back compares a digest or an authentication tag it trusts from elsewhere. The checksums of a shard file find accidental damage and are no signature.

Data can be a secret. The crate wipes every buffer it filled with data when it lets go of it, keeps the bytes of data out of every `Debug` text, and multiplies symbols without tables or branches on their values. Every result that holds data comes in a `Buffer`, and so does every working buffer of bytes that follow from data. With the `protect` feature, which is on by default, a `Buffer` lies in memory pages of its own from the `secret-protection-core` crate: out of core dumps and child processes, and out of swap where the system's lock limit allows. The crate passes that crate on as `reed_solomon_core::protect`, so that a program can also close its whole process against a secret leaving it (`protect::Isolation`), which only the program can decide. [How it works](docs/TECHNICAL.md#secrets-in-memory) says what is covered and what is not.

## Limits

- Shards of one set have one length, a whole number of symbols.
- Correction takes at most `Code::correction_memory` bytes of working memory, under 8 MiB for any set, whatever the length of the shards.
- Files are read and sets rebuilt in memory: the files and the data have to fit there. `Field::Gf2048` has no shard file, because its symbols are not the bytes of a file.
- A rebuild compares the SHA-256 only when the files state one; files written without it rebuild with nothing compared, and with exactly `k` files no shard is compared either. Making one lost file again compares no digest.
- The vote names the files that differ from what most shards share, within a stated bound: a wrong file takes two spare files in every block it is wrong in. It does not say that the files left hold the data once written; a digest the program trusts does.
- The [roadmap](docs/ROADMAP.md) lists reading large files piece by piece and other planned work.

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

The features: `protect`, on by default, keeps the crate's buffers in protected pages; `alloc` alone keeps them in the allocator's memory, wiped all the same, for a build without an operating system; without either the crate allocates nothing, and `Code::sources`, `Sources::build` and `Sources::agrees` work on buffers the caller provides.

## Self-test

`self_test()` checks the build against answers from outside the crate: published parity bytes, shards computed by hand in each field, the bytes of an independent implementation, two words whose errors must be found, a card with marked gaps whose plan must put it right, a shard file whose checksums were computed apart, a vote among five files whose answer was computed apart, the published SHA-256 of "abc", and files that state a digest their data does not have. A program runs it at start and goes on only when `passed()` is true.

## License

MIT or Apache-2.0, at your option. [Third-party notices](THIRD_PARTY_NOTICES.md) list the crates it uses and the one used by the tests.
