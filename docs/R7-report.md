# R7 verification report

Base: `ffea11d12c3f` (`integration/r6`). External WQUW-118/119 reports and JSON evidence were read only. No installed devup-mcp MCP tool was used.

## Changes and evidence

- R7-1: Extended the existing `implicitCssVerification`/source-map revalidation path. Positive remaining space is calculated from captured parent width, emitted padding/gap, sibling bases and flex grow, with explicit basis/shrink evidence. Main-axis mappings use `accounted-for-implicit-flex-grow`. Unsupported constraints, negative free space, unknown bases, replaced-element minimums and mismatched allocations remain unverified/lossy. HUG text uses its preserved intrinsic sizing contract; browser font metrics/pixel equivalence are not asserted.
- R7-2: **Real semantic loss**, not a diagnostic false positive. Actual fixture `3997:46310` is HORIZONTAL/HUG, height 56; child `3997:46313` is vertical FILL, width 55 and aspect ratio 1. Existing generated percentage height has no definite containing-block height and resolves as auto. Headless Edge reproduction: parent 56 / old child 55 / stretch child 56; when parent becomes 80, old child remains 55 while stretch child becomes 80. HUG horizontal parents now use child `alignSelf="stretch"` without percentage height. Verification checks the single flex line's independent intrinsic height and rejects overriding child constraints. No FILL axis was pinned to px.
- R7-3: Shared success/error `server: {version, buildId, commit}` identity; covers Devup errors, router errors, parameter-deserialization `isError` results, and resource errors. The stdio regression launches `CARGO_BIN_EXE_devup-mcp` from this build.
- R7-4: SECTION plugin errors now carry actual node/type/stage/ancestor SECTION and corrected URL. Nested non-candidate nodes receive the SECTION URL without an invalid frameIds guess. Legacy errors explicitly state unknown ancestor rather than inventing one. Collection adds file/node/stage context. Expired artifacts explain original URL + refresh and omission of artifactId.
- R7-5: Artifact reprojection defaults to stored selection; explicit selection takes precedence. Auto resource delivery of debug outputs includes resource-reading and inline-reprojection instructions, with the inline size limit.
- R7-6: `text-auto-size` stays lossy and `propertyMappingVerified=false`; explicit verification state is `unverified`, reason `font-metrics-not-measured`.

## Regression evidence

Initial R7 layout regressions failed before their fixes; fixture setup was corrected to include real `inferredAutoLayout` and explicit null max constraints before accepting the reproductions. R7 error tests first failed with missing identity/recovery. Selection/debug tests first failed with selection_required/missing nextAction. The actual preserved WQUW-118 payload `fixtures/r2/wquw-118-payload.json` was reprojected with frame 3997:46277; all three blocking nodes are now tested for absence of lossy diagnostics. Its capture content hash matches the supplied sender-2 response (`95cbfb6e2b03886ef81ba7031325629aeff398489192d99ec3e6ddb988b0c57b`).

Review regressions first reproduced false approvals for a flex-growing fixed sibling, a replaced element and mixed horizontal strokes; a nested TEXT SECTION correction, local protocol identity omissions and relayed plugin expiry also failed before correction. All regression tests are retained. Revalidation retains previously claimed dimensions even if the emitted tag changes, so replacing a Box cannot silently remove its coverage obligation.

First full suite found missing collection manifest field `layoutWrap`; the manifest was corrected, without changing the test. Golden snapshot changes: **0**.

## Local build provenance

`cargo metadata --no-deps --format-version 1` reports target directory:
`C:/Users/owjs3/orca/workspaces/devup-mcp/r7-main-axis-error-identity/target`.
`CARGO_TARGET_DIR` was unset and was never set. Compilation and execution logs name this worktree's crates and `target/debug/deps/r7_layout-ff7b26681661430c.exe`, and the stdio test executes this target's devup-mcp binary. Baseline-head dirty development identity during verification: `ffea11d12c3f-dirty`.

자기 target 에서 검증했고 실행된 테스트가 내 것임을 확인했다.

Build/test/clippy commands use `-j 2`. Non-build `fmt`, `metadata`, and `clean` do not accept that option. No separate target directory was configured.

Final validation: **727 passed / 0 failed / 2 ignored**, including 21 added regressions; **clippy 0**, **fmt 0**, **golden changes 0**. See [verification record](R7-verification.txt) for executed test names and binary SHA-256, and [local protocol responses](R7-protocol.jsonl) for raw identity-bearing error envelopes.

Delivery: local commit without push, followed by `cargo clean` to hand back this target.
