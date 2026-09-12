# Integer pixel line advances: verified, visual gate rejected

Measured on 2026-09-12 against base `442fc90699bb`. Absolute pixel emission
reproduces the supplied Figma block heights, but fails the required
no-regression bar at both wider landing widths. The experimental Rust change
and its tests were removed; this report is the deliverable. No threshold,
plugin golden, manifest checksum, fidelity classification, provenance entry,
or source map was changed.

## Binary identity and method

Both builds used this worktree's own binary, rebuilt between measurements:

```text
C:/Users/owjs3/orca/workspaces/devup-mcp/W12-integer-advance/target/debug/devup-mcp.exe
```

Both reported `devup-mcp 0.4.5 (442fc90699bb-dirty)`. The baseline's dirty
state contained only the newly written regression tests, with production
code unchanged. SHA-256 distinguishes the actual executables:

| Build | SHA-256 |
| --- | --- |
| Baseline | `38FBFAF5348131D509211AE8C57372FDF1128AB26EF93B8043B2968EF877CA45` |
| Absolute pixel experiment | `88CB77AA3DDFFAC93FC42097B8EFF56D18E9D161EAB8396A03934973995116B8` |

Each acquisition ran in the **main checkout** at
`C:/Users/owjs3/Desktop/projects/devup-mcp/harness/render`, with
`DEVUP_MCP_BIN` set to the path above, using `python scripts/acquire.py landing`.
The existing call bank, dependencies, Chromium, and release visual comparator
were used in place. There was no copied call bank or shared Cargo target.
Comparison used channel tolerance 24.

For the first two renders a temporary copy of `scripts/render.mjs` added
read-only DOM measurements before its existing screenshot step; the render
and comparison logic was unchanged. It selected the distinctive heading,
subtitle, and benchmark text, recorded computed font size and line height,
and read `getBoundingClientRect()`. Parent containers were excluded from the
table using the element's matching text and computed font size. The temporary
script was deleted before the test gate. Generated JSON measurement reports
were retained in the main harness's ignored `out/` directory as
`integer-baseline.json` and `integer-pixels.json`.

## Independent mechanism verification

Fresh snapshot acquisition reports `lineHeight.value = 129.99999523162842`
with `unit = PERCENT` for these nodes. The candidate calculates
`round(fontSize * percent / 100.0)` and emits the result with `px`; it does
**not** round the ratio before multiplying or divide the rounded advance
back by font size. Actual browser measurements reproduce all seven supplied
rows before the change and match all seven design heights afterward:

| Node | Font | Lines | Design height | Baseline DOM height | Pixel DOM height | Pixel computed line height |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `833:3646`, mobile heading | 38 | 5 | 245 | 246.953125 | 245 | 49px |
| `833:3647`, mobile subtitle | 18 | 3 | 69 | 70.171875 | 69 | 23px |
| `833:3664`, mobile benchmarks | 28 | 1 | 36 | 36.390625 | 36 | 36px |
| `833:3328`, tablet heading | 52 | 4 | 272 | 270.375 | 272 | 68px |
| `833:3329`, tablet subtitle | 24 | 2 | 62 | 62.375 | 62 | 31px |
| `833:3346`, tablet benchmarks | 36 | 1 | 47 | 46.796875 | 47 | 47px |
| `832:2980`, desktop heading | 52 | 4 | 272 | 270.375 | 272 | 68px |

The baseline's computed advances were 49.4, 23.4, 36.4, 67.6, 31.2,
46.8, and 67.6px respectively. The measurements validate the supplied rule
for these captures, including its font-size-dependent sign reversal.

## Tests first, then the actual emitter experiment

Before changing either emitter, a temporary integration test exercised
`generate_devup_json` with the captured percentage at font sizes 38, 18, 52,
and 36, expecting `49px`, `23px`, `68px`, and `47px`. A second test exercised
`generate_component` on an unstyled 52px text node, expecting
`lineHeight="68px"`.

```text
cargo test --locked -p devup-mcp-devup-ui --test integer_line_advance -j 2

RED: 0 passed; 2 failed
percentage_style_rounds_the_advance_at_each_font_size:
  font size 38; left: Number(1.3); right: "49px"
inline_percentage_text_rounds_the_advance:
  emitted <Text boxSize="100%" fontSize="52px" lineHeight="1.3">
```

Only afterward, `typography_value()`'s PERCENT branch was changed to read
the style's numeric `fontSize` and emit
`format_px((size * percent / 100.0).round())`. `line_height()` took a numeric
font-size argument and did the same for both node and rich-text segment
callers. AUTO and pixel-valued branches were unchanged. Repeating the same
test command gave **2 passed, 0 failed**. The binary was then rebuilt and
all three screens freshly acquired from that experimental binary.

