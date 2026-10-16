//! Caller observations rank actual Figma candidates; this is not vision.
use devup_mcp_figma::{DevupError, ErrorCode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[schemars(extend("additionalProperties"=serde_json::json!({"not":{}})))]
pub struct ScreenshotObservations {
    /// Up to 8 visible text labels, each 1..80 characters/320 UTF-8 bytes.
    #[serde(default)]
    #[schemars(extend("maxItems"=8))]
    pub labels: Vec<String>,
    /// Caller-observed whole-screen width/height ratio, never a logical size.
    #[serde(default)]
    pub aspect_ratio: Option<f64>,
}
impl ScreenshotObservations {
    pub fn validate(&self) -> Result<(), DevupError> {
        if self.labels.len() > 8
            || (self.labels.is_empty() && self.aspect_ratio.is_none())
            || self
                .labels
                .iter()
                .any(|s| s.trim().is_empty() || s.chars().count() > 80 || s.len() > 320)
            || self
                .aspect_ratio
                .is_some_and(|n| !n.is_finite() || !(0.01..=100.0).contains(&n))
        {
            return Err(DevupError::new(
                ErrorCode::DevupInvalidInput,
                "screenshotObservations needs 1..8 bounded visible labels or a finite aspectRatio in 0.01..100. Observations are caller-declared, not verified screenshot perception.",
                false,
            ));
        }
        Ok(())
    }
}
pub fn rank(result: &mut Value, hints: Option<&ScreenshotObservations>) {
    let Some(hints) = hints else {
        return;
    };
    let Some(candidates) = result["candidates"].as_array_mut() else {
        return;
    };
    let mut ranked = Vec::new();
    for candidate in candidates.drain(..) {
        let mut candidate = candidate;
        let name = candidate["node"]["name"]
            .as_str()
            .unwrap_or("")
            .to_lowercase();
        let preview = candidate["node"]["textPreview"]
            .as_str()
            .unwrap_or("")
            .to_lowercase();
        let matched = hints
            .labels
            .iter()
            .filter_map(|label| {
                let needle = label.trim().to_lowercase();
                if name.contains(&needle) {
                    Some(json!({"label":label,"field":"name"}))
                } else if preview.contains(&needle) {
                    Some(json!({"label":label,"field":"capturedTextPreview"}))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let ratio = candidate["node"]["bounds"]["width"]
            .as_f64()
            .zip(candidate["node"]["bounds"]["height"].as_f64())
            .filter(|(w, h)| w.is_finite() && h.is_finite() && *w > 0.0 && *h > 0.0)
            .map(|(w, h)| w / h);
        let difference = ratio
            .zip(hints.aspect_ratio)
            .map(|(actual, observed)| (actual / observed - 1.0).abs());
        let aspect_matches = difference.is_some_and(|delta| delta <= 0.1);
        let score = matched.len() * 10 + usize::from(aspect_matches);
        candidate["observationEvidence"] = json!({"matchedLabels":matched,"capturedAspectRatio":ratio,"relativeAspectDifference":difference,
            "rankScore":score,"state":if score>0 {"observation-candidate"} else {"no-observation-match"},"visionVerified":false});
        ranked.push((score, candidate));
    }
    ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    *candidates = ranked.into_iter().map(|(_, candidate)| candidate).collect();
    result["screenshotGuidance"] = json!({"source":"caller-declared-screenshot-observations","observations":hints,
        "rankingBasis":"literal captured names/text previews and geometric aspect ratio; stable original order breaks ties",
        "evidenceLimit":"No screenshot pixels or visual features were inspected. Text previews can be partial; matching observations only nominates real candidates and never verifies or automatically exports a selection."});
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observations_only_rank_real_candidates_and_disclose_no_pixel_verification() {
        let mut result = json!({"candidates":[{"node":{"nodeId":"1:1","name":"Other","textPreview":"","bounds":{"width":100,"height":100}}},{"node":{"nodeId":"1:2","name":"Landing","textPreview":"Learn more","bounds":{"width":400,"height":800}}}]});
        let hints = ScreenshotObservations {
            labels: vec!["learn more".into()],
            aspect_ratio: Some(0.5),
        };
        rank(&mut result, Some(&hints));
        assert_eq!(result["candidates"][0]["node"]["nodeId"], "1:2");
        assert_eq!(result["candidates"].as_array().unwrap().len(), 2);
        assert_eq!(
            result["candidates"][0]["observationEvidence"]["visionVerified"],
            false
        );
        assert!(
            ScreenshotObservations {
                labels: vec!["x".repeat(81)],
                aspect_ratio: None
            }
            .validate()
            .is_err()
        );
        assert!(
            ScreenshotObservations {
                labels: vec![],
                aspect_ratio: Some(f64::INFINITY)
            }
            .validate()
            .is_err()
        );
    }
}
