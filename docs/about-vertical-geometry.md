# About text geometry: resolved weights, and why Korean keeps `keep-all`

Base: `757dc4a`, measured on 2026-09-13 in the main checkout's render harness.

Two independent text defects were found on the `about` screens. One is fixed
here. The other was implemented, measured, and **deliberately rejected** - it
narrows the pixel gap by chopping Korean words in half. Both are recorded,
because the rejected one is cheap to rediscover and looks like a win from the
metric alone.

## What shipped: resolved weights survive a typography token

`bands.mjs`, `drift.mjs`, `crop.mjs`, `boxes.mjs` and `elements.mjs` localised
the difference before any generator change. The wider page's displacement
begins at the Solution paragraph rather than accumulating down the page. On
tablet the 619px-wide text measured 288px tall against the collected 253px box,
and the crop shows `통해` wrapping onto an extra line immediately before an
explicit line separator. Everything below inherits that displacement.

The collected segments share a text-style ID but explicitly alternate between
weights 400 and 700, and both text-property emitters suppressed `fontWeight`
whenever a typography token was present. The token name carries no resolved
metrics, so the bold runs vanished, and missing bold changes glyph advances and
therefore wrapping. Restoring the resolved weights removes one 36px line on each
wider screen and brings the rendered page height to within 1px of the design:

| About width | Design height | Baseline height | With resolved weights |
| ---: | ---: | ---: | ---: |
| 360 | 7240 | 7240 | 7240 |
| 992 | 5619 | 5656 (+37) | 5620 (+1) |
| 1920 | 4757 | 4794 (+37) | 4758 (+1) |

That also rules out a repeated padding contribution as the source of the ~36px
overshoot. The advance and inside-stroke rules from `line-box-displacement.md`
were not touched. No viewport, breakpoint, frame ID, capture height or tuned
numeric constant participates.

## What was rejected: removing `word-break: keep-all` for Korean

Mobile has a second, real difference. Figma breaks Korean **within** words;
the generated screen breaks only at spaces, because codegen detects Korean
characters and emits `wordBreak="keep-all"`. Deleting that heuristic was
implemented and measured. It works, by the metric:

| About width | Baseline | Weights only | Weights + keep-all removed |
| ---: | ---: | ---: | ---: |
| 360 | 7.44% | 7.46% | 7.25% |
| 992 | 6.90% | 4.06% | 3.98% |
| 1920 | 4.19% | 2.41% | 2.41% |

It is not shipped. The owner's decision, and the reasoning:

**Figma's Korean line breaking is a limitation to compensate for, not a
specification to reproduce.** Korean offers a browser no inter-word breaking
opportunity it can infer, so the default `word-break` splits a word wherever the
line happens to end. Matching Figma's PNG more closely here means generating a
screen whose Korean is chopped mid-word - worse code that scores better. The
plugin settles this the same way, deliberately and with a comment, in its own
text renderer:

```ts
// Add wordBreak: keep-all for Korean text
if (hasKorean) {
  defaultProps.wordBreak = 'keep-all'
}
```

The price of removing it was measured precisely: **0.21pp on one screen and
0.08pp on another**, against 38 deleted attributes across 17 plugin
byte-parity goldens and every Korean line break in every generated screen. The
pixel metric is a proxy for correctness, not correctness itself, and this is the
case where the two point in opposite directions.

`korean_characters_keep_words_whole` in
`crates/devup-mcp-devup-ui/tests/rich_text_weight.rs` locks the decision so the
next fidelity pass cannot quietly reverse it, and the rationale sits on
`segments_contain_korean` in `codegen/text.rs`.

The wider candidate pages remain 1px taller than the design. No correction was
introduced for that residual.

## Measurement identity and integrity

All named binaries were built in the worker worktree's own `target/debug`:

| Binary | SHA-256 | Role |
| --- | --- | --- |
| `devup-mcp-w14-baseline.exe` | `2CAB1F5AB767179B2D3553BE1F9704A24C20158BDC16DACD2B9B380FAE4E0D67` | Unmodified production at 757dc4a |
| `devup-mcp.exe`, first candidate | `E1E3B90A89476C010C2B8AC2244BC84FF462BF203426D9B52ECE95D469B5FC4C` | Weight preservation only - **this is what shipped** |
| `devup-mcp-w14-weight-wrap.exe` | `784775AF837E71727529AC6D84BC78C3D0E0D856ECF6685F2AB949276E1A3A58` | Weight preservation plus keep-all removal - **rejected** |
| `devup-mcp-w14-final.exe` | `B63D38FD537C6C890E316109E6877C4070F07ACEF84E3C2D1400DB89341181CB` | Same emission, completed segment provenance |

