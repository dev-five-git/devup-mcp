# Figma Single-Call Envelope Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collect an exact Figma node subtree and every referenced variable/style in one normal official Figma MCP call, validate the result losslessly in Rust, and fall back to the proven cursor collector without accepting partial fast-path data.

**Architecture:** A compile-in read-only Plugin API script serializes the existing snapshot schema plus exact used resources into a versioned JSON envelope and returns it in CRC-protected private chunks of a valid 1×1 PNG. Rust treats that image as untrusted transport, validates framing/schema/graph/resource integrity, normalizes it into the existing `CollectedParts`, and uses the unchanged DevupUI pipeline; the collector owns fast-first selection, clean restart, call statistics, and legacy optimization.

**Tech Stack:** Rust 1.88 workspace, `rmcp`, `serde`/`serde_json`, `base64`, compile-in Figma Plugin API JavaScript, `cargo insta`, Bun changepacks, official read-only Figma MCP.

**Spec:** `docs/superpowers/specs/2026-08-31-figma-single-call-envelope-design.md`

## Global Constraints

- Run every behavior change test-first: add one focused failing test, observe the intended failure, then add the smallest implementation and observe the pass.
- Do not add a JavaScript runtime, Bun runtime dependency, user-supplied upstream script, or a second code-generation pipeline.
- Do not call any Figma mutation API. The only write-shaped operation is `figma.io.write`, used solely as the official result transport.
- Never persist or report OAuth credentials, user identity, full private design text, variable values, or raw binary envelope bytes.
- A fast result is atomic: any transport, schema, graph, target, descriptor, or resource validation failure discards it and restarts legacy collection from cursor zero.
- Keep `standalone` root layout as the compatibility default; `embedded` is explicit.
- Preserve the 268 imported snapshot outputs byte-for-byte except fixtures intentionally corrected by stronger live Figma evidence.
- Use only `apply_patch` for authored source/document edits; snapshot regeneration and formatter output may use their normal tools.

---

## Task 1: Implement and harden the PNG envelope decoder

**Files:**

- Create: `crates/devup-mcp-figma/src/envelope.rs`
- Modify: `crates/devup-mcp-figma/src/lib.rs`
- Create: `crates/devup-mcp-figma/tests/envelope.rs`

- [ ] **Step 1: Add a valid single- and multi-chunk fixture builder in the integration test**

  Define test-only helpers that create a minimal valid PNG with `IHDR`, one or more `duVp` chunks, `IDAT`, and `IEND`. Each `duVp` payload starts with big-endian `sequence` and `total` values, and every PNG chunk receives a standards-compliant CRC32.

- [ ] **Step 2: Write the first failing round-trip test**

  Construct a schema-version-1 envelope containing two nodes, one variable, one style, and a descriptor. Call the planned `decode_fast_snapshot(&UpstreamResult, &FigmaTarget)` API and assert exact source IDs, node/resource counts, UTF-8 byte count, and transport byte/chunk statistics.

  Run: `cargo test -p devup-mcp-figma --test envelope valid_multichunk_envelope_round_trips -- --exact`

  Expected: compilation failure because `envelope` and `decode_fast_snapshot` do not exist.

- [ ] **Step 3: Add the minimal public decoder model and happy path**

  Introduce:

  ```rust
  pub struct FastSnapshotPayload {
      pub snapshot: SnapshotChunk,
      pub resources: UpstreamResult,
      pub stats: FastTransportStats,
  }

  pub struct FastTransportStats {
      pub raw_bytes: usize,
      pub wire_bytes: usize,
      pub chunk_count: usize,
  }

  pub fn decode_fast_snapshot(
      result: &UpstreamResult,
      target: &FigmaTarget,
  ) -> Result<FastSnapshotPayload, DevupError>;
  ```

  Find exactly one `image/png` content block, decode base64, parse checked PNG lengths, verify every PNG CRC, collect all `duVp` chunks, enforce unique contiguous sequence numbers, concatenate bytes, decode UTF-8/JSON, and verify the descriptor without logging content.

- [ ] **Step 4: Observe the round-trip test pass**

  Run the exact test from Step 2 and confirm it passes.

- [ ] **Step 5: Add table-driven failing corruption and limit tests**

  Cover bad PNG signature, truncated length, missing/duplicate/out-of-order envelope chunk, corrupted CRC, invalid UTF-8, invalid JSON, unsupported schema, image MIME mismatch, multiple image blocks, oversized PNG, oversized envelope, descriptor count mismatch, file key mismatch, root ID mismatch, declared node count mismatch, missing root, dangling child, and unresolved resource reference.

  Run: `cargo test -p devup-mcp-figma --test envelope`

  Expected: new cases fail at the missing validation boundary, never panic.

