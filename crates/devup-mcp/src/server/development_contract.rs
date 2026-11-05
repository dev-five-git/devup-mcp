//! Shipping rules accompany tool use even when a client never installs skills.
use serde_json::{Value, json};

#[cfg(test)]
const RULES: &str = include_str!("skills/devfive-frontend/references/delivery-quality.md");

pub(super) fn attach(result: &mut Value) {
    if !result.is_object() { return; }
    result["developmentContract"] = json!({
        "version": 2,
        "guide": "devup://guide/usage#behavior-and-data-contracts",
        "installedReference": "devfive-frontend/references/delivery-quality.md",
        "alwaysApplies": true,
        "requirements": [
            "pure-tsx-sidecar-provenance", "jsx-theme-tokens-and-axis-shorthands",
            "single-responsive-tree-and-source-text-wrapping", "persistent-server-layout",
            "route-owned-page-structure-and-data", "devup-api-for-all-application-requests",
            "stop-affected-work-on-missing-devup-api-capability",
            "preserve-caller-sizing-and-painted-empty-nodes",
            "functional-e2e-and-figma-reference-visual-together",
            "semantic-native-controls-and-complete-pseudo-states", "variant-palette-consistency",
            "single-source-url-content-selection", "viewport-footer-shell",
            "motion-candidates-and-safe-progressive-enhancement", "canonical-semantic-assets",
            "landing-detail-api-db-single-source", "runtime-evidence-before-completion"
        ],
        "completionEvidence": ["both-control-directions-and-history", "all-widths-and-zoom-context",
            "computed-style-and-animation-intermediate-state", "changed-empty-and-failing-api",
            "all-public-assets-and-persisted-url-references"],
        "staticExportProvesApplicationBehavior": false,
        "pageOwnership": "Actual page structure and data preparation belong in app/**/page.tsx. components/pages/<page>/ is for page-local partials; no whole-page delegation through wrappers or fragments.",
        "apiPolicy": "Every application API call uses devup-api, including server reads, actions and uploads. Update/reinstall and reproduce before declaring a gap. Document a concrete feature request and stop affected work if still unsupported; continue independent work. No substitute transport or externally posted request without authorization.",
        "visualPolicy": "Run functional E2E and Figma reference visual tests together for UI changes. Expected screenshots must come from Figma, never the implementation under test. Cover all changed sections and captured states; diagnosis alone does not complete an authorized fix.",
        "nextAction": "Apply this contract and behaviorPlan when wiring generated TSX. Resolve each required binding and test its runtime behavior; report unverified work explicitly."
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contract_is_present_independently_of_installed_skills() {
        let mut result = json!({"found": true});
        attach(&mut result);
        assert_eq!(result["developmentContract"]["alwaysApplies"], true);
        assert_eq!(result["developmentContract"]["staticExportProvesApplicationBehavior"], false);
        assert_eq!(result["developmentContract"]["version"], 2);
        assert!(result["developmentContract"]["apiPolicy"].as_str().unwrap().contains("devup-api"));
        assert!(result["developmentContract"]["pageOwnership"].as_str().unwrap().contains("app/**/page.tsx"));
        assert!(result["developmentContract"]["visualPolicy"].as_str().unwrap().contains("never the implementation"));
        for term in ["keep-all", "grayscale", "layout.tsx", "count", "footer", "DB", "aria-current", "sidecar"] {
            assert!(RULES.contains(term), "missing delivery policy: {term}");
        }
    }
}
