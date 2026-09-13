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

## Second fix: offset masks retain their HUG layout size

The follow-up explicitly authorizes the independent mobile defect. This work
starts on `d4b2056427ab`, preserving the tablet commit without amending or
squashing it. The scope limitation in the earlier sections records the first
dispatch's history; it does not apply to this authorized second fix.

The mechanism was independently rechecked using a fresh build of the HEAD
production sources, fresh acquisition, the generated TSX, collected snapshot,
and `boxes.mjs`. The baseline mask is **0×32** at x=220, its parent **80×32**
at x=140, and the toggle x=140. The collected node is **82×32**, with horizontal
HUG and vertical FIXED sizing. Its exported painted bounds are **82×36**,
offset **0,-2**. The generated opening has `h="32px"`, `maskSize="82px 36px"`
and `maskPos="0px -2px"`, but no width. Folding removes the children that
would supply an intrinsic HUG width; the unconditional `export_offset`
exclusion prevents restoring it. The baseline PNG visibly lacks both social
icons. The baseline production binary was preserved before implementation;
only regression-test additions were present during its build/acquisition.

The fix separates layout geometry from painted export geometry. An offset
mask can restore measured HUG axes when both axes are HUG/FIXED and it stays
in normal flow. Existing `maskSize` and `maskPos` continue to describe the
export. FILL axes, rotation, explicit absolute positioning and placement by a
free-layout parent remain excluded: their scaling or positioning needs a
different proof. There is no viewport, breakpoint, node ID or capture-specific
constant in the production condition. The earlier tablet `minW` logic and
Korean `wordBreak="keep-all"` are unchanged.

The same defect exists in the wider notice social masks
`I422:7136;148:1436` and `I422:6913;148:1436`: both are HUG/FIXED 82×32 with
82×36 painted bounds at y=-2. Their width restoration is the same data-driven
operation, not a separate tablet or desktop path.

### Regression and provenance evidence

Before implementation, `w16-mask-red.log` records **19 passed / 3 failed**.
The failures are missing HUG layout dimensions, missing restored-width source
mapping, and missing bounding-box fallback dimensions. The tests use a
different **137×29** layout and **143×33** export at **-3,-2**, exercising
HUG/FIXED, FIXED/HUG and HUG/HUG combinations rather than copying the notice
capture's values. Negative cases cover both FILL axes, rotation, explicit
absolute placement, free-layout placement and missing layout measurements.
All **22** folded-asset tests subsequently pass.

The existing shared restoration predicate also drives provenance, so the new
width maps to the measured width, HUG sizing and folded children through
`restored-hug-after-mask-child-folding`. Bounding-box fallback retains the
existing fallback resolution. A test replaces the correct 137px width with
the export's 143px width and verifies fidelity rejects it. Mobile's actual
layout coverage changes from **118/122 to 119/122**, removing exactly
`I422:6963;265:2564#width` from `uncoveredLayout`. Its other three uncovered
layout properties remain reported; the screen is still lossy overall.

All **268 plugin goldens** pass byte parity unchanged, and the corpus manifest
and coverage registry pass. No golden, manifest checksum, corpus assertion,
forbidden Rust file or `harness/render/scripts/` file changes.

### Fresh measurements and full gate for the second fix

| Screen | HEAD baseline | Repeated baseline | Mask candidate | Post-test final binary | Rendered height |
| --- | ---: | ---: | ---: | ---: | ---: |
| notice-422-6914 | 5.54% | 5.54% | 5.25% | 5.25% | 1217 |
| notice-422-7088 | 3.26% | 3.26% | 3.22% | 3.22% | 1145 |
| notice-422-6865 | 2.22% | 2.22% | 2.20% | 2.20% | 1145 |

The exact mobile changed ratio is **0.05541838134430727 →
0.05254229538180155**, a decrease of approximately **0.288 percentage points**.
Restoring the mask does not change page height. The first candidate's other
12 actual PNGs are byte-identical to the fresh HEAD baseline, and all 15
reference hashes match. Their percentages remain about **7.46 / 4.06 / 2.41**,
landing **4.99 / 2.47 / 1.50**, popup **3.64 / 2.06 / 0.85**, grid **2.96**,
keyframes **6.71**, and report **1.28**.

| Required gate | Second-fix result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1067 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | 1067 passed, 0 failed, 2 ignored; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | 2 passed, 0 failed |

All gates run in this worktree's own target with debug information and
incremental compilation disabled and two build jobs, including
`CARGO_BUILD_JOBS=2` for insta. `CARGO_TARGET_DIR` is never set. The ordinary
MSVC link steps retain their existing localized library-creation warning;
the Clippy command itself reports zero warnings. Logs use the
`w16-mask-{red,green,fmt,clippy,workspace,insta,smoke}.log` names in the main
checkout's ignored harness `out/` directory. Measurement logs and preserved
reports/PNGs use `w16-mask-baseline`, `w16-mask-candidate` and `w16-mask-final`.

The preserved binaries have distinct SHA-256 identities:

* HEAD production baseline: `1c9dda74c8503ab40364751bc58a1db422819a91c267cd3aa40380431aa9c76d`.
* First mask candidate: `516476ac9fad7b511e62811517d2a1bfbd66145e96c2e11bbad844710ba4d253`.
* Post-test final: `28d4c01357b324d7521d18a145ac232871ed1c9853036d9f28f6f36963fe0d06`.

Every measurement is freshly acquired with the exact binary passed through
`DEVUP_MCP_BIN`. The post-test executable is copied after the final smoke
gate, so no subsequent test build can replace its measured bytes. No binary
identity check is bypassed, and the harness scripts remain unmodified.

Final repeated acquisition confirms **5.25 / 3.22 / 2.20%** again. All 15 final
actual PNGs, exact ratios and heights match the first candidate; all reference
hashes match baseline. The final DOM measures the mask **82×32** at x=179,
its parent **162×32** at x=99, and the toggle x=99. The social icons are visible
again. Only the three measured notice thresholds are lowered, after repetition,
to **5.25 / 3.22 / 2.20**. The `maskFix` section of
[notice-layout-evidence.json](notice-layout-evidence.json) preserves the exact
ratios, repeated notice ratios, actual/reference hashes and binary identities.

The second fix is committed separately on top of `d4b2056`, without pushing.
Build output is cleaned after that commit, and the acquisition/diagnostic
processes started for this task are closed.
