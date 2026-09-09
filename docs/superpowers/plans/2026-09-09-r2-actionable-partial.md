# R2 actionable partial results implementation plan

Goal: restore usable production TSX and explain every projection loss with generated evidence.
Spec: C:/Users/owjs3/orca/devup-mcp-briefs/R2-make-partial-actionable.md
Architecture: reproduce through the checkout-local server and retain raw payloads; fix the proven asset policy mismatch before changing result presentation. Use existing source maps and projection traces to expose generated evidence without claiming visual equivalence.
Tech stack: Rust workspace, serde_json, existing TSX parser and provenance.

- [x] R2-1: replay the six exact frame arguments with the local binary, save rawPayload and pre-guard generated code, distinguish hidden-policy mismatch from path false positive. Add a payload regression in server/projection.rs asserting six TSX strings, syntax validity, and no invalid asset reference. Run red, implement the confirmed fix, run green.
- [x] R2-2: add tests for withheld outputs and partial deliverable metadata. Expose output failures as projectionIssues with property, before/after, reason and next action; always expose deliverable for requested code. Document relationship to failures in docs/r1-result-contract.md.
- [x] R2-3: test actual WQUW-118 and WQUW-120 output evidence. Use node/source ranges and ancestor replacement evidence in codegen/component.rs; label each diagnostic output, keep per-output fidelity, and group review items by output and affected owner with counts and deterministic priority. Retain conservative loss grades.
- [x] Run cargo test --workspace -j 2 --no-fail-fast, cargo clippy --workspace --all-targets -j 2 -- -D warnings, cargo fmt --all -- --check, cargo build --workspace -j 2. Never set CARGO_TARGET_DIR. Save logs outside target and record test executable paths.
- [x] Review diff and evidence, add .changepacks/changepack_log_r2_actionable_partial.json, record external NEW-FINDINGS-R2.md.

Final commit, cargo clean and Orca status evidence is recorded outside target in C:/Users/owjs3/orca/devup-mcp-briefs/R2-VERIFICATION.md.

Execution: inline in this already-authorized worktree, R2-1 root cause is the gate for R2-2/R2-3 implementation.
