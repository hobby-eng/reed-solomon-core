# AUD-005 strict file API checks

These five small Rust programs test the public file API on the AUD-005 working-tree snapshot on top of 73771bfb (diff SHA-256 2d41af8fdfe47f7e00fb9d3112a6438f568d580fba471c8148f052e8abf7caaa). Use the pinned toolchain and the default features. The runtime example uses only the four public bytes demo and requires a Linux process that can obtain protected pages.

Four fixtures test one argument at a time. [output.rs](output.rs) gives the strict writer valid input and a Vec destination. Its only type error should be E0277 for HeldMut. [payload.rs](payload.rs), [facts.rs](facts.rs) and [chain.rs](chain.rs) supply a Vec as input to three operations and must each report E0277 for Held. [positive.rs](positive.rs) supplies protected buffers, explicitly selects the portable API for a second write and checks that both files contain the same bytes.

Run the commands below from the checkout root with Bash. On the workspace laptop prefix each Cargo and Rust compiler invocation with the machine’s run-capped wrapper. Set the target directory to a directory for build artifacts.

~~~bash
export CARGO_TARGET_DIR="$(mktemp -d)"
cargo build --locked --lib
harness=docs/audits/AUD-005-harnesses
args=(--edition=2024 --extern "reed_solomon_core=$CARGO_TARGET_DIR/debug/libreed_solomon_core.rlib" -L "dependency=$CARGO_TARGET_DIR/debug/deps")
for case in output payload facts chain; do
  if rustc "${args[@]}" "$harness/$case.rs" --emit=metadata --error-format=json -o "$CARGO_TARGET_DIR/$case.rmeta" 2>"$CARGO_TARGET_DIR/$case.jsonl"; then
    exit 1
  fi
  python3 - "$case" "$CARGO_TARGET_DIR/$case.jsonl" <<'PY'
import json, sys
from pathlib import Path
case, path = sys.argv[1:]
coded = [d for d in map(json.loads, Path(path).read_text().splitlines()) if d.get("code")]
trait = "HeldMut" if case == "output" else "Held"
assert len(coded) == 1 and coded[0]["code"]["code"] == "E0277"
assert "Vec<u8>" in coded[0]["message"] and trait in coded[0]["message"]
PY
done
rustc "${args[@]}" "$harness/positive.rs" -o "$CARGO_TARGET_DIR/positive" && "$CARGO_TARGET_DIR/positive"
~~~

The positive executable prints one line confirming that the two writers returned identical fixture bytes. The negative checks compile only. No invalid fixture body is called.
