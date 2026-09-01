# Devup MCP Asset and Output Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reject mismatched cached asset captures and make every explicit filesystem output confined, staged, and rollback-safe.

**Architecture:** Preserve `AssetSelection` through handoff, artifact capability validation, manifest lookup, and output mapping instead of reducing it to an ID. Route all writes through a startup-configured `OutputPolicy` and one `OutputTransaction` that validates and stages every output before replacing targets.

**Tech Stack:** Rust 2024, rmcp 3.1.4, cap-std, Tokio, Serde, existing Devup error model.

**Spec:** `docs/superpowers/specs/2026-09-01-devup-mcp-hardening-and-delivery-design.md`

## Global Constraints

- Product runtime and default CI remain Cargo-only; no Bun or Node dependency.
- Default allowed write root is the canonical server startup current directory.
- Additional roots are startup-only repeatable `--allow-write-root <path>` arguments.
- A failed validation, strict gate, parse gate, staging operation, or commit performs no successful new output and restores replaced targets on runtime error.
- Figma remains read-only and no credential, design text, URL, token value, or asset ID enters logs/cache stats.

## File map

- `crates/devup-mcp/src/server/artifacts.rs`: internal captured-asset capability and public redacted summary.
- `crates/devup-mcp/src/server/handoff.rs`: preserve exact requested captures across continuation.
- `crates/devup-mcp/src/server/output.rs`: confined path resolution and staged transaction.
- `crates/devup-mcp/src/server/tools.rs`: unchanged public asset request shape.
- `crates/devup-mcp/src/server/mod.rs`: validate capture tuples and commit one output transaction.
- `crates/devup-mcp/src/lib.rs`: construct server with `ServerConfig`.
- `crates/devup-mcp/src/main.rs`: parse `--allow-write-root` and `--version` without accepting unknown arguments.
- `crates/devup-mcp/tests/artifact_cache.rs`: redacted capability and exact reuse tests.
- `crates/devup-mcp/tests/composite_export.rs`: URL/artifact capture tuple integration tests.
- `crates/devup-mcp/tests/output_policy.rs`: traversal, symlink/junction, atomic replacement and rollback tests.
- `crates/devup-mcp/tests/cli.rs`: CLI root parsing tests.
- `README.md`: write confinement and exact asset reuse contract.

---

### Task 1: Preserve exact asset captures

**Files:**
- Modify: `crates/devup-mcp/src/server/artifacts.rs`
- Modify: `crates/devup-mcp/src/server/handoff.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Test: `crates/devup-mcp/tests/artifact_cache.rs`
- Test: `crates/devup-mcp/tests/composite_export.rs`

**Interfaces:**
- Consumes: `devup_mcp_figma::AssetSelection` and `AssetManifestEntry`.
- Produces: `ArtifactCapabilities::supports_assets(&[AssetSelection]) -> Result<(), DevupError>` and `PendingOperation::Export { asset_captures: Vec<AssetSelection>, ... }`.

- [ ] **Step 1: Write failing artifact capability tests**

Add tests that build a design artifact captured with `AssetSelection { asset_id: "hero", format: Png, scale: 1 }`. Assert PNG 1x succeeds and SVG 1x, PNG 2x, and an unknown ID each return `DEVUP_FIGMA_HANDOFF_INVALID`. Serialize `artifact_metadata` and assert it contains `assetCaptureCount: 1` but not `hero`.

- [ ] **Step 2: Run the focused tests and confirm the ID-only implementation fails**

Run: `cargo test -p devup-mcp --test artifact_cache asset_capability -- --nocapture`

Expected: FAIL because `ArtifactCapabilities` has no captured tuple and the serialized summary cannot report the count.

- [ ] **Step 3: Add internal/public capability types**

Implement an internal capability with `asset_captures: Vec<AssetSelection>` sorted by `(asset_id, format, scale)`. Give it a manually serialized/public `ArtifactCapabilitySummary { kind, collection_scope, resource_scope, asset_capture_count }`; do not serialize the capture vector. Implement exact set containment in `supports_assets` and use the existing handoff-invalid error code with only requested/captured counts in details.

- [ ] **Step 4: Preserve captures through PendingOperation**

Replace `asset_ids` with `asset_captures`. Keep the existing duplicate-ID public validation. During manifest validation, require an `Exported` entry whose `asset_id`, `format == Some(request.format)`, and `scale == Some(request.scale)` all match. Treat exported entries missing format/scale as incompatible instead of defaulting them.

- [ ] **Step 5: Run asset tests**

Run: `cargo test -p devup-mcp --test artifact_cache --test composite_export asset -- --nocapture`

Expected: PASS, including zero upstream calls and zero writes for every incompatible artifact reuse case.

- [ ] **Step 6: Commit**

```text
git add crates/devup-mcp/src/server/artifacts.rs crates/devup-mcp/src/server/handoff.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/tests/artifact_cache.rs crates/devup-mcp/tests/composite_export.rs
git commit -m "fix: validate exact figma asset captures"
```

### Task 2: Add confined output policy

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/devup-mcp/Cargo.toml`
- Create: `crates/devup-mcp/src/server/output.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Test: `crates/devup-mcp/tests/output_policy.rs`

**Interfaces:**
- Produces: `OutputPolicy::from_roots(Vec<PathBuf>) -> Result<Self, DevupError>`, `OutputPolicy::resolve(&str) -> Result<OutputTarget, DevupError>`, `OutputTransaction::stage(OutputTarget, &[u8])`, and `OutputTransaction::commit() -> Result<BTreeMap<String, String>, DevupError>`.

- [ ] **Step 1: Write failing path resolution tests**

Use a temporary directory as the only root. Assert `component.tsx` and an absolute child succeed. Assert empty paths, the root directory itself, `../escape.tsx`, sibling absolute paths, duplicate normalized targets, and Windows drive/UNC/alternate-data-stream paths fail. On platforms that permit symlink/junction creation, create a link inside the root pointing outside and assert resolution fails before a file is created.

- [ ] **Step 2: Run and confirm the module is missing**

Run: `cargo test -p devup-mcp --test output_policy resolves_only_inside_preopened_roots -- --nocapture`

Expected: FAIL because `server::output::OutputPolicy` does not exist.

- [ ] **Step 3: Implement capability-relative resolution**

Add `cap-std` to workspace and crate dependencies. Open each canonical root once with ambient authority at server construction, but resolve all request paths relative to the resulting `cap_std::fs::Dir`. Reject lexical parent components and platform prefixes before traversal; use capability metadata/open operations to prevent symlink or junction escape. Return a display path assembled from the trusted canonical root and normalized relative components.

- [ ] **Step 4: Run path tests**

Run: `cargo test -p devup-mcp --test output_policy resolves_only_inside_preopened_roots -- --nocapture`

Expected: PASS with no file outside the temporary root.

- [ ] **Step 5: Commit**

```text
git add Cargo.toml Cargo.lock crates/devup-mcp/Cargo.toml crates/devup-mcp/src/server/output.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/tests/output_policy.rs
git commit -m "feat: confine devup output paths"
```

### Task 3: Stage and rollback multi-output writes

**Files:**
- Modify: `crates/devup-mcp/src/server/output.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Test: `crates/devup-mcp/tests/output_policy.rs`
- Test: `crates/devup-mcp/tests/composite_export.rs`

