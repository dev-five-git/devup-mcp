# Mobile excess: dense text regions and an outside-edge layout error

Investigated from unmodified `dc99142` in the W21 worktree. The three mobile
percentages have a shared **denominator/density effect**, but their localisation
does not support one accumulating mobile layout defect. A separate, proven
outside-stroke error affects all three notice widths. Its correction is keyed
only on stroke/layout data; no viewport, breakpoint, frame ID, or measured
screen-specific constant participates.

## Fresh baseline and cross-screen comparison

The preserved unmodified executable is SHA-256
`a76d47054646ab4ac190fb4a9547c4b529d7298bbd034fb4562f67ed5a5f1aa4`.
All 15 screens were freshly acquired with that binary. Two complete render runs
reproduce every exact metric, not merely the displayed percentages. The local
harness uses unchanged tracked scripts, its own generated inputs and a copied
call bank; it does not share mutable generated screens with another worktree.
The comparator executable is the unchanged existing visual comparator copied
from the main checkout; generation and all Cargo gates use this worktree's own
target. No binary/input identity check was bypassed.

| Design | Width | Frame pixels | Changed pixels | Baseline, both runs |
| --- | ---: | ---: | ---: | ---: |
| about | 360 | 2,606,400 | 143,728 | 5.5144260282% |
| about | 992 | 5,574,048 | 167,899 | 3.0121556183% |
| about | 1920 | 9,133,440 | 161,950 | 1.7731544741% |
| notice | 360 | 437,400 | 22,982 | 5.2542295382% |
| notice | 992 | 1,132,864 | 36,501 | 3.2220107621% |
| notice | 1920 | 2,192,640 | 41,232 | 1.8804728546% |
| landing | 360 | 1,063,800 | 53,054 | 4.9872156420% |
| landing | 992 | 2,940,288 | 72,515 | 2.4662550063% |
| landing | 1920 | 5,921,280 | 89,041 | 1.5037458117% |

Every mobile frame has **fewer changed pixels than its own desktop sibling**.
The ratios therefore do not mean three times as many erroneous pixels. For
about, notice and landing respectively, desktop/mobile frame area is
3.504 / 5.013 / 5.566, while mobile/desktop changed-pixel count is
0.887 / 0.557 / 0.596. Multiplication gives the observed approximately
3.11 / 2.79 / 3.32 discrepancy ratios. This arithmetic alone does not explain
the cause of each pixel, but it prevents conflating a percentage with the
amount of erroneous content.

### Localisation on all three mobile frames

The five required scripts were run on all three mobile frames before production
Rust changed, then the isolated fresh baseline was checked. `bands.mjs` places
about's worst bands at y=700 and 2200, notice's at y=900 and 800, and landing's
at y=1500 and 900. The crops show paragraph differences for about, displaced
footer controls for notice, and coincident but differently rasterised text for
landing. `elements.mjs` confirms the mobile about portraits retain their
300x400 boxes and the notice search illustration is 100x95. The existing image
crop and mask-width corrections are present.

`drift.mjs` distinguishes the cases:

- Landing's best offset is zero throughout the main content. There is no
  growing multi-pixel vertical drift; its final footer band prefers -1px.
- About's local text bands often prefer -1px, while intervening images and
  sections return to zero. This is not a uniform page displacement. Its large
  paragraph boxes match the collected geometry: nodes 422:3391, 422:3496,
  422:3490 and 422:3478 have zero x/y/width/height deltas.
- Notice's content acquires +2px below the results wrapper and +3px farther
  down. Its displaced illustration and footer are not merely text edge noise.

The all-width DOM analysis matches every top-level emitted text box to a
collected TEXT node by normalised text and position: about 55/62/62,
notice 20/23/23, landing 73/74/78. Text boxes are located using the screen's
own theme, full reference viewport, loaded fonts and readiness condition.
The supplied `boxes.mjs` limits viewport height to 2000; its reported root
height can differ from the harness's full-viewport `rendered` height. Those
values were not substituted for capture dimensions.

### Quantified spatial decomposition

Each changed pixel is assigned once: text if it falls in the union of a
matched source or rendered text box, otherwise asset if it falls in an emitted
image/background/mask box, otherwise remaining surface/edge space. This is a
**spatial partition**, not a claim that every pixel inside a text box is caused
by font rasterisation. The evidence retains the individual source/DOM boxes,
font metrics and per-box counts for inspection. Overlapping per-text counts
are not summed to form the partition.

