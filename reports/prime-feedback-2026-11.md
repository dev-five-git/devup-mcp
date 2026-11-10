# Prime feedback audit

Reviewed the application's `.feedback/001` through `031` against main
`0c6073e` (MCP 0.20.0). The records mix generator defects, integration mistakes
and resolved upstream API/release problems. This change does not claim to fix
all three categories by changing the generator.

| Feedback | Disposition in this change |
| --- | --- |
| 001 | Completion requires separately observed functional `e2e` command evidence and reruns Figma artifact visual comparisons. Missing/inconclusive comparisons cannot complete the application. |
| 002 | Delivery rules require checking/resetting native heading margins after semantic conversion. No claim of an upstream compiler fix. |
| 003 | Selected FILL roots retain width even with a fixed opposite axis and absent maximum. Missing parent allocation remains explicit. |
| 004 | Delivery rules preserve caller width/alignment through shared control wrappers. |
| 005 | Backdrop/blend/isolation review is required; the existing image policy is retained. This PR does not add a general blend compositor. |
| 006 | Keep source node/paint identity and collision refusal; do not choose the first ambiguous filename. Existing registry behavior is retained. |
| 007 | Validate theme tokens against the final assembly, including independently exported instances. Existing token evidence is retained. |
| 008 | Preserve the distinction between hidden masters and visible instances. Existing visibility/boolean override handling is retained. |
| 009 | The browser regression runs the installed Devup UI compiler and a production build. The reported polymorphic Link transform is an external compiler compatibility issue, not repaired here. |
| 010 | Retain existing bounded resource delivery/chunk recovery; the assembly checklist calls for hashes and bounded reads. |
| 011 | Keep acquisition, public projection and actual file writes distinct; preserve blocked-write evidence and collision guards. |
| 012 | Completion no longer promotes static runtime checks without native Figma visual comparisons. Geometry/source checks remain separate from pixels. |
| 013 | Retain generated-CSS reload diagnosis and production verification guidance. This PR does not change a Vite watcher. |
| 014 | Require bounds, focus and ARIA checks before/after validation state changes; a present input alone is insufficient. |
| 015 | List intentional backend-dependent design exceptions separately; prohibit pretend working controls or success state. |
| 016 | Remove the blanket Korean keep-all instruction. Source line breaks and measured wrapping take precedence. |
| 017 | Diagnosis preflight supplies debug, TSX, sourceMap and devupJson in one retry while explicitly preserving the original target/options. |
| 018 | `pageScaffold` emits the actual generated body directly into `src/app/**/page.tsx`, with no whole-screen component/proxy. Correct all three workflow entry points and contradictory examples. |
| 019 | Every application request, including server reads/actions/uploads, must use devup-api. Remove the direct-fetch example; record concrete gaps and stop affected work while continuing independent work. |
| 020 | Resolved upstream devup-api generator issue; no duplicate MCP transport workaround. |
| 021 | Require DCU update, reinstall, resolved-version verification and a minimal reproduction before declaring an API capability missing. |
| 022 | Require exact CI runtime, mock restoration, exit/coverage checks and remote CI verification. |
| 023 | Upstream release configuration issue; require actual package-local archive/path verification. |
| 024 | Upstream release retry issue; distinguish current source from an old pinned release source. |
| 025 | Require clean release fixtures and checking actual archive identity/dependencies; stale tarballs or dry runs are insufficient. |
| 026 | Resolution record for published devup-api and the application's migration; no new MCP defect. |
| 027 | Retain detached FILL width; expose unapplied reference root sizing as an approximated placement contract. Preserve instance HUG cross-axis alignment. Consumers still must apply the typed caller contract or inline the instance. |
| 028 | Measured native absolute rectangles retain both axes, including shapes with omitted layoutSizing fields. Read errors and fluid/stretch/scale constraints do not receive this repair; matching full-parent overlay axes retain their previous percentage relationship. |
| 029 | Explicitly prohibit hiding painted empty absolute nodes by a broad cleanup heuristic. The reported destructive postprocessor belongs to the application, not this MCP source. |
| 030 | Every workflow requires completing an authorized fix and rerunning functional/Figma visual checks; recording a diagnosis is not completion. |
| 031 | Another active project is not a desktop-wide lock. Use OS split-screen and separate windows/ports/browser contexts; coordinate only contended input, respect actual tool locks and preserve capture viewport/DPR/zoom. Ship this in bootstrap instructions, the guide and Bridge/frontend skills. |

