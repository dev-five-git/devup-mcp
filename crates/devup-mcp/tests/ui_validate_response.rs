//! What `devup_ui_validate`'s response says about itself.
//!
//! The MCP keys are assembled by hand in the tool handler rather than
//! serialized from `UiValidation`, so what that struct's documentation
//! explains reaches nobody unless the response answers it too. Two
//! readings went wrong in practice: `ok: true` beside ten violations read
//! as a contradiction, and `checkedTokens: 4` beside
//! `availableTokenCount: 81` read as "4 of 81 checked". These lock the
//! response to the meaning `UiValidation` documents.

// Source sections share this module to preserve privacy and test names.
include!("ui_validate_response/use_declaration.rs");
include!("ui_validate_response/a_page_named_module_outside_the_router_is_not_judged_as_a_route.rs");
