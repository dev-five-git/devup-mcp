# Figma Collection and Live JSON Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build direct/host-fallback Figma collection through one Rust state machine and validate its real JSON contract before any compatibility fixture schema is frozen.

**Architecture:** `devup-mcp-figma` owns source-independent read calls, the resumable collector, exhaustive raw payload types, and upstream error classification. `devup-mcp` owns short-lived host handoff sessions and resumes the same collector used by the direct path. A live contract probe inspects the provided Figma node in memory, round-trips it through the Rust types, and emits only a non-sensitive structural report.

**Tech Stack:** Rust 1.88, edition 2024, Tokio, rmcp 3.1, serde/serde_json, sha2, Axum OAuth callback, cargo test/clippy/fmt.

**Spec:** `docs/superpowers/specs/2026-08-31-figma-host-fallback-parity-design.md`

## Global Constraints

- `devup-mcp` remains Cargo-only: no `package.json`, Bun, Node, or local JavaScript runtime.
- Figma access is read-only; only built-in script templates and allowlisted read tools may be emitted.
- `auto` never opens a browser. Only `devup_figma_auth {"action":"login"}` starts OAuth.
- Direct Catalog/auth/capability failures may fall back to host; invalid URL, missing node, version conflict, and rate limit may not.
- Host sessions are memory-only, CSPRNG-addressed, expire after 10 minutes, and enforce 8 sessions, 16 MiB per result, and 64 MiB aggregate limits.
- Unknown Figma fields survive deserialize/serialize; functions, cycles, and binary payloads are represented only by safe IDs/metadata.
- Live Figma design values are never written to the repository or logs.
- Every production change follows TDD and ends in a focused commit.

## File Structure

### Create

- `crates/devup-mcp-figma/src/source.rs` — source policy and fallback classification.
- `crates/devup-mcp-figma/src/collector.rs` — resumable collection state machine shared by direct and host execution.
- `crates/devup-mcp-figma/src/metadata.rs` — metadata discovery and deterministic page/node planning.
- `crates/devup-mcp-figma/src/payload.rs` — real collected payload envelope and structural report.
- `crates/devup-mcp-figma/tests/source_policy.rs` — fallback decision matrix.
- `crates/devup-mcp-figma/tests/collector.rs` — node/page/file planning, chunking, version and payload tests.
- `crates/devup-mcp-figma/tests/payload_contract.rs` — serde round-trip and redacted structural report tests.
- `crates/devup-mcp/src/server/handoff.rs` — bounded in-memory continuation store.
- `crates/devup-mcp/tests/handoff.rs` — expiry, replay, size and context validation.
- `crates/devup-mcp/tests/source_orchestration.rs` — direct/auto/host server behavior.
- `crates/devup-mcp/tests/live_figma_contract.rs` — ignored opt-in live contract probe.

### Modify

- `Cargo.toml` — add shared dependencies only when a task consumes them.
- `crates/devup-mcp-figma/src/lib.rs` — export source, collector, metadata and payload interfaces.
- `crates/devup-mcp-figma/src/url.rs` — serialize/deserialize the validated Figma target inside collected payloads.
- `crates/devup-mcp-figma/src/errors.rs` — stable direct/handoff error codes and safe details.
- `crates/devup-mcp-figma/src/upstream.rs` — typed upstream failures and serializable read calls/results.
- `crates/devup-mcp-figma/src/snapshot.rs` — raw response preservation and strict merge invariants.
- `crates/devup-mcp-figma/src/scripts/snapshot.js` — exhaustive read-only serializer based on the checked-in manifest.
- `crates/devup-mcp-figma/src/scripts/variables.js` — local variables, collections and styles with explicit completeness.
- `crates/devup-mcp-figma/src/plugin_api_manifest.json` — public data-property contract.
- `crates/devup-mcp/src/server/tools.rs` — source policy, scope and continuation inputs.
- `crates/devup-mcp/src/server/mod.rs` — source orchestration and continuation tool.
- `crates/devup-mcp/tests/stdio_tools.rs` — public tool schemas.
- `crates/devup-mcp/tests/downstream_integration.rs` — conversion through the new collector.
- `README.md` — direct versus host behavior and live probe instructions.
- `.changepacks/*.md` — focused package change record.

