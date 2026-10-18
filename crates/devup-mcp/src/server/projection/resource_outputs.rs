pub(super) fn projected_outputs_from_result(
    result: &Map<String, Value>,
) -> Result<Vec<ProjectedOutput>, DevupError> {
    let mut outputs = Vec::new();
    for field in ["tsx", "componentTsx", "responsiveTsx"] {
        if let Some(tsx) = result.get(field).and_then(Value::as_str) {
            outputs.push(ProjectedOutput::text(
                field,
                "text/typescript",
                tsx.as_bytes().to_vec(),
            ));
        }
    }
    if let Some(devup_json) = result.get("devupJson").and_then(Value::as_str) {
        outputs.push(ProjectedOutput::text(
            "devupJson",
            "application/json",
            devup_json.as_bytes().to_vec(),
        ));
    }
    if let Some(reference) = result.get("referencePng") {
        let data = reference
            .get("dataBase64")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DevupError::new(
                    ErrorCode::DevupSnapshotUnsupported,
                    "The referencePng resource has no base64 data.",
                    false,
                )
            })?;
        let bytes = STANDARD.decode(data.as_bytes()).map_err(|_| {
            DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "The referencePng resource base64 is invalid.",
                false,
            )
        })?;
        outputs.push(ProjectedOutput::binary("reference.png", "image/png", bytes));
    }
    for (field, name) in [
        ("rawSnapshot", "raw-snapshot.json"),
        ("rawPayload", "raw-payload.json"),
        ("sourceMap", "source-map.json"),
        ("designFingerprints", "design-fingerprints.json"),
        ("designChanges", "design-changes.json"),
        ("assetManifest", "asset-manifest.json"),
        ("imageFillAudit", "image-fill-audit.json"),
        ("imageAudit", "image-audit.json"),
        ("assetCssMap", "asset-css-map.json"),
        ("webReadyImages", "web-ready-images.json"),
        ("pageScaffold", "page-scaffold.json"),
        ("outline", "outline.json"),
        ("selected", "selected.json"),
    ] {
        if let Some(value) = result.get(field) {
            if field == "imageFillAudit" && value["assets"].as_array().is_none_or(Vec::is_empty) {
                continue;
            }
            outputs.push(ProjectedOutput::text(
                name,
                "application/json",
                encode_projected_json(value)?,
            ));
        }
    }
    if let Some(frames) = result.get("frames").and_then(Value::as_array) {
        for (index, frame) in frames.iter().enumerate() {
            if let Some(scaffold) = frame.get("pageScaffold") {
                outputs.push(ProjectedOutput::text(
                    format!("frame-{}.page-scaffold.json", index + 1),
                    "application/json",
                    encode_projected_json(scaffold)?,
                ));
            }
            for field in ["tsx", "componentTsx"] {
                if let Some(tsx) = frame.get(field).and_then(Value::as_str) {
                    let name = if field == "tsx" {
                        format!("frame-{}.tsx", index + 1)
                    } else {
                        format!("frame-{}.component.tsx", index + 1)
                    };
                    outputs.push(ProjectedOutput::text(
                        name,
                        "text/typescript",
                        tsx.as_bytes().to_vec(),
                    ));
                }
            }
            if let Some(source_map) = frame.get("sourceMap") {
                outputs.push(ProjectedOutput::text(
                    format!("frame-{}.source-map.json", index + 1),
                    "application/json",
                    encode_projected_json(source_map)?,
                ));
            }
        }
    }
    Ok(outputs)
}

fn encode_projected_json(value: &Value) -> Result<Vec<u8>, DevupError> {
    serde_json::to_vec(value).map_err(|error| {
        DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            format!("Cannot serialize the resource output to JSON: {error}"),
            false,
        )
    })
}

pub(super) struct DeliveryAttachment {
    reservation: OutputReservation,
}

