# Original image bytes over the bridge; isolated-fill correction remains unmeasured

Base: `4a0e434b4448f4c6c28105e609c34cd3fb06e258`. This change implements the
read-only original-upload transport and a runnable capture entry point. It does
not implement or claim a pixel correction to `about-422-3376`.

## Implemented representation and transport

`AssetRequest::original_image(node_id, fill_index, image_hash)` creates a distinct
`$original-image/fills/N` request with an `original-image-v1` asset identity.
Pass it to `ReadToolCall::asset_export`, then decode the response with
`original_image_from_result`. The bridge sends the existing statically bundled
`assets` script with an explicit `transport: "bridge"` parameter. It never sends
caller-supplied JavaScript.

The script validates the selected IMAGE paint's hash before calling
`figma.getImageByHash(hash).getBytesAsync()` and `getSizeAsync()`. It creates no
nodes, removes no nodes, and never calls `node.exportAsync` on this route. A
hidden image before the selected image does not change the selection.

The response has kind `devupOriginalImage`, representation `original-image-v1`,
original MIME, intrinsic dimensions, byte count, SHA-256 and base64 bytes. Its
format and scale are null: the shared request envelope's PNG/scale placeholders
do **not** convert an original JPEG into a node-sized PNG. The separate Rust
decoder checks representation, file/request/version identity, positive intrinsic
dimensions, byte length, SHA-256 and the PNG/JPEG/GIF/WebP signature. Signature
checking is not a full image decode; dimensions are those reported by the Plugin
API. The original bytes are not re-encoded. The existing 8 MiB byte cap applies.

The remote request has no bridge opt-in and returns the named diagnostic
`DEVUP_ORIGINAL_IMAGE_REQUIRES_BRIDGE` before reading image bytes. The remote MCP
has a truncated text envelope and no general original-byte attachment transport;
this route does not attempt to hide base64 in that envelope or substitute a
whole-node export. An older plugin returns `DEVUP_ASSET_FIELD_UNSUPPORTED`, which
the decoder retains as a named failure. Missing image, unsupported codec, source
hash change, size limit and Plugin API read failure also retain distinct codes.

The ordinary node-export path is unchanged. No generated image URL, manifest
discovery policy, crop rule, codegen mapping, sourceMap resolution, Korean
`keep-all`, threshold or plugin golden changes. No new derived CSS mapping is
claimed. The raw response is deliberately separate from `AssetManifestEntry`.
The MCP `devup_figma_export` asset-selection surface still requests renditions;
the additive raw API and capture example are the entry points in this change.

## Tests first and verified boundary

Logs live under this worktree's ignored `harness/render/out/`.

* `w25-source-red.log`: four script tests failed before implementation, including
  wrong `failed` status and `DEVUP_ASSET_FIELD_UNSUPPORTED` in place of the
  required original-byte and remote-refusal responses.
* `w25-rust-red.log`: the bridge marker test failed with null instead of `bridge`.
* `w25-decoder-red.log`: the new decoder/constructor API was absent. This is a
  missing-API compile failure, separately recorded from the behavioral RED tests.
* `w25-source-read-error-red.log`: a source read failure incorrectly returned the
  generic node-export failure before its diagnostic was corrected.
* The final script contract covers source selection, unchanged original bytes,
  intrinsic codec/dimensions, remote refusal, stale hash, missing image, empty and
  oversized results, unsupported codec and read exceptions.
* The Rust decoder tests reject altered image hash, version, SHA-256, byte count,
  MIME, zero dimensions and representation. A real local WebSocket integration
  test round-trips **1,100,000 bytes** and compares every reconstructed byte.
  The fake plugin in that test does not establish live Figma execution.

## Own baseline, repeated

Production was built from the unmodified base before changing any production
source. The build label includes `-dirty` because the first untracked regression
test was present during compilation. The preserved own executable is
`harness/render/out/w25-baseline.exe`, SHA-256
`1c52c63d38bbb5c3dc99b33950a40dc23daddbf20620907d2c07a44f5f41b510`.

The unchanged harness ran in this worktree with a copied existing call bank and
asset inventory, avoiding shared generated inputs. `DEVUP_MCP_BIN` selected the
preserved executable. Acquisition completed for all three about screens and two
render runs reproduced the exact metrics, using theme `67d6de70e679`.

| Screen | Own baseline, both runs | Changed pixels |
| --- | ---: | ---: |
| about-422-3376 | 5.431783302639656% | 141,574 |
| about-422-3180 | 2.9337565804958983% | 163,529 |
| about-422-2987 | 1.768271319459043% | 161,504 |

The deliberate Korean allowance remains excluded from defect accounting:
5.4317833026 - approximately 0.21 = **5.2217833026pp**. No wrap experiment was
performed. The existing one-pixel excess page heights on the wider screens
remain 5,620 versus 5,619 and 4,758 versus 4,757.