## Regression evidence and limits

- `prime_placement_feedback.rs` covers detached and mixed-axis FILL roots,
  missing parent contracts, unapplied instance allocation, fixed rectangle
  dimensions, failed reads and fluid constraints. The initial three regressions
  failed on unmodified main before the fixes.
- The browser regression compiles production code and measures the assembled
  420px title / 40px gap / 520px card, 94px title height and 16px marker through
  resizing. It compares marker paint to a cropped native Figma reference and
  deliberately changes its colour to prove that the same comparison fails.
  The reference manifest records the original Figma section, original PNG hash,
  exact crop and crop hash. It is not a whole-page Prime visual baseline.
- Completion tests exercise missing visual evidence, missing renderer fields,
  changed PNG bytes, substituted local baselines, passing native comparisons,
  current source binding and revocation after source/evidence changes. Synthetic
  unit PNGs test the gate; they are not presented as real Figma design evidence.
- Page scaffold integration tests cover preview/write behavior and asset paths
  with the route-owned body. Skill digests and all frontend entry points are
  checked so the delivered binary installs the corrected instructions.

Full-page blend fidelity, framework watcher behavior, final application token
assembly and downstream API/release publication still require their own exact
application/runtime evidence. Passing this PR does not assert that every
screen or external package has been fixed.

The earlier ChondroGen generator default that infers keep-all from Korean text
is still present. Item 016 changes the workflow instruction here; replacing
that generator policy needs a separate source-line/native-reference regression
and must preserve explicit user/theme policies. It is not claimed resolved by
the marker crop or the page ownership changes in this PR.

The user confirmed that split-screen desktop work is possible. Item 031 is a
shipped instruction change; this update does not claim a new live GUI
concurrency experiment. Existing skill-install/digest and bootstrap/guide
delivery checks verify that the rule reaches clients. The 1,500-byte bootstrap
instruction limit is retained.

## Follow-up (2026-11-10)

This follow-up implements the remaining MCP-scope items after PR #119 and the
user's binding decision on Korean wrapping. It supersedes the 002 and 016 rows
above where they differ.

| Feedback | Change in this follow-up |
| --- | --- |
| 002 | Semantic `h1`–`h6` conversion now emits `my="0px"` unless the node already has a margin (`m`/`my`/`mt`/`mb`), with a verified semantic-stage property derivation. The browser regression compiles the installed Devup UI without a reset stylesheet and measures a generated `h2` margin of 0px. |
| 006 | Extra image fills are named `{stem}--fill-{index}` (0-based, as in `{nodeId}:fills:{index}`). Every stem is kebab-case with collapsed hyphens, so this suffix cannot collide with the `-2`/`-3` name of a different asset (previously fill 2 and the second same-named asset were both `stem-2.png`). The one-placeholder/two-pictures refusal now lists the conflicting assets, nodes, image hashes and canonical paths; it recommends `assetNamesPerNode:true` only when the hashes differ. For a shared imageHash (named deliberately as one file, with or without that flag) it points to separate exports or sourceMap nodeId/assetId binding. Existing collision guards and the shared-hash naming test are unchanged. |
| 007 | Every `devupJson` response carries `devupJsonScope` (`complete-theme`, `used-tokens` or `additions-only`). Missing-only adds `devupJsonMerge` (target file, deep-merge/add-missing-keys strategy, instruction). A missing-only `devupJson` output path named `devup.json` is refused before any Figma call because it would replace the project theme. |
| 008 | `componentSetVariants` reports per-variant `visible` and `allVariantsHidden`. When every variant of an exported COMPONENT_SET (or a single master) is hidden, the response adds `DEVUP_HIDDEN_MASTER_EXPORT` with the hidden variant ids, visible instances found in the snapshot and a next action: export the visible page INSTANCE with its overrides. |
| 011 | Asset job rows report `fileFacts` (exists, bytes, sha256 read from disk) for each projected path, `outputPathRedirected` when the file lives at a canonical path rather than the requested one, and a `fileStateReason` for every `not-written` row: missing after a blocked or rolled-back write, different bytes kept by the collision guard, quarantine, failed transaction, no acquisition or a duplicate. Grayscale reuse of a colour original is reported as written and redirected, not as not-written. |
| 014 | Guidance only: validation messages must not take a native input's width (flex-basis/minW contract, message on its own row); E2E must trigger the error and assert input bounds, visibility, focus and message. |
| 016 | Decision: Korean keeps word boundaries. Generated Korean text keeps `wordBreak="keep-all"` and `overflowWrap="break-word"` and adds `textWrap="pretty"` against large trailing gaps. Text with `whiteSpace` `pre`/`nowrap` gets no `textWrap`, because the shorthand resets `text-wrap-mode` and would re-enable wrapping. Explicit local/theme `textWrap` wins. The shipped guidance now states this policy: a diff caused only by Figma's mid-word Korean break is a documented policy difference, and the native Figma reference is never overwritten to pass. The `korean-word-boundary` validation rule's next action matches. |
| 032 | Guidance only: report CI failures by actual stage with the original message. Keep local pass, remote CI pass, push/permission rejection and transient external HTTP errors (for example setup-bun 504) as separate evidence. On a rejected push, finish independent work, leave an applicable patch and state the constraint. |
| axe-core 4.14 | Guidance only: a minor bump promoted `label-content-name-mismatch` (WCAG 2.5.3) to wcag21a. Drop redundant `aria-label`s that repeat visible text (including `aria-hidden` split-letter copies), make remaining labels contain the visible text verbatim and use sr-only text for extra hints. Fix components; never disable the rule. |

