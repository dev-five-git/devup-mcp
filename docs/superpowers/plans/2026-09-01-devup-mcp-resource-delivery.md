# Devup MCP Resource Delivery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver large generated text and binary through bounded standard MCP Resources without changing small inline results.

**Architecture:** Attach projected output blobs to the acquisition artifact entry, expose manifest/chunk URIs through rmcp resources, and select inline/resource delivery through a deterministic policy. Publish resources only after projection, strict validation and optional filesystem transaction all succeed.

**Tech Stack:** Rust 2024, rmcp 3.1.4 Resources API, Serde, SHA-256, base64, bounded in-memory LRU/TTL artifact store.

**Spec:** `docs/superpowers/specs/2026-09-01-devup-mcp-hardening-and-delivery-design.md`

## Global Constraints

- `delivery` is `auto`, `inline`, or `resource`; default `auto` inlines each output up to 256 KiB and all outputs up to 1 MiB.
- Each resource raw chunk is at most 256 KiB.
- URI paths contain only random/opaque IDs and output names, never Figma file/node/asset IDs or design names.
- Attached output resources expire and evict atomically with their acquisition artifact.
- Small auto responses preserve existing inline output fields.

## File map

- `crates/devup-mcp/src/server/delivery.rs`: delivery enum, thresholds, output staging and response assembly.
- `crates/devup-mcp/src/server/resources.rs`: URI parsing, resource manifest/chunk model, list/read handlers.
- `crates/devup-mcp/src/server/artifacts.rs`: attached resource storage, byte accounting and projection-option cache.
- `crates/devup-mcp/src/server/tools.rs`: `delivery` input.
- `crates/devup-mcp/src/server/mod.rs`: resource capability and handler wiring.
- `crates/devup-mcp/tests/resource_delivery.rs`: boundary, hash, TTL/LRU and protocol tests.
- `crates/devup-mcp/tests/stdio_tools.rs`: advertised capabilities and end-to-end Resource Link tests.
- `README.md`: client examples.

---

### Task 1: Model delivery and attached resource blobs

**Files:**
- Create: `crates/devup-mcp/src/server/delivery.rs`
- Modify: `crates/devup-mcp/src/server/tools.rs`
- Modify: `crates/devup-mcp/src/server/artifacts.rs`
- Create: `crates/devup-mcp/tests/resource_delivery.rs`

**Interfaces:**
- Produces: `DeliveryMode::{Auto, Inline, Resource}`, `ProjectedOutput`, `AttachedOutput`, `ArtifactStore::attach_outputs`, and `ArtifactStore::read_output_chunk`.

- [ ] **Step 1: Write failing delivery boundary tests**

Assert auto inlines an output of exactly 256 KiB, resources one byte above, and resources otherwise-small outputs when aggregate size exceeds 1 MiB. Assert explicit inline above the hard response limit errors and explicit resource always attaches. Verify serialization accepts only `auto`, `inline`, and `resource`.

- [ ] **Step 2: Write failing store tests**

Attach text and binary outputs, assert acquisition content hash is unchanged, output hashes match bytes, raw chunks never exceed 256 KiB, encoded bytes count toward store limits, and artifact eviction/expiry removes all attached output reads.

- [ ] **Step 3: Run focused tests**

Run: `cargo test -p devup-mcp --test resource_delivery model store -- --nocapture`

Expected: FAIL because delivery/attached output types do not exist.

- [ ] **Step 4: Implement model and store**

Parse delivery with Serde-backed enum defaults. Stage `ProjectedOutput { name, mime_type, bytes, is_binary }`, compute SHA-256, split into fixed raw chunks, generate independent random output/capture IDs, and attach all outputs under a projection option hash while holding the artifact store lock. Include output raw/encoded allocation in LRU limits without changing acquisition content hash.

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p devup-mcp --test resource_delivery model store -- --nocapture`

Expected: PASS.

```text
git add crates/devup-mcp/src/server/delivery.rs crates/devup-mcp/src/server/tools.rs crates/devup-mcp/src/server/artifacts.rs crates/devup-mcp/tests/resource_delivery.rs
git commit -m "feat: store bounded devup output resources"
```

### Task 2: Implement MCP manifest and chunk resources

**Files:**
- Create: `crates/devup-mcp/src/server/resources.rs`
- Modify: `crates/devup-mcp/src/server/artifacts.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/tests/resource_delivery.rs`
- Modify: `crates/devup-mcp/tests/stdio_tools.rs`

**Interfaces:**
- Produces: `ResourceAddress::parse`, paginated manifest listing, template listing, and `ServerHandler::{list_resources,list_resource_templates,read_resource}`.

- [ ] **Step 1: Add resource protocol tests**

Initialize the server and assert tools plus resources are advertised while subscribe/list-changed are absent. Attach an output and assert `resources/list` returns only its top-level manifest with cursor pagination, templates describe output/asset manifest/chunk URIs, manifest read returns MIME/size/hash/chunk count/expiry, and every chunk round-trips to the original bytes.

- [ ] **Step 2: Add negative protocol tests**

Assert malformed URI, unknown artifact/output, invalid chunk index, modified opaque ID, expired artifact and evicted artifact return MCP resource-not-found without leaking whether a Figma ID exists.

- [ ] **Step 3: Run and confirm capability/handlers are missing**

Run: `cargo test -p devup-mcp --test resource_delivery protocol --test stdio_tools resource -- --nocapture`

Expected: FAIL because the server advertises tools only.

- [ ] **Step 4: Implement resources module and handlers**

Parse only the documented `devup://artifact/...` URI grammar. Generate `Resource`, `ResourceTemplate`, `ReadResourceResult`, text `ResourceContents`, and blob `ResourceContents` through rmcp 3.1.4. Use bounded pagination and list manifests only. Map every missing/expired component to the same resource-not-found error.

