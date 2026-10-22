pub(super) async fn apply_delivery(
    result: &mut Value,
    mode: DeliveryMode,
    artifact_store: &ArtifactStore,
    artifact: &ArtifactLookup,
    mut outputs: Vec<ProjectedOutput>,
    reprojection_arguments: Option<&Value>,
) -> Result<Option<DeliveryAttachment>, DevupError> {
    if outputs.is_empty() || choose_delivery_for_result(mode, result, &outputs)?.inline {
        return Ok(None);
    }
    let frames = result["frames"].as_array();
    let selected_frame = frames.and_then(|frames| {
        frames.iter().enumerate().find(|(_, frame)| {
            frame.get("pageScaffold").is_some()
                || ["tsx", "componentTsx"]
                    .iter()
                    .any(|field| frame.get(*field).is_some_and(Value::is_string))
        })
    });
    let frame = selected_frame.map(|(_, frame)| frame);
    let selected = [
        "tsx",
        "componentTsx",
        "devupJson",
        "rawSnapshot",
        "rawPayload",
        "responsiveTsx",
        "assetManifest",
        "imageAudit",
        "pageScaffold",
        "referencePng",
        "designFingerprints",
        "designChanges",
    ]
    .into_iter()
    .find_map(|field| {
        frame
            .and_then(|f| f.get(field))
            .or_else(|| result.get(field))
            .map(|body| (field, body))
    });
    if let Some((field, body)) = selected {
        let binary = field == "referencePng";
        let bytes = if binary {
            outputs
                .iter()
                .find(|o| o.name == "reference.png")
                .map_or(0, |o| o.bytes.len())
        } else {
            body.as_str()
                .map_or_else(|| serde_json::to_vec(body).unwrap().len(), str::len)
        };
        let escaped_bytes = if binary {
            bytes.div_ceil(3) * 4
        } else {
            serde_json::to_vec(body).unwrap().len()
        };
        let coupled = reprojection_arguments.is_some_and(|a| {
            a.get("pageScaffold").is_some()
                || a["preset"] == "web-ready-images"
                || a["assetRequests"].as_array().is_some_and(|a| !a.is_empty())
                || (matches!(
                    a["devupJsonMode"].as_str(),
                    Some("used-only" | "missing-only")
                ) && a["outputs"]
                    .as_array()
                    .is_some_and(|outputs| outputs.iter().any(|output| output == "devupJson")))
        });
        let mut args = reprojection_arguments.cloned().unwrap_or_else(|| json!({}));
        if !coupled {
            args["artifactId"] = json!(artifact.artifact_id);
            args["outputs"] = json!([field]);
            if let Some(frame) = frame {
                args["frameIds"] = json!([frame["nodeId"]]);
                args["allScreens"] = json!(false);
                if frames.is_some_and(|frames| frames.len() > 1)
                    && let Some(name) = args["componentName"].as_str()
                {
                    args["componentName"] =
                        json!(format!("{name}{}", selected_frame.unwrap().0 + 1));
                }
            }
            args["debug"] = json!(true);
            args["delivery"] = json!(
                if escaped_bytes * 2 > super::delivery::MAX_INLINE_TOTAL_BYTES {
                    "resource"
                } else {
                    "inline"
                }
            );
            args.as_object_mut().unwrap().remove("outputPaths");
            args.as_object_mut().unwrap().remove("assetRequests");
        }
        if args.get("pageScaffold").is_some() {
            args["pageScaffold"]["write"] = json!(false);
        }
        if matches!(
            args["devupJsonMode"].as_str(),
            Some("used-only" | "missing-only")
        ) && args.get("pageScaffold").is_none()
            && !args["assetRequests"]
                .as_array()
                .is_some_and(|requests| !requests.is_empty())
        {
            args.as_object_mut().unwrap().remove("outputPaths");
        }
        let example = json!({"tool":"devup_figma_export","arguments":args});
        result["nextAction"] = json!({"tool":"devup_figma_export","arguments":args,"example":example,
            "how":if coupled { include_str!("../text/projection-apply_delivery-4796ba61.md") }
                else if field == "responsiveTsx" { "Read the linked resource or reproject this responsive output with the same breakpoint selection. A responsive merge requires multiple source screens." }
                else { include_str!("../text/projection-apply_delivery-6938bf43.md") },
            "sizeEstimate":{"outputBytes":bytes,"minimumWireBytes":escaped_bytes * 2,
                "limitBytes":super::delivery::MAX_INLINE_TOTAL_BYTES,"isMeasuredReprojection":false,
                "inlineFit":if escaped_bytes * 2 > super::delivery::MAX_INLINE_TOTAL_BYTES {"exceeds-limit"} else {"unknown"},
                "binary":binary,
                "basis":include_str!("../text/projection-apply_delivery-e5a0fbb2.md")}});
    }
    resource_diagnostic_details(result, &mut outputs, &artifact.artifact_id)?;
    spill_oversized_fields(result, &mut outputs, &artifact.artifact_id)?;
    let projection_key = projection_key(&outputs);
    materialize_asset_resource_references(result, &mut outputs, &artifact.artifact_id)?;
    let reservation = artifact_store
        .reserve_outputs(&artifact.artifact_id, &projection_key, outputs)
        .await?;
    let manifests = reservation.manifests().to_vec();
    if let Some(details) = manifests.iter().find(|m| m.name == "compact-details.json") {
        result["selectionEvidence"]["uri"] = json!(details.manifest_uri);
    }
    if let Some(old_uri) = result["metadataDelivery"]["uri"]
        .as_str()
        .map(str::to_owned)
        && let Some(manifest) = manifests
            .iter()
            .find(|m| m.name == "projection-details.json")
        && old_uri != manifest.manifest_uri
    {
        fn rebind(value: &mut Value, old: &str, new: &str) {
            match value {
                Value::String(s) if s == old => *s = new.to_owned(),
                Value::Array(items) => items.iter_mut().for_each(|v| rebind(v, old, new)),
                Value::Object(items) => items.values_mut().for_each(|v| rebind(v, old, new)),
                _ => {}
            }
        }
        rebind(result, &old_uri, &manifest.manifest_uri);
    }
    let result = result.as_object_mut().ok_or_else(|| {
        DevupError::new(
            ErrorCode::DevupFigmaHandoffInvalid,
            "The resource delivery result is not a JSON object.",
            false,
        )
    })?;
    for (field, name) in [
        ("imageFillAudit", "image-fill-audit.json"),
        ("imageAudit", "image-audit.json"),
        ("assetCssMap", "asset-css-map.json"),
    ] {
        if let Some(audit) = result.get_mut(field).and_then(Value::as_object_mut) {
            audit.remove("assets");
            if let Some(manifest) = manifests.iter().find(|m| m.name == name) {
                audit.insert("detailsResource".into(), json!({"uri":manifest.manifest_uri,"name":manifest.name,"contentHash":manifest.sha256,"requiresFullDetailRead":true}));
            }
        }
    }
    if let Some(plan) = result
        .get_mut("webReadyImages")
        .and_then(Value::as_object_mut)
    {
        plan.remove("plan");
        if let Some(manifest) = manifests.iter().find(|m| m.name == "web-ready-images.json") {
            plan.insert("detailsResource".into(), json!({"uri":manifest.manifest_uri,"name":manifest.name,"contentHash":manifest.sha256,"requiresFullDetailRead":true}));
        }
    }
    for field in [
        "tsx",
        "componentTsx",
        "responsiveTsx",
        "devupJson",
        "rawSnapshot",
        "rawPayload",
        "sourceMap",
        "designFingerprints",
        "designChanges",
        "assetManifest",
        "pageScaffold",
        "referencePng",
        "outline",
    ] {
        if field == "outline"
            && result
                .get("deliverable")
                .is_some_and(|d| d["kind"] == "outline-data")
        {
            continue;
        }
        result.remove(field);
    }
    if let Some(frames) = result.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames {
            if let Some(frame) = frame.as_object_mut() {
                frame.remove("tsx");
                frame.remove("componentTsx");
                frame.remove("sourceMap");
                frame.remove("pageScaffold");
            }
        }
    }
    if let Some(deliverable) = result
        .get_mut("deliverable")
        .filter(|d| d["kind"] != "selected-data" && d["kind"] != "outline-data")
    {
        deliverable["note"] = json!(if deliverable["isFinal"] == true {
            "The final TSX deliverables are in resources. Read the linked resources to implement them."
        } else {
            "The available TSX outputs are in resources. Review projectionIssues and outputResults before using them; these outputs are not final."
        });
    }
    result.insert(
        "resources".to_owned(),
        Value::Array(
            manifests
                .into_iter()
                .map(|manifest| {
                    json!({
                        "type": "resource_link",
                        "uri": manifest.manifest_uri,
                        "name": manifest.name,
                        "mimeType": manifest.mime_type,
                        "size": manifest.raw_bytes,
                        "contentHash": manifest.sha256,
                        "assetMetadata": manifest.asset_metadata,
                        "expiresAt": format_epoch_rfc3339(manifest.expires_at_epoch_seconds)
                    })
                })
                .collect(),
        ),
    );
    // Resource promotion must also bound the final metadata envelope.
    if let Err(mut error) =
        choose_delivery_for_result(DeliveryMode::Inline, &Value::Object(result.clone()), &[])
    {
        reservation.rollback();
        let mut sizes = result
            .iter()
            .map(|(key, value)| (key.clone(), value.to_string().len()))
            .collect::<Vec<_>>();
        sizes.sort_by_key(|a| std::cmp::Reverse(a.1));
        sizes.truncate(8);
        if !error.details.is_object() {
            error.details = json!({});
        }
        error.details["envelopeFields"] = json!(
            sizes
                .into_iter()
                .map(|(field, bytes)| json!({"field": field, "bytes": bytes}))
                .collect::<Vec<_>>()
        );
        return Err(error);
    }
    Ok(Some(DeliveryAttachment { reservation }))
}

