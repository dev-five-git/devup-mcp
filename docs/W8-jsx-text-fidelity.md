# W8: JSX text fidelity

Base: `c2b4e31944e69035d233f217ce0f1d5fb5e02fb4`.

## Contract clarification

The coordinator approved preserving faithful LF `<br />` emission and treating
CRLF, CR, U+2028 and U+2029 as design line separators, each painting one newline
(CRLF is one separator). The independent Rust oracle checks JSX child characters
plus explicit/implicit list breaks; it does not claim to simulate CSS layout.
No ordinary space, tab, NBSP or other Unicode space is normalized by that oracle.

The generator emits `whiteSpace="nowrap"` only for `maxLines=1`; otherwise normal
CSS whitespace behavior applies. Both modes collapse ordinary whitespace.
Exact ASCII runs and outer spaces cannot be preserved in painted output by
changing JSX syntax alone. The coordinator explicitly ruled out CSS changes and
required a lossy diagnostic for the existing collapsing population instead.

The harness reads `innerText` but normalizes spaces, tabs, NBSP and line separators
before comparison. Its reported 3/245 failures therefore used a looser oracle.
Its uncommitted inputs are absent here; that browser measurement is not claimed
as rerun. The three-width, multi-child regression reproduces its reported cause.

## Baseline and RED evidence (before production edits)

With `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`,
`CARGO_INCREMENTAL=0`, `-j 2`, and this worktree's own target directory:

`cargo test --workspace -j 2 --no-fail-fast`: **1011 passed, 0 failed, 2 ignored**,
98 test-result records, exit 0. Windows linking printed informational
`linker_messages` warnings; this is distinct from the required Clippy check.

`cargo test -p devup-mcp-devup-ui --test jsx_text_fidelity -j 2 -- --nocapture`:
**1 passed, 7 failed**, exit 1. Failing tests:

- `jsx_adjacent_plain_segments_do_not_insert_a_word_boundary`
- `jsx_mixed_styled_and_plain_children_round_trip`
- `jsx_whitespace_and_special_characters_round_trip`
- `jsx_empty_segment_array_preserves_node_characters`
- `jsx_list_text_preserves_line_separators_and_edge_spaces`
- `jsx_css_collapsing_is_reported_without_changing_whitespace_mode`
- `jsx_existing_fixture_texts_round_trip`

The decisive failure was actual `We under stand each other.` versus expected
`We understand each other.`; the CJK test similarly painted `강조 이해 합니다끝`
instead of `강조 이해합니다끝`. NBSP edges became ASCII spaces, an empty segment
array dropped `Still here`, and a trailing list newline disappeared.

The final RED sweep measured **466 existing text occurrences, 5 failures**:
`upstream-codegen-080-f6233dd6a0 / captured:105` inserted a space in `HelloWorld`;
`upstream-codegen-253-347a2e5fb2 / 213:7494`, `213:7497`,
`I213:7499;68:1703`, and `upstream-codegen-254-9f340b6a10 /
I277:17554;68:1703` replaced a trailing CR with a space.
The sweep includes 176 plugin-case text occurrences and 290 texts from the ten
committed WQUW-151 frames. Synthetic fixtures without a node-level `characters`
use their collected segment characters as the source.

`cargo test -p devup-mcp --test downstream_integration
jsx_css_whitespace_loss_is_exposed_without_optional_diagnostics -j 2 -- --nocapture`:
**0 passed, 1 failed, 5 filtered out**, exit 1. The expected always-visible
`projectionIssues` entry was absent.

Two additional RED checks during boundary review also failed before their fixes:
`jsx_empty_segments_do_not_reintroduce_source_line_spaces` (0 passed, 1 failed;
actual `a b`, expected `ab`) and the list marker assertion in
`jsx_list_text_preserves_line_separators_and_edge_spaces` (0 passed, 1 failed;
a trailing break must not introduce a third marker).

## Implementation and exact JSX rules

Adjacent unwrapped segments now share a source line only when a formatting
newline would otherwise join two pieces of bare JSX text with an inserted space.
Empty unstyled segments cannot introduce an intervening blank source line.
Existing element/expression boundaries and already-faithful spellings stay the
same. Empty segment arrays fall back to node characters instead of dropping them.

Edge whitespace string expressions retain their original Unicode characters;
interior tabs use JSON string expressions instead of becoming JSX spaces.
Design line separators use `<br />` at both internal and outer edges. Lists
retain trailing separators as breaks in the final item, without adding a marker.
The shared encoder still drives source-map matching; no provenance expectation
was weakened. New ordinary-text tests require complete character mapping as well
as a round trip. Existing list mapping limitations are not relabeled exact.

