use std::str::FromStr;

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use devup_mcp_figma::{DevupError, ErrorCode};
use rand::Rng;
use rmcp::model::{CallToolResult, ContentBlock, MetaObject, Resource};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_INLINE_OUTPUT_BYTES: usize = 256 * 1024;
pub const MAX_INLINE_TOTAL_BYTES: usize = 1024 * 1024;
pub const RESOURCE_CHUNK_BYTES: usize = 256 * 1024;
/// Debug outputs (rawSnapshot/rawPayload) describe the design, not the screen,
/// and dwarf the code. Under `auto` they travel as resources past this size so
/// the warnings and checklist are not buried under them; `inline` still forces
/// them into the response.
pub const MAX_INLINE_DEBUG_BYTES: usize = 32 * 1024;

fn is_debug_output(output: &ProjectedOutput) -> bool {
    output.name.starts_with("raw-")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DeliveryMode {
    #[default]
    Auto,
    Inline,
    Resource,
}

impl FromStr for DeliveryMode {
    type Err = DevupError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "inline" => Ok(Self::Inline),
            "resource" => Ok(Self::Resource),
            _ => Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "delivery must be auto, inline, or resource.",
                false,
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedOutput {
    pub name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub is_binary: bool,
    pub(crate) resource_id: String,
    pub(crate) asset_id: Option<String>,
    pub(crate) asset_metadata: Option<Value>,
}

impl ProjectedOutput {
    pub fn text(name: impl Into<String>, mime_type: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            mime_type: mime_type.into(),
            bytes,
            is_binary: false,
            resource_id: random_resource_id(),
            asset_id: None,
            asset_metadata: None,
        }
    }

    pub fn binary(name: impl Into<String>, mime_type: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            mime_type: mime_type.into(),
            bytes,
            is_binary: true,
            resource_id: random_resource_id(),
            asset_id: None,
            asset_metadata: None,
        }
    }

    pub(crate) fn asset(
        name: impl Into<String>,
        mime_type: impl Into<String>,
        bytes: Vec<u8>,
        asset_id: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            mime_type: mime_type.into(),
            bytes,
            is_binary: true,
            resource_id: random_resource_id(),
            asset_id: Some(asset_id.into()),
            asset_metadata: None,
        }
    }

    pub(crate) fn manifest_uri(&self, artifact_id: &str) -> String {
        format!(
            "devup://artifact/{artifact_id}/outputs/{}/manifest",
            self.resource_id
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryDecision {
    pub inline: bool,
}

pub fn choose_delivery(
    mode: DeliveryMode,
    outputs: &[ProjectedOutput],
) -> Result<DeliveryDecision, DevupError> {
    let total_bytes = outputs.iter().try_fold(0_usize, |total, output| {
        total.checked_add(output.bytes.len()).ok_or_else(|| {
            DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "The generated output size exceeded the safe range.",
                false,
            )
        })
    })?;
    let every_output_inline = outputs.iter().try_fold(true, |within_limit, output| {
        let wire_bytes = projected_output_wire_bytes(output)?;
        Ok::<_, DevupError>(within_limit && wire_bytes <= MAX_INLINE_OUTPUT_BYTES)
    })?;
    match mode {
        DeliveryMode::Auto => Ok(DeliveryDecision {
            inline: every_output_inline
                && total_bytes <= MAX_INLINE_TOTAL_BYTES
                && !outputs
                    .iter()
                    .any(|o| is_debug_output(o) && o.bytes.len() > MAX_INLINE_DEBUG_BYTES),
        }),
        DeliveryMode::Inline if total_bytes > MAX_INLINE_TOTAL_BYTES => {
            Err(inline_size_error(outputs, total_bytes, None))
        }
        DeliveryMode::Inline => Ok(DeliveryDecision { inline: true }),
        DeliveryMode::Resource => Ok(DeliveryDecision { inline: false }),
    }
}

fn projected_output_wire_bytes(output: &ProjectedOutput) -> Result<usize, DevupError> {
    let value = if output.is_binary {
        serde_json::json!({
            "output": {
                "mimeType": output.mime_type,
                "dataBase64": STANDARD.encode(&output.bytes)
            }
        })
    } else {
        let text = std::str::from_utf8(&output.bytes).map_err(|_| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "An inline text output must be UTF-8.",
                false,
            )
        })?;
        serde_json::json!({"output": text})
    };
    serde_json::to_vec(&CallToolResult::structured(value))
        .map(|bytes| bytes.len())
        .map_err(|error| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                format!("Cannot compute the MCP output size: {error}"),
                false,
            )
        })
}

