# reed-solomon-core

[![CI](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/ci.yml)
[![RustSec audit](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/audit.yml/badge.svg)](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/audit.yml)
[![Build provenance: GitHub attestations](https://img.shields.io/badge/build%20provenance-GitHub%20attestations-2ea44f)](https://github.com/hobby-eng/reed-solomon-core/attestations)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

reed-solomon-core is a Rust library for recovering data when pieces of a backup go missing or arrive damaged. It works with numbered shards spread across several drives and with short records that people copy by hand. The decoder can also locate a wrong symbol when the reader has no way to tell which one was copied incorrectly.

[Documentation index](docs/README.md)

| Section                                                               | Description                                                                       |
| --------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| [Recovery](#recovery)                                                 | Choose a threshold for missing shards and incorrect symbols.                      |
| [Shard files](#shard-files)                                           | Recover a backup when different parts of its files are damaged.                   |
| [Printed records and repair words](#printed-records-and-repair-words) | Add check words to a record that someone will copy by hand.                       |
| [Secrets in memory](#secrets-in-memory)                               | Choose the memory protections for plaintext and see which platforms support them. |
| [Verification](#verification)                                         | Find the measured coverage and the built-in self-test.                            |
| [License](#license)                                                   | Check the license options and dependency notices.                                 |
| [Security reviews](#security-reviews)                                 | Read the findings and follow-up checks.                                           |

## Recovery

A backup starts with `k` equal-sized data shards. You choose how many extra shards to write. Any `k` intact shards with distinct indexes recover the original. You can issue another shard later from any surviving set of `k`, even if the machine that wrote the backup is gone.

| Backup layout                     | What you can recover                                                                                                                                   |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 2 data shards, 1 extra (2 of 3)   | Any two intact shards rebuild the third. If all three are present and disagree, one extra detects the discrepancy but cannot identify the wrong value. |
| 3 data shards, no extras (3 of 3) | You need all three. The code has no spare information to repair a loss or check a symbol.                                                              |
| 3 data shards, 2 extras (3 of 5)  | Any three intact shards suffice. With all five present, the decoder can locate and repair one incorrect symbol at each position.                       |

A known gap uses one spare symbol. An error at an unknown location uses two. The decoder applies that budget independently at each position across the shards, so damage at different offsets can be repaired even when several files are affected. Beyond the correction bound, the decoder can return a different valid result. If you need to verify which original was intended, keep a trusted digest or authentication tag.

## Shard files

Suppose you have three files from a 2-of-3 backup. The beginning of one file is unreadable, the middle of another was overwritten and the third is truncated. Recovery can still succeed if **each block** survives at two distinct shard indexes. The reader checks blocks with CRC-32C and can use a different pair of files for each block. A second header at the end lets it identify a file whose first header was damaged.

You can inspect which blocks have enough copies before rebuilding. Where readable copies disagree, the library can report which files the remaining evidence contradicts and which blocks are still undecided. An optional SHA-256 in the header checks the reconstructed data for accidental corruption. Someone able to replace a backup can also replace its stored hash.

## Printed records and repair words

The cyclic code appends check symbols to a fixed-length record. In the 2,048-element field, each symbol can be an index into a 2,048-word list. For example, four repair words can recover four words left blank, two words copied incorrectly, or two blanks and one incorrect word. An application can show the proposed changes before applying them. This is the construction used in MHFE repair cards. The library also supports the byte-field parameters used for QR error correction.

## Secrets in memory

The original data shards contain plaintext. Encrypt confidential data before distributing its shards. The strict API uses [secret-protection-core](https://github.com/hobby-eng/secret-protection-core) to keep plaintext in protected buffers and run the decoder on a protected stack. It checks the caller’s buffers and refuses the operation if the OS cannot supply the required protection. The strict stack is available on Linux, Android, macOS and iOS. [Security policy and platform limits](SECURITY.md).

The crate requires Rust 1.99 or later. The default `protect` feature requests OS-backed buffers on supported native platforms. On WebAssembly it uses allocator memory. With `alloc` alone, owned buffers use ordinary allocator memory and are wiped on drop. With default features disabled and no allocator, the core operations use buffers supplied by the caller.

## Verification

Version 0.1.0 covered **98.23%** of measured source lines on Linux x86-64 with default features, across unit, public-interface and strict-mode tests. The crate also includes a known-answer self-test. [Test coverage and measurement](docs/COVERAGE.md).

## License

MIT or Apache-2.0, at your option. [Third-party notices](THIRD_PARTY_NOTICES.md).

## Security reviews

AI reviewers cross-checked the implementation and its fixes across five documented reviews. An independent human security audit is still outstanding. [Audit records](docs/audits/README.md).
