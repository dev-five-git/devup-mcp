# devup-mcp Figma Correctness and Reuse Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make Figma acquisition auditable and reusable, resolve theme conflicts deterministically without aborting useful output, export Section-contained screens, preserve provenance and large values, and reduce the pinned plugin ledger to zero `not_ported` entries.

**Architecture:** Keep the existing three-crate workspace. `devup-mcp-figma` owns raw acquisition, graph/resource completeness, large-value transport and target classification; `devup-mcp-devup-ui` owns deterministic TSX/theme projection and source spans; `devup-mcp` owns bounded in-memory artifacts, composite export and public MCP compatibility wrappers.

**Tech Stack:** Rust 2024, rmcp, Tokio, Serde/serde_json, SHA-256, cargo-insta, compiled read-only Figma Plugin API scripts.

**Spec:** `docs/superpowers/specs/2026-09-01-devup-mcp-figma-correctness-and-reuse-design.md`

## Global Constraints

- Apply strict red-green-refactor. No production behavior changes before a test has failed for the intended reason.
- Keep the public product/install name `devup-mcp`; do not add an IR/auth/cache/server crate.
- Never accept arbitrary JavaScript or add a Figma write tool.
- Do not persist credentials, live snapshots, screenshots or cache entries by default.
- Preserve existing MCP tool inputs and outputs additively unless a bug requires a status correction.
- Keep the 268 imported golden snapshots unchanged unless the approved output contract explicitly requires a reviewed change.
- Every new diagnostic and stat must avoid design text, credential values and Figma account information.
- Each task ends with focused tests and a focused commit.

## Task 1: Add Graph Completeness and Structured Diagnostics

**Files:**

- Modify: `crates/devup-mcp-figma/src/snapshot.rs`
- Modify: `crates/devup-mcp-figma/src/payload.rs`
- Modify: `crates/devup-mcp-figma/src/lib.rs`
- Modify: `crates/devup-mcp-figma/tests/snapshot.rs`
- Modify: `crates/devup-mcp-figma/tests/payload_contract.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/tests/downstream_integration.rs`

### Step 1: Write the graph-audit RED tests

Add literal snapshots covering:

- a root whose declared child is absent;
- a child with a conflicting in-scope `parentId`;
- an orphan node unreachable from every root;
- a valid hidden child and expanded instance child;
- an observed `childCount` that differs from `childrenIds`;
- a `$truncated` value and a field error.

Assert a wished-for `Snapshot::audit()` result containing explicit graph/field states and literal counts. The production mutation caught is “missing children or truncated fields are treated as complete.”

Run:

```text
cargo test -p devup-mcp-figma --test snapshot audit -- --nocapture
```

Expected RED: the audit API/type does not exist.

### Step 2: Implement the minimal audit types and traversal

Add serializable types equivalent to:

```rust
pub enum CompletenessState { Complete, Partial, Failed }

pub struct SnapshotAudit {
    pub state: CompletenessState,
    pub root_count: usize,
    pub preserved_node_count: usize,
    pub reachable_node_count: usize,
    pub orphan_node_ids: Vec<String>,
    pub declared_child_count: usize,
    pub exported_child_count: usize,
    pub missing_children: Vec<MissingChild>,
    pub parent_mismatches: Vec<ParentMismatch>,
    pub truncated_fields: Vec<FieldLocation>,
    pub field_error_count: usize,
}
```

Traverse roots in declared child order. A missing requested root is failed; missing descendants, count mismatches, field errors and truncation are partial. Hidden children remain reachable.

### Step 3: Verify GREEN and add payload aggregation RED

Run the focused snapshot tests, then add payload tests asserting graph audit and unresolved-resource state combine into one `CompletenessReport`.

Run:

```text
cargo test -p devup-mcp-figma --test snapshot audit -- --nocapture
cargo test -p devup-mcp-figma --test payload_contract completeness -- --nocapture
```

Expected: snapshot tests pass; payload test first fails because report aggregation is absent.

### Step 4: Implement payload completeness and public status

Replace the coarse payload completeness derivation with an additive report containing graph, fields and resources. Keep the legacy enum for compatibility but derive it from the report.

In `complete_operation`, return:

- `status: "complete"` only when the report is complete;
- `status: "partial"` for usable degraded output;
- the complete report under `completenessReport`.

