# Targets of reed-solomon-core

What each target is for the crate, and how CI and an audit check it. A release audit runs every row marked Required, on the candidate's exact commit, and records the result of each; an optional row it runs where it can and records why where it cannot. Keep this table in step with `rust-toolchain.toml` and `.github/workflows/ci.yml`.

| Target | Support | Release audit runs it | CI now | Runner and minimum cases |
| --- | --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu` | Supported | Required | Formatting; Clippy of every feature set; tests with `protect`, with `alloc` alone and without an allocator; documentation; the fuzz targets linted | Native Linux. Protected pages and what they report, the strict mode on its protected stack, the stack clearing, shard files, sets, the vote, the self-test. |
| `aarch64-unknown-linux-gnu` | Supported | Required | Tests | ARM64 Linux: the protected pages and the strict mode with the system's page size. |
| `aarch64-apple-darwin` | Supported | Required | Tests | macOS: protected pages, which the system keeps out of dumps only for the process, and the strict mode. |
| `x86_64-pc-windows-msvc` | Supported | Required | Tests | Windows: protected pages and the ordinary calls; every strict call on a secret refuses, as Windows gives no protected stack. |
| `wasm32-unknown-unknown` | Supported, as the browser build | Required | Clippy; the checks of `wasm/` in Node | A WebAssembly engine: the self-test's 17 known answers, a set written and rebuilt with buffers in the allocator's memory, and the strict mode's refusal. No stack is cleared. |
| `thumbv7em-none-eabihf` | Supported without an operating system, without an allocator or with `alloc` | Required (lint) | Clippy, with no features and with `alloc` | No runner: the crate compiles without the standard library. |
| `aarch64-linux-android` | Checked and linted by Clippy, not built; no runner tests it yet | Optional until a runner exists | Clippy only | A device or emulator: the ordinary and strict paths and their protected pages. |
| `aarch64-apple-ios` | Checked and linted by Clippy, not built; no runner tests it yet | Optional until a runner exists | Clippy only | A signed test app on a device: the ordinary and strict paths. |
| `aarch64-apple-ios-sim` | Checked and linted by Clippy, not built, for apps tested in the simulator; no runner tests it yet | Optional until a runner exists | Clippy only | The simulator, for the cases of the device row it can show. |

Not built: Intel macOS, Windows with the GNU toolchain and 32-bit Linux, which the crate does not test; their code is that of the rows above.

## Fuzzing

The five targets under `fuzz/` (`files`, `sets`, `plans`, `calls`, `cyclic`) are linted by CI. `release.yml` runs each for 90 seconds under the sanitizer before it packages a tag, `files` from the whole sets that `fuzz/examples/seeds.rs` writes; a longer campaign is the one `AGENTS.md` gives, before a release and after a change of the decoder, the shard file or a call on sets.
