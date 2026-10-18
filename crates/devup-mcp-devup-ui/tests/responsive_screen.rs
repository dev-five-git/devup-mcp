//! The whole join, run on a real screen.
//!
//! `fixtures/plugin-answers/notice/responsive.tsx` is what the plugin answered
//! for this screen, and the capture it was answered from is
//! `fixtures/local-screens/bp-family.json`. The capture is not committed, so
//! these skip when it is absent rather than pretending to have checked.

// Source sections share this module to preserve privacy and test names.
include!("responsive_screen/use_declaration.rs");
include!("responsive_screen/every_element_the_module_draws_is_one_it_imports.rs");
