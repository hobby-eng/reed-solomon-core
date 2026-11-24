# Test coverage

On 2026-11-24 UTC the tests covered **98.25%** of the source lines that LLVM reports for the Linux x86-64 build with the default features (6,049 of 6,157). The run passed every test the build has: 48 unit tests, 157 tests of the public interface (`tests/suite`) and 9 tests of the strict mode (`tests/strict`), which runs in a test process of its own.

## Measurement

Rust 1.99.1 and cargo-llvm-cov 0.9.1 measured the Linux build on kernel 7.0.0-43-generic, x86-64, with the `protect` feature, which is the default: the buffers lie in protected pages. The measured source is version 0.1.1 as committed after commit `3a42c0b5c51ba7867443a2b1230b99a160e4bf57`; the SHA-256 fingerprint of its inputs is `a04f468741a863965d7421adc64912173f95a798ddabe83c5283c8dd3294ad38`.

What the figure leaves out:

- The build without `protect`, whose buffers lie in the allocator's memory (`src/buffer/heap.rs`). The same tests pass on it at every push; its lines are not measured.
- The systems other than Linux for x86-64. The tests run on Linux for AArch64, macOS and Windows at every push, without a measurement of their lines.
- The large fields of the field proof, which run before a release with `--ignored`; doctests; files under `tests/`; and the fuzz targets.

Each audit under [audits](https://github.com/hobby-eng/reed-solomon-core/blob/main/docs/audits/README.md) records the coverage of the tree it reviewed.

## Reproduce on Linux

From the repository root, with the pinned toolchain.

```sh
cargo llvm-cov --locked --json --summary-only --output-path coverage.json -- --test-threads=4
```

The figure sums the lines of the files under `src/`. For the source fingerprint, hash each Rust file under `src/` and `tests/`, plus `Cargo.toml`, `Cargo.lock` and `rust-toolchain.toml`. Sort by repository-relative path in byte order. Hash the resulting manifest of lowercase SHA-256 digests, two spaces, path and a line feed per file.

## Coverage by source file

| File                     | Covered lines | Measured lines | Coverage |
| ------------------------ | ------------: | -------------: | -------: |
| `src/buffer.rs`          |            76 |             76 |  100.00% |
| `src/buffer/numbers.rs`  |            34 |             35 |   97.14% |
| `src/buffer/pages.rs`    |            30 |             30 |  100.00% |
| `src/code.rs`            |           181 |            193 |   93.78% |
| `src/code/owned.rs`      |            98 |             99 |   98.99% |
| `src/correct.rs`         |           695 |            697 |   99.71% |
| `src/correct/changes.rs` |           200 |            200 |  100.00% |
| `src/correct/errors.rs`  |            27 |             27 |  100.00% |
| `src/correct/plan.rs`    |           289 |            292 |   98.97% |
| `src/cyclic.rs`          |            10 |             10 |  100.00% |
| `src/cyclic/decode.rs`   |           235 |            237 |   99.16% |
| `src/cyclic/owned.rs`    |           294 |            297 |   98.99% |
| `src/cyclic/words.rs`    |           182 |            183 |   99.45% |
| `src/field.rs`           |           230 |            230 |  100.00% |
| `src/field/binary.rs`    |            63 |             69 |   91.30% |
| `src/field/gf2048.rs`    |            70 |             70 |  100.00% |
| `src/field/gf256.rs`     |            58 |             58 |  100.00% |
| `src/field/gf65536.rs`   |            64 |             64 |  100.00% |
| `src/field/lanes.rs`     |           118 |            118 |  100.00% |
| `src/field/proof.rs`     |           106 |            120 |   88.33% |
| `src/file.rs`            |           106 |            115 |   92.17% |
| `src/file/crc32c.rs`     |            28 |             28 |  100.00% |
| `src/file/facts.rs`      |           148 |            148 |  100.00% |
| `src/file/header.rs`     |           264 |            267 |   98.88% |
| `src/file/pile.rs`       |            46 |             47 |   97.87% |
| `src/file/reading.rs`    |           139 |            141 |   98.58% |
| `src/file/set.rs`        |           433 |            458 |   94.54% |
| `src/file/set/shards.rs` |           392 |            398 |   98.49% |
| `src/file/set/sink.rs`   |            58 |             58 |  100.00% |
| `src/file/set/vote.rs`   |           337 |            340 |   99.12% |
| `src/file/writing.rs`    |           208 |            210 |   99.05% |
| `src/list.rs`            |            52 |             52 |  100.00% |
| `src/recurrence.rs`      |            40 |             40 |  100.00% |
| `src/self_test.rs`       |           255 |            263 |   96.96% |
| `src/strict.rs`          |           125 |            126 |   99.21% |
| `src/strict/codes.rs`    |           152 |            152 |  100.00% |
| `src/strict/files.rs`    |           206 |            209 |   98.56% |