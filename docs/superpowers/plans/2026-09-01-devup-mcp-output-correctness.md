# Devup MCP Output Correctness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make DevupUI TSX, theme output, completion status, strict mode, and artifact reuse share one truthful correctness contract.

**Architecture:** Keep the existing three-crate workspace. Canonical token naming and attribute serialization stay inside `devup-mcp-devup-ui`; output-quality orchestration stays in a focused server module; artifact capabilities travel with the existing in-memory cache entry.

**Tech Stack:** Rust 1.88, serde/schemars, rmcp 3.1, Tokio, cargo test, insta fixtures

**Spec:** `docs/superpowers/specs/2026-09-01-devup-mcp-output-correctness-design.md`

## Global Constraints

- The runtime remains pure Rust; no Bun or Node runtime dependency is introduced.
- Figma access remains read-only and no design content is added to logs or metadata.
- Existing public fields remain compatible; new quality and capability fields are additive.
- Each production behavior is preceded by a failing test and verified independently.
- Section multi-root acquisition, MCP resources, safe filesystem roots, and visual diffing are not part of this increment.

---

### Task 1: Canonical Token Identity and JSX Attribute Escaping

**Files:**
- Modify: `crates/devup-mcp-devup-ui/src/theme/tokens.rs`
- Modify: `crates/devup-mcp-devup-ui/src/theme/mod.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/component.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/text.rs`
- Test: `crates/devup-mcp-devup-ui/tests/codegen.rs`
- Test: `crates/devup-mcp/tests/downstream_integration.rs`

**Interfaces:**
- Consumes: Figma variable `name` and optional `codeSyntax.WEB` from `UpstreamResult`.
- Produces: crate-visible `normalize_token`, `variable_token`, and `render_static_attribute(name, value) -> String` used by all static TSX prop emission.

- [ ] **Step 1: Write a failing token integration test**

Extend `converts_a_figma_link_to_structured_devup_ui` to assert that a bound variable named `Color/Primary` with `codeSyntax.WEB = "primary"` emits `bg="$primary"` and never `$colorPrimary`.

- [ ] **Step 2: Run the token test and verify RED**

Run: `cargo test -p devup-mcp --test downstream_integration converts_a_figma_link_to_structured_devup_ui -- --exact`

Expected: FAIL because current codegen emits `$colorPrimary`.

- [ ] **Step 3: Implement shared token resolution**

Expose the existing theme normalization functions crate-wide and change `named_tokens` so variables call `variable_token(name, codeSyntax.WEB)` while styles call `normalize_token(name)`.

- [ ] **Step 4: Run the token test and verify GREEN**

Run the exact command from Step 2; expected PASS.

- [ ] **Step 5: Write a failing JSX attribute escaping test**

Add `escapes_static_jsx_attribute_values` using a TEXT node whose font family is `A&B\"Font<UI>` and assert `fontFamily="A&amp;B&quot;Font&lt;UI&gt;"`.

- [ ] **Step 6: Run the escaping test and verify RED**

Run: `cargo test -p devup-mcp-devup-ui --test codegen escapes_static_jsx_attribute_values -- --exact`

Expected: FAIL because current component and styled-segment renderers interpolate raw values.

- [ ] **Step 7: Implement the central static attribute renderer**

Add one renderer in `component.rs`, use it from `render_props`, and call it from the nested text segment renderer. Preserve attribute ordering and quoted output.

- [ ] **Step 8: Run focused and crate tests**

Run: `cargo test -p devup-mcp-devup-ui --test codegen && cargo test -p devup-mcp --test downstream_integration`

Expected: PASS with no warnings.

- [ ] **Step 9: Commit Task 1**

Commit message: `fix: unify figma tokens and escape tsx props`

### Task 2: Output-Aware Quality and Strict Semantics

