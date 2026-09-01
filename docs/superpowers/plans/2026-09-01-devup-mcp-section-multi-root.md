# Devup MCP Section Multi-Root Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Discover Section screens without collecting their full fields, then acquire selected Frames in deterministic bounded multi-root batches.

**Architecture:** Add a compact `SectionIndex` acquisition kind and a multi-root collector state machine. Store selected roots as a composite artifact with version-checked resource deduplication, while retaining the existing single-node and already-collected full Section paths.

**Tech Stack:** Rust 2024, existing compiled-in Figma scripts, official Figma MCP handoff/direct sources, Serde, Tokio.

**Spec:** `docs/superpowers/specs/2026-09-01-devup-mcp-hardening-and-delivery-design.md`

## Global Constraints

- Discovery returns only bounded candidate metadata; selected root acquisition preserves every accessible field using the existing exhaustive serializer.
- Exact Frame requests retain the existing normal single-call path.
- Batches are deterministic in visual order, default maximum concurrency is 2, and version drift never produces a complete artifact.
- Figma scripts remain compiled-in, read-only, input-validated, and never accept user JavaScript.
- WQUW-151 Section `4217:7743` and its ten checked-in Frame fixtures remain the live/golden contract.

## File map

- `crates/devup-mcp-figma/src/section.rs`: SectionIndex, candidate estimate, visual ordering, batch plan, composite merge.
- `crates/devup-mcp-figma/src/upstream.rs`: upstream request variants for section index and multi-root snapshot.
- `crates/devup-mcp-figma/src/collector.rs`: multi-root continuation state and compiled scripts.
- `crates/devup-mcp-figma/src/payload.rs`: single/composite payload types and completeness aggregation.
- `crates/devup-mcp-figma/src/lib.rs`: exports.
- `crates/devup-mcp/src/server/acquisition.rs`: URL/artifact workflow planner.
- `crates/devup-mcp/src/server/artifacts.rs`: section-index/composite cache key and payload.
- `crates/devup-mcp/src/server/handoff.rs`: pending index and batch operations.
- `crates/devup-mcp/src/server/mod.rs`: route to acquisition planner and existing projection.
- `crates/devup-mcp-figma/tests/section.rs`: ordering, packing, merge and drift unit contracts.
- `crates/devup-mcp/tests/section_export.rs`: MCP workflow and WQUW fixture integration.

---

### Task 1: Model and test SectionIndex

**Files:**
- Create: `crates/devup-mcp-figma/src/section.rs`
- Modify: `crates/devup-mcp-figma/src/lib.rs`
- Create: `crates/devup-mcp-figma/tests/section.rs`

**Interfaces:**
- Produces: `SectionIndex`, `SectionCandidate`, `build_section_index(&Snapshot, &FigmaTarget)`, and `plan_batches(&SectionIndex, &[String], BatchLimits)`.

- [ ] **Step 1: Write failing index tests**

Build a fixture with hidden nodes, notes, three screen-like Frames at different x/y positions, and nested non-screen Frames. Assert candidates contain ID/name/type/visible/bounds/breadcrumb/direct child count/subtree count/estimated bytes/reasons/canonical URL and are sorted by y row then x. Assert only requested valid IDs are selected, duplicates and foreign IDs fail, and `allScreens` returns visual order.

- [ ] **Step 2: Run the missing-module test**

Run: `cargo test -p devup-mcp-figma --test section index -- --nocapture`

Expected: FAIL because `section` and `SectionIndex` do not exist.

- [ ] **Step 3: Implement compact indexing and batch packing**

Reuse screen classification rules from `explore.rs`, but emit a dedicated schema. Count descendants and estimate serialized bytes without cloning full descendant fields into the index. Implement first-fit-in-order packing constrained by maximum estimated bytes and nodes; a single oversized root forms its own continuation-capable batch.

- [ ] **Step 4: Run index tests**

Run: `cargo test -p devup-mcp-figma --test section index batch -- --nocapture`

Expected: PASS with stable candidate and batch order.

- [ ] **Step 5: Commit**

```text
git add crates/devup-mcp-figma/src/section.rs crates/devup-mcp-figma/src/lib.rs crates/devup-mcp-figma/tests/section.rs
git commit -m "feat: index figma section screens"
```

### Task 2: Collect multiple roots with bounded continuation

**Files:**
- Modify: `crates/devup-mcp-figma/src/upstream.rs`
- Modify: `crates/devup-mcp-figma/src/collector.rs`
- Modify: `crates/devup-mcp-figma/src/section.rs`
- Test: `crates/devup-mcp-figma/tests/collector.rs`
- Test: `crates/devup-mcp-figma/tests/upstream_contract.rs`
- Test: `crates/devup-mcp-figma/tests/section.rs`

**Interfaces:**
- Produces: `CollectionRequest::section_index`, `CollectionRequest::multi_root`, `MultiRootCollectorSession`, and `CompositePayload::merge`.

- [ ] **Step 1: Add compiled-script contract tests**

Assert section-index script requests only fixed read-only APIs, embeds IDs as JSON literals, emits schema/version/root counts, and excludes descendant `fields`. Assert multi-root script accepts only validated IDs, emits root-delimited chunks, stops before the response budget, and returns completed root IDs plus continuation.

