# Test coverage

On 2026-11-15 UTC the tests covered **98.35%** of the source lines that LLVM reports for the Linux x86-64 build with the default features (3,985 of 4,052). The run passed 44 unit tests and 126 tests of the public interface (`tests/suite`).

## Measurement

Rust 1.99.1 and cargo-llvm-cov 0.9.1 measured the Linux build on kernel 7.0.0-43-generic, x86-64, with the `protect` feature, which is the default: the buffers lie in protected pages. The measured source is the working tree that follows commit `4afe60775c150c34a4631d71ac50bab2a4f3a6b7`; the SHA-256 fingerprint of its inputs is `5f5a9adb05d4c2cf31f95b4a7975286c3b65cc5a3239194d35d218338d6e5fd4`.

What the figure leaves out:

- The build without `protect`, whose buffers lie in the allocator's memory (`src/buffer/heap.rs`). The same tests pass on it at every push; its lines are not measured.
- The systems other than Linux for x86-64. The tests run on Linux for AArch64, macOS and Windows at every push, without a measurement of their lines.
- Files under `tests/`, and the fuzz targets of the audits, which are no part of the crate.

## Reproduce on Linux

From the repository root, with the pinned toolchain. The workspace laptop runs each Cargo command through `run-capped --`.

```sh
cargo llvm-cov --summary-only
```

For the source fingerprint, hash each Rust file under `src/` and `tests/`, plus `Cargo.toml`, `Cargo.lock` and `rust-toolchain.toml`. Sort by repository-relative path in byte order. Hash the resulting manifest of lowercase SHA-256 digests, two spaces, path and a line feed per file.

## Coverage by source file

| File                     | Covered lines | Measured lines | Coverage |
| ------------------------ | ------------: | -------------: | -------: |
| `src/buffer.rs`          |            68 |             68 |  100.00% |
| `src/buffer/pages.rs`    |            35 |             35 |  100.00% |
| `src/code.rs`            |           123 |            136 |   90.44% |
| `src/code/owned.rs`      |            92 |             93 |   98.92% |
| `src/correct.rs`         |           638 |            640 |   99.69% |
| `src/correct/changes.rs` |           161 |            162 |   99.38% |
| `src/correct/plan.rs`    |           231 |            233 |   99.14% |
| `src/field.rs`           |           221 |            221 |  100.00% |
| `src/field/binary.rs`    |            63 |             69 |   91.30% |
| `src/field/gf2048.rs`    |            70 |             70 |  100.00% |
| `src/field/gf256.rs`     |            58 |             58 |  100.00% |
| `src/field/gf65536.rs`   |            64 |             64 |  100.00% |
| `src/field/lanes.rs`     |           116 |            116 |  100.00% |
| `src/file.rs`            |           113 |            117 |   96.58% |
| `src/file/crc32c.rs`     |            28 |             28 |  100.00% |
| `src/file/facts.rs`      |           146 |            146 |  100.00% |
| `src/file/header.rs`     |           242 |            243 |   99.59% |
| `src/file/pile.rs`       |            43 |             44 |   97.73% |
| `src/file/reading.rs`    |           139 |            141 |   98.58% |
| `src/file/set.rs`        |           341 |            360 |   94.72% |
| `src/file/set/shards.rs` |           392 |            398 |   98.49% |
| `src/file/set/vote.rs`   |           328 |            331 |   99.09% |
| `src/file/writing.rs`    |            74 |             75 |   98.67% |
| `src/self_test.rs`       |           199 |            204 |   97.55% |