**Files:**
- Create: `crates/devup-mcp/src/server/quality.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Test: `crates/devup-mcp/tests/composite_export.rs`
- Test: `crates/devup-mcp/tests/figma_explore.rs`
- Test: `crates/devup-mcp/tests/downstream_integration.rs`

**Interfaces:**
- Consumes: `PayloadCompletenessReport`, codegen diagnostics, theme conflicts/unresolved values, requested asset results.
- Produces: serializable `OutputQuality`, per-axis enums, `status()`, and `strict_violation()`.

- [ ] **Step 1: Write failing quality tests**

Add assertions that a simple UI result contains exact/complete quality axes, an intentionally shallow Explore result has top-level `status = "complete"` and `acquisition = "expected-projection"`, and strict TSX export rejects a mask/effect fallback even with a complete snapshot.

- [ ] **Step 2: Run focused tests and verify RED**

Run: `cargo test -p devup-mcp --test downstream_integration --test figma_explore --test composite_export`

Expected: FAIL because `quality` does not exist, Explore is partial, and strict ignores projection diagnostics.

- [ ] **Step 3: Implement quality types and classification**

Create `quality.rs` with operation-relative acquisition classification, diagnostic-code projection classification, theme and asset classification, top-level status derivation, and strict validation.

- [ ] **Step 4: Integrate quality into every completed operation**

Compute diagnostics regardless of `includeDiagnostics`, attach `quality`, derive `status` after requested outputs are projected, and move strict validation after projection. Preserve `selection_required` and `needs_figma` workflow results.

- [ ] **Step 5: Run focused tests and verify GREEN**

Run the exact command from Step 2; expected PASS.

- [ ] **Step 6: Commit Task 2**

Commit message: `fix: report output-aware figma quality`

### Task 3: Artifact Capability Validation

**Files:**
- Modify: `crates/devup-mcp/src/server/artifacts.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Test: `crates/devup-mcp/tests/artifact_cache.rs`
- Test: `crates/devup-mcp/tests/composite_export.rs`

**Interfaces:**
- Consumes: `CollectionRequest`, requested outputs, requested theme scope.
- Produces: `ArtifactCapabilities { kind, collection_scope, resource_scope }` on every `ArtifactLookup` and `validate_artifact_projection(...)` before reuse.

- [ ] **Step 1: Write failing cache capability test**

Assert that inserted design, theme-only, search, and explore requests return the expected non-sensitive capability metadata.

- [ ] **Step 2: Run the cache test and verify RED**

Run: `cargo test -p devup-mcp --test artifact_cache artifact_lookup_preserves_capture_capabilities -- --exact`

Expected: compilation failure because artifact capabilities do not exist.

- [ ] **Step 3: Implement immutable artifact capabilities**

Derive capabilities from `ArtifactRequestKey`, store them in cache entries, propagate them through hits and singleflight results, and expose them in artifact metadata.

- [ ] **Step 4: Write failing incompatible reuse tests**

Acquire a node design artifact and assert that reusing it for file-scoped `devupJson` fails; assert that compatible embedded TSX reuse remains zero-call.

- [ ] **Step 5: Run reuse tests and verify RED**

Run: `cargo test -p devup-mcp --test composite_export`

Expected: the incompatible file-theme request currently succeeds or projects incomplete data.

- [ ] **Step 6: Implement projection compatibility validation**

Validate capture kind, collection scope, and resource scope before `complete_operation`. Return `DEVUP_FIGMA_HANDOFF_INVALID` with capability enum details only.

- [ ] **Step 7: Run artifact tests and verify GREEN**

Run: `cargo test -p devup-mcp --test artifact_cache --test composite_export`

Expected: PASS and compatible reuse still performs zero upstream calls.

- [ ] **Step 8: Commit Task 3**

Commit message: `fix: validate cached figma artifact capabilities`

### Task 4: Documentation and Full Verification

**Files:**
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-09-01-devup-mcp-output-correctness-design.md` only if implementation names differ while preserving the approved behavior.

**Interfaces:**
- Consumes: finalized public quality and artifact capability JSON fields.
- Produces: user-facing contract examples and verified release binary.

- [ ] **Step 1: Document the additive response contract**

Add concise README examples for `quality`, strict rejection, canonical variable token naming, and artifact reuse restrictions without publishing real Figma content.

- [ ] **Step 2: Run formatting and lint verification**

Run: `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --all-features -- -D warnings`.

Expected: both exit 0 with no warnings.

- [ ] **Step 3: Run complete test and snapshot verification**

Run: `cargo test --workspace --all-features` and `cargo insta test --workspace --all-features --check`.

Expected: all default tests pass, the two stdin live tests remain ignored, and no unreviewed snapshot changes exist.

- [ ] **Step 4: Build the release binary**

Run: `cargo build --workspace --release`.

Expected: exit 0.

- [ ] **Step 5: Commit Task 4**

Commit message: `docs: describe figma output quality guarantees`

- [ ] **Step 6: Review, push, and update the existing pull request**

Inspect the focused diff and commit list, push `owjs3901/figma-remote-mcp`, and update PR #1 with the new correctness guarantees and exact verification commands.
