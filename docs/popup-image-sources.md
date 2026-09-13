# Popup: responsive image sources and the shared text residual

Investigated all three popup widths from production sources at
`937313e39448db7f7c05dbb0ab206432261b2203`. One source-data defect is fixed:
different image URLs were merged into an HTML `src` array. The remaining
intrinsic text-width differences are separately quantified below.

## Own baseline and localisation

All seven preceding reports named in the dispatch were read before choosing
a cause. The worktree built its own baseline executable, freshly acquired
popup, and rendered it twice with the unchanged harness scripts. Acquisition
and rendering ran in an isolated copy of the harness with the main checkout's
banked calls and unchanged visual comparator. The generated files were not
shared with the other workers. Each rendering used popup's own theme,
`d541f2ae9049`, the full reference viewport, and disabled animations.

Baseline SHA-256:
`0ae51b1421d38fe3ca7c7cf079e1f6a28e39f1e77d9e9e5784178df5a47155ab`.
The build metadata says `937313e39448-dirty`: temporary, untracked harness
setup directories existed when the build script ran. No production source
or test had changed when this executable was built; those setup directories
were removed. This is the own measured baseline, not a threshold value.

| Screen | Frame pixels | Own baseline, both runs | Changed pixels |
| --- | ---: | ---: | ---: |
| popup-422-5682, 390px | 312,000 | 3.6442307692% | 11,370 |
| popup-422-5705, 768px | 786,432 | 2.0626068115% | 16,221 |
| popup-422-5728, 1920px | 2,073,600 | 0.8535397377% | 17,699 |

The mobile frame has fewer changed pixels than desktop, but desktop has
6.646 times its frame area. Their changed-pixel ratio is 0.6424, producing
the observed 4.2696-fold percentage ratio. This denominator effect is not a
complete causal account: the following shared features were checked directly.

* All three screenshots show the same broken megaphone image.
* All six text boxes at every width have the collected Y positions and
  heights. There is no accumulating vertical drift, extra paragraph line,
  or missing resolved font weight in the baseline.
* The widest HUG paragraph sets the HUG card's intrinsic width. Its browser
  width exceeds the collected width at every size, shifting centered content.
* No popup container has a painted OUTSIDE stroke. The only painted strokes
  are CENTER strokes on the exported close-icon vectors. Rounded outside
  borders cannot explain these captures.
* No cropped photo or offset HUG mask appears in the broken megaphone path.

Changed pixels within the union of source and browser text boxes are
8,728 / 13,392 / 14,685, respectively: about 76.8% / 82.6% / 83.0% of the
baseline differences. This is a spatial partition, not an assertion that all
those pixels are caused by font rasterisation. The evidence JSON also retains
the 50px vertical bands, full node boxes, DOM boxes, fonts, and asset data.

## Proven defect and correction

The collected megaphone nodes are `422:5687`, `422:5710`, and `422:5733`.
They export valid SVGs at 20x20, 24x24, and 24x24. The responsive module
previously emitted their three asset paths as `src={[mobile, tablet, null,
null, desktop]}`. `src` is an HTML attribute, not a devup-ui CSS property;
React serialises this array into one comma-separated URL. Chromium reports
`naturalWidth=0` and paints a broken-image indicator at every width.

A browser-only probe assigned each frame's actual collected SVG URL to that
image, preserving every layout and text style. It removed 215 / 328 / 328
changed pixels. This proved the representation error before implementation.

`codegen/responsive.rs` now includes the scalar image source in a child
image's matching identity. Different sources remain separate children and
the existing missing-child merge supplies their display arrays. Equal
sources still merge. A present image explicitly restores its natural display
when absent from another drawn slot, including an image present only in the
middle slot. Its original explicit display value, if any, takes precedence.

The condition reads component kind and projected source values. No viewport
condition, new breakpoint rule, frame ID, image dimensions, or fitted constant
was added. Existing slot placement and visibility machinery is reused.
The change is limited to child images; a whole responsive root that folds
directly to an Image is outside this matching path and is not claimed fixed.

This retains three real image elements, one displayed at a time. Browsers may
download hidden sources; the correction does not claim picture/source-style
network selection. Browser verification confirms each SVG loads and exactly
the intended image is visible at each captured width.

## Tests first, fidelity, and corpus

`popup-images-red.log` records **1 passed / 2 failed** before implementation:
the converter emitted array-valued `src` and failed to preserve a returning
source as a separate image. The first candidate additionally exposed the
middle-only image's missing display restoration. All three final tests pass,
covering two different slot layouts, three distinct sources, identical
sources, and a source that disappears and returns. These are synthetic data
tests and do not rely on the popup node IDs or capture widths.

No style, layout, text, or provenance source file changed. No new source-map
mapping is introduced. Responsive output continues to report
`mergedMappingVerified=false`; its source fidelity explicitly describes
the individual breakpoint projections, not the merged module. The acquired
candidate remains `projection=lossy`, preserving its existing whitespace and
layout limitations. It is not promoted to exact or mapping-complete.
The scalar URLs still identify the original node exports and manifest assets.

