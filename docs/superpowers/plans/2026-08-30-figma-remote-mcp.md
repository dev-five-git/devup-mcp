# Figma Remote MCP Implementation Plan

> 이 최초 계획은 현재 구현 이후의 fallback·live contract·fixture parity·검색 범위를
> 포함하지 않는다. 후속 작업은 `2026-08-31-figma-collection-live-contract.md`,
> `2026-08-31-json-fixture-parity.md`, `2026-08-31-figma-search-and-export.md` 순서로 실행한다.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust Cargo workspace that ships one `devup-mcp` binary, authenticates to Figma Remote MCP, reads a linked design without modifying it, preserves an exhaustive node snapshot, and emits DevupUI TSX or `devup.json`.

**Architecture:** `crates/devup-mcp` is the downstream stdio MCP binary, `crates/devup-mcp-figma` owns OAuth, the read-only upstream MCP allowlist, credential storage and raw-preserving snapshots, and `crates/devup-mcp-devup-ui` owns deterministic TSX/theme projection. There is no separate IR, auth, or server crate.

**Tech Stack:** Rust 1.88+, edition 2024, Tokio, rmcp 3.1, reqwest/rustls, serde/schemars, keyring 4, axum loopback callback, oauth2/PKCE helpers, tracing, cargo-nextest-compatible tests.

**Spec:** `docs/superpowers/specs/2026-08-30-figma-remote-mcp-design.md`

> Workspace correction: the task-by-task paths below record the original implementation order. Their final locations are `src/figma/* -> crates/devup-mcp-figma/src/*`, `src/codegen/*` and `src/theme/* -> crates/devup-mcp-devup-ui/src/*`, and `src/server/*`, `src/main.rs`, `src/lib.rs` -> `crates/devup-mcp/src/*`.

## Global Constraints

- Product name, executable name and Cargo package are exactly `devup-mcp`.
- Keep one Cargo package; split responsibilities into focused internal modules only.
- Support Windows, macOS and Linux with Rust 1.88 or newer.
- The downstream transport is stdio; stdout contains MCP protocol frames only.
- The default upstream endpoint is exactly `https://mcp.figma.com/mcp` and is injectable only for tests.
- Figma access is read-only: only `get_metadata`, `get_variable_defs`, `get_design_context`, `get_code_connect_map`, `get_screenshot`, and fixed built-in read-only `use_figma` scripts may be called.
- Never accept arbitrary JavaScript, never call Figma write tools, and never persist source design snapshots or screenshots by default.
- Use OAuth discovery, dynamic client registration, PKCE S256 and a loopback callback; no embedded client secret or PAT.
- Persist OAuth registration and tokens only in the OS credential store; redact secrets from errors, Debug output and tracing.
- Preserve every readable public Plugin API data property in raw JSON; typed views are additive and unknown enumerable fields live under `extra`.
- Functions, cyclic host references, inaccessible private data and binary asset bytes are explicitly excluded as documented in the spec.
- Each implementation task follows red-green-refactor and ends with a focused commit.

---

