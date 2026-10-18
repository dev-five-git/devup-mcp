//! `devup_ui_validate` — the highest-leverage of the three ground-truth
//! tools. Parses TSX with the same `oxc_parser`/`oxc_allocator`/`oxc_span`
//! stack already used to validate every generated TSX (`validation.rs`),
//! then walks the AST with `oxc_ast_visit::Visit` to catch the exact
//! failure class documented in this repository's brief: three agents
//! independently inventing `$gray100` (a color token that does not exist
//! in the project's real `devup.json`), a 16px bubble radius, and a 36px
//! avatar size, none traceable to any source of truth.
//!
//! Two facts verified against `@devup-ui/react`'s own docs and ESLint rule
//! (`css-utils-literal-only`) shape the rules here and deliberately
//! *narrow* what the brief's "런타임 값" wording might suggest:
//!
//! - JSX style props on `Box`/`Flex`/`Text`/... (`bg={dynamicValue}`) ARE
//!   valid devup-ui: the compiler lowers them to a CSS custom property at
//!   build time (`className="a" style={{"--a": dynamicValue}}`). Flagging
//!   these as errors would itself be a fabricated rule.
//! - `css()`, `globalCss()`, and `keyframes()` utility calls are the actual
//!   "must be statically analyzable" boundary — devup-ui's own
//!   `css-utils-literal-only` ESLint rule rejects variables/expressions
//!   there, because these calls are extracted at build time with no
//!   runtime fallback. `runtime-value` therefore targets these three call
//!   sites, not general JSX props.
//!
//! `unknown-token` / `hardcoded-color` / `hardcoded-length` operate on the
//! `Box`/`Flex`/`Text`/`Center`/`Grid`/`Image` primitives' known color- and
//! length-like props (`style_props.rs`, itself sourced from devup-ui's
//! published Style Props API reference, not invented).
//!
//! The two `hardcoded-*` rules always report literals. Exact token matches
//! are warnings with actionable token advice; unmatched values are info
//! without token suggestions. Empty token categories are summarized once
//! in `themeNotes`, avoiding repeated, impossible advice (D11).

// Source sections share this module to preserve privacy and test names.
include!("ui_validate/use_declaration.rs");
include!("ui_validate/tests.rs");
