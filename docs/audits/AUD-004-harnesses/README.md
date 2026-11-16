# AUD-004 strict file API check

[public-api.rs](public-api.rs) checks whether two calls from the strict file API accept ordinary storage. It creates a protected file from the four public bytes `demo`, then tries to write the same file through the reader's header and the set's facts. It compares both outputs with the protected writer's result.

AUD-004 ran the fixture on Linux x86-64 with Rust 1.99.1 and the crate's default features. The reviewed sources were base commit `73771bfb62327c4eec540cc64c26d35c52cbf052` plus the uncommitted diff with SHA-256 `c2777f00d683034a4604d2f959f37729e63f9ad58e904e810cd2ecf22779442f`. Replaying the original successful run requires those source bytes, permission to lock the fixture's pages and stack, and the locked dependencies in the Cargo cache.

Run from the crate root. If the reviewed checkout predates this directory, set `AUD004_FIXTURE` to the path of the published `public-api.rs` before running the commands.

```sh
export CARGO_TARGET_DIR="$PWD/target/aud-004-public-api"
fixture="${AUD004_FIXTURE:-docs/audits/AUD-004-harnesses/public-api.rs}"
run-capped -- cargo build --locked --offline --lib &&
run-capped -- rustc --edition=2024 "$fixture" \
  --extern "reed_solomon_core=$CARGO_TARGET_DIR/debug/libreed_solomon_core.rlib" \
  -L "dependency=$CARGO_TARGET_DIR/debug/deps" \
  -o "$CARGO_TARGET_DIR/public-api" &&
run-capped -- "$CARGO_TARGET_DIR/public-api"
```

The original program exited with status 0 and printed `Both public API chains accept ordinary slice storage; outputs match the strict writer.` On the repaired API, compilation should reject the call that gives the writer an ordinary input and destination. The historical call to `facts.facts()` also refers to an accessor that the fix removed. The current strict-mode doctests check the supported call chains.
