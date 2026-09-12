# Feature trace implementation and verification

## Delivered behavior

`devup_feature_trace` is a read-only cross-layer slice and acceptance matrix. It reuses project-context UI/API/DB inventories, stack-diff route/model/client parsers and source-ownership rules, and the Figma code generator/source map/fingerprints. It never interprets requirement or acceptance text to invent an anchor.

The response contains an evidence-bearing chain, concrete artifact ownership and safe authored repair targets, ranked UI candidates with match reasons and inventory authority, separate request/response field differences, five required-state rows, preserved acceptance criteria, diagnostics, and named truncation metadata. File-level heuristics remain medium confidence; no high confidence is emitted. Design-name matching produces candidates, not a claimed design-to-code binding.

## Input schema

The exact published MCP JSON Schema is [input-schema.json](input-schema.json), captured from this worktree's `target/debug/devup-mcp.exe` through `tools/list` (9 tools).

All properties are optional at schema level; runtime requires at least one nonblank explicit anchor. Unknown properties are rejected. String properties accept null: `projectRoot`, `routePath`, `figmaNodeId`, `artifactId`, `operationId`, `apiPath`, `method`, `componentPath`, `tableName`, `requirement`, `componentTsx`. `acceptanceCriteria` is an array of strings with default `[]`. `maxItems` is a nullable integer in 1..200, runtime default 100.

Accepted anchors are `routePath`, `figmaNodeId`, `artifactId`, `operationId`, `apiPath` together with `method`, `componentPath`, and `tableName`. Anchorless prose returns `status: REFUSED` and lists these accepted anchors. API methods are checked case-insensitively against the HTTP method set. Route/component/table names are literal; component paths use project-relative forward slashes. No parameter substitution or semantic path guessing occurs.

`componentTsx` is an optional caller-supplied export tied to a Figma anchor; this association is explicitly UNVERIFIED. An `artifactId` instead reuses the cached snapshot and existing code generator, preserving property source-map evidence and design fingerprints. A node ID alone cannot retrieve design content and reports the missing export. Missing/expired artifacts report a reason without network acquisition or writes.

Field comparison extracts literal JSX `name` and `data-field` bindings, not visual labels. It compares declared top-level fields separately against request content and success-response content. Local schema references, allOf and array item schemas are followed with a depth cap. External/missing/cyclic references, union schemas, multiple success/media alternatives and open-ended shapes are disclosed as unverified; differences in an unverified comparison are only candidates. This does not prove type compatibility or runtime serialization.

States are `loading`, `error`, `empty`, `validation`, `authorization`. Literal `data-state` declarations provide specified/implemented evidence; scoped App Router loading/error boundary components provide implementation evidence. Missing evidence is unknown, never presumed absent. Presence describes structural evidence and does not claim a runtime test passed. Acceptance-criterion prose remains uninterpreted and UNVERIFIED.

## Bounds and ownership

Each output array is capped by `maxItems`; omitted counts use named paths. The full compact JSON payload is bounded to 65536 UTF-8 bytes before the MCP envelope/server identity. If necessary, large payload members are removed as units and named under `truncation.omitted` with the `maxBytes` cap; individual hop reasons are not silently cut. Direct source reads and supplied component TSX are capped at 2097152 bytes; schema traversal depth is 16. UI inventory caps, exclusions, diagnostics and unparsed counts remain visible under `inventoryEvidence`.

`sourceOwnership` is copied unchanged from stack_diff. Concrete generated OpenAPI artifacts point only to a unique authored route file when one is available. Generated-tree patterns from that same ownership record also protect authored-looking files under migrations. UI inventory alone cannot certify whether a local file was generated, so frontend edit targets remain null with an explicit ownership uncertainty rather than guessing. Reuse ranking does not authorize editing candidates.

## Changes outside the new module and sibling tests

- `server/tools.rs`: appended `FeatureTraceInput`.
- `server/mod.rs`: one module declaration and one appended thin tool method; no tracing logic moved into the router.
- `server/stack_diff.rs`: `parse` module became `pub(super)`; the existing functions `attach_source_ownership`, `extract_vespera_route_attributes`, `route_url_prefix`, `join_route_url`, `route_comparison_key`, and `client_path_key` became `pub(super)`.
- `server/stack_diff_parse.rs`: `RouteMapping`, its `unresolved` field and `mentions` method, and `route_mapping` / `client_references` widened from parent-only visibility to `pub(in crate::server)` so the sibling can reuse them. Their extraction logic is unchanged.
- No visibility change to project_context or its UI parser was needed; the existing `run` and `relative_display` functions were sufficient.
- `stdio_smoke.rs`, `stdio_schema_compat_smoke.rs`, and `stdio_tools.rs`: narrow publication-list updates from eight tools to nine, including the new tool. No snapshots were changed.

