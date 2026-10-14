# Nested asset export feedback: implementation and evidence

Feedback 003, 004 and item 3 of 005 were read as UTF-8 from the supplied feedback directory. Work is based on `18d113f`; the live installed server and bridge host identify themselves as `b7018cd967ef`. No design nodes were changed, cloned, detached, hidden or substituted.

## Live findings

The reported repeated export failure did **not** recur with the attached Prime file in this session. The original cause remains **unproven**. In particular, neither timeout, fonts, vector complexity, masks nor boolean operations can be named as the cause from these measurements. Existing repository documentation discusses Figma's out-of-view runtime failure, but that is prior evidence, not a reproduction of this incident.

The released server did reproduce the nested URL rejection: `node-id=I44%3A8533%3B10%3A973` returned `DEVUP_FIGMA_UNSUPPORTED_FILE`, `Figma node-id format is invalid.` The newly compiled local server accepted that exact URL and the raw asset request ID `I44:8533;10:973:node`, and exported the real logo successfully through the existing local bridge relay with direct credentials disabled.

| Format | Released build, parent URL | New server, nested URL | SHA-256 in both captures |
| --- | --- | --- | --- |
| SVG | 29,799 bytes, asset call 38 ms | 29,799 bytes, asset call 32 ms | `a1a7b554cff70a76781545065a0d933f78fa85abef4360ac5a4e32f84b849d4a` |
| PNG, scale 2 | 3,027 bytes, asset call 16 ms | 3,027 bytes | `2c6df7c87c35587020402cf324c4cebebc73f10425b2c8096703f95981e62b1a` |
| PDF | 23,434 bytes, asset call 35 ms | 23,434 bytes | `8d8f51e6a28d390dae076880d545978de1e34042c5e3ed233d4582cbe94ab30b` |

The actual before/after SVG files both parse as XML, have an SVG root and one nonempty path containing the logo geometry. The PNG and PDF have their expected binary signatures. Local binaries are under `fixtures/local-tools/prime-before.*` and `prime-after.*` (ignored captures, intentionally not committed as a design fixture). No replacement logo or placeholder was created. The existing Arrow asset `I38:6538;30:1937;30:843:node` exported as 448 bytes with hash `90bf095b1fd8a11f81c106b166381e20d7f268e49c4e08b47839504ff1dc1b7f`; its enclosing screen remained partial because other assets were not requested.

Exact structured responses and file hash checks are retained in [before evidence](feedback-assets-before-evidence.json) and [after evidence](feedback-assets-after-evidence.json). The new binary was compiled from the modified working tree on `18d113f`; that is why its build identity in the captured response names the base commit. The attached plugin still served the older scripts during these measurements: new fallback execution against the actual Prime document is not proven by these native-export successes. Reopen the rebuilt plugin after integration to check its live diagnostics and async identity acquisition. Reproducing the historical Figma exception remains dependent on the runtime exhibiting it again.

## Behavior and boundaries

- URL query values decode once. Strict numeric IDs and `I<id>;<id>[;<id>...]` chains accept colon or dash separators; emitted links encode semicolons. Malformed chains and double encoding are rejected.
- Nested lookup can walk the enclosing instance when direct descendant lookup fails. It returns only the exact requested ID, with a bounded traversal.
- Native export remains first. For SVG failures or a hung native renderer, a reserved portion of the total budget permits read-only reconstruction from the same subtree's actual `fillGeometry` and supported centered `strokeGeometry`. It preserves solid paint colors and opacity, winding rules, node opacity, child stacking and relative transforms, and accounts for render extents. A late native result cannot write or overwrite the selected fallback.
- No parent crops or bounding-box drawing are used. Masks, clipping, gradients, effects, text, unresolved paints, non-center/dashed strokes and ambiguous root transforms are explicitly refused by geometry recovery. They remain supported by native export. [Figma's Plugin API](https://developers.figma.com/docs/plugins/api/BooleanOperationNode/) says stroke geometry is centered regardless of stroke alignment, so using it for inside/outside strokes would change the image.
- `errorDetail` preserves the real exception, stage, elapsed milliseconds, node ID and type. `exportDiagnostics` preserves native failure evidence even on successful recovery and gives measured lookup/native/fallback/encode durations. Export fragments use the same rendering helper and retain original diagnostics and fragment-read errors.
- Failed manifests omit `path` and `outputPath`. Deterministic failures have `retryable: false`; explicit budget timeouts have `retryable: true`. Figma's own ten-second exception remains a renderer failure with its original text and corrective hint, rather than being mislabeled as our configurable timeout.
- `DEVUP_ASSET_TIMEOUT_MS` defaults to `90000`, accepts `1000` through `600000`, and falls back to the default for invalid settings. Server, bridge host, relay and export fragment calls leave transport time for the script's diagnostic. Polling includes `assetTimeoutMs`, per-call `timeoutMs` and current elapsed time for running stages. Both bridge host and plugin need updating for the complete behavior; an older host can retain its old limit. Changing this budget does not alter Figma's internal timeouts.
- At coordinator request, both snapshot paths additionally collect `mainComponentId`, `mainComponentName` and `mainComponentSetId` via the async instance API. Missing components return null; getter exceptions remain in `fieldErrors.mainComponentId`. Repacking reuses the lookup.

## Verification

The new Rust integration tests execute the actual compiled JavaScript with Node, so existing CI's `cargo insta test --workspace --all-features --check` includes them. They cover nested ID roundtrips, actual SVG geometry and XML parsing, color/opacity/holes/transforms, exact-subtree isolation, native mask export, unsupported fallback styles, real renderer messages, hung renderers, late native results, fragment recovery, and async component identity failures. Server regressions check truthful manifests, retryability and running progress.

Passed: the complete `devup-mcp-figma` suite; subsequent affected asset/collector/bridge/upstream suites after fragment diagnostic changes; `cargo test -p devup-mcp --lib nested_asset_failure`; `cargo test -p devup-mcp --lib asset_jobs`; `cargo test -p devup-mcp --test composite_export --test stdio_smoke` (20 composite and two stdio tests); `cargo clippy -p devup-mcp-figma -p devup-mcp --all-targets --all-features -- -D warnings`; plugin build, generated-script verification and typecheck; Rust formatting and whitespace checks. The existing virtual-clock pause/resume/cancellation tests advance beyond the advertised call timeout instead of hardcoding 91 seconds, and still verify that accepted asset bytes survive the timeout and client cancellation.

The broader server library run returned **408 passed, one failed**: existing `server::visual_compare::tests::symlink_escape_refused_before_read` cannot create its fixture on this Windows account (`Os` error 1314, missing symlink privilege). No change was made to that unrelated test or to machine permissions. No push, merge or release was performed.