---

### Task 1: Source Policy and Typed Upstream Failures

**Files:**
- Create: `crates/devup-mcp-figma/src/source.rs`
- Create: `crates/devup-mcp-figma/tests/source_policy.rs`
- Modify: `crates/devup-mcp-figma/src/errors.rs`
- Modify: `crates/devup-mcp-figma/src/upstream.rs`
- Modify: `crates/devup-mcp-figma/src/lib.rs`

**Interfaces:**
- Produces: `SourcePolicy::{Auto, Direct, Host}` with camelCase serde values.
- Produces: `SelectedSource::{Direct, Host}` returned by server source selection.
- Produces: `UpstreamFailureKind::{CatalogRejected, AuthUnavailable, CapabilityUnavailable, PermissionDenied, RateLimited, NodeNotFound, VersionChanged, Transport, InvalidResponse}`.
- Produces: `pub fn fallback_allowed(policy: SourcePolicy, kind: UpstreamFailureKind) -> bool`.
- Produces: `DevupError::with_details(code, message, retryable, details)` without sensitive raw errors.

- [ ] **Step 1: Write the failing fallback matrix tests**

```rust
#[test]
fn auto_falls_back_only_for_identity_or_capability_failures() {
    use SourcePolicy::Auto;
    assert!(fallback_allowed(Auto, UpstreamFailureKind::CatalogRejected));
    assert!(fallback_allowed(Auto, UpstreamFailureKind::AuthUnavailable));
    assert!(fallback_allowed(Auto, UpstreamFailureKind::CapabilityUnavailable));
    assert!(fallback_allowed(Auto, UpstreamFailureKind::PermissionDenied));
    assert!(!fallback_allowed(Auto, UpstreamFailureKind::RateLimited));
    assert!(!fallback_allowed(Auto, UpstreamFailureKind::NodeNotFound));
    assert!(!fallback_allowed(Auto, UpstreamFailureKind::VersionChanged));
}
```

- [ ] **Step 2: Run the focused test and confirm missing interfaces fail**

Run: `cargo test -p devup-mcp-figma --test source_policy`

Expected: compile failure for `SourcePolicy` and `fallback_allowed`.

- [ ] **Step 3: Implement the enums, error codes and safe classifier**

Add the spec codes to `ErrorCode` and implement a classifier that maps HTTP/status/message metadata to `UpstreamFailureKind` without storing the raw response body. Extend `UpstreamResult` to derive `Serialize`, `Deserialize` and `PartialEq`, and expose `ReadToolCall` as serializable safe tool name plus arguments.

```rust
pub fn fallback_allowed(policy: SourcePolicy, kind: UpstreamFailureKind) -> bool {
    policy == SourcePolicy::Auto
        && matches!(
            kind,
            UpstreamFailureKind::CatalogRejected
                | UpstreamFailureKind::AuthUnavailable
                | UpstreamFailureKind::CapabilityUnavailable
                | UpstreamFailureKind::PermissionDenied
        )
}
```

- [ ] **Step 4: Add classifier/redaction cases and run them**

Test Catalog rejection, 401/auth, missing `use_figma`, 403 permission, 429, node-not-found, generic transport, and messages containing bearer-token-shaped text. Assert serialized `DevupError` contains only stable code, source, retry hint and status.

