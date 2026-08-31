# Figma Linked Screen Exploration and Used Resources Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn a Figma link to a requirement heading into an explicit, ranked set of related screen candidates, then convert the selected screen with every used variable, style, mixed-text child, and typography token intact.

**Architecture:** A bounded Figma Plugin API projection returns compact spatial metadata while Rust classifies and ranks candidates deterministically. Collection uses a three-way `ResourceScope`: exploration and search collect no resources, UI conversion recursively discovers and resolves only referenced local or remote resources, and JSON export retains the full local file catalog. Live text collection adds styled segments so root and nested `Text` nodes share the same resolved typography map.

**Tech Stack:** Rust 1.88, edition 2024, serde/serde_json, Tokio, rmcp, embedded Figma Plugin API JavaScript, cargo-insta, Cargo-only build and test workflow.

**Spec:** `docs/superpowers/specs/2026-08-31-figma-linked-screen-exploration-design.md`

## Global Constraints

- All Figma access remains read-only and uses the existing direct OAuth or official-host handoff boundary.
- `devup_figma_explore` returns all plausible screens with reasons; it never silently chooses one when candidates are ambiguous.
- Projection node count, text preview length, resource batch size, and pending host calls have fixed bounds.
- Search and exploration use `ResourceScope::None`, UI conversion uses `ResourceScope::Used`, and JSON export uses `ResourceScope::File`.
- Used-resource discovery recursively scans the complete selected snapshot, including styled text segments, and resolves exact IDs without guessing names.
- A missing or inaccessible resource falls back to the resolved raw value and emits `DEVUP_RESOURCE_UNRESOLVED` without discarding successfully resolved resources.
- The live regression fixture contains no OAuth token, request header, email, user handle, callback payload, or other account identifier.
- The pinned 268-case JavaScript plugin fixture corpus remains a mandatory compatibility gate.
- Each task follows red-green-refactor: add one failing test, observe the intended failure, implement the smallest behavior, and rerun the focused test before committing.

## File Structure

### Create

- `crates/devup-mcp-figma/src/explore.rs` — spatial classification, grouping, ranking, and public result contract.
- `crates/devup-mcp-figma/src/resources.rs` — recursive used-resource reference discovery and unresolved-resource diagnostics.
- `crates/devup-mcp-figma/src/scripts/explore.js` — bounded page-neighborhood projection.
- `crates/devup-mcp-figma/src/text_segment_manifest.json` — exact fields requested from `getStyledTextSegments`.
- `crates/devup-mcp-figma/tests/explore.rs` — classification, boundary, ordering, and projection contract tests.
- `crates/devup-mcp-figma/tests/used_resources.rs` — local, remote, duplicate, styled-segment, and missing-resource tests.
- `crates/devup-mcp/tests/figma_explore.rs` — MCP direct/host continuation and schema tests.
- `crates/devup-mcp/tests/fixtures/wquw-151-neighborhood.json` — sanitized compact live projection around node `3879:35481`.
- `crates/devup-mcp-devup-ui/tests/fixtures/wquw-151-proofread.json` — sanitized selected subtree and resolved resource payload for node `3879:35518`.

### Modify

- `crates/devup-mcp-figma/src/lib.rs` — export exploration and resource-scope interfaces.
- `crates/devup-mcp-figma/src/collector.rs` — plan projection/resource phases and merge partial results.
- `crates/devup-mcp-figma/src/upstream.rs` — embed and invoke explore, styled-text, and used-resource scripts.
- `crates/devup-mcp-figma/src/variables.rs` — merge exact-ID resource batches and completeness metadata.
- `crates/devup-mcp-figma/src/scripts/snapshot.js` — capture styled text segments from the manifest.
- `crates/devup-mcp-devup-ui/src/codegen/text.rs` — pass text-style tokens through nested segment rendering.
- `crates/devup-mcp-devup-ui/src/codegen/component.rs` — preserve shared token maps for all text descendants.
- `crates/devup-mcp-devup-ui/tests/fixture_snapshot.rs` — assert the live-derived proofread snapshot.
- `crates/devup-mcp/src/server/tools.rs` — public exploration input schema.
- `crates/devup-mcp/src/server/handoff.rs` — pending host exploration operation.
- `crates/devup-mcp/src/server/mod.rs` — register exploration and select the correct resource scope per tool.
- `crates/devup-mcp/tests/stdio_tools.rs` — six-tool list and schema assertions.
- `crates/devup-mcp/tests/source_orchestration.rs` — source parity and collection policy assertions.
- `crates/devup-mcp/tests/live_figma_contract.rs` — ignored authenticated WQUW-151 smoke.
- `README.md` — heading-link exploration and exact-conversion workflow.
- `.changepacks/changepack_log_figma_remote_mcp.json` — release notes for all three crates.

