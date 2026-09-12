# W7 page scaffold

Base commit: `c662c4f4acb2fc579e0121cd727b17478c501e07`.

## Request and emitted files

```json
{
  "outputs": ["pageScaffold"],
  "componentName": "Proofread",
  "pageScaffold": { "route": "/proofread" },
  "outputPaths": { "pageScaffold": "C:/allowed/project" }
}
```

This is a preview, including when other `outputPaths` or asset requests are present. It writes no files or directories. Set `pageScaffold.write: true` to commit the file set; writing additionally requires the scaffold directory key. An omitted directory is allowed for preview and resolves against the first allowlisted root.

The manifest contains path, content, and encoding for each file:

- `src/app/proofread/page.tsx` contains exactly the page shell below.
- `components/pages/proofread/Proofread.tsx` contains the existing generated screen with `export function Proofread()` and no default export.
- Available captured assets go below `public` using the existing manifest paths, such as `public/icons/<existing-name>.svg` or `public/images/<existing-name>.png`. Binary manifest content is base64; code content is UTF-8.

```tsx
import { Proofread } from "../../../components/pages/proofread/Proofread";

export default function ProofreadPage() {
  return <Proofread />;
}
```

The route comes only from the caller, including `/` for the root page; nested routes determine the relative import depth. If the generated screen name already ends in Page, the shell adds ShellPage to avoid an import/function name collision. No client directive or multi-component split is inferred.

For one selected Section frame, the file-set key is `frame:<nodeId>:pageScaffold` and the response is `frames[].pageScaffold`. Multiple selected frames are refused for one route. `outputPathResults.supportedKeys` lists the usable key, and `pathKinds` identifies it as `directory`; committed `outputPaths` groups the two code writes under that requested key. Unsupported or unavailable keys have diagnostics. Existing file targets and TSX/TS/JSX/JS/JSON file-shaped targets produce `DEVUP_OUTPUT_PATH_EXPECTED_DIRECTORY` diagnostics without staging writes.

`assetPublicRoot` and `assetRequests[].outputPath` remain the mapping mechanism. The scaffold supplies default placement only for captured bytes using existing `AssetManifestEntry.path`; existing reconciliation deduplicates verified identical content and rewrites generated URLs before constructing the scaffold. No SVG classification, color detection, or asset naming algorithm was added. The asset test uses a custom public mapping and proves that the emitted URLs, manifest bytes, and actual written asset agree.

Resource delivery includes the complete scaffold manifest as JSON, removes its inline body, and makes subsequent resource comparison requests previews.

## Collision and rollback policy

The default response policy is `refuse-existing`. Scaffold transactions opt into `OutputTransaction::refuse_existing`; `OutputTransaction::new()` still defaults to the original replacement behavior for all existing callers. The existing allowlisted-root resolver checks every planned target before staging, including traversal, other-drive/UNC, ADS, and symlink/junction protections.

No-clobber publication uses a same-directory staging file and atomic hard-link creation, so a file created after the preflight check cannot be overwritten. Errors include the colliding/publication path. A filesystem without hard-link support fails with the existing typed transaction error; there is no non-atomic fallback. Unsupported network filesystems were not available in this worktree and were not directly tested.

Explicit `pageScaffold.overwrite: true` uses the existing backup-and-replace transaction. Both publication modes reuse the existing reverse-order rollback and recovery-backup reporting. Tests inject failure after the second replacement and verify both originals and restoration order, and inject a concurrent second-file collision and verify that the first publication is rolled back while the concurrent original survives. Existing staging-directory cleanup semantics are unchanged.

## Deliberately unresolved

- `client-boundary`: application state, events, browser APIs, and ownership are absent from Figma. A human or validator must choose client boundaries.
- `component-decomposition`: further boundaries and reusable component extraction require application context. Only one screen and its page shell are generated.
- `unavailableAssets` separately identifies discovered assets whose bytes have not been successfully collected. Their collection is not guessed or synthesized.

## Ownership and review

Original ownership: `projection.rs`, new sibling `page_scaffold.rs`, the export input in `tools.rs`, the export method body in `mod.rs`, and new tests. No other router regions or prohibited modules were edited.

The coordinator explicitly approved three necessary supporting surfaces:

- `operation.rs`: one additive typed option on `PendingOperation::Export`, needed to retain the request through collection/jobs.
- `validation.rs`: the new output constant and design/Section capability predicates, needed to advertise and accept the output without widening other artifact capabilities.
- `output.rs`: default-off no-clobber publication in the existing transaction and its tests, needed to prevent a check-then-write race while preserving existing overwrite behavior. Existing capability reads, guards, backups, rollback, and recovery reporting remain unchanged.

The coordinator directly reviewed preview suppression, route ownership, no-clobber default compatibility, hard-link publication, and rollback and approved proceeding. Follow-up directory diagnostics were added with failing tests first. Existing stdio schema tests caught the new strict options schema emitting a boolean schema; object-form `not: {}` now preserves strict parsing and MCP schema compatibility.

## Verification

Initial RED evidence and exact failure names are in `w7-page-scaffold-red.txt`. All builds use this worktree's own `target`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`, and two build jobs; `CARGO_TARGET_DIR` is not set.

Final checks on this worktree:

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --workspace --all-targets -j 2 -- -D warnings` | Pass; zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 903 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | 903 passed, 0 failed, 2 ignored; no snapshots to review |
| `cargo test -p devup-mcp --test stdio_smoke -j 2` | 2 passed, 0 failed |

There are 17 new tests: 12 public-tool integration tests and 5 projection/transaction tests. The coordinator corrected the task's initially inferred baseline of 890 to 886 for base commit c662c4f; the measured final accounting is 886 existing plus 17 new = 903. The first broad run also caught two existing stdio schema tests failing on the newly introduced boolean schema, and both pass after the object-schema fix.

Some test builds emitted MSVC localized linker informational-output warnings about creation of import libraries; the final Clippy run has zero warning lines. No snapshot files changed. The coordinator-reviewed Section continuation now requires choosing one candidate for the supplied route and translates the directory key to its frame key, rather than offering a multi-screen batch for one route.

Build artifacts are cleaned with `cargo clean` after committing; the cleanup result accompanies worker completion. No push is performed.