Evidence: Rust regressions cover the textWrap policy (including pre/nowrap and
explicit policies), heading margins with verified derivations, fill/dedupe path
uniqueness, both ambiguous-placeholder recoveries, theme scope markers and the
devup.json write refusal, hidden-master warnings (unit and full projection) and
per-path asset facts/reasons including grayscale reuse. Seven pinned plugin
goldens add exactly fifteen `textWrap="pretty"` lines; the 151 frame snapshots
add only `textWrap` lines and their property evidence. The harness browser
regression compiles with the installed Devup UI and checks computed
`word-break: keep-all`, `text-wrap-style: pretty`, a `nowrap` mode for
intrinsic-width text, line ends on word boundaries and a 0px `h2` margin.

Out of scope, unchanged: 013 (Vite watcher/HMR behavior) and 029 (the
application's destructive postprocessor). These belong to other components or
the application and still need their own evidence.

### 005 and 009 resolved (2026-11-10)

| Item | Resolution |
|---|---|
| 005 | Fixed in devup-mcp codegen. The devup-ui investigation (main 675f7e20) showed Devup UI emits `mix-blend-mode`, `filter`, `z-index`, `isolation`, `position` and `transform` faithfully: Chromium screenshots are byte-identical to the plain-CSS equivalent. The defect was where codegen opened the stacking context. In the SEO-GEO contact banner (instance 32:1929, decorations I32:1929;32:1876 and I32:1929;32:1877) the transparent content wrapper inside the gradient owner received `pos="relative" zIndex="0"`, so the `multiply` decorations with `zIndex="-1"` composited against a transparent backdrop and drew white blobs (center pixel 224,224,224 instead of 77,44,208). A blended negative-z child of a wrapper that paints nothing and opens no context of its own (no fill, stroke, effect, opacity, blend, rotation or own negative z-index) now has its stacking context on the nearest ancestor that paints the backdrop, provided nothing in that ancestor is drawn before it. The wrapper keeps `pos="relative"` for the decoration's coordinates. Unblended negative-z children, wrappers with their own fill and earlier painted siblings keep the existing containment. Rust regressions cover the lift and all three fallbacks; the harness compiles the fixture with the installed Devup UI and samples the decoration pixel (multiplied into the purple gradient), with a control that moves the context back to the wrapper and must show the light blob. |
| 009 | No MCP change. The polymorphic Link transform was a Devup UI compiler bug, fixed in `@devup-ui/wasm` 1.0.82 (devup-ui PR #681). |
