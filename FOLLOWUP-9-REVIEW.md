Follow-up 9 decisions and review

- Runtime reads Jira/Figma/spec connectors and passes source-labelled facts; the server reads local models. Commands are returned for runtime execution, never launched by the planning tool.
- API models are explicit vespertide wire objects; signup/post evidence seeds a reviewed draft. Other domains require explicit complete entities. Existing source models are preserved unless explicitly revised.
- Jira work uses supplied issue dependencies; otherwise shared foundations precede feature layer chains. Supervisor must sequence cross-feature model changes. Briefs retain the complete shared ERD and screen plan to avoid losing cross-feature context.
- Intake always asks the four required first-round questions, using source facts as defaults. Refactor is available as a custom work-type answer to preserve the runtime's four-option limit.
- MCP initialize trimmed repeated export sizing/connection prose to leave space for project intake; full guide retains the detailed rules, and the 1500-byte cap remains enforced.
- Review: planning persists commands and verification evidence; completion requires the executing runtime's actual checks, not a generated plan. Unit status is supervisor-reported. Existing screen-plan export gate remains authoritative for Figma page writes.

- jsonschema 0.59 enables serde_json float_roundtrip; two diagnostic snapshot floats change only their final decimal digit. The snapshot is updated for the more accurate parsing under workspace feature unification.

Validation: cargo fmt --all --check and cargo clippy --workspace --all-targets -- -D warnings passed. cargo test --workspace --no-fail-fast completed with only symlink_escape_refused_before_read and rejects_a_directory_symlink_parent_that_escapes_the_root (Windows os error 1314). Test-list baseline after follow-up 9: 1644 named tests.
