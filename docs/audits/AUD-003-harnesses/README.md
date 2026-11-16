# AUD-003 allocator contract check

This standalone test checks the public alloc-only API on synthetic two-byte shards. It belongs to the working-tree diff `158b257d0d2da2a0ead32b27c4c8eb0bb3f849c46b13c273f395b778c392bb30` over `73771bfb62327c4eec540cc64c26d35c52cbf052`. It counts allocations on a successful call, then denies each allocation in turn. Each run has one thread, uses public bytes and restores the ordinary allocator before logging or evaluating assertions.

Run from the repository root with the workspace toolchain. Set `AUDIT_DIR` to the directory containing this audit report.

```sh
export CARGO_TARGET_DIR="$AUDIT_DIR/target-allocator"
run-capped -- cargo build --locked --offline --no-default-features --features alloc
run-capped -- rustc --edition=2024 "$AUDIT_DIR/AUD-003-harnesses/allocation.rs" \
  --extern "reed_solomon_core=$CARGO_TARGET_DIR/debug/libreed_solomon_core.rlib" \
  -L "dependency=$CARGO_TARGET_DIR/debug/deps" -o "$AUDIT_DIR/allocation-check"
run-capped -- "$AUDIT_DIR/allocation-check" --io
ulimit -c 0
run-capped -- "$AUDIT_DIR/allocation-check"
```

The I/O sub-run returns zero for the reviewed revision. The full run also checks that the startup self-test reports an unavailable allocation. At this revision it ends with allocator error and exit status 134. After a correct fix, the full run should return zero. The number of allocations is a property of the toolchain, profile and fixture rather than an API guarantee.