---

### Task 1: Define Resource Scope and Recursively Discover Used References

**Files:**
- Create: `crates/devup-mcp-figma/src/resources.rs`
- Create: `crates/devup-mcp-figma/tests/used_resources.rs`
- Modify: `crates/devup-mcp-figma/src/lib.rs`
- Modify: `crates/devup-mcp-figma/src/collector.rs`

**Interfaces:**
- Produces: `ResourceScope::{None, Used, File}`.
- Produces: `UsedResourceRefs { variable_ids, styles, occurrences }`.
- Produces: `ResourceOccurrence { node_id, field, resource_id, resource_kind }`.
- Produces: `collect_used_resource_refs(chunks: &[SnapshotChunk]) -> UsedResourceRefs`.
- Replaces: `CollectionRequest::include_variables` with `CollectionRequest::resource_scope`.

- [ ] **Step 1: Write failing recursive-scanner tests**

Cover variable aliases under every `boundVariables` object, variable references inside `styledTextSegments`, all six style fields (`textStyleId`, `fillStyleId`, `strokeStyleId`, `effectStyleId`, `gridStyleId`, `backgroundStyleId`), mixed local/remote-looking IDs, duplicates, non-resource `id` fields, nulls, mixed sentinels, and deterministic ordering. Assert arbitrary JSON keys named `id` are not collected.

```rust
let refs = collect_used_resource_refs(&chunks);
assert_eq!(refs.variable_ids, ["VariableID:12:34", "VariableID:56:78"]);
assert_eq!(refs.styles[0].style_type, ResourceStyleType::Text);
assert_eq!(refs.occurrences[0].node_id, "3879:35518");
```

- [ ] **Step 2: Run and observe the missing API**

Run: `cargo test -p devup-mcp-figma --test used_resources scanner`

Expected: compile failure because `resources` and `ResourceScope` do not exist.

- [ ] **Step 3: Implement typed scope and field-aware recursive discovery**

Use ordered maps/sets for stable batches. Enter variable-ID collection only below a `boundVariables` key, map each supported style field to its Figma style type, record the owning node ID and JSON field for diagnostics, and leave raw snapshot values untouched.

- [ ] **Step 4: Migrate collection request constructors**

Set the default to `ResourceScope::None`; explicitly select `None` in metadata/search paths while preserving the existing file-catalog behavior behind `File`. Do not enable any resource call for a compact projection.

Run: `cargo test -p devup-mcp-figma --test used_resources scanner`