The own candidate was rebuilt, preserved as `harness/render/out/w25-candidate.exe`
and freshly reacquired before measurement. Its SHA-256 is
`9d36bb8cc8608f7d2ed041fe34e293868c780f9702ce14358aaddb0aa362b14c`.
Both candidate render runs are environment-valid and reproduce every exact
baseline metric above: **zero improvement and zero regression on about**.
The final actual PNGs are byte-identical to the final baseline PNGs, and all
acquisition input hashes agree. Exact run metrics and hashes are retained in
[bridge-original-image-evidence.json](bridge-original-image-evidence.json).
The other eleven screens were not measured by this bounded transport task;
no corpus-wide visual improvement or regression result is claimed.

## What specifically remains unobserved

No live original bytes were obtained. The process on port 1993 is an older
connected server; another server cannot share its in-memory bridge. A plugin is
attached, but it executes the old static bundle. Rebuilding a server or setting
`DEVUP_MCP_BIN` alone cannot update that running plugin. There is no arbitrary
script tool on the connected server. The coordinator confirmed these facts and
directed delivery of this tested route and bounded negative, with live capture
and pixel correction following a human plugin reload.

This is a deployment/reload boundary, **not authentication**. The token diagnosis
from the previous report is not reused. Searching the copied call bank found no
`getImageByHash`, `devupOriginalImage` or `original-image-v1` response to reuse.
The previously established whole-node PNGs remain unsuitable source bytes.

Even after capture, original uploads alone do not satisfy
[the isolated-fill contract](isolated-fill-export-contract.md). A host renderer
must still apply the selected paint's local box, FILL/FIT/CROP/TILE transform,
rotation, visibility, opacity and supported filters exactly once, retaining
transparent margins and excluding other paints, strokes, effects and children.
It must carry a new isolated-rendition identity through discovery, delivery,
cache URLs and provenance, retain folded-node asset semantics, refuse unsupported
composition explicitly, and validate the heroes and eleven existing CROP assets.
None of that missing renderer is disguised as source retrieval here.

## Exact next capture

1. Build the plugin: `cd plugin`, `npm install`, `npm run build`. The committed
   `plugin/dist/code.js` already contains this source route.
2. In Figma Desktop choose **Plugins → Development → Import plugin from
   manifest**, select this checkout's `plugin/manifest.json`, close the old
   running Devup Bridge plugin, and run the newly imported **Devup Bridge** on
   file `f1AJyo27afkkr6U9PhnWSu`. Keep its window open. The menu path is the
   repository's installation instruction; it was not exercised during this run.
3. Free port 1993 by stopping its current server through its owning client, then
   run the capture below. Alternatively a different port must match all three
   settings in `plugin/manifest.json`, `plugin/src/code.ts` and
   `DEVUP_FIGMA_BRIDGE_PORT`, followed by rebuilding and reloading the plugin.

From the repository root, with the required build environment:

```powershell
$env:CARGO_PROFILE_DEV_DEBUG='0'
$env:CARGO_PROFILE_TEST_DEBUG='0'
$env:CARGO_INCREMENTAL='0'
cargo run --locked -p devup-mcp-figma --example original_image_probe -j 2 -- f1AJyo27afkkr6U9PhnWSu 422:3378 0 09939a0d306a1dc967f5f04afa6756203ca92dec about-hero.original
```

The probe waits at most 60 seconds for the plugin, validates the response, writes
the exact bytes to a new file (refusing overwrite), and prints capture metadata.
It does not fall back to remote. The selected source is fill 0 of `422:3378`;
`422:3183` and `422:2989` share that hash. Its output explicitly records version
null: the Plugin API read does not establish a file-version assertion. Recollect
paint metadata before implementing the renderer if the design has changed.

## Final gate

All Cargo work used this worktree's own target, `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0` and two jobs (including
`CARGO_BUILD_JOBS=2` for Insta). `CARGO_TARGET_DIR` was never set.

| Gate | Observed result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1,122 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | 1,122 passed, 0 failed, 2 ignored; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | 2 passed, 0 failed |
| `npm install` and `npm run build` in `plugin/` | Pass; committed dist rebuilt |
| Repeated plugin build | Identical SHA-256 `dc7f8717807e0b095ded30073226257448d33b88fd8ffd435751a8561f7b5a31` |

Gate logs are `w25-fmt.log`, `w25-clippy-final.log`, `w25-workspace.log`,
`w25-insta.log`, `w25-smoke.log` and `w25-plugin-build.log` under the local
harness `out/`. Ordinary MSVC linking emits its existing localized
library-creation warning; Clippy itself has zero warnings. All 268 plugin
goldens and their manifest are unchanged, and the corpus consistency test
passes. No off-limits source file, harness script, shared codegen file or
provenance file was edited.

The change is one local commit without a push, followed by `cargo clean`.
Acquisition and rendering children exited; the pre-existing server on port
1993 was not stopped. The remaining deliverable is live original capture after
plugin reload, followed by an independently validated isolated-fill correction.
