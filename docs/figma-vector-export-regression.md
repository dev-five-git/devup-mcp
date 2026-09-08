# Loading logo export regression

Reproduced on 2026-09-08 from `origin/main` at
`1e8b0b6de9a6c10d59b2b27c282575922cc675d8` in an independent worktree.
No AGENTS.md was present in the repository or its applicable ancestor directories.

Target: `https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/?node-id=4279-7810`.
`devup_figma_explore` selected `3831:10708` and `3831:10723`.
Exports requested `tsx`, `devupJson`, `sourceMap`, `rawSnapshot`, and
`assetManifest`, with `refresh: true`.

## Confirmed causes

- Each screen's first upstream envelope contained nine nodes and a cursor
  `{offset: 0, nextOffset: 9, totalNodes: 15, complete: false}`.
  `accept_fast_multi_root` stored that page without following the cursor.
  The six remaining nodes per screen were the Boolean logo's operands.
  Neither vector filtering nor serialization nor related-explore reuse caused
  this loss: the snapshot script walks every child, and the unread continuation
  pages restore all twelve nodes. The same failure occurred in a fresh process.
- Asset discovery and TSX generation treated `BOOLEAN_OPERATION` as an ordinary
  container requiring all its children. With absent operands, both rejected the
  logo as an asset and rendered its solid fill over its rectangular bounds.
- Fidelity only counted collected nodes and discovered assets. The rectangle
  accounted for a collected node, while the missing operands and undiscovered
  logo asset were outside its denominator.

## Fix and quality semantics

The Section collector follows each root's cursor independently, validates its
range, removes internal markers, and merges resources from every page. Boolean
results are SVG assets, using their own paint rather than operand paint.
Figma's `exportAsync` supplies the geometry; no paths are inferred or drawn by hand.

First-page legacy fallback remains supported. A failed continuation aborts the
collection with its original error instead of restarting at offset zero over
already accepted pages and potentially pending large-value reads. The caller can
retry a fresh acquisition; no mixed or incomplete result is cached as complete.

`acquisition` measures snapshot/resource completeness. `projection` measures how
the acquired design is represented in TSX; `exact` is not a pixel comparison or
proof that referenced asset bytes have been downloaded. Missing visible children
without an asset representation now lower node coverage, add lossy impacts, and
appear in bounded `fidelity.uncoveredNodeIds`. An SVG can represent missing
operands, but the raw snapshot audit still reports those operands as missing.
Asset byte delivery is reported separately by each manifest entry's status.

## Live verification

Installed MCP 0.2.1 and a separately built baseline executable both reproduced
the bug. A separately built modified executable ran over stdio in this worktree;
the installed MCP processes and configuration were not replaced. Its process
had `DEVUP_FIGMA_CALL_CACHE` unset and URL requests used `refresh: true`.
The final release executable was copied to this worktree's ignored
`fixtures/local-tools/fixed.exe`, reporting `devup-mcp 0.2.1 (1e8b0b6de9a6-dirty)`.
Its SHA-256 is `10886d9efdb439ab52ae722b2a32671c911b6c26acc0d59021d3822d1aa1e135`;
the version string alone does not distinguish an installed build from this source build.

| Measurement | Before | After |
| --- | --- | --- |
| Status / acquisition | partial / partial | complete / complete |
| Preserved / reachable nodes | 18 / 18 | 30 / 30 |
| Declared / exported children | 28 / 16 | 28 / 28 |
| Missing children | 12 | 0 |
| Manifest entries | 2 image fills | 2 image fills + 2 SVG logos |
| Logo TSX | solid Box | Box with original SVG maskImage |
| Per-frame asset coverage | 1 / 1, logo absent | 2 / 2, logo included |

Explicit SVG asset requests for `3831:10710:node` and `3831:10725:node`
both returned `exported`: 606 bytes, viewBox `0 0 64 34`, SHA-256
`ace6e87aecd15a5237d36630135e297783ce17eec4e2f31b0de93a043ac7824f`.
The actual files, manifest hashes, TSX references, and sourceMap byte ranges
were checked together. The theme remained byte-equivalent as parsed JSON.
Reusing the artifact with the same `frameIds` made zero Figma calls and returned
the same complete snapshot and TSX. Section selection must also be specified on
artifact reuse; omitting it requests screen selection rather than screen export.
Large responses, binaries, and exported assets remain ignored local artifacts.

Regression coverage includes two interleaved screen continuations, resource
merging, invalid cursors, failed continuations with pending large values,
the small captured incomplete-logo fixture, uncovered
visible children, and hidden descendants. Cargo.lock only synchronizes the four
workspace package versions with the existing 0.2.1 manifests.

Validation commands passed:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- `cargo test --workspace --all-features` — 476 passed, 2 opt-in live tests ignored
- `cargo insta test --workspace --all-features --check` — no snapshots to review
- `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs` — 4 passed
- `cargo build --workspace --release`

The initial final-check attempt ran out of disk space. The complete check sequence
above was rerun successfully after space became available. Windows' linker emitted
informational library/export-file messages; clippy with warnings denied passed.
