mod animation;
mod compat;
mod component;
mod evidence;
mod image;
pub mod instance_overrides;
mod layout;
pub(crate) use layout::absolute_border_origin_calculation;
mod overlay_anchors;
pub use overlay_anchors::overlay_anchor_metadata;
pub mod node_ids;
mod property_derivations;
mod semantics;
pub(crate) use property_derivations::property_derivations;
pub mod responsive;
mod style;
mod text;
mod variant;
pub mod width;

pub use compat::{
    extract_custom_component_imports, extract_devup_imports, generate_import_statements,
    render_codegen_provider, render_component_usage, render_responsive_component_mock,
    render_variant_tree_merge, render_viewport_component,
};
pub use component::{
    CodegenOptions, CodegenOutput, LengthToken, RootLayout, generate_component,
    generate_component_set_target, generate_inlined_component_instance, generate_legacy_component,
    generate_node, normalize_component_name, render_component_registration_snapshot,
    render_component_source,
};
pub(crate) use image::horizontal_transferred_image_minimum;
pub use image::{
    AssetPixelSize, AssetRepresentationPolicy, AssetVariant, asset_css_map, validate_asset_effects,
};
pub(crate) use layout::{
    centers_lone_space_between_child, derived_padding, export_box, folded_mask_dimensions,
    layout_box, placed_by_a_free_layout, px, vertical_fill_container,
    vertical_fill_needs_minimum_reset,
};
pub(crate) use style::single_outside_stroke;
pub(crate) use style::{AssetKind, asset_kind};
pub use style::{asset_path, image_fill_path};
pub(crate) use text::{escape_jsx_text, render_segment_characters};
pub(crate) use text::{
    preserves_hard_break_spaces, preserves_intrinsic_text_width, preserves_wrapped_ascii_spaces,
};