Run: `cargo test -p devup-mcp-figma --test source_policy`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp-figma/src/errors.rs crates/devup-mcp-figma/src/source.rs crates/devup-mcp-figma/src/upstream.rs crates/devup-mcp-figma/src/lib.rs crates/devup-mcp-figma/tests/source_policy.rs
git commit -m "feat: classify figma source failures"
```

### Task 2: Resumable Collector State Machine

**Files:**
- Create: `crates/devup-mcp-figma/src/metadata.rs`
- Create: `crates/devup-mcp-figma/src/collector.rs`
- Create: `crates/devup-mcp-figma/tests/collector.rs`
- Modify: `crates/devup-mcp-figma/src/snapshot.rs`
- Modify: `crates/devup-mcp-figma/src/lib.rs`

**Interfaces:**
- Consumes: `ReadToolCall`, `UpstreamResult`, `SnapshotChunk`, `merge_chunks`.
- Produces: `CollectionScope::{Node, Page, File}`.
- Produces: `CollectionRequest { target: FigmaTarget, scope: CollectionScope, include_variables: bool, include_context: bool }`.
- Produces: `CollectorSession::new(request)`, `advance(&mut self) -> Result<CollectorStep, DevupError>`, and `accept(&mut self, call_id: &str, result: UpstreamResult) -> Result<(), DevupError>`.
- Produces: `PlannedCall { id: String, call: ReadToolCall, expected_file_key: String, expected_node_id: Option<String> }`.

- [ ] **Step 1: Write failing state-machine tests**

Cover a node request that first emits metadata, then a snapshot call; a page request that emits direct-child chunks in child order; a file request that visits every page; duplicate equal nodes; conflicting nodes; stale call IDs; mismatched file keys; and version changes.

```rust
let mut session = CollectorSession::new(node_request("file", "1:2"));
let CollectorStep::Call(metadata) = session.advance().unwrap() else { panic!() };
assert_eq!(metadata.call.tool_name(), "get_metadata");
session.accept(&metadata.id, metadata_result()).unwrap();
let CollectorStep::Call(snapshot) = session.advance().unwrap() else { panic!() };
assert_eq!(snapshot.call.tool_name(), "use_figma");
```

- [ ] **Step 2: Run the tests and confirm they fail**

Run: `cargo test -p devup-mcp-figma --test collector`

Expected: compile failure for the collector types.

- [ ] **Step 3: Implement deterministic metadata parsing and planning**

`metadata.rs` must recursively find metadata in structured content or text JSON, preserve page/node names and types, and emit stable child order. `collector.rs` stores pending calls in a `BTreeMap`, accepts each ID once, and refuses `finish` while any required call remains.

```rust
pub enum CollectorStep {
    Call(PlannedCall),
    Complete(CollectedPayload),
}

pub fn advance(&mut self) -> Result<CollectorStep, DevupError>;
```

Use metadata counts to snapshot a small node whole and split a large Page/Section/Frame by direct child. Cap in-flight calls at four; the state machine may expose at most four pending calls at once.

- [ ] **Step 4: Run collector and existing snapshot tests**

Run: `cargo test -p devup-mcp-figma --test collector`

Run: `cargo test -p devup-mcp-figma --test snapshot`

Expected: all tests pass, including version conflict and child-order assertions.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp-figma/src/metadata.rs crates/devup-mcp-figma/src/collector.rs crates/devup-mcp-figma/src/snapshot.rs crates/devup-mcp-figma/src/lib.rs crates/devup-mcp-figma/tests/collector.rs
git commit -m "feat: add resumable figma collector"
```

### Task 3: Exhaustive Serializer and Collected Payload

**Files:**
- Create: `crates/devup-mcp-figma/src/payload.rs`
- Create: `crates/devup-mcp-figma/tests/payload_contract.rs`
- Modify: `crates/devup-mcp-figma/src/scripts/snapshot.js`
- Modify: `crates/devup-mcp-figma/src/scripts/variables.js`
- Modify: `crates/devup-mcp-figma/src/plugin_api_manifest.json`
- Modify: `crates/devup-mcp-figma/src/collector.rs`
- Modify: `crates/devup-mcp-figma/src/lib.rs`

**Interfaces:**
- Produces: `CollectedPayload { target, scope, metadata, snapshot, variables, styles, completeness, source_version }` with serde round-trip.
- Produces: `PayloadCompleteness::{FullLocalPlusUsedRemote, UsedTokens, ResolvedValuesOnly}`.
- Produces: `PayloadStructure::from_payload(&CollectedPayload)` containing field names, JSON kinds, counts and a schema-only hash but no design values or hashes derived from design values.
- Produces: `pub fn validate_payload_context(payload, expected_target) -> Result<(), DevupError>`.

