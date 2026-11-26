# AUD-007 verification harness

This probe belongs to [AUD-007](../AUD-007-2026-11-24.md), the review of
`b0971436e41854b4986c14976c1e9af641233914..b16c64f1388ceddfcabc45186fdc60309adc83db`.
It depends on the existing repository by path and checks the source currently present there.
The audited runs used Rust 1.99.1 and the committed runtime source with its two Rustfmt corrections.

## What it checks

One multiblock vote reuses its decoder as holders change. For each block, the probe compares that
vote with a fresh one-block search and with a literal expected outcome. Both GF256 and GF65536
are covered. Twelve block states cover same-count holder substitutions, different wrong holders,
an unresolved block followed by agreement, repeated holders, changing counts, holder counts at
and below the threshold, and a shortened final block. It asserts the work limit refusal and exact
step accounting. The data consists only of synthetic counting-pattern bytes.

Fresh searches share the production decoder and are an oracle for state reuse, not an independent
coding implementation. Literal expected outcomes and the repository's independent decoder tests
provide the additional check. No external vectors, network access or secrets are needed.

## Run from the repository root

Use the pinned toolchain and the workspace toolchain homes. On the development machine, prefix
Cargo commands with the resource wrapper specified in AGENTS.md.

```sh
cargo test --locked --manifest-path docs/audits/AUD-007-harnesses/Cargo.toml -- --nocapture
cargo test --locked --manifest-path docs/audits/AUD-007-harnesses/Cargo.toml --no-default-features -- --nocapture
cargo clippy --locked --manifest-path docs/audits/AUD-007-harnesses/Cargo.toml --all-targets -- -D warnings
```

The first run uses protected pages; the second uses allocator memory. Add `--offline` when the
registry cache is populated. Each test run should print one successful 12-block comparison for
each field, report one passed test, and exit zero. Any failed expected outcome or accounting
assertion makes Cargo exit nonzero. Build output and command logs are local evidence, not part
of a release package.
