# reed-solomon-core

[![CI](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/ci.yml)
[![RustSec audit](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/audit.yml/badge.svg)](https://github.com/hobby-eng/reed-solomon-core/actions/workflows/audit.yml)
[![Build provenance: GitHub attestations](https://img.shields.io/badge/build%20provenance-GitHub%20attestations-2ea44f)](https://github.com/hobby-eng/reed-solomon-core/attestations)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](#license)

reed-solomon-core is a Rust library for recovering data from missing shards and correcting damaged symbols. It supports backup files spread across several media and short printed records where a word can be lost or misread.

[Documentation index](docs/README.md)

[Recovery](#recovery) · [Shard files](#shard-files) · [Secrets in memory](#secrets-in-memory) · [Verification](#verification) · [License](#license)

## Recovery

Choose a threshold and the number of shards to distribute. A 2-of-3 set survives the loss of any one shard. A 3-of-3 set needs all three. Only distinct shard indexes count. Locating and correcting one wrong symbol at an unknown position requires two spare shards.

The point code lets you add shards later using any threshold-sized set of intact shards. The cyclic code adds a fixed number of check symbols to a record, as used in QR codes and MHFE repair cards. Both can show a proposed correction before applying it.

## Shard files

Each file carries a copy of its header at both ends and a checksum for each block. Recovery can use different shards for different blocks, so a set can survive even when every file has some damage. An optional SHA-256 checks the recovered data against the digest recorded in the set.

## Secrets in memory

Reed–Solomon coding provides redundancy. The original data shards contain plaintext. For plaintext secrets, the strict API requires protected buffers and a protected stack. It refuses secret-bearing operations when the operating system cannot supply that protection. Linux, Android, macOS and iOS support the strict stack.

The crate requires Rust 1.99 or later. Applications that supply their own buffers can build the core without an allocator.

## Verification

Version 0.1.0 covered **98.23%** of measured source lines on Linux x86-64 with default features, across unit, public-interface and strict-mode tests. The crate also includes a known-answer self-test. [Test coverage and measurement](docs/COVERAGE.md).

Security reviews to date were AI-assisted. An independent human security audit is still outstanding. [Audit records](docs/audits/README.md).

## License

MIT or Apache-2.0, at your option. [Third-party notices](THIRD_PARTY_NOTICES.md).