- [ ] **Step 6: Implement all validation boundaries with stable safe errors**

  Use checked arithmetic and explicit constants sized below the existing 16 MiB handoff result ceiling after base64 overhead. Validate that every referenced variable/style appears either in the resource result or its `unresolved` list. Error messages contain only category/count/ID metadata, never design text or values.

- [ ] **Step 7: Run focused crate checks and commit**

  Run:

  ```powershell
  cargo fmt --all -- --check
  cargo clippy -p devup-mcp-figma --all-targets -- -D warnings
  cargo test -p devup-mcp-figma --test envelope
  ```

  Commit: `feat(figma): decode validated single-call envelopes`

---

## Task 2: Build the lossless compile-in Figma fast snapshot script

**Files:**

- Create: `crates/devup-mcp-figma/scripts/fast_snapshot.js`
- Modify: `crates/devup-mcp-figma/src/upstream.rs`
- Modify: `crates/devup-mcp-figma/tests/upstream_contract.rs`
- Modify if shared serializer extraction is needed: `crates/devup-mcp-figma/scripts/snapshot.js`

- [ ] **Step 1: Write failing source-contract tests**

  Assert that `BuiltinScript::FastSnapshotEnvelope` exists, is selected by `ReadToolCall::fast_snapshot`, receives only Rust-substituted file/root IDs, walks the complete subtree without text-page limits, reads manifest plus runtime fields, captures styled text segments and individual stroke weights, collects used variable/style IDs, resolves both kinds in one `Promise.all`, encodes lone surrogates safely, emits `duVp` chunks, and calls only `figma.io.write` for output.

  Also reject mutation tokens such as `createRectangle`, `appendChild`, `setPluginData`, property assignments on document nodes, and imports/network access.

  Run: `cargo test -p devup-mcp-figma --test upstream_contract fast_snapshot -- --nocapture`

  Expected: compilation/assertion failure because the variant and script do not exist.

- [ ] **Step 2: Implement the script from the current serializer contract**

  Reuse the snapshot field/marker semantics rather than inventing a projection. Traverse breadth-first, serialize all public manifest and enumerable/prototype data properties, preserve getter errors and mixed/styled text, scan the completed serialized graph for used IDs, resolve resources in stable sorted order, assemble schema version 1 and integrity counts, encode UTF-8 in pure JavaScript, construct a valid 1×1 PNG with private ancillary chunks and CRC32, then call `figma.io.write` once.

- [ ] **Step 3: Add deterministic script-level contract cases**

  Assert placeholder escaping, stable resource ordering, no `maxFieldBytes`/`maxNodeBytes` truncation in fast mode, a bounded maximum envelope size before allocation, and a descriptor containing only schema/count/byte/chunk metadata.

- [ ] **Step 4: Run upstream and snapshot contract suites**

  Run:

  ```powershell
  cargo test -p devup-mcp-figma --test upstream_contract
  cargo test -p devup-mcp-figma --test snapshot_contract
  ```

- [ ] **Step 5: Commit**

  Commit: `feat(figma): add lossless single-call snapshot script`

---

## Task 3: Make the collector fast-first, atomic, observable, and safely fall back

**Files:**

- Modify: `crates/devup-mcp-figma/src/collector.rs`
- Modify: `crates/devup-mcp-figma/src/payload.rs`
- Modify: `crates/devup-mcp-figma/src/server/mod.rs`
- Modify: `crates/devup-mcp-figma/src/server/handoff.rs`
- Modify: `crates/devup-mcp-figma/tests/collector.rs`
- Modify or create: `crates/devup-mcp-figma/tests/fast_fallback.rs`

- [ ] **Step 1: Write a failing one-call collector test**

  Start an exact-node `CollectionRequest` with node scope and used resources. Assert the first emitted call is `FastSnapshotEnvelope`; after accepting one valid envelope, assert the session completes without metadata, snapshot-page, variable, or style calls and reports `figma_tool_calls=1`, `transport="png-envelope-v1"`, `fallback_used=false`.

  Run: `cargo test -p devup-mcp-figma --test collector exact_node_fast_path_completes_in_one_call -- --exact`

- [ ] **Step 2: Add collection stats and the minimal fast success transition**

  Add a serializable `CollectionStats` to `CollectedParts`/`CollectedPayload` with tool call count, transport name, fallback flag, node/variable/style counts, raw bytes, and wire bytes. Count calls when the state machine dispatches a unique upstream request, not when a result is parsed.

- [ ] **Step 3: Write failing atomic fallback tests**

  For text-only result, malformed PNG, target mismatch, graph mismatch, resource mismatch, and rejected direct upstream call, assert that the next request is legacy metadata at cursor zero; fast partial nodes/resources are absent; the final payload is produced only from legacy results; stats preserve `fallback_used=true` and a sanitized failure category.