- [ ] **Step 1: Write failing raw-preservation and structure-report tests**

Create a synthetic payload containing a future node field, failed getter, remote variable alias, two modes, paint/text/effect styles, asset metadata and a text value such as `PRIVATE_TEXT_MUST_NOT_APPEAR`. Assert round-trip equality and assert the structure report contains keys/types but not that value.

```rust
let encoded = serde_json::to_value(&payload).unwrap();
let decoded: CollectedPayload = serde_json::from_value(encoded).unwrap();
assert_eq!(decoded, payload);
let report = PayloadStructure::from_payload(&payload);
assert!(!serde_json::to_string(&report).unwrap().contains("PRIVATE_TEXT"));
```

- [ ] **Step 2: Run the payload test and confirm it fails**

Run: `cargo test -p devup-mcp-figma --test payload_contract`

Expected: compile failure for `CollectedPayload` and `PayloadStructure`.

- [ ] **Step 3: Implement the payload and harden built-in scripts**

Keep `fields`, `extra` and `fieldErrors` separate. Serialize node references to IDs, recurse through plain records/arrays with cycle detection, never serialize functions, and retain asset IDs/metadata instead of bytes. Extend `variables.js` to return local collections/variables and local paint/text/effect/grid styles with explicit `localComplete` and `usedRemoteComplete` flags. Every Plugin API call must be a read method from the allowlist.

Add serde derives to `FigmaTarget` and `CollectionScope` so the target already validated by the URL parser is the exact target stored in `CollectedPayload`.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectedPayload {
    pub target: FigmaTarget,
    pub scope: CollectionScope,
    pub metadata: serde_json::Value,
    pub snapshot: Snapshot,
    pub variables: Option<UpstreamResult>,
    pub styles: Option<UpstreamResult>,
    pub completeness: PayloadCompleteness,
    pub source_version: Option<String>,
}
```

- [ ] **Step 4: Add the manifest drift assertion**

Extend `upstream_contract.rs` so a synthetic added public data property fails until present in `plugin_api_manifest.json`. Assert the snapshot script includes no assignment to Figma nodes and none of `create`, `set`, `remove`, `append`, `insert`, `deleteAsync`, or write-tool names.

Run: `cargo test -p devup-mcp-figma --test upstream_contract`

Run: `cargo test -p devup-mcp-figma --test payload_contract`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp-figma/src/payload.rs crates/devup-mcp-figma/src/collector.rs crates/devup-mcp-figma/src/url.rs crates/devup-mcp-figma/src/scripts crates/devup-mcp-figma/src/plugin_api_manifest.json crates/devup-mcp-figma/src/lib.rs crates/devup-mcp-figma/tests/payload_contract.rs crates/devup-mcp-figma/tests/upstream_contract.rs
git commit -m "feat: preserve complete figma payloads"
```

### Task 4: Bounded Host Handoff Sessions

**Files:**
- Create: `crates/devup-mcp/src/server/handoff.rs`
- Create: `crates/devup-mcp/tests/handoff.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/Cargo.toml`

**Interfaces:**
- Consumes: `CollectorSession`, `PlannedCall`, `UpstreamResult`.
- Produces: `HandoffStore::begin(PendingOperation, CollectorSession)`, `next(session_id)`, `accept(session_id, call_id, Value)`, and `remove(session_id)`.
- Produces: public `ContinueInput { session_id: String, call_id: String, result: Value }`.
- Produces: `NeedsFigma { session_id, expires_at, calls, resume_tool }` with tool arguments generated only from `ReadToolCall`.

- [ ] **Step 1: Write failing session safety tests with a fake clock**

Test 10-minute expiry, eight active sessions, 16 MiB single result, 64 MiB aggregate, CSPRNG-shaped opaque IDs, one-time call consumption, mismatched call/session IDs, and removal on completion.

- [ ] **Step 2: Run and confirm failures**

Run: `cargo test -p devup-mcp --test handoff`

