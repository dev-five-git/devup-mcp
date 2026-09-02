# JSON Fixture and DevupUI Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the pinned JavaScript plugin test corpus to self-contained production-shaped JSON inputs and make the Rust DevupUI/devup.json converter pass the corresponding cargo-insta snapshots.

**Architecture:** This plan starts only after `2026-08-31-figma-collection-live-contract.md` has completed and `CollectedPayload` has passed the live JSON contract gate. Fixtures deserialize directly into that real Rust payload type; a manifest and ledger prove one-to-one accounting for the pinned 54 files, 978 tests and 268 existing snapshots. Codegen parity is then implemented in focused layers using the JSON cases as end-to-end tests.

**Tech Stack:** Rust 1.88, edition 2024, serde/serde_json, insta/cargo-insta 1.48.0 with `json` and `glob`, DevupUI TSX, Cargo-only build/test.

**Spec:** `docs/superpowers/specs/2026-08-31-figma-host-fallback-parity-design.md`

## Global Constraints

- Prerequisite: the collection/live-contract plan is complete and its tests pass.
- Do not invent fixture node fields. Every fixture payload must deserialize as the finalized `devup_mcp_figma::CollectedPayload`.
- `devup-mcp` does not run Bun/Node and contains no JavaScript fixture generator.
- Pinned source is `dev-five-git/devup-figma-plugin@243db650f1d635ab5385546a2a297eae4ea93515` until an explicitly reviewed update changes it.
- Baseline evidence is 54 test files, 978 passing tests, 0 failures, 268 snapshots and 1,974 assertions.
- Existing upstream snapshot strings are normalized only from CRLF to LF; formatting is otherwise byte-identical.
- Actual user Figma payloads are not fixtures. Fixtures use the pinned plugin's synthetic mocks normalized into the real payload shape.
- JSON, manifest, ledger and snapshot changes are reviewed together; tests never auto-accept snapshots.
- Each codegen layer follows red-green-refactor and ends in a focused commit.

## File Structure

### Create

- `fixtures/devup-figma-plugin/manifest.json` — source provenance, schema version, counts and checksums.
- `fixtures/devup-figma-plugin/ledger.json` — all upstream test IDs and their Rust mapping.
- `fixtures/devup-figma-plugin/cases/{codegen,responsive,devup-json,snapshot,errors}/*.json` — one self-contained JSON case per conversion behavior.
- `fixtures/devup-figma-plugin/snapshots/{codegen,responsive,devup-json,snapshot,errors}/*.snap` — one insta result per `rust_snapshot` case.
- `crates/devup-mcp-devup-ui/tests/support/mod.rs` — fixture discovery, validation and snapshot settings.
- `crates/devup-mcp-devup-ui/tests/compat_fixtures.rs` — JSON-to-output dispatcher.
- `crates/devup-mcp-devup-ui/tests/compat_manifest.rs` — manifest/ledger/orphan/checksum gate.
- `crates/devup-mcp-devup-ui/src/codegen/node_tree.rs` — production-shaped payload to codegen IR.
- `crates/devup-mcp-devup-ui/src/codegen/render.rs` — deterministic JSX/import rendering.
- `crates/devup-mcp-devup-ui/src/codegen/props/mod.rs` — property aggregation.
- `crates/devup-mcp-devup-ui/src/codegen/props/layout.rs` — layout, constraints, sizing, padding and gap.
- `crates/devup-mcp-devup-ui/src/codegen/props/visual.rs` — fills, gradients, borders, radii, blend and effects.
- `crates/devup-mcp-devup-ui/src/codegen/props/typography.rs` — text segments and text CSS.
- `crates/devup-mcp-devup-ui/src/codegen/props/variables.rs` — bound-variable resolution.
- `crates/devup-mcp-devup-ui/src/codegen/components.rs` — components, instances, variants and slots.
- `crates/devup-mcp-devup-ui/src/codegen/responsive.rs` — breakpoint grouping/merging.
- `crates/devup-mcp-devup-ui/src/codegen/assets.rs` — SVG/PNG/image/mask decisions.
- `crates/devup-mcp-devup-ui/src/theme/aliases.rs` — variable alias graph and cycle handling.
- `crates/devup-mcp-devup-ui/src/theme/styles.rs` — typography and shadow style projection.

### Modify