- [ ] **Step 5: Advertise resource capability**

Change `get_info` to `ServerCapabilities::builder().enable_resources().enable_tools().build()` without subscribe/list-changed. Route reads to the artifact store and touch artifact LRU on a successful read.

- [ ] **Step 6: Run tests and commit**

Run: `cargo test -p devup-mcp --test resource_delivery --test stdio_tools -- --nocapture`

Expected: PASS.

```text
git add crates/devup-mcp/src/server/resources.rs crates/devup-mcp/src/server/artifacts.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/tests/resource_delivery.rs crates/devup-mcp/tests/stdio_tools.rs
git commit -m "feat: expose devup artifacts as mcp resources"
```

### Task 3: Integrate auto/resource delivery with export and outputPath

**Files:**
- Modify: `crates/devup-mcp/src/server/delivery.rs`
- Modify: `crates/devup-mcp/src/server/handoff.rs`
- Modify: `crates/devup-mcp/src/server/mod.rs`
- Modify: `crates/devup-mcp/tests/composite_export.rs`
- Modify: `crates/devup-mcp/tests/resource_delivery.rs`

**Interfaces:**
- Consumes: validated projected outputs and committed output transaction hashes.
- Produces: inline fields or Resource Links plus manifest metadata.

- [ ] **Step 1: Add export delivery tests**

Generate small TSX/JSON and assert default auto preserves current fields. Generate large raw snapshot/text and assert auto omits duplicated content, returns Resource Links, and reading chunks reconstructs exact output. Assert resource mode works for small content, inline hard-limit error writes/publishes nothing, and `outputPath + resource` produces identical SHA-256 values.

- [ ] **Step 2: Run focused tests**

Run: `cargo test -p devup-mcp --test composite_export delivery --test resource_delivery export -- --nocapture`

Expected: FAIL because export ignores delivery.

- [ ] **Step 3: Thread delivery through handoff and projection**

Preserve `DeliveryMode` in every pending export/wrapper. Convert TSX, devup JSON, raw snapshot, source map, manifest and asset bytes into `ProjectedOutput` values. Run capability/quality/syntax checks, stage filesystem transaction, decide delivery, commit files, then attach resources and build response. If any step fails, publish neither files nor resources.

- [ ] **Step 4: Cache projection outputs**

Hash component name, outputs, root layout, scope, selected roots, diagnostics flag and asset captures into a projection key. Reuse attached outputs for the same key without codegen or upstream calls. Do not reuse output paths as part of resource identity.

- [ ] **Step 5: Run integration tests and commit**

Run: `cargo test -p devup-mcp --test composite_export --test resource_delivery -- --nocapture`

Expected: PASS and existing small-response assertions remain unchanged.

```text
git add crates/devup-mcp/src/server/delivery.rs crates/devup-mcp/src/server/handoff.rs crates/devup-mcp/src/server/mod.rs crates/devup-mcp/tests/composite_export.rs crates/devup-mcp/tests/resource_delivery.rs
git commit -m "feat: deliver large figma outputs as resources"
```

### Task 4: Document and verify Resource delivery

**Files:**
- Modify: `README.md`
- Modify: `.changepacks/changepack_log_figma_remote_mcp.json`

**Interfaces:**
- Documents: `delivery`, thresholds, URI lifetime, outputPath interaction and privacy.

- [ ] **Step 1: Update documentation and changepack**

Add JSON examples for auto/resource/inline, describe 256 KiB/1 MiB thresholds, TTL/LRU expiry, manifest/chunk reads, and explicit inline rejection. Add a minor changepack entry for resource delivery and secure output behavior using the existing changepack schema.

- [ ] **Step 2: Run focused and workspace checks**

Run: `cargo fmt --all -- --check`, `cargo clippy -p devup-mcp --all-targets --all-features -- -D warnings`, `cargo test -p devup-mcp --all-features`, and `cargo build -p devup-mcp --release`.

Expected: all commands exit 0.

- [ ] **Step 3: Commit**

```text
git add README.md .changepacks/changepack_log_figma_remote_mcp.json
git commit -m "docs: describe mcp resource delivery"
```