- [ ] **Step 4: Implement an explicit fast-call rejection/fallback transition**

  Add a collector method used by direct execution when the fast upstream request errors. In `accept`, convert every decoder/integrity error for the fast call into the same clean restart. Retain hard errors for stale/wrong call IDs and failures after legacy collection begins. Host handoff uses the identical `accept` path when it submits a text/error-shaped result.

- [ ] **Step 5: Write a failing legacy combined-resource test**

  Feed a paginated legacy snapshot referencing variables and styles whose combined serialized request fits the configured limit. Assert one stable, sorted used-resource batch follows the final snapshot page instead of separate variable/style groups.

- [ ] **Step 6: Implement bounded combined batching**

  Merge variable/style IDs into one read-only used-resource request when it fits; deterministically split only when the serialized ID/count limit requires it. Keep existing partial completeness and unresolved behavior.

- [ ] **Step 7: Verify direct and handoff result observability**

  Add server tests that both execution modes expose the same safe fields: `figmaToolCalls`, `transport`, `fallbackUsed`, `nodeCount`, `variableCount`, `styleCount`, `rawBytes`, and `wireBytes`, without raw envelope or credentials.

- [ ] **Step 8: Run focused checks and commit**

  Run:

  ```powershell
  cargo test -p devup-mcp-figma --test collector
  cargo test -p devup-mcp-figma --test fast_fallback
  cargo test -p devup-mcp-figma
  ```

  Commit: `feat(figma): collect exact nodes with atomic fast fallback`

---

## Task 4: Correct mixed strokes and add explicit root layout semantics

**Files:**

- Modify: `crates/devup-mcp-devup-ui/src/codegen/component.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/layout.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/style.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/wquw_151.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/fixtures/wquw-151-proofread.json`
- Modify: `crates/devup-mcp-devup-ui/tests/snapshots/wquw_151__wquw_151_proofread_devup_ui.snap`
- Create or modify: `crates/devup-mcp-devup-ui/tests/codegen_layout.rs`
- Modify: `crates/devup-mcp-figma/src/server/tools.rs`
- Modify: `crates/devup-mcp-figma/src/server/handoff.rs`
- Modify: `crates/devup-mcp-figma/src/server/mod.rs`

- [ ] **Step 1: Write failing mixed-stroke unit tests**

  A node with mixed `strokeWeight` plus top/right/bottom/left values `1/0/0/0` must emit only `borderTop="solid 1px $border"`. A mixed/unsupported weight without individual side data must not guess `border="solid 1px ..."`.

  Run: `cargo test -p devup-mcp-devup-ui mixed_individual_stroke -- --nocapture`

  Expected: first case lacks `borderTop` or second case emits a guessed all-side border.

- [ ] **Step 2: Fix stroke selection without changing uniform strokes**

  Prefer explicit individual side fields whenever uniform weight is non-numeric/mixed. Preserve current inside/center/outside handling for genuinely numeric uniform weights. Omit the guessed border when neither form is trustworthy.

- [ ] **Step 3: Write failing standalone/embedded root tests**

  Add `RootLayout::{Standalone, Embedded}` to the planned options assertions. Standalone must retain selected-frame width, height, and relative containing block; embedded must omit only root fixed width/height/position while preserving descendants and all other root props.

- [ ] **Step 4: Implement root layout through every API boundary**

  Add `root_layout` to `CodegenOptions`, default it to standalone, parse optional MCP input `rootLayout` as `standalone|embedded`, preserve it through pending handoff state, and pass it into root layout property generation. Return the selected mode in the tool result.

- [ ] **Step 5: Correct the WQUW live fixture and snapshots**

  Add the four observed individual weights for node `3879:35564` without changing its 144-node graph, 13 variable names, or 11 typography names. Regenerate standalone and embedded snapshots and assert `borderTop`, all children, variable tokens, and typography tokens.

  Run:

  ```powershell
  $env:INSTA_UPDATE='always'
  cargo test -p devup-mcp-devup-ui --test wquw_151
  Remove-Item Env:INSTA_UPDATE
  cargo insta test -p devup-mcp-devup-ui --check
  ```

- [ ] **Step 6: Re-run all imported compatibility fixtures**

  Run:

  ```powershell
  cargo test -p devup-mcp-devup-ui --test compat_fixtures
  cargo test -p devup-mcp-devup-ui --test compat_manifest
  ```

  Expected: 268/268 JSON-to-golden snapshot comparisons still pass; only the dedicated live WQUW regression changes.

- [ ] **Step 7: Commit**

  Commit: `fix(devup-ui): preserve individual strokes and root modes`