const BODY_FIELDS: [&str; 12] = [
    "tsx",
    "componentTsx",
    "responsiveTsx",
    "devupJson",
    "rawSnapshot",
    "rawPayload",
    "sourceMap",
    "designFingerprints",
    "designChanges",
    "assetManifest",
    "pageScaffold",
    "referencePng",
];

const KEPT_INLINE: [&str; 10] = [
    "status",
    "nextAction",
    "resources",
    "deliverable",
    "artifactId",
    "outputPaths",
    "quality",
    "server",
    "metadataDelivery",
    "envelopeOverflow",
];

const ESSENTIAL_INLINE: [&str; 6] = [
    "status",
    "nextAction",
    "resources",
    "artifactId",
    "outputPaths",
    "server",
];

const HANDLED_LATER: [&str; 4] = ["imageFillAudit", "imageAudit", "assetCssMap", "webReadyImages"];

fn envelope_bytes(result: &Value) -> usize {
    let mut probe = result.clone();
    if let Some(object) = probe.as_object_mut() {
        for field in BODY_FIELDS {
            object.remove(field);
        }
    }
    if let Some(frames) = probe.get_mut("frames").and_then(Value::as_array_mut) {
        for frame in frames.iter_mut().filter_map(Value::as_object_mut) {
            for field in ["tsx", "componentTsx", "sourceMap", "pageScaffold"] {
                frame.remove(field);
            }
        }
    }
    serde_json::to_vec(&probe).map_or(0, |bytes| bytes.len())
}

