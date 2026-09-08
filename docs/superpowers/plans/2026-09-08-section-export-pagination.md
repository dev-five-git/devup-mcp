# Selected SECTION export pagination

**Goal:** Preserve all selected frame descendants across fast snapshot pages.

**Contract:** README.md (Section export), the public tool instruction 7, and
`section_requires_selection_then_exports_requested_or_all_screens_from_one_artifact`
distinguish the compact selection menu from selected per-screen artifacts.
The menu's node/candidate count is not evidence of a bug. Explicit `frameIds`
and `allScreens` request screen exports. Canonical URL recapture is an alternative;
it is required separately for `referencePng`.

**Scope:** Only the SECTION multi-root collector continuation and its regression
tests. Preserve strict completeness checks, index selection and visual root order.
No design edits, parent workspace edits, global MCP changes, push, PR or release.

- [x] Verify the public contract before treating the observed partial export as a bug.
- [x] Reproduce the installed build's partial result and inspect the original script cursor.
- [x] Add collector regression tests for one/many selected roots, multiple pages,
  resource merge/deduplication, cursor progress/termination and root ordering.
- [x] Run the new tests before production changes and retain the actual failure.
- [x] Continue the same multi-root call at nextOffset; validate its range and
  completion against the requested offset before recording the page.
- [x] Run the regression tests, existing SECTION integration tests and CI checks:
  cargo fmt, Node script tests, stdio_smoke, workspace clippy, cargo insta and
  release build. Distinguish tool/environment failures from code failures.
- [x] Run the local fixed binary against the real SECTION with one and two frameIds;
  compare nodes, text and tokens with direct frame exports; inspect planning notes.
- [x] Record measurements, update the Orca comment and prepare the local commit.
