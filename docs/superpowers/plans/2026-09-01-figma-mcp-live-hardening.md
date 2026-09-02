# Figma MCP Live Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for execution tracking.

**Goal:** Make nested-node exploration, host handoff continuation, installed-binary diagnosis, and strict fidelity reporting agree with actual WQUW-151 behavior while preserving the MCP-only scope.

**Architecture:** Keep acquisition and scene-graph projection in `devup-mcp-figma`, code-generation fidelity in `devup-mcp-devup-ui`, and CLI/session orchestration in `devup-mcp`. Exploration promotes only its internal scope anchor; handoff mutations become transactional under the existing store lock; CLI diagnostics remain local and stdout-safe; fidelity derives strictness from one typed impact contract.

**Tech Stack:** Rust 2024, Tokio, rmcp, Serde/serde_json, compiled read-only Figma Plugin API JavaScript, Node 24 test runner, cargo-insta.

**Spec:** `docs/superpowers/specs/2026-09-01-figma-mcp-live-hardening-design.md`

## Global Constraints

- [ ] Apply strict red-green-refactor for every production behavior change and record the intended RED failure before implementing.
- [ ] Change only the MCP repository. Do not modify `girok-space` application code or its worktree.
- [ ] Preserve the user's existing dirty work. Review and stage files by explicit path; never reset, checkout, clean, or bulk-revert.
- [ ] Keep Figma access read-only. Do not accept arbitrary JavaScript or add mutation tools.
- [ ] Never print or persist OAuth material, raw live payloads, screenshots, design text, account identifiers, or user filesystem paths in diagnostics.
- [ ] Keep public MCP inputs and existing response fields backward compatible; additions to error `details`, CLI output, and diagnostics are allowed.
- [ ] Keep projection output at or below 14,000 JSON characters and retain existing session/result memory limits.
- [ ] Because implementation files already contain pre-existing uncommitted work, do not create task commits automatically. Use scoped diffs and a final explicit-path review instead.
- [ ] After deterministic verification, repeat actual WQUW-151 probes and report candidate IDs/counts, tool-call counts, bytes/chunks, elapsed time, and fallback state only.

## Task 1: Promote Nested Explore Anchors Without Enlarging the Projection

**Files:**

- Modify: `crates/devup-mcp-figma/src/scripts/explore.js`
- Modify: `crates/devup-mcp-figma/src/explore.rs`
- Modify: `crates/devup-mcp-figma/tests/explore_script_behavior.mjs`
- Modify: `crates/devup-mcp-figma/tests/explore.rs`
- Modify: `crates/devup-mcp/tests/figma_explore.rs`
- Modify only if needed: `crates/devup-mcp/tests/fixtures/wquw-151-neighborhood.json`

### Step 1: Add script-level RED coverage for the current live hierarchy

- [ ] Construct a literal mock graph `PAGE -> SECTION 4217:7743 -> heading 3879:35481 + wrappers/screens` in `explore_script_behavior.mjs`.
- [ ] Invoke the real compiled `explore.js` once with the heading and once with the SECTION.
- [ ] Assert both projections contain the same ten literal screen IDs in visual order, preserve the heading node, include a complete `parentId` chain to the SECTION, and stay within 14,000 JSON characters.
- [ ] Assert the mock records no more than `projectionLimit * 8` descendant visits.

Run:

```text
node --test --test-name-pattern="nested heading" crates/devup-mcp-figma/tests/explore_script_behavior.mjs
```

Expected RED: the heading-linked run does not traverse the enclosing SECTION and therefore omits its screens.

### Step 2: Implement the minimum JavaScript scope-anchor promotion

- [ ] Replace the single `anchorPeer` concept with `requestedAnchor`, nearest ancestor `scopeAnchor`, and `page`.
- [ ] Select the nearest SECTION for non-screen descendants; otherwise retain the current PAGE-direct peer behavior.
- [ ] Use `scopeAnchor` for eligible selection, bounded SECTION traversal, required IDs, and truncation accounting.
- [ ] Use `requestedAnchor` for the mandatory projected anchor and leave exact screen selection unchanged.
- [ ] Include only page, scope anchor, requested anchor, necessary ancestor chain, and bounded candidate chains.

Run the focused Node test again. Expected GREEN with the existing three projection-bound tests still passing.

### Step 3: Add Rust RED coverage for semantic promotion

- [ ] Add a synthetic snapshot whose original target is a heading below a SECTION and whose ten screens are nested through wrapper groups.
- [ ] Assert `ExploreResult.anchor.node_id == "3879:35481"`, `group.heading_node_id` retains that ID, and candidate IDs/order exactly match a SECTION-target call.
- [ ] Add mutations for a PAGE-direct heading and exact screen anchor to prove their prior behavior remains unchanged.