Each measurement used a fresh `python scripts/acquire.py GROUP` process with
`DEVUP_MCP_BIN` pointing at the named binary, followed immediately by
`node scripts/render.mjs`. A group was rendered before the next was acquired.
No acquire log contains `quality=None` or a nonempty `missing` list. Every
before/after reference PNG is byte-identical, and the before/after theme hashes
printed by render are identical for every screen. All eight non-`about` actual
PNGs are byte-identical before and after, so their unchanged percentages are not
rounding over a smaller regression.

| Screen | Theme hash, before = after | Baseline | Shipped candidate |
| --- | --- | ---: | ---: |
| about-422-3376 | 67d6de70e679 | 7.44% | 7.46% |
| about-422-3180 | 67d6de70e679 | 6.90% | 4.06% |
| about-422-2987 | 67d6de70e679 | 4.19% | 2.41% |
| landing-833-3640 | e18d9d7e25b4 | 4.99% | 4.99% |
| landing-833-3322 | 87ef9f58fdb8 | 2.47% | 2.47% |
| landing-832-2975 | 3ae50e9a165f | 1.50% | 1.50% |
| popup-422-5682 | d541f2ae9049 | 3.64% | 3.64% |
| popup-422-5705 | d541f2ae9049 | 2.06% | 2.06% |
| popup-422-5728 | d541f2ae9049 | 0.85% | 0.85% |
| grid-429-1966 | a1a8437993a0 | 2.96% | 2.96% |
| keyframes-458-2021 | a1a8437993a0 | 6.71% | 6.71% |

Mobile's +0.02pp is retained in the record rather than smoothed away. It is the
cost of correct bold runs, and the wider screens pay it back many times over.

### The harness does not reproduce across sessions

The supplied `about` baseline of 11.24 / 6.92 / 4.37 was **not** reproduced by
an unmodified build of the same commit. Popup's supplied `f26ad027d6de` theme
was likewise not reproduced: isolated acquisition repeatedly produced
`d541f2ae9049` and 3.64 / 2.06 / 0.85. Landing, grid and keyframes reproduce
exactly.

Isolating each group into a fresh acquisition process - the coordinator's first
hypothesis - does **not** resolve it. Two sessions on the same commit with the
same binary can therefore print different theme hashes and different absolute
percentages, which means no absolute figure in `thresholds.json` is currently
backed by a reproducible measurement.

Acceptance was consequently changed mid-task to paired deltas against the
measurer's own unmodified baseline, with the before/after theme hash required to
match. `thresholds.json` is deliberately **left unchanged** here: lowering an
entry to a number one environment produced would assert a reproducibility that
does not exist yet. Harness determinism is tracked separately.

## Tests first

Recorded RED output, in the main harness `out/`:

* `w14-weight-red.log` - 0 passed, 1 failed: token-bearing text omitted both resolved weights.
* `w14-weight-provenance-red.log` - 1 passed, 1 failed: the outer default weight pointed at a node property instead of its actual styled segment.

Explicit legacy node weights keep their node-property mapping; segment-sourced
weights map to `styledTextSegments`. The strict projection mapping-gap test now
uses differing font families, which is a remaining real gap, and keeps its
strict rejection and diagnostic assertions. The old typography test now expects
its explicitly supplied weight 600. `korean_characters_keep_words_whole` and
`latin_only_text_gets_no_word_break_constraint` fix the wrapping behaviour in
both directions.

## Reviewed plugin byte-parity goldens

Exactly **one** of 268 goldens changes:

| Golden stem | Reviewed reason |
| --- | --- |
| upstream-codegen-109-e2824ad5be | Token-bearing segment explicitly carries weight 400; emit it |

Its snapshot checksum in `fixtures/devup-figma-plugin/manifest.json` was synced.
No corpus input, count or consistency assertion was weakened. Keeping `keep-all`
is what holds the other 16 goldens at parity; the rejected candidate would have
changed all 17.

The WQUW proofread and frame snapshots change only by gaining weights, with
`wordBreak="keep-all"` intact throughout. Their source-map entries and weight
ownership follow the actual emission.
