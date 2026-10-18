//! `devup_runtime_audit`: a declarative plan for a browser run the consumer's
//! project executes, and the ingestion of the report that run writes.
//!
//! The server never launches a browser or runs a command. `plan` validates a
//! schema-checked plan - routes, viewports, steps by role and accessible name
//! or test id, geometry checks - and gives it an id derived from its content.
//! The consumer-side executor (`harness/runtime-audit`) runs it in fresh,
//! isolated browser contexts and writes a report with screenshots. `ingest`
//! reads both from the allowed roots and trusts neither blindly: it rebinds the
//! report to the plan by id and by the plan file's hash, recomputes every count
//! from the items, verifies each screenshot's hash, size and decoded width, and
//! says which facts it verified and which only the executor declared.

// Source sections share this module to preserve privacy and test names.
include!("runtime_audit/use_declaration.rs");
include!("runtime_audit/write_plan.rs");
include!("runtime_audit/capture_comparison.rs");
include!("runtime_audit/tests.rs");
