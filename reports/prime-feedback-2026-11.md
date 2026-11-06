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