Do not reject partial output yet; strict behavior belongs to the composite export task.

### Step 5: Verify and commit

Run:

```text
cargo test -p devup-mcp-figma --test snapshot
cargo test -p devup-mcp-figma --test payload_contract
cargo test -p devup-mcp --test downstream_integration
cargo test -p devup-mcp --test source_orchestration
```

Commit:

```text
feat(figma): audit snapshot completeness
```

## Task 2: Make Theme Projection Deterministic, Non-Fatal and Scope-Aware

**Files:**

- Modify: `crates/devup-mcp-devup-ui/src/theme/devup_json.rs`
- Modify: `crates/devup-mcp-devup-ui/src/theme/tokens.rs`
- Modify: `crates/devup-mcp-devup-ui/src/theme/mod.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/theme.rs`
- Create: `crates/devup-mcp-devup-ui/tests/fixtures/theme-conflicts.json`
- Modify: `crates/devup-mcp-figma/src/resources.rs`
- Modify: `crates/devup-mcp-figma/src/payload.rs`

### Step 1: Write deterministic conflict RED tests

Construct two snapshots with identical variables in reversed input order. Use literal local/remote candidates that normalize to the same token but have different mode values.

Assert:

- byte-identical `devup.json` for both orders;
- explicit WEB syntax wins over normalized name;
- local wins over used remote when both are otherwise equal;
- the losing candidates appear in `conflicts` diagnostics;
- serialization still succeeds and no `DEVUP_THEME_CONFLICT` error is returned.

Run:

```text
cargo test -p devup-mcp-devup-ui --test theme conflict -- --nocapture
```

Expected RED: current HashMap iteration changes the winner or conflict metadata is absent.

### Step 2: Implement sorted candidates and conflict records

Replace `HashMap::values()` projection with stable candidate vectors sorted by:

1. explicit WEB syntax;
2. local before remote;
3. collection name;
4. variable name;
5. variable ID.

Deduplicate equal values. Emit one winner into the existing devup.json schema and return all non-equal candidates as structured conflict metadata. Reserve `DevupThemeConflict` for unrecoverable output serialization/schema errors only.

### Step 3: Write alias fallback and scope RED tests

Add tests for:

- an alias cycle alongside independent valid tokens;
- a missing collection alongside valid tokens;
- node scope excluding an unused local variable;
- page scope retaining used alias dependencies;
- file scope retaining all available local variables and used remote variables;
- raw resolved color/length fallback without invented token names.

The production mutations caught are “one bad resource aborts all theme output” and “scope argument is ignored.”

### Step 4: Implement explicit source and usage sets

Extend resource data enough to retain local/remote origin and binding occurrences. Change `generate_devup_json` to accept a projection context containing scope and used resource IDs instead of ignoring `_scope`.

Resolve aliases per `(variable_id, mode_id)` and keep valid independent branches. Missing/cyclic candidates add diagnostics and unresolved records, never remove unrelated output.

### Step 5: Verify snapshots and commit

Run:

```text
cargo test -p devup-mcp-devup-ui --test theme
cargo test -p devup-mcp-devup-ui --test compat_fixtures
cargo test -p devup-mcp-devup-ui --test wquw_151
cargo insta pending-snapshots
```

Commit:

```text
fix(theme): resolve conflicts without aborting export
```

## Task 3: Introduce Reusable Acquisition Artifacts and Composite Export

**Files:**

- Create: `crates/devup-mcp/src/server/artifacts.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/src/server/handoff.rs`
- Modify: `crates/devup-mcp/src/lib.rs`
- Create: `crates/devup-mcp/tests/artifact_cache.rs`
- Create: `crates/devup-mcp/tests/composite_export.rs`
- Modify: `crates/devup-mcp/tests/stdio_tools.rs`
- Modify: `crates/devup-mcp/tests/source_orchestration.rs`

### Step 1: Write bounded-cache RED tests

Test a real in-memory store API using deterministic fake time and literal payloads. Assert:

- the same request key reuses one artifact;
- expired entries miss;
- `refresh` bypasses an entry;
- LRU eviction respects item and aggregate byte limits;
- concurrent same-key insertions use one acquisition result;
- credential-like input fields do not appear in serialized stats or keys.

Run:

