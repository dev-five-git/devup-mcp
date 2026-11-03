//! Shipping rules accompany tool use even when a client never installs skills.
use serde_json::{Value, json};

#[cfg(test)]
const RULES: &str = include_str!("skills/devfive-frontend/references/delivery-quality.md");

pub(super) fn attach(result: &mut Value) {
    if !result.is_object() { return; }
    result["developmentContract"] = json!({
        "version": 1,
        "guide": "devup://guide/usage#behavior-and-data-contracts",
        "installedReference": "devfive-frontend/references/delivery-quality.md",
        "alwaysApplies": true,
        "requirements": [
            "pure-tsx-sidecar-provenance", "jsx-theme-tokens-and-axis-shorthands",
            "single-responsive-tree-and-korean-word-boundaries", "persistent-server-layout",
            "semantic-native-controls-and-complete-pseudo-states", "variant-palette-consistency",
            "single-source-url-content-selection", "viewport-footer-shell",
            "motion-candidates-and-safe-progressive-enhancement", "canonical-semantic-assets",
            "landing-detail-api-db-single-source", "runtime-evidence-before-completion"
        ],
        "completionEvidence": ["both-control-directions-and-history", "all-widths-and-zoom-context",
            "computed-style-and-animation-intermediate-state", "changed-empty-and-failing-api",
            "all-public-assets-and-persisted-url-references"],
        "staticExportProvesApplicationBehavior": false,
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
        for term in ["keep-all", "grayscale", "layout.tsx", "count", "footer", "DB", "aria-current", "sidecar"] {
            assert!(RULES.contains(term), "missing delivery policy: {term}");
        }
    }
}
