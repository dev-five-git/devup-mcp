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