```text
cargo test -p devup-mcp --test artifact_cache -- --nocapture
```

Expected RED: artifact store module/API is absent.

### Step 2: Implement the minimal memory-only ArtifactStore

Use Tokio synchronization and standard collections; avoid a new dependency unless profiling proves necessary. Store an `Arc<CollectedPayload>` plus safe metadata. Generate opaque random artifact IDs and a SHA-256 content hash from canonical payload bytes.

Enforce TTL, per-entry bytes, aggregate bytes and entry count. Do not write to disk.

### Step 3: Write composite export RED tests

Add a test upstream that returns a production-shaped fast envelope. Invoke the wished-for `devup_figma_export` with outputs `tsx`, `devupJson`, `rawSnapshot` and `sourceMap`.

Assert:

- one acquisition call;
- both TSX and used-token devup.json come from the same payload;
- a returned `artifactId` can produce a second projection with zero new calls;
- `strict: true` rejects a partial completeness report;
- `refresh: true` makes one new call;
- compatibility wrappers still work.

### Step 4: Implement composite export and compatibility wrappers

Add public input fields equivalent to:

```text
url | artifactId
outputs[]
scope
rootLayout
strict
refresh
outputPath(s)
sourcePolicy
```

Keep `devup_figma_to_ui` and `devup_figma_to_json` registered. Route both through shared artifact acquisition/projection code.

Return safe collection stats including `cacheHit`, `artifactId`, timestamps and actual tool-call counts.

### Step 5: Verify and commit

Run:

```text
cargo test -p devup-mcp --test artifact_cache
cargo test -p devup-mcp --test composite_export
cargo test -p devup-mcp --test source_orchestration
cargo test -p devup-mcp --test stdio_tools
```

Commit:

```text
feat(server): reuse figma acquisitions across exports
```

## Task 4: Add One-Call Full Theme Fast Path

**Files:**

- Create: `crates/devup-mcp-figma/src/scripts/fast_theme.js`
- Modify: `crates/devup-mcp-figma/src/upstream.rs`
- Modify: `crates/devup-mcp-figma/src/collector.rs`
- Modify: `crates/devup-mcp-figma/src/envelope.rs`
- Modify: `crates/devup-mcp-figma/src/lib.rs`
- Modify: `crates/devup-mcp-figma/tests/upstream_contract.rs`
- Modify: `crates/devup-mcp-figma/tests/collector.rs`
- Modify: `crates/devup-mcp-figma/tests/envelope.rs`
- Modify: `crates/devup-mcp/tests/live_figma_contract.rs`

### Step 1: Write read-only script contract RED tests

Assert the new built-in script:

- calls only async local collection/variable/style read APIs;
- emits the same versioned PNG envelope integrity fields as node fast snapshot;
- records local/remote source, mode and code syntax;
- never reads user JS, private data or mutation APIs.

Expected RED: `FastTheme` script kind does not exist.

### Step 2: Implement fast theme script and decoder

Collect file-local theme data in one `use_figma` execution and encode bounded chunks. Reuse envelope CRC/hash/schema checks. Decode directly to the production resource snapshot contract.

### Step 3: Write collector atomic-fallback RED tests

Assert a variables-only file request:

- completes in one call on valid fast theme output;
- discards the entire fast result and restarts catalog/batch legacy collection on corruption or size overflow;
- reports `transport`, `fallbackUsed`, reason and cumulative call count correctly.

### Step 4: Implement collector branch and verify

Add fast-theme eligibility only for file-scope theme acquisition. Preserve the existing legacy 89-call-capable path for oversized or changed contracts.

Run:

```text
cargo test -p devup-mcp-figma --test upstream_contract fast_theme
cargo test -p devup-mcp-figma --test envelope theme
cargo test -p devup-mcp-figma --test collector theme
cargo test -p devup-mcp --test source_orchestration
```

Commit:

```text
perf(figma): collect full theme in one fast call
```

## Task 5: Classify Section Targets and Export Multiple Frames

**Files:**