| Design / width | Text-region pp | Asset-region pp | Remaining pp | Total pp |
| --- | ---: | ---: | ---: | ---: |
| about 360 | 5.3569 | 0.0415 | 0.1161 | 5.5144 |
| about 992 | 2.7599 | 0.2119 | 0.0404 | 3.0122 |
| about 1920 | 1.6090 | 0.1293 | 0.0348 | 1.7732 |
| notice 360 | 3.2270 | 0.8304 | 1.1968 | 5.2542 |
| notice 992 | 1.9396 | 0.3579 | 0.9246 | 3.2220 |
| notice 1920 | 0.9952 | 0.1850 | 0.7003 | 1.8805 |
| landing 360 | 4.8419 | 0.0995 | 0.0458 | 4.9872 |
| landing 992 | 2.3237 | 0.1121 | 0.0305 | 2.4663 |
| landing 1920 | 1.4316 | 0.0490 | 0.0232 | 1.5037 |

About and landing mobile have 97.1% and 97.1% of changed pixels in text
regions. Their source text-box areas occupy about 22.37% and 32.14% of their
frames, compared with 13.35% and 9.04% on desktop. Notice has only 61.4% of
its mobile changed pixels in text regions and a demonstrable layout cause.
The broad similarity is dense content in a smaller denominator; the local
mechanisms differ. For landing specifically, changed text-region pixels per
source text-box area are 15.06% mobile versus 15.83% desktop: its 3.38-fold
text-region percentage ratio tracks the 3.55-fold text-box occupancy ratio,
without a higher within-box mismatch rate on mobile. About does not follow
that stronger model (23.95% versus 12.05%), so density alone is not offered
as a complete explanation for about.

The owner's intentional Korean word-breaking allowance is **subtracted**:
about mobile 5.514426% - approximately 0.21pp = **5.304426%** to investigate.
The adjusted mobile-minus-desktop gap is approximately **3.531272pp**.
That 0.21pp comes from the prior accepted experiment, not a new intervention.
No Korean policy, attribute or test is changed. The partition above shows raw
measured pixels, so it can be reconciled exactly with the PNGs; this allowance
is separate and is not relabelled as a production defect.

This investigation does not claim a complete known-cause decomposition of all
remaining text pixels. Matching boxes alone cannot establish identical font
files, glyph coverage, baseline quantisation or wrapping. There is no evidence
here for a width-specific text correction, and no such correction is proposed.
The stronger claim that all mobile residuals are fully understood remains
unproven; this delivery instead includes the measured general defect below.

## Proven cause: one outside edge became an inside layout border

Notice mobile node `422:6936` is a vertical HUG frame at (16,402), 328x319.
Its collected stroke is OUTSIDE, top=2, right/bottom/left=0; `strokeWeight`
is the unsupported mixed-value sentinel, not a uniform numeric weight.
The generator's asymmetric branch ignored stroke alignment and emitted
`borderTop="solid 2px $primary"`. CSS placed the child at y=404 and made
this wrapper 322px high. The 322 includes another existing 1px discrepancy
from child `422:6937`'s inside bottom edge. That inside path is independent
and is not changed in this commit.

The same data condition occurs at tablet `422:7111` and desktop `422:6888`:
932x327 and 1280x327 wrappers at y=442 rendered 330px high. An isolated
browser probe replaces only the outside edge with a zero-blur translated
box shadow. Child y returns to the collected y, wrapper height loses exactly
2px, and subsequent content moves up 2px. All source padding stays intact.
The probe changes neither fonts nor Korean wrapping nor any exported asset.

| Notice width | Baseline pixels | Outside-only probe pixels | Probe percentage |
| ---: | ---: | ---: | ---: |
| 360 | 22,982 | 16,810 | 3.8431641518% |
| 992 | 36,501 | 23,995 | 2.1180830179% |
| 1920 | 41,232 | 25,422 | 1.1594242557% |

A broader exploratory probe also removed asymmetric inside-edge layout costs.
It reduced notice further, but worsened about tablet/desktop to
3.1174112602% / 1.8475404667%. About's footer begins 1px above the collected
position on those widths; its existing extra border pixel partially cancels
that upstream displacement. The broader probe is retained as rejected evidence,
not production code. The final tests preserve INSIDE and CENTER behavior.
The earlier exploratory inside-edge failing expectation was narrowed with the
hypothesis; no pre-existing test or corpus assertion was relaxed.

## Correction and provenance

For non-asset horizontal/vertical auto-layout data with exactly one positive finite
per-side weight, three zero weights, a solid paint, square corners and
OUTSIDE alignment, codegen emits a zero-blur `boxShadow` translated toward
that side. Top/right/bottom/left use (0,-w)/(w,0)/(0,w)/(-w,0).
It consumes no layout space on either HUG or FIXED axes. It is not inset.
Existing effect shadows are appended to the same attribute, preserving both
writers instead of letting one overwrite the other. Rounded, dashed,
multi-edge, missing-weight, INSIDE and CENTER cases retain their old behavior.