The independent test oracle models
[Babel's JSXText cleanup](https://github.com/babel/babel/blob/main/packages/babel-types/src/utils/react/cleanJSXElementLiteralChild.ts):

1. Split each JSXText token at CRLF, LF, or CR; CRLF is one boundary.
2. Replace each tab with one ASCII space.
3. Trim leading ASCII spaces except on the first source line, and trailing ASCII
   spaces except on the last source line. Unicode spaces are not trimmed.
4. Drop empty cleaned lines. Append one space after a retained line unless it is
   the last source line containing a non-space/non-tab character.
5. Process each token independently: elements and expression containers interrupt
   JSXText cleanup. Evaluate emitted JSON strings exactly; `<br />` and list-item
   boundaries contribute explicit design line breaks.

This is not a pixel layout or CSS normal/nowrap simulator. Runs and edge spaces
are checked before CSS collapsing; the unavoidable CSS residue is exposed through
`DEVUP_CODEGEN_TEXT_WHITESPACE_COLLAPSE`, `property=characters`, and
`fidelityImpact=lossy`. The export test proves that `projectionIssues` contains it
with `includeDiagnostics=false` and that the quality projection axis is `lossy`.

The measured population is **35/466 occurrences** (9/176 plugin cases, 26/290
WQUW-151 texts), including spaces adjacent to explicit line breaks and tabs.
Counting only repeated spaces and the whole string's outer edges gives 8/466
(1 plugin and 7 WQUW-151). The diagnostic checks the actual emitted whole text,
not each styled segment's edges, so a legitimate inter-word space shared by two
segments is not incorrectly reported as an outer space. The sweep asserts the
35 count and reports zero remaining JSX character round-trip failures.

## Reviewed plugin snapshots

The plugin goldens **were affected**. Exactly **2/268** change; **266 remain
byte-identical**. Both changed snapshots repair defects identified in RED:

| Snapshot under `fixtures/devup-figma-plugin/snapshots/codegen/` | Reviewed change and reason |
| --- | --- |
| `upstream-codegen-080-f6233dd6a0.snap` | `Hello` and `World` formerly occupied two bare source lines and painted `Hello World`; they now emit `HelloWorld`, matching their adjacent source segments. |
| `upstream-codegen-253-347a2e5fb2.snap` | The trailing CR in text nodes `213:7494` and `213:7497` formerly became `{" "}`; each now emits `<br />`. No other line in this snapshot changes. |

Only these reviewed snapshot hashes changed in `manifest.json`:

| Snapshot suffix | Previous SHA-256 | New SHA-256 |
| --- | --- | --- |
| `080-f6233dd6a0` | `8e6072bef47346b75c343914a6259780879b21d38292ede697d6c9c354a518be` | `8fa99f0fe20f07891c75572d82fd86c9d0af901e5a51966e92a6907e14bbbdf9` |
| `253-347a2e5fb2` | `1f978c360fcceb2509f7f3ebe52ccbbc60cf3251481f05bf1019e564742ecc0c` | `fc13c44bd00fa699a9212a0c8500f1a696fad6d4930239c5f795931f79a0c35a` |

The corpus consistency test is unchanged and passes, including all 268 snapshot
checksums and the coverage registry. Diagnostic changes are intentional fidelity
accounting, not suppressed expectations. No harness or excluded source file was
modified.

One additional non-plugin snapshot changes:
`crates/devup-mcp-devup-ui/tests/snapshots/wquw_151__wquw_151_proofread_diagnostics.snap`.
It adds exactly three lossy CSS-whitespace diagnostics for `3879:35520`,
`3879:35535`, and `3879:35539`, whose collected strings have spaces before line
breaks (the last also ends in a space). Its assertion-line metadata follows the
expanded test. Existing diagnostics are unchanged; the TSX and source-map
snapshots are unchanged. This diagnostic snapshot is outside the plugin manifest.

The first full workspace run exposed two old impact-count expectations:
`r4_export_exposes_placement_contract_without_diagnostics` now records 41 rather
than 40 lossy impacts, and the WQUW-151 test records 4 rather than 1. Each revised
test additionally identifies the exact new `characters` diagnostics and their
lossy classification; the layout-coverage assertions remain unchanged. The R4
test's additional text is `3997:46604`. Only test code changed in
`crates/devup-mcp/src/server/projection.rs`.

## Final validation

All required commands passed against the final implementation in this worktree:

| Command | Measured result |
| --- | --- |
| `cargo fmt --all -- --check` | Exit 0; no formatting differences |
| `cargo clippy --locked --workspace --all-targets --all-features -j 2 -- -D warnings` | Exit 0; **0 warnings, 0 errors** |
| `cargo test --workspace -j 2 --no-fail-fast` | Exit 0; **1022 passed, 0 failed, 2 ignored** |
| `cargo insta test --workspace --all-features --check` | Exit 0; **1022 passed, 0 failed, 2 ignored**; no snapshots to review |
| `cargo test --locked -p devup-mcp --test stdio_smoke -j 2` | Exit 0; **2 passed, 0 failed, 0 ignored** |

The 11 added tests comprise 10 JSX/fixture tests and one public export test.
The baseline was independently measured at **1011 passed, 0 failed, 2 ignored**.
The first full implementation run reported 1020 passed / 2 failed / 2 ignored;
those two failures were the explicit diagnostic accounting expectations described
above, corrected with node-specific evidence before the successful final rerun.

All builds used this worktree's own `target`, debug information disabled in both
dev and test profiles, incremental compilation disabled, and two build jobs
(`CARGO_BUILD_JOBS=2` also covered the nested Insta Cargo invocation).
`CARGO_TARGET_DIR` was never set. `git diff --check` passed and there are no pending
snapshot files. The implementation and this report are committed together on
`owjs3901/W8-jsx-text-fidelity`, without pushing; the coordinator completion receipt
records the commit hash and the required post-commit `cargo clean` result.
