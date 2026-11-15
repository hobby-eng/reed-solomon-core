# AUD-001 harnesses

The fuzz targets of audit AUD-001 of reed-solomon-core. They belong to that audit and to the commit it reviewed, which [the report](../AUD-001-2026-11-15.md) names; they are not part of the crate or of its gates.

| Target  | What it gives the crate                                                                                 | What must hold                                                                                                                                                  |
| ------- | ------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `files` | any bytes, cut into up to four files                                                                    | nothing panics or stops; the survey of a file counts each of its blocks once; rebuilt data is as long as the set says                                           |
| `sets`  | a set of files written by the crate, then damaged: bytes changed, files cut, lengthened, left out, mixed | a set left whole rebuilds and makes each of its files again byte for byte; a set that states the SHA-256 of its data gives back that data or refuses            |
| `plans` | shards in every field, with symbols changed and symbols marked unreadable                               | within `2e + s ≤ m − k` a plan puts every column right; whatever a plan or a correction changes leaves shards that agree; a refusal leaves the shards untouched |

## Inputs

None but the fuzzer's own. The `files` target starts from four whole sets, which `examples/seeds.rs` writes; the other two build their inputs from the fuzzer's bytes.

## Run

A nightly toolchain and cargo-fuzz, from the repository root. The target is named because the repository pins a stable toolchain for its own builds.

```sh
fuzz=docs/audits/AUD-001-harnesses/fuzz
mkdir -p $fuzz/corpus/files
cargo +nightly run --manifest-path $fuzz/Cargo.toml --example seeds --target x86_64-unknown-linux-gnu -- $fuzz/corpus/files
for target in files sets plans; do
  cargo +nightly fuzz run --fuzz-dir $fuzz --target x86_64-unknown-linux-gnu $target -- -max_total_time=200 -rss_limit_mb=3000 -max_len=4096
done
```

## Expected output

Each run ends with `Done N runs in 201 second(s)` and leaves no file under `fuzz/artifacts/`. The audit's runs made 1,329,965, 194,989 and 658,240 runs, under AddressSanitizer, without a finding.
