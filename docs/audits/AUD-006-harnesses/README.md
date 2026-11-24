# AUD-006 harnesses

The probes behind the findings of [AUD-006](../AUD-006-2026-11-24.md), the audit of commit
`b0971436e41854b4986c14976c1e9af641233914` with a working-tree diff of SHA-256
`38d896bee64f27ad787b52adb6d279e82a3f640c33d70e8b1c577da3ff8eb170` (the dependency requirements made
compatible). Each folder is a Cargo package of its own that takes the crate from the repository
root (`path = "../../../.."`), so it checks whatever tree it is run in. Run the commands from the
folder, with the toolchain of `rust-toolchain.toml`; add `--offline` where the registry cache holds
the dependencies.

| Folder | What it checks | Finding |
| --- | --- | --- |
| `timing/` | `CyclicWords::correct_word` on 2,048 clean all-zero words against 2,048 random valid words, 41 interleaved rounds, in the named GF(2^11) and in GF(2^10) made by `Field::binary`, built as a program with `panic = "abort"`, LTO and one codegen unit | AUD-006-SEC001 |
| `one-copy/` | A program that takes this crate and `secret-protection-core = "0.1"`: one copy of the protection crate, its `Secret` accepted as `Held` | Observation |
| `codes/` | An independent oracle (log tables, Gaussian elimination, long division, brute-force and information-set decoding) against the fields, the MDS code, both decoders and the cyclic code; MHFE-REPAIR-1 cards and repairs; the `Mark` refusal | AUD-006-DOC010, observations |
| `files/` | An independent reader of the shard file against `ShardFile::read`, random piles against an oracle, the vote's time when the holders change, the fields that have a shard file | AUD-006-DOC008, AUD-006-DOC009 |

## timing

```sh
cargo build --locked --release --bin timing
taskset -c 3 target/release/timing
```

Expected at the audited snapshot: a per-round ratio near 1.00 for GF(2^11) and about 2.8 for
GF(2^10), and exit code 1, as a median ratio outside 0.8 to 1.25 fails the run. After a fix of
AUD-006-SEC001 both are near 1.00 and the exit code is 0. The standing check that follows from it
is the crate's `examples/timing.rs`, which covers products and inverses in every kind of field and
which the release workflow runs.

## one-copy

```sh
cargo build --locked --bin probe-one-copy
cargo tree --locked -e normal -i secret-protection-core
target/debug/probe-one-copy
```

Expected: one `secret-protection-core` node, used by the probe and by reed-solomon-core; the
program prints that both `Secret` and `WorkArea` are the same types and that a strict encode with
the program's own secrets works, and exits with 0 (1 if either fails). `--isolate` enters
isolation first, through the program's own dependency.

## codes

```sh
cargo build --locked --release --features protect --bins
target/release/mhfe data
target/release/mark_doc
target/release/fields
```

`data/mhfe-vectors.txt` holds the containers, cards and repairs of the MHFE specification's
`vectors/profiles/repair-words.json` (SHA-256 `fa4f77a5…`, as that file's `SHA256SUMS` gives it) as
word numbers of the BIP39 English list in `bip39-crate-english.txt` (SHA-256 `2f5eed53…`, the list of
BIP-0039). Expected: 16 of 16 cards and 3 of 3 repairs agree, by every path, and exit code 0.
`mark_doc` only shows behaviour: `Err(Symbol)` for a marked symbol 0xFFFF and a plan for 0x0000
(AUD-006-DOC010). The other programs (`mds`, `decode`, `cyclic`, `edges`, `known`, `apply_cross`,
`fields`, `mhfe_diff`) print their results and end with exit code 1 when any comparison with the
oracle differs, or, for `apply_cross`, when a plan applies where its binding forbids it.
`mhfe_diff` holds a copy of the GF(2^11) repair code of mhfe_core and compares the two on 24,000
random containers. With `AUD006_NEGATIVE_CONTROL=1` the oracle takes one wrong power of x: every
program that compares with it (`fields`, `decode`, `cyclic`, `mds`, `known`, `mhfe`)
must then end with a non-zero exit code.

## files

```sh
cargo build --locked --release --bins
target/release/holders 100000
cargo build --locked --bin aud006-files-probe
target/debug/aud006-files-probe fields
target/release/reader
target/release/random
```

Expected at the audited snapshot: `holders` only measures: the time per counted step of a vote
whose holders stay the same and of one whose holders change at every block (about 100 times more
with the default `protect` feature, AUD-006-DOC008); `fields` only shows which fields have a shard
file (AUD-006-DOC009); `reader` and `random` print their counts and end with a non-zero exit code
on the first disagreement with the crate.
