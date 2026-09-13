# Isolated image fills: contract and the remaining validation boundary

Investigated on `937313e39448db7f7c05dbb0ab206432261b2203`. The independent
native-bridge delivery correction is described in
[bridge-asset-delivery.md](bridge-asset-delivery.md). It changes transport only;
it does **not** turn a whole-node PNG into an isolated image fill.

## Required representation

An isolated-fill asset must explicitly identify a **node-local rectangular
rendition of one IMAGE paint**, distinct from both original uploaded image
bytes and a rendered node. This is the appropriate contract for the existing
background-layer generator:

1. Identify the node, original fill index and image hash, file/version,
   representation revision, format and scale. Validate the selected paint
   before exporting. A different paint/representation cannot reuse an old
   whole-node cache entry merely because the image hash matches.
   The delivered asset identity/URL or cache inventory must also distinguish
   the revision: acquisition deliberately reuses existing paths already listed
   in its asset inventory, so changing script bytes alone cannot replace them.
2. Export exactly the selected image's pixels over transparency in the node's
   local width/height box. Include its crop/scale mode, transform, rotation,
   image filters, paint visibility and paint opacity once. Exclude other
   fills, strokes, effects, node opacity, corner clipping, child contents and
   ancestor composition. Those remain the responsibility of live CSS/layout.
3. Apply CROP in the renderer once; the consumer displays the resulting box
   without reapplying `imageTransform`. Do not substitute original bytes under
   this representation. FILL/FIT also require the local box and transparent
   margins to be retained. TILE requires an explicit decision about a rendered
   box versus a repeatable tile; it must not silently inherit a raw-image rule.
4. Preserve original paint ordering and unique fill-index URLs. Opacity belongs
   to the isolated paint; blending that depends on other layers belongs to the
   composition. An unsupported blend/filter/shape must produce an honest
   limitation, not a successful claim of isolated parity.
5. Produce the requested encoding, with actual MIME, byte length and SHA-256.
   PNG transparency is necessary for a reusable isolated layer. JPEG cannot
   silently flatten transparency onto a background not named by the contract.
   Transport fragments must reconstruct the **same selected rendition** with
   the same scale and identity; re-exporting the whole node is invalid.
6. Keep folded node assets on their existing node-render contract. Discovery
   must carry this distinction through request resolution, collection,
   continuation, manifest, source map and delivery. `fills/N` alone currently
   identifies both folded image assets and live background layers. A new
   derived mapping should describe a rendered single paint and explicitly
   disclaim original source bytes and pixel-perfect browser composition.

The alternative is an explicitly tagged **raw-image** contract. It needs the
original codec/MIME and intrinsic dimensions, plus a renderer for the paint
transform, filters, opacity and transparent crop bounds. In particular, raw
JPEG bytes are not a PNG at the requested node scale. Removing the established
CROP exception while leaving old node-export assets in the cache would repeat
the W18 regression. No raw-image substitution is shipped here.

## Why current exports fail this contract

`assets.js` reads the selected fill only to validate its image hash, then calls
`node.exportAsync(settings)`. Neither the fill index nor the selected paint is
passed to that renderer. Two different IMAGE-fill requests on the same node,
using the same format/scale, therefore invoke precisely the same renderer with
precisely the same settings. Their filenames and source claims differ; the
rendered content cannot be isolated by those differences.

This is not fixed by `contentsOnly`: that option excludes overlapping outside
layers, not the node's children or its other fills. `useAbsoluteBounds` controls
bounds, not paint selection. The installed official `@figma/plugin-typings`
`ExportSettingsImage` exposes no fill-selector option.

`large_value.js` compounds the boundary: `$export:png@scale` and `$export:svg`
identify only a node rendition. They do not carry the selected fill index or
image hash. Replacing only the initial export would let a large isolated asset
announce one hash and retrieve whole-node fragments with another, which the
assembler correctly rejects.

## Collected crop and multi-paint cases

All three hero nodes have the same source image hash
`09939a0d306a1dc967f5f04afa6756203ca92dec`, FILL mode, an identity transform,
image opacity 1, a later white SOLID paint at approximately 0.7 opacity, and a
live child subtree. Their dimensions differ, but the defective composition
condition does not depend on width. The PNG already includes the white paint
and live children; the generator adds both again.