### Task 1: Rust Package and Downstream stdio MCP Skeleton

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`
- Create: `.github/workflows/ci.yml`
- Create: `src/lib.rs`
- Create: `src/main.rs`
- Create: `src/server/mod.rs`
- Create: `src/server/tools.rs`
- Test: `tests/stdio_tools.rs`

**Interfaces:**
- Produces: `pub struct DevupServer`, `impl DevupServer { pub fn new(services: Services) -> Self }`, and `pub async fn run_stdio() -> anyhow::Result<()>`.
- Produces: downstream input structs `AuthInput`, `FigmaToUiInput`, `FigmaToJsonInput` and output structs that derive `Serialize`, `Deserialize`, `JsonSchema`.
- Consumes: no earlier implementation task.

- [ ] **Step 1: Add the package manifest and a failing tool-list integration test**

  Configure `rmcp = { version = "3.1", features = ["server", "transport-io", "client", "transport-streamable-http-client-reqwest", "auth", "reqwest"] }`, Tokio full, serde, serde_json, schemars, thiserror, anyhow, tracing, tracing-subscriber, url, reqwest with rustls, axum, keyring, oauth2, sha2, base64, rand, subtle, webbrowser and async-trait. In `tests/stdio_tools.rs`, connect an rmcp client to an in-memory duplex transport and assert the sorted names are `devup_figma_auth`, `devup_figma_to_json`, and `devup_figma_to_ui`.

- [ ] **Step 2: Run the test and confirm the package is absent**

  Run: `cargo test --test stdio_tools -- --nocapture`

  Expected: FAIL because `Cargo.toml` or `DevupServer` does not exist.

- [ ] **Step 3: Implement the minimal rmcp server and safe process entrypoint**

  Use `#[tool_router]`, `#[tool_handler(router = self.tool_router)]`, and `ServiceExt::serve((tokio::io::stdin(), tokio::io::stdout()))`. Initialize tracing with stderr as the writer, return rmcp structured errors from placeholder service methods, and keep all request/output types in `server/tools.rs`.

- [ ] **Step 4: Add CI and verify the skeleton**

  CI runs `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all-features`, and `cargo build --release` on Windows, macOS and Ubuntu. Run locally: `cargo fmt --all && cargo test --test stdio_tools && cargo clippy --all-targets --all-features -- -D warnings`.

  Expected: all commands PASS and the integration test observes exactly three downstream tools.

- [ ] **Step 5: Commit**

  Run: `git add Cargo.toml Cargo.lock rust-toolchain.toml .gitignore .github src tests/stdio_tools.rs && git commit -m "feat: add Rust MCP server skeleton"`.

### Task 2: Strict Figma URL Parsing and Stable Errors

**Files:**
- Create: `src/figma/mod.rs`
- Create: `src/figma/url.rs`
- Create: `src/figma/errors.rs`
- Modify: `src/lib.rs`
- Test: `tests/figma_url.rs`

**Interfaces:**
- Produces: `pub struct FigmaTarget { pub file_key: String, pub node_id: Option<String>, pub branch_key: Option<String> }`.
- Produces: `impl FigmaTarget { pub fn parse(input: &str) -> Result<Self, DevupError> }`.
- Produces: `pub struct DevupError { pub code: ErrorCode, pub message: String, pub retryable: bool, pub details: serde_json::Value }` and the error codes enumerated by the spec.
- Consumes: downstream string URLs from Task 1.

- [ ] **Step 1: Write table-driven failing parser and redaction tests**

  Cover `/design/<key>/...`, `/file/<key>/...`, URL-encoded names, `node-id=3879-35481` normalized to `3879:35481`, branch URLs, missing node ids, wrong schemes, deceptive suffix hosts such as `figma.com.evil.test`, short file keys, fragments, and query parameters containing token-like values. Assert `Display` and `Debug` never contain input query secrets.

- [ ] **Step 2: Run the tests and confirm missing parser failures**

  Run: `cargo test --test figma_url -- --nocapture`

  Expected: FAIL because `figma::url::FigmaTarget` is unresolved.

- [ ] **Step 3: Implement strict parsing and safe errors**

  Accept only HTTPS and hosts `figma.com` or `www.figma.com`; accept only `design`, `file`, and `branch` routes; validate keys with ASCII alphanumeric, underscore and hyphen; parse only the `node-id` query parameter and discard all other query data. Implement manual redacted `Debug` for sensitive error contexts.

- [ ] **Step 4: Verify parser behavior and lint**

  Run: `cargo test --test figma_url && cargo clippy --all-targets --all-features -- -D warnings`.

  Expected: PASS, including hostile-host and secret-redaction cases.

- [ ] **Step 5: Commit**

  Run: `git add src/figma src/lib.rs tests/figma_url.rs && git commit -m "feat: parse Figma links safely"`.

### Task 3: OAuth Discovery, PKCE, Loopback Callback and Keyring Storage

**Files:**
- Create: `src/figma/credentials.rs`
- Create: `src/figma/oauth.rs`
- Modify: `src/figma/mod.rs`
- Test: `tests/oauth_flow.rs`

