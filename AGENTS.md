# reed-solomon-core

Reed–Solomon coding for sets of shards. Workspace rules: `AGENTS.md` in the workspace root, and the
coding guide it names (CORE, RUST, TOOL, DOC).

Toolchain: Rust 1.99.1 (`rust-toolchain.toml`). On the development machine set `CARGO_HOME` and
`RUSTUP_HOME` as the workspace rules say, and start every cargo command through `run-capped`.

Fast gates (CORE-43):

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo clippy --no-default-features --features alloc --all-targets -- -D warnings` (buffers in
  the allocator's memory)
- `cargo clippy --no-default-features -- -D warnings` (the crate without an allocator)
- `cargo clippy --no-default-features --target thumbv7em-none-eabihf -- -D warnings`, and with
  `--features alloc`: a target without an operating system (`rustup target add` it first)
- `cargo test`, and `cargo test --no-default-features --features alloc`
- `cargo fmt --check --manifest-path fuzz/Cargo.toml` and `cargo clippy --manifest-path
  fuzz/Cargo.toml --all-targets -- -D warnings`: the fuzz targets

CI (`.github/workflows/ci.yml`) runs these gates on x86-64 Linux and the tests on ARM Linux, macOS
and Windows; `audit.yml` holds `Cargo.lock` against the RustSec advisories.

Before a release, and after a change of the decoder, the shard file or a call on sets: the fuzz
targets under `fuzz/` (`files`, `sets`, `plans`, `cyclic`, `calls`) run under the sanitizer, `cargo +nightly
fuzz run --fuzz-dir fuzz --target x86_64-unknown-linux-gnu <target> -- -max_total_time=60
-rss_limit_mb=3000 -max_len=4096`, after `cargo +nightly run --manifest-path fuzz/Cargo.toml
--example seeds --target x86_64-unknown-linux-gnu -- fuzz/corpus/files` gives `files` whole sets to
start from; and the proof that each field is one checks its largest fields, `cargo test --release
--lib field::proof -- --ignored` (minutes).

Layout: `src/buffer*` the memory every result and working buffer of data lies in, protected
pages or the allocator's; `src/field*` the three fields and the word-wide scaling kernel;
`src/code*` the code, its sources and the whole-shard helpers; `src/correct*` the decoder, the correction and the plan of
changes for marked gaps; `src/file*` the shard file, its header, checksum and reading, and sets of
files (`pile`, `set`, `writing`), with the vote that names the files that differ from the rest
(`set/vote`); `tests/suite/` the tests against an independent implementation, over every choice
of shards, of shard files written apart and damaged, and of sets put together from their files.
`docs/TECHNICAL.md` describes all of it and must change with it.

The crate holds no secret; its one cryptographic function is SHA-256 of the data, from the `sha2`
crate, never written here. It never indexes a table by a byte of a shard and never branches on
one in the scaling kernel or in the checksum; keep that when changing `src/field/lanes.rs` and
`src/file/crc32c.rs`. Every buffer that holds data or values that follow from it is wiped
when let go, every result that holds data and every working buffer of its bytes is a `Buffer`,
and no `Debug` text shows data; keep that in new code (`docs/TECHNICAL.md`, Secrets in memory).