- `Cargo.toml` — add workspace `insta` only as a test dependency consumer.
- `crates/devup-mcp-devup-ui/Cargo.toml` — `insta` dev dependency with `glob,json`.
- `crates/devup-mcp-devup-ui/src/codegen/mod.rs` — export the decomposed codegen pipeline.
- `crates/devup-mcp-devup-ui/src/codegen/component.rs` — delegate to NodeTree/component/render units.
- `crates/devup-mcp-devup-ui/src/codegen/layout.rs` — move or replace minimal rules.
- `crates/devup-mcp-devup-ui/src/codegen/style.rs` — move or replace minimal rules.
- `crates/devup-mcp-devup-ui/src/codegen/text.rs` — move or replace minimal rules.
- `crates/devup-mcp-devup-ui/src/theme/devup_json.rs` — mode/theme/treeshaking output.
- `crates/devup-mcp-devup-ui/src/theme/tokens.rs` — stable token naming and aliases.
- `crates/devup-mcp-devup-ui/src/theme/mod.rs` — export new theme units.
- `.github/workflows/ci.yml` — install `cargo-insta` and run snapshot check.
- `README.md` — fixture authoring and snapshot review.
- `.changepacks/*.md` — parity change record.

---

### Task 1: Freeze Fixture Types from the Live Payload Contract

**Files:**
- Create: `crates/devup-mcp-devup-ui/tests/support/mod.rs`
- Create: `crates/devup-mcp-devup-ui/tests/compat_fixtures.rs`
- Create: `fixtures/devup-figma-plugin/cases/codegen/live-shape-smoke.json`
- Modify: `Cargo.toml`
- Modify: `crates/devup-mcp-devup-ui/Cargo.toml`

**Interfaces:**
- Consumes: finalized `devup_mcp_figma::CollectedPayload` from the prerequisite plan.
- Produces: test-only `FixtureCase { schema_version, id, operation, source, request, payload: CollectedPayload }`.
- Produces: `FixtureOperation::{Tsx, ResponsiveTsx, DevupJson, Snapshot, Error}`.
- Produces: `load_case(path) -> Result<FixtureCase, FixtureError>` and `run_case(case) -> Result<serde_json::Value, DevupError>`.

- [ ] **Step 1: Add insta and write a failing one-case loader test**

```rust
#[test]
fn production_shaped_json_runs_through_the_converter() {
    let case = support::load_case(fixture("codegen/live-shape-smoke.json")).unwrap();
    assert_eq!(case.schema_version, 1);
    let output = support::run_case(&case).unwrap();
    insta::assert_json_snapshot!(case.id, output);
}
```

The JSON must be generated by serializing a redacted synthetic `CollectedPayload` accepted by the live contract test, not handwritten against the design document.

- [ ] **Step 2: Run and confirm the missing loader fails**

Run: `cargo test -p devup-mcp-devup-ui --test compat_fixtures`

Expected: compile failure for the support module/types.

- [ ] **Step 3: Implement the envelope and dispatcher**

Use `#[serde(deny_unknown_fields)]` on the envelope and request, but preserve unknown node fields through `CollectedPayload`. Validate schema version 1, non-empty unique IDs, source commit format, root existence and operation-required data before invoking production functions.

```rust
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FixtureCase {
    schema_version: u32,
    id: String,
    operation: FixtureOperation,
    source: FixtureSource,
    request: FixtureRequest,
    payload: CollectedPayload,
}
```

- [ ] **Step 4: Run the test and inspect the first insta snapshot**

Run: `cargo insta test -p devup-mcp-devup-ui --test compat_fixtures`

Run: `cargo insta review`

