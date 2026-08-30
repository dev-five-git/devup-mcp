# Figma Name Search and Workspace Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Find Figma pages/canvases by human-readable name and let an agent deterministically convert every confirmed result into DevupUI artifacts.

**Architecture:** Search builds a deterministic in-memory index from the same source-independent collector used by conversion. It returns canonical node URLs plus type/page/breadcrumb metadata and keeps no conversational server state; the agent reuses those URLs for one or more existing `devup_figma_to_ui` calls. Optional file output is confined to the startup workspace and uses atomic writes.

**Tech Stack:** Rust 1.88, edition 2024, serde/serde_json, unicode-normalization, Tokio, rmcp, Cargo-only CI.

**Spec:** `docs/superpowers/specs/2026-08-31-figma-host-fallback-parity-design.md`

## Global Constraints

- Prerequisites: both the collection/live-contract and JSON-fixture-parity plans are complete and green.
- Search uses direct or host source through the existing collector; results must be identical across sources.
- Default node types are PAGE, SECTION, FRAME, COMPONENT_SET and COMPONENT.
- Ranking is exact, normalized exact, prefix, contains; fuzzy matching is opt-in only.
- Normalization uses Unicode NFC, whitespace collapse and Unicode lowercase without modifying returned original names.
- Duplicate names are never silently reduced to one result.
- Search results include canonical URL, node ID, type, page name and complete breadcrumb.
- Follow-up natural language is interpreted by the agent, not stored or parsed by `devup-mcp`.
- First release has no batch conversion tool; agents call `devup_figma_to_ui` per confirmed canonical URL with bounded concurrency.
- Raw search indexes and Figma content remain memory-only.
- File output occurs only when `outputPath` is explicit and resolves beneath the startup workspace.

## File Structure

### Create

- `crates/devup-mcp-figma/src/search.rs` — name normalization, index and ranking.
- `crates/devup-mcp-figma/tests/search.rs` — Korean/Unicode/ranking/duplicate/source parity tests.
- `crates/devup-mcp/src/server/workspace.rs` — workspace-confined atomic artifact writer.
- `crates/devup-mcp/tests/figma_search.rs` — MCP search direct/host continuation tests.
- `crates/devup-mcp/tests/workspace_export.rs` — traversal, symlink and atomic-write tests.
- `crates/devup-mcp/tests/search_to_ui_flow.rs` — search followed by every-result conversion.

### Modify

- `Cargo.toml` — add `unicode-normalization = "0.1"` as a workspace dependency.
- `crates/devup-mcp-figma/Cargo.toml` — consume unicode-normalization.
- `crates/devup-mcp-figma/src/lib.rs` — export search interfaces.
- `crates/devup-mcp/src/server/tools.rs` — search input and optional output paths.
- `crates/devup-mcp/src/server/handoff.rs` — persist pending search operation without persisting result indexes after completion.
- `crates/devup-mcp/src/server/mod.rs` — register search and workspace export.
- `crates/devup-mcp/tests/stdio_tools.rs` — tool schema assertions.
- `README.md` — conversational search-to-implementation examples.
- `.changepacks/*.md` — search/export release notes.

---

### Task 1: Deterministic Figma Name Index

**Files:**
- Create: `crates/devup-mcp-figma/src/search.rs`
- Create: `crates/devup-mcp-figma/tests/search.rs`
- Modify: `Cargo.toml`
- Modify: `crates/devup-mcp-figma/Cargo.toml`
- Modify: `crates/devup-mcp-figma/src/lib.rs`

**Interfaces:**
- Consumes: `CollectedPayload` metadata and snapshot nodes.
- Produces: `SearchMatchKind::{Exact, NormalizedExact, Prefix, Contains, Fuzzy}`.
- Produces: `SearchQuery { query, node_types, match_mode, limit }`.
- Produces: `SearchMatch { name, node_id, node_type, page_name, breadcrumb, canonical_url, match_kind, score }`.
- Produces: `FigmaSearchIndex::from_payload(&CollectedPayload)` and `search(&self, &SearchQuery) -> Vec<SearchMatch>`.

- [ ] **Step 1: Write failing normalization and ranking tests**

Cover composed/decomposed Korean, repeated whitespace, ASCII case, exact versus prefix versus contains, duplicate names on different pages, hidden nodes, type filters, deterministic ties and limit enforcement.