Expected: scanner tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp-figma/src/resources.rs crates/devup-mcp-figma/src/lib.rs crates/devup-mcp-figma/src/collector.rs crates/devup-mcp-figma/tests/used_resources.rs
git commit -m "refactor: model figma resource collection scopes"
```

### Task 2: Resolve Exact Used Local and Remote Resources

**Files:**
- Modify: `crates/devup-mcp-figma/src/upstream.rs`
- Modify: `crates/devup-mcp-figma/src/collector.rs`
- Modify: `crates/devup-mcp-figma/src/variables.rs`
- Modify: `crates/devup-mcp-figma/tests/used_resources.rs`
- Modify: `crates/devup-mcp-figma/tests/collector.rs`
- Modify: `crates/devup-mcp-figma/tests/upstream_contract.rs`

**Interfaces:**
- Produces: `ReadToolCall::used_resources(variable_ids, styles)` with fixed-size batches.
- Produces: `UsedResourceBatchResult { variables, styles, unresolved }`.
- Produces: `UnresolvedResource { id, kind, reason }` and `DEVUP_RESOURCE_UNRESOLVED` payload diagnostics.
- Preserves: `VariableCatalog` and style-consumer pagination only for `ResourceScope::File`.

- [ ] **Step 1: Write failing exact-ID lookup and merge tests**

Assert the embedded script calls `getVariableByIdAsync` and `getStyleByIdAsync`, does not call `getLocalVariablesAsync` or `getStyleConsumersAsync` in `Used` mode, retains the actual `remote` flag/name/type, merges successful entries when another ID is null, emits one stable unresolved item per occurrence, and marks completeness partial without inventing a token.

- [ ] **Step 2: Run and observe the missing used-resource read call**

Run: `cargo test -p devup-mcp-figma --test used_resources exact_id`

Run: `cargo test -p devup-mcp-figma --test upstream_contract used_resource`

Expected: failures because no exact-ID batch script or merge path exists.

- [ ] **Step 3: Implement bounded direct resource batches**

Generate batches only after the node snapshot is complete. Fetch variables/styles independently so a null or inaccessible ID is represented in `unresolved` rather than aborting the successful portion. Preserve the collector version guard and existing maximum pending-call bound.

- [ ] **Step 4: Merge payload resources and diagnostics**

Build the same `variables`/`styles` payload shape consumed by `with_payload_tokens`, append occurrence-aware diagnostics with node ID, field, resource ID, and reason, set `usedRemoteComplete` true only after every discovered ID was attempted, and keep raw node values as codegen fallback.

Run: `cargo test -p devup-mcp-figma --test used_resources`

Run: `cargo test -p devup-mcp-figma --test collector resource_scope`

Expected: all exact-ID and partial-completeness tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp-figma/src/upstream.rs crates/devup-mcp-figma/src/collector.rs crates/devup-mcp-figma/src/variables.rs crates/devup-mcp-figma/tests/used_resources.rs crates/devup-mcp-figma/tests/collector.rs crates/devup-mcp-figma/tests/upstream_contract.rs
git commit -m "feat: resolve used figma resources by id"
```

### Task 3: Capture Styled Text Segments and Preserve Nested Typography

**Files:**
- Create: `crates/devup-mcp-figma/src/text_segment_manifest.json`
- Modify: `crates/devup-mcp-figma/src/scripts/snapshot.js`
- Modify: `crates/devup-mcp-figma/src/upstream.rs`
- Modify: `crates/devup-mcp-figma/tests/upstream_contract.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/text.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/component.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/tests.rs`

**Interfaces:**
- Embeds: `TEXT_SEGMENT_MANIFEST` with characters/range, fills, style IDs, font, decoration, case, line-height, letter-spacing, list, indentation, and hyperlink fields.
- Changes: `render_text_children(view, text_style_tokens, variable_tokens, depth)`.
- Changes: `typography_props(segment, text_style_tokens, variable_tokens)`.

- [ ] **Step 1: Write the nested typography regression test first**

Create a `TEXT` snapshot whose root uses `body`, whose `[1. 이름]` segment uses `bodySemibold`, and whose colors are variable-bound. Assert generated TSX contains nested `Text typography="bodySemibold"`, preserves surrounding spaces and `<br />`, and omits redundant raw `fontSize`/`fontWeight` on resolved segments.

- [ ] **Step 2: Run and observe the current typography loss**

Run: `cargo test -p devup-mcp-devup-ui nested_text_style_uses_typography -- --exact`

Expected: assertion failure because nested segments receive only variable tokens and render raw font properties.

- [ ] **Step 3: Thread text-style tokens through segment rendering**

Pass the same text-style map used for root `Text` into every segment. Resolve `textStyleId` before raw font properties; only segment differences produce nested `Text`, while identical runs retain the existing text/newline structure.

Run: `cargo test -p devup-mcp-devup-ui nested_text_style_uses_typography -- --exact`

Expected: pass.

- [ ] **Step 4: Write and implement the live snapshot contract**

Add an upstream contract test that evaluates the embedded snapshot script against a mocked `TEXT` node and asserts one exact `getStyledTextSegments` call using the manifest plus serialized `styledTextSegments`. Then embed the manifest, guard the method by node type/capability, and collect its result without changing the general node-field manifest.

Run: `cargo test -p devup-mcp-figma --test upstream_contract styled_text`

Run: `cargo test -p devup-mcp-devup-ui`

