//! Known `@devup-ui/react` primitive prop names, taken from devup-ui's own
//! source rather than invented: the keys of every `ResponsiveValue` prop in
//! `packages/react/src/types/props/*.ts` and every alias in
//! `libs/css/src/constant.rs`, at devup-ui 1.0.41 (e407524), plus the
//! entries the published Style Props API reference listed before that. The
//! docs page alone was 409 props short — `maskPos`, `bgPos`,
//! `WebkitLineClamp`, `visibility`, `zoom` among them — and the plugin writes
//! `maskPos` on every mask icon, so `devup_ui_validate` was rejecting real
//! devup-ui code as `unknown-prop`. `devup_ui_validate`'s `unknown-prop`
//! rule checks JSX attributes on the primitive elements it recognizes
//! (`Box`, `Flex`, `Text`, `Center`, `Grid`, `Image`) against this list plus
//! a small set of standard React/HTML/devup-ui-specific non-style props;
//! anything else is flagged rather than guessed at.
//!
//! `DEVUP_COLOR_LIKE_PROPS` / `DEVUP_LENGTH_LIKE_PROPS` are the subsets
//! whose CSS-property counterpart is a single color or length value; they
//! drive the `hardcoded-color` / `hardcoded-length` / `unknown-token`
//! rules. Deliberately conservative: composite props like `background`
//! (can hold a gradient) or `border` (shorthand for width+style+color) are
//! excluded from both subsets rather than guessed at, since flagging them
//! incorrectly would repeat exactly the fabrication failure this tool
//! exists to prevent.

// Source sections share this module to preserve privacy and test names.
include!("style_props/devup_primitive_elements.rs");
include!("style_props/tests.rs");