```rust
let results = index.search(&SearchQuery::normalized("A : STORY-F-PROOFREAD"));
assert_eq!(results[0].match_kind, SearchMatchKind::Exact);
assert_eq!(results[0].breadcrumb, ["Writer", "Proofread", "A : STORY-F-PROOFREAD"]);
assert_eq!(results.len(), 2); // duplicate names are retained
```

- [ ] **Step 2: Run and confirm search types are missing**

Run: `cargo test -p devup-mcp-figma --test search`

Expected: compile failure.

- [ ] **Step 3: Implement normalization, index construction and stable sorting**

Construct breadcrumbs from parent IDs and page roots with cycle detection. Normalize only the comparison key. Sort by match rank, then normalized name, page name, breadcrumb and node ID. Canonical URLs must use the source file key and percent-safe `node-id` query.

```rust
fn normalize_name(value: &str) -> String {
    value.nfc().flat_map(char::to_lowercase).collect::<String>()
        .split_whitespace().collect::<Vec<_>>().join(" ")
}
```

- [ ] **Step 4: Add opt-in fuzzy matching tests**

Implement a bounded edit-distance calculation only when `matchMode:"fuzzy"`; require a threshold tied to query length and rank fuzzy below contains. Assert normalized mode never returns a fuzzy-only candidate.

Run: `cargo test -p devup-mcp-figma --test search`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/devup-mcp-figma/Cargo.toml crates/devup-mcp-figma/src/search.rs crates/devup-mcp-figma/src/lib.rs crates/devup-mcp-figma/tests/search.rs
git commit -m "feat: index figma nodes by name"
```

### Task 2: Expose `devup_figma_search` Through Direct and Host Sources

**Files:**
- Create: `crates/devup-mcp/tests/figma_search.rs`
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/src/server/handoff.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/tests/stdio_tools.rs`

**Interfaces:**
- Consumes: `FigmaSearchIndex`, `CollectionScope::File`, existing source orchestration.
- Produces: `FigmaSearchInput { url, query, node_types, match_mode, limit, source_policy }`.
- Produces: public `devup_figma_search` result `{status, query, matches, diagnostics, source}`.

- [ ] **Step 1: Write failing MCP tool tests**

Assert search appears in `tools/list`; a file URL without node ID is accepted; host mode returns `needs_figma`; continuation eventually returns matches; direct mode returns the same ordered JSON; invalid node type, empty query and limit outside 1..=100 return stable errors.

- [ ] **Step 2: Run and confirm the tool is absent**

Run: `cargo test -p devup-mcp --test figma_search`

Run: `cargo test -p devup-mcp --test stdio_tools`

Expected: failures for missing tool/schema.

- [ ] **Step 3: Implement search as a pending operation**

Add `PendingOperation::Search(FigmaSearchInput)` to the handoff session. Collect file metadata/snapshots using the existing state machine, build the index only after completion, return matches, then erase all session/index data.

```rust
async fn complete_search(
    input: FigmaSearchInput,
    payload: CollectedPayload,
) -> Result<Value, DevupError> {
    let index = FigmaSearchIndex::from_payload(&payload)?;
    let query = input.into_query()?;
    Ok(serde_json::json!({
        "status": "complete",
        "matches": index.search(&query),
    }))
}
```

- [ ] **Step 4: Verify source parity and privacy**

Feed the identical synthetic collected payload through direct and host test paths and assert byte-identical matches. Inspect server debug/error output and assert node names do not appear outside the final successful response.

Run: `cargo test -p devup-mcp --test figma_search`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp/src/server crates/devup-mcp/tests/figma_search.rs crates/devup-mcp/tests/stdio_tools.rs
git commit -m "feat: expose figma name search"
```

### Task 3: Verify the Conversational Search-to-All-Results Flow

**Files:**
- Create: `crates/devup-mcp/tests/search_to_ui_flow.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: `devup_figma_search` matches and existing `devup_figma_to_ui` canonical URL input.
- Produces: documented stateless two-turn agent workflow.

- [ ] **Step 1: Write the failing end-to-end flow test**

Create two same-name frames on different pages. Search for the shared name, retain both results, then invoke UI conversion once per returned canonical URL. Assert two distinct artifacts and source node IDs; assert no search-result session ID is required after search completes.

- [ ] **Step 2: Run and fix only URL handoff incompatibilities**

Run: `cargo test -p devup-mcp --test search_to_ui_flow`

Expected: pass after canonical URLs round-trip through `FigmaTarget::parse`.

- [ ] **Step 3: Document the Korean conversational flow**

