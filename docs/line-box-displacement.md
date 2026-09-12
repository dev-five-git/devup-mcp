# Integer advances and the remaining two-pixel displacement

Measured from base `8ab69cbe42b68c6b8eac23e70a6da087c1153083` on 2026-09-12.
The integer-advance mechanism in `integer-line-advance-verification.md` was
accepted as the starting point. This change also removes the separate,
width-independent displacement before the benchmark section.

## Root cause and browser evidence

The Star control contains a 44px child (24px icon plus 10px padding on each
side). Its outer auto-layout frame has zero padding and a 1px inside stroke.
Figma gives both frames a height of 44px. The generated outer flex container
had an automatic height of **46px**, even with `box-sizing: border-box`.
Border-box includes borders in a specified height; it does not subtract
borders from an automatic height around intrinsically sized children.

Existing `push_padding()` already subtracts inside stroke width from padding,
but clamps the result at zero. That cannot absorb a border when padding is
less than stroke width. The sibling Sponsor control stretches to the taller
Star control, making the whole row two pixels too tall.

The reset was checked once with the requested script: five stylesheets,
body margin 0px, root font size 16px, root line height 24px. Browser measurements
also confirmed border-box on the measured elements. All containers between
the subtitle and benchmarks use flex layout; no unintended inherited line
box was found. Each screen was measured using its own acquired theme; the
reset script alone otherwise uses the last theme built by the harness.

| Width | Baseline benchmark Y | Browser-only outer-border removal | Change | Candidate Y | Design Y |
| --- | ---: | ---: | ---: | ---: | ---: |
| 360 | 893.125 | 891.125 | -2 | 888 | 888 |
| 992 | 940.75 | 938.75 | -2 | 940 | 940 |
| 1920 | 900.75 | 898.75 | -2 | 900 | 900 |

The border probe changed just the Star outer container's border width in
the browser, without changing typography or the reset. On the candidate,
the same probe changes benchmark Y by zero: the stroke is already painted
without taking layout space. The prior report's pixel-only values were
890 / 942 / 902; this fixes that precise residual.

## Emission and token validity

Both percentage emitters now write `round(fontSize * percent / 100)` as a
pixel length. AUTO and pixel-valued conversion branches retain their existing
semantics. Each responsive theme slot computes its own advance from its own
font size. Node and rich-text callers supply resolved numeric font sizes.

Codegen still receives token names without style metrics. It therefore emits
resolved `fontSize` and `lineHeight` together even when it emits a typography
token. A reused token at a different source size gets that size's own advance;
it cannot silently retain the token's advance. This also preserves explicit
AUTO or pixel line-height overrides. A token-only source with no explicit
metrics continues to use the token's paired font size and advance.

If a styled source supplies a size but no usable line-height metrics, codegen
refuses with `DevupCodegenFailed`. A percentage advance with a missing numeric
size or a bound font-size variable is likewise refused. Theme generation
refuses such percentage styles with `DevupThemeConflict`. These cases require
mode-aware metrics that the name-only map cannot represent; no guessed pixel
advance or partially generated theme is returned. Consumers must also update
line height if they subsequently override a generated token's font size by hand.

For an inside uniform stroke on a horizontal or vertical auto-layout node,
if any collected padding is smaller than the stroke width, codegen paints
the stroke with `outline` and `outlineOffset = -strokeWeight`. Padding is then
left intact. Otherwise the existing border/padding treatment stays in use.
This decision reads stroke, padding and layout fields, never viewport width,
frame identity, current font scale, or a landing-specific correction.

Outline properties map to their stroke source fields. New rich-text metric
attributes map to the owning `styledTextSegments`; the source-map test catches
the previously unmapped nested advance. Fidelity assertions and corpus
consistency assertions were not weakened.

## Tests first

Captured logs are in the main harness's ignored `out/` directory.

| Log | RED result before implementation |
| --- | --- |
| `w13-red.log` | 0 passed, 2 failed: theme returned 1.3 instead of 49px; inline text returned 1.3 instead of 68px |
| `w13-geometry-red.log` | 0 passed, 3 failed: zero-padding wrapper emitted a border |
| `w13-token-red.log` | 3 passed, 3 failed: reused-token metrics absent and unsafe cases accepted |
| `w13-provenance-red.log` | 6 passed, 1 failed: nested 23px advance lacked segment provenance |

All seven regression tests subsequently passed. Handbuilt typography and
provenance test inputs that supplied a size but omitted line height now state
AUTO explicitly, keeping those tests about their original concerns while
meeting the new representation contract.

The server's strict mapping-gap regression previously used nested font size
as its unmapped input. Since that metric is now mapped, the input now varies
nested font weight instead, retaining the same strict rejection and diagnostic
assertions for an actual remaining mapping gap.

## Reviewed plugin goldens

Exactly 24 of 268 plugin goldens change. The following table enumerates every
one under `fixtures/devup-figma-plugin/snapshots/codegen/`; each was compared
with its corresponding collected fixture. Only these 24 snapshot checksums
were updated in the manifest. No fixture inputs or consistency test changed.
Percentages below are readable abbreviations of the captured floating-point
values, and the emitter uses the original value before rounding the advance.

