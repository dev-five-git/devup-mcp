//! Image bytes and the layout painted with those bytes are separate contracts.
//! Figma's Paint API defines imageTransform for CROP, rotation for FILL/FIT,
//! and paint order/blending: https://developers.figma.com/docs/plugins/api/Paint/

// Source sections share this module to preserve privacy and test names.
include!("image/image_projection.rs");
include!("image/project_tree.rs");
