# SECTION frame selection pagination verification

Verified locally on 2026-09-08 against base commit `1e8b0b6` (0.2.1).

## Contract and diagnosis

The compact SECTION index, explore candidates and `selection_required` response
are intentional menus. Their node counts do not establish an export defect.
README's Section export instructions, public tool instruction 7, and
`section_requires_selection_then_exports_requested_or_all_screens_from_one_artifact`
establish that explicit `frameIds` / `allScreens` requests per-screen artifacts.
Following each canonical URL is an alternative, and a separate requirement for
`referencePng`; it is not required to finish a selected TSX export.

The observed selected export was partial, with 20 preserved nodes and 32 missing
child edges. This alone was distinguished from the pagination hypothesis.
Executing the original read-only `fast_snapshot.js` for selected FRAME
`3831:10548`, with SECTION envelope root `4279:7811`, returned the raw marker:

```json
{"offset":0,"nextOffset":10,"complete":false,"totalNodes":144}
```

`decode_fast_multi_snapshot` legitimately accepts that partial page.
`accept_fast_multi_root` previously recorded it without scheduling its successor.
The single-frame path already followed cursors. The fix continues the existing
multi-root call with its original SECTION/root IDs and updated snapshot offset,
validates the returned range and completion, strips the internal marker, and
uses the existing node/resource merge. Completeness checks remain unchanged.

## Regression evidence

Before production changes, `cargo test -p devup-mcp-figma --test collector
selected_section_ -- --nocapture` failed with:

- `selected frames need page 1 before collection can complete`
- `wrong request offset: ()` (invalid cursor was accepted)

The final three regression tests pass. They cover one/two explicit selections,
three pages, late text and tokens, resource deduplication, reversed response
order and visual root order, successful termination, incorrect offset/range,
premature/absent completion, replayed pages and empty nonadvancing pages.
Existing no-cursor complete-envelope fixtures remain supported.

## Live comparison

File: `85CgSws3o5XsLv7aAwWJyS`; SECTION: `4279:7811`.
Requests used `refresh: true`, `delivery: resource`, direct Figma acquisition.
Resource manifests were read and every output's byte count and SHA-256 checked.
No canvas changes were made.

| Request | Status | Nodes | Declared/exported child edges | Missing edges | Figma calls |
| --- | --- | ---: | --- | ---: | ---: |
| Original SECTION, both frameIds | partial | 20 | 50 / 18 | 32 | 3 |
| Fixed SECTION, both frameIds | complete | 288 | 286 / 286 | 0 | 31 |
| Fixed SECTION, frameId 3831:10548 | complete | 144 | 143 / 143 | 0 | 16 |
| Direct FRAME 3831:10548 | complete | 144 | 143 / 143 | 0 | 15 |
| Direct FRAME 3831:10741 | complete | 144 | 143 / 143 | 0 | 15 |

For both screens, all 144 node objects (including every field) match the direct
FRAME export exactly. Collections, variables, styles and remote-variable values
also match by ID. Each generated TSX is byte-identical to its direct counterpart:
14,304 bytes, versus 1,074 bytes before the fix. Both include the high-resolution
upload guidance (minimum 1000px on the long side) and TIP. There are 26 actual TEXT
nodes per frame; the existing fidelity report counts 31 text items.
The merged selection contains 12 variables and 11 styles, versus 5 and 1 before.

The single-frame selection matches direct acquisition too. All fixed requests
have acquisition `complete`, projection `exact`, and `missingChildren: []`.
Transport changes from `text` to `text-paginated` as pages are followed.

The unselected SECTION remains `selection_required` with a compact index (7 nodes,
6 candidates). Planning notes `3831:10542`, `3831:10885` and Alert `3831:10547`
were acquired through explicit SECTION selection: complete, 13 nodes, 5 calls,
no missing children. The notes describe advance upload guidance, revised warning
copy and moving TIP above the body/title. No implementation or design was edited.

## Validation and local artifacts

- `cargo fmt --all -- --check`: passed.
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: 4 passed.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `cargo insta test --workspace --all-features --check`: 473 passed, 2 existing
  manual/live tests ignored; no snapshots to review. Includes `stdio_smoke` and
  `section_export` integration tests.
- Debug build and real Figma comparisons: passed.
- `cargo build --workspace --release -j 1`: passed. The initial parallel
  release build failed with Windows disk-full error 112 while creating an
  archive; the serial retry completed. No unrelated caches were deleted.
  MSVC emitted its informational import-library linker message as a warning;
  Clippy's warnings-as-errors check passed separately.
- Independent read-only code review: no findings requiring changes.

Local-only evidence is under `target/section-pagination/`: raw first-page response,
before/after exports, comparison JSON, verification drivers and command logs.
`devup-mcp-fixed.exe` is the debug binary used for live verification; it is a local
copy, not an installed replacement. Global MCP configuration was not modified.
`devup-mcp-fixed-release.exe` is also available locally from the successful
optimized build. Live comparisons above used the debug binary.
No push, PR creation, release or deployment was performed.

## Selection-list follow-up

The subsequent list improvement preserves the compact selection flow and adds
visible-text previews, list status/count, and a concrete `nextAction.example`.
On the same live SECTION it still returns 6 candidates / 7 summary nodes, now
with upload guidance, TIP and planning-note previews. Executing the returned
example unchanged in the same MCP session successfully exports its selected
candidate with `status: complete`.

For this follow-up, Node behavior tests (6), SECTION integration tests (3),
format and workspace Clippy passed. Independent review found no functional
issues. The full workspace test build failed with Windows error 112, and the
release build failed with LLVM `no space on device`. These are disk-capacity
limitations; the earlier full-suite/release success above applies to the
pagination commit, not this follow-up. The live-verified follow-up debug binary
is `target/section-pagination/devup-mcp-selection.exe`; no installation was
replaced. Follow-up logs and live responses use the `selection-*` and
`improved-index*` names in that local evidence directory.
