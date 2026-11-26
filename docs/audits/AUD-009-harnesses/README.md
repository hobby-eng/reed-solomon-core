# AUD-009 WebAssembly public-contract probe

The probe exercises the public known-answer self-test and seven checks of the ordinary and strict APIs on wasm32-unknown-unknown. It belongs to AUD-009, whose report records the reviewed working-tree digest against Git commit `4ababf97cabe23f0ca23ab7f430ecb134d31f051`. It uses generated public bytes and requires the workspace Rust toolchain, its wasm32-unknown-unknown target, and WABT 1.0.36.

From the repository root, with the workspace Cargo and Rustup homes configured, run:

```sh
set -o pipefail
run-capped -- cargo build --offline --locked --release \
  --target wasm32-unknown-unknown --target-dir target/aud-009-wasm \
  --manifest-path docs/audits/AUD-009-harnesses/Cargo.toml
wasm-interp target/aud-009-wasm/wasm32-unknown-unknown/release/reed_solomon_aud_009_wasm.wasm \
  --run-all-exports | rg -Fx 'check_public_contracts() => i32:24'
```

The last command exits successfully only when the interpreter reports all 17 public known answers and the seven platform checks. It tests the library in an interpreter and establishes no browser integration or native stack retention property.