Expected: all styled-text and codegen tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp-figma/src/text_segment_manifest.json crates/devup-mcp-figma/src/scripts/snapshot.js crates/devup-mcp-figma/src/upstream.rs crates/devup-mcp-figma/tests/upstream_contract.rs crates/devup-mcp-devup-ui/src/codegen/text.rs crates/devup-mcp-devup-ui/src/codegen/component.rs crates/devup-mcp-devup-ui/src/codegen/tests.rs
git commit -m "fix: preserve figma styled text typography"
```

### Task 4: Build the Bounded Spatial Exploration Projection

**Files:**
- Create: `crates/devup-mcp-figma/src/explore.rs`
- Create: `crates/devup-mcp-figma/src/scripts/explore.js`
- Create: `crates/devup-mcp-figma/tests/explore.rs`
- Modify: `crates/devup-mcp-figma/src/lib.rs`
- Modify: `crates/devup-mcp-figma/src/upstream.rs`
- Modify: `crates/devup-mcp-figma/src/collector.rs`
- Modify: `crates/devup-mcp-figma/tests/upstream_contract.rs`
- Modify: `crates/devup-mcp-figma/tests/collector.rs`

**Interfaces:**
- Produces: `ExploreReadOptions { projection_limit, text_preview_limit }`.
- Produces: `ExploreNode { node_id, name, node_type, bounds, child_count, text_preview }`.
- Produces: `ExploreKind::{Screen, Heading, Annotation, Container, Unknown}`.
- Produces: `ExploreResult { anchor, group, candidates, truncated }`.
- Produces: `explore_projection(projection, target, limit) -> Result<ExploreResult, ExploreError>`.
- Adds: `BuiltinScript::ExploreSnapshot` and `ReadToolCall::explore_snapshot`.

- [ ] **Step 1: Write failing Rust classification and grouping tests**

Cover a wide/short heading, 360px screen frames, annotation text, a container with screen children, the next-heading cutoff, candidates with duplicate names, non-overlapping distant frames, WQUW-like horizontal rows, stable top/left/node-ID ordering, score reasons, caller limit, and projection truncation propagation.

```rust
let result = explore_projection(&projection, &target, 50)?;
assert_eq!(result.anchor.kind, ExploreKind::Heading);
assert!(result.candidates.iter().any(|candidate| candidate.node_id == "3879:35518"));
assert!(result.candidates.iter().all(|candidate| !candidate.selection_reasons.is_empty()));
```

- [ ] **Step 2: Run and observe missing exploration types**

Run: `cargo test -p devup-mcp-figma --test explore classification`

Expected: compile failure.

- [ ] **Step 3: Implement deterministic Rust classification and ranking**

Use explicit dimension/aspect/child-count rules, group only candidates spatially connected to the anchor band before the next peer heading, retain every state/duplicate, attach all matching reasons, and sort by group order rather than score so output remains visually predictable.

- [ ] **Step 4: Add the failing bounded-script contract**

Mock a page with more than the projection limit and long text descendants. Assert the script returns anchor/page ancestry, bounded sibling metadata, short first-text previews, next-heading candidates, and `truncated:true`; assert it never serializes arbitrary full node fields or a whole page subtree.

- [ ] **Step 5: Implement and wire the compact projection**

Embed fixed default/max limits, request only the target and ancestor page, walk bounded page children, collect at most the configured text preview, and pass the compact result to Rust ranking. Explore collection must use `ResourceScope::None` and a single projection phase.

Run: `cargo test -p devup-mcp-figma --test explore`

Run: `cargo test -p devup-mcp-figma --test upstream_contract explore`

Run: `cargo test -p devup-mcp-figma --test collector explore`

Expected: all exploration projection tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/devup-mcp-figma/src/explore.rs crates/devup-mcp-figma/src/scripts/explore.js crates/devup-mcp-figma/src/lib.rs crates/devup-mcp-figma/src/upstream.rs crates/devup-mcp-figma/src/collector.rs crates/devup-mcp-figma/tests/explore.rs crates/devup-mcp-figma/tests/upstream_contract.rs crates/devup-mcp-figma/tests/collector.rs
git commit -m "feat: explore linked figma screen groups"
```

### Task 5: Expose `devup_figma_explore` Through Direct and Host Sources

