# R13 implementation plan

Spec: `C:/Users/owjs3/orca/devup-mcp-briefs/R13-postprocess-provenance.md` and the four original reports plus F13-1 calls.json (read only in girok-space).

## Audit before implementation

| Stage | Code | Position relative to map | Required treatment |
| --- | --- | --- | --- |
| Layout, asset boundary offsets/sizes, style/effects, text, animation, asset layout filtering | codegen/{component,layout,style,text,animation}.rs | Before finalize_tsx | Audit actual final JSX attributes against map and stage derivation evidence. F13-1 is omitted reverse-map coverage, not lost TSX. |
| Function/import wrapping and indentation, component empty Box sizing, instance dimension insertion/usage comment | codegen/component.rs | Before finalize_codegen_output | Final AST audit; unknown derivations must be diagnosed. |
| Variant selector/tree merging and conditional attributes | codegen/variant.rs | Before ordinary finalizer | Final AST audit including expressions and spreads. |
| Responsive tree/array merge, resets and module wrapper | codegen/responsive.rs; server/projection.rs | Bypasses ordinary map finalizer | Explicit missing merged-provenance diagnostic; source breakpoint maps alone do not prove merged props. |
| Marker removal | provenance::finalize_tsx / strip_markers | Creates private ranges during removal | Preserve map generation and semantic property freezing. |
| Host-safe filename conversion, collected/deduplicated/public asset URL reconciliation | server/projection.rs::rewrite_result_asset_references | After map and quality creation | Rewrite semantic maps and evidence with code, then audit delivered attributes. |
| Excluded-asset output withholding | server/projection.rs | After codegen | Existing explicit failure, preserve. |
| Resource extraction and disk serialization | server/projection.rs::apply_delivery | After rewriting | Transport final code and matching evidence; no code transformation. |
| ui_validate correction preview | server/validation_guidance.rs | Separate user-source validation operation, no export map | Outside generated export pipeline. |

## Execution

- [x] R13-1: Add original raw snapshot fixture and failing AST attribute coverage regression. Add mutation coverage for new attributes, changed values, multiple outputs, and responsive bypass. Implement reusable parser audit, computed stage evidence, and non-lossy mapping-incomplete quality. Preserve generated values and R6–R12 classifications.
- [x] R13-2: Fail tests for generic and unknown path keys, frame keys and actual bytes. Implement explicit supported keys, diagnostics, and transactional frame writes.
- [x] R13-3: Fail explicit resource nextAction regression. Provide same-artifact one-screen one-output arguments matching R12 recovery, bounded size information without claiming measured reprojected wire bytes.
- [x] Run workspace tests and clippy with `-j 2`, fmt check and 12 JS tests. Verify local target/test executables, document each changed golden individually if any.
- [x] Review completed. No CARGO_TARGET_DIR changes and no installed MCP validation.
- Final handoff: commit, then cargo clean; no push. Completion and commit ID are recorded in the final worktree comment.

SVG/CSS browser composition and pixel equivalence are not measured in this work.
