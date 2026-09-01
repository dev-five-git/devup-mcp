# Devup MCP Fidelity, Visual, and Module Boundaries Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent syntactically invalid or semantically untraceable TSX, provide deterministic Rust image comparison, and leave the MCP server split into focused modules.

**Architecture:** Replace diagnostic-code whitelists with typed fidelity impact, parse every generated TSX in Rust, and validate AST/source-map coverage through a projection trace. Add a Figma-independent image comparator crate and a consumer renderer contract, then finish moving orchestration responsibilities out of the router.

**Tech Stack:** Rust 2024, MSRV 1.88, oxc_parser compatible with MSRV, image decoding/encoding crate, Serde, insta, existing source-map types.

**Spec:** `docs/superpowers/specs/2026-09-01-devup-mcp-hardening-and-delivery-design.md`

## Global Constraints

- No JavaScript runtime or browser is a product/default-test dependency.
- Every returned/published/written TSX parses as TypeScript JSX first.
- Unknown codegen warning/error diagnostics cannot silently produce `projection=exact`.
- Browser pixel fidelity is opt-in through a consumer-owned renderer; the server never executes arbitrary commands.
- Existing 268 plugin golden outputs remain reviewed and WQUW-151 typography/token/text assertions remain exact.

## File map

- `crates/devup-mcp-figma/src/snapshot.rs`: additive `FidelityImpact` on Diagnostic.
- `crates/devup-mcp-devup-ui/src/validation.rs`: TSX parse and semantic coverage validation.
- `crates/devup-mcp-devup-ui/src/provenance.rs`: `ProjectionTrace` and `FidelityReport`.
- `crates/devup-mcp-devup-ui/src/codegen/*`: populate trace and typed impacts.
- `crates/devup-mcp/src/server/quality.rs`: aggregate typed impacts.
- `crates/devup-mcp/src/server/projection.rs`: projection orchestration and validation gate.
- `crates/devup-mcp/src/server/validation.rs`: artifact/output strict validation.
- `crates/devup-mcp/src/server/delivery.rs`: delivery only.
- `crates/devup-mcp/src/server/mod.rs`: router/wiring only.
- `crates/devup-mcp-visual/`: pure Rust PNG comparator library/CLI.
- `crates/devup-mcp-devup-ui/tests/validation.rs`: syntax and semantic coverage.
- `crates/devup-mcp-visual/tests/compare.rs`: deterministic image fixtures.
- `docs/visual-renderer-contract.md`: consumer adapter schema and command sequence.

---

### Task 1: Replace diagnostic string matching with fidelity impact

**Files:**
- Modify: `crates/devup-mcp-figma/src/snapshot.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/component.rs`
- Modify: `crates/devup-mcp-devup-ui/src/theme/devup_json.rs`
- Modify: `crates/devup-mcp/src/server/quality.rs`
- Modify: `crates/devup-mcp/tests/composite_export.rs`

**Interfaces:**
- Produces: `FidelityImpact::{None, Approximated, Lossy, Failed}` and `Diagnostic::fidelity_impact()`.

- [ ] **Step 1: Write failing quality matrix tests**

Assert absolute fallback maps to approximated, mask/effect to lossy, projection failure to failed, and info to none. Create an unknown codegen warning and error and assert they map to at least approximated and failed. Assert collector warning remains governed by acquisition, and `includeDiagnostics=false` does not change quality.

- [ ] **Step 2: Run quality tests**

Run: `cargo test -p devup-mcp --test composite_export quality -- --nocapture`

Expected: FAIL because quality matches three code strings.

- [ ] **Step 3: Add typed impact and fallback registry**

Add optional serialized `fidelityImpact`. Set it at every codegen diagnostic producer. Implement a domain-aware legacy registry for deserialized diagnostics without the field; codegen warning defaults to approximated and codegen error to failed, while collector diagnostics do not change projection quality.

- [ ] **Step 4: Aggregate by maximum impact and test**

Replace code string matching in `projection_quality`. Run `cargo test -p devup-mcp --test composite_export quality -- --nocapture` and `cargo test -p devup-mcp-devup-ui --all-features`.

Expected: PASS.

- [ ] **Step 5: Commit**

```text
git add crates/devup-mcp-figma/src/snapshot.rs crates/devup-mcp-devup-ui/src/codegen/component.rs crates/devup-mcp-devup-ui/src/theme/devup_json.rs crates/devup-mcp/src/server/quality.rs crates/devup-mcp/tests/composite_export.rs
git commit -m "fix: classify projection fidelity structurally"
```

