# R5: size evidence and cumulative asset collection

Only a child-bearing SVG mask whose projection removes a HUG sizing basis receives a size repair. WQUW-119 node `3997:46317` is FILL/HUG: it emits `w="100%" aspectRatio="320 / 48"`, preserving 160×24, 320×48 and 640×96 proportions across host widths. FILL is never replaced with measured pixels. Both-HUG masks receive measured anchors; FIXED axes retain the existing layout policy. Ordinary blocks, Image elements, and childless shapes do not trigger the repair. Embedded roots continue to delegate sizing to the host.

Source maps link the derived property to width, height, HUG sizing and childrenIds with `resolution="restored-hug-after-mask-child-folding"`; bounding-box fallback is explicitly named in the resolution. Ratios retain the full precision of the source dimensions. A ratio must also have an independent FILL anchor: an auto-height or shrink-to-fit/HUG parent cannot establish a cyclic percentage size. Rotated exports and pixel-offset masks are excluded from this repair because their transforms need separate proof.

Fidelity checks asset axes before canvas/HUG/internal-layout exemptions. It checks values and the pair of axes, rather than the existence of a source-map entry. Missing, zero, incorrect and unanchored sizes produce output-scoped `DEVUP_CODEGEN_LAYOUT_UNCOVERED`; they cannot yield exact/final TSX. Empty HUG layout leaves are checked too. Resource checks compare token/style identities and asset paths; flattened resources must belong to an actual enclosing asset. Text comparisons reject extra content.

These are generated-evidence checks, not a claim that browser pixels were compared. The complete pinned corpus retains its existing TSX except for these two justified mask cases:

- `upstream-codegen-193-bd488a5710`, node `171:1553`: HUG/HUG frame loses children `171:1559`, `171:1554` to the `recommend.svg` mask; restore its measured 19.61×34.03 box.
- `upstream-codegen-194-17922977f6`, node `171:1561`: HUG/HUG frame loses children `171:1565`, `171:1563` to the same mask representation; restore its measured 19.61×34.03 box.

The separate WQUW-151 source-map snapshot removes incorrect parent/sibling and token-prefix attribution; its generated TSX is unchanged.

## Combine batches

Save the final responses from each recommended 1–3 asset batch. Then run:

```powershell
devup-mcp --merge-asset-batches batch1.json batch2.json batch3.json
```

The command reads local JSON and prints a cumulative JSON summary. It accepts raw export responses, MCP structuredContent/text envelopes, and saved poll records containing `response`. It requires export source/discovery evidence; pending jobs and bare manifests lack sufficient scope evidence.

- `collectedCount`: distinct asset IDs with positive byte lengths and SHA-256 evidence.
- `unrequestedCount`: discovered assets not collected by any supplied batch. A batch-level `capture-not-in-artifact` is not an export failure.
- `failedCount`, `pendingCount`, `excludedCount`, `conflictCount`: separate unresolved states. Different successful hashes/lengths for one asset remain conflicts, independent of input order.
- `reportedWrittenCount`: historical file-write reports; the command does not re-read asset files or verify their present contents.
- `scopeRootIds`: union of supplied scopes, not the entire Figma file. Incomplete discovery prevents a complete summary.
- Different files or known versions are rejected. Missing source versions are disclosed by `source.versionVerified=false`; collected bytes do not prove that separate captures share one Figma revision.

Repeated batches are idempotent for asset counts. Successful collection in another batch satisfies earlier unrequested/failed entries. Conflicting captures remain visible. This summary never merges or upgrades TSX projection quality.

## Release lockfile consistency

CI checks `cargo metadata --locked` before its cache step or any build can silently repair Cargo.lock. On the main-branch version update, an action-scoped Git adapter runs `cargo update --workspace` and stages Cargo.lock before changepacks/action's automatic version commit. It also refuses a stale version-branch push. Other Git commands, superseded-run guards, changepack-required checks, draft releases and Linux bundle assembly retain their existing behavior.

The integration test uses a real temporary Git/Cargo workspace, verifies that a patch bump fails locked metadata before synchronization, and inspects the automatic commit to prove all workspace packages were updated without external dependency churn. Its next-patch calculation also works after future releases. This repository hygiene change needs no separate user-facing changepack; the R5 product changes have a separate entry from R4.