The new `derived-single-edge-outside-stroke` resolution maps the generated
attribute to actual strokes, and additionally to effects when present. Its
public description states that side weights and paint determine the result,
and explicitly disclaims both a source shadow effect and pixel parity.
The stage derivation lists alignment and all four side weights as inputs.
A malformed first selected paint cannot borrow a later paint's validity and
incorrectly label a real effect as a derived stroke.

Tests recorded RED before the correction: missing non-layout paint, missing
composition with a real shadow, and missing derived provenance. The malformed
paint provenance case separately recorded RED before its guard was tightened.
Six focused tests now cover the four directions, zero/large padding, real
shadow composition, unsupported cases, truthful source mapping, and preservation of the exported-asset boundary. Logs are
`harness/render/out/w21-{outside-red,invalid-paint-red,outside-green-final}.log`.

## Candidate measurement and final gate

The preserved final delivery executable is SHA-256
`ca9138bd506ac20af33aadb4e2a6391c9732182cfcad6148d85ff288707d67bc`.
Its new acquisition and two full render runs confirm the same improvement
while preserving the exported-asset boundary.
After the final implementation rebuild, all 15 screens were reacquired with
that exact binary. Both complete candidate render runs are environment-valid
and reproduce every exact metric. The generator results exactly match the
outside-only browser probe above.

| Notice width | Own baseline, both runs | Final generator, both runs | Improvement |
| ---: | ---: | ---: | ---: |
| 360 | 5.2542295382% | **3.8431641518%** | 6,172 pixels / 1.4110653864pp |
| 992 | 3.2220107621% | **2.1180830179%** | 12,506 pixels / 1.1039277442pp |
| 1920 | 1.8804728546% | **1.1594242557%** | 15,810 pixels / 0.7210485989pp |

Mobile notice's reported rendered height changes 1217 to 1215; tablet and
desktop change 1145 to 1143. No other rendered dimension changes. Full-size
crops visually confirm the outside top line and illustration placement.
The remaining inside bottom stroke, text metrics and footer differences are
not silently folded into this correction.

All **12 non-notice actual PNGs are byte-identical to the fresh baseline**.
All 15 reference PNG hashes and theme hashes are unchanged. This verifies
no regression below the rounding precision of a displayed percentage.
The other measured results remain about 5.51/3.01/1.77, landing
4.99/2.47/1.50, popup 3.64/2.06/0.85, grid 2.96, keyframes 2.69 and report
1.28 percent. Only notice thresholds move, downward to 3.84/2.12/1.16 after
the repeated run; threshold tolerance is unchanged.

[mobile-excess-evidence.json](mobile-excess-evidence.json) contains the exact
four-run metrics, baseline/final binary identities, source and DOM text boxes,
spatial partition, rejected and accepted browser probes, and PNG hashes.
Raw logs, collected snapshots and crops live in this worktree's ignored
`harness/render/out/w21-*` paths. No tracked harness script was edited.

### Corpus review and verification status

All **268 plugin goldens and their manifest remain unchanged**. The initial
broader candidate changed only `upstream-codegen-181-5afa626d49`: node `80:40`
has an outside top edge, but it is folded into an exported `Image`. That review
exposed an additional representation boundary, not evidence about live CSS
layout: an asset export has its own painted bounds and may already contain
the stroke. This task's browser proof does not establish that composition.
The final correction therefore runs only on the existing non-asset style path;
asset folding is determined from the collected subtree, not from a fixture ID.
The golden stays `Image borderTop="solid 1px #000"` with its original checksum.
An additional regression first failed on the exported-image boundary and then
passed with the guard, alongside all original positive container tests. The
corpus consistency test was not changed. The previously requested golden
permission (`msg_9d41d6dcabf4`) became unnecessary; no approval was inferred.

The non-plugin WQUW source-map snapshot was reviewed separately. Its only
content addition is the public description of the new resolution; all existing
mappings remain unchanged. No assertion was removed or relaxed.

All Cargo work uses this worktree's own target with
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`,
`CARGO_INCREMENTAL=0`, and two jobs (`CARGO_BUILD_JOBS=2` for Insta).
`CARGO_TARGET_DIR` is never set.

| Required gate | Final result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | **1080 passed, 0 failed, 2 ignored** |
| `cargo insta test --workspace --all-features --check` | **1080 passed, 0 failed, 2 ignored**, no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | **2 passed, 0 failed** |

Logs are `harness/render/out/w21-{fmt,clippy,workspace,insta,smoke}-delivery.log`.
The ordinary MSVC linker prints its existing localized library-creation
warning; the Clippy gate itself has zero warnings. The corpus manifest and
all 268 plugin goldens are unchanged, and `korean_characters_keep_words_whole`
passes. No off-limits Rust file or harness script changed.

This is one correction and one local commit, without a push. The worktree's
own target is cleaned after committing; task-started acquisition and browser
processes are stopped before reporting completion.