Expected: compile failure for `HandoffStore`.

- [ ] **Step 3: Implement the in-memory store**

Use `tokio::sync::Mutex<BTreeMap<String, Session>>`; inject a clock trait for deterministic expiry tests. Compute input byte length before deserialization, validate the expected file/node context after deserialization, and erase a session on terminal success or terminal error.

```rust
#[derive(Clone)]
pub struct HandoffStore {
    sessions: Arc<tokio::sync::Mutex<BTreeMap<String, Session>>>,
    clock: Arc<dyn Clock>,
    limits: HandoffLimits,
}

pub struct HandoffLimits {
    pub ttl: Duration,
    pub max_sessions: usize,
    pub max_result_bytes: usize,
    pub max_total_bytes: usize,
}
```

- [ ] **Step 4: Run handoff tests including concurrent accepts**

Run: `cargo test -p devup-mcp --test handoff`

Expected: all tests pass and a replay receives `DEVUP_FIGMA_HANDOFF_INVALID`.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp/src/server/handoff.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/src/server/tools.rs crates/devup-mcp/Cargo.toml crates/devup-mcp/tests/handoff.rs Cargo.lock
git commit -m "feat: add figma host handoff sessions"
```

### Task 5: Wire Direct, Auto, Host and Continuation Tools

**Files:**
- Create: `crates/devup-mcp/tests/source_orchestration.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/tests/stdio_tools.rs`
- Modify: `crates/devup-mcp/tests/downstream_integration.rs`

**Interfaces:**
- Consumes: source policy, collector, handoff store and existing `DevupAuth`/`FigmaUpstream`.
- Produces: `devup_figma_continue` MCP tool.
- Produces: conversion result union `{status:"needs_figma", ...}` or `{status:"complete", artifacts, diagnostics, completeness, source}`.

- [ ] **Step 1: Write failing source-orchestration tests**

Cover: `auto` + disconnected returns handoff without login; `direct` + disconnected returns auth-required; `host` never calls auth/upstream; `auto` + connected uses direct; fallbackable direct error returns handoff; 429 and version errors do not; continuation can require multiple calls before completion; direct and host sequences produce byte-identical `CollectedPayload`.

- [ ] **Step 2: Run and confirm current auto-login behavior fails the contract**

Run: `cargo test -p devup-mcp --test source_orchestration`

Expected: failures showing `ensure_authenticated` invokes login and continuation is absent.

- [ ] **Step 3: Replace `ensure_authenticated` with source orchestration**

Add `sourcePolicy` and `scope` fields to tool inputs. Drive `CollectorSession` in a loop for direct mode and through `HandoffStore` for host mode. Keep explicit OAuth login behavior unchanged. Return safe structured results rather than MCP protocol errors for `needs_figma`.

```rust
async fn start_operation(
    &self,
    operation: PendingOperation,
    policy: SourcePolicy,
) -> Result<Json<Value>, ErrorData> {
    match self.select_source(policy).await.map_err(to_mcp_error)? {
        SelectedSource::Direct => self.run_direct(operation).await,
        SelectedSource::Host => self.begin_handoff(operation).await,
    }
}
```

- [ ] **Step 4: Register and test the public tool schemas**

Assert `tools/list` includes `devup_figma_continue`, and tool schemas expose only documented fields. Assert no host call ever contains write tool names or user-supplied code.

Run: `cargo test -p devup-mcp --test stdio_tools`

Run: `cargo test -p devup-mcp --test source_orchestration`

Run: `cargo test -p devup-mcp --test downstream_integration`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp/src/server crates/devup-mcp/tests/stdio_tools.rs crates/devup-mcp/tests/source_orchestration.rs crates/devup-mcp/tests/downstream_integration.rs
git commit -m "feat: add figma source fallback orchestration"
```

### Task 6: Validate the Real JSON Contract

**Files:**
- Create: `crates/devup-mcp/tests/live_figma_contract.rs`
- Modify: `README.md`
- Modify: `crates/devup-mcp-figma/src/payload.rs`