pub fn choose_delivery_for_result(
    mode: DeliveryMode,
    result: &Value,
    outputs: &[ProjectedOutput],
) -> Result<DeliveryDecision, DevupError> {
    // Compute the full wire measurement even if raw outputs already exceed
    // the limit, so callers can distinguish payload size from JSON overhead.
    let projected = choose_delivery(DeliveryMode::Auto, outputs)?;
    let wire_bytes = serde_json::to_vec(&tool_result(result.clone()))
        .map_err(|error| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                format!("Cannot compute the MCP tool response size: {error}"),
                false,
            )
        })?
        .len();
    match mode {
        DeliveryMode::Auto => Ok(DeliveryDecision {
            inline: projected.inline && wire_bytes <= MAX_INLINE_TOTAL_BYTES,
        }),
        DeliveryMode::Inline
            if wire_bytes > MAX_INLINE_TOTAL_BYTES
                || outputs.iter().map(|o| o.bytes.len()).sum::<usize>()
                    > MAX_INLINE_TOTAL_BYTES =>
        {
            let mut error = inline_size_error(
                outputs,
                outputs.iter().map(|o| o.bytes.len()).sum(),
                Some(wire_bytes),
            );
            error.details["envelopeFields"] = envelope_field_sizes(result);
            Err(error)
        }
        DeliveryMode::Inline => Ok(DeliveryDecision { inline: true }),
        DeliveryMode::Resource => Ok(DeliveryDecision { inline: false }),
    }
}

pub(super) fn envelope_field_sizes(result: &Value) -> Value {
    let mut sizes = result.as_object().into_iter().flatten()
        .map(|(field, value)| (field, value.to_string().len())).collect::<Vec<_>>();
    sizes.sort_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
    serde_json::json!(sizes.into_iter().take(8).map(|(field, bytes)| serde_json::json!({"field":field,"bytes":bytes})).collect::<Vec<_>>())
}

fn inline_size_error(
    outputs: &[ProjectedOutput],
    output_bytes: usize,
    wire_bytes: Option<usize>,
) -> DevupError {
    let measured = wire_bytes.unwrap_or(output_bytes).max(output_bytes);
    DevupError::with_details(
        ErrorCode::DevupFigmaResponseTooLarge,
        if wire_bytes.is_some() {
            "The serialized inline MCP response exceeded the 1 MiB limit. Use delivery=auto or resource."
        } else {
            "The inline output exceeded the 1 MiB limit. Use delivery=auto or resource."
        },
        false,
        serde_json::json!({
            "stage":"inline-delivery", "limitBytes":MAX_INLINE_TOTAL_BYTES,
            "outputBytes":output_bytes,"serializedResponseBytes":wire_bytes,
            "exceededByBytes":measured.saturating_sub(MAX_INLINE_TOTAL_BYTES),
            "outputs":outputs.iter().map(|o| serde_json::json!({"name":o.name,"bytes":o.bytes.len(),"binary":o.is_binary})).collect::<Vec<_>>(),
            "measurement":include_str!("text/delivery-inline_size_error-e8696ca0.md"),
            "recoveryState":"unrecoverable",
            "recoveryReason":"The delivery size check alone has no artifact or original projection arguments. The export boundary must supply a concrete resource-delivery call.",
            "nextAction":null
        }),
    )
}