The plugin-answer captures and their three thresholds are comparison
baselines, not targets; none was edited. The 268 plugin goldens and manifest
are unchanged. Corpus and final-gate results are recorded below.

## Repeated generator measurement

Candidate SHA-256:
`eb54a7c9c6e2517ef4308b8376915d04d37b475a16b01c91f6921ccee84304bb`.
After the complete gate, the own binary was rebuilt and preserved. Both
final candidate runs freshly reacquired popup with that exact executable;
no acquisition identity check was bypassed. They reproduce the earlier
candidate's paired measurements, whose preserved binary hash is
`ece1dc9b8b68657b05a93973232636784bd54b6bf42b68f3fcac6ec0bcb1a0ab`.

| Screen width | Own baseline, repeated | Candidate, repeated | Pixels removed | Improvement |
| --- | ---: | ---: | ---: | ---: |
| 390 | 3.6442307692% | 3.5753205128% | 215 | 0.0689102564pp |
| 768 | 2.0626068115% | 2.0208994548% | 328 | 0.0417073568pp |
| 1920 | 0.8535397377% | 0.8377218364% | 328 | 0.0158179012pp |

The candidate exactly reproduces the isolated image-repair probe's metrics.
Frame dimensions, theme and reference inputs are unchanged. Only the three
generated popup thresholds move, to 3.58 / 2.02 / 0.84; tolerance and all
plugin-answer thresholds stay unchanged. Other screen groups were not
visually remeasured by this popup worker; the full repository gate covers
their generated-code contracts.

## Bounded negative: HUG text metrics

The baseline's longest paragraph has these measured boxes:

| Width | Source text width | Browser width | Source X | Browser X | Height, both |
| --- | ---: | ---: | ---: | ---: | ---: |
| 390 | 258 | 259.25 | 66.5 | 65.375 | 78 |
| 768 | 320 | 321.84375 | 224 | 223.078125 | 96 |
| 1920 | 356 | 357.609375 | 782 | 781.1875 | 108 |

Every popup text node is `textAutoResize=WIDTH_AND_HEIGHT`. This is the HUG
case intentionally excluded from the previous hard-break whitespace fix.
The existing report already proves that adding `pre-wrap` increases these
intrinsic widths; that rejected experiment was not reimplemented.

Two independent browser probes were run after repairing the image:

| Probe | 390 | 768 | 1920 |
| --- | ---: | ---: | ---: |
| Disable font kerning | 11,155 pixels, unchanged | 15,893, unchanged | 17,371, unchanged |
| Force all text to collected widths | 18.15224% | 10.40866% | 4.78361% |

The latter is a rejected diagnostic, not a candidate. The first paragraph
grows from 78 to 104px, 96 to 128px, and 108 to 144px; the second paragraph
and some controls also gain lines. A collected width cannot simply replace
HUG sizing when the browser's glyph advances require more space. It would
also change the intended content-driven sizing contract.

Korean `keep-all` was retained in every production change and browser probe.
For conservative mobile defect accounting, reserving the owner's 0.21pp
allowance leaves **3.4342307692pp before / 3.3653205128pp after**. This is an
accounting allowance originally measured on about mobile, not a new claim
that popup has exactly that wrapping floor: the baseline popup paragraph
boxes show no soft-wrap disagreement. No intentional Korean wrapping
difference is offered as a defect or improvement. The next bounded
lead is the font's resolved advances and intrinsic text-width computation;
neither kerning removal nor forcing the measured HUG width resolves it.
Matching box positions does not prove matching glyph rasterisation.

## Verification and artifacts

[popup-image-sources-evidence.json](popup-image-sources-evidence.json) retains
the exact four measurement runs, binary identities, reference/actual hashes,
spatial partition, source/DOM geometry, image-loading proof, rejected probes,
and the candidate's unchanged fidelity limitations. Raw logs and probe PNGs
are in this worktree's ignored `harness/render/out/popup-*` paths.

All Cargo commands use the own target, two jobs,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, and
`CARGO_INCREMENTAL=0`; `CARGO_TARGET_DIR` is never set.

| Required gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1,115 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | 1,115 passed, 0 failed, 2 ignored; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | 2 passed, 0 failed |

The unchanged plugin corpus consistency and byte-parity tests pass, as does
`korean_characters_keep_words_whole`. Ordinary MSVC link steps retain their
existing localized library-creation warning; the Clippy gate has zero warnings.
No off-limits file, plugin golden, manifest checksum, or harness script changed.

This is one local fix commit without a push. The task's preview and acquisition
processes are stopped, and the worktree's own target is cleaned after committing.
The modified production file is `codegen/responsive.rs`; there is no overlap
with `codegen/style.rs`, `layout.rs`, `text.rs`, or `provenance.rs`.
