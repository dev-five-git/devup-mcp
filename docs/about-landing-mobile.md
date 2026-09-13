# About and landing mobile: spaces at explicit line breaks

Investigated from unmodified `da04103e03149662a9c2a08a27f05990048d605f`
in the W22 worktree. One shared correction improves both mobile screens:
preserve source spaces adjacent to explicit line breaks in text whose inline
width is fixed and whose height is automatic. No viewport, breakpoint, node
ID or screen-specific constant participates. Korean `keep-all` is unchanged.

## Localisation before Rust changes

All six preceding reports were read. Both screens were freshly acquired with
this worktree's preserved base executable, SHA-256
`cc73158e929ff6ee5bcd98231377dc31ac02472614b18d4d46f08ad0ec1b872f`.
Two complete baseline render runs reproduce every exact metric. The local
harness uses unchanged tracked scripts, its own generated inputs and a copied
call bank/export cache; the visual comparator is the existing unchanged
executable copied from main. Cargo builds and gates use this worktree's target.

`bands.mjs`, `drift.mjs`, `boxes.mjs`, `elements.mjs` and `crop.mjs` ran on
both mobile screens before production changed. About's worst bands are
y=700–800 (30.33%) and 2200–2300 (24.77%); landing's are y=1500–1600
(15.18%) and 900–1000 (13.60%). Their common feature is **text-region
differences inside correctly positioned sections**, without an accumulating
page displacement. About's paragraph bands sometimes prefer a one-pixel
offset, but intervening sections return to zero; the extreme offset matches
in dense paragraphs are not evidence of a corresponding page translation.
Landing's main-content bands prefer zero vertical offset.

The portraits still have their collected 300x400 boxes, and the earlier CROP
correction remains present. Page heights remain 7240 and 2955. The documented
`boxes.mjs` viewport-height limit is not substituted for full-capture height.

The live outside-stroke lead is ruled out for these captures: about has 34
descendant nodes with nonempty stroke arrays, landing 13, and **neither has
a painted OUTSIDE stroke**. Some empty-stroke nodes carry OUTSIDE alignment;
those do not paint an edge. Rounded, dashed and multi-edge OUTSIDE exclusions
therefore cannot explain these two mobile residuals.

## Shared source condition and rejected broader probes

Examples from the collected data:

* Landing `833:3725`, at (16,1520), 328x84: its English paragraph contains
  `theme typing, \n`. CSS normal removes the line-end space during layout.
* Landing `833:3665`, at (16,940), 328x42: the benchmark caption has a space
  before its explicit newline.
* About `422:3478`, at (20,2176), 320x145: its centered paragraph contains
  spaces before explicit newlines, in addition to a Unicode line separator.
* About `422:3490` and `422:3496` have the same HEIGHT sizing and explicit
  line-break whitespace. Their measured boxes match the collected geometry.

The existing JSX renderer already retains the source characters and emits
`br` for design line breaks. A JSX string expression does not prevent CSS
normal from collapsing whitespace. The correction is a CSS whitespace policy,
not a character rewrite, font change, new line advance or Korean-wrap change.

The investigation deliberately rejected two broader variants before shipping:

| Browser probe | Outcome and reason rejected |
| --- | --- |
| `pre-wrap` for every text with collapsible spaces | Improves both mobile targets but worsens landing tablet/desktop by 232 pixels each; also changes a single-line HUG table label with a trailing space. |
| `pre-wrap` for spaces adjacent to hard breaks, including HUG text | Improves about at all widths and landing mobile, but worsens popup by 2,039 / 2,457 / 3,722 pixels. |
| Same hard-break condition, `textAutoResize=HEIGHT` only | Improves both target mobiles and all other affected screens; leaves HUG text unchanged. This is the shipped condition. |

The HUG failure is measured geometry, not just a percentage: popup mobile
text `422:5694` grows from 259.25px to 263.3125px and its centered group moves
left. Tablet's equivalent grows 321.84375px to 326.890625px; desktop grows
357.609375px to 363.203125px. `pre-wrap` includes the trailing space in
intrinsic width. Figma's collected text widths are 258 / 320 / 356px, so
applying that CSS policy to HUG text needs a separate sizing treatment.
Restricting the correction to Figma's HEIGHT text fixes the inline width by
source semantics, independently of the viewport. No fitted width is emitted.

The separate kerning-disabled probe worsens about mobile by 92 pixels and
landing mobile by 2,138 pixels. No kerning or glyph-position workaround is
introduced. The remaining text residual is not claimed to be fully explained.

## Implementation, tests and provenance

`preserves_hard_break_spaces` requires a TEXT node with
`textAutoResize=HEIGHT`, a source ASCII space adjacent to CR/LF/U+2028/U+2029,
no positive `maxLines`, no tab characters and no list segments. Characters
come from the node, or from concatenated styled segments when the node field
is absent. Such text emits `whiteSpace="pre-wrap"`; unsupported cases retain
their existing policy and CSS-collapse diagnostic.

