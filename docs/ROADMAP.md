# Roadmap

What is planned, in rough order. Done items move to the [changelog](../CHANGELOG.md).

- **Reading a file piece by piece.** `ShardFile::read` and `Pile::new` take files that are in memory, and `FileSet::rebuild` returns the data in memory. Files too large for that need the same rules over the first and the last bytes of a file, and an output that is written in place and counts only once the call has succeeded.
- **Measured limits.** Time and memory of `build`, `correct` and `FileSet::rebuild` for large sets, and what a step of the work limit comes to in time, above all in `Field::Gf65536`, and the sizes that are practical on a phone and in a browser.
- **A build without an operating system.** The crate compiles without an allocator; a build for a bare-metal target is not part of the gates yet.
- **Fuzzing** of every public call with arbitrary numbers, lengths and bytes.
- **Proofs.** A machine-checked proof that any `k` values fix the polynomial in each field's arithmetic as implemented.
- **Publishing** on crates.io once the interface and the shard file are settled.
