# R6 verification

## Source evidence (read only)

- Brief: `C:/Users/owjs3/orca/devup-mcp-briefs/R6-vertical-sizing-and-diagnostic-truth.md`.
- WQUW-119: `docs/wquw-119/r4-r5-report.md`, `completion-layout-evidence.json` in its original girok-space worktree.
- WQUW-118: `docs/wquw-118-devup-r4-r5/REPORT.md`, `recheck.json` in its original girok-space worktree.
- WQUW-120: `docs/WQUW-120-devup-r4-r5-report.md`, `WQUW-120-devup-r4-r5-diagnostics.json` in its original girok-space worktree.

The regression fixture `crates/devup-mcp-devup-ui/tests/fixtures/r6-completion.json` contains the original 21-node closure of 3997:46333. Only the snapshot roots and node collection were narrowed; node fields were not reconstructed. `r6-completion-tokens.json` preserves token aliases extracted from the same response's sourceMap ranges and original TSX, so the offline replay uses the original names without a new Figma request. Original completion-layout-evidence.json SHA256: `6624f09143450890b8f82cd2ed9d4b867fe90a9fab09ed4ec1252a5870023878`.

## Confirmed cause

At base 4af948ae9b69, push_layout_props skips ordinary dimensions for a PAGE/SECTION/COMPONENT_SET child, then restores FIXED dimensions only when holds_positioned_children is true. The measured root has no positioned children. The separate vertical FILL flex branch also requires a positioned child. Neither condition describes the fixed-height/filling-body relationship.

The fidelity LAYOUT_FIELDS list does not contain layoutSizingVertical or layoutGrow, and canvas dimensions are excluded. The painted divider is counted by leaf geometry but its implicit cross-axis stretch has no property mapping.

## Intended scope

Preserve the height basis for FIXED vertical containers with in-flow vertical FILL children independently of containing blocks. Preserve vertical main-axis FILL with flex growth, never captured pixel height. Keep existing R4 positioned-root behavior and R5 FILL/aspectRatio mask behavior. Verify implicit CSS against generated parent and child tags; preserve lossy diagnostics where proof fails or cannot be performed.

## Validation

Final suite: **706 passed / 0 failed / 2 ignored**, against the 689/0/2 baseline. Clippy: 0. Fmt: 0.

All cargo build/test/clippy commands used -j 2 with this worktree's default local target. No installed MCP tool was used and CARGO_TARGET_DIR was not set. `validation.json` records the exact executable paths, SHA-256 values, CLI version and the new tests listed by the actual regression executable. CLI identity during verification: `devup-mcp 0.4.3 (4af948ae9b69-dirty)`. The dirty suffix identifies the modified worktree build before committing.

**자기 target에서 검증했고 실행된 테스트가 내 것임을 확인했다.**

`completion.tsx` and `completion-sizing-evidence.json` were produced by the final test execution from the original captured nodes and token aliases. This verifies generated CSS and diagnostic contracts, not browser pixel equivalence. External source worktrees were read only. Commit is local; no push. The required cargo clean is performed after the commit.

## Reviewed golden changes

- `upstream-codegen-199-829a5c949d.snap`: the image's vertical FILL now uses remaining-space flex inside its FIXED 600px column, retaining fluid width and maxH/maxW.
- `upstream-codegen-201-8207a4008e.snap`: restores the FIXED 760px root height and the nested column/image FILL chain; no FILL axis becomes pixels.
- `wquw_151_frames__wquw_151_frame_3879_35503.snap`: child 3879:35504 now grows vertically inside the existing fixed-height root.
- `wquw_151_frames__wquw_151_frame_3879_36059.snap`: body 3879:36063 and text 3879:36070 now express their recorded vertical FILL.
- `wquw_151_frames__wquw_151_frame_3879_36108.snap`: body 3879:36112 and text 3879:36119 now express their recorded vertical FILL.

Only these five TSX goldens change (two out of 268 plugin corpus cases). No unrelated pixel sizes or canvas coordinates were added.

## Diagnostic truth and contract updates

- Implicit stretch is accounted for only after checking the actual immediate generated flex parent, source layout mode and child FILL, generated cross-axis alignment, positioning, dimension overrides, margins/min/max constraints, and an established cross-axis basis. Evidence records the parent/child tags and CSS behavior. Validation rechecks the proof rather than trusting the source-map resolution string.
- Failed CSS proof is `not-accounted-for`; absent/opaque parent CSS is `not-verifiable`. Both retain lossy uncovered-property diagnostics.
- Positive layoutGrow and vertical FILL (plus the FIXED height basis for vertical FILL containers) enter fidelity coverage. Nested main-axis FILL can use a verified ancestor height through the emitted flex chain. Embedded roots disclose the required root height while keeping child growth.
- The WQUW-120 R4 fixture's inline layout coverage rises from 105 to 114: two vertical FILL fields and seven positive layoutGrow fields. Its positioned root dimensions remain unchanged. Existing tests now assert the expanded exact count, retaining the placement and component-reference checks.
- Existing R2 node 3997:46313 remains explicitly lossy: its FILL height is emitted as 100% inside an auto-height row. The test retains its old checks and adds an exact assertion for this new vertical dependency rather than suppressing it.
- Auth response tests still compare the complete response object and verify secret exclusion; their expected object now includes the server identity required by R6-4.
- `server.version` is the package version, `server.buildId` is exactly the existing CLI build identity (including dirty/source overrides), and `server.commit` is the separately captured Git commit, or null when unavailable. An overridden buildId is never misrepresented as a Git commit.

## Red/green record

- `cargo test --workspace -j 2 r6_ --no-fail-fast`: all original eight new regressions failed against the original production code (0 passed / 8 failed).
- The nested-FILL regression was subsequently observed failing before its ancestor-height proof was added.
- The regression suite also protects HUG/FILL from pixel pinning, unknown parent CSS, removal of sizing source-map entries, and stale implicit-stretch proof after generated parent CSS changes.
- `r6-red.log`, `r6-nested-red.log`, and subsequent validation logs remain local and ignored by git.


## Independent review

A read-only reviewer found two additional issues; both were reproduced as failing regressions before correction:

- An overflowing FILL body's automatic CSS minimum could displace the following action. The generator resets minH only for demonstrated collected content overflow, preserves explicit source minimums, and fidelity requires the reset when necessary.
- Fractional FIXED heights were compared using raw float formatting. The mapping now uses the same two-decimal CSS formatter as the emitter, eliminating that false loss.

The original five snapshot diffs remain individually reviewed. The corpus manifest updates only the checksums of the two changed plugin goldens; the corpus validation test itself is unchanged.


The overflow fix was additionally narrowed using a failing horizontal-body regression: only vertical layouts sum child heights and gaps; side-by-side children use the oversized-child check. No additional goldens changed. Final verification below supersedes earlier intermediate failures.
