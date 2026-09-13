# Notice layout divergence

Base: `9c27de0301ff5eb9dd9f746bd9d05f5c9e102154`, measured on 2026-09-13.

## Localisation before implementation

Read `line-box-displacement.md` and `about-vertical-geometry.md` before
investigation. Built the unmodified base in this worktree's own target,
acquired notice with that binary, and rendered it with the unchanged harness.
The fresh baseline reproduces the dispatch's values exactly:

| Screen | Design height | Baseline rendered height | Baseline divergence |
| --- | ---: | ---: | ---: |
| notice-422-6914 | 1215 | 1217 | 5.54% |
| notice-422-7088 | 1142 | 1214 | 3.35% |
| notice-422-6865 | 1142 | 1145 | 2.22% |

`bands.mjs` localises tablet's worst band to y=900–1000 (9.46%).
`drift.mjs` finds the preceding y=700–900 content at approximately +3px;
the footer does not match through a uniform translation. The footer crop
shows extra address lines and a wrapped SWING EZ label. `boxes.mjs` and
the collected snapshot identify the exact source of the page's +72px:

* Address text `I422:7136;147:985`: Figma **397×115**, browser **176×184**.
  It gains **69px**, exactly three of its already-correct 23px line advances.
* Its company-details column `I422:7136;147:983`: 143px becomes 212px.
* Footer `422:7136`: design y=879/h=263, browser y=882/h=332.
  The 69px internal growth plus the existing upstream 3px gives +72px.
* Navigation sibling `I422:7136;147:986`: Figma allocates **95px** of the
  footer's 872px content width, but CSS gives it **316px**. Its four HUG
  children total 256px, with three 30px gaps: **346px** of content. Every
  individual child is narrower than 95px, so the existing single-child
  overflow test misses the row's collective overflow.

A browser-only probe changes just that navigation's `min-width` to zero.
The address returns to 115px, the company column to 143px, and the page to
**1145px**. No font, line advance, weight, border, or Korean wrapping policy
changes. This rules out recurrence of the three previously fixed causes for
the 69px growth. The remaining upstream 3px is not corrected here.

## Mobile is independent

Mobile's worst band is y=900–1000 (14.64%). The crop shows two missing
social icons and a displaced theme toggle, not a vertically growing footer.
The social row `I422:6963;265:2564` has a collected 82×32 layout box and an
82×36 export beginning at y=-2 relative to that box. Codegen correctly emits
`maskSize="82px 36px"` and `maskPos="0px -2px"`, but omits the HUG width.
`boxes.mjs` measures the mask as **0×32** and its parent as 80×32 instead
of 162×32. Centering that narrower parent moves the toggle from x=99 to x=140.

The width is lost because `folded_mask_dimensions` excludes every node with
an export offset. Child folding removes the intrinsic width, and CSS masks
provide no intrinsic layout size. `elements.mjs` reports the 100×95 search
illustration correctly; it does not enumerate mask images, so the zero-width
mask finding comes from DOM boxes and the crop rather than that script.

A separate browser-only probe restores the collected 82px mask width, keeping
the generated module unchanged. The mask becomes 82×32, its parent becomes
162×32 at x=99, and the toggle returns to x=99; page height stays 1217px.
`w16-mask-probe.log` records the intervention. This is a localisation probe,
not a generator candidate or a claimed mobile divergence improvement.

The tablet navigation correction cannot affect this mobile mask. A scope
question was sent to the coordinator because the dispatch requires mobile
improvement but permits a mobile fix only through the same change.

## Tests and measurement records

The main checkout's ignored `harness/render/out/` holds `w16-*` logs,
baseline PNGs in `w16-baseline/`, and the browser-only probe. Harness scripts
were not edited. The initial unfiltered render correctly refused other
groups acquired by a different binary; subsequent runs acquired and rendered
each group separately with the explicit worktree binary.

Baseline binary SHA-256:
`f657817d22e026ea81cad892f46c903c68ba5517c62a69e0058ab8c86fc16dea`.

The tablet tests first recorded **2 passed / 2 failed** in
`w16-fill-red.log`: missing `minW="0"` for collective overflow and missing
source mapping for that derived minimum. After implementation all four pass.
The other tests preserve explicit minima and exclude fitting rows, wrapping
rows, vertical columns, hidden children and absolutely positioned children.

