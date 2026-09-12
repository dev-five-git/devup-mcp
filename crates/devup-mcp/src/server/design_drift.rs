//! Persisted converter-input fingerprints and capture comparison.
use std::collections::BTreeMap;

use devup_mcp_figma::{DevupError, ErrorCode};
use serde::Deserialize;
use serde_json::{Map, Value, json};

pub(super) type Fingerprints = BTreeMap<String, String>;
const VERSION: &str = "v1:sha256:";
const KIND: &str = "devup-design-fingerprints";
const MEANING: &str = "Changed means a field the converter reads changed; it does not mean the rendered screen changed and does not locate the change inside the node. Unchanged means captured converter inputs for that node are identical; it does not prove generated code on disk still matches, because a human may have edited it. Added and removed mean absence in the previous or current capture map, respectively; compare the same capture scope. These fingerprints do not hash generated code, external variable definitions, asset bytes, or pixels.";

/// Pure comparison of validated maps from the same fingerprint version.
fn compare(previous: &Fingerprints, current: &Fingerprints) -> Value {
    let mut unchanged = Vec::new();
    let mut changed = Vec::new();
    let mut added = Vec::new();
    for (id, fingerprint) in current {
        match previous.get(id) {
            Some(old) if old == fingerprint => unchanged.push(id),
            Some(_) => changed.push(id),
            None => added.push(id),
        }
    }
    let removed: Vec<_> = previous
        .keys()
        .filter(|id| !current.contains_key(*id))
        .collect();
    json!({"fingerprintVersion":VERSION,"unchanged":unchanged,"changed":changed,
        "added":added,"removed":removed,"meaning":MEANING})
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Sidecar {
    kind: String,
    fingerprint_version: String,
    design_fingerprints: Fingerprints,
    overwrite_policy: String,
}

fn malformed(reason: impl std::fmt::Display) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupInvalidInput,
        format!("Malformed design fingerprint sidecar: {reason}"),
        false,
        json!({"type":"designFingerprintMalformed"}),
    )
}

fn version_mismatch(previous: &str) -> DevupError {
    DevupError::with_details(
        ErrorCode::DevupInvalidInput,
        format!(
            "Cannot compare design fingerprint versions {previous} and {VERSION}. Re-export a matching baseline."
        ),
        false,
        json!({"type":"designFingerprintVersionMismatch","previousVersion":previous,"currentVersion":VERSION}),
    )
}

pub(super) fn parse(text: &str) -> Result<Fingerprints, DevupError> {
    let sidecar: Sidecar = serde_json::from_str(text)
        .map_err(|_| malformed("expected a complete design fingerprint sidecar JSON object"))?;
    if sidecar.kind != KIND || sidecar.overwrite_policy != "replace-existing" {
        return Err(malformed("unrecognized sidecar kind or overwrite policy"));
    }
    if sidecar.fingerprint_version != VERSION {
        return Err(version_mismatch(&sidecar.fingerprint_version));
    }
    for (id, fingerprint) in &sidecar.design_fingerprints {
        let Some((prefix, digest)) = fingerprint.rsplit_once(':') else {
            return Err(malformed("a node fingerprint has no version prefix"));
        };
        if id.is_empty()
            || digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(malformed("a node ID or SHA-256 digest is invalid"));
        }
        let version = format!("{prefix}:");
        if version != VERSION {
            return Err(version_mismatch(&version));
        }
    }
    Ok(sidecar.design_fingerprints)
}

pub(super) fn validate_request(
    outputs: &[String],
    previous: Option<&str>,
) -> Result<Option<Fingerprints>, DevupError> {
    let previous = previous.map(parse).transpose()?;
    if outputs.iter().any(|output| output == "designChanges") && previous.is_none() {
        return Err(malformed(
            "designChanges requires previousDesignFingerprints containing the saved sidecar JSON text",
        ));
    }
    Ok(previous)
}

