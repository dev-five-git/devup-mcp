# ChondroGen: 14 follow-up improvements

This supplements the [22-item feedback matrix](chondrogen-feedback-coverage.md).
It records implementation and executable regression coverage, rather than
treating instructions or inferred behavior as working application features.
The work is based on main `6cdd6d4`; the installed MCP binary is not replaced by
an unmerged branch.

| # | Improvement | Implementation and evidence |
| --- | --- | --- |
| 1 | Animated content must not hide unrelated clipping | `harness/runtime-audit/clipping-checks.mjs` removes the blanket animated-descendant exemption. `allowedMarquees` requires one explicitly identified horizontal, infinitely translating nowrap track; vertical and sibling clipping still fail. Two real Chromium regressions cover invalid exemptions and mixed clipping. |
| 2 | Run real browser regressions in CI | The existing Linux verification entry in `test/checks.test.mjs` invokes `ci-browser.mjs`: install locked dependencies/Chromium, prepare binaries, then execute every browser test. A child-process guard prevents recursion. No workflow file or extra token permission is required. |
| 3 | Test a production App Router consumer | `fixtures/app-router` and `test/app-router-consumer.test.mjs` use actual vinext, React Compiler, devup-ui extraction and the MCP stdio server. Build, typecheck, SSR without JS, hydration, Korean wrapping, footer height and token values run across 11 widths. Both route transitions verify the same mounted shared-layout DOM and intermediate indicator geometry. |
| 4 | Deliver actual motion implementation | `behavior_contracts/templates/{MotionController.tsx,motion-runtime.ts}` are exported as executable outputs plus explicit semantic/API bindings. One shared controller implements marquee, numeric count-up, progress cycles, persistent route highlights and entrances. Progress stays in 0..1; dynamic step insertion/removal preserves surviving selection; equal-width marquee edits and keyed source replacements refresh copies without unrelated restarts. No-JS readability, teardown, resize and explicit playback preferences are covered in `test/motion.test.mjs`. |
| 5 | Verify intermediate motion | `motion-checks.mjs` samples rendered geometry/state, including leftward marquee movement/visible seam coverage, partial numeric values, progress/manual reset, sliding indicator continuity and reveal opacity/translation. Count probes start before navigation; tab sampling follows actual animation start after route latency. Reversed movement, an empty seam and a static final counter are negative regressions. |
| 6 | Verify real source-bound data changes | `data-checks.mjs` runs baseline, changed, empty and error phases against an explicitly isolated disposable SQLite/API fixture, checking landing/list/detail. `data_verification.rs` rehashes schema and snapshot files, validates typed OpenAPI/model anchors and independently re-executes read-only parameterized SQL. API-only success and edited evidence cannot establish DB verification. |
| 7 | Separate generation from application completion | `completion.rs`, `source-digest.mjs` and `record-project-checks.mjs` implement generated → implemented → browser-verified → data-verified stages. Authentic behavior inventories, actual command exits, current source/public hashes, URL × width coverage, exact applicable motion rows and binding IDs are required. The browser runner independently hashes files before/after execution and rejects stale or changed sources. Screen-plan status re-evaluates the canonical receipt; edited source revokes completion. |
| 8 | Distinguish statistics from years | `behavior_contracts/detect.rs` recognizes unit/context evidence before calendar ranges. `2000건` remains a counter; decimal, negative and currency cases retain precision/units. Rust candidate regressions and real `2000`/negative count-up executions cover the distinction. |
| 9 | Preserve application wiring on regeneration | New `devup_source_sync` plans AST-based three-way merges with source/baseline hashes and an expected plan hash. It merges safe styles and unchanged literal copy while preserving imports, APIs, events and ARIA. Structural ambiguity and edited/dynamic text are conflicts before writing. Source and refreshed identity sidecars commit atomically. |
| 10 | Resolve identity after extraction/reordering | `source_identity.rs` and `export_record/semantic_drift.rs` resolve unique semantic/structural AST matches without production node attributes. Reordered/extracted code is matched conservatively; ambiguous candidates stay unresolved. No runtime DOM identity is inferred from a source match. |
| 11 | Quarantine unresolved public filenames | `projection/asset_quarantine*.rs` withholds the entire TSX/public/registry publication when a meaningful public name is missing. Authorized private staging retains bytes; public-only authority retains the artifact without making a public staging directory. Same-artifact explicit semantic-path reprojection publishes atomically. |
| 12 | Migrate assets with references and rollback | New `devup_assets` inventories public files plus source, seed, persisted JSON and typed PostgreSQL URL references. Plans retain preconditions, canonical copies, compatibility aliases, guarded apply/rollback and reviewable forward/verify/rollback SQL. Changed files and unbounded/dynamic references prevent unsafe retirement. Tests cover commit races and rollback recovery. |
| 13 | Resume bounded behavior analysis | Signed, snapshot-indexed cursors bind artifact, snapshot and scope. Each call scans at most 8,192 nodes and emits at most 128 candidates; final signed inventory retains all candidate/binding IDs. Long compound IDs round-trip through compact cursors. Both token directions enforce 8 MiB; overflow returns explicit incomplete coverage and an executable raw-snapshot resource recovery. Same-artifact continuation supports private sidecar/controller outputs without overwriting the original source export. |
| 14 | Separate tooling setup from browser timing | `prepared-binaries.mjs` fingerprints inputs and builds locked Rust binaries once before assertions. The emitted runner has a minimal Playwright-only package/lock. `scripts/minimize-fixture.mjs` accepts reductions only when complete codegen evidence remains identical. The news-grid fixture shrank from 240,510 to 206,506 bytes with all 100 nodes and TSX/source maps/diagnostics/traces/fidelity/merge evidence preserved. |

## Verification and limits

Verified locally on Windows:

- `cargo fmt --all -- --check` and workspace/all-target/all-feature Clippy with
  `-D warnings`: passed.
- `cargo test --locked --workspace --all-features --no-fail-fast -j 2`:
  2,239 passed, zero failures, two existing ignored tests across 188 targets.
- `npm test --prefix harness/runtime-audit`: 92 passed, zero failures; the one
  Linux CI bootstrap test is intentionally skipped locally on Windows.
- Plugin generation verification, TypeScript check and distribution build:
  passed; plugin/read-script tests: 61 passed.
- The production consumer also passes strict MCP source validation against its
  actual disk theme. MCP shutdown is awaited before Windows fixture cleanup.

Browser preparation errors are reported separately from application assertions.
The fixture reduction is also covered by the six responsive-layout-default
regressions. Linux/macOS/Windows GitHub verification is reported on the PR.

Motion bindings intentionally require unambiguous semantic selectors and real
API values. Incoming regeneration TSX remains explicitly caller-supplied until
grounded in a design export. Unsupported/ambiguous merges never silently discard
application logic. SQLite proof uses disposable snapshots; PostgreSQL migration
SQL is an artifact for review and execution, not a claim of a live DB migration.
Process-bound behavior cursors/coverage receipts must be regenerated after a
server restart. Broad or incomplete evidence blocks completion.

All application commands and browser observations are executor evidence. The
server independently rechecks source hashes, plan/report bindings, screenshots,
schema/snapshot hashes and supported SQL results. It does not launch browsers,
run project commands or modify a live ChondroGen database.
