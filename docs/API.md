# API description

Import the types below from `reed_solomon_core`. Names starting with `strict::` belong to the protected API. Shard indexes identify points in the field. Positions identify entries in the caller’s slice. Lengths are in bytes unless the entry specifies symbols, elements or work steps.

| API group                                                 | Types and operations                                                                                                                                                                                                                 |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [Add the dependency](#add-the-dependency)                 | Cargo features and Rust version                                                                                                                                                                                                      |
| [Encode and recover shards](#encode-and-recover-shards)   | `Code`, `Sources`, `COMPARED`, `Redundancy`, `Correction`                                                                                                                                                                            |
| [Choose a field](#choose-a-field)                         | `Field`, `Binary`                                                                                                                                                                                                                    |
| [Plan and apply corrections](#plan-and-apply-corrections) | `Mark`, `Plan`, `Changes`, `ChangesIter`, `Change`, `ChangeKind`                                                                                                                                                                     |
| [Use a cyclic code](#use-a-cyclic-code)                   | `CyclicCode`, `CyclicWords`                                                                                                                                                                                                          |
| [Write and read shard files](#write-and-read-shard-files) | `SetFacts`, `DataDigest`, `Header`, `ShardFile`, `Block`, `Frame`, `Length`, `Condition`, `Survey`                                                                                                                                   |
| [Rebuild a set of files](#rebuild-a-set-of-files)         | `Pile`, `FileSet`, `Member`, `Coverage`, `Stretch`, `Stretches`, `Dispute`, `Vote`, `Outvoted`, `Rebuilt`, `Written`, `DigestCheck`                                                                                                  |
| [Handle protected data](#handle-protected-data)           | `Buffer`, `strict::Held`, `strict::HeldMut`, `strict::Code`, `strict::Sources`, `strict::CyclicCode`, `strict::values`, `strict::SetFacts`, `strict::Header`, `strict::read`, `strict::ShardFile`, `strict::Pile`, `strict::FileSet` |
| [Inspect errors](#inspect-errors)                         | `FieldError`, `CodeError`, `FileError`, `SetError`                                                                                                                                                                                   |
| [Run the self-test](#run-the-self-test)                   | `self_test`, `SelfTest`, `Check`                                                                                                                                                                                                     |

The Rust examples use the default features. Each snippet can be run in a function that returns `Result<(), Box<dyn std::error::Error>>`. The protected-memory example also needs a supported operating system and sufficient locked memory.

## Add the dependency

The crate uses Rust 2024 and requires Rust 1.99 or later. It is on crates.io, and the package there is the same file as the signed one on the release page, with the same SHA-256. To build from the source of a signed tag instead, use `{ git = "https://github.com/hobby-eng/reed-solomon-core", tag = "v0.2.0" }`; for a sibling checkout, `{ path = "../reed-solomon-core" }`.

```toml
[dependencies]
reed-solomon-core = "0.2.0"
```

The default feature, `protect`, enables `alloc` and the standard library; with it every call that works on data, and `==` on a `Buffer`, a `Changes` or an intact `Block`, overwrites 128 KiB of the calling thread's stack below it when it returns or a panic unwinds out of it, so the thread needs a little more free stack than that. WebAssembly clears nothing, a build that aborts on panic clears nothing, and the window is fixed, best effort (docs/TECHNICAL.md, Secrets in memory). A result a strict call returns is compared, like any other, where the caller compares it, with this clearing and not on a protected stack. The clearing costs about 2 µs for each outermost call on the measuring machine: a third more for a call on 512-byte stripes, a twentieth on 4 KiB, nothing measurable on 64 KiB or on the whole-set calls of `examples/measure.rs`. Select `default-features = false, features = ["alloc"]` for owned results without OS page protection. With `default-features = false` alone, callers supply the buffers. The signature tables mark calls that need an optional feature.

### Verify a release package

Each release ships with a detached OpenPGP signature on the crate and a signed checksum manifest. With the release signing key in your keyring, run `gpg --verify reed-solomon-core-<version>.crate.asc reed-solomon-core-<version>.crate` from the download directory. [Releases](https://github.com/hobby-eng/reed-solomon-core/releases).

For tags built by the release provenance workflow, run the following in a checkout of this repository. Set `tag` to the release tag and `archive` to the exact `.crate` file you want to verify. To fetch the workflow's copy, use `gh run download "$run_id" -n release-package` with the run ID for that tag. GitHub checks the digest of that file against the attestation for the packaging workflow, tag and commit.

```sh
commit=$(git rev-parse --verify "refs/tags/${tag}^{commit}") && \
  gh attestation verify "$archive" --repo hobby-eng/reed-solomon-core \
  --signer-workflow hobby-eng/reed-solomon-core/.github/workflows/release.yml \
  --source-ref "refs/tags/$tag" --source-digest "$commit"
```

## Encode and recover shards

For a set with n distinct shard indexes, k is the minimum needed to recover from losses. With three output shards, k = 2 tolerates one missing shard and k = 3 requires all three. A duplicate copy of a shard can replace a damaged copy of the same index. It supplies no additional index.

The [2-of-3 example in the README](../README.md#recover-a-missing-shard) shows how to rebuild a missing data shard from one data shard and one parity shard.

### Recover, extend and correct a set

Here k = 3 and n = 5. The first part recovers two missing shards and creates a new index without the original data. The second part starts with all five shards and repairs one wrong byte at an unknown position. Two spare shards suffice to locate and correct that one error.

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

### Code

Systematic evaluation code. Choose the field and the threshold k. Each input shard has a distinct field index.

| Signature                                                                                          | Contract                                                                                                                                                                                                                         |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `apply(&self, shards: &mut [(u16, &mut [u8])], changes: &Changes) -> Result<(), CodeError>`        | Requires `alloc`. Checks the plan fingerprint, shard order, current values and replacement values before applying the list. A stale or mismatched plan returns CodeError::Changes.                                               |
| `correct(&self, shards: &mut [(u16, &mut [u8])]) -> Result<Correction, CodeError>`                 | Requires `alloc`. Repairs in place within the correction radius. Counts follow input order. With exactly k valid inputs the result is Unchecked. An unresolved column leaves every shard as supplied.                            |
| `correction_memory(&self, shards: usize) -> usize`                                                 | Requires `alloc`. Bound in bytes on the decoder’s working storage for this shard count. Page rounding, guard pages, allocator overhead, the caller’s shards and the strict thread stack affect process memory separately.        |
| `encode(&self, data: &[&[u8]], targets: &[u16]) -> Result<Vec<Buffer>, CodeError>`                 | Requires `alloc`. Takes exactly k equally long data shards in index order (0 through k − 1). Returns one buffer per target in the order requested. Repeated targets are allowed.                                                 |
| `field(&self) -> Field`                                                                            | The field of the code.                                                                                                                                                                                                           |
| `new(field: Field, threshold: usize) -> Result<Self, CodeError>`                                   | The code of `field` with this threshold, from 1 to the field's number of shards.                                                                                                                                                 |
| `plan(&self, shards: &[(u16, &[u8])], marks: &[Mark]) -> Result<Plan, CodeError>`                  | Requires `alloc`. Finds marked erasures and unknown errors. A mark names the position in the supplied slice and the symbol offset. Marked values must still fit the field. Exactly k inputs and any valid marks give Unresolved. |
| `rebuild(&self, shards: &[(u16, &[u8])], targets: &[u16]) -> Result<Vec<Buffer>, CodeError>`       | Requires `alloc`. Takes at least k distinct indexed inputs and interpolates from the first k. Extra inputs are checked for valid indexes, lengths and symbols. Use redundancy to test whether they agree. Targets may repeat.    |
| `redundancy(&self, shards: &[(u16, &[u8])]) -> Result<Redundancy, CodeError>`                      | Requires `alloc`. Returns None (the Redundancy variant) with exactly k valid inputs. With spare inputs, checks their contents against the first k.                                                                               |
| `sources<'a>(&self, indexes: &'a [u16], weights: &'a mut [u16]) -> Result<Sources<'a>, CodeError>` | Prepare interpolation weights for exactly k distinct indexes using at least k u16 elements. Computing the weights takes k(k − 1) multiplications and k inversions. Reuse them for further targets.                               |
| `threshold(&self) -> usize`                                                                        | How many shards rebuild a set.                                                                                                                                                                                                   |

### Sources

`threshold` shards of one set, chosen as the sources of any other shard of it.

| Signature                                                                                                    | Contract                                                                                                                                                                                                                                                                                                       |
| ------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agrees(&self, target: u16, stripes: &[&[u8]], shard: &[u8]) -> Result<bool, CodeError>`                     | Whether `shard` is what `Sources::build` would write for `target`, made `COMPARED` bytes at a time. Fails as `Sources::build` does, then, with the `protect` feature, with `CodeError::Memory` when the system gives no page for those bytes; `Sources::agrees_in` takes them from the caller instead.         |
| `agrees_in(&self, target: u16, stripes: &[&[u8]], shard: &[u8], room: &mut [u8]) -> Result<bool, CodeError>` | `Sources::agrees` with the expected bytes made in `room`, at least `COMPARED` bytes, which a caller comparing many stretches keeps for all of them and wipes, since they follow from the data. Fails as `Sources::build` does, and with `CodeError::Buffer` for a shorter room after the counts and lengths.   |
| `build(&self, target: u16, stripes: &[&[u8]], out: &mut [u8]) -> Result<(), CodeError>`                      | Writes into `out` shard `target` for one stretch of the set, from the same stretch of every source, in the order given to `Code::sources`. A large shard can be made piece by piece. Fails on a bad target, a wrong stripe count, unequal lengths or a symbol outside the field; `out` is then left as it was. |

### COMPARED

Bytes `Sources::agrees` compares at a time, and the least room `Sources::agrees_in` takes: a whole number of symbols of every field.

`const COMPARED: usize = 256`

### Redundancy

Requires `alloc`. What the shards beyond the threshold say about a set.

| Variant   | Contract                                                                                                             |
| --------- | -------------------------------------------------------------------------------------------------------------------- |
| `None`    | Exactly `threshold` shards were given: any such shards form a set, so there is nothing to compare them with.         |
| `Agrees`  | Every shard beyond the first `threshold` is what those make.                                                         |
| `Differs` | A shard beyond the first `threshold` differs from what those make: some shard was changed or belongs to another set. |

### Correction

Requires `alloc`. What `Code::correct` found.

| Variant                          | Contract                                                                                                                             |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `Unchecked`                      | Exactly `threshold` shards were given: there is nothing to compare them with, and nothing was checked.                               |
| `Agrees`                         | The shards agree; nothing was changed.                                                                                               |
| `Corrected(Vec<usize>)`          | One count per supplied shard. Cyclic correction counts each restored lost position even when it already contained the right value.   |
| `Unresolved { columns: usize, }` | Counts columns for which the decoder could not establish a result, including columns with too many erasures. No shards were changed. |

## Choose a field

### Field

The field a code works in. Its elements are the symbols of every shard and the points of the shard indexes.

| Variant          | Contract                                                                                                                                                                                                                                                                                                                                    |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Gf256`          | GF(2^8) with the polynomial x^8 + x^4 + x^3 + x^2 + 1 (0x11D): one byte per symbol and up to 256 shards in a set. The bytes are those of the Backblaze, klauspost and `reed-solomon-erasure` codes.                                                                                                                                         |
| `Gf65536`        | GF(2^16) as the extension GF(2^8)[x] / (x^2 + 2x + 128) of the field above: two bytes `[high, low]` per symbol, standing for `high·x + low`, and up to 65,536 shards in a set. The bytes are those of `reed-solomon-erasure`'s `galois_16`.                                                                                                 |
| `Gf2048`         | GF(2^11) with the polynomial x^11 + x^2 + 1 (0x805): a symbol is a number below 2,048 in two bytes, the high byte first, and a set has up to 2,048 shards. One symbol is one word of a list of 2,048 words, so that the shards a code adds are words of the list too. Bytes that hold a larger number are refused with `CodeError::Symbol`. |
| `Binary(Binary)` | A primitive binary field of degree 2–16. Symbols use one byte through degree 8 and two big-endian bytes above it. This variant has no file-format identifier.                                                                                                                                                                               |

| Signature                                                                 | Contract                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `binary(degree: u8, polynomial: Option<u32>) -> Result<Self, FieldError>` | GF(2^`degree`) for a degree from 2 to 16, by `polynomial` (with its x^m term, so x^4 + x + 1 is 0x13) or by the standard primitive polynomial of that degree. Degree 8 with 0x11D is `Field::Gf256` and degree 11 with 0x805 is `Field::Gf2048`. Take the smallest field whose symbols hold your data and that has as many elements as you need shards. Fails for a degree outside 2..=16 or a polynomial that isn't primitive. |
| `order(self) -> usize`                                                    | The number of elements: the most shards a set can have, with the indexes 0 to `order() - 1`.                                                                                                                                                                                                                                                                                                                                    |
| `symbol_bytes(self) -> usize`                                             | Bytes of one symbol. Every shard is a whole number of symbols long.                                                                                                                                                                                                                                                                                                                                                             |

### Binary

A field GF(2^m) by a primitive polynomial: what `Field::binary` makes when the program chooses a field other than the three that have names.

| Signature                 | Contract                                                    |
| ------------------------- | ----------------------------------------------------------- |
| `degree(self) -> u8`      | `m`: the bits of an element.                                |
| `polynomial(self) -> u32` | The polynomial, its x^m term included: x^4 + x + 1 is 0x13. |

## Plan and apply corrections

### Mark

Requires `alloc`. One known erasure. shard is a position in the supplied list and symbol is an offset in symbols. For a single mark supply a valid field element, such as zero, as the placeholder.

| Field           | Contract                                                                 |
| --------------- | ------------------------------------------------------------------------ |
| `shard: usize`  | The place of its shard among the shards given.                           |
| `symbol: usize` | The place of the symbol in the shard, counted in symbols from the first. |

### Plan

Requires `alloc`. What `Code::plan` found. Nothing was changed.

| Variant                          | Contract                                                                                                                                                                                                                                                                                                                       |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `Unchecked`                      | Exactly `threshold` shards and no mark: there is nothing to compare them with, and nothing was checked.                                                                                                                                                                                                                        |
| `Agrees`                         | The shards agree and no symbol was marked.                                                                                                                                                                                                                                                                                     |
| `Changes(Changes)`               | Every column can be put right by these changes, in the order of the symbols and, within one, of the shards. Every marked symbol is among them. The list is bound to the shards it was made for. Whether the result is the original data is for a digest or an authentication to say. The list wipes itself when it is dropped. |
| `Unresolved { columns: usize, }` | In `columns` symbol columns the spare shards cannot put the column right: more wrong and marked symbols than `2e + s ≤ m − k` allows, or more marks than spare shards.                                                                                                                                                         |

### Changes

Requires `alloc`. The changes of a `Plan`, in the order of the symbols and, within one, of the shards, bound to the shards the plan was made for: `apply` refuses other shards before it writes. They lie in a `Buffer`, wiped when dropped and, with the `protect` feature, in pages of their own. Its `Debug` text gives their number.

| Signature                                                                    | Contract                                                                                                                                                                                                                                                    |
| ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `get(&self, at: usize) -> Option<Change>`                                    | The place and kind of the change at place `at`, counted from zero; `None` past the last. What the symbol held and becomes is not read.                                                                                                                      |
| `is_empty(&self) -> bool`                                                    | Whether there is no change.                                                                                                                                                                                                                                 |
| `is_locked(&self) -> bool`                                                   | Whether the list lies in locked pages, as `Buffer::is_locked` says of a buffer.                                                                                                                                                                             |
| `iter(&self) -> ChangesIter<'_>`                                             | The places and kinds of the changes, in their order.                                                                                                                                                                                                        |
| `len(&self) -> usize`                                                        | How many changes there are.                                                                                                                                                                                                                                 |
| `values<R>(&self, at: usize, look: impl FnOnce(u16, u16) -> R) -> Option<R>` | What the symbol of the change at place `at` holds and becomes, given to `look`; `None` past the last. They can be a secret: a program looks at what it needs and keeps no copy, or uses the strict mode's `values`, which runs `look` on a protected stack. |

### ChangesIter

Requires `alloc`. Exact-size iterator over Change metadata. Iterating &Changes also yields Change values. Use values to inspect the old and new symbols.

### Change

Requires `alloc`. One symbol that a `Plan` changes: its place and how. What it holds and becomes can be a secret, such as a word of a recovery phrase, so a change does not carry them: `Changes::values` lets a program look at them where the list keeps them, and the strict mode's `values` does so on a protected stack.

| Field              | Contract                                                                 |
| ------------------ | ------------------------------------------------------------------------ |
| `shard: usize`     | The place of its shard among the shards given.                           |
| `symbol: usize`    | The place of the symbol in the shard, counted in symbols from the first. |
| `kind: ChangeKind` | Whether it was marked or found.                                          |

### ChangeKind

Requires `alloc`. How a symbol comes to change.

| Variant    | Contract                                                                                                 |
| ---------- | -------------------------------------------------------------------------------------------------------- |
| `Filled`   | The symbol was marked unreadable and gets the value the others give it, which may be the value it holds. |
| `Replaced` | The symbol was not marked, and the search found it wrong.                                                |

## Use a cyclic code

Choose these types when the format fixes the parity count, first root and root step. Data shards come first in the slice. A whole shard listed in `lost` costs one parity symbol per column. Unknown errors cost two. [Generator and ordering](TECHNICAL.md#the-cyclic-code).

### Correct a smudged or mistyped word on a card

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
let Plan::Changes(changes) = code.plan(&views, &[], &[Mark { shard: 2, symbol: 0 }])? else {
    return Err("the card is within 2e + s ≤ 4".into());
};
let mut buffers: Vec<&mut [u8]> = read.iter_mut().map(|word| word.as_mut_slice()).collect();
code.apply(&mut buffers, &changes)?;
assert_eq!((read[2], read[10]), (0x220_u16.to_be_bytes(), 0x6A9_u16.to_be_bytes()));
```

### CyclicCode

Requires `alloc`. Owned shortened systematic cyclic code. The first data places carry the message and the last parity places carry the remainder of d(x)·x^parity divided by the generator.

| Signature                                                                                                   | Contract                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agrees(&self, shards: &[&[u8]]) -> Result<bool, CodeError>`                                                | Whether every word of `shards`, all `length` of them in their order, is a word of the code: it stops at the first stripe of words that holds one that is not. Fails on another number of shards, unequal lengths, a symbol outside the field, or without memory.                                                                                                                                                                                              |
| `apply(&self, shards: &mut [&mut [u8]], changes: &Changes) -> Result<(), CodeError>`                        | Makes `changes`, the list `CyclicCode::plan` found, as `Code::apply` does: the shards must be those the plan was made for, in the same order with every byte as it was, and the code this one; otherwise nothing is changed and it fails with `CodeError::Changes`. Fails with `CodeError::Memory` when the list of the shards gets no memory.                                                                                                                |
| `correct(&self, shards: &mut [&mut [u8]], lost: &[usize]) -> Result<Correction, CodeError>`                 | Corrects the words of `shards`, all `length` of them in their order, in place: the shards at the places `lost` are written anew, and wrong symbols elsewhere put right, while `2e + s ≤ parity` with `s` the lost shards. Either every word is put right or nothing changes; neither pass keeps a list. Fails on another number of shards, unequal lengths, a lost place outside the word, a symbol of a shard not lost outside the field, or without memory. |
| `data(&self) -> usize`                                                                                      | The number of data shards, which come first.                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `encode(&self, data: &[&[u8]]) -> Result<Vec<Buffer>, CodeError>`                                           | The `parity` shards of `data`, in their order: exactly `data` shards of one length, a whole number of symbols, each an element of the field. Counts and lengths are refused before symbols, as in `Code`.                                                                                                                                                                                                                                                     |
| `field(&self) -> Field`                                                                                     | The field of the symbols.                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `generator(&self) -> &[u16]`                                                                                | The generator, its coefficients from x^parity, which is 1, down to x^0.                                                                                                                                                                                                                                                                                                                                                                                       |
| `length(&self) -> usize`                                                                                    | The number of shards of a word: data and parity.                                                                                                                                                                                                                                                                                                                                                                                                              |
| `new(field: Field, data: usize, parity: usize, first_root: u32, root_step: u32) -> Result<Self, CodeError>` | Requires positive data and parity counts, distinct root positions and a word no longer than the order of β = α^root_step. The composite Gf65536 representation is unsupported. Returns CodeError::Shape for an unsupported shape.                                                                                                                                                                                                                             |
| `parity(&self) -> usize`                                                                                    | The number of parity shards, which follow the data.                                                                                                                                                                                                                                                                                                                                                                                                           |
| `plan(&self, shards: &[&[u8]], lost: &[usize], marks: &[Mark]) -> Result<Plan, CodeError>`                  | The changes that would put the words of `shards` right, with the shards at the places `lost` and the symbols `marks` unreadable; nothing is changed until `CyclicCode::apply`. Every lost and marked symbol is listed, as `ChangeKind::Filled`, so the list grows with them. Fails as `CyclicCode::correct` does, or with `CodeError::Mark` for a mark outside the shards; never `Plan::Unchecked`.                                                           |
| `words(&self) -> CyclicWords<'_>`                                                                           | The same code over this one's generator, for its calls without an allocator.                                                                                                                                                                                                                                                                                                                                                                                  |

### CyclicWords

The cyclic code over memory the caller gives, without an allocator: the code of `CyclicCode`, its generator in a slice of the caller's, with the check shards and the check of stripes of shards, and one word at a time put right in place. A card of words is one word.

| Signature                                                                                                                             | Contract                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agrees(&self, shards: &[&[u8]], syndromes: &mut [u8]) -> Result<bool, CodeError>`                                                    | Whether every word of `shards`, all `length` of them in their order, is a word of the code. `syndromes` is room for the syndromes of a stripe of words, at least `parity` symbols, and the more room, the fewer stripes; it is wiped before the call returns, unless the call is refused. It stops at the first stripe with a word that is not of the code. Fails as `CyclicWords::encode` does, and with `CodeError::Length` for too little room. |
| `correct_word(&self, word: &mut [u16], erased: &[usize], room: &mut [u16]) -> Result<Option<usize>, CodeError>`                       | Pass exactly length() symbols, at least room() scratch elements, and strictly increasing, unique erased positions. Erased values may be placeholders outside the field. Ok(Some(n)) counts writes. Ok(None) leaves the word intact. Scratch is wiped after a search and untouched on input-validation errors.                                                                                                                                      |
| `data(&self) -> usize`                                                                                                                | The number of data shards, which come first.                                                                                                                                                                                                                                                                                                                                                                                                       |
| `encode(&self, data: &[&[u8]], parity: &mut [&mut [u8]], remainder: &mut [u16]) -> Result<(), CodeError>`                             | The parity shards of `data` written into `parity`: `data` shards and `parity` outputs, all of one length, a whole number of symbols of the field; `remainder` is room of exactly `parity` elements. Counts, lengths, then symbols are refused, before anything is written.                                                                                                                                                                         |
| `field(&self) -> Field`                                                                                                               | The field of the symbols.                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `generator(&self) -> &[u16]`                                                                                                          | The generator, its coefficients from x^parity, which is 1, down to x^0.                                                                                                                                                                                                                                                                                                                                                                            |
| `length(&self) -> usize`                                                                                                              | The number of shards of a word: data and parity.                                                                                                                                                                                                                                                                                                                                                                                                   |
| `new(field: Field, data: usize, parity: usize, first_root: u32, root_step: u32, generator: &'g mut [u16]) -> Result<Self, CodeError>` | Builds the same generator in the caller’s slice of exactly parity + 1 elements. The shape restrictions are the same as for CyclicCode.                                                                                                                                                                                                                                                                                                             |
| `parity(&self) -> usize`                                                                                                              | The number of parity shards, which follow the data.                                                                                                                                                                                                                                                                                                                                                                                                |
| `room(parity: usize) -> usize`                                                                                                        | The elements of working room `CyclicWords::correct_word` takes for `parity` parity shards.                                                                                                                                                                                                                                                                                                                                                         |

## Write and read shard files

A file contains one shard and two copies of its header. All indexes are zero-based. The reader borrows the supplied bytes. Keep them available while using the result. [Version 1 wire format](TECHNICAL.md#the-shard-file).

### SetFacts

What every shard file of a set says about the set: the code, the set's identifier, the length of the data, the length of a block and, when the program gave one, the digest of the data. Files that say the same belong to one set; the lengths of a shard and of a file follow from it and are the same for every file of the set.

| Signature                                                                                                                            | Contract                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `block_len(&self) -> u32`                                                                                                            | The length of a block in bytes; the last block of a shard may be shorter.                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `blocks(&self) -> u64`                                                                                                               | How many blocks every shard of the set has: none for empty data, at most 2^32.                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `code(&self) -> Code`                                                                                                                | The code of the set: its field and its threshold.                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `data_len(&self) -> u64`                                                                                                             | The length of the data in bytes.                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `digest(&self) -> Option<&[u8; 32]>`                                                                                                 | The SHA-256 of the data that the files of the set state, if they state one. It says whether the data came back unchanged, not who wrote the set: whoever changes the data can state its new digest. Only a digest the program trusts from elsewhere tells that.                                                                                                                                                                                                                                         |
| `file_len(&self) -> u64`                                                                                                             | The length of every shard file of the set in bytes.                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `files(&self, data: &[u8], indexes: &[u16]) -> Result<Vec<Buffer>, FileError>`                                                       | Requires `alloc`. The shard files numbered `indexes` for the set whose data is `data`, in the order asked. The data is cut into the `k` data shards (the last padded with zeros), so files 0 to `k − 1` hold the data itself. Copies made along the way are wiped, and each file comes in a `Buffer`. If the facts state a SHA-256, data with another digest gets no files. Fails on wrong data length or digest, an index outside the field, or no memory. `SetFacts::files_into` writes into outputs. |
| `files_into(&self, data: &[u8], indexes: &[u16], outs: &mut [&mut [u8]]) -> Result<(), FileError>`                                   | Requires `alloc`. Writes one complete shard file per requested index into an exactly sized buffer. The writer verifies data length and any declared digest. Failures before writing preserve outputs. A later failure zeroes all outputs.                                                                                                                                                                                                                                                               |
| `for_data(field: Field, id: &[u8], threshold: usize, data: &[u8], block_len: u32, digest: DataDigest) -> Result<Self, FileError>`    | The facts of the set whose data is `data`, with its length and, for `DataDigest::Sha256`, its digest computed from `data`. Fails as `SetFacts::new` does, before the digest is computed.                                                                                                                                                                                                                                                                                                                |
| `id(&self) -> &[u8]`                                                                                                                 | The identifier of the set.                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `new(field: Field, id: &[u8], threshold: usize, data_len: u64, block_len: u32, digest: Option<[u8; 32]>) -> Result<Self, FileError>` | The facts of a set: field, threshold, an identifier of 1 to 32 bytes, data length, block length and optionally the data's SHA-256. A block is the unit of damage: a wrong byte loses its block. A program that has the data should use `SetFacts::for_data`; this is for facts read from a header. Fails for a field other than Gf256 and Gf65536, a bad identifier, threshold or block length, or files that would be too large.                                                                       |
| `shard_len(&self) -> u64`                                                                                                            | The length of every shard of the set in bytes: the data length divided by the threshold, rounded up to a whole number of symbols.                                                                                                                                                                                                                                                                                                                                                                       |

### DataDigest

Whether the files of a set state the SHA-256 of the data. The crate computes it from the data when `SetFacts::for_data` describes the set, and compares it with the rebuilt data before it gives the data back.

| Variant  | Contract                                                                           |
| -------- | ---------------------------------------------------------------------------------- |
| `None`   | The files state no digest. A rebuild then says that nothing was compared.          |
| `Sha256` | The files state the SHA-256 of the data: 32 more bytes in each copy of the header. |

### Header

The header of one shard file: the facts of its set and the index of its shard.

| Signature                                                                 | Contract                                                                                                                                                                                                                                                                                                                         |
| ------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `as_bytes(&self) -> &[u8]`                                                | The bytes of the header: a shard file starts with them and holds them again before its trailer.                                                                                                                                                                                                                                  |
| `block_checksum(&self, number: u32, block: &[u8]) -> u32`                 | The checksum written after block `number` of the shard, whose bytes are `block`. It covers the header's own checksum and the block's number as well, so the bytes of another file or of another place do not pass unless their checksum happens to be the same.                                                                  |
| `file(&self, shard: &[u8]) -> Result<Buffer, FileError>`                  | Requires `alloc`. The whole shard file for `shard`: header, blocks with checksums, the header again and the trailer, in a `Buffer`. Fails with `FileError::Shard` for a wrong length or `FileError::Memory` when there's no memory.                                                                                              |
| `file_into(&self, shard: &[u8], out: &mut [u8]) -> Result<(), FileError>` | The shard file for `shard` written into `out`, exactly `SetFacts::file_len` bytes long, as `Header::file` makes it, without an allocator: `out` can be a mapping of a file. Fails with `FileError::Output` for an output of another length, then with `FileError::Shard` for a shard of another length; `out` is then as it was. |
| `index(&self) -> u16`                                                     | The index of the file's shard in its set.                                                                                                                                                                                                                                                                                        |
| `new(set: SetFacts, index: u16) -> Result<Self, FileError>`               | The header of shard `index` of `set`.                                                                                                                                                                                                                                                                                            |
| `set(&self) -> &SetFacts`                                                 | The facts of the set the file belongs to.                                                                                                                                                                                                                                                                                        |
| `trailer(&self) -> [u8; 12]`                                              | The trailer: the last twelve bytes of the file, which name the length of the header.                                                                                                                                                                                                                                             |

### ShardFile

A shard file that was read: its header and the bytes its blocks come from. Blocks are checked only when asked for (`ShardFile::block`, `ShardFile::survey`); nothing is allocated, and `Debug` shows no shard bytes.

| Signature                                          | Contract                                                                                                                                                                                                                                                                                                                                                                               |
| -------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `block(&self, number: u32) -> Option<Block<'a>>`   | Block `number` of the shard, its checksum checked now; `None` when the shard has no such block. The bytes come from the block's own place between the two copies of the header and from nowhere else.                                                                                                                                                                                  |
| `frame(&self) -> Frame`                            | The condition of the two copies of the header, of the trailer and of the file's length.                                                                                                                                                                                                                                                                                                |
| `header(&self) -> &Header`                         | The header of the file.                                                                                                                                                                                                                                                                                                                                                                |
| `read(bytes: &'a [u8]) -> Result<Self, FileError>` | Reads the shard file `bytes`, whole or what is left of it. The header at the start and the copy at the end are judged apart; one good copy is enough, and a file cut short keeps its whole blocks. Fails when the two copies are good and differ (`FileError::Conflict`), when the file is of an unsupported field, flag or version, or when it is damaged or not a shard file at all. |
| `survey(&self) -> Survey`                          | Checks every block that is wholly there and counts the pieces of the file by their condition: one pass over the bytes given.                                                                                                                                                                                                                                                           |

### Block

One block of a shard, as it was found. Its `Debug` text gives the length of an intact block, not its bytes.

| Variant            | Contract                                                                   |
| ------------------ | -------------------------------------------------------------------------- |
| `Intact(&'a [u8])` | The bytes of the block, whose checksum holds.                              |
| `Damaged`          | The block is there and its checksum does not hold: its bytes are not used. |
| `Missing`          | The file ends before the end of the block.                                 |

#### Read a shard file with a damaged header

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

### Frame

What surrounds the blocks of a shard file, as it was found.

| Field                | Contract                                                                              |
| -------------------- | ------------------------------------------------------------------------------------- |
| `lead: Condition`    | The header at the start. When it is damaged the file was read by the copy at the end. |
| `tail: Condition`    | The copy of the header at the end.                                                    |
| `trailer: Condition` | The trailer after that copy.                                                          |
| `length: Length`     | The length of the file.                                                               |

### Length

The length of a file against the length its header gives.

| Variant      | Contract                                                 |
| ------------ | -------------------------------------------------------- |
| `Exact`      | As long as the header says.                              |
| `Short(u64)` | Shorter by this many bytes: the end of the file is lost. |
| `Long(u64)`  | Longer by this many bytes, which belong to nothing.      |

### Condition

How a piece of a shard file was found.

| Variant   | Contract                                  |
| --------- | ----------------------------------------- |
| `Intact`  | Its bytes are there and pass their check. |
| `Damaged` | Its bytes are there and do not pass.      |
| `Missing` | The file ends before its last byte.       |

### Survey

A count of a shard file's pieces by their condition.

| Field          | Contract                               |
| -------------- | -------------------------------------- |
| `frame: Frame` | What surrounds the blocks.             |
| `intact: u64`  | Blocks whose checksum holds.           |
| `damaged: u64` | Blocks that are there and do not pass. |
| `missing: u64` | Blocks the file ends before.           |

| Signature              | Contract                                                                                                                                         |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `whole(&self) -> bool` | Whether the file is exactly what `Header::file_into` writes for its header and the bytes of its blocks: every piece intact and the length exact. |

## Rebuild a set of files

Keep files and their mappings unchanged while a pile or any derived set borrows them. Thresholds apply to distinct shard indexes per block. Disagreeing copies of one index appear in disputes and cease to be holders for that block. Rebuild can still succeed from other holders. Work limits are in steps. Pass None for no limit or use the matching work estimate.

### Pile

Requires `alloc`. Any number of files looked at together, sorted into the sets whose facts they state. The order of the files doesn't matter. The work grows with the files and their bytes, whatever their headers claim.

| Signature                                                | Contract                                                                                                                                                                   |
| -------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `new(files: &[&'a [u8]]) -> Result<Self, FileError>`     | Reads `files` and sorts them into sets. A file that doesn't read is not an error but an entry of `Pile::readings`; only running out of memory fails (`FileError::Memory`). |
| `readings(&self) -> &[Result<ShardFile<'a>, FileError>]` | What became of each file, in the order given: the file as read, or why it does not read.                                                                                   |
| `sets(&self) -> &[FileSet<'a>]`                          | The sets found, in an order of their facts that the order of the files does not change.                                                                                    |

### FileSet

Requires `alloc`. A set as it was found among files: the files that state its facts, shard by shard.

| Signature                                                                                                     | Contract                                                                                                                                                                                                                                                                                                                                                                                                             |
| ------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `coverage(&self) -> Coverage`                                                                                 | How many shards hold the blocks of the set.                                                                                                                                                                                                                                                                                                                                                                          |
| `disputed_blocks(&self, index: u16) -> impl Iterator<Item = u64> + '_`                                        | The blocks of shard `index` on which its files differ, in order: every one of them, where `FileSet::disputes` has their number and the first. Nothing for a shard without a dispute.                                                                                                                                                                                                                                 |
| `disputes(&self) -> &[Dispute]`                                                                               | The shards whose files differ in a block that each holds intact.                                                                                                                                                                                                                                                                                                                                                     |
| `facts(&self) -> &SetFacts`                                                                                   | The facts every file of the set states.                                                                                                                                                                                                                                                                                                                                                                              |
| `id_shared(&self) -> bool`                                                                                    | Whether another set among the files has the same identifier and other facts. Two sets never share an identifier by the rules, so one of them is not what it says; nothing here tells which, and each is rebuilt only when asked for by itself.                                                                                                                                                                       |
| `members(&self) -> &[Member]`                                                                                 | The files of the set in the order of their shard indexes, each as it was found.                                                                                                                                                                                                                                                                                                                                      |
| `rebuild(&self, limit: Option<u128>) -> Result<Rebuilt, SetError>`                                            | Interpolates each block from its k lowest distinct undisputed holders and checks extra holders. Validates padding and any declared digest before returning the data.                                                                                                                                                                                                                                                 |
| `rebuild_into(&self, out: &mut [u8], limit: Option<u128>) -> Result<Written, SetError>`                       | Writes exactly data_len() bytes to out. Early validation failures leave the destination intact. Failures after writing starts, including a digest mismatch, zero the entire destination.                                                                                                                                                                                                                             |
| `rebuild_work(&self) -> Result<u128, SetError>`                                                               | The steps `FileSet::rebuild` needs, to set its limit by. A step counts the work of making shards only, not time, reading or memory. The count says what the call costs, not that it will succeed. Fails when the files can't rebuild the set, or without memory.                                                                                                                                                     |
| `repair(&self, index: u16, limit: Option<u128>) -> Result<Buffer, SetError>`                                  | The file of shard `index`, made from the files at hand: a lost file byte for byte, or a new one. Spare shards are still compared, but the data isn't assembled or its digest checked; a later rebuild checks that. Fails as `FileSet::rebuild`, or for an index outside the field.                                                                                                                                   |
| `repair_work(&self, index: u16) -> Result<u128, SetError>`                                                    | The steps `FileSet::repair` needs for shard `index`, counted as `FileSet::rebuild_work` counts. Fails for an index outside the field first, otherwise as that call.                                                                                                                                                                                                                                                  |
| `repairs(&self, indexes: &[u16], limit: Option<u128>) -> Result<Vec<Buffer>, SetError>`                       | The files of shards `indexes`, in the order asked, made in one pass: each block is read and compared once for all of them. A repeated index gives the same file twice. Fails as `FileSet::repair`. `FileSet::repairs_into` writes the files into the program's own outputs instead.                                                                                                                                  |
| `repairs_into(&self, indexes: &[u16], outs: &mut [&mut [u8]], limit: Option<u128>) -> Result<u128, SetError>` | Writes exactly file_len() bytes per requested index in output order. Repeated targets are allowed. Early validation failures preserve all outputs and late failures zero them all.                                                                                                                                                                                                                                   |
| `repairs_work(&self, indexes: &[u16]) -> Result<u128, SetError>`                                              | The steps `FileSet::repairs` needs for `indexes`. All of them are made in one pass, so each block's sources are prepared once; a repeated index counts once.                                                                                                                                                                                                                                                         |
| `stretches(&self) -> Result<Stretches<'_, 'a>, FileError>`                                                    | The blocks of the set in stretches held by the same number of shards: what rebuilds, what spare shards confirm, what is lost. Fails only without memory.                                                                                                                                                                                                                                                             |
| `vote(&self, limit: Option<u128>) -> Result<Vote, SetError>`                                                  | Identifies files contradicted by an established block. A disagreement among holders can remain undecided. When holders agree but disputed copies give insufficient support, the block can produce neither an outvoted file nor an undecided entry.                                                                                                                                                                   |
| `vote_work(&self) -> Result<u128, SetError>`                                                                  | An upper bound on the steps `FileSet::vote` can take, to set its limit by. It assumes every column disagrees and the most searches per block; reading files and asking for memory are not counted, and a change in the number of holders from one block to the next makes a new search with two buffers of its own, which with `protect` lie in pages of their own. Fails only when there's no memory for the count. |
| `without(&self, files: &[usize]) -> Result<Self, FileError>`                                                  | The set without the files at `files`, for a program that distrusts them, as after `FileSet::vote`. Files are dropped whole, so a file wrong in one block can't silently feed another. Fails only without memory.                                                                                                                                                                                                     |

#### Recover data and recreate a lost file

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

#### Write recovered data and two repaired files into caller buffers

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

### Member

Requires `alloc`. One file of a set, as it was found.

| Field             | Contract                                              |
| ----------------- | ----------------------------------------------------- |
| `position: usize` | The place of the file among the files given.          |
| `index: u16`      | The index of its shard.                               |
| `survey: Survey`  | Its pieces by their condition.                        |
| `copy: bool`      | Whether another file of the set holds the same shard. |

### Coverage

Requires `alloc`. What a set found among files still holds.

| Field                      | Contract                                                                                                       |
| -------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `shards: usize`            | Different shards among the files.                                                                              |
| `needed: usize`            | Shards that rebuild the set: its threshold.                                                                    |
| `blocks: u64`              | Blocks in every shard of the set.                                                                              |
| `short: u64`               | Blocks held by fewer shards than needed. While there is one, the set does not rebuild.                         |
| `first_short: Option<u64>` | The first such block.                                                                                          |
| `bare: u64`                | Blocks held by exactly as many shards as needed: they rebuild, and nothing is left to compare the result with. |

### Stretch

Requires `alloc`. A run of blocks, one after another, that the same number of shards hold.

| Field                | Contract                                              |
| -------------------- | ----------------------------------------------------- |
| `blocks: Range<u64>` | The blocks: from the first number to before the last. |
| `holders: usize`     | How many shards hold each of them.                    |

### Stretches

Requires `alloc`. Iterator over Stretch values in block order. Each run has the same holder count.

### Dispute

Requires `alloc`. Files of one shard that differ in a block although each has a checksum that holds there. The shard is not used for such a block; nothing says which file is the right one.

| Field               | Contract                                                                                                                                                                         |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `index: u16`        | The index of the shard.                                                                                                                                                          |
| `blocks: u64`       | How many of its blocks the files differ on; `FileSet::disputed_blocks` lists them.                                                                                               |
| `first: u64`        | The first such block.                                                                                                                                                            |
| `files: Vec<usize>` | The places, among the files given, of the files that took part, in rising order: every file that holds one of those blocks with a checksum that holds, whichever it agrees with. |

### Vote

Requires `alloc`. What the files of a set say about each other, block by block: the answer of `FileSet::vote`. It is read and not changed: a program that shows its rows in another order sorts a copy of them, and asks for the blocks of a file by the file's place.

| Signature                                                                   | Contract                                                                                                                                                   |
| --------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `first_undecided(&self) -> Option<u64>`                                     | The first such block.                                                                                                                                      |
| `outvoted(&self) -> &[Outvoted]`                                            | The files that differ from an established block, in the order of their shard indexes and then of their places.                                             |
| `outvoted_blocks(&self, position: usize) -> impl Iterator<Item = u64> + '_` | The blocks in which the file at the place `position` was outvoted, in order. Nothing for a file that was not.                                              |
| `steps(&self) -> u128`                                                      | The steps the call took: at most what `FileSet::vote_work` counts.                                                                                         |
| `undecided(&self) -> u64`                                                   | How many blocks have holders that are not one set and no established word: nothing says which of them is wrong there. `Vote::undecided_blocks` lists them. |
| `undecided_blocks(&self) -> impl Iterator<Item = u64> + '_`                 | The undecided blocks, in order.                                                                                                                            |

#### Identify a conflicting file before rebuilding

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

### Outvoted

Requires `alloc`. A file that differs from the word its set establishes, in blocks that it holds with a checksum that holds.

| Field             | Contract                                                           |
| ----------------- | ------------------------------------------------------------------ |
| `position: usize` | The place of the file among the files given.                       |
| `index: u16`      | The index of its shard.                                            |
| `blocks: u64`     | In how many blocks it differs; `Vote::outvoted_blocks` lists them. |
| `first: u64`      | The first such block.                                              |

### Rebuilt

Requires `alloc`. The data of a set, rebuilt. Its `Debug` text gives the length of the data, not its bytes.

| Field                 | Contract                                                                                                                                                    |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `data: Buffer`        | The data, in a buffer that wipes itself when it is dropped.                                                                                                 |
| `digest: DigestCheck` | Whether the SHA-256 of the data was compared with the one the files state.                                                                                  |
| `bare: u64`           | Blocks that were held by exactly `k` shards: rebuilt with nothing to compare them with. With zero, every block was confirmed by at least one further shard. |
| `steps: u128`         | The steps the call took, as `FileSet::rebuild_work` counts them.                                                                                            |

### Written

Requires `alloc`. What a rebuild into the program's own output found; the data is in that output.

| Field                 | Contract                                                                                |
| --------------------- | --------------------------------------------------------------------------------------- |
| `digest: DigestCheck` | Whether the SHA-256 of the data was compared with the one the files state.              |
| `bare: u64`           | Blocks that were held by exactly `k` shards: rebuilt with nothing to compare them with. |
| `steps: u128`         | The steps the call took, as `FileSet::rebuild_work` counts them.                        |

### DigestCheck

Requires `alloc`. What a rebuild found when it compared the digest of the data. Data whose digest differs from the one the files state is not given back: `SetError::Digest`.

| Variant             | Contract                                                                                                                                                                                                                                                      |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Matched([u8; 32])` | The files state a SHA-256, and the rebuilt data, exactly as long as the files say, has it. The data is the data the files were written for, or someone stated the digest of other data along with it: only a digest trusted from elsewhere tells these apart. |
| `Unchecked`         | The files state no digest: nothing was compared. That the data is the data once written is for the program to check.                                                                                                                                          |

## Handle protected data

Use the root API for public data or ciphertext. For plaintext secrets, initialize protected storage and call the strict module. Its sealed traits accept Buffer, protect::Secret and protect::WorkArea. Check guarded() before filling a caller-owned buffer with a secret. Each new internal buffer is checked before secret bytes enter it. as_portable() exposes the root metadata type. A subsequent root call has the root guarantees. The crate re-exports secret-protection-core as protect and zeroize::Zeroize as Zeroize, so that a program can wipe the buffers it lends to the calls, such as a `Vec<u8>`, with the same version of zeroize: `use reed_solomon_core::Zeroize; shard.zeroize();` empties and overwrites a `Vec<u8>` the program gave as a shard. [Protection API](https://github.com/hobby-eng/secret-protection-core/blob/main/docs/API.md) · [Platform guarantees](TECHNICAL.md#the-strict-mode).

### Buffer

Requires `alloc`. Bytes of data the crate made, wiped when dropped: a shard, a shard file, rebuilt data. With the `protect` feature they lie in pages of their own (the allocator's in WebAssembly) that the system is asked to lock and keep out of dumps and child processes; the methods below say what it did, and a strict call refuses pages that lack what it requires. `Debug` gives the length only, and two buffers are compared without stopping at a difference.

| Signature                              | Contract                                                                                                                                                                                                                                                                                                                           |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `as_mut_slice(&mut self) -> &mut [u8]` | The bytes, to change in place.                                                                                                                                                                                                                                                                                                     |
| `as_slice(&self) -> &[u8]`             | The bytes.                                                                                                                                                                                                                                                                                                                         |
| `is_locked(&self) -> bool`             | Whether the system keeps the buffer's pages in memory, out of swap; an image written when the machine hibernates still holds them. Without the `protect` feature this, like the two answers below, holds for an empty buffer only.                                                                                                 |
| `is_out_of_children(&self) -> bool`    | Whether a child process gets none of the bytes: no mapping of the pages, or zeros in their place. A child of `fork` must leave its parent's buffers alone: there a buffer says it is not protected, dropping it touches nothing, and asking it for its bytes ends the child. A slice taken before the `fork` is not checked again. |
| `is_out_of_dumps(&self) -> bool`       | Whether the system leaves the buffer's pages out of the dumps it writes of the process.                                                                                                                                                                                                                                            |

### strict::Held

Requires `protect`. Memory whose protection the crate can check, which a strict call takes a secret in: a `Buffer` of this crate, and a `Secret` or a `WorkArea` of the protection crate. No other type can be, so ordinary memory such as a `Vec` given to a strict call does not compile.

| Signature                | Contract                                                                                                                                                                      |
| ------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `guarded(&self) -> bool` | Whether the system protects the pages as the strict mode requires on this system: locked, out of child processes and, where it keeps single pages out of dumps, out of dumps. |
| `held(&self) -> &[u8]`   | The bytes.                                                                                                                                                                    |

### strict::HeldMut

Requires `protect`. `Held` memory a strict call can write a secret into.

| Signature                          | Contract                       |
| ---------------------------------- | ------------------------------ |
| `held_mut(&mut self) -> &mut [u8]` | The bytes, to change in place. |

### strict::Code

Requires `protect`. `Code` in the strict mode: shards only in `Held` memory, results and every buffer of the work protected as the mode requires, checked before a byte is written, the work on a protected stack. A refusal is `CodeError::Unprotected`; nothing falls back to other memory.

| Signature                                                                                          | Contract                                                                                                                                                                                                       |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `apply(&self, shards: &mut [(u16, &mut dyn HeldMut)], changes: &Changes) -> Result<(), CodeError>` | Checks the plan fingerprint, shard order, current values and replacement values before applying the list. A stale or mismatched plan returns CodeError::Changes.                                               |
| `correct(&self, shards: &mut [(u16, &mut dyn HeldMut)]) -> Result<Correction, CodeError>`          | Repairs in place within the correction radius. Counts follow input order. With exactly k valid inputs the result is Unchecked. An unresolved column leaves every shard as supplied.                            |
| `correction_memory(&self, shards: usize) -> usize`                                                 | Bound in bytes on the decoder’s working storage for this shard count. Page rounding, guard pages, allocator overhead, the caller’s shards and the strict thread stack affect process memory separately.        |
| `encode(&self, data: &[&dyn Held], targets: &[u16]) -> Result<Vec<Buffer>, CodeError>`             | Takes exactly k equally long data shards in index order (0 through k − 1). Returns one buffer per target in the order requested. Repeated targets are allowed.                                                 |
| `field(&self) -> Field`                                                                            | The field of the symbols.                                                                                                                                                                                      |
| `new(field: Field, threshold: usize) -> Result<Self, CodeError>`                                   | The code of `field` with this threshold, from 1 to the field's number of shards.                                                                                                                               |
| `plan(&self, shards: &[(u16, &dyn Held)], marks: &[Mark]) -> Result<Plan, CodeError>`              | Finds marked erasures and unknown errors. A mark names the position in the supplied slice and the symbol offset. Marked values must still fit the field. Exactly k inputs and any valid marks give Unresolved. |
| `rebuild(&self, shards: &[(u16, &dyn Held)], targets: &[u16]) -> Result<Vec<Buffer>, CodeError>`   | Takes at least k distinct indexed inputs and interpolates from the first k. Extra inputs are checked for valid indexes, lengths and symbols. Use redundancy to test whether they agree. Targets may repeat.    |
| `redundancy(&self, shards: &[(u16, &dyn Held)]) -> Result<Redundancy, CodeError>`                  | Returns None (the Redundancy variant) with exactly k valid inputs. With spare inputs, checks their contents against the first k.                                                                               |
| `sources<'a>(&self, indexes: &'a [u16], weights: &'a mut [u16]) -> Result<Sources<'a>, CodeError>` | Prepare interpolation weights for exactly k distinct indexes using at least k u16 elements. Computing the weights takes k(k − 1) multiplications and k inversions. Reuse them for further targets.             |
| `threshold(&self) -> usize`                                                                        | How many shards rebuild a set.                                                                                                                                                                                 |

### strict::Sources

Requires `protect`. `Sources` in the strict mode: stripes and outputs only in `Held` memory.

| Signature                                                                                          | Contract                                                                                                                                                                                                                                                                                                       |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agrees(&self, target: u16, stripes: &[&dyn Held], shard: &dyn Held) -> Result<bool, CodeError>`   | Compares the supplied target shard with interpolation from the protected source slices. The comparison scratch is allocated in checked protected storage.                                                                                                                                                      |
| `build(&self, target: u16, stripes: &[&dyn Held], out: &mut dyn HeldMut) -> Result<(), CodeError>` | Writes into `out` shard `target` for one stretch of the set, from the same stretch of every source, in the order given to `Code::sources`. A large shard can be made piece by piece. Fails on a bad target, a wrong stripe count, unequal lengths or a symbol outside the field; `out` is then left as it was. |

### strict::CyclicCode

Requires `protect`. `CyclicCode` in the strict mode, as `Code` is for the code by points. A card of words is one word of it: one shard of one symbol for each word.

| Signature                                                                                                   | Contract                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agrees(&self, shards: &[&dyn Held]) -> Result<bool, CodeError>`                                            | Whether every word of `shards`, all `length` of them in their order, is a word of the code: it stops at the first stripe of words that holds one that is not. Fails on another number of shards, unequal lengths, a symbol outside the field, or without memory.                                                                                                                                                                                              |
| `apply(&self, shards: &mut [&mut dyn HeldMut], changes: &Changes) -> Result<(), CodeError>`                 | Makes `changes`, the list `CyclicCode::plan` found, as `Code::apply` does: the shards must be those the plan was made for, in the same order with every byte as it was, and the code this one; otherwise nothing is changed and it fails with `CodeError::Changes`. Fails with `CodeError::Memory` when the list of the shards gets no memory.                                                                                                                |
| `correct(&self, shards: &mut [&mut dyn HeldMut], lost: &[usize]) -> Result<Correction, CodeError>`          | Corrects the words of `shards`, all `length` of them in their order, in place: the shards at the places `lost` are written anew, and wrong symbols elsewhere put right, while `2e + s ≤ parity` with `s` the lost shards. Either every word is put right or nothing changes; neither pass keeps a list. Fails on another number of shards, unequal lengths, a lost place outside the word, a symbol of a shard not lost outside the field, or without memory. |
| `data(&self) -> usize`                                                                                      | The number of data shards, which come first.                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `encode(&self, data: &[&dyn Held]) -> Result<Vec<Buffer>, CodeError>`                                       | The `parity` shards of `data`, in their order: exactly `data` shards of one length, a whole number of symbols, each an element of the field. Counts and lengths are refused before symbols, as in `Code`.                                                                                                                                                                                                                                                     |
| `field(&self) -> Field`                                                                                     | The field of the symbols.                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `generator(&self) -> &[u16]`                                                                                | The generator, its coefficients from x^parity, which is 1, down to x^0.                                                                                                                                                                                                                                                                                                                                                                                       |
| `length(&self) -> usize`                                                                                    | The number of shards of a word: data and parity.                                                                                                                                                                                                                                                                                                                                                                                                              |
| `new(field: Field, data: usize, parity: usize, first_root: u32, root_step: u32) -> Result<Self, CodeError>` | Requires positive data and parity counts, distinct root positions and a word no longer than the order of β = α^root_step. The composite Gf65536 representation is unsupported. Returns CodeError::Shape for an unsupported shape.                                                                                                                                                                                                                             |
| `parity(&self) -> usize`                                                                                    | The number of parity shards, which follow the data.                                                                                                                                                                                                                                                                                                                                                                                                           |
| `plan(&self, shards: &[&dyn Held], lost: &[usize], marks: &[Mark]) -> Result<Plan, CodeError>`              | The changes that would put the words of `shards` right, with the shards at the places `lost` and the symbols `marks` unreadable; nothing is changed until `CyclicCode::apply`. Every lost and marked symbol is listed, as `ChangeKind::Filled`, so the list grows with them. Fails as `CyclicCode::correct` does, or with `CodeError::Mark` for a mark outside the shards; never `Plan::Unchecked`.                                                           |

#### Repair a card in protected memory

```rust
use reed_solomon_core::protect::Secret;
use reed_solomon_core::strict::{self, CyclicCode, Held, HeldMut};
use reed_solomon_core::{Field, Mark, Plan};

// The system may protect less than it is asked to: memory is checked before a byte goes in.
let protected = |length| match Secret::with_capacity(length) {
    secret if Held::guarded(&secret) => Ok(secret),
    _ => Err("the system does not protect this memory"),
};
// Each word of a card in protected memory of its own, as a program reads it in; the numbers
// stand in the open here only to keep the example short.
let code = CyclicCode::new(Field::Gf2048, 12, 4, 1, 1)?;
let card: [u16; 12] = [
    0x371, 0x6CE, 0x220, 0x11D, 0x2DB, 0x73E, 0x12B, 0x7C6, 0x7D5, 0x7AF, 0x6A9, 0x453,
];
let mut words = Vec::new();
for word in card {
    let mut held = protected(2)?;
    held.push(&word.to_be_bytes())?;
    words.push(held);
}
let data: Vec<&dyn Held> = words.iter().map(|word| word as &dyn Held).collect();
let mut repair = code.encode(&data)?;
assert!(repair.iter().all(|word| word.is_locked()));

// Word 3 is smudged and marked. The plan keeps what it read and what it writes in protected
// pages and fits only these words; `strict::values` hands a value over on the protected
// stack, here straight into protected memory of the program's.
words[2].as_mut_bytes().fill(0);
let mut read: Vec<&dyn Held> = words.iter().map(|word| word as &dyn Held).collect();
read.extend(repair.iter().map(|word| word as &dyn Held));
let Plan::Changes(changes) = code.plan(&read, &[], &[Mark { shard: 2, symbol: 0 }])? else {
    return Err("the card is within 2e + s ≤ 4".into());
};
let mut shown = protected(2)?;
let pushed = strict::values(&changes, 0, |_, becomes| shown.push(&becomes.to_be_bytes()))?;
assert!(matches!(pushed, Some(Ok(()))) && shown.as_bytes() == 0x220_u16.to_be_bytes());
let mut shards: Vec<&mut dyn HeldMut> = words.iter_mut().map(|w| w as &mut dyn HeldMut).collect();
shards.extend(repair.iter_mut().map(|w| w as &mut dyn HeldMut));
code.apply(&mut shards, &changes)?;
assert_eq!(words[2].as_bytes(), 0x220_u16.to_be_bytes());
```

### strict::values

Requires `protect`. What the change at place `at` of a plan held and becomes, given to `look` on the stack of a strict call, so that a program can move them into protected memory of its own with no copy on its stack; `None` past the last change. The plan must lie in protected pages.

`values<R: Send>(changes: &Changes, at: usize, look: impl FnOnce(u16, u16) -> R + Send) -> Result<Option<R>, CodeError>`

### strict::SetFacts

Requires `protect`. `SetFacts` in the strict mode, for writing the files of a set: the data only in `Held` memory, the files in buffers protected as the mode requires.

| Signature                                                                                                                             | Contract                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `as_portable(&self) -> &crate::SetFacts`                                                                                              | The same facts for the calls of the root types, which take any memory: an explicit step out of the strict mode, for public data.                                                                                                                                                                                                                                                                                                                                                      |
| `block_len(&self) -> u32`                                                                                                             | The length of a block in bytes; the last block of a shard may be shorter.                                                                                                                                                                                                                                                                                                                                                                                                             |
| `blocks(&self) -> u64`                                                                                                                | How many blocks every shard of the set has: none for empty data, at most 2^32.                                                                                                                                                                                                                                                                                                                                                                                                        |
| `code(&self) -> strict::Code`                                                                                                         | The code of the set, in the strict mode.                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `data_len(&self) -> u64`                                                                                                              | The length of the data in bytes.                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `digest(&self) -> Option<&[u8; 32]>`                                                                                                  | The SHA-256 of the data that the files of the set state, if they state one. It says whether the data came back unchanged, not who wrote the set: whoever changes the data can state its new digest. Only a digest the program trusts from elsewhere tells that.                                                                                                                                                                                                                       |
| `file_len(&self) -> u64`                                                                                                              | The length of every shard file of the set in bytes.                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `files(&self, data: &dyn Held, indexes: &[u16]) -> Result<Vec<Buffer>, FileError>`                                                    | The shard files numbered `indexes` for the set whose data is `data`, in the order asked. The data is cut into the `k` data shards (the last padded with zeros), so files 0 to `k − 1` hold the data itself. Copies made along the way are wiped, and each file comes in a `Buffer`. If the facts state a SHA-256, data with another digest gets no files. Fails on wrong data length or digest, an index outside the field, or no memory. `SetFacts::files_into` writes into outputs. |
| `files_into(&self, data: &dyn Held, indexes: &[u16], outs: &mut [&mut dyn HeldMut]) -> Result<(), FileError>`                         | Writes one complete shard file per requested index into an exactly sized buffer. The writer verifies data length and any declared digest. Failures before writing preserve outputs. A later failure zeroes all outputs.                                                                                                                                                                                                                                                               |
| `for_data(field: Field, id: &[u8], threshold: usize, data: &dyn Held, block_len: u32, digest: DataDigest) -> Result<Self, FileError>` | The facts of the set whose data is `data`, with its length and, for `DataDigest::Sha256`, its digest computed from `data`. Fails as `SetFacts::new` does, before the digest is computed.                                                                                                                                                                                                                                                                                              |
| `header(&self, index: u16) -> Result<Header, FileError>`                                                                              | The header of shard `index`, as `Header::new` makes it.                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `id(&self) -> &[u8]`                                                                                                                  | The identifier of the set.                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `new(field: Field, id: &[u8], threshold: usize, data_len: u64, block_len: u32, digest: Option<[u8; 32]>) -> Result<Self, FileError>`  | The facts of a set: field, threshold, an identifier of 1 to 32 bytes, data length, block length and optionally the data's SHA-256. A block is the unit of damage: a wrong byte loses its block. A program that has the data should use `SetFacts::for_data`; this is for facts read from a header. Fails for a field other than Gf256 and Gf65536, a bad identifier, threshold or block length, or files that would be too large.                                                     |
| `shard_len(&self) -> u64`                                                                                                             | The length of every shard of the set in bytes: the data length divided by the threshold, rounded up to a whole number of symbols.                                                                                                                                                                                                                                                                                                                                                     |

### strict::Header

Requires `protect`. `Header` in the strict mode: the file of one shard in `Held` memory.

| Signature                                                                            | Contract                                                                                                                                                                                                          |
| ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `as_bytes(&self) -> &[u8]`                                                           | The bytes of the header: a shard file starts with them and holds them again before its trailer.                                                                                                                   |
| `as_portable(&self) -> &crate::Header`                                               | The same header for the calls of the root types, which take any memory: an explicit step out of the strict mode, for public data.                                                                                 |
| `file(&self, shard: &dyn Held) -> Result<Buffer, FileError>`                         | The whole shard file for `shard`: header, blocks with checksums, the header again and the trailer, in a `Buffer`. Fails with `FileError::Shard` for a wrong length or `FileError::Memory` when there's no memory. |
| `file_into(&self, shard: &dyn Held, out: &mut dyn HeldMut) -> Result<(), FileError>` | Checks the protection of the input and destination before validating their lengths. Writes one complete file into the protected destination. Ordinary file mappings cannot be supplied as HeldMut.                |
| `index(&self) -> u16`                                                                | The index of the file's shard in its set.                                                                                                                                                                         |
| `set(&self) -> SetFacts`                                                             | The facts of the set, in the strict mode.                                                                                                                                                                         |

### strict::read

Requires `protect`. `ShardFile::read` of a file in `Held` memory: what it gives refers to that memory.

`read(file: &dyn Held) -> Result<ShardFile<'_>, FileError>`

### strict::ShardFile

Requires `protect`. `ShardFile` in the strict mode: a file read from `Held` memory, whose blocks are checked on a protected stack as well, each time they are asked for.

| Signature                                                           | Contract                                                                                                                                                                                              |
| ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `block(&self, number: u32) -> Result<Option<Block<'a>>, FileError>` | Block `number` of the shard, its checksum checked now; `None` when the shard has no such block. The bytes come from the block's own place between the two copies of the header and from nowhere else. |
| `frame(&self) -> Frame`                                             | The condition of the two copies of the header, of the trailer and of the file's length.                                                                                                               |
| `header(&self) -> Header`                                           | The header of the file.                                                                                                                                                                               |
| `survey(&self) -> Result<Survey, FileError>`                        | Checks every block that is wholly there and counts the pieces of the file by their condition: one pass over the bytes given.                                                                          |

### strict::Pile

Requires `protect`. `Pile` in the strict mode: files only in `Held` memory, read on a protected stack, sorted into sets that work in the strict mode.

| Signature                                                | Contract                                                                                                                                                                    |
| -------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `new(files: &[&'a dyn Held]) -> Result<Self, FileError>` | Checks the protection of every input and reads on a protected stack. Inspect readings() for per-file parse errors. Memory or Unprotected can fail construction of the pile. |
| `readings(&self) -> &[Result<ShardFile<'a>, FileError>]` | What became of each file, in the order given: the file as read, or why it does not read.                                                                                    |
| `sets(&self) -> &[FileSet<'a>]`                          | The sets, as `Pile::sets` has them.                                                                                                                                         |

### strict::FileSet

Requires `protect`. `FileSet` in the strict mode: the data and the files it makes in buffers protected as the mode requires, or in `Held` memory the program gives, never in a mapping of a file.

| Signature                                                                                                            | Contract                                                                                                                                                                                                                                                                                                                                                                                                             |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `coverage(&self) -> Coverage`                                                                                        | How many shards hold the blocks of the set.                                                                                                                                                                                                                                                                                                                                                                          |
| `disputed_blocks(&self, index: u16) -> impl Iterator<Item = u64> + '_`                                               | The blocks of shard `index` on which its files differ, in order: every one of them, where `FileSet::disputes` has their number and the first. Nothing for a shard without a dispute.                                                                                                                                                                                                                                 |
| `disputes(&self) -> &[Dispute]`                                                                                      | The shards whose files differ in a block that each holds intact.                                                                                                                                                                                                                                                                                                                                                     |
| `facts(&self) -> SetFacts`                                                                                           | The facts every file of the set states.                                                                                                                                                                                                                                                                                                                                                                              |
| `id_shared(&self) -> bool`                                                                                           | Whether another set among the files has the same identifier and other facts. Two sets never share an identifier by the rules, so one of them is not what it says; nothing here tells which, and each is rebuilt only when asked for by itself.                                                                                                                                                                       |
| `members(&self) -> &[Member]`                                                                                        | The files of the set in the order of their shard indexes, each as it was found.                                                                                                                                                                                                                                                                                                                                      |
| `rebuild(&self, limit: Option<u128>) -> Result<Rebuilt, SetError>`                                                   | Rebuilds into protected storage and verifies padding and any declared digest. On a mismatch, DigestDiffers carries only the stated digest.                                                                                                                                                                                                                                                                           |
| `rebuild_into(&self, out: &mut dyn HeldMut, limit: Option<u128>) -> Result<Written, SetError>`                       | Writes exactly data_len() bytes into the protected destination. Early failures preserve it. Late failures zero it. DigestDiffers exposes only the stated digest.                                                                                                                                                                                                                                                     |
| `rebuild_work(&self) -> Result<u128, SetError>`                                                                      | The steps `FileSet::rebuild` needs, to set its limit by. A step counts the work of making shards only, not time, reading or memory. The count says what the call costs, not that it will succeed. Fails when the files can't rebuild the set, or without memory.                                                                                                                                                     |
| `repair(&self, index: u16, limit: Option<u128>) -> Result<Buffer, SetError>`                                         | The file of shard `index`, made from the files at hand: a lost file byte for byte, or a new one. Spare shards are still compared, but the data isn't assembled or its digest checked; a later rebuild checks that. Fails as `FileSet::rebuild`, or for an index outside the field.                                                                                                                                   |
| `repair_work(&self, index: u16) -> Result<u128, SetError>`                                                           | The steps `FileSet::repair` needs for shard `index`, counted as `FileSet::rebuild_work` counts. Fails for an index outside the field first, otherwise as that call.                                                                                                                                                                                                                                                  |
| `repairs(&self, indexes: &[u16], limit: Option<u128>) -> Result<Vec<Buffer>, SetError>`                              | The files of shards `indexes`, in the order asked, made in one pass: each block is read and compared once for all of them. A repeated index gives the same file twice. Fails as `FileSet::repair`. `FileSet::repairs_into` writes the files into the program's own outputs instead.                                                                                                                                  |
| `repairs_into(&self, indexes: &[u16], outs: &mut [&mut dyn HeldMut], limit: Option<u128>) -> Result<u128, SetError>` | Writes exactly file_len() bytes per requested index in output order. Repeated targets are allowed. Early validation failures preserve all outputs and late failures zero them all.                                                                                                                                                                                                                                   |
| `repairs_work(&self, indexes: &[u16]) -> Result<u128, SetError>`                                                     | The steps `FileSet::repairs` needs for `indexes`. All of them are made in one pass, so each block's sources are prepared once; a repeated index counts once.                                                                                                                                                                                                                                                         |
| `stretches(&self) -> Result<Stretches<'_, 'a>, FileError>`                                                           | The blocks of the set in stretches held by the same number of shards: what rebuilds, what spare shards confirm, what is lost. Fails only without memory.                                                                                                                                                                                                                                                             |
| `vote(&self, limit: Option<u128>) -> Result<Vote, SetError>`                                                         | Identifies files contradicted by an established block. A disagreement among holders can remain undecided. When holders agree but disputed copies give insufficient support, the block can produce neither an outvoted file nor an undecided entry.                                                                                                                                                                   |
| `vote_work(&self) -> Result<u128, SetError>`                                                                         | An upper bound on the steps `FileSet::vote` can take, to set its limit by. It assumes every column disagrees and the most searches per block; reading files and asking for memory are not counted, and a change in the number of holders from one block to the next makes a new search with two buffers of its own, which with `protect` lie in pages of their own. Fails only when there's no memory for the count. |
| `without(&self, files: &[usize]) -> Result<Self, FileError>`                                                         | Excludes files by their original positions and recomputes disputes. Can return FileError::Memory or FileError::Unprotected.                                                                                                                                                                                                                                                                                          |

## Inspect errors

The four error enums implement Debug, Display and core::error::Error. Match variants to decide whether to supply another shard, resize a buffer, lower a work request or report an unsupported input.

### FieldError

Why a field was not chosen.

| Variant      | Contract                                                                                                                                                                     |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Degree`     | A degree outside 2 to 16.                                                                                                                                                    |
| `Polynomial` | A polynomial that is not a primitive polynomial of the degree: another highest term, no constant term, or a polynomial whose powers of x do not reach every nonzero element. |

### CodeError

Why a request was refused. Nothing was written then; lent working memory may hold anything. Counts, indexes and lengths are reported before a symbol outside the field.

| Variant       | Contract                                                                                                                                                                                                           |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `Threshold`   | The threshold is zero or above the number of shards the field allows.                                                                                                                                              |
| `Index`       | A shard index the field does not have.                                                                                                                                                                             |
| `Repeated`    | The same shard index twice.                                                                                                                                                                                        |
| `Count`       | Wrong input count. Encoding and prepared sources require exactly k inputs. Rebuild requires at least k.                                                                                                            |
| `Length`      | Shard or symbol lengths do not match, or a cyclic word or its working slice has the wrong size.                                                                                                                    |
| `Buffer`      | A working buffer shorter than the threshold, or a room shorter than `COMPARED`.                                                                                                                                    |
| `Symbol`      | An input symbol lies outside the selected field, for example 2048 in Gf2048. Single marks require valid placeholders. Whole cyclic erasures and erased positions in correct_word permit out-of-field placeholders. |
| `Mark`        | A mark lies outside the input, or the erased positions passed to CyclicWords::correct_word repeat or are out of order.                                                                                             |
| `Changes`     | A change that does not fit the shards: a place outside them, a symbol that no longer holds what the change says was there, or a new value that is not an element of the field.                                     |
| `Memory`      | The system gave no memory for a shard to return, for a search, for a list of changes or, with the `protect` feature, for the bytes `Sources::agrees` compares.                                                     |
| `Shape`       | Unsupported field or cyclic parameters, including zero data or parity and a word longer than the order of β = α^root_step.                                                                                         |
| `Unprotected` | In the strict mode, memory a secret would pass through is not protected as that mode requires: given by the caller, or made by the system for the call. Nothing was written.                                       |

### FileError

Why a shard file was not described, written or read.

| Variant                   | Contract                                                                                                                                                                                                                                                    |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Field`                   | Version 1 supports Gf256 and the composite Gf65536. Gf2048 and the Binary variant have no file encoding. The standard degree-8 polynomial selects Gf256.                                                                                                    |
| `Id`                      | A set identifier that is empty or longer than 32 bytes.                                                                                                                                                                                                     |
| `Threshold`               | A threshold that is zero or above the number of shards the field allows.                                                                                                                                                                                    |
| `Index`                   | A shard index the field does not have.                                                                                                                                                                                                                      |
| `Block`                   | A block length of zero, or one that is not a whole number of symbols.                                                                                                                                                                                       |
| `Blocks`                  | A shard would have more than 2^32 blocks: the set needs longer blocks.                                                                                                                                                                                      |
| `Size`                    | A file would be longer than 2^64 − 1 bytes, or data of 2^61 bytes or more would state a SHA-256, which takes fewer than 2^64 bits.                                                                                                                          |
| `Shard`                   | The bytes given for a shard are not as long as the set's shards are.                                                                                                                                                                                        |
| `Data`                    | The data given is not as long as the facts of the set say.                                                                                                                                                                                                  |
| `Digest`                  | The data given does not have the SHA-256 that the facts of the set state.                                                                                                                                                                                   |
| `Memory`                  | This machine has no memory for the file: it is longer than the machine addresses, or the memory was not given. The format allows the file. A call that returns a buffer wrote nothing; one that writes in place says what its outputs hold after a refusal. |
| `Unprotected`             | In the strict mode, memory the file or its data would pass through is not protected as that mode requires. A call refused before its work wrote nothing.                                                                                                    |
| `Unrecognized`            | The bytes have no mark of a shard file, at the start or at the end.                                                                                                                                                                                         |
| `Damaged`                 | The marks of a shard file are there, and neither copy of the header passes its checks.                                                                                                                                                                      |
| `Output { needed: u64, }` | The outputs given to be written in place are not one for each file, each as long as a file of the set.                                                                                                                                                      |
| `Version(u8)`             | Header magic is followed by an unrecognized version. The reader does not interpret that version’s CRC layout. A sound version-1 copy can still allow recovery.                                                                                              |
| `Unsupported`             | No header holds, and one whose checksum holds names a field or a flag this version does not read.                                                                                                                                                           |
| `Conflict`                | Two independently credible header positions disagree, or the expected duplicate is different and classified as sound or unsupported.                                                                                                                        |

### SetError

Requires `alloc`. Why a set was not rebuilt, or a file of it not made.

| Variant                                                             | Contract                                                                                                                                                                                                                    |
| ------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Shards { found: usize, needed: usize, }`                           | Fewer different shards among the files than the set needs, `k`. This holds for a set of no data as well. With `k` = 1 every file holds the whole data, and one file is a set.                                               |
| `Blocks { short: u64, first: u64, }`                                | Some blocks are held by fewer shards than the set needs.                                                                                                                                                                    |
| `Inconsistent { block: u64, base: Vec<u16>, differing: Vec<u16>, }` | An extra holder disagrees with interpolation from the listed sources at that block. Either side can contain a wrong shard. Use vote for evidence about individual files.                                                    |
| `Padding`                                                           | The bytes after the end of the rebuilt data are not zero: the set was not written by the rules of the format.                                                                                                               |
| `Digest { stated: [u8; 32], found: [u8; 32], }`                     | The files agree on data whose SHA-256 is not the one they state. The data is not given back. Either the data or the stated digest was changed after writing, in every file alike, or the files were written for other data. |
| `DigestDiffers { stated: [u8; 32], }`                               | In the strict mode, as `SetError::Digest`: the files agree on data whose SHA-256 is not the one they state. The digest the data has follows from it, so it is not given.                                                    |
| `Work { needed: u128, }`                                            | The work is above the limit given. Nothing was computed.                                                                                                                                                                    |
| `Memory`                                                            | This machine has no memory for the result or for a list the call keeps.                                                                                                                                                     |
| `Unprotected`                                                       | In the strict mode, memory the data or a file would pass through is not protected as that mode requires.                                                                                                                    |
| `Index`                                                             | The field has no shard with the index asked for.                                                                                                                                                                            |
| `Code(CodeError)`                                                   | The code refused shards that had passed the checks of their files. That is a fault of this crate, not of the files.                                                                                                         |
| `Output { needed: u64, }`                                           | The outputs given to be written in place are not one for each result, each as long as the result.                                                                                                                           |

## Run the self-test

### self_test

Runs the known answers: a few field multiplications' and hashes' worth of work. Without the `alloc` feature it allocates nothing and leaves out the three checks of the error correction, the plan of changes for a card, the vote among the files of a set and a digest that differs.

`self_test() -> SelfTest`

### SelfTest

The checks of one run.

| Signature                   | Contract                                                             |
| --------------------------- | -------------------------------------------------------------------- |
| `checks(&self) -> &[Check]` | Every check, in the order run.                                       |
| `passed(&self) -> bool`     | Whether every check passed. A program relies on the crate only then. |

### Check

One check: what was checked against which source, and whether the answer was the expected one.

| Field                | Contract                                                   |
| -------------------- | ---------------------------------------------------------- |
| `name: &'static str` | What was checked and where the expected answer comes from. |
| `passed: bool`       | Whether the answer was the expected one.                   |