Add examples for “이 파일에 이름이 ~인 화면이 있니?” followed by “맞아, 그 목록 다 구현해줘.” Explain that the client agent retains structured matches and calls the existing converter for each; `devup-mcp` does not interpret natural language or persist the list.

- [ ] **Step 4: Commit**

```bash
git add crates/devup-mcp/tests/search_to_ui_flow.rs README.md
git commit -m "test: cover figma search to ui flow"
```

### Task 4: Workspace-Confined Atomic Artifact Export

**Files:**
- Create: `crates/devup-mcp/src/server/workspace.rs`
- Create: `crates/devup-mcp/tests/workspace_export.rs`
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`

**Interfaces:**
- Produces: `WorkspaceWriter::new(root: PathBuf)` and `write(relative_path, bytes) -> Result<ArtifactFile, DevupError>`.
- Produces: optional `outputPath` on UI and JSON conversion tools.
- Produces: `ArtifactFile { path, sha256, byte_len }` without echoing content when a file is written.

- [ ] **Step 1: Write failing Windows-safe path tests**

Cover relative file and directory outputs, `..`, absolute paths, nonexistent nested directories, a symlink/junction escaping the root, reserved/invalid target handling, existing-file replacement and cleanup after simulated write failure.

- [ ] **Step 2: Run and confirm writer is missing**

Run: `cargo test -p devup-mcp --test workspace_export`

Expected: compile failure.

- [ ] **Step 3: Implement root capture and atomic writes**

Capture the canonical startup current directory in `Services::production`. Resolve the nearest existing parent of a requested relative path, reject any canonical parent outside root, create only validated descendants, write to a same-directory random temporary file, flush, then rename. Never recursively delete or overwrite a directory.

```rust
impl WorkspaceWriter {
    pub fn write(&self, relative: &Path, bytes: &[u8]) -> Result<ArtifactFile, DevupError> {
        let target = self.resolve_confined_target(relative)?;
        atomic_write_same_directory(&target, bytes)?;
        Ok(ArtifactFile::from_bytes(target, bytes))
    }
}
```

- [ ] **Step 4: Wire output paths into completed conversions**

UI directory output writes deterministic `.tsx` component files and a manifest; JSON file output writes `devup.json`. Without `outputPath`, preserve the existing in-memory content response. On write failure return no partial success and remove the temporary file.

Run: `cargo test -p devup-mcp --test workspace_export`

Run: `cargo test -p devup-mcp --test downstream_integration`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp/src/server/workspace.rs crates/devup-mcp/src/server/tools.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/tests/workspace_export.rs crates/devup-mcp/tests/downstream_integration.rs
git commit -m "feat: export devup artifacts safely"
```

### Task 5: Live Search Smoke, Changepack and Final Verification

**Files:**
- Modify: `crates/devup-mcp/tests/live_figma_contract.rs`
- Modify: `README.md`
- Modify: `.changepacks/changepack_log_figma_remote_mcp.json`

**Interfaces:**
- Produces: opt-in live search smoke and release-ready search/export feature.

- [ ] **Step 1: Add an ignored live name-search smoke**

Against file `85CgSws3o5XsLv7aAwWJyS`, search exact name `A : STORY-F-PROOFREAD` through host fallback. Assert at least one result has the expected original name, type FRAME and a canonical URL whose node ID parses. Do not snapshot the remaining names or raw metadata.

- [ ] **Step 2: Run the live smoke only in the authenticated environment**

Run: `cargo test -p devup-mcp --test live_figma_contract -- --ignored`

Expected: search smoke and payload round-trip pass; no response body is written under the repository.

- [ ] **Step 3: Add changepack and finish documentation**

Record minor changes for `devup-mcp` and `devup-mcp-figma`: deterministic name search, stateless multi-result flow and safe workspace export.

- [ ] **Step 4: Run full verification**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Run: `cargo insta test --workspace --all-features`

Run: `cargo test --workspace --all-features`

Run: `cargo build --workspace --release`

Expected: every command exits 0 and no pending snapshots exist.

- [ ] **Step 5: Verify clean scope and commit**

Run: `git status --short`

Run: `git diff --check`

Expected: only changepack/documentation changes remain before commit; no live Figma payload or secret exists.

```bash
git add .changepacks/changepack_log_figma_remote_mcp.json README.md crates/devup-mcp/tests/live_figma_contract.rs
git commit -m "docs: release figma search workflow"
```