### Task 2: Parse every generated TSX in Rust

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/devup-mcp-devup-ui/Cargo.toml`
- Create: `crates/devup-mcp-devup-ui/src/validation.rs`
- Modify: `crates/devup-mcp-devup-ui/src/lib.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/component.rs`
- Create: `crates/devup-mcp-devup-ui/tests/validation.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/compat_fixtures.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/wquw_151_frames.rs`

**Interfaces:**
- Produces: `validate_tsx(source: &str) -> Result<TsxValidation, DevupError>` and a parse gate in every public generator.

- [ ] **Step 1: Select and pin the parser version**

Use `cargo info oxc_parser` and its Cargo metadata to select the newest release whose `rust-version` is at most 1.88. Add the exact compatible version to workspace dependencies so a later incompatible MSRV update is explicit.

- [ ] **Step 2: Write failing syntax tests**

Assert valid nested DevupUI TSX parses. Assert unclosed tags, malformed attribute quotes and invalid expression children return codegen projection failure with byte ranges but without logging the surrounding Korean text.

- [ ] **Step 3: Run the missing validator test**

Run: `cargo test -p devup-mcp-devup-ui --test validation syntax -- --nocapture`

Expected: FAIL because `validate_tsx` does not exist.

- [ ] **Step 4: Implement parser gate**

Parse with TypeScript+JSX source type, collect all parser errors, return a content-redacted error containing byte ranges and parser categories, and call the validator before `CodegenOutput` leaves each component/component-set path.

- [ ] **Step 5: Parse the entire fixture corpus**

Extend compatibility and WQUW tests to call `validate_tsx` for every one of the 268 snapshot cases and all ten WQUW frames. Run `cargo test -p devup-mcp-devup-ui --all-features` and `cargo insta test -p devup-mcp-devup-ui --all-features --check`.

Expected: PASS with no unreviewed snapshots.

- [ ] **Step 6: Commit**

```text
git add Cargo.toml Cargo.lock crates/devup-mcp-devup-ui/Cargo.toml crates/devup-mcp-devup-ui/src/validation.rs crates/devup-mcp-devup-ui/src/lib.rs crates/devup-mcp-devup-ui/src/codegen/component.rs crates/devup-mcp-devup-ui/tests/validation.rs crates/devup-mcp-devup-ui/tests/compat_fixtures.rs crates/devup-mcp-devup-ui/tests/wquw_151_frames.rs
git commit -m "feat: validate generated devup tsx syntax"
```

### Task 3: Produce and enforce semantic fidelity reports

**Files:**
- Modify: `crates/devup-mcp-devup-ui/src/provenance.rs`
- Modify: `crates/devup-mcp-devup-ui/src/validation.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/component.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/text.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/style.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/layout.rs`
- Modify: `crates/devup-mcp/src/server/quality.rs`
- Modify: `crates/devup-mcp/tests/composite_export.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/provenance.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/wquw_151.rs`

**Interfaces:**
- Produces: `ProjectionTrace`, `FidelityCoverage`, `FidelityReport`, and `validate_fidelity(&Snapshot, &CodegenOutput)`.

- [ ] **Step 1: Write failing provenance coverage tests**

Build fixtures for visible emitted nodes, intentional flattening, ignored hidden nodes, styled text segments, token/style bindings, resolved fallback, image placeholder and absolute-layout fallback. Assert every source item receives exactly one trace disposition and coverage percentages/impact counts are deterministic. Remove one trace entry and assert strict validation fails.

- [ ] **Step 2: Run provenance tests**

Run: `cargo test -p devup-mcp-devup-ui --test provenance fidelity -- --nocapture`

Expected: FAIL because source maps do not contain complete disposition/coverage data.

- [ ] **Step 3: Populate ProjectionTrace during generation**

Record node disposition, text segment ranges/order, variable/style/asset provenance and layout mapping kind while fragments are emitted. Keep trace separate from TSX so golden runtime output remains unchanged.

- [ ] **Step 4: Validate AST and trace**

Use parsed TSX spans plus trace/source map to compute node, text, token, typography, asset and layout coverage. Require visible items to be emitted, flattened, or ignored-with-reason exactly once. Return `FidelityReport` in export metadata without source text/token values.

- [ ] **Step 5: Integrate strict gate and WQUW assertions**

Require syntax success, 100% requested coverage, failed=0 and lossy=0 for strict export. Assert WQUW nested `[1. 이름]`, all typography styles/tokens and footer stroke have direct trace entries.

- [ ] **Step 6: Run tests and commit**

Run: `cargo test -p devup-mcp-devup-ui --test provenance --test wquw_151 -- --nocapture` and `cargo test -p devup-mcp --test composite_export strict -- --nocapture`.

Expected: PASS.

```text
git add crates/devup-mcp-devup-ui/src/provenance.rs crates/devup-mcp-devup-ui/src/validation.rs crates/devup-mcp-devup-ui/src/codegen crates/devup-mcp/src/server/quality.rs crates/devup-mcp/tests/composite_export.rs crates/devup-mcp-devup-ui/tests/provenance.rs crates/devup-mcp-devup-ui/tests/wquw_151.rs
git commit -m "feat: report devup projection fidelity"
```

### Task 4: Add pure Rust visual comparator and adapter contract

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/devup-mcp-visual/Cargo.toml`
- Create: `crates/devup-mcp-visual/src/lib.rs`
- Create: `crates/devup-mcp-visual/src/main.rs`
- Create: `crates/devup-mcp-visual/tests/compare.rs`
- Create: `crates/devup-mcp-visual/tests/fixtures/exact-reference.png`
- Create: `crates/devup-mcp-visual/tests/fixtures/changed-actual.png`
- Create: `docs/visual-renderer-contract.md`
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/tests/composite_export.rs`

**Interfaces:**
- Produces: `compare_png(reference, actual, CompareOptions) -> VisualReport`, CLI `devup-mcp-visual compare`, and Figma `referencePng` output through existing screenshot acquisition.

- [ ] **Step 1: Write failing comparator tests**

Generate tiny deterministic PNG fixtures in the test setup using the image crate. Assert exact images report zero changed pixels, a known changed rectangle reports exact count/ratio and a diff PNG, dimension mismatch reports invalid dimensions, and anti-aliased one-channel differences under configured tolerance are ignored.

- [ ] **Step 2: Run the missing-crate test**

Run: `cargo test -p devup-mcp-visual --test compare -- --nocapture`

Expected: FAIL because the crate is not a workspace member.

- [ ] **Step 3: Implement comparator library and CLI**

Decode to normalized RGBA8, compare dimensions, calculate per-channel absolute delta and changed-pixel ratio, render opaque diff pixels, serialize a content-free `VisualReport`, and exit nonzero when dimensions differ or ratio exceeds the requested threshold. Default threshold is 0.5% and is always included in the report.

- [ ] **Step 4: Expose reference screenshot output**

Add `referencePng` as an explicit export output. Acquire it through the existing read-only screenshot request only when selected, deliver it via Resource/output transaction, and exclude bytes from cache keys/logs. It does not make server strict depend on pixel comparison.

- [ ] **Step 5: Document consumer renderer contract**

Specify JSON fields for TSX/resource manifest, viewport, theme, asset directory, output PNG, renderer/version, DevupUI version and font manifest. Provide exact consumer sequence: export resources, build/type-check in consumer, render at viewport, write PNG, invoke `devup-mcp-visual compare`, and reject `environment-invalid` metadata before treating metrics as pass.

- [ ] **Step 6: Run tests and commit**

Run: `cargo test -p devup-mcp-visual --all-features`, `cargo test -p devup-mcp --test composite_export reference_png -- --nocapture`, and `cargo build -p devup-mcp-visual --release`.

Expected: PASS without Bun/Node.

```text
git add Cargo.toml Cargo.lock crates/devup-mcp-visual docs/visual-renderer-contract.md crates/devup-mcp/src/server/tools.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/tests/composite_export.rs
git commit -m "feat: compare figma visual references in rust"
```

### Task 5: Complete server module boundaries and final verification

**Files:**
- Create: `crates/devup-mcp/src/server/projection.rs`
- Create: `crates/devup-mcp/src/server/validation.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `README.md`
- Modify: `.changepacks/changepack_log_figma_remote_mcp.json`