fn spill_oversized_fields(
    result: &mut Value,
    outputs: &mut Vec<ProjectedOutput>,
    artifact_id: &str,
) -> Result<(), DevupError> {
    let budget = super::delivery::MAX_INLINE_TOTAL_BYTES / 2;
    if envelope_bytes(result) <= budget {
        return Ok(());
    }
    let mut spilled = serde_json::Map::new();
    // Second pass: when only fields kept inline remain, the final inline check
    // would fail the whole call, so they spill too (all but a small core).
    let mut strict = false;
    while envelope_bytes(result) > budget {
        let largest = result
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(key, _)| {
                (if strict {
                    !ESSENTIAL_INLINE.contains(&key.as_str())
                } else {
                    !KEPT_INLINE.contains(&key.as_str())
                }) && !BODY_FIELDS.contains(&key.as_str())
                    && !HANDLED_LATER.contains(&key.as_str())
            })
            .map(|(key, value)| (key.clone(), value.to_string().len()))
            .max_by_key(|(_, bytes)| *bytes);
        let Some((key, bytes)) = largest.filter(|(_, bytes)| *bytes > 1024) else {
            if strict {
                break;
            }
            strict = true;
            continue;
        };
        let original = result[&key].take();
        result[&key] = json!({"state":"resource-details","originalBytes":bytes,
            "jsonPointer":format!("/{key}"),"uriPath":"/envelopeOverflow/uri"});
        spilled.insert(key, original);
    }
    if spilled.is_empty() {
        return Ok(());
    }
    let fields = spilled.keys().cloned().collect::<Vec<_>>();
    let output = ProjectedOutput::text(
        "envelope-overflow.json",
        "application/json",
        encode_projected_json(&Value::Object(spilled))?,
    );
    result["envelopeOverflow"] = json!({"uri":output.manifest_uri(artifact_id),"fields":fields,
        "reason":"The response metadata exceeded the inline limit; these fields are complete in the linked resource."});
    outputs.push(output);
    Ok(())
}

pub(super) async fn rollback_delivery(
    _artifact_store: &ArtifactStore,
    _artifact: &ArtifactLookup,
    attachment: Option<DeliveryAttachment>,
) {
    if let Some(attachment) = attachment {
        attachment.reservation.rollback();
    }
}

pub(super) fn commit_delivery(attachment: Option<DeliveryAttachment>) {
    if let Some(attachment) = attachment {
        attachment.reservation.commit();
    }
}

#[cfg(test)]
mod spill_tests {
    use super::*;

    #[test]
    fn kept_inline_fields_spill_when_nothing_else_can_shrink_the_envelope() {
        let mut result = json!({"status":"complete","quality":{"blob":"q".repeat(700_000)}});
        let mut outputs = Vec::new();
        spill_oversized_fields(&mut result, &mut outputs, "artifact").unwrap();
        assert_eq!(result["quality"]["state"], "resource-details");
        assert_eq!(result["status"], "complete");
        assert_eq!(outputs.len(), 1);
        assert!(envelope_bytes(&result) <= super::super::delivery::MAX_INLINE_TOTAL_BYTES / 2);
    }
}
