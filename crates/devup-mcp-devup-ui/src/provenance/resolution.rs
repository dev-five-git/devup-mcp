//! Public interpretation of legacy mapping labels, independent of fidelity grades.
use serde_json::{Value, json};

pub fn resolution_semantics() -> Value {
    json!({
        "axis":"mapping-method",
        "verificationAxis":"diagnostics.details.components.*.state",
        "relation":"Independent axes; join nodeId/property in projectionIssues/projectionEvidence within the same screen/output.",
        "values":{
            "exact":"Tag/text mapping.",
            "raw-fallback":"Value/policy mapping, not fidelity; ABSOLUTE can separately be verified.",
            "derived-lone-child-center":"Horizontal/vertical SPACE_BETWEEN becomes center for exactly one visible non-ABSOLUTE child; child membership, visibility and positioning determine the count. This mapping does not claim pixel parity.",
            "derived-single-edge-outside-stroke":"One solid, square-cornered OUTSIDE edge on a non-asset auto-layout node paints as a zero-blur translated box shadow without consuming layout space; side weights and stroke paint determine the translation and color. Existing effects are composed after the stroke. This is a stroke projection, not an assertion that Figma supplied a shadow effect, a claim about exported-asset composition, or a claim of pixel parity.",
            "derived-hard-break-whitespace":"Source spaces adjacent to explicit line breaks select pre-wrap for non-list, unclamped text whose inline width is fixed and height is automatic. Characters come from the node or collected styled segments. This preserves source whitespace without changing Korean keep-all; it does not claim intrinsic HUG sizing, glyph parity or identical wrapping.",
            "verified-explicit-dimension":"Error-free source = emitted px.",
            "verified-layout-sizing":"FIXED/FILL/layoutGrow to dimension/flex.",
            "accounted-for-content-sizing":"textAutoResize omission; pixels unmeasured.",
            "accounted-for-implicit-flex-stretch":"Emitted parent cross-axis stretch.",
            "accounted-for-implicit-flex-grow":"Emitted parent main-axis allocation.",
            "restored-hug-after-mask-child-folding":"Folded-mask geometry restores HUG.",
            "restored-hug-after-mask-child-folding-from-absoluteBoundingBox":"HUG from absoluteBoundingBox.",
            "variable-token":"Variable token.",
            "style-token":"Style token.",
            "asset":"Asset prop; binary state separate.",
            "variant-selector":"Variant to selector.",
            "unverified-property-mapping":"Generated property unproven.",
            "variable":"Variable to JSON pointer.",
            "alias":"Resolved alias to JSON pointer.",
            "style":"Style to JSON pointer.",
            "node":"Internal; excluded from public entries."
        }
    })
}
