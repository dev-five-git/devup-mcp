# R16 implementation plan

Spec: `C:/Users/owjs3/orca/devup-mcp-briefs/R16-resolution-labels-and-verdict-scope.md`.

Goal: explain existing mapping, bounded verification, and aggregate verdicts without changing generated TSX or weakening R6–R15.

- [x] R16-1: first assert serialized sourceMap explains the mapping axis and raw-fallback/verified coexistence using the modal fixture. Add a serialization-only resolution dictionary in provenance; document every emitted label.
- [x] R16-2: first assert independently named failures for unknown parent width, unequal parent width, read errors, non-FIXED sizing, and dimension overrides. Also cover successful/null reasons and asset boundary failures. Derive blockedBy from the existing proof conditions; keep every acceptance predicate unchanged.
- [x] R16-3: first project the R14 asset fixture and assert response/frame verdictScope identifies only horizontal/vertical as unresolved for the target node. Include complete, non-projection, and delivery cases. Derive summaries after final property audit and preserve them in resource delivery.
- [x] Run `cargo test --workspace -j 2 --no-fail-fast`, `cargo clippy --workspace --all-targets -j 2 -- -D warnings`, `cargo fmt --all -- --check`, and the 12 JS tests. Record RED/GREEN and executable paths in docs/r16 verification logs; do not set CARGO_TARGET_DIR or call installed MCP tools.
- [x] Inspect each snapshot delta and explain it individually. Existing TSX snapshots remain identical; limits and verification evidence are recorded in docs/r16/REPORT.md.

Final execution order: commit locally, then `cargo clean` (jobs limited through Cargo configuration because clean has no -j flag). Do not push. The commit/clean result is reported in the final response.