**Interfaces:**
- `mod.rs` retains tool router, `ServerHandler`, shared state construction and delegates all behavior.

- [ ] **Step 1: Add module boundary checks**

Move projection and validation tests with their implementation modules. Add a source-level test that `server/mod.rs` contains no direct `std::fs` call, no TSX/theme generator call and no resource URI parser, while public tool names and schemas remain unchanged.

- [ ] **Step 2: Move orchestration without behavior changes**

Move TSX/theme/source-map/asset-manifest generation into `projection.rs`; move artifact capability, quality, syntax, fidelity and strict decisions into `validation.rs`; leave delivery/output/acquisition in their named modules. Keep `mod.rs` as routing/wiring and retain existing error JSON shapes.

- [ ] **Step 3: Update docs and changepack**

Document fidelity fields, syntax gate, reference screenshot, comparator CLI, adapter limitation and server module ownership. Extend the existing minor changepack summary without adding a second conflicting version entry.

- [ ] **Step 4: Run complete verification**

Run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo insta test --workspace --all-features --check
cargo build --workspace --release
```

Expected: all commands exit 0, 268/268 plugin goldens and all WQUW-151 fixtures pass, and no pending snapshots exist.

- [ ] **Step 5: Commit**

```text
git add crates/devup-mcp/src/server README.md .changepacks/changepack_log_figma_remote_mcp.json
git commit -m "refactor: separate devup mcp server boundaries"
```

- [ ] **Step 6: Push, update PR and reinstall Codex MCP**

Push `owjs3901/figma-remote-mcp`, update PR #1 with Korean test/security/limitation details, build the release binary, update the existing Codex MCP command to the new binary path without embedding secrets, restart/reconnect it, and run `tools/list` plus a read-only fixture smoke. Record final commit SHA, PR URL and installed binary version.