**Files:**
- Create: `crates/devup-mcp/tests/figma_explore.rs`
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/src/server/handoff.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/tests/stdio_tools.rs`
- Modify: `crates/devup-mcp/tests/source_orchestration.rs`

**Interfaces:**
- Produces: `FigmaExploreInput { url, limit, include_text_preview, source_policy }`.
- Produces: public `devup_figma_explore` response `{ status, anchor, group, candidates, truncated, source, diagnostics }`.
- Adds: `PendingOperation::Explore(FigmaExploreInput)`.

- [ ] **Step 1: Write failing tool-list, schema, and orchestration tests**

Assert the tool list grows from five to six, `url` is required, `limit` is bounded to `1..=100`, direct and host completions return byte-equivalent candidate data, host mode returns `needs_figma` with a continuation ID, and invalid/file-only URLs return stable validation errors.

- [ ] **Step 2: Run and observe the absent MCP tool**

Run: `cargo test -p devup-mcp --test figma_explore`

Run: `cargo test -p devup-mcp --test stdio_tools`

Expected: failures for the missing tool and old five-tool assertion.

- [ ] **Step 3: Register direct completion and host continuation**

Parse the link through `FigmaTarget`, plan the compact exploration read call, store only the pending input/collector state during host handoff, rank after collection, include canonical URLs built from the source file key, and erase the session after completion.

- [ ] **Step 4: Enforce per-tool resource policies**

Add orchestration assertions that explore/search produce no resource calls, UI conversion always schedules `Used` after snapshot completion, and JSON conversion retains `File`. Ensure source policy changes transport only, not output ordering or resource semantics.

Run: `cargo test -p devup-mcp --test figma_explore`

Run: `cargo test -p devup-mcp --test source_orchestration`

Run: `cargo test -p devup-mcp --test stdio_tools`

Expected: all MCP and resource-policy tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp/src/server/tools.rs crates/devup-mcp/src/server/handoff.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/tests/figma_explore.rs crates/devup-mcp/tests/stdio_tools.rs crates/devup-mcp/tests/source_orchestration.rs
git commit -m "feat: expose figma screen exploration"
```

### Task 6: Add the Sanitized WQUW-151 Live-Derived Regression

**Files:**
- Create: `crates/devup-mcp/tests/fixtures/wquw-151-neighborhood.json`
- Create: `crates/devup-mcp-devup-ui/tests/fixtures/wquw-151-proofread.json`
- Modify: `crates/devup-mcp/tests/figma_explore.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/fixture_snapshot.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/snapshots/fixture_snapshot__wquw_151_proofread.snap`

**Interfaces:**
- Consumes: sanitized real projection for heading `3879:35481` and selected screen `3879:35518`.
- Produces: stable explore result and DevupUI TSX snapshot with real token/typography names.

- [ ] **Step 1: Create minimal sanitized fixtures from the authenticated read-only result**

Keep the exact node geometry/names needed for spatial grouping and the selected subtree fields/resources needed for codegen. Remove source request metadata, account/user data, access credentials, plugin session identifiers, and unrelated page contents. Validate both files as JSON before use.

- [ ] **Step 2: Write and run the failing real-regression assertions**

Assert exploration includes all related proofread states and exact node `3879:35518`. Assert the generated component contains the root and story-section `VStack` children, the long first paragraph, `bg="$backgroundLight"`, `color="$primary"`, `typography="h3"`, `typography="body"`, and nested `typography="bodySemibold"` around `[1. 이름]`.

Run: `cargo test -p devup-mcp --test figma_explore wquw_151 -- --exact`

Run: `cargo insta test -p devup-mcp-devup-ui wquw_151_proofread -- --exact`

Expected: failures until the fixture adapters and approved snapshot are present.

- [ ] **Step 3: Add fixture adapters and review the snapshot**

Route the compact fixture through the same public Rust exploration function and the subtree fixture through the same production `generate_component` path. Review the entire new insta snapshot, confirm no child/story section is missing and every token/style name comes from the captured resource payload, then accept only this named snapshot.

Run: `cargo test -p devup-mcp --test figma_explore wquw_151 -- --exact`

Run: `cargo insta test -p devup-mcp-devup-ui wquw_151_proofread -- --exact`

Run: `cargo insta pending-snapshots --workspace`

Expected: both regressions pass and no pending snapshots remain.

- [ ] **Step 4: Commit**

```bash
git add crates/devup-mcp/tests/fixtures/wquw-151-neighborhood.json crates/devup-mcp/tests/figma_explore.rs crates/devup-mcp-devup-ui/tests/fixtures/wquw-151-proofread.json crates/devup-mcp-devup-ui/tests/fixture_snapshot.rs crates/devup-mcp-devup-ui/tests/snapshots/fixture_snapshot__wquw_151_proofread.snap
git commit -m "test: lock wquw 151 figma conversion"
```