The new `derived-hard-break-whitespace` resolution maps the property to the
actual character source. Its description explicitly disclaims HUG sizing,
glyph parity and identical wrapping, and says Korean keep-all is unchanged.
The source-stage derivation records characters, segments, resize mode and
line constraints. The source-map descriptor distinguishes this character-
derived attribute from a `children` mapping.

Before implementation, `w22-whitespace-red.log` records **1 passed / 3 failed**:
missing preserving CSS, missing character provenance, and missing segment-
boundary provenance. Earlier exploratory HUG expectations were discarded
with the rejected HUG hypothesis; their RED log is retained separately.
No pre-existing assertion was relaxed to permit HUG changes.

Four final regression tests cover different fixed inline widths, all supported
line separators, segment-boundary spaces, character/segment provenance,
unchanged Korean keep-all, and exclusion of HUG/fixed-height/missing resize,
clamps, lists, tabs and unrelated whitespace. The existing text sweep still
asserts the original **35-case** whitespace population: 30 now have preserving
CSS and derived mappings, five still report collapse. Every character
round-trip assertion remains. WQUW asserts the same three corrected node IDs
through preserving properties rather than obsolete loss diagnostics, while
its unrelated FILL loss and strict incompatibility remain asserted. The
server placement test likewise checks the new style and the 40 remaining
losses instead of the previous 41.

## Fresh generator measurement

The candidate is preserved as `harness/render/out/w22-candidate.exe` before
test builds can overwrite the ordinary executable. All 15 screens were
reacquired with that binary; no acquisition identity check was bypassed.
Its SHA-256 is
`84b35d603eaa672cb8a5f5208c7a2a1e7020f1dbd937cfce8e5a53d084363dfa`.

| Screen | Own baseline, both runs | Candidate, repeated | Pixels removed |
| --- | ---: | ---: | ---: |
| about 360 | 5.5144260282% | **5.4317833026%** | 2,154 |
| about 992 | 3.0121556183% | **2.9337565805%** | 4,370 |
| about 1920 | 1.7731544741% | **1.7682713195%** | 446 |
| landing 360 | 4.9873096447% | **4.7830419252%** | 2,173 |
| notice 360 | 3.8431641518% | **3.7368541381%** | 465 |

About mobile improves **0.0826427256pp**, landing mobile **0.2042677195pp**.
About's intentional Korean allowance is subtracted from the accounting:
5.514426% - approximately 0.21pp = **5.304426%** baseline residual;
5.431783% - approximately 0.21pp = **5.221783%** candidate residual.
This uses the owner's accepted allowance, not a new keep-all experiment.

The other ten actual PNGs are byte-identical to baseline: landing tablet and
desktop, notice tablet and desktop, all popup widths, grid, keyframes and
report. All reference hashes, theme hashes and rendered dimensions remain
unchanged. Their results remain 2.466289/1.503763, 2.118083/1.159424,
3.644231/2.062607/0.853540, 2.963988, 2.685547 and 1.276467 percent.
This session's landing baseline has one more changed pixel per width than
the preceding cycle; both own baseline runs agree, and no before value is
taken from thresholds.

The second complete candidate run rejected popup mobile for
`ERR_NO_BUFFER_SPACE`; it is retained as environment-invalid. A separate
popup repeat subsequently passed and reproduced all three exact metrics,
rather than counting that rejection as a measurement. All other candidate
metrics match exactly between runs. Only about mobile/tablet, landing mobile
and notice mobile thresholds move downward, to 5.43 / 2.93 / 4.78 / 3.74.
The small about-desktop improvement does not change its rounded 1.77 threshold.

## Separate quantified finding: about hero exports include children

This is an independent composition defect, left as a bounded finding rather
than folded into the whitespace correction. Mobile `422:3378` has an IMAGE
fill and a 70% white SOLID fill, with a live child subtree containing its
illustration and headings. Its generated background uses
`/images/Section1-422-3378.png` plus a white overlay. That PNG already
contains **the complete rendered node, including overlay and child text**.
The generated child text is painted over a faded copy of itself; the full-
size hero crop visibly shows the duplicate glyphs.

The asset manifest identifies the file as `422:3378:fills:0`, and its hash
is included in the fresh acquisition. This is not an untracked decorative
image assumption. `crates/devup-mcp-figma/src/scripts/assets.js` verifies the
requested fill's image hash, then calls `node.exportAsync(settings)`; it
does not return the original image bytes.

| About hero | Current region changed pixels | Node export against reference region | Region pp of screen |
| --- | ---: | ---: | ---: |
| 360, `422:3378`, 360x520 at (0,46) | 9,217 | **0** | 0.3536295273 |
| 992, `422:3183`, 992x400 at (0,64) | 13,058 | **0** | 0.2342642188 |
| 1920, `422:2989`, 1920x400 at (0,64) | 12,922 | **0** | 0.1414800995 |

