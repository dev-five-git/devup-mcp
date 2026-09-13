# Render input identity and reproducibility

Base: `d039623b0d1a`; investigation on 2026-09-13. No generator, theme generator,
provenance, plugin corpus, or golden files were changed.

## Root cause and causal reproduction

The two reported theme hashes identify **different generations of saved inputs**.
They do not demonstrate nondeterministic theme generation by the current binary.
The harness previously trusted files already present under `themes/`, `src/screens/`
and `out/`, without recording which binary produced them or checking their bytes.
`render.mjs` does not invoke the generator: pointing `DEVUP_MCP_BIN` at a current
binary did not establish that the files being rendered came from that binary.

The exact `d3137cabf5a5` about theme survives in the W11 worktree. Comparing it
with `67d6de70e679` gives precisely 25 changes, all typography `lineHeight` values:
unitless ratios such as `1.6` become rounded pixel strings such as `"26px"`.
There are no changed token names, selected brands, colours, or resource ordering.
Commit `757dc4a` changed that exact percentage conversion in the theme generator.

For popup, reversing only the same conversion in the current theme reproduces
`f26ad027d6de` exactly, including the serializer's final newline. The twelve
slots use `1.2` for `buttonSm`/`modalBtn`, `1.8` for `modalText`, and `1.5` for
`textboxTitle`; current output is `d541f2ae9049` with pixel line heights.

The asymmetry is explained by percentage-based typography in about and popup.
Grid and keyframes have no typography entries; landing already uses pixel line
heights. The historical conversion therefore changes only the former hashes.

Controlled reproduction in the main harness, with no binary or generator edits:

| About inputs | Theme hash | Mobile / tablet / desktop divergence |
| --- | --- | --- |
| Current modules and current theme | `67d6de70e679` | 7.46 / 4.06 / 2.41 |
| Current modules and saved W11 theme | `d3137cabf5a5` | 7.46 / 4.06 / 2.41 |
| Saved W11 modules and saved W11 theme | `d3137cabf5a5` | **11.24 / 6.92 / 4.37** |

The final row reproduces all three Session A figures exactly. Theme replacement
alone does not recreate them because current TSX explicitly writes pixel line
heights. This also eliminates a stale Vite theme cache as the explanation for
those absolute values. Saved module bytes matter as well as theme bytes.

The historical filesystem operation that put old bytes into the main harness
was not logged, so this report does not invent one. What is proven is the exact
old input identity, its ability to recreate Session A, and the missing harness
validation that allowed those inputs to be attributed to a different generator.

## Requested leads

1. **Call bank:** `CallCache::path_for` computes a single filename from the tool
   name and sorted argument keys. It directly reads that path; it neither scans
   candidates nor elects among multiple responses. All 605 initial entries were
   inside their 48-hour TTL. Bank growth does not select another matching entry.
   The bank remains a live capture cache, not an immutable design fixture; a
   future expired or changed upstream response can legitimately change inputs.
2. **Node scope/history:** acquisition still requests node scope. The old/new
   theme diff changes no used token set. The exact hashes are explained by the
   historical percentage conversion, without a history-dependent resource set.
3. **Group leader:** targets are traversed in manifest order and grouped by theme
   content hash. All three about themes are byte-identical; popup explicitly
   shares the first frame's responsive module/theme. No leader race was found.
4. **Accumulated artifacts:** this is the demonstrated unsafe boundary. An old
   manifest could render old generated files without any acquisition identity.
   A separate failing test proves that a refused acquisition would even append
   a target when an old snapshot/module/theme/reference happened to exist.

## Harness changes

Acquisition removes only the explicit outputs it is about to request, saves the
full server response, checks completion status and required files, and records
failed frames as skipped while continuing later frames. Missing asset files are
also skipped. The existing shared asset-path cache is preserved; responsive
followers require a successfully acquired leader.

Each target records the actual server identity, executable SHA-256, and SHA-256
of its module, theme, reference PNG, raw snapshot and required assets. Rendering
validates those inputs before building, compares the binary hash when
`DEVUP_MCP_BIN` is supplied, and refuses a missing explicit theme instead of
falling back to another file. Old manifests require reacquisition. Hand-authored
`*-answer` comparison screens retain their existing separate workflow.

The render report includes acquisition evidence and the full theme hash, and
reports skipped acquisitions with a failing overall exit status. No thresholds
are used to hide an invalid environment or missing input.

Regression tests were run red first: missing outputs raised `FileNotFoundError`,
stale outputs were incorrectly appended to the manifest, and old/replaced input
validation lacked the expected rejection. Those tests pass after the fix.
The four pre-existing pyright errors were fixed in touched code: subprocess
stream assertions and guarded stdout reconfiguration; acquire.py is now clean.

## Repeated measurements and gates

| Run | Group | Server PID | Theme hash | Divergence percentages |
| --- | --- | ---: | --- | --- |
| 1 | about | 128552 | `67d6de70e679` | 7.46 / 4.06 / 2.41 |
| 2 | about | 67468 | `67d6de70e679` | 7.46 / 4.06 / 2.41 |
| 1 | popup | 18020 | `d541f2ae9049` | 3.64 / 2.06 / 0.85 |
| 2 | popup | 94264 | `d541f2ae9049` | 3.64 / 2.06 / 0.85 |