### Task 7: Live Smoke, Documentation, Changepack, and Full Fixture Parity

**Files:**
- Modify: `crates/devup-mcp/tests/live_figma_contract.rs`
- Modify: `README.md`
- Modify: `.changepacks/changepack_log_figma_remote_mcp.json`

**Interfaces:**
- Produces: ignored authenticated exploration-to-conversion smoke and release-ready documentation.

- [ ] **Step 1: Add an ignored live WQUW-151 smoke**

Call explore for the canonical heading URL, verify candidate `3879:35518`, convert that exact canonical URL with `ResourceScope::Used`, and assert the result contains named color/typography tokens and the `[1. 이름]` nested text. Keep all returned live payloads in memory and avoid printing/snapshotting the full story.

- [ ] **Step 2: Run the authenticated smoke**

Run: `cargo test -p devup-mcp --test live_figma_contract wquw_151 -- --ignored --exact`

Expected: exploration and exact conversion pass through the authenticated source available to the environment; the repository receives no generated live payload.

- [ ] **Step 3: Document the explicit explore-then-convert workflow**

Add the Korean/English-safe command flow, explain duplicate/state candidate preservation, document `None`/`Used`/`File`, and state that unresolved resources fall back to raw values with a diagnostic. Do not imply that the MCP semantically interprets Jira or chooses a product state.

- [ ] **Step 4: Update the existing JSON changepack**

Record minor changes for `devup-mcp`, `devup-mcp-figma`, and `devup-mcp-devup-ui`: linked-screen exploration, exact local/remote used-resource collection, and styled-text typography preservation.

- [ ] **Step 5: Run focused and compatibility suites**

Run: `cargo test -p devup-mcp-figma`

Run: `cargo test -p devup-mcp-devup-ui`

Run: `cargo test -p devup-mcp`

Run: `cargo insta test --workspace --all-features`

Run: `cargo test -p devup-mcp-devup-ui --test fixture_snapshot`

Expected: all tests pass, including all 268 imported plugin fixtures, and no `.snap.new` file remains.

- [ ] **Step 6: Commit**

```bash
git add crates/devup-mcp/tests/live_figma_contract.rs README.md .changepacks/changepack_log_figma_remote_mcp.json
git commit -m "docs: release linked figma exploration"
```

### Task 8: Final Verification, Installation, Push, and PR Update

**Files:**
- Verify: entire workspace and Git history.
- Install: release `devup-mcp` binary into the active Codex MCP configuration target already used by this branch.

- [ ] **Step 1: Run the complete verification matrix from a clean index**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Run: `cargo test --workspace --all-features`

Run: `cargo insta test --workspace --all-features`

Run: `cargo insta pending-snapshots --workspace`

Run: `cargo build --workspace --release`

Expected: every command exits 0, no ignored failure is hidden, and no pending snapshot exists.

- [ ] **Step 2: Audit privacy, generated artifacts, and diff scope**

Run targeted `rg` searches for bearer tokens, OAuth callback parameters, email addresses, `.snap.new`, and temp/live payload names under tracked changes. Run `git diff --check`, inspect `git status --short`, and review every changed file against the approved spec.

Expected: only intended source, test, fixture, docs, and changepack changes remain; no credential or unrelated Girok worktree file is present.

- [ ] **Step 3: Install and smoke the release binary in Codex**

Copy the verified release executable to the existing Codex devup-mcp installation path without changing secrets, restart/reconnect the MCP process through the existing configuration, list tools, and confirm `devup_figma_explore` plus the five existing tools are callable. Run one bounded authenticated explore request and one exact conversion request; do not persist their full response.

- [ ] **Step 4: Commit any verification-only metadata, push, and update PR**

If formatting or the reviewed snapshot changed tracked files, rerun the affected verification and create one focused final commit. Push `owjs3901/figma-remote-mcp` to `origin` and update PR `https://github.com/dev-five-git/devup-mcp/pull/1` with requirements, implementation, tests, privacy considerations, assumptions, and remaining risks.

- [ ] **Step 5: Record final evidence**

Capture the final commit SHA, pushed branch status, PR URL, exact verification commands/results, Codex installation target, live smoke outcome, unresolved limitations, and the fact that Figma access remained read-only for the user-facing completion report.