Run:

```text
cargo test -p devup-mcp-figma --test explore nested_heading -- --nocapture
cargo test -p devup-mcp --test figma_explore nested_heading -- --nocapture
```

Expected RED: `explore_snapshot` enters the generic below-heading branch and returns no SECTION descendants.

### Step 4: Implement Rust ancestor promotion

- [ ] Build a parent lookup from both declared `childrenIds` and preserved `parentId` fields.
- [ ] Resolve the nearest SECTION ancestor without replacing the public anchor or target kind.
- [ ] Reuse the SECTION candidate filtering, nested-screen suppression, visual sorting, notes, bounds, and truncation logic with the resolved scope ID.
- [ ] Set selection reasons to distinguish `inside-section` while preserving `exact-screen-anchor` for screens.

### Step 5: Verify Task 1

Run:

```text
node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs
cargo test -p devup-mcp-figma --test explore
cargo test -p devup-mcp --test figma_explore
```

Expected: all pass; heading and SECTION links yield the same ten candidate IDs/order; exact screen remains one candidate.

## Task 2: Make Handoff Acceptance Transactional and Lease-Aware

**Files:**

- Modify: `crates/devup-mcp/src/server/handoff.rs`
- Modify: `crates/devup-mcp/tests/handoff.rs`
- Modify only if public detail assertions require it: `crates/devup-mcp/tests/source_orchestration.rs`

### Step 1: Add session-survival RED tests

- [ ] Add `invalid_call_id_does_not_destroy_the_session`: submit a literal unknown call ID, assert reason `unknown_call`, then submit the original call and advance.
- [ ] Add `collector_rejection_keeps_the_call_pending`: submit a structurally malformed metadata result, assert the collector error, then submit corrected metadata under the same call ID and advance.
- [ ] Update the aggregate/per-result limit assertions so destructive removal is tested only where the approved limit policy requires it, not as an accidental consequence of validation order.

Run:

```text
cargo test -p devup-mcp --test handoff invalid_call_id -- --nocapture
cargo test -p devup-mcp --test handoff collector_rejection -- --nocapture
```

Expected RED: both corrected retries fail because `accept` removed the session before validation.

### Step 2: Implement validate-then-commit acceptance

- [ ] Keep the session present in `StoreState.sessions` while checking session expiry and call membership.
- [ ] Clone only the collector state needed to validate `CollectorSession::accept`; do not consume the pending call until collector acceptance succeeds.
- [ ] On success, replace the collector, move the call ID from pending to a bounded consumed-call set, add encoded bytes exactly once, and renew `expires_at` to `now + ttl`.
- [ ] Keep aggregate accounting unchanged on every error path.

### Step 3: Add consumed/expired/lease RED tests

- [ ] Assert replaying an accepted call returns `DEVUP_FIGMA_HANDOFF_INVALID` with `details.reason == "consumed"`.
- [ ] Assert a never-seen call returns the same code with `reason == "unknown_call"`.
- [ ] Advance fake time beyond an expired session and assert `DEVUP_FIGMA_HANDOFF_EXPIRED`, `retryable == true`, and `reason == "expired"` after pruning.
- [ ] Model three sequential valid continuation results with time advances below TTL and assert each returned expiry is renewed and the final call completes.
- [ ] Assert `next()` without an accepted result does not renew the lease.

Expected RED: consumed/unknown are conflated, expired state disappears, and accepted chunks do not renew the lease.

### Step 4: Add bounded state tombstones and finish state transitions

- [ ] Add small tombstones containing only session/call opaque IDs, terminal reason, and expiry; never store result bytes, call arguments, or collector payloads.
- [ ] Bound tombstones independently by count and TTL and prune them on store operations.
- [ ] Return structured safe details `{ "source": "host", "reason": ... }` for unknown session, unknown call, consumed call, and expired session.
- [ ] Preserve completion cleanup and total-result-byte subtraction exactly once.

### Step 5: Verify Task 2

Run:

```text
cargo test -p devup-mcp --test handoff
cargo test -p devup-mcp --test source_orchestration handoff -- --nocapture
```

Expected: all transitions pass, including concurrent replay with exactly one accepted result and no session loss.

## Task 3: Add Local Binary Self-Check and End-to-End stdio Smoke Coverage

**Files:**