The control and previously failing screens also repeat exactly:

| Screen | Theme hash | Round 1 | Round 2 |
| --- | --- | ---: | ---: |
| landing-833-3640 | `e18d9d7e25b4` | 4.99 | 4.99 |
| landing-833-3322 | `87ef9f58fdb8` | 2.47 | 2.47 |
| landing-832-2975 | `3ae50e9a165f` | 1.50 | 1.50 |
| grid-429-1966 | `a1a8437993a0` | 2.96 | 2.96 |
| keyframes-458-2021 | `a1a8437993a0` | 6.71 | 6.71 |
| report-446-1971 | `46d713e0a852` | 1.28 | 1.28 |
| notice-422-6914 | `89fe4447f2ad` | 5.54 | 5.54 |
| notice-422-7088 | `89fe4447f2ad` | 3.35 | 3.35 |
| notice-422-6865 | `89fe4447f2ad` | 2.22 | 2.22 |

All 15 screens were acquired and measured in both rounds, with zero skipped
screens or missing assets. Report and all three notice frames returned
`status=partial`, `quality.acquisition=complete`, and lossy projection; there is
no acquisition blocker in this tested environment. Their original refusal was
not reproduced, so no unsupported historical refusal cause is asserted.

`thresholds.json` was updated **only after both rounds matched**; its generated-
screen numbers are now backed by repeated measurements. This includes increases
where the old unsupported number was lower. These are corrected measurement
baselines, not claimed generator improvements. The plugin-answer thresholds
remain unchanged and are explicitly not claimed as remeasured.

A final combined render of all 15 screens passed the updated thresholds with
exit zero and exactly the same metrics. Temporary copies of the changed harness
scripts and thresholds in the main checkout were restored afterward; generated
inputs and ignored evidence remain there. No task-owned server process remained
after the runs.

Full hashes, PIDs, exact ratios and the old-theme diff are retained in
[harness-determinism-evidence.json](harness-determinism-evidence.json). Raw run
logs and reports are in main `harness/render/out/w15-round{1,2}-<group>-*`.

Every group in each round uses a new `acquire.py` process, immediately followed
by a new `render.mjs` process for that group, in the main checkout. The executable
is held constant and hashed before every acquisition and after both rounds:
`1c511f2d863eb092ec9301425cc1e1228bdf8d718786a4d0ad64316e9180f133`.
The second round checks equality of every recorded input hash, the full theme
hash and the unrounded divergence value, not just the displayed percentages.

All Cargo gates ran against this worktree's own target, with
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`, `CARGO_INCREMENTAL=0`,
and no `CARGO_TARGET_DIR` override.

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Pass, zero warnings |
| `cargo test --workspace -j 2 --no-fail-fast` | 1,059 passed, zero failed, two ignored |
| `cargo insta test --workspace --all-features --check` | Same counts; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke` | Two passed |
| `node --test crates/devup-mcp-figma/tests/explore_script_behavior.mjs` | 13 passed |
| Python acquisition regression tests | Two passed |
| Node input identity regression tests | Two passed |
| `npx --yes pyright harness/render/scripts/acquire.py` | Zero errors or warnings |

An initial workspace test build collided with the still-running probe's Windows
executable lock; it was rerun after that probe exited. The successful test and
snapshot builds emitted Windows linker informational-output warnings; the
required clippy command emitted zero warnings. No lint suppression was added.

To repeat a group after installing these harness changes:

```powershell
cd C:\Users\owjs3\Desktop\projects\devup-mcp\harness\render
$env:DEVUP_MCP_BIN = "<built worktree>\target\debug\devup-mcp.exe"
python scripts\acquire.py about
node scripts\render.mjs about
```

Repeat those two commands in another process, and use `popup` for the other
required group. For migration from an old manifest, acquire all groups once;
unproven saved inputs intentionally require reacquisition. A changed upstream
design or expired call bank is a changed input set, not a guarantee of matching
an older design's pixels.

## Which earlier claims this supersedes

The Session A about values are invalid **as a measurement of `d039623`**. They
remain reproducible historical values for the saved older generated inputs.
The Session A popup theme is likewise the older percentage representation.

In `docs/about-vertical-geometry.md`, the section "The harness does not reproduce
across sessions" incorrectly treats those older hashes as evidence that the same
current generator produced different themes. This report supersedes that
interpretation and the attribution of Session A to the current baseline.
Its `757dc4a` before column (7.44 / 6.90 / 4.19) is a historical pre-weight-fix
measurement, not a baseline for `d039623`; its shipped after column is
7.46 / 4.06 / 2.41. Neither historical about baseline is the current absolute
level. Popup's current values match Session B.

No claim is made that historical before/after experiments in
`docs/line-box-displacement.md`, `docs/integer-line-advance-verification.md`, or
`docs/line-box-fidelity-investigation.md` are invalid merely because their base
commits have different absolute figures. Their numbers must retain their stated
version context. The former report's final landing baseline is checked again
here. Old unverified current thresholds are replaced only after repetition.
