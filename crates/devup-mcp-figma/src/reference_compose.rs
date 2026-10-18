//! referencePng over the bridge where other layers overlap the node.
//!
//! `get_screenshot` draws the node in its ancestors' context - their paints,
//! clipping, opacity, blend modes, layer blur and drop shadow - with every
//! other layer left out. A plugin cannot leave a layer out of an export, so it
//! exports the node alone, describes its ancestors (and sends the images their
//! fills use), and this draws the rest. Measured live on the devup Test file
//! against `get_screenshot`: gradient backdrops, a translucent, blurred and
//! shadowed ancestor, a node with its own background blur over a layer behind
//! it, a uniform backdrop with a layer above; each within 2/255 on at least
//! 99.6% of pixels, the rest on text edges and the clipped rim of a blurred
//! ancestor.

// Source sections share this module to preserve privacy and test names.
include!("reference_compose/types_and_helpers.rs");
include!("reference_compose/impl.rs");
include!("reference_compose/tests.rs");