**Interfaces:**
- Consumes: `CollectedPayload`, `PayloadStructure`, host continuation sequence.
- Produces: ignored live test controlled by `DEVUP_MCP_LIVE_FIGMA=1` and host-supplied result files outside the repository when needed.
- Produces: finalized serde JSON contract used by the next implementation plan.

- [ ] **Step 1: Write an ignored live contract test before running it**

Use file `85CgSws3o5XsLv7aAwWJyS` and node `3879:35481`. The test must deserialize the real official-MCP result, validate context, round-trip the collected payload, assert at least one root/node, and create only `PayloadStructure` in memory.

```rust
#[tokio::test]
#[ignore = "requires explicit official Figma MCP results"]
async fn official_mcp_payload_round_trips_without_value_logging() {
    let payload = load_live_payload_from_process_input().await.unwrap();
    validate_payload_context(&payload, &expected_target()).unwrap();
    let round_trip: CollectedPayload = serde_json::from_value(
        serde_json::to_value(&payload).unwrap(),
    ).unwrap();
    assert_eq!(round_trip, payload);
    assert!(!round_trip.snapshot.nodes.is_empty());
}
```

- [ ] **Step 2: Run the ignored test without input and verify it stays skipped**

Run: `cargo test -p devup-mcp --test live_figma_contract`

Expected: one ignored test; no network or browser activity.

- [ ] **Step 3: Exercise the official host fallback with the provided node**

Invoke the new server tool with `sourcePolicy:"host"`, execute every returned official Figma MCP read call, and feed each result to `devup_figma_continue`. Keep results in process memory or an OS temporary file outside the repository; remove the temporary file after the test. Do not print raw result bodies.

- [ ] **Step 4: Fix only observed contract mismatches and rerun focused tests**

If the real envelope differs, add a redacted synthetic regression case containing the observed shape but fake values. Do not add guessed fields. Run:

`cargo test -p devup-mcp-figma --test payload_contract`

`cargo test -p devup-mcp --test source_orchestration`

`cargo test -p devup-mcp --test live_figma_contract -- --ignored`

Expected: round-trip and context checks pass; no raw payload exists under `git status --short`.

- [ ] **Step 5: Commit the finalized contract and documentation**

```bash
git add crates/devup-mcp-figma/src/payload.rs crates/devup-mcp-figma/tests/payload_contract.rs crates/devup-mcp/tests/live_figma_contract.rs README.md
git commit -m "test: validate live figma json contract"
```

### Task 7: Plan-One Verification and Changepack

**Files:**
- Modify: `.changepacks/changepack_log_figma_remote_mcp.json`
- Modify: `README.md`

**Interfaces:**
- Consumes: all preceding tasks.
- Produces: a reviewable working milestone that obtains and round-trips real Figma JSON through direct or host source without defining compatibility fixtures.

- [ ] **Step 1: Add the changepack**

Record minor changes for `devup-mcp` and `devup-mcp-figma`: host fallback, resumable collection, exhaustive payload preservation and the live contract gate. Do not include actual Figma content.

- [ ] **Step 2: Run formatting and focused suites**

Run: `cargo fmt --all -- --check`

Run: `cargo test -p devup-mcp-figma --all-features`

Run: `cargo test -p devup-mcp --all-features`

Expected: all non-live tests pass.

- [ ] **Step 3: Run workspace lint and release build**

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Run: `cargo test --workspace --all-features`

Run: `cargo build --workspace --release`

Expected: all commands exit 0.

- [ ] **Step 4: Verify repository privacy and scope**

Run: `git status --short`

Run: `git diff --check`

Run: `rg -n "85CgSws3o5XsLv7aAwWJyS|3879:35481" . -g '!docs/**' -g '!crates/devup-mcp/tests/live_figma_contract.rs'`

Expected: only the intentional live-test reference exists; no raw design JSON, screenshot, token, OAuth code or temporary payload is tracked.

- [ ] **Step 5: Commit**

```bash
git add .changepacks/changepack_log_figma_remote_mcp.json README.md
git commit -m "docs: document figma collection fallback"
```
