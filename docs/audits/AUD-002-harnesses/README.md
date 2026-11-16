# AUD-002 public memory-contract check

Reviewed base commit `73771bfb62327c4eec540cc64c26d35c52cbf052`, working-tree fingerprint `0d6de2c4d47fc219d5b24094ddd3103e5bdba8c952511bff1387fac6d821303f`. A matching snapshot and the pinned Rust toolchain are required. Run from the crate root. The two inputs are ASCII test data. The output reports the operating system's decisions for two ordinary systematic outputs. This check does not inspect system memory.

```sh
cargo build --locked --lib
rustc --edition=2024 docs/audits/AUD-002-harnesses/memory_contract.rs --extern reed_solomon_core=target/debug/libreed_solomon_core.rlib -L dependency=target/debug/deps -o target/aud002-memory-contract
target/aud002-memory-contract protect
cargo build --locked --lib --no-default-features --features alloc
rustc --edition=2024 docs/audits/AUD-002-harnesses/memory_contract.rs --extern reed_solomon_core=target/debug/libreed_solomon_core.rlib -L dependency=target/debug/deps -o target/aud002-memory-contract
target/aud002-memory-contract alloc
```

## Public correction and size contracts

With the default library built, run the following from the repository root. The program uses four two-byte shards and two metadata-only descriptors. It checks the current observation for API001 and API007; an implementation that strengthens either contract should change the expected result.

```sh
cargo build --locked --lib
rustc --edition=2024 docs/audits/AUD-002-harnesses/recovery_contract.rs --extern reed_solomon_core=target/debug/libreed_solomon_core.rlib -L dependency=target/debug/deps -o target/aud002-recovery-contract
target/aud002-recovery-contract
```

Both programs return zero for the audited behavior and a nonzero status on an assertion failure. Native protection flags reflect the host. Portable buffers report three false flags. The recovery check reports an accepted stale plan, an inconsistent result and two accepted descriptors.