All 268 plugin goldens remain unchanged in the tablet-only candidate;
`compat_fixtures` and `compat_manifest` pass. No manifest checksum changes.

Fresh non-notice baseline measurements:

| Group | Baseline divergence, narrow to wide |
| --- | --- |
| about | 7.46 / 4.06 / 2.41% |
| landing | 4.99 / 2.47 / 1.50% |
| popup | 3.64 / 2.06 / 0.85% |
| grid | 2.96% |
| keyframes | 6.71% |
| report | 1.28% |

## Tablet-only candidate

Candidate binary SHA-256:
`009214264ff45012c331c04f4257f2c8a62cc3ce494b02a406560e5734c1e3e1`.

The change extends the existing FILL minimum reset to non-wrapping horizontal
rows whose visible, in-flow child widths plus gaps exceed their inner width.
It keeps explicit minimum widths. The new `minW="0"` mapping names the
source's `layoutSizingHorizontal`, not a `minWidth` field that does not exist.
No frame ID, viewport width, breakpoint, or capture-specific constant enters
the condition. The existing 0.5px geometry comparison tolerance is reused.

| Notice width | Baseline | Candidate | Fresh repeated candidate | Candidate height |
| --- | ---: | ---: | ---: | ---: |
| 360 | 5.54% | 5.54% | 5.54% | 1217 |
| 992 | 3.35% | 3.26% | 3.26% | 1145 |
| 1920 | 2.22% | 2.22% | 2.22% | 1145 |

Every non-notice group was freshly acquired and rendered with the candidate.
All 14 other actual PNGs, including mobile and desktop notice, are
**byte-identical** to their respective fresh baseline PNGs. Thus no unchanged
percentage conceals a smaller regression. The non-notice table above applies
equally to the candidate. Only the tablet notice threshold is lowered, to 3.26,
after the second fresh acquire/render confirms it.

The mobile improvement acceptance condition is **not met** by this candidate.
Its independent folded-mask width defect is identified above and is untouched
under the original same-change-only scope. The coordinator scope question
received no reply during the work, so no expansion was assumed. This is not
a claim that every acceptance criterion has passed.

The documented negative is specific: the mobile mask is HUG horizontally,
whereas the corrected mechanism allocates a FILL child of a horizontal flex
parent. It cannot restore this mask's erased child-based intrinsic width.
The next cycle should address the export-offset exclusion in
`folded_mask_dimensions`, proving layout dimensions separately from the
export's larger painted bounds and preserving the existing mask offset.
Removing that guard indiscriminately would also admit FILL/ratio cases whose
pixel offsets need a separate scaling proof. Neither Korean line breaking
nor the three already-fixed text/stroke causes explains the missing mask.

## Verification

| Required gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1063 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | 1063 passed, 0 failed, 2 ignored; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | 2 passed, 0 failed |

The UI-only run separately passed 327 tests. All commands use
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`,
`CARGO_INCREMENTAL=0`, and two build jobs (`CARGO_BUILD_JOBS=2` for the
insta wrapper). `CARGO_TARGET_DIR` was never set. Ordinary MSVC build/test
linking prints its existing localized library-creation warning; the clippy
invocation itself is clean. Logs are `w16-workspace.log`, `w16-insta.log`,
`w16-clippy.log`, and `w16-smoke.log` in the main harness's ignored `out/`.

The workspace test build changed executable identity without production
source changes. Its binary was preserved as
`target/debug/devup-mcp-w16-final.exe`, SHA-256
`06707befc5571f73030459e86493bf5ae959fac84149f1b79f85a1b46f823c92`,
and final measurements were reacquired from that exact immutable copy.
The later isolated smoke build cannot overwrite the measured copy.
All 15 final actual PNGs match the first candidate, and all final reference
PNGs match baseline. Final notice was acquired and rendered twice, again
confirming 5.54 / 3.26 / 2.22% and 1217 / 1145 / 1145px. The hashes are recorded
in [notice-layout-evidence.json](notice-layout-evidence.json).

No plugin golden, manifest checksum, forbidden Rust file, harness script,
Korean `wordBreak` rule, or corpus consistency assertion changed.

The change is committed locally without pushing. Build output is cleaned
after the commit as requested; acquisition and diagnostic browser processes
were closed, and no worktree devup-mcp process is retained.
