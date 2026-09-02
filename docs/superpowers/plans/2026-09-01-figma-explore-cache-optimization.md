# Figma Explore Cache Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make compatible repeated Figma exploration local, refreshable, observable, concurrency-safe, and measurably faster.

**Architecture:** Extend `ArtifactRequestKey` compatibility and `ArtifactLookup` provenance rather than introducing a second cache. Exact, completed-related, superset, and compatible direct in-flight reuse all converge on the existing bounded artifact store; projection uses the requested target while the artifact retains its acquisition target.

**Tech Stack:** Rust 1.88, Tokio watch channels, rmcp, serde/schemars, Bun/Node test runner.

**Spec:** `docs/superpowers/specs/2026-09-01-figma-explore-cache-optimization-design.md`

## Global Constraints

- Use Bun for repository JavaScript commands.
- Keep cache memory-only with the existing ten-minute TTL and byte/entry bounds.
- Never share an artifact unless the requested node exists in its snapshot.
- Never share incomplete host handoff call IDs.
- `refresh=true` bypasses all reuse paths.
- Preserve existing public fields while making top-level `collection` current-request accurate.

---

### Task 1: Refresh and Superset Cache Selection

**Files:**
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/src/server/artifacts.rs`
- Test: `crates/devup-mcp/tests/figma_explore.rs`
- Test: `crates/devup-mcp/tests/stdio_tools.rs`

**Interfaces:**
- Consumes: `ArtifactRequestKey::from_collection`, `ExploreReadOptions`.
- Produces: `FigmaExploreInput.refresh: bool` and compatible exact/related/superset lookup.

- [ ] **Step 1: Write failing public integration tests** for refresh bypass and larger-projection reuse of a smaller related request.
- [ ] **Step 2: Run the focused tests** and confirm cache miss/upstream count failures.
- [ ] **Step 3: Implement minimal compatibility selection** with exact-first and smallest-sufficient projection ordering.
- [ ] **Step 4: Run `cargo test -p devup-mcp --test figma_explore`** and confirm green.

### Task 2: Cache Provenance and Current Request Statistics

**Files:**
- Modify: `crates/devup-mcp/src/server/artifacts.rs`
- Modify: `crates/devup-mcp/src/server/projection.rs`
- Test: `crates/devup-mcp/tests/artifact_cache.rs`
- Test: `crates/devup-mcp/tests/figma_explore.rs`

**Interfaces:**
- Produces: `CacheReuseKind`, lookup age/TTL, avoided-call count, `cache.originCollection`, and zero current `collection` on hits.

- [ ] **Step 1: Write failing response assertions** for `reuseKind`, age, TTL, avoided calls, origin statistics, and current statistics.
- [ ] **Step 2: Run focused tests** and verify missing-field and stale-stat failures.
- [ ] **Step 3: Add typed lookup provenance** and serialize it from the shared cache metadata helper.
- [ ] **Step 4: Re-run artifact and explore tests** and confirm green.

### Task 3: Compatible Direct In-Flight Reuse

**Files:**
- Modify: `crates/devup-mcp/src/server/artifacts.rs`
- Test: `crates/devup-mcp/tests/artifact_cache.rs`

**Interfaces:**
- Consumes: explore compatibility from Task 1.
- Produces: compatible follower waiting with node-coverage validation and independent fallback.

- [ ] **Step 1: Write failing concurrent tests** proving a related request shares one acquisition and an uncovered request performs its own acquisition.
- [ ] **Step 2: Run the focused tests** and confirm the related upstream count is two before implementation.
- [ ] **Step 3: Track in-flight request keys** and wait for a compatible owner before exact acquisition.
- [ ] **Step 4: Re-run cancellation, concurrency, and cache tests** and confirm green.

### Task 4: Dirty Build Identity

**Files:**
- Modify: `crates/devup-mcp/build.rs`
- Modify: `crates/devup-mcp/src/lib.rs`
- Test: `crates/devup-mcp/tests/cli.rs`

**Interfaces:**
- Produces: testable build-ID composition and `<commit>-dirty` for modified Git worktrees.

- [ ] **Step 1: Write failing helper tests** for clean, dirty, unsafe, and source-unknown identities.
- [ ] **Step 2: Run CLI tests** and confirm the dirty expectation fails.
- [ ] **Step 3: Implement Git status detection and safe composition** without reading credentials or file contents.
- [ ] **Step 4: Re-run CLI and stdio smoke tests** and confirm green.

### Task 5: Full Verification and WQUW-151 Measurement

**Files:**
- Modify: `.changepacks/changepack_log_figma_remote_mcp.json`
- Modify: `README.md`

**Interfaces:**
- Consumes: completed cache behavior and installed release binary.
- Produces: documented diagnostics and measured cold/exact/related/superset comparison.

- [ ] **Step 1: Document refresh and cache diagnostics** with safe restart guidance.
- [ ] **Step 2: Run formatting, Clippy, full workspace tests, Node script tests, and snapshot checks.**
- [ ] **Step 3: Install the release binary and run `--self-check`.**
- [ ] **Step 4: Measure WQUW-151** and report duration, cache kind, and Figma-call savings for every path.