- Modify: `crates/devup-mcp-figma/src/explore.rs`
- Modify: `crates/devup-mcp-figma/src/snapshot.rs`
- Modify: `crates/devup-mcp-figma/src/scripts/explore.js`
- Modify: `crates/devup-mcp-figma/tests/explore.rs`
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/tests/figma_explore.rs`
- Create: `crates/devup-mcp/tests/section_export.rs`
- Modify: `crates/devup-mcp/tests/fixtures/wquw-151-neighborhood.json`

### Step 1: Write target-classification RED tests

Using a literal Section with nested frames, assert classifications for file/page/section/screen/component/other and verify candidates preserve visual order, visibility, bounds, breadcrumb, child count and canonical URL.

Expected RED: target kind and full candidate metadata are absent.

### Step 2: Implement additive classification

Add `TargetKind` and candidate fields without changing existing search/explore fields. Exact screen links remain a one-candidate result. Section links return direct/nested screen candidates without treating annotations as screens.

### Step 3: Write multi-frame export RED tests

Assert:

- a Section URL without selection returns `selection_required`, not a giant Section TSX;
- `frameIds` exports exactly those frames in requested order;
- `allScreens` exports all classified screen candidates in visual order;
- invalid/out-of-section IDs fail safely;
- shared Section acquisition is reused where the artifact contains each selected subtree.

### Step 4: Implement section export orchestration

Extend composite export selection with `frameIds` and `allScreens`. Name generated components deterministically and report a per-frame completeness/source map result.

### Step 5: Verify and commit

Run:

```text
cargo test -p devup-mcp-figma --test explore
cargo test -p devup-mcp --test figma_explore
cargo test -p devup-mcp --test section_export
```

Commit:

```text
feat(figma): export screens from section links
```

## Task 6: Generate TSX and Theme Provenance Sidecars

**Files:**

- Modify: `crates/devup-mcp-devup-ui/src/codegen/mod.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/component.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/style.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/text.rs`
- Modify: `crates/devup-mcp-devup-ui/src/theme/devup_json.rs`
- Create: `crates/devup-mcp-devup-ui/src/provenance.rs`
- Modify: `crates/devup-mcp-devup-ui/src/lib.rs`
- Create: `crates/devup-mcp-devup-ui/tests/provenance.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/wquw_151.rs`
- Create: `crates/devup-mcp-devup-ui/tests/snapshots/wquw_151__wquw_151_proofread_source_map.snap`

### Step 1: Write source-range RED tests

For a small hand-checked frame, assert literal TSX byte ranges map to the expected node ID, input field and variable/style ID. Assert root/component/text/prop entries and fallback kinds.

The mutation caught is “generated code changes but provenance silently points to the wrong node/property.”

Expected RED: codegen output has no source map.

### Step 2: Implement fragment spans

Have rendered fragments carry local provenance before concatenation. The final renderer adjusts UTF-8 byte offsets as imports, component wrapper and indentation are assembled. Do not inject debug props/comments into TSX.

### Step 3: Add theme JSON pointer provenance

Write a failing test mapping `/theme/colors/default/primary` and typography entries to variable/style IDs and resolution/fallback kinds. Implement provenance next to deterministic theme insertion.

### Step 4: Verify WQUW source map and commit

Snapshot the WQUW source map without embedding text content. Assert representative heading, nested placeholder typography and footer border map back to exact Figma nodes/resources.

Run:

```text
cargo test -p devup-mcp-devup-ui --test provenance
cargo test -p devup-mcp-devup-ui --test wquw_151
cargo insta pending-snapshots
```

Commit:

```text
feat(devup-ui): trace generated output to figma sources
```

## Task 7: Preserve Large Fields and Export Requested Assets

**Files:**

- Modify: `crates/devup-mcp-figma/src/snapshot.rs`
- Modify: `crates/devup-mcp-figma/src/collector.rs`
- Modify: `crates/devup-mcp-figma/src/upstream.rs`
- Modify: `crates/devup-mcp-figma/src/scripts/snapshot.js`
- Modify: `crates/devup-mcp-figma/src/scripts/fast_snapshot.js`
- Create: `crates/devup-mcp-figma/src/scripts/large_value.js`
- Create: `crates/devup-mcp-figma/src/scripts/assets.js`
- Create: `crates/devup-mcp-figma/src/assets.rs`
- Modify: `crates/devup-mcp-figma/src/lib.rs`
- Create: `crates/devup-mcp-figma/tests/large_values.rs`
- Create: `crates/devup-mcp-figma/tests/assets.rs`
- Modify: `crates/devup-mcp-figma/tests/upstream_contract.rs`
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`

