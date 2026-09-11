# R14 absolute evidence implementation plan

**Goal:** Correct ABSOLUTE classification without changing generated TSX.

**Architecture:** Extend the existing `absolute_component_verification` audit in
`provenance/sizing.rs`. Keep generation in `codegen/layout.rs` unchanged. Attach
width preservation and an axis summary in `codegen/component.rs`.

**Tech stack:** Rust, serde_json, existing integration fixtures and insta goldens.

**Spec:** `C:/Users/owjs3/orca/devup-mcp-briefs/R14-absolute-width-and-asset-bounds.md`.

User authorized execution in this session. All Cargo compilation uses `-j 2`;
do not set CARGO_TARGET_DIR, use installed MCP, push, or modify source repositories.

- [x] Read all three reports and original response evidence; trace R8 height,
  R9 dimension/constraint audit, and export_box/push_absolute generation.
- [x] Add `tests/r14_absolute_evidence.rs`: equal fixed parent, unequal or unknown
  parent, read errors/FILL, asset boundary vs rounding vs inherited constraints,
  and per-axis summary. Save a reduced fixture with source attribution.
- [x] Run `cargo test -p devup-mcp-devup-ui -j 2 --test r14_absolute_evidence`
  and observe assertion failures before changing production code.
- [x] Extend only the width percentage proof to non-auto-layout ABSOLUTE nodes
  in the emitted immediate positioned parent. Record source/generated values,
  exact parent px equality, resolved px, and failed proof conditions.
- [x] Audit asset dimensions against render bounds selected by existing export
  policy; report sourceFields, calculations, boundary delta, and 0.005px rounding
  separately. Read errors and non-FIXED sizing prevent promotion.
- [x] Report effective first-child/default constraints separately from declared
  constraints. Inherited GROUP constraints remain assumptions; horizontal SCALE
  and vertical MAX in the observation remain approximated with exact calculations.
- [x] Attach `widthPreservation` and `componentSummary` without removing prior
  evidence. Update superseded R8/R9 diagnostic expectations while keeping all
  original height/constraint assertions and adding an unequal-width control.
- [x] Run full workspace tests, JS behavior tests, clippy and fmt. Review each
  changed golden individually and document its cause. Keep R13 tests unchanged.
- [x] Verify executable paths and test names from this worktree target; save the final report.
- Final packaging order: commit changes, then `cargo clean` and confirm clean git status.