## Links that remain unverified

- Requirement/acceptance prose to any code or anchor: semantic interpretation is deliberately not performed.
- Design JSX names to a particular code component: names and props rank candidates but establish no authoritative binding.
- Caller-supplied export to a Figma node: only the caller asserts the association.
- Cached design to current live Figma: no live freshness comparison is performed; cached fingerprints are supplied as evidence.
- Missing/expired artifacts or a bare node ID to design fields: no design payload is available.
- Runtime rendering, dynamic imports/calls, wrappers, helper-only calls outside the inventory slice, package export maps, macro expansion, cfg/reexports and merged-app route registration: the reused bounded static parsers do not evaluate them.
- Per-handler model/column use in a file with several handlers: the reused mapping is file-level, so no particular handler receives that mapping as resolved.
- Model references to actual serialization or type-alias usage: syntactic file references are not execution/type-checking proof.
- API references with absent or ambiguous operations/spec authorities: no operation is guessed. The shared client parser also loses some call-method information, so method compatibility is never claimed by a resolved identifier match.
- Unresolved/alternative schema shapes and dynamic design bindings to definitive contract gaps: report reasons and candidate differences only.
- Absence of a required state, runtime state coverage, or frontend generator ownership without explicit provenance: missing evidence remains unknown.
- Completeness after a scan/response cap or parse/read failure: exclusions, named caps and diagnostics qualify the result.

## Verification

Environment for all builds: `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`, two build jobs. `CARGO_TARGET_DIR` was absent and never set. All builds used this worktree's own target.

Measured base commit: `43e0a576f246233c725c038298027ef51fb1bf2c`.

`cargo test --workspace -j 2 --no-fail-fast` on the base: **976 passed / 0 failed / 2 ignored**, exit 0. The base binaries were built before the new module was connected; unconnected test/report files did not participate. The baseline emitted Windows linker import-library informational warnings; these are separate from Clippy diagnostics.

Initial behavioral RED against an empty `run` stub: **0 passed / 8 failed / 0 ignored**. A fixture syntax typo was corrected before recording this behavioral result. The failing tests were:

1. `route_anchor_resolves_chain_to_selected_columns`
2. `absent_api_is_unverified_with_reason`
3. `design_names_field_missing_from_request`
4. `generated_artifacts_are_never_edit_targets`
5. `anchorless_requirement_is_refused`
6. `reuse_is_ranked_and_explained`
7. `states_without_proof_are_unknown`
8. `named_cap_reports_omitted_candidates`

Additional RED runs recorded before corresponding fixes: state evidence (11 passed / 1 failed), concrete ownership (13 / 1), boundary/method/path normalization regressions (13 / 3), generated migrations (16 / 1), and database diagnostic forwarding (17 / 1). The final workspace suite includes **18 feature-trace tests**, covering the original eight behaviors plus cached provenance, byte bounds, schema references, shared handler ambiguity, state evidence, scoped boundaries, method filtering, concrete ownership migrations protection and preserved database parse errors.

Final validation used HEAD `43e0a576f246233c725c038298027ef51fb1bf2c` plus the working-tree additions subsequently committed with this report; no code edits are made after these checks.

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets -j 2 -- -D warnings`: exit 0, zero warnings.
- `cargo test --workspace -j 2 --no-fail-fast`: **994 passed / 0 failed / 2 ignored**, exit 0; **18 added tests** over the measured base.
- `cargo test -p devup-mcp --test stdio_smoke`: **2 passed / 0 failed**, exit 0; the fresh binary lists the new tool among nine tools.
- `cargo insta test --workspace --all-features --check`: exit 0, **994 passed / 0 failed / 2 ignored**, and `no snapshots to review`; no snapshot files changed.

Machine-readable command exit codes are retained in [check-results.json](check-results.json). Raw execution logs remain in this worktree under `docs/feature-trace` (git-ignored); the RED names and measured results above are retained in the committed report. The commit ID and post-commit `cargo clean` result are delivered in the worker completion receipt; nothing is pushed.
