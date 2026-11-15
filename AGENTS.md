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
- `cargo test`, and `cargo test --no-default-features --features alloc`

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