The table is a pixel oracle for the export's contents, **not a shipped
screenshot substitution**. Replacing the section with that flattened export
would remove live text and layout semantics. The CSS generator cannot recover
the original background pixels behind already-composited children from this
PNG. A distinct isolated-fill/raw-image export contract still needs implementation
and validation, including crop semantics and multi-paint composition. This is
the technical blocker, not a permission request: the standing authorization
already permits a general fix proven from the collected data. The current
measurement proves the duplicated composition but does not validate a raw-fill
replacement or quantify a production fix. Simply removing
the white layer, reusing the CROP exception or drawing the node export behind
the same children does not solve that representation boundary. No transport
or asset-folding change is included here.

The second defect is therefore not ready to land as its own fix commit. The
quantified finding remains available for that work; the zero-difference export
oracle must not be presented as an implemented improvement.

## Corpus and final gate

The four approved plugin golden updates are individually reviewed in
[about-landing-mobile-golden-review.md](about-landing-mobile-golden-review.md).
They contain only six preserving-whitespace attributes and four checksum
updates; no corpus assertion is weakened. The linked approval rationale records
why this fixes painted characters, following W8, while the Korean keep-all
compensation remains intentionally unchanged.
Non-plugin WQUW snapshots add only the affected preserving attributes, their
three derived mappings and glossary entry, and remove the three now-inaccurate
collapse diagnostics.

[about-landing-mobile-evidence.json](about-landing-mobile-evidence.json)
retains exact baseline/candidate metrics, binary and PNG hashes, all rejected
and accepted probes, reviewed corpus diffs and the hero export comparison.
Raw logs and crops live in this worktree's ignored `harness/render/out/w22-*`
paths. No tracked harness script or off-limits source file is changed.

All Cargo commands use `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0` and two jobs;
`CARGO_TARGET_DIR` is never set. Current verification is:

| Required gate | Final post-approval result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1,084 passed / 0 failed / 2 ignored |
| `cargo insta test --workspace --all-features --check` | 1,084 passed / 0 failed / 2 ignored; no pending snapshots |
| `cargo test --locked -p devup-mcp --test stdio_smoke` | 2 passed / 0 failed |

The full 268-case corpus parity and its unchanged consistency test pass.
Exactly the four approved goldens changed, with six attributes and only their
four manifest checksums; no fifth golden appeared. The earlier approval wait
is resolved by the continuation instruction. All gates ran from this worktree's
own target, with two jobs including `CARGO_BUILD_JOBS=2` for commands lacking
an explicit jobs flag. Ordinary MSVC builds emit the existing localized
library-creation linker warning; Clippy reports zero warnings.

## Post-approval reacquisition and repeated measurement

After the gates, `cargo build --locked -p devup-mcp -j 2` rebuilt the own
candidate, preserved as `harness/render/out/w22-approved-candidate.exe`, SHA-256
`cb62c6e616d3c4ccb55b3777773033e220e716c46d514bed3c8b3a33668bc9a0`. Its identity differs from the earlier candidate;
`binary differs from acquisition` is correct rejection after a rebuild.
No identity check was bypassed. Two complete acquisitions with this binary
and two subsequent complete render runs each cover all 15 screens, with valid
environments and identical metrics. The before column is the twice-confirmed
own baseline, not a threshold. Both final runs reproduce the earlier candidate
measurements. Only the four previously measured thresholds move downward;
the tiny about-desktop gain does not change its rounded threshold.

| Screen | Before: percent (changed pixels) | After: both runs | Pixels removed |
| --- | ---: | ---: | ---: |
| popup-422-5682 | 3.6442307692% (11,370) | 3.6442307692% (11,370) | 0 |
| popup-422-5705 | 2.0626068115% (16,221) | 2.0626068115% (16,221) | 0 |
| popup-422-5728 | 0.8535397377% (17,699) | 0.8535397377% (17,699) | 0 |
| keyframes-458-2021 | 2.6855468750% (110) | 2.6855468750% (110) | 0 |
| grid-429-1966 | 2.9639884816% (41,234) | 2.9639884816% (41,234) | 0 |
| report-446-1971 | 1.2764674160% (17,989) | 1.2764674160% (17,989) | 0 |
| notice-422-6914 | 3.8431641518% (16,810) | 3.7368541381% (16,345) | 465 |
| notice-422-7088 | 2.1180830179% (23,995) | 2.1180830179% (23,995) | 0 |
| notice-422-6865 | 1.1594242557% (25,422) | 1.1594242557% (25,422) | 0 |
| landing-833-3640 | 4.9873096447% (53,055) | 4.7830419252% (50,882) | 2,173 |
| landing-833-3322 | 2.4662890166% (72,516) | 2.4662890166% (72,516) | 0 |
| landing-832-2975 | 1.5037627000% (89,042) | 1.5037627000% (89,042) | 0 |
| about-422-3376 | 5.5144260282% (143,728) | 5.4317833026% (141,574) | 2,154 |
| about-422-3180 | 3.0121556183% (167,899) | 2.9337565805% (163,529) | 4,370 |
| about-422-2987 | 1.7731544741% (161,950) | 1.7682713195% (161,504) | 446 |
