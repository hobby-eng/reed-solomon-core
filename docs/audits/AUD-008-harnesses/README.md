# AUD-008 expanded verification probes

Reviewed runtime commit: `23968f420857f66540f6e02b1b310a36c02bd14e`. The following
`15643f2a90d1b831c8d786c31ec5dad8fa87654d` changes only the repository README.

Expected successful result: three tests pass, reporting 8,192 polynomial cases, 6,144 cyclic cases
and 2,862 file subsets, with exit code 0. These are expected totals until the report records a
completed execution.

All inputs are synthetic. This crate depends on the authoritative checkout through a path; it
contains no copy of the implementation and does not modify production files.

- Exhaustive polynomial decoding: GF(4), k=1/2, all 256 received words and 16 erasure masks.
- Exhaustive cyclic decoding: GF(4), n=3, k=1/2, all six valid first-root/step pairs,
  all 64 received words and eight erasure masks; caller-memory and owned APIs.
- File operations: both supported fields, k=1..3, lengths 0..8, block widths 1..3 symbols,
  all nonempty subsets of k+2 files, digest checks, work accounting and repeated repair indexes.

The GF(4) oracle uses a literal multiplication table and exhaustive codebooks. It does not use
library field arithmetic or encoding to construct expected words. Library encoding is compared
with that codebook separately. Outside the unique-decoding radius, a word may decode to another
valid codeword; the oracle tests distance to every valid word, not the identity of sent data.

Run from the repository root, with workspace toolchains and resource controls:

```sh
cargo test --locked --manifest-path docs/audits/AUD-008-harnesses/Cargo.toml -- --nocapture
```

These probes complement retained field-proof/fuzz evidence. They do not establish constant-time
behavior or protection against every operating-system failure.
