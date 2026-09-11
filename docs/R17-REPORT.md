# R17: FILL width and response clarity

Base: `integration/r6`, `f6bfa7cedc3f2be2c81c3baf26de400f3d419160`.

## Evidence read

Read the requested R17 brief and the original WQUW-147, WQUW-166, WQUW-165 and WQUW-119 REPORT sections. Also read WQUW-166 `devup-context.json` and `devup-validation.json`. These girok-space worktrees were read only.

The WQUW-147 `3831:10720` report describes a FILL Center inside a column with `alignItems=flex-end`, missing width and horizontal sizing provenance despite exact/complete claims. R6/R7 `implicit_css_verification` already rejects non-stretch alignment. The bypass was structural: `LAYOUT_FIELDS` included vertical sizing but omitted horizontal sizing, and ordinary FILL container widths were excluded as captured dimensions. Separately, the final delivery audit only enumerated attributes that remained in generated TSX.

## Implementation contract

- R17-1: Treat horizontal FILL as a source layout obligation, independently of emitted attributes. Explicit fluid width, verified implicit cross-axis stretch and verified main-axis remaining-space allocation are checked against emitted CSS. Unknown sizing remains uncovered/lossy. No FILL-to-pixel replacement. Preserve the R6/R7 minimum, parent size, alignment, sibling and positioning checks. Source-obligation gaps must also keep final `mappingComplete=false`.
- The source-to-output mutation matrix removes mappings for mode, width, horizontal FILL and each padding side, while the R13 output-to-source contract continues to run. A separate final-output mutation deletes an attribute, and another replaces a fluid width by captured pixels.
- R17-2: `displayVersion` and `identityGuidance` accompany `version`, `commit`, `buildId`; tool/server instructions identify deployments by commit/buildId.
- R17-3/8: Preserve `tokenCount` as the pre-filter compatibility field; add total/matched distinct-name counts and per-category source/filter states. Document literal, case-sensitive substring matching and scope-specific fields.
- R17-4: Each matching token carries matching modes and whether all category modes match. Partial-mode matches require a human choice rather than automatic replacement.
- R17-5: Keep UTF-8 byteRange; add 1-based Unicode scalar line/column, bounded snippet and optional caller-provided sourceName.
- R17-6: referencePng selection errors retain the error classification and provide direct frame URL correction arguments. Manual source correction is `manual-fix-required`; states with no recoverable source metadata retain their existing classification.
- R17-7: Preview states distinguish available, no-text, traversal/character truncation, byte-budget exhaustion and unavailable metadata in older cached captures. Candidate text inspection calls and frame/output-budget-aware continuation batches are returned together.

## Conditional expression investigation

Read the local devup-ui checkout `du1-worktree-css-path-leak` at `b00367166683bf0e9d61aab2419e7de328f6c7fb` without changing it.

- `packages/eslint-plugin/src/rules/css-utils-literal-only/index.ts`: identifiers in conditional tests are exempt; undefined is permitted.
- `libs/extractor/src/visit.rs`: css() calls use `extract_style_from_expression` and then `gen_class_names`.
- `libs/extractor/src/extractor/extract_style_from_expression.rs`: conditional expressions recursively extract consequent and alternate into `ExtractStyleProp::Conditional`; ignored identifiers include undefined.
- `libs/extractor/src/gen_class_name.rs`: conditional styles produce conditional class selection.
- Existing extractor tests include conditional css selector values in `libs/extractor/src/lib.rs::test_conditional_expression_with_selector`.

Therefore `css({backdropFilter: backdropVariant === 'strong' ? 'blur(4px)' : undefined})` is a validator false positive. The validator now allows finite css() branches, including nested branches, and still rejects arbitrary variable/function-call value leaves. This does not relax globalCss()/keyframes() to permit runtime class selection.

## RED evidence

- `r17-red-fill.log`: 0 passed / 5 failed before library fixes: width omission, absent horizontal source obligation, missing mode metadata, missing Korean positions, finite conditional false positive.
- `r17-red-all.log`: context and SECTION batch regressions failed; referencePng and manual recovery regressions failed.
- `r17-red-js.log`: existing 12 passed; added preview exhaustion regression failed.
- `r17-red-stdio.log`: the locally built `target/debug/devup-mcp.exe` failed assertions for readable identity, sourceName, manual recovery, matched count and filter schema explanation.

Logs are local ignored files. Final verification and golden reasons follow below.

## Reviewed golden changes