The mobile portraits are a different multi-paint case: their active CROP image
is fill **2**, above a SOLID plate at fill 1, with a hidden image at fill 0.
For `422:3551`, the active transform's vertical scale is
`0.8508567214012146`, with translation `0.036508746445178986`. The existing
300 by 400 export has already applied this crop. Selecting the first image by
type would pick the hidden, wrong image. Reapplying the active transform would
stretch the correct crop again. Both mistakes are ruled out as remedies.

The reproducible probe [hero-export-contract-probe.py](hero-export-contract-probe.py)
walks each acquired frame, matches manifest asset IDs to its acquisition hash
inventory, and compares exported pixels at the collected bounds against the
freshly acquired reference. It never replaces text or rewrites render inputs.
Exact results and measurement identities are in
[isolated-fill-export-evidence.json](isolated-fill-export-evidence.json).

| Screen | Own base, both runs | Changed pixels | Rendered height |
| --- | ---: | ---: | ---: |
| about 360 | 5.4317833026% | 141,574 | 7,240 |
| about 992 | 2.9337565805% | 163,529 | 5,620 |
| about 1920 | 1.7682713195% | 161,504 | 4,758 |

The mobile design/capture height is exactly 7,240. The wider reference PNGs
are 5,619 and 4,757 pixels high; the harness captures those viewport sizes and
separately reports the unchanged, one-pixel-taller rendered elements. These
pre-existing values are not evidence of a new vertical correction.

| Validation | Result |
| --- | --- |
| Existing node-box CROP assets versus reference | All 11 have **0** changed pixels at channel tolerance 24. |
| Existing hero exports versus the entire reference hero | All 3 have **0** changed pixels, including children and white overlay; this violates isolation. |
| Generated hero region versus reference | 9,217 / 13,058 / 12,922 changed pixels at 360 / 992 / 1920. |
| Two different selected image fills on one node | Both call the same `node.exportAsync` with identical settings; the current renderer cannot select either paint. |
| An isolated replacement against crop and multi-paint references | **Not validated**: no original or isolated bytes were obtained. |

The mobile hero region accounts for 0.3536295273 percentage points of the
screen. That is a localization/oracle result, **not** a promised production
improvement: live text can still differ after correct background isolation.
After subtracting the deliberate Korean allowance, the measured mobile
residual is 5.4317833026 - approximately 0.21 = **5.2217833026pp**.

The baseline executable is this worktree's own build of unchanged production
at `937313e`, SHA-256
`21f41954f76d4fe1fbe867191156fc8cc574879885062f792bdd830a95af8cf8`.
Its `-dirty` build label records the new, untracked test/probe files written
while the initial build was running; no production source had changed.
Acquisition ran the unchanged harness in an isolated local copy with the
existing call bank and exported-asset cache. A first acquisition without the
complete export cache was invalid and was stopped; it is not a baseline.
The accepted acquisition binds its own generated modules, snapshots, themes,
references and asset hashes to the preserved base binary. No threshold value
was used as a before measurement, and no harness script was edited.

## What must change before a pixel correction can land

The source hash names the image but does not supply original bytes or intrinsic
dimensions in the collected snapshot. A flattened composite loses the image
behind opaque children. There is no invertible CSS correction that can recover
those pixels, so the zero-difference whole-node oracle is not a valid fix.

The Figma connector's `download_assets` request for `422:3378` returned
`UNAUTHORIZED`: "This app connection requires reauthentication before other
actions on this app can succeed." No live source image or isolated rendition
was obtained through that route. Cached whole-node assets can confirm the
defect and existing crop behavior, but cannot certify replacement pixels.

The read-only Plugin API path has `getImageByHash(hash).getBytesAsync()` for
original bytes and `exportAsync()` for node renditions; it has no selected-paint
node export. A temporary rectangle with one paint could provide a rendition,
but it creates/removes document nodes and must not be smuggled into scripts
advertised as read-only. A host-side rasterizer of raw bytes preserves that
boundary but must first establish the crop/filter/composition contract above.

Before shipping: obtain authenticated raw/isolated pixels; introduce explicit
representation identity through initial and chunked export; validate all three
heroes and all eleven already-correct CROP portraits; verify two active IMAGE
paints, hidden earlier paints, semi-transparent image paint over a solid,
filters, transformed crop, FIT padding, and cleanup/failure behavior if a
temporary-node route is deliberately adopted. Repeat generator acquisition
and rendering with the new asset identity, and review any changed corpus
goldens. This investigation does not claim those replacement tests passed.

Korean `keep-all` is untouched. Its accepted approximately 0.21 percentage-point
allowance is excluded from the mobile defect accounting.
