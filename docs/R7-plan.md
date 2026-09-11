# R7 implementation and verification plan

Brief: C:/Users/owjs3/orca/devup-mcp-briefs/R7-main-axis-and-error-identity.md

Authorized execution in this worktree; external WQUW evidence is read-only.

1. Add r7_layout regression tests and observe failures in local target. Extend provenance/sizing.rs implicit CSS verification for horizontal flex allocation, retaining conservative rejection reasons and revalidation. Correct horizontal-parent vertical FILL output only after reproducing missing percentage basis. Test wrong gap, constraints, sibling uncertainty and forged mapping.
2. Add error identity and plugin recovery regression tests, observe failures, then share success/error identity and carry collection context plus SECTION ancestor/recovery arguments through plugin errors.
3. Add selection reuse/debug resource guidance and explicit unverified font-metrics tests, observe failures, implement minimal changes.
4. Run cargo test --workspace -j 2 --no-fail-fast, cargo clippy --workspace --all-targets -j 2 -- -D warnings and cargo fmt --all -- --check. Inspect each golden difference. Record local target/executable provenance and red/green results.
5. Commit without push, then cargo clean (this non-build subcommand has no -j option) and update Orca comment. Do not set CARGO_TARGET_DIR or use installed MCP for validation.