/// Keep complete diagnostic evidence readable without duplicating it into a
/// multi-megabyte tool envelope. Only resource delivery changes these details;
/// diagnostic identities/counts/severities and verdicts remain inline.
fn resource_diagnostic_details(
    result: &mut Value,
    outputs: &mut Vec<ProjectedOutput>,
    artifact_id: &str,
) -> Result<(), DevupError> {
    const FIELDS: [&str; 3] = ["diagnostics", "projectionIssues", "projectionEvidence"];
    const BULKY: [&str; 5] = [
        "verdictScope",
        "projectionReview",
        "placementContracts",
        "outputResults",
        "widthBehavior",
    ];
    let mut full = json!({"response":{},"frames":[]});
    for field in FIELDS.into_iter().chain(BULKY) {
        if let Some(value) = result.get(field) {
            full["response"][field] = value.clone();
        }
    }
    if let Some(frames) = result["frames"].as_array() {
        for frame in frames {
            let mut record = json!({"nodeId":frame["nodeId"]});
            for field in FIELDS.into_iter().chain(BULKY) {
                if let Some(value) = frame.get(field) {
                    record[field] = value.clone();
                }
            }
            full["frames"].as_array_mut().unwrap().push(record);
        }
    }
    let bytes = encode_projected_json(&full)?;
    if bytes.len() <= super::delivery::MAX_INLINE_OUTPUT_BYTES / 2 {
        return Ok(());
    }
    let output = ProjectedOutput::text("projection-details.json", "application/json", bytes);
    let uri = output.manifest_uri(artifact_id);
    fn preview(
        value: &Value,
        depth: usize,
        path: &str,
        omissions: &mut Map<String, Value>,
    ) -> Value {
        let (original, returned) = match value {
            Value::Array(v) => (v.len(), if depth > 0 { v.len().min(8) } else { 0 }),
            Value::Object(v) => (v.len(), if depth > 0 { v.len().min(32) } else { 0 }),
            Value::String(v) => (v.chars().count(), v.chars().count().min(320)),
            _ => (0, 0),
        };
        if original > returned {
            omissions.insert(path.into(), json!({"originalCount":original,"returnedCount":returned,"omittedCount":original-returned}));
        }
        match value {
            Value::String(s) => json!(s.chars().take(320).collect::<String>()),
            Value::Array(items) if depth > 0 => json!(
                items
                    .iter()
                    .take(8)
                    .enumerate()
                    .map(|(i, v)| preview(v, depth - 1, &format!("{path}/{i}"), omissions))
                    .collect::<Vec<_>>()
            ),
            Value::Object(items) if depth > 0 => Value::Object(
                items
                    .iter()
                    .take(32)
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            preview(
                                v,
                                depth - 1,
                                &format!("{path}/{}", k.replace('~', "~0").replace('/', "~1")),
                                omissions,
                            ),
                        )
                    })
                    .collect(),
            ),
            Value::Array(items) => json!({"state":"resource-details","entryCount":items.len()}),
            Value::Object(items) => json!({"state":"resource-details","entryCount":items.len()}),
            _ => value.clone(),
        }
    }
    fn summarize(value: &mut Value, prefix: &str) {
        for field in FIELDS {
            if let Some(items) = value[field].as_array_mut() {
                for (index, item) in items.iter_mut().enumerate() {
                    let Some(details) = item.get_mut("details").filter(|d| d.is_object()) else {
                        continue;
                    };
                    let mut summary = Map::new();
                    // Preserve small source values (notably FILL/HUG) and
                    // contracts; only bulky detail values need a resource.
                    for (key, value) in details.as_object().unwrap() {
                        if value.to_string().len() <= 512
                            || (key == "originalValue" && value.to_string().len() <= 1024)
                        {
                            summary.insert(key.clone(), value.clone());
                        }
                    }
                    for key in [
                        "classification",
                        "nextAction",
                        "output",
                        "nodeIds",
                        "candidateCount",
                        "state",
                        "propertyMappingVerified",
                    ] {
                        if let Some(value) = details.get(key) {
                            summary.insert(key.into(), value.clone());
                        }
                    }
                    if Value::Object(summary.clone()) == *details {
                        continue;
                    }
                    // Share the long URI once at the response root rather
                    // than repeating it for every diagnostic in both MCP forms.
                    summary.insert("fullDetailsResource".into(), json!({"uriPath":"/metadataDelivery/uri","jsonPointer":format!("{prefix}/{field}/{index}/details")}));
                    *details = Value::Object(summary);
                }
            }
        }
        for field in BULKY {
            if let Some(original) = value
                .get(field)
                .filter(|v| v.to_string().len() > 16_384)
                .cloned()
            {
                let mut omissions = Map::new();
                value[field] = preview(&original, 3, "", &mut omissions);
                value["metadataPreviewFields"][field] = json!({"truncated":true,"uriPath":"/metadataDelivery/uri","jsonPointer":format!("{prefix}/{field}"),"originalBytes":original.to_string().len(),"omissions":omissions,"maxArrayEntries":8,"maxObjectEntries":32,"maxStringChars":320,"maxDepth":3});
            }
        }
    }
    summarize(result, "/response");
    if let Some(frames) = result["frames"].as_array_mut() {
        for (index, frame) in frames.iter_mut().enumerate() {
            summarize(frame, &format!("/frames/{index}"));
        }
    }
    result["metadataDelivery"] = json!({"mode":"resource-details","uri":uri,"name":"projection-details.json","fields":FIELDS.into_iter().chain(BULKY).collect::<Vec<_>>(),"requiresFullDetailRead":true,
        "note":include_str!("../text/projection-resource_diagnostic_details-9b1c1ed8.md")});
    outputs.push(output);
    Ok(())
}