**Interfaces:**
- Produces: `#[async_trait] pub trait CredentialStore { async fn load(&self) -> Result<Option<StoredAuthorization>, DevupError>; async fn save(&self, value: &StoredAuthorization) -> Result<(), DevupError>; async fn clear(&self) -> Result<(), DevupError>; }`.
- Produces: `pub struct OAuthManager<S: CredentialStore>`, `pub async fn status(&self) -> Result<AuthStatus, DevupError>`, `pub async fn login(&self, opener: &dyn BrowserOpener) -> Result<StoredAuthorization, DevupError>`, `pub async fn logout(&self) -> Result<(), DevupError>`, and `pub async fn access_token(&self) -> Result<SecretString, DevupError>`.
- Produces: `BrowserOpener` and in-memory `CredentialStore` test seams.
- Consumes: `DevupError` from Task 2.

- [ ] **Step 1: Write failing mock-server OAuth tests**

  Use an axum test server for protected-resource metadata, authorization-server metadata, dynamic registration, and token endpoints. Assert discovery ordering, exact resource binding, requested scope `mcp:connect`, S256 challenge, random state, callback host `127.0.0.1`, successful code exchange, refresh-on-expiry, single refresh retry, logout, callback timeout, state mismatch, and that serialized/logged errors omit code, verifier, access token, refresh token and registration secret.

- [ ] **Step 2: Run OAuth tests and confirm missing implementation**

  Run: `cargo test --test oauth_flow -- --nocapture`

  Expected: FAIL because `OAuthManager` and `CredentialStore` are unresolved.

- [ ] **Step 3: Implement discovery and dynamic registration**

  Validate metadata issuer/resource URLs and HTTPS endpoints except injected loopback test endpoints. Register a public client with the exact ephemeral redirect URI, `authorization_code` grant and `none` token authentication when supported. Generate 32-byte CSPRNG state and verifier, derive S256 using SHA-256/base64url without padding, and compare callback state with `subtle::ConstantTimeEq`.

- [ ] **Step 4: Implement ephemeral callback and keyring persistence**

  Bind `TcpListener` to `127.0.0.1:0`, serve only `/callback`, cap the request size, enforce a three-minute timeout, show a static success/failure page, then shut down. Store one versioned JSON credential value under service `devup-mcp` and account `figma-remote-mcp`; do not provide a plaintext fallback.

- [ ] **Step 5: Verify OAuth security behavior**

  Run: `cargo test --test oauth_flow && cargo test && cargo clippy --all-targets --all-features -- -D warnings`.

  Expected: PASS; timeout and state mismatch are stable error codes and secret sentinel strings do not occur in captured diagnostics.

- [ ] **Step 6: Commit**

  Run: `git add src/figma tests/oauth_flow.rs Cargo.toml Cargo.lock && git commit -m "feat: authenticate Figma with OAuth"`.

### Task 4: Read-only Figma Remote MCP Client

**Files:**
- Create: `src/figma/upstream.rs`
- Modify: `src/figma/mod.rs`
- Test: `tests/upstream_contract.rs`

**Interfaces:**
- Produces: `#[async_trait] pub trait FigmaUpstream { async fn list_tools(&self) -> Result<Vec<String>, DevupError>; async fn call_read_tool(&self, call: ReadToolCall) -> Result<CallToolResult, DevupError>; }`.
- Produces: `pub enum ReadToolCall { Metadata { file_key: String, node_id: Option<String> }, VariableDefs { file_key: String, node_id: String }, DesignContext { file_key: String, node_id: String }, CodeConnectMap { file_key: String, node_id: String }, Screenshot { file_key: String, node_id: String }, Snapshot { file_key: String, node_id: String, script: BuiltinScript } }`.
- Produces: `pub struct RemoteFigmaClient<S: CredentialStore>` with endpoint injection restricted to `cfg(test)` or an explicit constructor hidden from downstream tool input.
- Consumes: `OAuthManager::access_token` from Task 3 and rmcp Streamable HTTP client support.

