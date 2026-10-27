//! `devup_screen_plan`: which Figma screen becomes which URL, decided and
//! confirmed before any page is written.
//!
//! An agent handed "implement this design" used to export and write pages
//! straight away. Afterwards nobody could say which Figma screen had become
//! which URL, so nobody could say what had been built or whether it worked:
//! on one site the pages rendered "This page couldn't load" at some widths and
//! were cut off on the right at others, and there was no URL-by-width table on
//! which anyone could have noticed.
//!
//! So the order is fixed. `draft` maps every screen to a route from evidence
//! only - an existing page built from the frame, a nav label already bound to
//! an href, an explicit `navigationTargets` entry or the project's config -
//! and turns everything it cannot decide into questions shaped for the
//! runtime's question tool, split into rounds that are all asked up front, back
//! to back. A guess from a layer name is never silent: it is marked
//! `name-match` and put to the user in the same rounds. `confirm` takes every
//! answer that applies at once and refuses to proceed with any missing. After
//! it nothing is asked: implementation records what it decided alone with
//! `assume`. `status` reads the confirmed plan against the routes on disk and a
//! runtime audit report, one row per URL and width, where a width nobody ran is
//! `not-run` and never `pass`.
//!
//! Nothing here reaches Figma or runs a browser. The screen list comes from
//! `devup_figma_explore` (or the caller, from an earlier explore), the routes
//! from `devup_project_context`, the links from the navigation parser and the
//! widths from `devup_runtime_audit`.

// Source sections share this module to preserve privacy and test names.
include!("screen_plan/state.rs");
include!("screen_plan/routes.rs");
include!("screen_plan/app_select.rs");
include!("screen_plan/draft.rs");
include!("screen_plan/actions.rs");
include!("screen_plan/audit_base_url.rs");
include!("screen_plan/remap.rs");
include!("screen_plan/status.rs");
include!("screen_plan/draft_plan_with.rs");
include!("screen_plan/plan_route_group.rs");
include!("screen_plan/state_variants.rs");
include!("screen_plan/lookup.rs");
include!("screen_plan/component_width.rs");
include!("screen_plan/responsive_contract.rs");