---

## Task 5: Make upstream fixture coverage claims executable and accurate

**Files:**

- Modify: `crates/devup-mcp-devup-ui/tests/compat_manifest.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/compat_fixtures.rs`
- Create: `crates/devup-mcp-devup-ui/tests/fixtures/devup-figma-plugin/coverage-registry.json`
- Modify: `crates/devup-mcp-devup-ui/tests/fixtures/devup-figma-plugin/ledger.json`
- Modify: `crates/devup-mcp-devup-ui/tests/fixtures/devup-figma-plugin/README.md`
- Modify repository documentation that currently implies all 978 tests run as Rust parity tests.

- [ ] **Step 1: Add a failing coverage-registry integrity test**

  Require every one of the 978 ledger entries to reference a registered executable Rust test or an explicit non-parity classification with a non-empty reason. Require all 268 generated fixture paths to exist as JSON input plus golden output and to be exercised by `compat_fixtures`.

- [ ] **Step 2: Replace the nonexistent umbrella symbol mapping**

  Register actual Rust test symbols/categories. Map all snapshot-producing entries to the executable fixture parity harness; map TSX-affecting assertions such as strokes, layout, text styles, variables, instances, and responsive output to concrete unit/integration tests; leave plugin-runtime and write-only cases explicitly classified rather than falsely covered.

- [ ] **Step 3: Correct documentation and generated metadata wording**

  State precisely: 268 imported snapshot outputs pass byte parity, while 978 upstream tests are inventoried and categorized. Do not claim 978 independently executed Rust parity tests.

- [ ] **Step 4: Run corpus verification**

  Run:

  ```powershell
  cargo test -p devup-mcp-devup-ui --test compat_manifest
  cargo test -p devup-mcp-devup-ui --test compat_fixtures
  ```

- [ ] **Step 5: Commit**

  Commit: `test(devup-ui): make upstream coverage registry executable`

---

## Task 6: Live contract, release metadata, full verification, installation, and PR update

**Files:**

- Modify or create: `crates/devup-mcp/tests/live_figma_contract.rs`
- Modify: relevant README/tool documentation
- Create: one changepack file under `.changepacks/`
- Modify only if generated by tests: snapshots and lockfile

- [ ] **Step 1: Add opt-in read-only live contract assertions**

  For file `85CgSws3o5XsLv7aAwWJyS`, node `3879:35518`, assert one normal fast tool call, 144 nodes, 13 variables, 11 text styles, `borderTop` on node-derived output, and no credential/raw-content fields in stats. Add a deterministic forced-corruption mode that proves automatic legacy fallback without modifying Figma.

- [ ] **Step 2: Run the authenticated live contract when credentials are available**

  Use the existing devup-mcp credential store/OAuth flow; never print a token. If interactive OAuth is required, report only the localhost authorization action needed. Record actual call count and fallback status in the test output.

- [ ] **Step 3: Add user-facing documentation and changepack**

  Document exact-node one-call behavior, automatic legacy fallback, `rootLayout`, safe stats, and the distinction between 268 snapshot parity and the 978-test inventory. Add a focused release note for `devup-mcp`/affected crates according to `.changepacks/config.json`.

- [ ] **Step 4: Run focused and workspace verification from a clean environment**

  Run:

  ```powershell
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo test --workspace --all-features
  cargo build --workspace --all-features
  ```

  Run any repository-specific Bun lint/build commands only if package scripts exist; do not introduce Bun as a Rust runtime dependency.

- [ ] **Step 5: Inspect the final diff for privacy, scope, and generated artifacts**

  Confirm no credential/token/account identifier, full binary envelope, temporary live output, `.env`, or unrelated file is tracked. Confirm every changed snapshot is explained by the intended WQUW stroke/root mode behavior.

- [ ] **Step 6: Install the verified binary for Codex**

  Run:

  ```powershell
  cargo install --path crates/devup-mcp --locked --force
  devup-mcp --version
  ```

  Verify the configured Codex MCP entry still points at the installed executable; note that an already-running Codex session may require a restart to load the new binary.

- [ ] **Step 7: Commit, push, and update PR #1**

  Create a final focused docs/release commit if needed, push `owjs3901/figma-remote-mcp`, and update PR `https://github.com/dev-five-git/devup-mcp/pull/1` with requirements, architecture, test evidence, live call count, fixture audit wording, security/privacy considerations, assumptions, and remaining live-runtime risks.

- [ ] **Step 8: Final evidence report**

  Report changed files, all command outcomes, commit SHAs, PR URL, installed binary path/version, observed Figma counts, privacy/security checks, assumptions, and any remaining risk. Do not claim completion before re-running and reading the final verification outputs.