pub(super) fn with_inline_recovery(mut error: DevupError, arguments: Value) -> DevupError {
    if error.code != ErrorCode::DevupFigmaResponseTooLarge
        || error.details["stage"] != "inline-delivery"
    {
        return error;
    }
    let outputs = arguments["outputs"].as_array().cloned().unwrap_or_default();
    let coupled_assets = arguments["preset"] == "web-ready-images"
        || arguments["assetRequests"]
            .as_array()
            .is_some_and(|assets| !assets.is_empty());
    let mut split = Vec::new();
    if outputs.len() > 1 && !coupled_assets {
        for output in &outputs {
            let mut selected = vec![output.clone()];
            // sourceMap is invalid on its own, even for a retained artifact.
            if output == "sourceMap"
                && let Some(generated) = outputs
                    .iter()
                    .find(|o| matches!(o.as_str(), Some("tsx" | "componentTsx" | "devupJson")))
            {
                selected.insert(0, generated.clone());
            }
            let mut args = arguments.clone();
            if !selected.iter().any(|o| o == "assetManifest") {
                args.as_object_mut().unwrap().remove("assetRequests");
            }
            if let Some(paths) = args["outputPaths"].as_object_mut() {
                paths.retain(|key, _| selected.iter().any(|o| o.as_str() == Some(key)));
            }
            args["outputs"] = serde_json::json!(selected);
            split.push(serde_json::json!({"tool":"devup_figma_export","arguments":args}));
        }
    }
    if arguments["delivery"] == "resource" && error.details.get("envelopeFields").is_some() && split.is_empty() {
        let mut narrowed = arguments.clone();
        if let Some(object) = narrowed.as_object_mut() {
            object.remove("assetRequests");
            object.insert("responseFields".into(), serde_json::json!(["status", "quality"]));
        }
        error.details["recoveryState"] = serde_json::json!("available");
        error.details["recoveryReason"] = serde_json::json!(
            "resource delivery was already requested; the response metadata itself (see envelopeFields) exceeds the inline limit, so repeating this call fails again."
        );
        error.details["nextAction"] =
            serde_json::json!({"tool":"devup_figma_export","arguments":narrowed});
        error.details["nextActionReason"] = serde_json::json!(
            "Same artifact with responseFields (and outputPaths to write files). Details are then read from the linked compact-details resource. Alternatively export fewer frameIds per call."
        );
        error.details["splitRequests"] = serde_json::json!([]);
        error.details["splitReason"] = serde_json::json!(
            "Split by frameIds (one section frame per call) or write outputs with outputPaths and select only the fields you need with responseFields."
        );
        return error;
    }
    error.details["recoveryState"] = serde_json::json!("available");
    error.details["recoveryReason"] = serde_json::json!(
        "The acquisition is retained; resource delivery avoids the inline limit without calling Figma again. Resource storage limits still apply."
    );
    error.details["nextAction"] =
        serde_json::json!({"tool":"devup_figma_export","arguments":arguments});
    error.details["nextActionReason"] = serde_json::json!(
        "Use the same artifact with resource delivery, then read the linked manifests and chunks. Do not repeat the oversized inline request."
    );
    error.details["splitRequests"] = serde_json::json!(split);
    error.details["splitReason"] = serde_json::json!(if coupled_assets {
        include_str!("text/delivery-with_inline_recovery-01306837.md")
    } else {
        include_str!("text/delivery-with_inline_recovery-e6ab7ccd.md")
    });
    error
}

pub(crate) fn server_identity() -> Value {
    serde_json::json!({"version":env!("CARGO_PKG_VERSION"),"buildId":crate::build_id(),
        "commit":option_env!("DEVUP_MCP_GIT_COMMIT").filter(|s|!s.is_empty()),
        "displayVersion":format!("{}+{}", env!("CARGO_PKG_VERSION"), crate::build_id()),
        "builtAt":env!("DEVUP_MCP_BUILD_TIME").parse::<u64>().ok(),
        "identityGuidance":include_str!("text/delivery-server_identity-f8e03915.md"),
        // Cache-only. The lookup lives in a background task, so adding this to
        // the identity every response carries costs no network on the call path
        // and cannot delay a tool call.
        "updateAvailable":super::release_check::snapshot()})
}

pub fn tool_result(mut value: Value) -> CallToolResult {
    if let Some(object) = value.as_object_mut() {
        object.insert("server".into(), server_identity());
    }
    let links = value
        .get("resources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|summary| {
            let uri = summary.get("uri")?.as_str()?;
            let name = summary.get("name")?.as_str()?;
            let mime_type = summary.get("mimeType")?.as_str()?;
            let size = summary.get("size")?.as_u64()?;
            let mut meta = MetaObject::new();
            if let Some(hash) = summary.get("contentHash").and_then(Value::as_str) {
                meta.0.insert("payloadSha256".to_owned(), Value::from(hash));
            }
            meta.0
                .insert("payloadMimeType".to_owned(), Value::from(mime_type));
            meta.0.insert("payloadBytes".to_owned(), Value::from(size));
            if let Some(expires_at) = summary.get("expiresAt").and_then(Value::as_str) {
                meta.0
                    .insert("expiresAt".to_owned(), Value::from(expires_at));
            }
            Some(ContentBlock::resource_link(
                Resource::new(uri, name)
                    .with_title(format!("Devup {name} output"))
                    .with_description(
                        "Generated Devup output; read the linked manifest for bounded chunks",
                    )
                    .with_mime_type("application/json")
                    .with_meta(meta),
            ))
        })
        .collect::<Vec<_>>();
    let mut result = CallToolResult::structured(value);
    result.content.extend(links);
    result
}

