//! Shipping rules accompany tool use even when a client never installs skills.
//!
//! The reply carries identifiers only. What each one requires, what to do
//! next and where the installed copy lives are in the linked guide, under
//! "Contract identifiers": the export reply has a byte budget, and prose
//! repeated in every reply is what spent it.
use serde_json::{Value, json};

#[cfg(test)]
const RULES: &str = include_str!("skills/devfive-frontend/references/delivery-quality.md");
#[cfg(test)]
const MOTION_RULES: &str = include_str!("skills/devfive-frontend/references/animation.md");

pub(super) const REQUIREMENTS: &[&str] = &[
    "pure-tsx-sidecar-provenance",
    "jsx-theme-tokens-and-axis-shorthands",
    "single-responsive-tree-and-source-text-wrapping",
    "persistent-server-layout",
    "route-owned-page-body",
    "devup-api-only",
    "stop-affected-api-gaps",
    "preserve-instance-sizing-and-paint",
    "paired-e2e-and-native-figma-visuals",
    "semantic-native-controls-and-complete-pseudo-states",
    "field-focus-on-visual-container",
    "variant-palette-consistency",
    "single-source-url-content-selection",
    "viewport-footer-shell",
    "motion-candidates-and-safe-progressive-enhancement",
    "default-entrance-and-transition-motion",
    "devup-ui-static-keyframes-motion",
    "one-shot-reveals-without-ssr-flash",
    "canonical-semantic-assets",
    "landing-detail-api-db-single-source",
    "user-approved-plan-before-implementation",
    "runtime-evidence-before-completion",
    "offer-devup-retro-in-final-report",
];

pub(super) const COMPLETION_EVIDENCE: &[&str] = &[
    "both-control-directions-and-history",
    "all-widths-and-zoom-context",
    "computed-style-and-animation-intermediate-state",
    "animation-start-middle-end-frames",
    "motion-trigger-reentry-history-and-image-failure",
    "stagger-total-and-progress-sync-timing",
    "changed-empty-and-failing-api",
    "all-public-assets-and-persisted-url-references",
];

pub(super) fn attach(result: &mut Value) {
    if !result.is_object() { return; }
    result["developmentContract"] = json!({
        "version": 6,
        "guide": "devup://guide/usage#behavior-and-data-contracts",
        "alwaysApplies": true,
        "requirements": REQUIREMENTS,
        "completionEvidence": COMPLETION_EVIDENCE,
        "staticExportProvesApplicationBehavior": false,
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
        assert_eq!(result["developmentContract"]["version"], 6);
        let requirements = result["developmentContract"]["requirements"].as_array().unwrap();
        for required in ["route-owned-page-body", "devup-api-only", "stop-affected-api-gaps", "paired-e2e-and-native-figma-visuals"] {
            assert!(requirements.contains(&json!(required)));
        }
        assert!(requirements.contains(&json!("offer-devup-retro-in-final-report")));
        for term in ["Final report: offer the retrospective", "Ask once per completed task", "Never run it
yourself", "carries no offer", super::super::skills::RETRO_OFFER_FIELD] {
            assert!(RULES.contains(term), "missing completion offer: {term}");
        }
        for term in ["keep-all", "grayscale", "layout.tsx", "count", "footer", "DB", "aria-current", "sidecar"] {
            assert!(RULES.contains(term), "missing delivery policy: {term}");
        }
        // Motion is a default item, each part under its own identifier: the
        // candidates the plan infers, the default entrance/transition pass,
        // the static-keyframes implementation and the one-shot/no-flash rule.
        for required in [
            "motion-candidates-and-safe-progressive-enhancement",
            "default-entrance-and-transition-motion",
            "devup-ui-static-keyframes-motion",
            "one-shot-reveals-without-ssr-flash",
        ] {
            assert!(requirements.contains(&json!(required)), "{required}");
        }
        let evidence = result["developmentContract"]["completionEvidence"].as_array().unwrap();
        for required in [
            "computed-style-and-animation-intermediate-state",
            "animation-start-middle-end-frames",
            "motion-trigger-reentry-history-and-image-failure",
            "stagger-total-and-progress-sync-timing",
        ] {
            assert!(evidence.contains(&json!(required)), "{required}");
        }
        for (document, terms) in [
            (RULES, &["without being asked", "plays once", "opacity: 0", "prefers-reduced-motion", "back/forward", "failed images", "keyframes", "animationTimeline"][..]),
            (MOTION_RULES, &["user stated", "keyframes", "animationTimeline=\"view()\"", "animationRange", "animationFillMode", "_motionReduce", "_focusWithin", "IntersectionObserver", "data-reveal", "framer-motion", "stagger", "background", "Progress line", "hydration", "JavaScript disabled", "focus", "start, a middle and the end", "Trigger timing", "Fast scroll", "back/forward", "block one", "unverified"][..]),
        ] {
            for term in terms {
                assert!(document.contains(term), "missing motion policy: {term}");
            }
        }
    }

    /// The reply names identifiers; the guide must say what each one means,
    /// and carry the standing text that used to ride in every reply.
    #[test]
    fn every_contract_identifier_is_explained_in_the_guide() {
        for id in REQUIREMENTS.iter().chain(COMPLETION_EVIDENCE) {
            assert!(RULES.contains(&format!("`{id}`")), "the guide does not explain {id}");
        }
        // Prose is wrapped; compare it as a reader does, without line breaks.
        let guide = super::super::guide::GUIDE.split_whitespace().collect::<Vec<_>>().join(" ");
        for moved in [
            // developmentContract.nextAction and installedReference.
            "fix and rerun functional E2E plus native Figma visuals before claiming completion",
            "devfive-frontend/references/delivery-quality.md",
            // behaviorPlan intentPolicy, clientBoundary, sourcePurity, contextIndex.
            "explicit user requirements take precedence",
            "mount the supplied tiny shared motion controller in the persistent layout",
            "node identities and implementation evidence stay in JSON sidecars",
            "candidate inference and returned rows are bounded per page",
            // server.updateAvailable.autoUpdate.installedPathNote.
            "self-update replaces the file inside it and never renames the directory",
            // verificationScope's three explanations.
            "each generated property against its source field, at the collected widths",
            "read from constraints and geometry, not rendered",
            "devup_visual_compare compares a consumer-rendered PNG at a drawn width",
        ] {
            assert!(guide.contains(moved), "moved out of the reply but not into the guide: {moved}");
        }
        // No identifier is listed twice, and none was dropped by a rename.
        let mut unique: Vec<_> = REQUIREMENTS.iter().chain(COMPLETION_EVIDENCE).collect();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), REQUIREMENTS.len() + COMPLETION_EVIDENCE.len());
        assert_eq!((REQUIREMENTS.len(), COMPLETION_EVIDENCE.len()), (23, 8));
    }
}
