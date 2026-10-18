//! `devup_stack_diff` — cross-layer drift detection across the devup
//! stack (`vespertide model -> sea-orm entity -> vespera route ->
//! openapi.json -> @devup-api client`). This is the one ground-truth tool
//! that cannot be reduced to "read one file and report its contents": it
//! compares authored sources with derived layers that a human reviewer would
//! normally have to cross-reference by hand.
//!
//! Every check here is text/JSON-based, not a real compiler front end for
//! Rust or TypeScript. That is a deliberate, disclosed limitation, not an
//! oversight: extraction can miss macro-generated routes (e.g.
//! `vespera::export_app!`-merged sub-apps), non-standard formatting, or
//! re-exported client wrappers. Every reported drift and every skipped
//! layer carries an explicit `confidence` (`"low"` or `"medium"`) — never
//! `"high"`, since a bounded parse cannot prove scan completeness — and the tool
//! never claims a clean layer is drift-free with unwarranted certainty;
//! see each layer's doc comment for exactly what it can and cannot see.

// Source sections share this module to preserve privacy and test names.
include!("stack_diff/use_declaration.rs");
include!("stack_diff/openapi_client_layer.rs");
