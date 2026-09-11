# R15 — evidence wording accuracy

## Change and scope

The external WQUW-118 REPORT section 6, projectionEvidence.json, target-evidence.json and audit-result.json were read only. The observed 3997:46668 maskPos=center/maskSize=contain mismatch is reproduced with the repository's pinned R14 asset snapshot.

The old evidence selector used `export_offset(node).is_some()` without the style writer's ABSOLUTE/free-layout guards. It therefore described an available boundary as if the boundary override had executed. The same selector also mislabelled IMAGE FIT objectFit=contain; a separate red test reproduces it.

`StyleDerivations::push` now writes both the unchanged CSS value and its typed generating route. Boundary writes replace both records; maskRepeat retains the policy route. Evidence replay requires the responsible style stage and exact recorded value. It derives sourceFields/calculation and derivationPath from that route; missing or stale records return no proof and use the existing PROPERTY_UNMAPPED audit. No duplicate geometry predicate is used to guess the route.

The five routed properties are maskRepeat, maskSize, maskPos, objectFit, objectPos. Other property_derivations arms were inspected: their descriptions cover the named layout/text/effect/animation stage or explicitly describe combined policies. This review found no further concrete wrong-path example; it is not a claim that every source calculation or browser result is measured or verified. The route recorder can be extended at other property writers when adding more precise route claims.

No layout condition, generated CSS value, sourceMap rule, R6–R14 classification or guidance was changed. Browser pixels, responsive equivalence, asset bytes and SVG/CSS composition were not measured.

## Regression sequence

- `cargo test -j 2 -p devup-mcp-devup-ui --test r15_evidence_wording`: four failing tests before implementation (red-mask.txt). ABSOLUTE, free-layout and observed masks expose wrong calculation; in-flow override exposes missing machine-readable route. Synthetic parent setup was corrected before this saved red run to prevent unrelated asset folding.
- `cargo test -j 2 -p devup-mcp-devup-ui --lib r15_image_fit`: IMAGE FIT wrong-boundary explanation fails before implementation (red-image.txt).
- Same regression tests after the fix: five passed (green.txt), including policy vs boundary distinction and unchanged literal values, and maskRepeat remaining policy after other properties are overridden.

## Verification

`cargo test --workspace -j 2 --no-fail-fast`: exit 0; **811 passed / 0 failed / 2 ignored**, baseline 806 plus five new R15 tests (workspace-final.txt). R13 provenance and R14 absolute-evidence suites passed unchanged. All existing TSX goldens passed unchanged. `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: exit 0, no warnings/errors (clippy.txt). `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: 12 passed, 0 failed (js.txt). `cargo fmt --all -- --check`: exit 0 (fmt.txt).

The local workspace/target is `C:/Users/owjs3/orca/workspaces/devup-mcp/r15-evidence-wording/target`. CARGO_TARGET_DIR was not set. The installed devup-mcp MCP was not used. Test logs identify this checkout as the compiled source and show target/debug/deps executables. local-target.json records the resolved target, executable path and SHA-256; local-executable-list.txt lists the four new R15 integration tests, and local-executable-run.txt records successful direct execution of the observed-fixture test. 자기 target 에서 검증했고 실행된 테스트가 내 것임을 확인했다.

## Golden changes

`crates/devup-mcp-devup-ui/tests/snapshots/wquw_151__wquw_151_proofread_diagnostics.snap`: add `derivationPath=svg-mask-policy` to nine existing mask evidence records so the generating policy is machine-readable; no other JSON content changed (verified by removing exactly those nine added lines and comparing the decoded text). All TSX goldens are unchanged.

First workspace run: 810 passed / 1 failed / 2 ignored. Its sole failure was this reviewed diagnostic golden addition (workspace-first.txt).

## Delivery

Commit the reviewed changes and verification artifacts, then run `cargo clean` against the verified local target. No push. Cleanup completion is reported in the final session response after the commit.