- [ ] **Step 1: Write a failing MCP contract test**

  Start a local Streamable HTTP MCP fixture that records initialize, tools/list and tools/call. Assert the client negotiates capabilities, propagates only bearer authorization, calls all six allowed read tools, rejects names such as `generate_figma_design`, `upload_assets`, `add_code_connect_map`, and does not forward user-provided JavaScript. Exercise 401 refresh once, 429 with bounded Retry-After, malformed content and missing `use_figma` capability.

- [ ] **Step 2: Run and observe the missing upstream client**

  Run: `cargo test --test upstream_contract -- --nocapture`

  Expected: FAIL because `RemoteFigmaClient` and `ReadToolCall` do not exist.

- [ ] **Step 3: Implement the rmcp client and closed call enum**

  Build `StreamableHttpClientTransport` with a bearer-aware reqwest client, initialize with rmcp, inspect `tools/list`, and translate each enum variant into exact upstream arguments. The `Snapshot` variant maps only an internal `BuiltinScript` enum to a compiled-in source string; no public function accepts script text.

- [ ] **Step 4: Implement retry and response validation**

  Refresh a rejected token once, honor a valid bounded retry hint once for 429, cap accumulated tool result bytes, validate text/JSON content blocks, and map errors to stable `DevupError` codes without response headers or tokens.

- [ ] **Step 5: Verify the contract**

  Run: `cargo test --test upstream_contract && cargo clippy --all-targets --all-features -- -D warnings`.

  Expected: PASS and the fixture log contains no write-tool invocation.

- [ ] **Step 6: Commit**

  Run: `git add src/figma tests/upstream_contract.rs && git commit -m "feat: add read-only Figma MCP client"`.

### Task 5: Exhaustive Raw-preserving Figma Snapshot

**Files:**
- Create: `src/figma/snapshot.rs`
- Create: `src/figma/plugin_api_manifest.json`
- Create: `src/figma/scripts/snapshot.js`
- Create: `src/figma/scripts/variables.js`
- Create: `scripts/check_plugin_api_manifest.rs`
- Modify: `src/figma/mod.rs`
- Test: `tests/snapshot.rs`
- Test: `tests/fixtures/snapshot/frame.json`
- Test: `tests/fixtures/snapshot/text.json`
- Test: `tests/fixtures/plugin-api.d.ts`

**Interfaces:**
- Produces: `pub struct Snapshot { pub file_key: String, pub version: Option<String>, pub roots: Vec<String>, pub nodes: BTreeMap<String, RawNode>, pub diagnostics: Vec<Diagnostic> }`.
- Produces: `pub struct RawNode { pub id: String, pub node_type: String, pub fields: serde_json::Map<String, Value>, pub extra: serde_json::Map<String, Value>, pub field_errors: BTreeMap<String, String> }`.
- Produces: `pub fn merge_chunks(chunks: Vec<SnapshotChunk>) -> Result<Snapshot, DevupError>` and `pub fn typed_view(node: &RawNode) -> TypedNode<'_>`.
- Consumes: `FigmaUpstream::call_read_tool(ReadToolCall::Snapshot { ... })` from Task 4.

- [ ] **Step 1: Add failing raw preservation and merge tests**

  Load synthetic Frame and Text fixtures with fields not consumed by codegen. Assert every fixture property survives deserialization/serialization, unknown enumerable values remain in `extra`, getter failures remain in `fieldErrors`, node references are ids, child order is stable, duplicate equal nodes deduplicate, conflicting nodes fail, and mismatched versions return `DEVUP_FIGMA_VERSION_CHANGED`.

- [ ] **Step 2: Add a failing manifest coverage test**

  Parse the checked-in synthetic Plugin API typings fixture, collect readonly/data properties from node interfaces and mixins, subtract only documented exclusions (`methods`, `parent` object, raw bytes, private plugin data), and assert the resulting names equal `plugin_api_manifest.json`. Include a synthetic newly added property so the first run proves the check fails.