- [ ] **Step 2: Add merge/drift tests**

Feed chunks with shared byte-identical resources and assert one merged resource. Feed a duplicate node/resource ID with different canonical bytes, missing chunk, repeated chunk, file-key mismatch, and source-version mismatch; assert each result is partial/failed and never cacheable complete.

- [ ] **Step 3: Run focused tests**

Run: `cargo test -p devup-mcp-figma --test collector --test upstream_contract --test section multi_root -- --nocapture`

Expected: FAIL because the new request/session/merge types are absent.

- [ ] **Step 4: Implement collector session**

Add index and multi-root upstream request variants and allowlist entries. Execute planned batches with at most two outstanding calls. Resume only an unfinished oversized root; preserve completed roots. Merge roots in index visual order, dedupe resources by ID plus canonical hash, and compute root-level and aggregate completeness.

- [ ] **Step 5: Run collector tests**

Run: `cargo test -p devup-mcp-figma --test collector --test upstream_contract --test section -- --nocapture`

Expected: PASS and existing exact-node single-call tests remain unchanged.

- [ ] **Step 6: Commit**

```text
git add crates/devup-mcp-figma/src/upstream.rs crates/devup-mcp-figma/src/collector.rs crates/devup-mcp-figma/src/section.rs crates/devup-mcp-figma/src/payload.rs crates/devup-mcp-figma/tests/collector.rs crates/devup-mcp-figma/tests/upstream_contract.rs crates/devup-mcp-figma/tests/section.rs
git commit -m "feat: collect figma roots in bounded batches"
```

### Task 3: Add index-first MCP orchestration and composite artifact reuse

**Files:**
- Create: `crates/devup-mcp/src/server/acquisition.rs`
- Modify: `crates/devup-mcp/src/server/artifacts.rs`
- Modify: `crates/devup-mcp/src/server/handoff.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/tests/section_export.rs`
- Modify: `crates/devup-mcp/tests/artifact_cache.rs`

**Interfaces:**
- Produces: `AcquisitionPlan::{Single, SectionIndex, SectionRoots}`, `ArtifactKind::SectionIndex`, and `ArtifactPayload::{Single, Composite}`.

- [ ] **Step 1: Write failing orchestration tests**

For a Section URL without selection, assert one compact index call, `selection_required`, and zero exhaustive subtree calls. With one, many, and `allScreens`, assert selected roots only, minimal planned batch count, visual-order `frames[]`, and zero extra Figma calls when reusing the resulting artifact. Assert a SectionIndex artifact cannot project TSX.

- [ ] **Step 2: Run section MCP tests**

Run: `cargo test -p devup-mcp --test section_export index_first -- --nocapture`

Expected: FAIL because current code acquires the full Section before selection.

- [ ] **Step 3: Implement acquisition planning**

Move URL/request classification out of the router into `acquisition.rs`. For Section exports involving TSX, acquire/reuse `SectionIndex` first. Return its candidates when no selection exists; otherwise start the multi-root collector with validated selected IDs. Keep direct projection from a previously cached full Section design artifact.

- [ ] **Step 4: Implement composite artifact projection adapter**

Teach artifact store byte accounting, metadata and capability checks about single/composite payloads. Project each composite root with shared token/resources, retain root completeness and current `frames[]` shape, and reject source-map/raw/theme scopes not covered by the composite.

- [ ] **Step 5: Run section and cache tests**

Run: `cargo test -p devup-mcp --test section_export --test artifact_cache -- --nocapture`

Expected: PASS, including the existing full Section artifact compatibility test.

- [ ] **Step 6: Commit**

```text
git add crates/devup-mcp/src/server/acquisition.rs crates/devup-mcp/src/server/artifacts.rs crates/devup-mcp/src/server/handoff.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/tests/section_export.rs crates/devup-mcp/tests/artifact_cache.rs
git commit -m "feat: acquire selected figma section roots"
```

### Task 4: Verify WQUW-151 batching and document behavior

**Files:**
- Modify: `crates/devup-mcp/tests/section_export.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/wquw_151_frames.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: checked-in WQUW-151 ten-frame JSON fixtures.

- [ ] **Step 1: Add WQUW contract assertions**

Assert Section `4217:7743` yields the known ten Frame IDs in visual order, the planner uses fewer than ten exhaustive calls, and every generated snapshot/TSX preserves current text, child, variable, typography and footer-stroke assertions.

- [ ] **Step 2: Run WQUW tests**

Run: `cargo test -p devup-mcp --test section_export wquw -- --nocapture` and `cargo test -p devup-mcp-devup-ui --test wquw_151_frames -- --nocapture`.

Expected: PASS with no snapshot update.

- [ ] **Step 3: Update README and verify crates**

Document index-first selection, compact discovery, batching/continuation, composite cache reuse and version-drift failure. Run `cargo fmt --all -- --check`, `cargo clippy -p devup-mcp-figma -p devup-mcp --all-targets --all-features -- -D warnings`, and `cargo test -p devup-mcp-figma -p devup-mcp --all-features`.

Expected: all commands exit 0.

- [ ] **Step 4: Commit**

```text
git add crates/devup-mcp/tests/section_export.rs crates/devup-mcp-devup-ui/tests/wquw_151_frames.rs README.md
git commit -m "test: verify wquw section batching"
```