**Interfaces:**
- Consumes: validated `OutputTarget` values.
- Produces: a transaction that stages named text/binary outputs and publishes paths only after complete commit.

- [ ] **Step 1: Add transaction fault tests**

Create two existing files with known bytes. Inject failures after temp write, after first backup, and after first target replacement through a test-only `OutputFs` fault point. Assert both original files are restored, result contains no output paths, and no `.devup-tmp-*` or `.devup-bak-*` remains after normal error handling. Assert two successful replacements return canonical display paths and exact bytes.

- [ ] **Step 2: Run and confirm direct writes fail rollback assertions**

Run: `cargo test -p devup-mcp --test output_policy transaction -- --nocapture`

Expected: FAIL because the existing implementation writes each file immediately.

- [ ] **Step 3: Implement staged commit**

Create exclusive random temp files in each target directory, write and `sync_all`, move existing targets to exclusive backups, rename all temps, then remove backups. On runtime error, reverse completed replacements and restore backups. Keep crash recovery scoped to internal names created by this process and never delete arbitrary matching user paths.

- [ ] **Step 4: Integrate one transaction into export**

Build all text and decoded binary entries only after capability/quality validation. Resolve every path before staging any bytes. Commit once, then set manifest `outputPath`, remove returned asset base64, and publish `outputPaths`. Delete `write_output` and `write_binary_output`.

- [ ] **Step 5: Run output and export tests**

Run: `cargo test -p devup-mcp --test output_policy --test composite_export -- --nocapture`

Expected: PASS; strict/capability/base64/path failure creates or modifies zero target files.

- [ ] **Step 6: Commit**

```text
git add crates/devup-mcp/src/server/output.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/tests/output_policy.rs crates/devup-mcp/tests/composite_export.rs
git commit -m "feat: commit devup outputs transactionally"
```

### Task 4: Wire startup root configuration and document migration

**Files:**
- Modify: `crates/devup-mcp/src/main.rs`
- Modify: `crates/devup-mcp/src/lib.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/tests/cli.rs`
- Modify: `README.md`

**Interfaces:**
- Produces: `ServerConfig { allowed_write_roots: Vec<PathBuf> }`, `run_stdio_with_config(ServerConfig)`, and CLI parsing for `--allow-write-root`.

- [ ] **Step 1: Add CLI tests**

Assert `--version` still prints only the version; no arguments selects current directory; repeated `--allow-write-root A --allow-write-root B` preserves order; missing values, unknown flags, and non-directory roots fail before stdio starts.

- [ ] **Step 2: Run CLI tests**

Run: `cargo test -p devup-mcp --test cli -- --nocapture`

Expected: FAIL for the new flag.

- [ ] **Step 3: Implement config wiring**

Parse arguments into `ServerConfig`, construct `OutputPolicy` once, store it in `DevupServer`, and retain `DevupServer::default()` for tests by using canonical current directory. Do not allow a tool request to add a root.

- [ ] **Step 4: Update README and run verification**

Document exact asset reuse, current-directory default, the repeated CLI flag, runtime rollback and crash-atomic limitation. Run `cargo fmt --all -- --check`, `cargo clippy -p devup-mcp --all-targets --all-features -- -D warnings`, and `cargo test -p devup-mcp --all-features`.

Expected: all commands exit 0.

- [ ] **Step 5: Commit**

```text
git add crates/devup-mcp/src/main.rs crates/devup-mcp/src/lib.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/tests/cli.rs README.md
git commit -m "docs: secure devup output configuration"
```