- [ ] **Step 3: Run snapshot tests and confirm failures**

  Run: `cargo test --test snapshot -- --nocapture`

  Expected: FAIL because snapshot types, merge and manifest coverage are absent.

- [ ] **Step 4: Implement raw-first Rust snapshot types and merge**

  Deserialize `fields`, `extra`, and `fieldErrors` without `deny_unknown_fields`; keep `serde_json::Map` as the source of truth. Implement borrowed typed accessors for layout, paint, text, component and variable bindings so adding codegen support never removes raw data.

- [ ] **Step 5: Implement fixed read-only serializer scripts**

  The script obtains a node by id, walks descendants, reads the manifest property-by-property inside `try/catch`, serializes primitives/arrays/plain records recursively with cycle detection, maps node references to ids, sends unsupported enumerable data to `extra`, and records failed getters by field name. The variable script uses only `getLocalVariableCollectionsAsync`, `getLocalVariablesAsync`, collection modes and resolved binding reads; neither script assigns to a Figma object or calls a mutation API.

- [ ] **Step 6: Implement chunk planning and deterministic merge**

  Use metadata counts to select whole-node or direct-child chunks, execute with bounded concurrency four, sort by child order, reject cross-version merges, and split a failed oversized subtree once before returning a diagnostic.

- [ ] **Step 7: Verify snapshot completeness checks**

  Run: `cargo test --test snapshot && cargo test && cargo clippy --all-targets --all-features -- -D warnings`.

  Expected: PASS; adding an undeclared property to the typings fixture makes only the manifest coverage assertion fail.

- [ ] **Step 8: Commit**

  Run: `git add src/figma scripts tests/snapshot.rs tests/fixtures && git commit -m "feat: preserve exhaustive Figma snapshots"`.

### Task 6: Deterministic DevupUI TSX Code Generation

**Files:**
- Create: `src/codegen/mod.rs`
- Create: `src/codegen/component.rs`
- Create: `src/codegen/layout.rs`
- Create: `src/codegen/style.rs`
- Create: `src/codegen/text.rs`
- Modify: `src/lib.rs`
- Test: `tests/codegen.rs`
- Test: `tests/fixtures/codegen/auto-layout.json`
- Test: `tests/fixtures/codegen/absolute-and-mask.json`
- Test: `tests/fixtures/codegen/expected.tsx`

**Interfaces:**
- Produces: `pub struct CodegenOptions { pub component_name: Option<String>, pub include_diagnostics: bool }`.
- Produces: `pub struct CodegenOutput { pub tsx: String, pub imports: Vec<String>, pub used_tokens: BTreeSet<String>, pub diagnostics: Vec<Diagnostic> }`.
- Produces: `pub fn generate_component(snapshot: &Snapshot, root_id: &str, options: &CodegenOptions) -> Result<CodegenOutput, DevupError>`.
- Consumes: `Snapshot`, `RawNode`, and `typed_view` from Task 5.

- [ ] **Step 1: Write failing golden tests**

  Assert horizontal/vertical Auto Layout maps to `Flex`, ordinary containers to `Box`, text to `Text`, theme bindings to `$token`, unbound values to deterministic CSS, component names such as `[FR-026] 본연체` become valid non-empty TypeScript identifiers, text is escaped, imports are sorted, and identical input produces byte-identical TSX. Assert absolute positioning, masks, vectors, images and unsupported effects emit explicit diagnostics.

- [ ] **Step 2: Run and confirm missing codegen**

  Run: `cargo test --test codegen -- --nocapture`

  Expected: FAIL because `generate_component` is unresolved.

- [ ] **Step 3: Implement component tree and identifier generation**

  Traverse child ids from the snapshot, normalize Unicode names into a PascalCase identifier with `FigmaComponent` fallback and a leading underscore for digits, generate JSX with two-space indentation, and escape strings using JSON string encoding.

- [ ] **Step 4: Implement layout, style and text mapping**

  Map supported typed fields to DevupUI props, prefer variable aliases rendered with `$`, normalize colors and lengths, sort properties and imports, and append node-id-scoped diagnostics for every fallback. Do not read fields outside typed accessors, so raw preservation remains independent.

