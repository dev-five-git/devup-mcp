# R3 assets and placement evidence implementation plan

Goal: implement the approved R3 brief, preserving 0.4.2 output recovery.
Spec: C:/Users/owjs3/orca/devup-mcp-briefs/R3-asset-batching-and-placement-evidence.md
Architecture: bounded asset jobs retain the collector across client timeouts; evidence is collected alongside generated source; fidelity stays scoped to each output. Rust/Tokio/serde/rmcp. Execute inline in the existing isolated worktree.

- [x] Add failing R3 regressions: 16 assets rejected before upstream with full split requests; split batches export; paused job resumes without repeating accepted calls; numeric placement evidence; responsive output report.
- [x] validation.rs/tools.rs/mod.rs: recommend 1–3 assets, cap 6, publish schema and split details before acquisition.
- [x] asset_jobs.rs/mod.rs/collector.rs: bounded in-memory jobs, immediate recoverable ID after a short wait, status/resume through export, per-call and per-asset progress, retain pending collector call on timeout. Identical requests recover a job after lost replies.
- [x] codegen/evidence.rs/component.rs: expose node and parent geometry, coordinate basis, constraints, transforms, sizing, padding, missing-source reasons; enrich mask boolean fallback too. Keep conservative approximation grade.
- [x] projection.rs/result_contract.rs: preserve existing tsx/componentTsx isolation; add responsive result with explicitly scoped breakpoint fidelity and merged loss evidence.
- [x] Document contracts and actual WQUW-120 findings; add .changepacks entry directly.
- [x] Run cargo test --workspace -j 2 --no-fail-fast, clippy --workspace --all-targets -j 2 -- -D warnings, fmt --all -- --check, build --workspace -j 2. No CARGO_TARGET_DIR; retain logs outside target, verify executable paths.
- [x] Review diff and prepare committed handoff. Run cargo clean immediately after commit; preserve its final log outside target and update Orca/final report.