1. `fixtures/devup-figma-plugin/snapshots/codegen/upstream-codegen-223-4dc8092992.snap`: four horizontal FILL children (heading Text, two VStacks and divider) under a non-stretch column now emit `w="100%"`.
2. `crates/devup-mcp-devup-ui/tests/snapshots/wquw_151_frames__wquw_151_frame_3879_35503.snap`: the FILL Center inside the right-aligned column now emits `w="100%"`.
3. `crates/devup-mcp-devup-ui/tests/snapshots/wquw_151__wquw_151_proofread_source_map.snap`: add 26 verified horizontal FILL source mappings, using existing explicit width or proven implicit stretch.
4. `crates/devup-mcp-devup-ui/tests/snapshots/wquw_151__wquw_151_proofread_diagnostics.snap`: expose `3879:35547#layoutSizingHorizontal` as lossy because the unchanged R7 allocation proof rejects its gap mismatch.

Only two TSX goldens changed. No pixel width replacements. Original snapshot metadata is retained.

## Existing regression contracts

- R6 centered-parent negative proof now removes the newly repaired fluid width from the final output and still requires both width and horizontal FILL to be uncovered; the negative test was not removed.
- WQUW-151 now asserts exactly one newly uncovered horizontal source field, its `gap-mismatch` reason, lossy=1 and strict rejection; node/text/variable/typography/asset and absolute-position proofs remain checked.
- R16 keeps the original real frame in both inline/resource delivery: exactly 18 newly inspected horizontal FILL fields are lossy, with mappingComplete=false. An additional fully specified fixture retains every prior complete/exact and mapping-method assertion. This tightens coverage; it does not promote uncertain CSS to verified.
- R2/R4 retain modal width's verified 100% containing-block proof. Source FILL obligations add 17 verified layout fields (117 -> 134). Component-reference uncovered count is 39 instead of 40 because the fluid width repair resolves one missing mapping; the aggregate component output still has lossy=40 after the new source check. Modal inline TSX has an independently uncovered FILL source obligation and therefore reports lossy rather than exact.
- R8 keeps the 1 MiB inline boundary. The 3-frame/9-output-unit fixture now exceeds it due to newly disclosed source obligations. Its test asserts the refusal and same-artifact recovery, reads all seven resource payloads (shared snapshot and three TSX/sourceMap pairs), verifies snapshot equality and retains all original generated-output checks.
- R12's automatic correction positive fixture now has the same exact token in both modes; the original differing-mode behavior has a separate no-auto-replacement regression. No ambiguity or partial-mode match is promoted to a safe automatic change.
- Review identified two unrelated-axis provenance cases (`h` and vertical `flex`). Both were reproduced RED before fixing. Explicit horizontal flex mappings now require a horizontal parent; horizontal implicit stretch remains a distinct mapping.

The original 3831 capture fixture contains the 15-node descendant subtree of 3831:10708 with raw fields retained. Its source is WQUW-147's checked-in `docs/jira-audit/figma-raw.json` at `e248725151f824e3a94e7f112bea0e601bd7e580`; the other repository was only read.

## Validation environment

All Cargo compilation used `-j 2` and this worktree's default `target`; `CARGO_TARGET_DIR` was never set. To avoid disk exhaustion from debug symbols, checks used `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0` consistently. No installed devup-mcp MCP connection was used.

A direct stdio session launched the absolute path `C:\Users\owjs3\orca\workspaces\devup-mcp\r17-fill-width-clarity\target\debug\devup-mcp.exe` and checked identity, Korean sourceName, manual recovery, counts, filter schema and direct-frame PNG correction. The tested binary SHA256 was `8748471690f55f79b146e7fd39c707849d0de16fed2b0b581a142605cf89c1bf`, with pre-commit build identity `0.4.4+f6bfa7cedc3f-dirty`. The stdio integration tests also launch Cargo's `CARGO_BIN_EXE_devup-mcp`.

## Final verification

- `cargo test --workspace -j 2 --no-fail-fast`: **851 passed / 0 failed / 2 ignored**, exit 0; includes 22 passing R17 Rust tests. Baseline was 829/0/2.
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs`: **13 passed / 0 failed**; baseline was 12.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: exit 0, **0 warnings / 0 errors**.
- `cargo fmt --all -- --check`: exit 0, **0 differences**.
- `git diff --check`: exit 0.
- The upstream golden manifest checksum was updated only for the reviewed codegen-223 snapshot; the corpus consistency test passes without weakening its checksum check.

자기 target에서 검증했고 실행된 테스트가 내 것임을 확인했다. The final test log contains `target\debug\deps\r17_fill-...exe` and all 13 R17 library test names, alongside the actual worktree build paths. The direct stdio binary path and hash are recorded above. Local logs: `r17-verified-test.log`, `r17-final-js.log`, `r17-clippy.log`, `r17-fmt.log`, `r17-green-stdio.log`.

No push is authorized or performed. After committing, the worktree's Cargo target is cleaned as requested.
