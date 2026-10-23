//! `devup_screen_plan action: "auditPlan"`: one runtime-audit plan per
//! confirmed URL that checks everything the Figma frames can be compared
//! with - page health at every width, the page, each section and each asset
//! against their Figma renders at the drawn widths, size and position of
//! every section, card and asset, containment and clipped text between the
//! widths, interaction states, and the view transition to the next URL.
//!
//! A plan per URL, because a page comparison needs the viewport as tall as
//! that URL's frame, and because the work goes URL by URL.

// Source sections share this module to preserve privacy and test names.
include!("audit_plan/planning_and_capture.rs");
include!("audit_plan/static_params.rs");
include!("audit_plan/prerequisites.rs");