### Step 1: Write large-value descriptor/reassembly RED tests

Assert an inline-overflow field creates a descriptor with node ID, field, byte length, hash and cursor; multiple out-of-order/duplicate/missing fragments are respectively reassembled or rejected. Use a literal large value and hand-computed expected hash.

Expected RED: descriptor and reassembly APIs are absent.

### Step 2: Implement bounded continuation

Replace size-only terminal truncation for readable fields with a descriptor. Add a compiled script that reads only the named descriptor field and byte range. Validate file/version/node/field/hash before replacing the marker.

Keep upstream getter errors as explicit `unsupported-by-upstream`; do not loop on them.

### Step 3: Write asset manifest/export RED tests

Use production-shaped image/vector nodes. Assert manifest entries include source node, field, image hash/export settings and requested format. Assert only explicit requested assets execute read-only export; failures preserve layout output and add diagnostics.

### Step 4: Implement safe asset output

Add `assetManifest` projection and optional bounded output-path export. Validate paths with the same output safety rules, avoid implicit repository writes and never include binary bytes in diagnostics/stats.

### Step 5: Verify and commit

Run:

```text
cargo test -p devup-mcp-figma --test large_values
cargo test -p devup-mcp-figma --test assets
cargo test -p devup-mcp-figma --test upstream_contract
cargo test -p devup-mcp --test composite_export
```

Commit:

```text
feat(figma): preserve large fields and requested assets
```

## Task 8: Complete WQUW-151 Section and Ten-Frame Regression Coverage

**Files:**

- Create: `crates/devup-mcp/tests/fixtures/wquw-151-section.json`
- Create: `crates/devup-mcp-devup-ui/tests/fixtures/wquw-151-frames/`
- Create: `crates/devup-mcp-devup-ui/tests/wquw_151_frames.rs`
- Create: `crates/devup-mcp-devup-ui/tests/snapshots/wquw_151_frames__*.snap`
- Modify: `crates/devup-mcp/tests/live_figma_contract.rs`
- Modify: `README.md`

### Step 1: Capture/validate actual read-only shapes

Use the authenticated official host MCP flow. Do not print or persist credentials. Record only approved design payload fields and scrub user/account metadata before committing.

Verify the Section is `4217:7743`, contains the expected ten screen candidates, and that the proofread target remains `3879:35518` unless live Figma evidence shows a versioned change.

### Step 2: Write failing Section/frame contract tests

Before changing production code for any newly observed field, add literal fixture assertions for node counts, full child reachability, text segment preservation, variable/style IDs, frame order and diagnostic state.

### Step 3: Add per-frame TSX snapshots

Generate standalone and embedded snapshots as relevant. Review differences against the actual Devup plugin copy result. Any intentional transformation must be documented by node/property and reason.

### Step 4: Add opt-in live call-count contract

Assert:

- exact proofread composite export: one normal host call;
- repeated projection by artifact ID: zero additional calls;
- Section exploration and selected exports report truthful counts;
- fast full-theme path uses one call when under the bound.

### Step 5: Verify and commit

Run:

```text
cargo test -p devup-mcp --test section_export
cargo test -p devup-mcp-devup-ui --test wquw_151
cargo test -p devup-mcp-devup-ui --test wquw_151_frames
cargo insta pending-snapshots
```

Commit:

```text
test: cover all wquw 151 figma screens
```

## Task 9: Reduce the Plugin Corpus Ledger to Zero `not_ported`

**Files:**

- Modify: `fixtures/devup-figma-plugin/ledger.json`
- Modify: `fixtures/devup-figma-plugin/manifest.json` only if fixture files change
- Create/Modify: `fixtures/devup-figma-plugin/cases/{codegen,responsive,devup-json,snapshot,errors}/*.json`
- Create/Modify: `fixtures/devup-figma-plugin/snapshots/{codegen,responsive,devup-json,snapshot,errors}/*.snap`
- Modify: `crates/devup-mcp-devup-ui/tests/compat_fixtures.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/compat_manifest.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/support/mod.rs`
- Modify: focused production files only when a newly ported test first fails
- Modify: `fixtures/devup-figma-plugin/README.md`

### Step 1: Add the zero-not-ported RED gate

