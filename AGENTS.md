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
- `cargo test --no-default-features`: the unit tests and the self-test without an allocator
- `cargo doc --no-deps`, and with `--no-default-features`, both with `RUSTDOCFLAGS="-D warnings"`
- `cargo fmt --check --manifest-path fuzz/Cargo.toml` and `cargo clippy --manifest-path
  fuzz/Cargo.toml --all-targets -- -D warnings`: the fuzz targets

CI (`.github/workflows/ci.yml`) runs these gates on x86-64 Linux and the tests on ARM Linux, macOS
and Windows; `audit.yml` holds `Cargo.lock` and `fuzz/Cargo.lock` against the RustSec advisories.

Before a release, and after a change of the decoder, the shard file or a call on sets: the fuzz
targets under `fuzz/` (`files`, `sets`, `plans`, `cyclic`, `calls`) run under the sanitizer, `cargo +nightly
fuzz run --fuzz-dir fuzz --target x86_64-unknown-linux-gnu <target> -- -max_total_time=60
-rss_limit_mb=3000 -max_len=4096`, after `cargo +nightly run --manifest-path fuzz/Cargo.toml
--example seeds --target x86_64-unknown-linux-gnu -- fuzz/corpus/files` gives `files` whole sets to
start from; and the proof that each field is one checks its largest fields, `cargo test --release
--lib field::proof -- --ignored` (minutes). A change of the field arithmetic runs `cargo run --release --no-default-features
--features alloc --example timing`: products and inverses must take the same time whatever their
elements, which only the compiled code shows (exit code 0); the release workflow runs it before it
packages a tag.

Layout: `src/buffer*` the memory every result and working buffer of data lies in, protected
pages or the allocator's; `src/strict*` the strict mode, the calls for a secret in the open;
`src/list.rs` the short lists of a search; `src/field*` the three fields and the word-wide scaling kernel;
`src/code*` the code, its sources and the whole-shard helpers; `src/correct*` the decoder, the correction and the plan of
changes for marked gaps; `src/cyclic*` the cyclic code in the caller's memory (`words.rs`) and in
its own (`owned.rs`), with the search of one word (`decode.rs`); `src/recurrence.rs`
Berlekamp–Massey for both codes; `src/file*` the shard file, its header, checksum and reading, and sets of
files (`pile`, `set`, `writing`), with the vote that names the files that differ from the rest
(`set/vote`); `tests/strict/` the strict mode, in a test process of its own whose lock limit
no other test uses up; `tests/suite/` the tests against an independent implementation, over every choice
of shards, of shard files written apart and damaged, and of sets put together from their files.
Keep `docs/API.md` in sync with public Rust calls and their contracts. Update
`docs/TECHNICAL.md` when the algorithms, file format or implementation guarantees change.

The crate holds no key; its one cryptographic function is SHA-256 of the data, from the `sha2`
crate, never written here. It never indexes a table by a byte of a shard and never branches on
one in the scaling kernel or in the checksum; keep that when changing `src/field/lanes.rs` and
`src/file/crc32c.rs`. Every buffer that holds data or values that follow from it is wiped
when let go, every result that holds data and every working buffer of its bytes is a `Buffer`,
and no `Debug` text shows data; keep that in new code (`docs/TECHNICAL.md`, Secrets in memory).
A new buffer of data comes from `Buffer::zeros`, which a strict call checks before a byte goes
in, and the calls of `src/strict*` take a secret only through the sealed `Held` traits; a new
call that handles data gets its strict counterpart (`docs/TECHNICAL.md`, The strict mode).

## Audits

An audit, a review of findings or a remediation follow-up of this repository follows the workspace's
audit procedure in `../.github/audits/`: `FULL_AUDIT_GUIDE_V2.md` (the checks and their order;
section 17 for Rust crates and every target they are built for), `AUDIT_STANDARD.md`,
`AUDIT_TEMPLATE.md` and `audit-report.schema.json`. Section 17 asks for `docs/audits/TARGETS.md`
with the targets, their runners and the fuzzing budgets; until this repository has one, the audit
derives the targets as section 17.3 says and records the missing file. Reports, their JSON twins and
their harnesses stay in this repository's `docs/audits/`.
