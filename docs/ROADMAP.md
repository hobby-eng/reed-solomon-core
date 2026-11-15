# Roadmap

What is planned, in rough order. Done items move to the [changelog](../CHANGELOG.md).

- **A second construction: the cyclic code.** Besides the code by points (`Code`), a shortened systematic cyclic Reed–Solomon code with stated parameters: field, primitive element, first root, step between roots, order of the symbols, shortening and lengths. It encodes, corrects wrong symbols at unknown places together with marked unreadable ones, and checks its result. Its first known answers are the published words of MHFE-REPAIR-1 and the worked QR example of ISO/IEC 18004. Both constructions stay in one build, chosen by type; nothing is guessed from symbols. The code by points stays as it is, bytes included.
- **Marked unreadable symbols and a report of every change.** For both constructions: a call that takes the places of symbols known to be unreadable inside a shard, not only whole shards left out, and that returns every symbol it would change, with its place, what was there and what it becomes, before anything is written.
- **Several lost files in one pass.** `FileSet::repair` makes one file; making several at once reads and compares the blocks at hand once.
- **Reading a file piece by piece.** `ShardFile::read` and `Pile::new` take files that are in memory, and `FileSet::rebuild` returns the data in memory. Files too large for that need the same rules over the first and the last bytes of a file, and an output that is written in place and counts only once the call has succeeded.
- **Measured limits.** Time and memory of `build`, `correct` and `FileSet::rebuild` for large sets, and what a step of the work limit comes to in time, above all in `Field::Gf65536`, and the sizes that are practical on a phone and in a browser.
- **A build without an operating system.** The crate compiles without an allocator; a build for a bare-metal target is not part of the gates yet.
- **Fuzzing** of every public call with arbitrary numbers, lengths and bytes.
- **Proofs.** A machine-checked proof that any `k` values fix the polynomial in each field's arithmetic as implemented.
- **Publishing** on crates.io once the interface and the shard file are settled.