/// Assemble only explicitly requested outputs; file writes remain in projection's transaction.
pub(super) fn attach(
    outputs: &[String],
    previous: Option<&Fingerprints>,
    current: &Fingerprints,
    result: &mut Map<String, Value>,
    pending_text_outputs: &mut BTreeMap<String, String>,
) {
    if outputs.iter().any(|output| output == "designFingerprints") {
        let sidecar = json!({"kind":KIND,"fingerprintVersion":VERSION,
            "designFingerprints":current,"overwritePolicy":"replace-existing"});
        pending_text_outputs.insert(
            "designFingerprints".into(),
            serde_json::to_string_pretty(&sidecar).expect("JSON sidecar"),
        );
        result.insert("designFingerprints".into(), sidecar);
    }
    if outputs.iter().any(|output| output == "designChanges") {
        result.insert(
            "designChanges".into(),
            compare(previous.expect("validated previous sidecar"), current),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devup_mcp_devup_ui::provenance::design_fingerprints;
    use devup_mcp_figma::Snapshot;

    fn capture() -> Snapshot {
        serde_json::from_value(
            json!({"fileKey":"fixture","diagnostics":[],"roots":["1:1","1:2"],"nodes":{
                "1:1":{"id":"1:1","type":"TEXT","fields":{"characters":"Before","fontSize":16}},
                "1:2":{"id":"1:2","type":"FRAME","fields":{"width":100}}
            }}),
        )
        .unwrap()
    }

    fn sidecar(version: &str, fingerprints: Fingerprints) -> String {
        json!({"kind":"devup-design-fingerprints","fingerprintVersion":version,
            "designFingerprints":fingerprints,"overwritePolicy":"replace-existing"})
        .to_string()
    }

    fn assert_groups(
        value: &Value,
        unchanged: &[&str],
        changed: &[&str],
        added: &[&str],
        removed: &[&str],
    ) {
        assert_eq!(value["unchanged"], json!(unchanged));
        assert_eq!(value["changed"], json!(changed));
        assert_eq!(value["added"], json!(added));
        assert_eq!(value["removed"], json!(removed));
    }

    #[test]
    fn w10_identical_capture_is_all_unchanged() {
        let map = design_fingerprints(&capture());
        assert_groups(&compare(&map, &map), &["1:1", "1:2"], &[], &[], &[]);
    }

    #[test]
    fn w10_one_converter_read_field_changes_exactly_one_node() {
        let mut snapshot = capture();
        let previous = design_fingerprints(&snapshot);
        snapshot
            .nodes
            .get_mut("1:1")
            .unwrap()
            .fields
            .insert("characters".into(), json!("After"));
        assert_groups(
            &compare(&previous, &design_fingerprints(&snapshot)),
            &["1:2"],
            &["1:1"],
            &[],
            &[],
        );
    }

    #[test]
    fn w10_added_node_is_not_unchanged() {
        let current = design_fingerprints(&capture());
        let mut previous = current.clone();
        previous.remove("1:2");
        assert_groups(&compare(&previous, &current), &["1:1"], &[], &["1:2"], &[]);
    }

    #[test]
    fn w10_removed_node_is_not_unchanged() {
        let previous = design_fingerprints(&capture());
        let mut current = previous.clone();
        current.remove("1:2");
        assert_groups(&compare(&previous, &current), &["1:1"], &[], &[], &["1:2"]);
    }

    #[test]
    fn w10_sidecar_round_trips() {
        let map = design_fingerprints(&capture());
        assert_eq!(parse(&sidecar("v1:sha256:", map.clone())).unwrap(), map);
    }

    #[test]
    fn w10_version_mismatch_names_both_versions() {
        let map = design_fingerprints(&capture())
            .into_iter()
            .map(|(id, hash)| (id, hash.replace("v1:", "v2:")))
            .collect();
        let error = parse(&sidecar("v2:sha256:", map)).unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
        assert_eq!(error.details["type"], "designFingerprintVersionMismatch");
        assert_eq!(error.details["previousVersion"], "v2:sha256:");
        assert_eq!(error.details["currentVersion"], "v1:sha256:");
        assert!(error.message.contains("v2:sha256:") && error.message.contains("v1:sha256:"));
    }

    #[test]
    fn w10_malformed_truncated_and_unrelated_sidecars_are_typed_errors() {
        let valid = sidecar("v1:sha256:", design_fingerprints(&capture()));
        for text in [
            "{}",
            "[]",
            "null",
            "not JSON",
            &valid[..valid.len() - 1],
            r#"{"kind":"unrelated","fingerprintVersion":"v1:sha256:","designFingerprints":{}}"#,
            r#"{"kind":"devup-design-fingerprints","fingerprintVersion":"v1:sha256:","designFingerprints":{"1:1":"v1:sha256:abc"}}"#,
        ] {
            let error = parse(text).unwrap_err();
            assert_eq!(error.code, ErrorCode::DevupInvalidInput);
            assert_eq!(error.details["type"], "designFingerprintMalformed");
        }
    }

    #[test]
    fn w10_comparison_explains_limits_without_rendered_equivalence() {
        let map = design_fingerprints(&capture());
        let value = compare(&map, &map);
        let meaning = value["meaning"].as_str().unwrap();
        assert!(meaning.contains("field the converter reads"));
        assert!(meaning.contains("does not mean the rendered screen changed"));
        assert!(meaning.contains("does not locate the change inside the node"));
        assert!(meaning.contains("does not prove generated code on disk still matches"));
        assert!(meaning.contains("human may have edited"));
    }
}