This was a bounded experiment, not a completed general token implementation.
The landing themes use their own font sizes: mobile h1 is 38px and the two
wider h1 values are 52px. Computed browser sizes and advances above prove that
the larger headings did not accidentally receive the mobile 49px advance.

A fixed pixel token is valid for its associated font size only. A node that
overrides that size needs a newly computed pixel advance from its resolved
size and percentage; responsive token slots likewise need their own advances.
Codegen currently receives a style-ID-to-token-name map, not the style
metrics needed to compare overrides. A complete implementation would need
that metadata or explicit resolved node/segment overrides. A missing numeric
font size or a font-size variable that can change by mode cannot safely use
one precomputed pixel advance. The temporary candidate did not solve those
cases and is not retained as production code. They do not explain this
experiment's regression: the measured landing headings have known numeric
sizes and exact correct pixel advances.

## All-width visual result

| Screen | Width | Baseline binary | Pixel-emission binary | Required maximum | Decision |
| --- | ---: | ---: | ---: | ---: | --- |
| `landing-833-3640` | 360 | 11.41% | 9.64% | strictly below 11.41% | Local gain |
| `landing-833-3322` | 992 | 3.10% | 3.71% | 3.10% | Reject |
| `landing-832-2975` | 1920 | 1.84% | 2.31% | 1.84% | Reject |

The candidate render exited 1 for both wider regressions. Page heights were
2955 / 2964 / 3084px at baseline and 2955 / 2966 / 3086px with pixel emission.
No threshold was tightened for the rejected mobile-only improvement.

The local correction succeeded; the aggregate screenshot constraint resisted
it because local line advance is not the only remaining geometry or glyph
difference. Concrete evidence:

| Node | Design Y | Baseline Y | Pixel Y |
| --- | ---: | ---: | ---: |
| Mobile subtitle | 359 | 360.953125 | 359 |
| Mobile benchmarks | 888 | 893.125 | 890 |
| Tablet subtitle | 546 | 544.375 | 546 |
| Tablet benchmarks | 940 | 940.75 | 942 |
| Desktop subtitle | 496 | 494.375 | 496 |
| Desktop benchmarks | 900 | 900.75 | 902 |

The corrected subtitle positions match exactly, but each benchmark section
still begins two pixels below its design position. At wider widths the
baseline's short text partly offset that downstream displacement; fixing the
text exposes more of it. The source of that separate displacement is not
established here, and no width-specific or unrelated layout correction was
added to conceal it.

Direct PNG measurement also separates advance from glyph placement. Selecting
primary-blue pixels (`abs(R-90)<25`, `abs(G-68)<25`, `B>230`, more than two
pixels per row) within the heading rectangles produced these inclusive bands:

| Heading | Figma | Baseline | Pixel emission |
| --- | --- | --- | --- |
| Mobile, x=16 y=90 w=328 h=245 | 101–127 / 150–176 / 199–225 | 101–127 / 150–176 / 200–226 | 101–127 / 150–176 / 199–225 |
| Desktop, x=280 y=200 w=551 h=272 | 216–251 / 284–319 / 352–387 | 214–250 / 282–318 / 349–385 | 215–251 / 283–319 / 351–387 |

The desktop pixel candidate has exact 68px band advances, yet its glyph bands
start one pixel above Figma's and are a different height. Absolute advances
therefore solve the demonstrated accumulation defect without solving all
font rasterization/placement or downstream layout errors. The wider
regression cannot be attributed to re-emitting a ratio or sharing the wrong
font-size advance: neither occurred in this run.

## Corpus and final verification

Changed plugin goldens: **none**. Changed manifest checksums: **none**.
The rejected experiment was removed before the final gate, so all 268 pinned
goldens remain the original reviewed contract. The consistency test was not
weakened. All forbidden files remain untouched.

Final gate commands used
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`,
`CARGO_INCREMENTAL=0`, and `CARGO_BUILD_JOBS=2`, without `CARGO_TARGET_DIR`.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1049 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | Pass; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke` | 2 passed, 0 failed |

Test linking emitted the existing localized MSVC library-creation
`linker_messages` warnings; the clippy command itself emitted zero warnings.
The report was staged before the gate, and no temporary untracked files were
introduced during it, so the build-identity check passed.

Finally, fresh acquisition from the restored worktree binary followed by the
unchanged `node scripts/render.mjs` passed the existing thresholds at
**11.41% / 3.10% / 1.84%**. The restored executable's SHA-256, recorded before
that acquisition, was
`8479CA649C9B8DCDC45F1E5AEB11A2A148851654434BB8C671240827A9D18159`.
The generated harness inputs are thus back on the baseline generator. No
experimental emitter, failing experimental test, or temporary render script
is retained in the commit. The line-advance defect remains open because its
isolated correction fails the mandatory no-regression gate.
