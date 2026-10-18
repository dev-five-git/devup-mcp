//! `devup_project_context` — the ground-truth reader. Reads a project's
//! real `devup.json` (theme tokens), `openapi.json` (endpoints/schemas),
//! and Vespertide `models/*.json` (database tables/columns) so an agent
//! never has to guess what identifiers a project actually has.
//!
//! Every scope reads its target file(s) fresh on every call (no session
//! cache — see `project_root.rs`'s module docs) and, when a target file is
//! missing, returns the shared `{"found":false,"guardrail":{...}}`
//! envelope rather than an empty/ambiguous success.

// Source sections share this module to preserve privacy and test names.
include!("project_context/use_declaration.rs");
include!("project_context/tests.rs");