Change the manifest test to require zero `not_ported` classifications. Print only IDs/categories, not fixture contents.

Run:

```text
cargo test -p devup-mcp-devup-ui --test compat_manifest coverage_registry -- --nocapture
```

Expected RED: exactly 137 entries remain.

### Step 2: Classify and port in focused batches

Process the 137 IDs by source file and behavior:

1. codegen/layout/style/text/variant;
2. variable/theme/export-devup;
3. snapshot/serialization/error paths;
4. search/plugin boundary helpers.

For each behavior, add a production-shaped JSON case or focused Rust assertion, run it RED, then implement the smallest production change. Do not relabel read/transform semantics as runtime-only to satisfy the count.

Only genuine plugin bootstrap/module lifecycle can become `upstream_runtime_only`, and each such entry must cite a concrete Rust boundary test. Mutation remains `out_of_scope_write` and cites the read-only allowlist test.

### Step 3: Recompute manifest hashes only after reviewed changes

Use the existing Rust support utilities or a repository script that canonicalizes LF. Do not hand-edit checksums. Review all snapshot diffs before acceptance.

### Step 4: Verify exact inventory and commit

Run:

```text
cargo test -p devup-mcp-devup-ui --test compat_manifest
cargo test -p devup-mcp-devup-ui --test compat_fixtures
cargo insta pending-snapshots
```

Expected: 978 ledger IDs, 268 or more reviewed fixture cases, zero orphan files, zero `not_ported`.

Commit in behavior-focused batches, ending with:

```text
test: complete devup figma plugin parity ledger
```

## Task 10: Final Verification, Release Metadata, Installation and PR Update

**Files:**

- Modify: `.changepacks/changepack_log_figma_remote_mcp.json`
- Modify: `README.md`
- Modify: `.github/workflows/ci.yml` only if local commands are not represented
- Modify: `Cargo.lock` only if dependencies changed

### Step 1: Run formatting and static checks

Run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: both exit 0 with no warnings.

### Step 2: Run all deterministic tests and snapshot checks

Run:

```text
cargo test --workspace
cargo insta pending-snapshots
cargo build --workspace --release
```

Expected: all tests pass, no pending snapshots, release build succeeds.

### Step 3: Run authenticated read-only live contracts

Using the official host MCP result path without reading or printing its token, run the WQUW-151 exact-node, Section and full-theme probes. Record only counts, hashes, transport, fallback and call totals.

If live access requires a human browser action, request only that action and resume from the same handoff session.

### Step 4: Security/privacy audit

Check tracked changes for:

- tokens, OAuth codes, verifier, registration secrets;
- account/user identifiers;
- `.env` or keyring material;
- unapproved live screenshots or raw payloads;
- binary envelope/debug dumps;
- arbitrary JavaScript or write-tool additions.

Confirm cache is memory-only and diagnostics/stats do not contain design text.

### Step 5: Update changepack and documentation

Document:

- exact-node and full-theme fast call expectations;
- cache freshness and `refresh` semantics;
- complete/partial/failed meanings;
- Section selection/batch export;
- source map and asset behavior;
- direct OAuth catalog limitation and host fallback;
- zero-not-ported corpus status and exact test counts.

### Step 6: Install the release binary into Codex

Build the release binary, locate the configured Codex MCP command, replace only the devup-mcp executable/configured path, and verify `devup-mcp --version` plus MCP tool listing. Do not modify unrelated Codex servers.

### Step 7: Commit, push and update PR #1

Run final `git status`, review every changed file, create the final focused documentation/release commit, then push `owjs3901/figma-remote-mcp`.

Update PR #1 with:

- requirement and architecture summary;
- file/feature groups;
- exact local command outcomes;
- live Figma counts and call totals;
- commit SHAs;
- privacy/security review;
- intentional Figma-to-TSX differences;
- assumptions and remaining upstream constraints.

Do not describe disabled/cost-paused CI as an implementation failure; report remote CI state separately from local verification.

## Execution Checkpoints

After Tasks 1–2, re-run the WQUW proofread snapshot before proceeding to cache changes. After Tasks 3–5, validate the public MCP schema and one-call assertions. After Tasks 6–9, run the complete parity and snapshot suite before release work. Never continue from a red checkpoint by weakening a completeness or security assertion.
