# R8 execution

1. Reproduce WQUW-120 saved offsets; follow the supervisor revision to remove public offsets. Preserve node/field/generatedProperty/resolution mappings and four sizing proofs; measure actual response size. Keep private validator bookkeeping and generatedSource excerpts.
2. Extend R6 sizing on the absolute FIXED-height path. Test HUG/FILL exclusions and height provenance before removing the derived-padding exemption.
3. Measure local 3-frame projection and acquisition boundaries. Add bounded resumable collection with stage/call timings and frame-output budget.
4. Test expired versus unknown artifacts; retain bounded recovery metadata and return canonical URL/selection when known.
5. Execute capture script against scroll fixtures, add overflowDirection to manifest and distinguish absent/capture failure. Test scroll axes and clipping.
6. Run workspace tests, clippy -j 2, fmt; explain each golden delta. Commit without push, cargo clean, update Orca.

External evidence files are read-only. All Rust executions use this checkout target; CARGO_TARGET_DIR is never set.
