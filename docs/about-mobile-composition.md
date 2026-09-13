# About mobile: an exported crop was cropped again

Base: `b7451aa410246f066e1dc5fd685a23456d46d94a`, measured on 2026-09-13.

## Localisation before implementation

Read the three preceding geometry reports before building the unmodified base
in this worktree's own target. The fresh binary SHA-256 is
`2933385fdb7a495f0fb2b05afe9b1228270ed80cc2318b605a685a685bd9dca6`.
Fresh acquisition and rendering reproduce about **7.46 / 4.06 / 2.41%**,
with rendered heights **7240 / 5620 / 4758px**. These are measurements of
the base binary, not values copied from the thresholds file.

`bands.mjs` places a major non-text difference at y=6000–6100 (24.25%).
`drift.mjs` cannot explain the portrait differences by a uniform page shift.
The y=5720 crop, `elements.mjs`, and the snapshot identify the element and
axis: portrait `422:3551` has the correct **300×400** box at **(30,5746)**,
but its image is stretched vertically *inside* that box. Its generated
background is **100% 117.53%**, positioned at **0% 24.48%**. Horizontal
frame placement and page height are correct.

The same defect affects mobile portraits `422:3514` and `422:3560`:

| Node | Layout position | Layout/export size | Generated background size |
| --- | --- | --- | --- |
| `422:3514` | 30,4514 | 300×400 | 100% 120.06% |
| `422:3551` | 30,5746 | 300×400 | 100% 117.53% |
| `422:3560` | 30,6275 | 300×400 | 100% 117.49% |

Each untransformed PNG, composited onto white at its collected position,
has **zero differing pixels at the harness's 24-channel tolerance** against
the corresponding reference region. The same zero-difference comparison
holds for all eight wider CROP portraits: tablet `422:3320`, `422:3340`,
`422:3358`, `422:3367`, and desktop `422:3125`, `422:3145`, `422:3162`,
`422:3171`. Those exports are 305×400, not the mobile 300×400.
This comparison uses exported pixels,
not an assumption about what the original photograph looks like.

`crates/devup-mcp-figma/src/scripts/assets.js` validates a `fills/N` request
against the selected image hash, then calls **`node.exportAsync(settings)`**.
It does not return `figma.getImageByHash(...).getBytesAsync()`.
The resulting PNG already contains the source crop. The chunked transport
also re-exports the node. Meanwhile, `paint_css` applies `imageTransform` to
that exported file as if it were the original photograph. For `422:3551`,
the vertical scale 0.8508567214 becomes 117.53% and expands the already
cropped 400px image to roughly 470px.

This rules out the previous line-advance, stroke, weight, overflowing row,
and collapsed-mask causes for these portrait pixels. Korean `keep-all`
remains intentional and is not part of this defect. The prior crop comments
in `responsive_screen.rs` describe original-image semantics; the actual
export transport supplies node-render semantics instead.

## Test-first record

`harness/render/out/w18-crop-red.log` in the main checkout records the
regression against unchanged production: **1 passed / 2 failed**.
The failing synthetic crop renders `50% 50%/200% 125%`; the node-export
contract requires displaying the already cropped image once. Inputs vary
both transform axes, node dimensions, a rotated transform, and a missing
transform. The negative test preserves FILL, FIT, and TILE behavior. The second RED
assertion requires the mapped `fills` property to describe the corrected
background. After implementation, all three tests pass; changing the mapped
image URL to an unrelated asset still makes asset fidelity incomplete.

All **268 plugin goldens** pass unchanged, along with the manifest hash and
coverage registry checks. No snapshot or manifest checksum was updated.
`responsive_screen.rs` removes seven obsolete crop-matrix allowlist strings
and records the actual node-export semantics; its comparison remains exact
outside the explicitly enumerated differences.

## Generator change

The IMAGE paint's `scaleMode == CROP` branch now emits
`0 0/100% 100% no-repeat`. The source filename still identifies the same
node and fill index. Its crop is preserved in the exported pixels rather
than reapplied as a CSS transform. FILL, FIT, TILE, layer ordering, theme
colors, Korean wrapping, and layout emission retain their existing behavior.
No raw-image source exists in this export path, so the unused inverse-crop
helper is removed rather than retained behind an invented heuristic.
The relative 100% sizing is the complete exported frame, not a tuned pixel
constant. No node identity, viewport width, or breakpoint is consulted.

The existing source-map path maps `bg` to `fills` and the asset to its real
`nodeId:fills:index`; the regression verifies both. No provenance or fidelity
expectation was weakened, and no new exactness claim is made about the
screen's remaining lossy projection.

## Fresh measurements

Candidate binary SHA-256:
`7f1bf937c8da32739851e45900a40d03d9cc051d9a6ec7fbf0e3eebca7264924`.
It was copied to `target/debug/devup-mcp-w18-candidate.exe` immediately after
building, so the full gate cannot overwrite the measured binary. Every
measurement reacquires with that exact executable and the unchanged harness.

| About screen | Fresh baseline | Candidate | Fresh repeat | Rendered height, all |
| --- | ---: | ---: | ---: | ---: |
| `about-422-3376` | 7.46% | 5.51% | 5.51% | 7240 |
| `about-422-3180` | 4.06% | 3.01% | 3.01% | 5620 |
| `about-422-2987` | 2.41% | 1.77% | 1.77% | 4758 |

Mobile's exact ratio falls from **0.07457565991405771** to
**0.05514426028238183**, a **1.9431399632 percentage-point** decrease.
The browser-only probe produces the same candidate ratio. The revised
portrait crop was visually inspected against the reference at full size.

| Other group | Fresh baseline = candidate, narrow to wide |
| --- | --- |
| landing | 4.99 / 2.47 / 1.50% |
| notice | 5.25 / 3.22 / 2.20% |
| popup | 3.64 / 2.06 / 0.85% |
| grid | 2.96% |
| keyframes | 6.71% |
| report | 1.28% |

All **12 non-about actual PNGs are byte-identical** to their fresh baseline.
All 15 reference hashes, theme hashes, and reported rendered sizes match.
No increase is hidden by rounding or by the harness threshold tolerance.
[about-mobile-evidence.json](about-mobile-evidence.json) records exact ratios,
actual/reference hashes, theme hashes, sizes, and binary identities.

## Full gate

The full gate runs in this worktree's own target with
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`,
`CARGO_INCREMENTAL=0`, and two jobs (`CARGO_BUILD_JOBS=2` for insta).
`CARGO_TARGET_DIR` is never set.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1070 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | 1070 passed, 0 failed, 2 ignored; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | 2 passed, 0 failed |

The ordinary MSVC link steps print the existing localized library-creation
warning; Clippy itself has zero warnings. Logs are
`w18-{fmt,clippy,workspace,insta,smoke}.log` under the main harness's `out/`.
No forbidden Rust file, harness script, plugin golden, manifest checksum,
Korean word-break rule, or corpus consistency assertion changed.

Measurement logs, diagnostic crops, and saved baseline PNGs are under the
main checkout's ignored `harness/render/out/w18-*` paths. Harness scripts
are unchanged.

The second fresh about acquire/render run reproduces every exact ratio,
rendered size, and actual/reference PNG hash from the first candidate.
Only the three measured about thresholds are lowered, after that repetition,
to **5.51 / 3.01 / 1.77%**. The other thresholds remain unchanged.

This delivery addresses one cause in one local commit, without pushing.
The worktree target is cleaned after the commit, and all acquisition,
diagnostic browser, and test processes started for this task are closed.
