//! Per-state and per-requirement verification matrix for `devup_feature_trace`.
//!
//! Pure verdict logic over caller evidence, the cached Figma snapshot and the
//! static UI inventory. It never launches a browser. Missing design, missing
//! capture, static-only evidence and incomplete Figma data are `unverified`,
//! never a pass, and they block design-match and completion claims.

// Source sections share this module to preserve privacy and test names.
include!("verification_matrix/types.rs");
include!("verification_matrix/style.rs");
include!("verification_matrix/states_and_gaps.rs");
include!("verification_matrix/requirements_and_report.rs");

#[cfg(test)]
#[path = "verification_matrix_tests.rs"]
mod tests;
