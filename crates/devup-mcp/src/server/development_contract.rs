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
            "route-owned-page-body", "devup-api-only",
            "stop-affected-api-gaps",
            "preserve-instance-sizing-and-paint",
            "paired-e2e-and-native-figma-visuals",
            "semantic-native-controls-and-complete-pseudo-states", "variant-palette-consistency",
            "single-source-url-content-selection", "viewport-footer-shell",
            "motion-candidates-and-safe-progressive-enhancement", "canonical-semantic-assets",
            "landing-detail-api-db-single-source", "runtime-evidence-before-completion"
        ],
        "completionEvidence": ["both-control-directions-and-history", "all-widths-and-zoom-context",
            "computed-style-and-animation-intermediate-state", "changed-empty-and-failing-api",
            "all-public-assets-and-persisted-url-references"],
        "staticExportProvesApplicationBehavior": false,
        "nextAction": "Read the linked rules, apply behaviorPlan, fix and rerun functional E2E plus native Figma visuals before claiming completion."
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
        let requirements = result["developmentContract"]["requirements"].as_array().unwrap();
        for required in ["route-owned-page-body", "devup-api-only", "stop-affected-api-gaps", "paired-e2e-and-native-figma-visuals"] {
            assert!(requirements.contains(&json!(required)));
        }
        for term in ["keep-all", "grayscale", "layout.tsx", "count", "footer", "DB", "aria-current", "sidecar"] {
            assert!(RULES.contains(term), "missing delivery policy: {term}");
        }
    }
}
