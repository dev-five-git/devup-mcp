# Keyframes screen: a static alignment difference

Investigated from unmodified `ee7fc6d616ecd0e3cd36ae63238d5ace91f44a0e`.

## Classification and measured baseline

The 6.71% discrepancy is **not a mid-flight animation capture**. The existing
render harness's `index.html` sets `animation: none !important` and
`transition: none !important` on every element and pseudo-element. At the
same readiness condition as capture, Chromium reports **zero animations**,
root `animation-name: none`, duration `0s`, transform `none`, and a 64×64
root at (0,0). No harness file was changed to establish this.

The worktree's own baseline executable has SHA-256
`c451093a2fb1bbeb436bf5a364055fa469fc45241476317a21e3e253fb22c352`.
Two fresh acquire/render runs reproduce keyframes **6.7138671875% (275/4096
pixels)** and grid **2.96%**, under their common theme `a1a8437993a0`.
These are measurements of the baseline, not copied threshold values.

`elements.mjs` finds no images or image backgrounds: the spinner consists
of CSS circles. The previous CROP-image defect is therefore inapplicable.
`bands.mjs` puts the worst eight-row band at y=48–56 (16.21%), and
`crop.mjs` shows horizontally displaced dots. `boxes.mjs` confirms their
12×12 wrappers have the correct positions and sizes.

## The collected chain and emitted keyframes

All eight chain frames are collected. They are sibling frames in a section,
with IDs `458:2021`, `458:2038`, `458:2055`, `458:2072`, `458:2089`,
`458:2106`, `458:2123`, `458:2140`, then back to `458:2021`. Each reaction
has `AFTER_TIMEOUT`, timeout `0.0010000000474974513`, a NODE action and
SMART_ANIMATE transition with duration `0.20000000298023224` and LINEAR easing.

The section-relative x coordinates are 89, 184, 268, 352, 436, 520, 604,
688; y is always 830. All direct, same-named child wrappers retain their
positions and 12×12 sizes. Their nested ellipse sizes vary. The documented
plugin rule compares direct children, finds no change, and falls back to
the frame itself; it does not recurse into those grandchildren.

The emitted animation exactly matches the stored plugin answer:

```tsx
animationDuration="1.6s"
animationFillMode="forwards"
animationIterationCount="infinite"
animationName={keyframes({
  "0%": { "transform": "translate(-95px, 0px)" },
  "13%": { "transform": "translate(95px, 0px)" },
  "25%": { "transform": "translate(84px, 0px)" },
  "100%": { "transform": "translate(-95px, 0px)" }
})}
animationTimingFunction="linear"
```

The first reverse difference is -95; successive frame deltas are +95 then
six +84 values. Repeated values are omitted by the plugin's incremental
keyframe rule. Eight transitions give 1.6s, first arrival rounds to 13%,
second to 25%, and the loop closes at 100%. The sub-10ms timeout emits no
delay. This explains byte parity; it does **not** claim that moving an entire
frame reproduces the prototype's changing nested dots. That inherited
animation limitation contributes zero pixels to this animation-disabled
capture and is separate from the measured static defect.

## Static cause and isolated browser probe

Each wrapper has `primaryAxisAlignItems: SPACE_BETWEEN`, horizontal layout,
and one visible in-flow ellipse. Figma's collected coordinates center the
single child on both axes. CSS `justify-content: space-between` places a
single flex item at the start of the main axis instead.

| Dot size | Wrapper position | Collected local dot position | Baseline global dot x | Correct global dot x |
| ---: | --- | --- | ---: | ---: |
| 2 | 44,8 | 5,5 | 44 | 49 |
| 4 | 52,26 | 4,4 | 52 | 56 |
| 6 | 44,44 | 3,3 | 44 | 47 |
| 8 | 26,52 | 2,2 | 26 | 28 |
| 9 | 8,44 | 1.5,1.5 | 8 | 9.5 |
| 10 | 0,26 | 1,1 | 0 | 1 |
| 11 | 8,7 | 0.5,0.5 | 8 | 8.5 |
| 12 | 26,0 | 0,0 | 26 | 26 |

A browser-only probe changes just those lone-item distributions to `center`.
The dots then occupy their collected positions, while root geometry and
animation state remain unchanged. With the same RGB(30,30,30) transparency
flattening and 24-channel tolerance, the discrepancy becomes **110/4096
pixels, 2.685546875%**: 165 fewer changed pixels, or **4.0283203125 percentage
points**. This is localisation evidence, not a measured generator candidate.
All 110 residual pixels lie within 1.02px of a designed circle circumference;
no pixel-tuned circle correction is proposed.

The layout cause is independent of the Smart Animate emission path. Scope
question `msg_559d8e972241` was sent to the coordinator before implementing a
correction; after its first ten-minute timeout, the same question was resumed
and escalation `msg_eab0d9b0b90b` identified the blocker. No permission was
inferred from elapsed time. No animation code, threshold, golden, or harness
script has been changed. The export already reports projection
`mapping-incomplete`: `DEVUP_CODEGEN_PROPERTY_UNMAPPED` explicitly names
`animationName` in the final JSX and delivery property audits. That existing
diagnostic is preserved, and its status is not relabeled as exact.

## Other fresh baseline screens

All 15 screens were freshly acquired with the preserved baseline executable
and rendered with the unchanged harness. The same binary bytes were retained
as `target/debug/devup-mcp-w19-baseline.exe` before test builds could replace
the ordinary executable. No input-identity check was bypassed.

| Group | Baseline divergence, narrow to wide |
| --- | --- |
| keyframes | 6.71% |
| grid | 2.96% |
| about | 5.51 / 3.01 / 1.77% |
| landing | 4.99 / 2.47 / 1.50% |
| notice | 5.25 / 3.22 / 2.20% |
| popup | 3.64 / 2.06 / 0.85% |
| report | 1.28% |

[keyframes-static-evidence.json](keyframes-static-evidence.json) preserves
the exact baseline metrics, binary identity, PNG hashes, and before/after
browser-probe DOM geometry. The probe's 2.69% remains separate from the
baseline measurements and is not used to lower any threshold.

## Verification and remaining work

The required gate ran in this worktree's own target with
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`,
`CARGO_INCREMENTAL=0`, and two jobs (`CARGO_BUILD_JOBS=2` for Insta).
`CARGO_TARGET_DIR` was never set.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1070 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | 1070 passed, 0 failed, 2 ignored; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | 2 passed, 0 failed |

Ordinary MSVC linking retains its existing localized library-creation
messages; the Clippy gate has no warnings. All 268 plugin goldens and their
manifest checks remain unchanged. No corpus assertion, Korean word-break
rule, forbidden Rust source, or harness script was modified.

**The requested generator improvement is not implemented.** This is not
category (b), so it would be incorrect to retire the 6.71% entry as a timing
artifact. There is a demonstrated static generator/layout cause outside the
original reaction-field constraint. Its correction requires the pending
scope expansion: test lone visible in-flow children first, reproduce RED,
emit the data-derived alignment, verify source mapping, reacquire with the
new binary, repeat keyframes/grid and all-screen regressions, then consider
lowering only measured thresholds. Hidden and absolutely positioned siblings,
vertical layout, and multi-item distributions need regression coverage.
No capture-specific constant, node ID, or breakpoint is needed.

Logs and probe artifacts are in the main checkout's ignored
`harness/render/out/w19-*` paths. The four preceding geometry reports were
read before investigation; no previously fixed text, stroke, crop, or mask
behavior is part of this cause.