| Golden stem | Reviewed reason |
| --- | --- |
| upstream-codegen-109-e2824ad5be | Token safety: explicitly emit the fixture's 16px size and PIXELS 1.5 advance as a pair; 1.5px is already the source's pixel value, not a ratio conversion |
| upstream-codegen-161-5b230821f2 | 16px at 160% becomes 26px |
| upstream-codegen-162-51e1254b4c | 16px at 160% becomes 26px |
| upstream-codegen-163-2a9f9a056a | 16px at 160% becomes 26px |
| upstream-codegen-164-33e4c6591c | 18px at 160% becomes 29px |
| upstream-codegen-165-501ee1df12 | 20px at 140% becomes 28px; 16px at 160% becomes 26px |
| upstream-codegen-166-cda14f0b08 | Three 36px card labels at 160% become 58px |
| upstream-codegen-184-0c09788bb7 | 16px at 160% becomes 26px |
| upstream-codegen-185-e79c2ae189 | 16px at 160% becomes 26px |
| upstream-codegen-186-0988e78887 | 16px at 160% becomes 26px |
| upstream-codegen-191-2021d398f8 | 12px/160%, 20px/120%, 14px/160% become 19px, 24px, 22px |
| upstream-codegen-213-aa53408d2f | 16px/150% and 15px/150% become 24px and 23px |
| upstream-codegen-222-f2d780c847 | 64px at 140% becomes 90px |
| upstream-codegen-223-4dc8092992 | 20px/150%, 16px/150%, 15px/150% become 30px, 24px, 23px |
| upstream-codegen-243-14366a47ff | 36px at 130% becomes 47px |
| upstream-codegen-245-537356bcf4 | 40px/140% becomes 56px; the 42px rich segment explicitly gets 59px rather than inheriting 56px |
| upstream-codegen-246-295f39e09b | 64px at 140% becomes 90px |
| upstream-codegen-247-39d6959685 | 64px at 140% becomes 90px; text stroke and shadows unchanged |
| upstream-codegen-248-d1bc18e068 | 15px at 200% becomes 30px; list semantics unchanged |
| upstream-codegen-249-6b8a42981d | 16px at 140% becomes 22px |
| upstream-codegen-251-50acddfb96 | 20px/200%, 16px/180%, 17px/170% become 40px, 29px, 29px |
| upstream-codegen-252-49e979239e | 20px/150%, 35px/150%, 17px/200%, 15px/170%, 16px/170% become 30px, 53px, 34px, 26px, 27px |
| upstream-codegen-253-347a2e5fb2 | 28px/150%, 16px/180%, 24px/170% become 42px, 29px, 41px |
| upstream-codegen-254-9f340b6a10 | Frame 638 has horizontal auto-layout, zero padding and a 1px inside stroke: border becomes inward outline, the same geometry defect as Star |

The WQUW proofread standalone/embedded TSX and ten frame snapshots also gain
resolved metric pairs and the applicable inward strokes. The proofread source
map gains mappings for those actual attributes; diagnostic embedded source
excerpts change accordingly. No new unmapped-advance diagnostic is accepted.
Snapshot tooling removes stale `assertion_line` headers where it rewrites files.

## Visual identity and final gate

All acquisition builds use this worktree's own executable:

`C:/Users/owjs3/orca/workspaces/devup-mcp/W13-line-box-displacement/target/debug/devup-mcp.exe`

| Build | SHA-256 |
| --- | --- |
| Baseline, production at 8ab69cb | EC8407214626EB5D635548BB104ECAF8E30F55322AFA35C1A4A6F84B3D1915DA |
| First measured candidate | 460E421A73B90A3C2F6D7A7BB12147BC8F7F2AD73E109FEB4FD2ED9FCBFAA1C6 |
| Final candidate with token refusal and segment provenance | 3E35923B557CD7AC045F4991CB732B3D8F939596B19BC877931ED33795240427 |

Baseline and final candidate were each acquired in the main checkout's
`harness/render` with `DEVUP_MCP_BIN` set to that path, then rendered with the
unchanged `node scripts/render.mjs`. No harness Python script was edited.

| Screen | Width | Baseline binary | First candidate | Final candidate |
| --- | ---: | ---: | ---: | ---: |
| landing-833-3640 | 360 | 11.41% | 7.75% | 4.99% |
| landing-833-3322 | 992 | 3.10% | 2.47% | 2.47% |
| landing-832-2975 | 1920 | 1.84% | 1.50% | 1.50% |

The final browser measurement confirms benchmark Y remains exactly
888 / 940 / 900 after adding token-safety emission. Final rendered page sizes
are 360x2955, 992x2964 and 1920x3084. Only these three measured thresholds
were tightened, to 4.99 / 2.47 / 1.50. Other screens remain unmeasured by this
task. Font rasterization differences remain; no glyph-position correction was
introduced.

| Width | Heading Y / height / advance | Subtitle Y / height / advance | Benchmark height / advance |
| --- | --- | --- | --- |
| 360 | 90 / 245 / 49px | 359 / 69 / 23px | 36 / 36px |
| 992 | 250 / 272 / 68px | 546 / 62 / 31px | 47 / 47px |
| 1920 | 200 / 272 / 68px | 496 / 62 / 31px | 47 / 47px |

Every Cargo command uses `CARGO_PROFILE_DEV_DEBUG=0`,
`CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`, and two jobs; no
`CARGO_TARGET_DIR` was set.

| Final gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1056 passed, 0 failed, 2 ignored |
| `cargo insta test --workspace --all-features --check` | Pass; 1056 passed, 0 failed, 2 ignored; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke` | 2 passed, 0 failed |

Final logs: `w13-workspace-final.log`, `w13-insta.log`,
`w13-clippy-final.log`, and `w13-smoke.log` in the main harness `out/`.
MSVC test linking reports its existing localized library-creation messages;
the clippy gate itself has zero warnings. No forbidden source file or harness
Python script was modified. All temporary measurement scripts are removed;
browser and acquisition child processes were closed. The change is committed
locally without pushing, followed by `cargo clean` as required.