- [ ] **Step 5: Verify golden output and parseability**

  Run: `cargo test --test codegen && cargo test && cargo clippy --all-targets --all-features -- -D warnings`.

  Expected: PASS and `expected.tsx` matches byte-for-byte.

- [ ] **Step 6: Commit**

  Run: `git add src/codegen src/lib.rs tests/codegen.rs tests/fixtures/codegen && git commit -m "feat: generate DevupUI from Figma snapshots"`.

### Task 7: Figma Variables and `devup.json`

**Files:**
- Create: `src/theme/mod.rs`
- Create: `src/theme/tokens.rs`
- Create: `src/theme/devup_json.rs`
- Modify: `src/lib.rs`
- Test: `tests/theme.rs`
- Test: `tests/fixtures/theme/variables.json`
- Test: `tests/fixtures/theme/expected.devup.json`

**Interfaces:**
- Produces: `pub enum ThemeScope { Node, Page, File }` and `pub enum Completeness { FullLocalPlusUsedRemote, UsedTokens, ResolvedValuesOnly }`.
- Produces: `pub struct VariableSnapshot`, `pub struct ThemeOutput { pub json: String, pub counts: ThemeCounts, pub completeness: Completeness, pub diagnostics: Vec<Diagnostic> }`.
- Produces: `pub fn generate_devup_json(snapshot: &Snapshot, variables: &VariableSnapshot, scope: ThemeScope) -> Result<ThemeOutput, DevupError>`.
- Consumes: built-in variable snapshots from Task 5 and used bindings from `Snapshot`.

- [ ] **Step 1: Write failing theme mapping tests**

  Assert COLOR maps to `theme.colors.<mode>`, FLOAT dimensions to `theme.length.<mode>`, font composites to `theme.typography`, effect composites to `theme.shadow`, WEB `codeSyntax` wins token naming, original names remain in diagnostics, aliases resolve transitively, alias cycles and normalized-name collisions are reported, modes are stable keys, and JSON key order is deterministic.

- [ ] **Step 2: Run and confirm missing theme generator**

  Run: `cargo test --test theme -- --nocapture`

  Expected: FAIL because theme types and `generate_devup_json` are absent.

- [ ] **Step 3: Implement token normalization, alias resolution and completeness**

  Normalize tokens deterministically, preserve original collection/variable/mode ids in diagnostics, resolve aliases with an explicit visiting set, and compute `full-local-plus-used-remote`, `used-tokens`, or `resolved-values-only` from collection and binding evidence without inventing absent tokens.

- [ ] **Step 4: Implement deterministic Devup schema serialization**

  Build ordered maps for `theme.colors`, `theme.typography`, `theme.length`, and `theme.shadow`; retain all modes; format four-space pretty JSON with a final newline; return `DEVUP_THEME_CONFLICT` only when deterministic disambiguation cannot retain both meanings.

- [ ] **Step 5: Verify schema output**

  Run: `cargo test --test theme && cargo test && cargo clippy --all-targets --all-features -- -D warnings`.

  Expected: PASS and the golden `devup.json` is byte-identical.

- [ ] **Step 6: Commit**

  Run: `git add src/theme src/lib.rs tests/theme.rs tests/fixtures/theme && git commit -m "feat: generate devup theme from Figma variables"`.

### Task 8: Wire Tools, Live Smoke Test, Documentation and Release Checks

**Files:**
- Modify: `src/server/mod.rs`
- Modify: `src/server/tools.rs`
- Modify: `src/main.rs`
- Modify: `README.md`
- Create: `.changepacks/config.json`
- Create: `.changepacks/figma-remote-mcp.md`
- Create: `tests/downstream_integration.rs`
- Create: `tests/live_figma.rs`

**Interfaces:**
- Produces: working `devup_figma_auth`, `devup_figma_to_ui`, and `devup_figma_to_json` calls over stdio.
- Produces: opt-in live test selected with `DEVUP_MCP_LIVE_FIGMA=1`, using file `85CgSws3o5XsLv7aAwWJyS` and node `3879:35481` without saving its returned design data.
- Consumes: all public interfaces from Tasks 2 through 7.