- Create: `crates/devup-mcp/build.rs`
- Modify: `crates/devup-mcp/src/lib.rs`
- Modify: `crates/devup-mcp/src/main.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/tests/cli.rs`
- Create: `crates/devup-mcp/tests/stdio_smoke.rs`
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`

### Step 1: Add CLI RED tests

- [ ] Change the version test to parse `devup-mcp <package-version> (<build-id>)` and require a non-empty safe identifier containing only ASCII alphanumeric, `.`, `_`, or `-`.
- [ ] Add a `--self-check` child-process test that requires exit 0, empty stderr, exactly one JSON document on stdout, and literal keys `status`, `version`, `buildId`, `binary`, `credentialBackend`, and `serverConfig`.
- [ ] Assert the serialized JSON contains none of the process current directory, environment token names, credential values, or OAuth endpoints.
- [ ] Add parser tests proving `--self-check` is exclusive and does not accept `--allow-write-root`.

Run:

```text
cargo test -p devup-mcp --test cli -- --nocapture
```

Expected RED: build ID and `SelfCheck` action do not exist.

### Step 2: Implement deterministic build identity and self-check

- [ ] Have `build.rs` prefer the CI-provided `DEVUP_MCP_BUILD_ID`, then a short local Git commit obtained without failing non-Git package builds, and finally `source-unknown`.
- [ ] Emit rerun directives for the environment variable and Git HEAD/ref files without embedding paths in runtime output.
- [ ] Add `CliAction::SelfCheck` and a serializable report whose status values reveal only pass/fail/degraded capability.
- [ ] Initialize the production credential backend object and server configuration locally without reading credential contents, opening a browser, or making network requests.
- [ ] Ensure both `--version` and `--self-check` return before tracing/stdin startup so stdout cannot contain protocol/log noise.

### Step 3: Add real stdio protocol RED test

- [ ] Spawn `CARGO_BIN_EXE_devup-mcp` with piped stdin/stdout/stderr.
- [ ] Send newline-delimited JSON-RPC `initialize`, `notifications/initialized`, `tools/list`, and `tools/call` for `devup_figma_auth` with `{ "action": "status" }`.
- [ ] Read responses with explicit timeouts, assert matching request IDs, all expected tool names, and a safe connected/disconnected auth status.
- [ ] Close stdin, require a clean child exit, and assert stderr contains no secret-shaped values or raw JSON-RPC payloads.

Run:

```text
cargo test -p devup-mcp --test stdio_smoke -- --nocapture
```

Expected RED: any startup/configuration/stdio framing defect is exposed by the real child process.

### Step 4: Wire CI and operator diagnostics

- [ ] Add the stdio smoke test explicitly to the cross-platform CI job before the workspace suite.
- [ ] Document `--version`, `--self-check`, then host restart/reconnect as the diagnostic order after binary hash/version replacement.
- [ ] State precisely that a newly healthy child process cannot revive a stale pipe retained by the MCP host.

### Step 5: Verify Task 3

Run:

```text
cargo test -p devup-mcp --test cli
cargo test -p devup-mcp --test stdio_smoke
cargo test -p devup-mcp --test stdio_tools
```

Expected: all pass on the actual release-shaped binary boundary without network or OAuth login.

## Task 4: Unify Fidelity Strictness and Remove False Absolute Fallbacks

**Files:**

- Modify: `crates/devup-mcp-devup-ui/src/provenance.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/layout.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/component.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/provenance.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/codegen.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/wquw_151.rs`
- Modify if the reviewed output changes: `crates/devup-mcp-devup-ui/tests/snapshots/*.snap`
- Modify only if public status coverage is missing: `crates/devup-mcp/tests/projection_quality.rs`

### Step 1: Add strict-impact RED coverage

- [ ] Construct a literal otherwise-complete `FidelityReport` with `approximated == 1` and assert `strict_compatible() == false`.
- [ ] Assert the same approximated diagnostic produces public `quality.projection == "approximated"` and `status == "partial"` even when diagnostics are omitted from output.

Run:

```text
cargo test -p devup-mcp-devup-ui --test provenance approximated -- --nocapture
cargo test -p devup-mcp --test projection_quality approximated -- --nocapture
```

Expected RED: `strict_compatible` currently ignores approximated impacts.

### Step 2: Enforce one strictness rule

- [ ] Require `approximated == 0`, `lossy == 0`, and `failed == 0` in `FidelityReport::strict_compatible`.
- [ ] Keep `none` informational and preserve existing complete coverage requirements.

### Step 3: Add exact-versus-approximated absolute-layout RED tests

- [ ] Add an exact absolute child fixture with parent-relative coordinates, width, height, and fully emitted position/size props; assert no `DEVUP_CODEGEN_ABSOLUTE_FALLBACK` and complete layout source-map coverage.
- [ ] Add a nonrepresentable fixture with an unsupported transform/constraint or missing parent geometry; assert the diagnostic remains `Approximated` and strict compatibility is false.
- [ ] Split the existing mixed unsupported-visual test so mask/effect coverage does not force every absolute node to be approximated.

Expected RED: `add_fallback_diagnostics` emits the absolute fallback unconditionally.

### Step 4: Implement an evidence-based absolute representation predicate

- [ ] Make layout emission return or expose a small typed result describing which required absolute fields were emitted.
- [ ] Treat an absolute node as exact only when parent-relative position, both required dimensions or fixed edges, and supported transforms/constraints are represented.
- [ ] Add the fallback diagnostic only when that predicate is false; do not infer exactness merely from the presence of `pos="absolute"`.
- [ ] Keep source-map `layoutPositioning`, `x/y`, width/height, and edge entries aligned with the predicate.

### Step 5: Validate WQUW absolute nodes

- [ ] Assert nodes `3879:35540` and `3879:35564` either have complete literal position/size source mappings and no fallback, or retain the fallback with a specific missing representation assertion.
- [ ] Assert WQUW output diagnostics, fidelity impacts, strict compatibility, and public status all tell the same story.
- [ ] Review any snapshot diff by node/property rather than accepting it wholesale.

### Step 6: Verify Task 4

Run:

```text
cargo test -p devup-mcp-devup-ui --test codegen
cargo test -p devup-mcp-devup-ui --test provenance
cargo test -p devup-mcp-devup-ui --test wquw_151
cargo test -p devup-mcp --test projection_quality
cargo insta pending-snapshots
```

Expected: no pending unreviewed snapshots; exact absolute nodes are diagnostic-free and all remaining approximations make strict output partial.

## Task 5: Full Verification and Actual Inefficiency Audit

**Files:**

- Modify: `.changepacks/changepack_log_figma_remote_mcp.json`
- Modify: `README.md`
- Modify only when the measurement exposes a regression: focused production/test files from Tasks 1–4

### Step 1: Run deterministic quality gates

- [ ] Run formatting and lint checks:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- [ ] Run all tests and release build:

```text
node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs
cargo test --workspace --all-features
cargo insta pending-snapshots
cargo build --workspace --release
```

- [ ] Run `target/release/devup-mcp --version` and `--self-check`; record only version, build ID, and safe status fields.

### Step 2: Repeat the live WQUW-151 exploration measurements

- [ ] Through the official read-only Figma MCP, explore heading `3879:35481` and SECTION `4217:7743` separately.
- [ ] Assert candidate IDs and order are identical and contain ten screens.
- [ ] Record calls, projected node count, serialized character count, truncation flag, and elapsed time without recording node text.
- [ ] Compare those measurements with the prior baseline: heading `0` candidates versus SECTION `10`, one official call, 25 projected nodes, no truncation.

### Step 3: Repeat the exact-node and handoff measurements

- [ ] Collect target `3879:35518` through the host handoff and assert 144 nodes, 20 variables, 11 styles, three PNG envelope chunks, and completion.
- [ ] During a disposable synthetic session, submit one invalid call ID before the valid result and verify the valid continuation still completes.
- [ ] Record total calls, bytes, chunks, elapsed time, lease renewals, and fallback state only.

### Step 4: Diagnose remaining inefficiency from evidence

- [ ] Compare live values against the pre-change baseline and identify any unnecessary traversal, duplicate official calls, oversized projection, lease churn, fallback, reconnect ambiguity, or codegen diagnostic work.
- [ ] For every material inefficiency, add a focused failing test first, implement the smallest bounded improvement, and rerun the affected live measurement.
- [ ] Do not optimize a path whose measurement is dominated by the official upstream call unless the MCP can reduce call count or payload safely.

### Step 5: Final privacy and change review

- [ ] Inspect explicit changed paths for secrets, raw design payloads, screenshots, account identifiers, arbitrary JS, or Figma writes.
- [ ] Confirm `girok-space` has no changes from this implementation.
- [ ] Update README and changepack with the corrected contracts and measured before/after counts.
- [ ] Run `git diff --check` and report existing unrelated dirty files separately from the implementation result.

## Execution Checkpoints

- [ ] After Task 1, do not continue if heading and SECTION candidate IDs differ.
- [ ] After Task 2, do not continue if any invalid continuation can delete an otherwise valid session or corrupt aggregate byte accounting.
- [ ] After Task 3, do not claim transport health unless a fresh child completes initialize, tools/list, and auth status.
- [ ] After Task 4, do not claim exact fidelity while any approximated/lossy/failed impact remains.
- [ ] After Task 5, distinguish MCP-controlled inefficiency from official Figma MCP latency and report both with measured evidence.
