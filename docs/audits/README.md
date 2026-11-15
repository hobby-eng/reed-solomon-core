# Audits

| Audit | Completed (UTC) | Reviewed commit | Scope | Findings at the reviewed commit |
| --- | --- | --- | --- | --- |
| [AUD-001](AUD-001-2026-11-15.md) · [JSON](AUD-001-2026-11-15.json) | 2026-11-15 | `4969ec8f020e430983f68188152bb0ccff527c2e` | The whole crate after its buffers moved into protected pages: the code, the correction, shard files and sets, secrets in memory, documents, build | 16: 15 low, 1 medium |

The report records the findings as they stood at the reviewed commit; the table below says where each stands now. Audit evidence folders are machine-local and ignored by Git. The harnesses are kept beside the report so that each check can be run again.

## Where the findings stand

| Finding ID | Status | Fix commit | Verification commit | Evidence |
| --- | --- | --- | --- | --- |
| AUD-001-DOC001 | fixed | `4afe607` | none | The documents, the feature's comment and the type's own comment say what each system gives and that the feature brings the standard library. |
| AUD-001-DOC002 | fixed | `4afe607` | none | docs/COVERAGE.md records 98.19 % of 4,026 lines on Linux x86-64 with the default features, by file, with the command and the source fingerprint; the README names the figure. |
| AUD-001-BLD001 | fixed | `4afe607` | none | `.github/workflows/ci.yml` runs the gates on Linux for x86-64, with both feature sets, and the tests on Linux for AArch64, macOS and Windows; `audit.yml` holds Cargo.lock against RustSec. |
| AUD-001-ARC001 | fixed | `92b842d` | none | The test makes the shards 4,096 at a time and compares the same 65,536 shards. |
| AUD-001-DOC003 | fixed | `92b842d` | none | Both lines compare with `*data`, as the tests do. |
| AUD-001-DOC004 | fixed | `92b842d` | none | The paragraph names `sha2`, `zeroize` and, by default, `secret-protection-core`, and says that the crate needs no allocator without its default features. |
| AUD-001-FUN001 | fixed | `92b842d` | none | A record is 24 bytes and keeps both places in eight; a unit test reads back places up to `usize::MAX` and tells 70,000 from 65,535. |
| AUD-001-API002 | fixed | `92b842d` | none | `apply` takes `&Changes`: the plan's own list, or one a program collects. The list lies in a `Buffer` and is read twice from the same records. |
| AUD-001-API003 | fixed | `92b842d` | none | The three paths return `CodeError::Memory`; only collecting a list with `FromIterator` stops the program, as a `Vec` does. The documents say so. |
| AUD-001-DOC005 | fixed | `92b842d` | none | The figure is the memory asked for, and the comments and the README say that with `protect` the system maps whole pages for the syndromes; the bound of 8 MiB holds either way. |
| AUD-001-API004 | fixed | `92b842d` | none | `Buffer::is_out_of_dumps` and `Buffer::is_out_of_children` pass the protection crate's answers on; the documents name the Windows limit (`WER_MAX_REGISTERED_ENTRIES`) and hibernation. |
| AUD-001-ARC002 | fixed | `92b842d` | none | A list starts with room for 128 records, most of a page. The vote keeps making a new search per change of holders, and the documents now name that cost: keeping the buffer would change the decoder for piles that only damage makes. |
| AUD-001-ARC003 | fixed | `92b842d` | none | `a_wrong_word_below_or_between_the_gaps_keeps_its_place_in_the_list` puts a wrong word before and then between two marks; the fuzz target `plans` checks the order, the kind and the value read of every change, and that every mark is listed. |
| AUD-001-ARC004 | fixed | `92b842d` | none | The loop stops at the number of intact and damaged blocks the survey counts. |
| AUD-001-API005 | fixed | `92b842d` | none | `Changes::is_locked` exists; the documents and the comment of `Buffer::is_out_of_children` say what a child gets after `fork` on each system (zeros on Linux and Android, no pages on macOS and iOS), and the documents name the shared lock limit. |
| AUD-001-BLD002 | accepted | none | none | Accepted: the pinned commit is on the published main branch, which is rewritten only by a force-push; the pin moves in the same step as any regrouping. |