- [ ] **Step 1: Write failing downstream end-to-end tests**

  Inject memory credentials and a fake `FigmaUpstream`, invoke each tool through an rmcp client, and assert structured outputs include source identifiers, TSX or JSON, counts, completeness and optional diagnostics. Verify auto-login on missing auth, logout clearing credentials, invalid URL rejection before upstream calls, and a call log containing only allowlisted reads.

- [ ] **Step 2: Run and confirm wiring failures**

  Run: `cargo test --test downstream_integration -- --nocapture`

  Expected: FAIL because placeholder handlers do not invoke the services.

- [ ] **Step 3: Wire service orchestration and structured MCP results**

  Add a `Services` aggregate of credential, OAuth and upstream abstractions, collect the requested target, call snapshot/theme/codegen pipelines, and map `DevupError` to stable rmcp error data. Include diagnostics only when requested while always returning completeness and safe counts.

- [ ] **Step 4: Add the opt-in live smoke test**

  When `DEVUP_MCP_LIVE_FIGMA=1` is absent, return early. When present, load the OS keyring authorization, call the provided node, assert the output contains an exported React function and valid JSON, and keep all response bodies in memory only.

- [ ] **Step 5: Document installation, OAuth and limitations**

  Document Cargo installation, generic MCP client stdio configuration, first-call browser authorization, three tool schemas, read-only guarantee, credential location, logout, supported theme categories, exhaustive-snapshot exclusions, external unused-variable limitation, live test command and troubleshooting codes. Add a changepack describing the first private Figma feature without including design content or credentials.

- [ ] **Step 6: Run focused and full verification**

  Run in order:

  ```text
  cargo fmt --all -- --check
  cargo clippy --all-targets --all-features -- -D warnings
  cargo test --all-features
  cargo build --release
  cargo test --test downstream_integration
  ```

  Expected: every command exits 0. If valid authorization is already present, additionally run `DEVUP_MCP_LIVE_FIGMA=1 cargo test --test live_figma -- --ignored --nocapture`; otherwise record the live OAuth smoke test as the only manual verification remaining.

- [ ] **Step 7: Self-review against the specification**

  Confirm every downstream tool exists, no public API accepts JavaScript, all upstream calls are enum-closed, raw snapshot fields survive, CI catches manifest drift, stdout is protocol-only, keyring has no plaintext fallback, test fixtures are synthetic, no credential or real Figma response is tracked, and `rg -n "PAT|client_secret|access_token|refresh_token" . --glob '!Cargo.lock'` shows only documentation, types or redaction tests with no literal secret values.

- [ ] **Step 8: Commit, push and open the pull request**

  Run:

  ```text
  git add src tests README.md Cargo.toml Cargo.lock .github .changepacks scripts docs
  git commit -m "feat: complete Figma to Devup MCP"
  git push origin owjs3901/figma-remote-mcp
  gh pr create --base main --head owjs3901/figma-remote-mcp --title "feat: add Figma Remote MCP integration" --body-file <reviewed-pr-body-path>
  ```

  The PR body states architecture, read-only/OAuth security, exhaustive snapshot semantics, codegen/theme behavior, exact verification commands, live-test status and remaining risks; create the body in a temporary file outside the repository and remove it after use.

## Plan Self-review

- Spec coverage: OAuth, upstream MCP, read-only allowlist, exhaustive snapshot, chunk/version handling, codegen, variables, downstream tools, security, tests, packaging and documentation each map to Tasks 1–8.
- Placeholder scan: implementation steps name concrete files, interfaces, commands, assertions and expected outcomes; no deferred implementation markers are used.
- Type consistency: `DevupError`, `FigmaTarget`, `CredentialStore`, `OAuthManager`, `FigmaUpstream`, `ReadToolCall`, `Snapshot`, `CodegenOutput` and `ThemeOutput` are introduced before their downstream consumers and retain the same signatures throughout.