Expected: exactly one reviewed smoke snapshot whose output is generated by the production converter.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/devup-mcp-devup-ui/Cargo.toml crates/devup-mcp-devup-ui/tests fixtures/devup-figma-plugin/cases/codegen/live-shape-smoke.json fixtures/devup-figma-plugin/snapshots
git commit -m "test: add production-shaped json fixtures"
```

### Task 2: Manifest, Ledger and One-to-One Corpus Validator

**Files:**
- Create: `fixtures/devup-figma-plugin/manifest.json`
- Create: `fixtures/devup-figma-plugin/ledger.json`
- Create: `crates/devup-mcp-devup-ui/tests/compat_manifest.rs`
- Modify: `crates/devup-mcp-devup-ui/tests/support/mod.rs`

**Interfaces:**
- Produces: `FixtureManifest { repository, commit, schema_version, baseline, cases, files }`.
- Produces: `LedgerEntry { test_id, classification, fixture_id, rust_test, rationale }`.
- Produces: `validate_corpus(root) -> Result<CorpusSummary, Vec<CorpusViolation>>`.

- [ ] **Step 1: Write failing validator tests with deliberately broken temporary corpora**

Test duplicate case ID, orphan JSON, orphan `.snap`, missing ledger entry, nonexistent fixture reference, missing rationale for `out_of_scope_write`, wrong checksum, wrong baseline counts and unsupported schema version.

- [ ] **Step 2: Run and confirm validator interfaces are missing**

Run: `cargo test -p devup-mcp-devup-ui --test compat_manifest`

Expected: compile failure.

- [ ] **Step 3: Implement deterministic recursive discovery and checksums**

Use `std::fs::read_dir` recursively and sort normalized `/`-separated relative paths. Hash exact committed bytes with SHA-256. Require ledger classifications to be one of `rust_snapshot`, `rust_assertion`, `contract`, `out_of_scope_write`, `upstream_runtime_only` and apply the spec's required fields per class.

```rust
fn validate_corpus(root: &Path) -> Result<CorpusSummary, Vec<CorpusViolation>> {
    let discovered = discover_sorted_files(root)?;
    let manifest = load_manifest(root.join("manifest.json"))?;
    let ledger = load_ledger(root.join("ledger.json"))?;
    validate_sets_and_checksums(&discovered, &manifest, &ledger)
}
```

- [ ] **Step 4: Seed the pinned baseline inventory**

Populate manifest evidence with source SHA, 54 test files, 978 passed, 0 failed, 268 snapshots and 1,974 assertions. Populate ledger entries from all pinned upstream test IDs before adding classifications. At this step unclassified entries must intentionally fail with a count of 978, proving the completeness gate is active.

Run the already-Bun-based source project only for this one-time inventory; this does not add a runtime or CI dependency to `devup-mcp`.

```powershell
$pluginRepo = 'C:\Users\owjs3\Desktop\projects\devup-figma-plugin'
$junitPath = Join-Path ([System.IO.Path]::GetTempPath()) 'devup-figma-plugin-junit.xml'
Push-Location -LiteralPath $pluginRepo
bun run test
bun test --reporter=junit --reporter-outfile=$junitPath
Pop-Location
[xml]$junit = Get-Content -Raw -LiteralPath $junitPath
$testIds = $junit.SelectNodes('//testcase') | ForEach-Object { "$($_.classname) > $($_.name)" }
$testIds.Count
Remove-Item -LiteralPath $junitPath
```

Expected: the source suite remains 978 pass / 0 fail and `$testIds.Count` is 978. Use the expanded IDs, including parameterized names, as the initial ledger set.

- [ ] **Step 5: Classify only the smoke case and run the validator**

Run: `cargo test -p devup-mcp-devup-ui --test compat_manifest`

Expected: failure reports the exact remaining unclassified count and no duplicate IDs.

- [ ] **Step 6: Commit the validator and baseline inventory**

```bash
git add fixtures/devup-figma-plugin/manifest.json fixtures/devup-figma-plugin/ledger.json crates/devup-mcp-devup-ui/tests/support/mod.rs crates/devup-mcp-devup-ui/tests/compat_manifest.rs
git commit -m "test: track the complete plugin test corpus"
```

### Task 3: Migrate All 268 Existing Golden Snapshots

**Files:**
- Create: `fixtures/devup-figma-plugin/cases/codegen/*.json`
- Create: `fixtures/devup-figma-plugin/cases/responsive/*.json`
- Create: `fixtures/devup-figma-plugin/snapshots/codegen/*.snap`
- Create: `fixtures/devup-figma-plugin/snapshots/responsive/*.snap`
- Modify: `fixtures/devup-figma-plugin/manifest.json`
- Modify: `fixtures/devup-figma-plugin/ledger.json`

**Interfaces:**
- Consumes: pinned upstream snapshots: codegen 256, viewport 2, render 3, responsive 3 and root codegen 4.
- Produces: 268 JSON inputs and 268 ID-matched insta golden snapshots.

- [ ] **Step 1: Migrate the five upstream snapshot files in fixed batches**

For every snapshot export, locate the originating test/parameter case, normalize its node graph through the finalized `CollectedPayload` serde type, and create one JSON. Use these required batch totals: `codegen.test.ts.snap=256`, `codegen-viewport.test.ts.snap=2`, `render.test.ts.snap=3`, `ResponsiveCodegen.test.ts.snap=3`, `code.test.ts.snap=4`.

- [ ] **Step 2: Preserve expected strings exactly**

Convert each upstream snapshot entry into insta metadata plus body. Change only CRLF to LF. Retain whitespace, quote style, import order, JSX formatting and comments.

- [ ] **Step 3: Run corpus validation before codegen parity**

Run: `cargo test -p devup-mcp-devup-ui --test compat_manifest`

Expected: no orphan/checksum errors for the 268 cases; remaining ledger entries may still be unclassified.

- [ ] **Step 4: Run all migrated cases and keep mismatches failing**

Run: `cargo insta test -p devup-mcp-devup-ui --test compat_fixtures`

Expected: failures enumerate unsupported Rust codegen behavior. Do not accept Rust-produced `.snap.new` over imported upstream golden files.

- [ ] **Step 5: Commit inputs and goldens without claiming parity**

```bash
git add fixtures/devup-figma-plugin
git commit -m "test: import plugin codegen json goldens"
```

### Task 4: Implement Core NodeTree, Props and Rendering Parity

**Files:**
- Create: `crates/devup-mcp-devup-ui/src/codegen/node_tree.rs`
- Create: `crates/devup-mcp-devup-ui/src/codegen/render.rs`
- Create: `crates/devup-mcp-devup-ui/src/codegen/props/mod.rs`
- Create: `crates/devup-mcp-devup-ui/src/codegen/props/layout.rs`
- Create: `crates/devup-mcp-devup-ui/src/codegen/props/visual.rs`
- Create: `crates/devup-mcp-devup-ui/src/codegen/props/typography.rs`
- Create: `crates/devup-mcp-devup-ui/src/codegen/props/variables.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/mod.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/component.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/{layout,style,text}.rs`

**Interfaces:**
- Produces: `NodeTree { component, props, children, node_type, node_name, is_component, is_slot, condition, text_children, leading_comment }`.
- Produces: `build_tree(payload, root_id, options) -> Result<NodeTree, DevupError>`.
- Produces: `render_tree(tree, depth) -> RenderedNode { jsx, imports, used_tokens, diagnostics }`.

- [ ] **Step 1: Select the smallest failing core cases**

Run only JSON IDs for simple Frame, clipped Frame, Auto Layout row/column, free layout, fixed/fill/hug sizing, Text, solid/gradient fill, border/radius, shadow, transform and bound variable. Confirm each fails against the imported upstream golden.

- [ ] **Step 2: Implement NodeTree construction and deterministic renderer**

Use `BTreeMap` for deterministic prop/import ordering only where upstream sorts; preserve source child order. Render Box/Flex/Grid/Text/Image, self-closing versus child JSX, multiline indentation, comments, token `$name` values and diagnostic locations exactly as the corresponding goldens specify.

```rust
pub struct NodeTree {
    pub component: String,
    pub props: Vec<(String, PropValue)>,
    pub children: Vec<NodeTree>,
    pub node_type: String,
    pub node_name: String,
    pub condition: Option<String>,
    pub text_children: Vec<String>,
}
```

- [ ] **Step 3: Implement property modules one behavior at a time**

For each selected case: run the single snapshot, implement the minimal typed-field rule, rerun, then run the entire core category. Include auto layout/wrap/alignment/gap/padding, constraints/position/sizing/min-max, fills/gradients/image, borders/radii, opacity/blend/effects, transform/overflow, typography/styled segments/ellipsis/text stroke, and bound variables.

- [ ] **Step 4: Run core compatibility and existing unit tests**

Run: `cargo insta test -p devup-mcp-devup-ui --test compat_fixtures -- codegen`

Run: `cargo test -p devup-mcp-devup-ui --test codegen`

Expected: all core-category goldens pass without snapshot updates.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp-devup-ui/src/codegen crates/devup-mcp-devup-ui/tests fixtures/devup-figma-plugin
git commit -m "feat: port core devup figma codegen rules"
```

### Task 5: Implement Components, Responsive Behavior and Assets

**Files:**
- Create: `crates/devup-mcp-devup-ui/src/codegen/components.rs`
- Create: `crates/devup-mcp-devup-ui/src/codegen/responsive.rs`
- Create: `crates/devup-mcp-devup-ui/src/codegen/assets.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/node_tree.rs`
- Modify: `crates/devup-mcp-devup-ui/src/codegen/render.rs`

**Interfaces:**
- Produces: component definitions/usages, variant metadata and import metadata.
- Produces: responsive breakpoint arrays and duplicate-collapse rules.
- Produces: asset references `{kind: Svg|Png, path, mask_color, dimensions}`.

- [ ] **Step 1: Run the component/viewport/asset golden subsets and record exact failures**

Include component, component set, instance reference/inline, Pure Code, boolean/text/instance-swap/native slots, selectors, reactions/keyframes, viewport variants, non-viewport variants, SVG/PNG, masks, image scale modes and wrapper collapse.

- [ ] **Step 2: Implement component and slot semantics**

Resolve `mainComponentId`, default variant IDs and component property definitions from the payload. Implement single native slot as `children`, multiple slots as named `React.ReactNode`, boolean conditions, text bindings and instance-swap placeholders.

```rust
fn build_instance(
    payload: &CollectedPayload,
    node: &RawNode,
    options: &CodegenOptions,
) -> Result<NodeTree, DevupError>;
```

- [ ] **Step 3: Implement responsive merging and asset decisions**

Match upstream breakpoint boundaries and collapse duplicate responsive values. Preserve token replacement for box/text shadow. Classify assets only using captured node fields; produce explicit diagnostics when required binary/vector data is unavailable.

- [ ] **Step 4: Run all 268 imported goldens**

Run: `cargo insta test -p devup-mcp-devup-ui --test compat_fixtures`

Expected: all 268 imported upstream snapshots pass without accepting Rust-generated replacements.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp-devup-ui/src/codegen crates/devup-mcp-devup-ui/tests fixtures/devup-figma-plugin
git commit -m "feat: port component responsive and asset codegen"
```

### Task 6: Implement devup.json Variable and Style Parity

**Files:**
- Create: `crates/devup-mcp-devup-ui/src/theme/aliases.rs`
- Create: `crates/devup-mcp-devup-ui/src/theme/styles.rs`
- Create: `fixtures/devup-figma-plugin/cases/devup-json/*.json`
- Create: `fixtures/devup-figma-plugin/snapshots/devup-json/*.snap`
- Modify: `crates/devup-mcp-devup-ui/src/theme/devup_json.rs`
- Modify: `crates/devup-mcp-devup-ui/src/theme/tokens.rs`
- Modify: `crates/devup-mcp-devup-ui/src/theme/mod.rs`
- Modify: `fixtures/devup-figma-plugin/manifest.json`
- Modify: `fixtures/devup-figma-plugin/ledger.json`

**Interfaces:**
- Produces: cycle-safe alias resolution per collection mode.
- Produces: colors, typography, length and shadow theme projections with breakpoint mapping and treeshaking.

- [ ] **Step 1: Convert all export-devup read cases to JSON before changing theme code**

Cover colors, color aliases, modes/themes, FLOAT lengths, breakpoint maps, typography, effect/text shadows, local/library bound variables, treeshaking, missing colors, empty buckets, theme replication and conflict diagnostics. Write current JavaScript expected objects as initial insta JSON snapshots.

- [ ] **Step 2: Run and confirm current theme projection mismatches**

Run: `cargo insta test -p devup-mcp-devup-ui --test compat_fixtures -- devup-json`

Expected: failures for aliases, scope, styles, treeshaking and replication.

- [ ] **Step 3: Implement alias/style/mode projection**

Detect alias cycles by variable ID and mode, prefer WEB `codeSyntax`, use deterministic fallback token normalization, preserve original names in diagnostics, filter node/page scope by actual bindings, and never report `full-local-plus-used-remote` unless completeness flags justify it.

```rust
fn resolve_alias(
    variables: &BTreeMap<String, VariableDefinition>,
    variable_id: &str,
    mode_id: &str,
    visiting: &mut BTreeSet<(String, String)>,
) -> Result<TokenValue, DevupError>;
```

- [ ] **Step 4: Run theme compatibility and unit tests**

Run: `cargo insta test -p devup-mcp-devup-ui --test compat_fixtures -- devup-json`

Run: `cargo test -p devup-mcp-devup-ui --test theme`

Expected: all devup-json cases pass without snapshot replacement.

- [ ] **Step 5: Commit**

```bash
git add crates/devup-mcp-devup-ui/src/theme crates/devup-mcp-devup-ui/tests fixtures/devup-figma-plugin
git commit -m "feat: port devup json export parity"
```

### Task 7: Account for the Remaining 978-Test Ledger

**Files:**
- Create: additional `fixtures/devup-figma-plugin/cases/{snapshot,errors}/*.json`
- Create: additional `fixtures/devup-figma-plugin/snapshots/{snapshot,errors}/*.snap`
- Modify: `fixtures/devup-figma-plugin/manifest.json`
- Modify: `fixtures/devup-figma-plugin/ledger.json`
- Modify: Rust unit/contract tests referenced by ledger entries.

**Interfaces:**
- Consumes: every pinned upstream test ID.
- Produces: zero unclassified, duplicate, orphan or unjustified ledger entries.

- [ ] **Step 1: Process remaining tests by source area**

Use the pinned 54-file inventory and finish in this order: `src/codegen` 26 files, `src/utils` 13 files, `src/commands` 12 files, root `src/__tests__` 3 files. Conversion/helper inputs become JSON snapshots or Rust assertions. MCP/file boundary behavior maps to contract tests.

- [ ] **Step 2: Classify intentional non-converter behavior narrowly**

Only Figma mutation/import tests may use `out_of_scope_write`; link each to the read-only allowlist test. Only plugin bootstrap, iframe/download transport or JavaScript module lifecycle may use `upstream_runtime_only`; require a written rationale and the closest Rust boundary test. Do not classify codegen, variable, style, asset or export semantics out of scope.

- [ ] **Step 3: Run the completeness gate until it reaches exact baseline counts**

Run: `cargo test -p devup-mcp-devup-ui --test compat_manifest`

Expected: 54 source files, 978 ledger IDs, 268 imported upstream snapshots, zero unclassified IDs, zero orphan files and valid checksums.

- [ ] **Step 4: Run every JSON fixture**

Run: `cargo insta test -p devup-mcp-devup-ui --test compat_fixtures`

Expected: all cases pass and `cargo insta pending-snapshots` reports none.

- [ ] **Step 5: Commit**

```bash
git add fixtures/devup-figma-plugin crates/devup-mcp-figma/tests crates/devup-mcp-devup-ui/tests crates/devup-mcp/tests
git commit -m "test: complete plugin parity ledger"
```

### Task 8: CI, Documentation, Changepack and Full Verification

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md`
- Modify: `.changepacks/changepack_log_figma_remote_mcp.json`

**Interfaces:**
- Produces: Cargo-only offline fixture CI and documented JSON authoring/review workflow.

- [ ] **Step 1: Add snapshot CI without JavaScript setup**

Install `cargo-insta` 1.48.0 in CI and run `cargo insta test --workspace --all-features`. Fail on `.snap.new` files and run the manifest test before the broader suite.

```yaml
- run: cargo install cargo-insta --version 1.48.0 --locked
- run: cargo test -p devup-mcp-devup-ui --test compat_manifest
- run: cargo insta test --workspace --all-features
```

- [ ] **Step 2: Document fixture authoring**

Document that new fixtures must be serialized from `CollectedPayload`, must use synthetic values, require source provenance, and are reviewed with `cargo insta review`. State that changing snapshots is an output-contract change.

- [ ] **Step 3: Add the changepack**

Record minor changes for `devup-mcp-devup-ui` and any public payload-type change in `devup-mcp-figma`.

- [ ] **Step 4: Run all verification commands**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Run: `cargo insta test --workspace --all-features`

Run: `cargo test --workspace --all-features`

Run: `cargo build --workspace --release`

Expected: all exit 0 and there are no pending snapshots.

- [ ] **Step 5: Verify the project remains Cargo-only and fixtures are synthetic**

Run: `rg --files -g 'package.json' -g 'bun.lock*' -g 'node_modules/**'`

Expected: no output.

Run: `rg -n "85CgSws3o5XsLv7aAwWJyS|3879:35481" fixtures`

Expected: no output.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/ci.yml README.md .changepacks/changepack_log_figma_remote_mcp.json Cargo.toml Cargo.lock crates/devup-mcp-devup-ui/Cargo.toml
git commit -m "ci: enforce json fixture parity"
```
