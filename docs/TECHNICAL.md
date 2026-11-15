# How reed-solomon-core works

This document describes how the crate is built and why, in enough detail to check every result by hand or to write another implementation that gives the same bytes. The [README](../README.md) is the short way in; the [roadmap](ROADMAP.md) lists what is planned.

## Contents

- [Overview](#overview)
- [Principles](#principles)
- [Terms](#terms)
- [Where things are in the code](#where-things-are-in-the-code)
- [The code](#the-code)
- [Fields](#fields)
- [Making shards](#making-shards)
- [Checking a set](#checking-a-set)
- [Correcting wrong symbols](#correcting-wrong-symbols)
- [Gaps at known places and a plan of changes](#gaps-at-known-places-and-a-plan-of-changes)
- [What is guaranteed](#what-is-guaranteed)
- [Memory and work](#memory-and-work)
- [The data path](#the-data-path)
- [Secrets in memory](#secrets-in-memory)
- [The shard file](#the-shard-file)
- [Self-test and tests](#self-test-and-tests)
- [References](#references)

## Overview

reed-solomon-core keeps data recoverable when parts of it are lost or damaged. The data is cut into `k` shards, and the code makes further shards from them: any `k` shards of the set give the data back, and shards beyond `k` find and put right wrong symbols. The crate is a library. What a shard is, a file, a photo or a printed word, is decided by the program that uses it.

The crate is built in layers, each using only the ones above it:

```
fields          GF(2^8), GF(2^11), GF(2^16): adding, multiplying and inverting symbols
  code          any shard of a set from any k shards, by interpolation: write, rebuild, extend, check
    correction  wrong symbols at unknown places found and put right; gaps at known places;
                a plan of every change before anything changes
      file      one shard with its set's facts, block checksums, a second header and a trailer
        sets    files sorted into sets; data rebuilt block by block from whichever files hold
                each block; lost files made again; the files that differ named; SHA-256 compared
```

Without the `alloc` feature the crate allocates nothing and offers the fields, the code on the caller's buffers, the reading of shard files and the self-test. With it come the owned helpers, the correction, the writing of files and everything about sets.

## Principles

- **One construction.** Shard `i` is the value at point `i` of one polynomial. Writing a set, rebuilding it, making a lost shard again and giving a set more shards are the same interpolation, and "any `k` shards are enough" follows from one fact: `k` values fix a polynomial of degree below `k`.
- **The bytes of established implementations.** Where another implementation of the same code exists, the crate writes its bytes, and the tests compare them shard by shard.
- **Refuse first, then work.** Every request is checked before the first byte of an output is written. A correction changes every column or none, a rebuild returns all the data or none, and a list the crate keeps is asked for before it is filled.
- **Never guess.** A file whose two headers differ is refused, not resolved by choosing one. Two sets with one identifier are both listed. Which construction made a list of symbols is never guessed from the symbols.
- **Bounds are stated and checked before the work.** The working memory of a correction has a published bound, sizes are worked out in 128-bit numbers before they are narrowed, and the calls on sets take a limit in steps that is compared with their cost before anything is computed.
- **A check says what it can.** Shards agree or do not; data has the digest its files state or does not. No check says who made a shard: that needs an authentication the program trusts from elsewhere.
- **Known answers from outside.** Every value the self-test and the tests expect was computed apart from the crate, and each one names its source.
- **No secret leaks through the data path.** The crate has no `unsafe` code, and no byte of a shard chooses a branch or a table entry where shards are scaled or checksummed.

## Terms

| Term                  | Meaning                                                                                      |
| --------------------- | -------------------------------------------------------------------------------------------- |
| symbol                | one element of the field: one byte, or two bytes                                             |
| shard                 | a row of symbols; every shard of a set has the same length                                   |
| set, `n`              | the shards made from one piece of data; `n` is how many there are                            |
| threshold, `k`        | how many shards give the data back; the shards 0 to `k − 1` are the data itself              |
| point                 | the field element of a shard's number: shard `i` sits at the point `i`                       |
| column, offset        | the symbols at one place of every shard; each column is one code word                        |
| spare shards, `m − k` | of `m` shards at hand, those beyond `k`: they check and correct                              |
| syndromes             | `m − k` sums of a column that are all zero exactly when its symbols are one code word        |
| wrong symbol, error   | a symbol that differs from the code word, at a place nobody knows; it costs two spare shards |
| gap, mark             | a symbol known to be unreadable, at a known place; it costs one spare shard                  |
| block                 | the unit of damage in a shard file: a run of the shard's bytes under one checksum            |
| set facts             | what every file of a set states alike: field, `k`, identifier, lengths, digest               |
| holder                | a shard that has a block intact in one of its files                                          |
| dispute               | files of one shard that differ on a block although both checksums hold                       |
| step                  | the unit in which the work of a call on a set is counted and limited                         |

## Where things are in the code

| Module                             | What it holds                                                                                        |
| ---------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `src/field.rs` and `src/field/`    | the three fields; `lanes.rs` is the scaling kernel through which every byte of a shard passes        |
| `src/code.rs`, `src/code/owned.rs` | `Code`, `Sources`: interpolation on buffers, and the owned helpers `encode`, `rebuild`, `redundancy` |
| `src/correct.rs`                   | the decoder: syndromes, Berlekamp–Massey, error values; `Code::correct` and the search the vote uses |
| `src/correct/plan.rs`              | marked gaps and the plan of changes: `Code::plan`, `Code::apply`                                     |
| `src/file/header.rs`, `crc32c.rs`  | the header of a shard file, byte for byte, and its checksum                                          |
| `src/file/facts.rs`                | `SetFacts`: what a set states, the sizes that follow, the SHA-256 of the data                        |
| `src/file/reading.rs`              | `ShardFile::read`: a file that may be damaged, cut short or followed by other bytes                  |
| `src/file/writing.rs`              | `SetFacts::files`: the files of a set from its data                                                  |
| `src/file/pile.rs`                 | `Pile::new`: any files sorted into sets                                                              |
| `src/file/set.rs`, `set/shards.rs` | `FileSet`: who holds each block, rebuild, repair, coverage, stretches, the set without some files    |
| `src/file/set/vote.rs`             | `FileSet::vote`: which files differ from what the others share                                       |
| `src/self_test.rs`                 | the known answers a program runs before it relies on the crate                                       |
| `tests/suite/`                     | the tests of the public interface, each file one subject                                             |

## The code

The data is cut into `k` shards of equal length. A shard is a row of symbols; a symbol is one or two bytes, depending on the field. Take the symbols at one offset of the `k` data shards: they are the values, at the points 0 to `k − 1`, of exactly one polynomial of degree below `k` over the field. Shard `i` of the set holds that polynomial's value at the point `i`, where the point of a shard is its number read as a field element.

Three things follow. The shards 0 to `k − 1` are the data itself. The coefficients that make a shard from the data depend only on its number and on `k`, not on how many shards there are, so shards made later never change the ones made before. And a set can have as many shards as the field has elements.

A polynomial of degree below `k` is fixed by any `k` of its values. That is why any `k` shards rebuild all the others, and it is the whole proof that the code loses nothing (it is "maximum distance separable"): no property of a matrix has to be shown.

Every operation is one interpolation. From `k` shards at the points `x_1 … x_k`, the shard at the point `t` is

```
shard_t = Σ L_i(t) · shard_i,    L_i(t) = Π_{l≠i} (t − x_l) / (x_i − x_l)
```

with the field's arithmetic, in which subtraction is exclusive or. Writing a set takes the data shards as sources; making a lost shard again or a new one takes any `k` shards at hand.

The Reed–Solomon libraries of Backblaze and Klaus Post, and the `reed-solomon-erasure` crate, describe the same code as a Vandermonde matrix multiplied by the inverse of its top `k` rows. A row of that product is the list of values `L_i(t)`, so the bytes are the same.

## Fields

| Field            | Elements                | Polynomial                                                                                   | A symbol in bytes                                                                     | Most shards |
| ---------------- | ----------------------- | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ----------- |
| `Field::Gf256`   | 256                     | x^8 + x^4 + x^3 + x^2 + 1 (0x11D)                                                            | one byte                                                                              | 256         |
| `Field::Gf65536` | 65,536                  | the field above extended by x^2 + 2x + 128                                                   | `[high, low]` for `high·x + low`                                                      | 65,536      |
| `Field::Gf2048`  | 2,048                   | x^11 + x^2 + 1 (0x805)                                                                       | two bytes, the high one first, holding a number below 2,048                           | 2,048       |
| `Field::Binary`  | 2^m, for m from 2 to 16 | a primitive polynomial of degree m: the standard one of the degree, or one the program gives | one byte holding a number below 2^m up to m = 8; two bytes, the high one first, above | 2^m         |

`Field::Gf256` and `Field::Gf65536` take any bytes. `Field::Gf2048` exists so that one symbol can be one word of a list of 2,048 words: the shards a code adds are then words of the list too. Two bytes that hold 2,048 or more are not a symbol of this field, and every call that is given one returns `CodeError::Symbol` before it writes anything. The element 2 generates all 2,047 nonzero elements of this field, so the polynomial is primitive.

**Choosing a field.** `Field::binary(m, polynomial)` makes GF(2^m) for a degree `m` from 2 to 16, by a primitive polynomial written with its x^m term (x^4 + x + 1 is 0x13), or by the standard one of the degree for `None`: 0x7, 0xB, 0x13, 0x25, 0x43, 0x89, 0x11D, 0x211, 0x409, 0x805, 0x1053, 0x201B, 0x4443, 0x8003 and 0x1100B for the degrees 2 to 16. A polynomial is taken only when its highest term is x^m, its constant term 1, and x has the order 2^m − 1 modulo it, so that the powers of x are every nonzero element: at most 65,535 steps, done once. The degree 8 with 0x11D is `Field::Gf256` and the degree 11 with 0x805 is `Field::Gf2048`, so one field has one value. `Field::Gf65536` stays a field of its own: GF(2^16) as an extension of GF(2^8), whose bytes are those of `reed-solomon-erasure`; the degree 16 of `Field::binary` is a field of polynomials and writes other bytes.

The size of a field limits only how many shards a set can have: 2^m. A larger field makes every symbol longer and every step slower and gains nothing else, so a program takes the smallest field whose symbols hold what it codes and whose elements number at least its shards. Fields above 2^16 are not offered: 65,536 shards are more than any set needs, a symbol of more than two bytes would be slower for every set, and showing a polynomial of a large degree primitive needs the factors of 2^m − 1. The degree 1 makes two points and only `k` = 1. These fields have no shard file: the file format names only `Field::Gf256` and `Field::Gf65536`.

**The arithmetic.** In all fields adding is exclusive or, so subtracting is the same as adding.

- `Field::Gf256`: a byte is a polynomial over GF(2), bit `i` the coefficient of `x^i`, and a product is reduced by 0x11D. The element 2 generates the nonzero elements. A product is eight steps of add, shift and reduce, each chosen by a mask made from one bit of an operand, and an inverse is `a^254` by the same seven squarings and multiplications for every `a`: no table is indexed by an element and no branch depends on one (see [The data path](#the-data-path)).
- `Field::Gf65536`: the element `[high, low]` is `high·x + low` with `high` and `low` in the field above, and `x^2 = 2x + 128`. So `(a_h·x + a_l)(b_h·x + b_l)` has the high byte `a_h·b_l + a_l·b_h + 2·a_h·b_h` and the low byte `a_l·b_l + 128·a_h·b_h`. For example `[01,00]·[01,00] = x^2 = [02,80]`. The inverse is the conjugate `high·x + (2·high + low)` divided by the norm `low^2 + 2·high·low + 128·high^2`, which lies in GF(2^8).
- `Field::Gf2048`: bit `i` of the number is the coefficient of `x^i`, and a product is reduced by 0x805. There are no tables: a product is eleven steps of add, shift and reduce, each chosen by a mask, and an inverse the same chain of squarings and multiplications for every element. The sets this field serves are a few dozen symbols.
- `Field::Binary`: bit `i` of the number is the coefficient of `x^i`, and a product is `m` steps of add, shift and reduce by the polynomial, each chosen by a mask; the inverse is `a^(2^m − 2)` by the same `m − 1` squarings and multiplications for every `a`. Whole shards are scaled by the kernel of the other fields, with lanes of one or two bytes: every lane is doubled at once by moving its low `m − 1` bits up and bringing its bit `m − 1` back as the polynomial without its x^m term.

**Points.** The point of shard `i` is the element whose bytes are the number `i`: in `Field::Gf256` the byte `i` for `i` up to 255, in `Field::Gf2048` the number `i` below 2,048, in `Field::Gf65536` the element `[i / 256, i mod 256]`, and in a field of degree `m` the number `i` below 2^m. A number the field has no element for is refused with `CodeError::Index`.

## Making shards

`Code::sources` takes the numbers of `k` different shards and computes one weight for each, `1 / Π_{l≠i} (x_i − x_l)`. That is `k²` field multiplications, done once however many shards are made afterwards. `Sources::build` then writes any shard of the set for one stretch of bytes, from the same stretch of each source, into a buffer of the caller; a large file is processed stretch by stretch and never has to be in memory at once. A shard that is itself among the sources is copied.

With the `alloc` feature, `Code::encode` writes shards from the data and `Code::rebuild` from any `k` or more shards given as `(number, bytes)`. Asking `rebuild` for numbers a set never had is how a set grows.

Every refusal comes before the first byte of an output is written, and a shard given for correction is left as it was. Refused are wrong counts, numbers the field does not have, a number given twice, shards of different lengths or of a length that is not a whole number of symbols, and, in the field of words, a symbol that is not a word. When a request is wrong in several ways, the counts, the numbers and the lengths are reported before a symbol. The working memory a caller lends to `Code::sources` is not an output: after a refusal it may hold anything.

## Checking a set

Any `k` shards form a valid set, so `k` shards cannot be checked against each other. With more, `Sources::agrees` and `Code::redundancy` compare each further shard with what the first `k` make. The answer of `redundancy` is `None` for exactly `k` shards, `Agrees`, or `Differs`.

## Correcting wrong symbols

`Code::correct` finds wrong symbols without being told which shards are wrong.

For the `m` shards at hand, with points `x_i` and weights `u_i = 1 / Π_{l≠i} (x_i − x_l)` taken over those `m`, the symbols at one offset give `m − k` syndromes:

```
S_j = Σ u_i · x_i^j · symbol_i,    j = 0 … m − k − 1
```

All are zero exactly when the symbols lie on one polynomial of degree below `k`. Otherwise the data cancels out and the syndromes are the power sums of the errors alone, `S_j = Σ Y_e · X_e^j`, where `X_e` is the point of a wrong symbol and `Y_e = u_e · error_e`.

The Berlekamp–Massey algorithm finds the shortest recurrence the syndromes obey, of length `L`. Its characteristic polynomial has the points of the wrong symbols as its roots. The point 0 of shard 0 is one of the possible roots: an error there shows as a polynomial whose degree is one below `L`. The decoder evaluates the polynomial at every point at hand and goes on only if it finds exactly `L` different roots and `2L ≤ m − k`.

The error values come from the product `Q(z) = Π (z − X_e)`: dividing `Q` by `(z − X_e)` gives a polynomial that is zero at every other wrong point, so one synthetic division per error yields `Y_e`, with memory in proportion to the number of errors and no matrix. The decoder then recomputes every syndrome from its answer, requires every error to be nonzero, and divides by `u_e`.

The shards are read twice. The first pass finds every wrong symbol and changes nothing; only if every offset was resolved does the second pass correct them. So either everything is put right or nothing is touched.

A line of symbols read from paper is the same problem with shards one symbol long: the unreadable symbols are left out, the misread ones are found.

## Gaps at known places and a plan of changes

`Code::correct` knows of a gap only as a shard that is not given at all. A file or a card often holds a shard that is there with one unreadable symbol, and leaving the whole shard out would waste the rest of it. `Code::plan` takes the shards and a list of marks, each naming a shard by its place among the shards given and a symbol by its place in the shard, counted in symbols. A marked symbol may hold any element of the field; its value is not used. A mark given twice counts once.

A gap at a known place costs one spare shard of its column, where a wrong symbol at an unknown place costs two. With `s` marked symbols at the points `x_1 … x_s` of a column, take `G(X) = Π (X − x_j)` with coefficients `g_0 … g_s`, and from the syndromes `S_j` of the column form

```
T_j = Σ_{i=0..s} g_i · S_{j+i},    j = 0 … m − k − s − 1
```

Each `S_j` is a sum of terms `Y·X^j` over the wrong and the marked symbols, so `T_j` is the sum of `Y·G(X)·X^j`. `G` is zero at every marked point and at no other, so the marked symbols drop out and the `T_j` are power sums of the wrong symbols alone. The point 0 needs no care of its own: if it is marked, `G` has the factor `X`. Berlekamp–Massey finds the recurrence of the `T_j` as it finds that of the syndromes, and the wrong symbols are its roots among the points that are not marked. A column goes on only with `2e ≤ m − k − s` for its `e` wrong symbols, that is `2e + s ≤ m − k`. Then the errors of all `e + s` symbols follow from the first `e + s` syndromes by the synthetic division of `Code::correct`, and every syndrome is checked against them. A wrong symbol must have an error that is not zero; a marked symbol may turn out to have held the right value. A column with more marks than spare shards is unresolved.

Nothing is changed. The answer is `Plan::Changes`, a list in which each change names the shard and the symbol, what the symbol holds, what it becomes, and whether it was marked (`ChangeKind::Filled`) or found (`ChangeKind::Replaced`), in the order of the symbols and within one of the shards. Every marked symbol is in the list, also when it keeps its value, so that a program can show a person each place with what was read there and what it becomes. As for `Code::correct`, the plan covers every column or there is none: `Plan::Unresolved` counts the columns that could not be put right. `Plan::Agrees` means no column disagrees and nothing was marked, and `Plan::Unchecked` that exactly `k` shards and no mark were given.

`Code::apply` makes the changes. It first checks every one: the place is in the shards, the symbol there still holds what the change says it held, and the new value is an element of the field. If one does not fit, it returns `CodeError::Changes` and changes nothing. So a plan made for one reading of a card cannot be applied to another.

Example: a card of 12 words of a list of 2,048 and 4 words that the code adds, in `Field::Gf2048`, each word one shard of one symbol. Words 3 and 8 are unreadable and marked, and word 11 was copied wrong. That is `2 · 1 + 2 = 4` spare words, exactly the 4 there are: the plan fills words 3 and 8 and replaces word 11.

Marks are checked after the counts, the numbers and the lengths of the shards and before their symbols: a mark outside the shards is `CodeError::Mark`. The working memory is that of `Code::correct` with room for `m − k` symbols of a column instead of half as many, a sorted copy of the marks and the list of changes, which holds at most the marks and `(m − k) / 2` symbols for each column.

## What is guaranteed

- Any `k` shards of a set make every shard of it.
- With `m` shards at hand, up to `(m − k) / 2` wrong symbols at every offset are found and corrected, wherever they are. A missing shard costs half as much as a wrong one: a set of `n` shards survives `e` wrong and `s` missing shards when `2e + s ≤ n − k`. The count is taken at each offset by itself, and which shards are missing is known: they are the ones not given.
- With `s` symbols of an offset marked unreadable, `Code::plan` puts the offset right when its `e` wrong symbols meet `2e + s ≤ m − k`, and every marked symbol is among the changes it lists. Beyond that the offset is unresolved, or the plan leads to another valid set that fills every marked symbol and replaces at most `(m − k − s) / 2` others.
- An offset with between one and `m − k` wrong symbols is never reported clean.
- Beyond `(m − k) / 2` wrong symbols at an offset there are three possible answers. `Unresolved`: no valid set is near enough, and nothing is changed. `Corrected`: another valid set differs from the shards given in no more than `(m − k) / 2` symbols at that offset, and the shards are changed into it. `Agrees`: with more than `m − k` wrong symbols the shards given may already be another valid set, and nothing is changed. The code cannot tell such a set from the original. A program that must know checks a digest or an authentication tag of the data that it trusts independently.
- With exactly `k` shards nothing can be checked: any `k` shards are a valid set, and the answer is `Unchecked`.
- Nothing here says who made a shard. Someone who can replace every shard can make a consistent set of other data.

## Memory and work

Without the `alloc` feature nothing is allocated: the caller provides `k` elements for the weights and the output buffer.

`Code::correct` holds the syndromes of one stretch of the shards, at most 4 MiB, and a few elements per shard and per spare shard. `Code::correction_memory` returns the bound for a number of shards; it is below 8 MiB for the largest set of any field, whatever the length of the shards. All of it is allocated once, before the first pass.

Its work is `m²` field multiplications for the weights, then `m · (m − k)` scalings of every stretch, and for each wrong offset about `(m − k)² + m · e` multiplications, with `e` wrong symbols.

## The data path

`out ^= factor · source` is the one operation applied to the bytes of shards. It works on eight bytes at a time with shifts and masks. The factor follows from shard numbers and is public; the bytes of the shards choose no branch and no table entry. The CRC-32C of a block is computed one bit at a time without a table, and `Sources::agrees` compares every byte of a stretch, whichever differs.

Single elements are multiplied and inverted without tables and without a branch on their value in all three fields, so the values the decoder works with, which follow from the errors and, for a marked gap, are the symbol itself, choose no table entry either. What the decoder does branch on is the shape of the errors: whether a column disagrees, the length of its recurrence and where its roots are, whether a discrepancy is zero. So the time a correction takes can show how many symbols were wrong and where, not what they were. The inverse of zero is told apart, as an answer of `None`.

## Secrets in memory

The data a program gives the crate can be a secret, such as the words of a recovery phrase on a card. The crate keeps no copy of it longer than a call and leaves none behind in memory it gives back:

- Every buffer the crate fills with data or with values that follow from it is wiped when it is let go, by the `zeroize` crate, whose writes the compiler cannot leave out: the copies a writer makes of the end of the data and of each shard it makes, the buffer a set is put together in, the block a vote makes to judge a dispute, the expected bytes `Sources::agrees` compares, and the syndromes, recurrences and errors of the decoder. A list that has to grow, such as the changes of a plan, moves into a new one and the old one is wiped.
- Every result that holds data comes in a `Zeroizing` buffer, which wipes itself when it is dropped: the shards of `Code::encode` and `Code::rebuild`, the files of `SetFacts::files`, `Header::file`, `FileSet::repair` and `FileSet::repairs`, the data of `FileSet::rebuild`, and the changes of `Code::plan`. The crate passes on `Zeroizing` and `Zeroize`, so a program needs no dependency of its own for them.
- The state of SHA-256 is wiped when the hash is done (the `zeroize` feature of `sha2`).
- No `Debug` text shows the bytes of data: a file read, a block and rebuilt data give their lengths, a change gives its place and kind, and a `Zeroizing` buffer shows nothing of what it holds.

What stays with the program. Buffers the program lends the crate, the shards it gives and the outputs of the calls without an allocator, are the program's to wipe and to keep where it wants. Memory that the operating system writes to a swap file or to a crash dump, and other programs that read the memory of the process, are beyond what a library can wipe: the program keeps such data in locked pages and shuts out dumps and debuggers for its whole process, as the hosts of this crate do. The crate holds no `unsafe` code and makes no call to the operating system.

## The shard file

A shard file holds one shard together with what a program needs to find its set again and to tell which of its bytes survived. This is version 1 of the format; it stays a draft until the crate is published.

```
header | block 0, checksum | block 1, checksum | … | header | trailer
```

Every number is big-endian and has a fixed width.

### The header

| Offset | Bytes   | Field                                                                                                                                             |
| ------ | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0      | 4       | the mark `53 48 52 44` ("SHRD")                                                                                                                   |
| 4      | 1       | the version, 1                                                                                                                                    |
| 5      | 1       | the field: 1 for `Field::Gf256`, 2 for `Field::Gf65536`. 3 is kept for `Field::Gf2048` and never written: its symbols are not the bytes of a file |
| 6      | 1       | flags: bit 0 says that the digest of the data is there; every other bit is zero                                                                   |
| 7      | 1       | the length of the set's identifier, 1 to 32                                                                                                       |
| 8      | 1 to 32 | the identifier of the set: bytes the program chooses, different for every set                                                                     |
| …      | 4       | `k`, from 1 to the number of elements of the field                                                                                                |
| …      | 4       | the number of the shard, below the number of elements of the field                                                                                |
| …      | 8       | the length of the data in bytes                                                                                                                   |
| …      | 4       | the length of a block in bytes: above zero and a whole number of symbols                                                                          |
| …      | 32      | the SHA-256 of the data, when bit 0 of the flags is set                                                                                           |
| …      | 4       | the CRC-32C of every byte of the header before it                                                                                                 |

A header is 33 to 96 bytes long. No bit of it is free, so a header that passes the checks is written back to the same bytes. Everything but the number of the shard is the same in every file of a set (`SetFacts`): files that agree in it belong together.

### Blocks, the second header and the trailer

The length of a shard, `S`, is the data length divided by `k`, rounded up, then rounded up to a whole number of symbols. The data shards are the data cut in order into `k` pieces of `S` bytes, the last ones filled up with zero bytes.

A shard is written in blocks of the block length; the last block may be shorter. There are `B = ⌈S / block length⌉` blocks, none for empty data. Block `b`, counted from zero, is followed by the CRC-32C of three things in a row: the four bytes of the header's own checksum, `b` in four bytes, and the bytes of the block. A block is so tied to its file and to its place in it.

After the last block comes the header a second time, byte for byte, and then the trailer of twelve bytes: the mark `53 48 52 54` ("SHRT"), the length of the header in four bytes, and the CRC-32C of those eight bytes. A file is `2·H + S + 4·B + 12` bytes long for a header of `H` bytes, the same for every file of a set.

Two limits belong to the format. `B` is at most 2^32, because the number of a block is written in four bytes; the last block then has the number FFFFFFFF. And a file is at most 2^64 − 1 bytes long. Facts that give more are refused when a set is described (`FileError::Blocks`, `FileError::Size`), and a header that states them does not hold when a file is read. The sizes are worked out in 128-bit numbers, rounding up through quotient and remainder, and checked before they are narrowed.

The checksum is CRC-32C: the polynomial 0x1EDC6F41, input and output reflected, the initial value and the final exclusive-or FFFFFFFF, written with the high byte first. The checksum of the nine characters "123456789" is E3069283. It is computed one bit at a time, without a table, so the bytes of a shard choose no branch and no table entry here either.

### Reading

`ShardFile::read` takes the bytes of a file, or what is left of them. It looks for a header at the start, and for the copy at the end through the trailer in the last twelve bytes. What it finds in each place is one of five things:

| Found           | Meaning                                                                                                                                     |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| sound           | a header of version 1 whose checksum holds and whose every value, the sizes included, is one the format allows                              |
| unsupported     | bytes whose checksum holds by the layout of version 1 and that name a field or a flag this version does not read                            |
| another version | the mark and, in the byte after it, a version number other than 1. Nothing else is read: a later version may lay its header out differently |
| damaged         | the mark of a header or a trailer, and no header that holds                                                                                 |
| absent          | no mark: no header's mark at the start, no trailer's mark in the last twelve bytes                                                          |

A sound header at the end counts only where its own sizes put it: the trailer's checksum holds, the length it names is the header's own, and the file is exactly as long as the header says. A trailer or a header anywhere else proves nothing and is not searched for. The sizes of an unsupported header cannot be worked out, so its place at the end is never certain.

The answer follows from the two findings, in this order:

1. Two headers in places that are certain, and different: `FileError::Conflict`. Neither is chosen, whichever comes first. That is a sound header at each end, or an unsupported one at the start and a sound one at the end.
2. A sound header: the file is read by it.
3. An unsupported header: `FileError::Unsupported`.
4. A header that names another version: `FileError::Version`, the number at the start before the one at the end.
5. A mark and nothing that holds: `FileError::Damaged`. No mark at either end: `FileError::Unrecognized`.

| Start \ end     | sound                               | unsupported     | another version       | damaged     | absent       |
| --------------- | ----------------------------------- | --------------- | --------------------- | ----------- | ------------ |
| sound           | the same: read; different: conflict | read, see below | read                  | read        | read         |
| unsupported     | conflict                            | unsupported     | unsupported           | unsupported | unsupported  |
| another version | read                                | unsupported     | version, of the start | version     | version      |
| damaged         | read                                | unsupported     | version, of the end   | damaged     | damaged      |
| absent          | read                                | unsupported     | version, of the end   | damaged     | unrecognized |

So one wrong bit in a version byte or in a flag costs nothing while the other copy is sound: the sound copy is believed and the other is reported as damaged. `Unsupported` rests on a checksum; `Version` rests on nothing but the mark and one byte, and a file of version 1 whose version byte went wrong in the only copy left reads the same way.

Once a header is believed, the copy at the end and the trailer are looked for where its sizes put them, also when other bytes follow. What stands in the place of the copy is read by its own length: another header whose checksum holds there, sound or unsupported, longer or shorter than the first, is a `Conflict` as well. An unsupported header that the trailer of a longer file points at, after the end the sound header gives, is among the bytes that follow and is used for nothing. `ShardFile::frame` then says whether the header at the start, the copy at the end and the trailer are intact, damaged or missing, and whether the file has its exact length, is short or is long.

What this means for a damaged file:

- A file that lost its end is read by the header at its start. A block that is wholly there and whose checksum holds is used; the others are missing. Bytes are only ever taken from a block's own place, never from the place of the copy at the end.
- A file whose first header is damaged is read by the copy at the end, as long as the file still has its exact length.
- A file that lost its first bytes, or both its first header and its end, is not read: nothing is promised for it.
- Bytes after the end are counted and used for nothing.

`ShardFile::block` checks one block when it is asked for and `ShardFile::survey` checks them all. `Survey::whole` is true for exactly the files `Header::file` writes: every piece intact and the length exact.

Reading allocates nothing, and its work is bounded by the bytes given, whatever a header says: `read` looks at three headers at most, a block is checked in one pass over its bytes, and a survey ends at the first block the file does not hold, so a header that counts 2^32 blocks in a file of a hundred bytes costs nothing.

### What the checksums do not say

CRC-32C finds accidental damage: every run of wrong bits up to 32 bits long in what one checksum covers, and other damage unless the changed bytes happen to have the same checksum. Such collisions exist; for random damage they are about one in four billion. A block moved to another place or taken from another file fails the same way, with the same exception. The checksums are no signature: someone who changes a file on purpose can make every one of them fit.

The digest in the header is the SHA-256 of the data, computed by the `sha2` crate. `SetFacts::for_data` with `DataDigest::Sha256` computes it from the data, and `SetFacts::files` writes no file for data that does not have the digest the facts state, so a set written by the crate never states the digest of other data. `SetFacts::new` takes a digest as given, for facts known otherwise, such as those of a header that was read; `Header::file` writes the facts it is given. Files that read without a fault say nothing yet about the digest: it is compared when the data is put together. A match says that the data came back as the digest describes it, never who wrote the set: whoever changes the data in every file can state its new digest as well. Only a digest or an authentication tag the program trusts from elsewhere tells that.

### A set from its files

`SetFacts::files` writes the files of a set from its data. The data is cut in order into the `k` data shards, what is left of the last ones is filled up with zero bytes, and every other shard follows from them. A file for a further number can be written at any time and leaves the earlier files as they are.

`Pile::new` takes any number of files in any order. Each one is read as above; the ones that read are sorted into sets. Files are one set when they state the same facts: field, `k`, identifier, data length, block length and digest. Two sets with one identifier and other facts are both listed and both marked (`FileSet::id_shared`): by the rules that cannot happen, so one of them is not what it says, and nothing tells which. Neither is preferred; a program asks for the set it wants. Nothing depends on the order of the files: the sets come in an order of their facts and their files in the order of their numbers.

For each set, `FileSet::members` lists its files, each with its place among the files given, its number, the condition of its pieces and whether another file holds the same shard. These are separate facts: a file can be a copy and damaged at once.

**Who holds a block.** A shard holds a block when one of its files has the block with a checksum that holds, and no two of its files differ on it. Files of one shard fill each other in: each may have lost other blocks. They are compared block by block with the first file that has the block. Where two of them differ although both checksums hold, the shard is left out of that block and of no other, and the difference is listed (`FileSet::disputes`: the shard, how many blocks, the first one, the files that took part). Every file of the shard that has such a block with a checksum that holds took part, whichever of the others it agrees with, so the list is the same in any order of the files. `FileSet::disputed_blocks` gives every such block. Nothing says which file is right.

**How a block is made.** The `k` lowest shards that hold a block are its sources. Every further shard that holds it is compared with what the sources make. So a set rebuilds when every block has `k` holders, whichever files they are, even when no `k` files are whole. `FileSet::coverage` says how many blocks have fewer than `k` holders and which is the first. `FileSet::stretches` goes through all the blocks in stretches that one number of shards hold, so that a program can show which parts still rebuild, which are confirmed by spare shards and which are lost. Blocks that no file reaches come as one stretch, not one by one.

| Holders of a block | What happens                                                                                                                                                                                                                          |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| fewer than `k`     | the set does not rebuild: `SetError::Blocks`                                                                                                                                                                                          |
| exactly `k`        | the block is made, with nothing to compare it with; `Rebuilt::bare` counts such blocks                                                                                                                                                |
| more than `k`      | the block is made and every further holder compared. One that differs stops everything: `SetError::Inconsistent` names the block, the sources and the shards that differ. Which is wrong cannot be told from that: it may be a source |

`FileSet::rebuild` returns the data only when every block was made and every comparison held; there is no partial result. Then the bytes after the end of the data, up to `k·S`, must be zero (`SetError::Padding`). Fewer than `k` different shards never rebuild a set (`SetError::Shards`), and that holds for a set of no data too: one header says that the data is empty, and it is still one file, not a set.

`FileSet::repair` makes the shard file of one number from the files at hand: a lost file again, byte for byte, or a file the set never had. It makes that one shard only. Every further holder is still compared, but the data is not put together and the bytes after its end are not looked at.

What comes back is the data the files agree on. Files that were all replaced agree as well, and with exactly `k` holders no shard is compared at all. When the files state a SHA-256, `FileSet::rebuild` computes the digest of exactly `data_len` bytes, without the zero bytes after them, and gives the data back only when it is the digest stated: `Rebuilt::digest` is then `DigestCheck::Matched` with that digest. Data of another digest is not given back; the error `SetError::Digest` names both digests. Files that state no digest rebuild with `DigestCheck::Unchecked`: nothing was compared, and the program checks the data itself. The digest is computed outside the steps the limit counts; its work follows the length of the data, which the call holds anyway.

`FileSet::repair` puts no data together and compares no digest. The file it makes states the digest the set states, and a rebuild that uses it compares that digest.

**Several lost files at once.** `FileSet::repairs` makes the files of several numbers in one pass: every block is read and compared once, and its sources are prepared once, for all of them. Each file is the one `repair` makes for its number, and they come back in the order asked. A number asked for twice gives the same file twice; an empty list gives no file and costs nothing. `FileSet::repairs_work` counts the steps by the rule below. For `k` = 3, two blocks of four symbols, four files left of seven and the three others asked for, the pass costs 9 + 2 · 4 · 204 = 1,641 steps: per block one spare holder compared and three shards made. Three separate repairs would cost 3 · (9 + 2 · 2 · 204) = 2,475.

**Which file differs.** A rebuild that stops with `SetError::Inconsistent` has shown that the shards are not one set, not which of them is wrong. `FileSet::vote` goes through the blocks and names files wherever the shards at hand can tell. It changes nothing and rebuilds nothing.

The holders of a block vote on it. With `m` holders there are `r = m − k` spare ones. A word that all holders but at most `⌊r/2⌋` share in every column of the block is the only such word: two of them would agree on `k` holders. A shard whose files differ on the block does not vote; each of its files is compared with what the agreeing holders make of the block.

Count as a witness of a word every shard that has a file carrying it in the block, however many of its files do. With `w` holders that differ from the word of the others, and `u` shards in dispute of which no file carries that word, the word is established when

```
2·w + u ≤ r
```

Then it is sure to have more witnesses than any other word: another word agrees with fewer than `k` of the agreeing holders, and can gain at most every holder that differs and every shard in dispute. A holder that differs takes two spare shards, a shard in dispute that does not confirm takes one. The rule is on the safe side: a word that is ahead of all others without meeting it is not established.

| A block with                                                             | What the vote says                           |
| ------------------------------------------------------------------------ | -------------------------------------------- |
| fewer than `k` holders                                                   | nothing: there is no word                    |
| `k` holders and no dispute                                               | nothing: there is nothing to compare with    |
| `k` holders, and every shard in dispute has a file that agrees with them | the other files of those shards are outvoted |
| `k + 1` holders that differ                                              | undecided: one spare shard cannot say which  |
| `k + 2` holders of which one differs                                     | the files of that one are outvoted           |
| holders that differ beyond the rule                                      | undecided                                    |

Every file that holds the block with a checksum that holds and differs from an established word is outvoted there: every file of a holder that differs, and every differing file of a shard in dispute. `Vote::outvoted` lists the files, each with the number of such blocks and the first, and `Vote::outvoted_blocks` gives every block of a file, asked for by the file's place among the files given. The answer is read and not changed: a program that shows the rows in another order sorts its own copy of them. A block whose holders differ and whose word is not established is undecided (`Vote::undecided`, `Vote::undecided_blocks`): a rebuild fails there, and the vote cannot say because of which file.

`FileSet::without` is the set without the files a program names by their places among the files given. Leaving a file out is the program's decision, and the file goes whole: a file that was wrong in one block is then no silent source of another. No block is read again. The files left of each shard are compared anew, so a shard whose files differed holds its blocks again once the files left agree, and the coverage is counted anew: if a block needed the file that is gone, the coverage says so. The set as it was found stays as it was.

A program asks for a vote after `SetError::Inconsistent`, and whenever `FileSet::disputes` is not empty: a rebuild passes over a shard in dispute and can succeed while the vote still tells which of its files the other shards do not confirm. It then shows the files named, takes the set without those the user gives up and rebuilds, which compares the digest when the files state one.

What the vote does not say. It is right when the wrong files are within the rule in every block; beyond it a block is undecided or settled on other files. Files written together from wrong data agree with each other. No undecided block does not mean that every block was confirmed: with `k` holders or fewer there was nothing to compare, which the coverage shows. Being outvoted means differing from what most shards share, not being forged or damaged. That data is the data once written is said only by a digest or an authentication tag the program trusts from elsewhere; the digest the files state shows only that the data fits what those files say of it.

How the holders that differ are found. The syndromes of a block over all its holders are zero when the holders agree. Otherwise the list of holders found starts empty, in every block. A column whose syndromes are not all zero is first held against the list: the syndromes of wrong symbols of those holders alone obey the recurrence of the polynomial `Π (1 − X·z)` over their points, from the syndrome numbered by their count to the last, and the point zero keeps its place in it with a coefficient of zero. A column that obeys it has no wrong symbol elsewhere. Any other column is searched as `Code::correct` searches, and the holders it finds join the list. With more than `⌊r/2⌋` of them, or a search that finds nothing, the block is undecided and its other columns are not looked at. So a file that is wrong in every column costs one search a block, not one a column.

**Work and its limit.** `rebuild`, `repair` and `repairs` take a limit in steps and refuse before they compute anything when the call would take more (`SetError::Work`, with the number needed); `None` sets no limit. `FileSet::rebuild_work`, `FileSet::repair_work` and `FileSet::repairs_work` give the number beforehand. A step is a unit of this count and of nothing else. It grows with the work of making shards; it measures neither time nor operations, and reading the files, their checksums and memory are no part of it:

- Preparing the sources of a block: `k²` steps, for the first block and for every block whose `k` lowest holders are not those of the block before it. Sources that change and come back are prepared again each time. The steps are charged also when nothing of the block has to be made.
- A block of a shard that no file holds, which is made: `k · (s + 64)` steps for its `s` symbols. The 64 stand for working out the coefficients of the block.
- A block of every holder beyond the `k` lowest, which is compared: the same.
- A block that a file holds is copied: nothing.

With `k` = 3, blocks of four symbols and two blocks, the three data files alone cost 9 steps; with one data file lost and one spare file in its place, 9 + 2 · 204; with all three and two spare files, 9 + 4 · 204. The number says what a call costs, not that it succeeds: shards that do not agree and bytes that are not zero after the data show only when the shards are made.

`FileSet::vote` takes the same limit and compares it with `FileSet::vote_work` before anything is computed. That count is a bound and not the exact number, because how many columns disagree shows only when they are looked at; `Vote::steps` is what the call took, never more than the bound. For a block with `m` holders, `r = m − k` spare ones, `t = ⌊r/2⌋` and `s` symbols:

- The syndromes, when `r` is at least 1: `r · m · (s + 64)` steps, and `m²` for the weights of the holders when it is the first such block or its holders are not those of the one before.
- When `t` is at least 1, a column whose holders disagree: `r · (t + 1)` steps, and `(4 · m + 64) · (r + 1)` more when it is searched. At most `t + 1` columns of a block are searched, and after the column on which a block is given up its columns cost nothing. With `t` = 0 the block is given up at the first such column and no column is charged.
- A block with `k` or more holders and `d` shards in dispute, unless its holders settle on nothing: `k² + d · k · (s + 64)`.

The bound takes every column to disagree, `t + 1` columns of each block to be searched, or all of them when it has fewer, and every shard in dispute to be judged. Comparing the bytes of files is no part of the count. Five files of a set of three with one block of two symbols, one file wrong in both columns: 25 + 2 · 5 · 66 + 2 · 4 + 252 = 945 steps, of at most 1,197.

To size a limit by: on one core of a 2022 laptop, with blocks of 4,096 bytes, a step took about one nanosecond in `Field::Gf256` and three to five in `Field::Gf65536`, so ten thousand million steps were some ten seconds in the first field. Checking the files of a pile ran at about 220 megabytes of file per second, which is the speed of the checksum, and writing a set of five files with `k` = 3 at about 100 megabytes of data per second. A vote costs about as much as a rebuild of the same files: in one run, five files with `k` = 3 were voted on at half the speed of their rebuild when they agreed and at a quarter with a file of other data among them, which is wrong in every column; forty files with `k` = 30 at five sixths and at three quarters. These are the figures of one machine, not a promise for another.

**What looking at a pile costs.** The work grows with the number of files and the bytes that are in them, whatever their headers say. Every block that is there is checked once. Files of one shard are compared once, each with one other and only for the blocks it reaches. And the walk over the blocks looks, for each block, only at the shards and the files that reach it, so that thousands of files that end early beside one long file cost no more than their own bytes, be they files of other shards or further files of the same one.

Memory: one entry per file and one bit per block that is in a file. The data is asked for only after every block is known to have `k` holders, so it is never longer than what the files hold. Every list these calls keep, like the data itself, is asked for before it is filled: when the machine refuses the memory, `Pile::new`, `SetFacts::files` and the calls of a set answer `Memory` and nothing is half done. Sorting the files takes no memory of its own. A vote keeps the search for the holders of a block, sized by the bytes a block of the set can have and under 8 MiB as in the correction, one block for the shards in dispute, and one bit per block for every file it names and for the undecided blocks.

## Self-test and tests

Every expected value comes from outside the crate, and every one of them matches what the crate computes: a test fails on the first that does not. The sources:

| Source                                      | What it gives                                                                                                                                        |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `reed-solomon-erasure` 6.0.0                | every shard in `Field::Gf256` and `Field::Gf65536`; its products in the field of pairs                                                               |
| the parity 45 5E of Backblaze and klauspost | the shards of the data 01 02 03 04                                                                                                                   |
| interpolation by hand                       | a set grown from three shards to seven                                                                                                               |
| values computed independently               | products, an inverse, shards and syndromes of `Field::Gf2048`; the votes of five piles                                                               |
| the published check value of CRC-32C        | E3069283 for "123456789"                                                                                                                             |
| a bit-by-bit CRC and a long division        | the checksums of the shard files the tests hold                                                                                                      |
| FIPS 180-4 and Python's `hashlib`           | the SHA-256 of "abc", of no bytes and of the texts of the tests                                                                                      |
| Lagrange interpolation in Python            | the words a card adds, for the plans of changes                                                                                                      |
| tables of logarithms in Python              | for every degree from 2 to 16 and two further polynomials: a product, an inverse and two shards of a set; that each standard polynomial is primitive |
| the shift-and-reduce product and its tables | every product and inverse of `Field::Gf256` and of `Field::Gf2048`, against the mask arithmetic                                                      |

`self_test()` checks, against answers from outside the crate: the published parity 45 5E for the data 01 02 03 04; a set of three shards grown to seven, computed by hand; one shard of the field of pairs, equal to what `reed-solomon-erasure` writes; two words of the field of words computed apart; a changed shard being told from the right one; the published check value of CRC-32C; a shard file of 86 bytes whose three checksums were computed apart, read back whole and refused in the block where one bit was changed; the SHA-256 of "abc" given in FIPS 180-4. With the `alloc` feature it also corrects two byte words and one word of the field of words whose errors were placed by hand, gives up a word with three wrong symbols of seven for which no valid set lies within two symbols, and names, among five files of a set of three, the one that holds other bytes, in the number of steps computed apart; and refuses three files that agree on "abc" and state the SHA-256 of "abd", while the same files stating their own digest rebuild. It checks these known answers; it is not a proof that the build has no other fault.

For the fields of a chosen degree, a reference written apart from the crate in Python, with tables of logarithms and Lagrange interpolation, gave for each of the fifteen standard polynomials and for 0x187 and 0x1002D a product, an inverse and two shards of a set; all seventeen fields match it in every value. The tests also scale shards in every degree against single products and write, rebuild, correct and plan a set in every degree.

The tests in `tests/suite` compare every shard with `reed-solomon-erasure` 6.0.0 for all shapes up to 16 shards in its two fields, for large sets and for each of the 65,536 shard numbers. In all three fields they rebuild every shard from each choice of `k` out of up to 10, correct each placement of wrong symbols within the bound for up to 9 shards, check that one wrong symbol too many is never called clean, and compare the decoder with a search over every choice of `k` shards. The field of words has no second implementation to compare with; its products, an inverse, two shards and three sets of syndromes were computed independently and are pinned in the tests.

For the shard file the tests hold four files whose checksums were computed apart from the crate, by a bit-by-bit CRC and by a long division. They write files of several hundred shapes with a writer of their own that shares no code with the crate and expect the same bytes. They change every single bit of a file, cut it at every length, add bytes after its end, exchange headers and blocks between files, put a header of another length in the place of the copy, and write headers of another version, with unread fields and flags and with values out of range; a run of three thousand random damages checks that what is read is the original header or a refusal, and that a block called intact holds the original bytes.

For sets the tests write files and rebuild the data from every choice of `k` of them in both fields; give every file another damaged block so that no `k` files are whole; leave a block with too few holders; give the same shard twice, whole, damaged in different blocks and with other bytes under right checksums; put a file with other bytes among the sources and among the spare shards; fill the bytes after the data with something else; mix three sets, two of them with one identifier, with files that do not read, in several orders; count the steps of several piles by hand, with sources that change and come back; and make every lost file again byte for byte. A file of 92 bytes computed apart pins what the writer makes of data.

For the vote the tests put a file of other data among five; one wrong symbol in a file at the point zero, among the sources and among the spare shards; as many wrong files as the spare shards outvote, and one more; files of one shard that differ, with and without a file the others confirm, beside exactly `k` holders and beside more; and files cut short, so that the holders change from block to block. They count the steps by the rule in two ways, hold five votes worked out apart from the crate with their steps and bounds, compare the set without some files with the remaining files read anew, and give headers that state four thousand million blocks over files of three. The search itself is run over every choice of wrong shards of small sets in all three fields.

## References

- I. S. Reed, G. Solomon, "Polynomial codes over certain finite fields", Journal of the Society for Industrial and Applied Mathematics 8(2), 1960.
- E. R. Berlekamp, Algebraic Coding Theory, McGraw-Hill, 1968.
- J. L. Massey, "Shift-register synthesis and BCH decoding", IEEE Transactions on Information Theory 15(1), 1969.
- G. Castagnoli, S. Bräuer, M. Herrmann, "Optimization of cyclic redundancy-check codes with 24 and 32 parity bits", IEEE Transactions on Communications 41(6), 1993: the polynomial of CRC-32C. RFC 3720, section 12.1 and appendix B.4, has its conventions as used here.
- M. O. Rabin, "Efficient dispersal of information for security, load balancing, and fault tolerance", Journal of the ACM 36(2), 1989.
- J. S. Plank, "A tutorial on Reed–Solomon coding for fault-tolerance in RAID-like systems", Software: Practice and Experience 27(9), 1997; J. S. Plank, Y. Ding, "Note: correction to the 1997 tutorial on Reed–Solomon coding", Software: Practice and Experience 35(2), 2005.
- [Backblaze JavaReedSolomon](https://github.com/Backblaze/JavaReedSolomon), [klauspost/reedsolomon](https://github.com/klauspost/reedsolomon), [reed-solomon-erasure](https://crates.io/crates/reed-solomon-erasure): implementations of the same code over GF(2^8).