fn random_resource_id() -> String {
    let mut bytes = [0_u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod r12_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn feedback_28_single_output_error_identifies_metadata_and_recovery() {
        let result = json!({"tsx":"<Box/>","projectThemeValidation":{"details":"x".repeat(800_000)}});
        let outputs = vec![ProjectedOutput::text("tsx", "text/typescript", b"<Box/>".to_vec())];
        let error = choose_delivery_for_result(DeliveryMode::Inline, &result, &outputs).unwrap_err();
        assert_eq!(error.details["outputBytes"], 6);
        assert_eq!(error.details["envelopeFields"][0]["field"], "projectThemeValidation");
        assert!(error.details["envelopeFields"][0]["bytes"].as_u64().unwrap() >= 800_000);
        let args = json!({"artifactId":"retained","outputs":["tsx"],"delivery":"resource"});
        let recovered = with_inline_recovery(error, args.clone());
        assert_ne!(recovered.details["nextAction"]["arguments"], args);
        assert_eq!(recovered.details["splitRequests"], json!([]));
        let reason = recovered.details["splitReason"].as_str().unwrap();
        assert!(reason.contains("outputPaths") && reason.contains("responseFields") && reason.contains("frameIds"));
    }

    #[test]
    fn oversized_envelope_under_resource_delivery_never_repeats_the_failing_call() {
        let mut error = choose_delivery(
            DeliveryMode::Inline,
            &[ProjectedOutput::text(
                "tsx",
                "text/typescript",
                vec![b'x'; MAX_INLINE_TOTAL_BYTES + 1],
            )],
        )
        .unwrap_err();
        error.details["envelopeFields"] = json!([{"field":"projectionIssues","bytes":1_200_000}]);
        let args = json!({"artifactId":"retained","outputs":["tsx"],"delivery":"resource",
            "assetRequests":[{"assetId":"a","format":"svg","scale":1,"outputPath":"p.svg"}]});
        let recovered = with_inline_recovery(error, args.clone());
        assert_ne!(recovered.details["nextAction"]["arguments"], args);
        assert_eq!(
            recovered.details["nextAction"]["arguments"]["responseFields"],
            json!(["status", "quality"])
        );
        assert_eq!(recovered.details["splitRequests"], json!([]));
        assert!(recovered.details["envelopeFields"].is_array());
    }

    #[test]
    fn r12_asset_mapping_recovery_keeps_coupled_outputs_together() {
        let output = ProjectedOutput::text(
            "tsx",
            "text/typescript",
            vec![b'x'; MAX_INLINE_TOTAL_BYTES + 1],
        );
        let error = choose_delivery(DeliveryMode::Inline, &[output]).unwrap_err();
        let args = json!({"artifactId":"retained","outputs":["tsx","assetManifest","rawSnapshot"],
            "delivery":"resource","debug":true,"assetPublicRoot":"C:/project/public",
            "assetRequests":[{"assetId":"icon","format":"svg","scale":1,"outputPath":"C:/project/public/icon.svg"}]});
        let recovered = with_inline_recovery(error, args.clone());
        assert_eq!(recovered.details["nextAction"]["arguments"], args);
        assert_eq!(
            recovered.details["splitRequests"],
            json!([]),
            "assetPublicRoot requires assetRequests with outputPath, and mapped TSX depends on those assets"
        );
        assert!(
            recovered.details["splitReason"]
                .as_str()
                .unwrap()
                .contains("asset")
        );
    }
}

#[cfg(test)]
mod debug_delivery_tests {
    use super::*;

    #[test]
    fn large_debug_output_goes_to_resource_under_auto_but_inline_forces_it() {
        let big = ProjectedOutput::text(
            "raw-snapshot.json",
            "application/json",
            vec![b'a'; 40 * 1024],
        );
        let code = ProjectedOutput::text("Screen.tsx", "text/plain", vec![b'a'; 40 * 1024]);
        assert!(
            !choose_delivery(DeliveryMode::Auto, std::slice::from_ref(&big))
                .unwrap()
                .inline
        );
        assert!(choose_delivery(DeliveryMode::Auto, &[code]).unwrap().inline);
        assert!(
            choose_delivery(DeliveryMode::Inline, &[big])
                .unwrap()
                .inline
        );
    }
}
