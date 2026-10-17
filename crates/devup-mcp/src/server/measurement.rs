use super::{DevupServer, Unnamed, parse_link, tools::FigmaMeasureInput};
use devup_mcp_figma::{DevupError, ErrorCode, FigmaTarget, ReadToolCall};
use serde_json::{Value, json};

fn invalid(message: &str) -> DevupError {
    DevupError::new(ErrorCode::DevupInvalidInput, message, false)
}
pub(super) fn decode(raw: &Value) -> Result<Value, DevupError> {
    if raw["kind"] == "devup-measure" {
        return Ok(raw.clone());
    }
    if raw["structuredContent"]["kind"] == "devup-measure" {
        return Ok(raw["structuredContent"].clone());
    }
    raw["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["text"].as_str())
        .filter_map(|text| serde_json::from_str::<Value>(text).ok())
        .find(|v| v["kind"] == "devup-measure")
        .ok_or_else(|| invalid("The measurement read returned no measurement envelope."))
}

fn validated_node(node: &Value, id: &str) -> Result<Value, DevupError> {
    if node["nodeId"] != id
        || !matches!(
            node["status"].as_str(),
            Some("measured" | "partial" | "unavailable")
        )
    {
        return Err(invalid(
            "Measurement node identity or state does not match the request.",
        ));
    }
    if node["status"] == "unavailable" {
        return Ok(json!({"nodeId":id,"status":"unavailable","reason":"node-read-unavailable"}));
    }
    let numeric = |key: &str, v: &Value| -> Result<Value, DevupError> {
        if v.is_null() {
            return Ok(Value::Null);
        }
        let n = v
            .as_f64()
            .filter(|n| n.is_finite() && n.abs() <= 1e12)
            .ok_or_else(|| invalid("Measurement contains an invalid numeric layout value."))?;
        if matches!(key, "width" | "height") && n < 0.0 {
            return Err(invalid("Logical dimensions cannot be negative."));
        }
        Ok(v.clone())
    };
    let label = |v: &Value| -> Result<Value, DevupError> {
        if v.is_null()
            || v.as_str().is_some_and(|s| {
                s.len() <= 32 && s.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
            })
        {
            Ok(v.clone())
        } else {
            Err(invalid(
                "Measurement layout labels must be null or bounded strings.",
            ))
        }
    };
    let width = numeric("width", &node["width"])?;
    let height = numeric("height", &node["height"])?;
    if node["status"] == "measured" && (width.is_null() || height.is_null()) {
        return Err(invalid(
            "A measured node requires finite logical width and height.",
        ));
    }
    Ok(
        json!({"nodeId":id,"status":node["status"],"width":width,"height":height,
        "padding":{"top":numeric("padding",&node["padding"]["top"] )?,"right":numeric("padding",&node["padding"]["right"] )?,"bottom":numeric("padding",&node["padding"]["bottom"] )?,"left":numeric("padding",&node["padding"]["left"] )?},
        "gap":numeric("gap",&node["gap"] )?,"layoutMode":label(&node["layoutMode"] )?,
        "align":{"primary":label(&node["align"]["primary"] )?,"counter":label(&node["align"]["counter"] )?}}),
    )
}
pub(super) fn calibrate(result: &Value, input: &super::tools::MeasureCalibration) -> Value {
    let Some(node) = result["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|n| n["nodeId"] == input.node_id)
    else {
        return json!({"state":"unverified","reason":"calibration-node-unavailable"});
    };
    let (Some(width), Some(height)) = (node["width"].as_f64(), node["height"].as_f64()) else {
        return json!({"state":"unverified","reason":"logical-dimensions-unavailable"});
    };
    if width <= 0.0 || height <= 0.0 {
        return json!({"state":"unverified","reason":"zero-logical-dimension"});
    }
    let x = input.screen_width_px / width;
    let y = input.screen_height_px / height;
    let consistent = x.is_finite() && y.is_finite() && (x - y).abs() / x.max(y) <= 0.02;
    json!({"state":if consistent {"calibrated-from-caller-pixels"} else {"unverified"},
        "nodeId":input.node_id,"logicalWidth":width,"logicalHeight":height,
        "scaleX":x.is_finite().then_some(x),"scaleY":y.is_finite().then_some(y),
        "uniformScale":consistent.then_some((x+y)/2.0),"aspectConsistent":consistent,
        "evidenceLimit":"Screen pixels are caller-declared. No screenshot, canvas mapping, crop or browser viewport was inspected; UI DPR and zoom do not prove capture scale. Compare DOM CSS logical dimensions to these Figma logical dimensions."})
}

pub(super) async fn run(
    server: &DevupServer,
    mut input: FigmaMeasureInput,
) -> Result<Value, DevupError> {
    if input.node_ids.is_empty() || input.node_ids.len() > 100 {
        return Err(invalid("nodeIds requires 1..100 distinct IDs."));
    }
    // Reuse the URL parser's canonical instance-descendant grammar.
    for id in &mut input.node_ids {
        if id.len() > 128 {
            return Err(invalid("A node ID exceeds 128 bytes."));
        }
        let mut url = url::Url::parse("figma-bridge://current").expect("constant URL");
        url.query_pairs_mut().append_pair("node-id", id);
        *id = FigmaTarget::parse(url.as_str())?
            .node_id
            .expect("explicit ID");
    }
    if input
        .node_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != input.node_ids.len()
    {
        return Err(invalid("nodeIds must be distinct after normalization."));
    }
    if let Some(calibration) = &input.calibration
        && (!input.node_ids.contains(&calibration.node_id)
            || !calibration.screen_width_px.is_finite()
            || !calibration.screen_height_px.is_finite()
            || calibration.screen_width_px <= 0.0
            || calibration.screen_height_px <= 0.0
            || calibration.screen_width_px > 1_000_000.0
            || calibration.screen_height_px > 1_000_000.0)
    {
        return Err(invalid(
            "calibration needs a requested canonical nodeId and finite positive screen dimensions at most 1000000 pixels.",
        ));
    }
    let target = server
        .bind_to_bridge(
            parse_link(input.url.as_deref())?,
            Unnamed::File,
            &json!({"tool":"devup_figma_measure","arguments":{"nodeIds":input.node_ids}}),
        )
        .await?;
    let mut measured = Vec::new();
    let mut stats = devup_mcp_figma::CollectionStats {
        transport: "explicit-node-list".into(),
        ..Default::default()
    };
    // At most 10 nodes per direct text response; even maximal ID/float/label
    // lengths fit below the upstream 20 KiB ceiling. Bridge uses the same bound.
    for ids in input.node_ids.chunks(10) {
        let call = ReadToolCall::measure_nodes(&target.file_key, ids.to_vec());
        if server.services.upstream.is_metered(&call).await {
            server.services.pacer.acquire().await;
        }
        // Live measurement deliberately bypasses the acquisition call bank.
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            server.services.upstream.call_read_tool(call),
        )
        .await
        .map_err(|_| {
            DevupError::new(
                ErrorCode::DevupFigmaDirectUnavailable,
                "Measurement timed out; no cached dimensions were substituted.",
                true,
            )
        })??;
        if let Some(error) = super::operation::upstream_error(&read.raw) {
            return Err(error);
        }
        stats.figma_tool_calls += 1;
        stats.raw_bytes += read.raw.to_string().len();
        stats.wire_bytes += read.raw.to_string().len();
        if let Some(mut served) = devup_mcp_figma::BridgeServed::from_result(&read.raw) {
            served.reads += stats.bridge.as_ref().map_or(0, |b| b.reads);
            stats.bridge = Some(served);
        }
        let result = decode(&read.raw)?;
        let nodes = result["nodes"]
            .as_array()
            .ok_or_else(|| invalid("Measurement nodes are missing."))?;
        if result["schemaVersion"] != 1
            || result["coordinateSpace"] != "figma-logical"
            || nodes.len() != ids.len()
            || result.to_string().len() > 20_000
        {
            return Err(invalid(
                "Measurement envelope size, count or coordinate space is invalid.",
            ));
        }
        for (node, id) in nodes.iter().zip(ids) {
            measured.push(validated_node(node, id)?);
        }
    }
    let mut result = json!({"kind":"devup-measure","schemaVersion":1,"coordinateSpace":"figma-logical","nodes":measured,
        "evidenceLimit":"Only explicit node layout fields were read; null is unavailable or not applicable. No descendants, resources, image bytes or generated code were acquired."});
    let nodes = result["nodes"]
        .as_array()
        .ok_or_else(|| invalid("Measurement nodes are missing."))?;
    if result["schemaVersion"] != 1
        || result["coordinateSpace"] != "figma-logical"
        || nodes.len() != input.node_ids.len()
        || nodes.iter().zip(&input.node_ids).any(|(n, id)| {
            n["nodeId"].as_str() != Some(id.as_str())
                || !matches!(
                    n["status"].as_str(),
                    Some("measured" | "partial" | "unavailable")
                )
        })
    {
        return Err(invalid(
            "Measurement response identity, count or coordinate space does not match this request.",
        ));
    }
    let partial = nodes.iter().any(|n| n["status"] != "measured");
    let retained_nodes = nodes
        .iter()
        .filter(|n| n["status"] != "unavailable")
        .map(|n| {
            (
                n["nodeId"].as_str().unwrap().to_owned(),
                json!({"id":n["nodeId"],"type":"MEASUREMENT",
            "fields":{"width":n["width"],"height":n["height"]}}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    stats.node_count = retained_nodes.len();
    let file_key = stats.bridge.as_ref().map_or_else(
        || (!target.is_bridge_only()).then_some(target.file_key.clone()),
        |b| b.file_key.clone(),
    );
    let source_kind = if stats.bridge.is_some() {
        "bridge"
    } else {
        "direct"
    };
    let payload: devup_mcp_figma::CollectedPayload = serde_json::from_value(json!({
        "target":target,"scope":"node","metadata":{"kind":"explicit-node-measurement","partial":partial},
        "snapshot":{"fileKey":target.file_key,"version":null,"roots":retained_nodes.keys().collect::<Vec<_>>(),"nodes":retained_nodes,"diagnostics":[]},
        "variables":null,"styles":null,"completeness":"resolved-values-only","sourceVersion":null,"stats":stats
    })).map_err(|_| invalid("Measurement receipt could not be retained."))?;
    let artifact = server
        .artifacts
        .get_or_acquire(
            super::artifacts::ArtifactRequestKey::measurement(
                target.clone(),
                input.node_ids.clone(),
            ),
            true,
            || async { Ok(payload) },
        )
        .await?;
    result["cache"] = super::projection::artifact_metadata(&artifact);
    result["source"] = json!({"kind":source_kind,"fileKey":file_key,"requestedFileKey":(!target.is_bridge_only()).then_some(&target.file_key),"canonicalUrl":target.link(None),"nodeIds":input.node_ids,"freshness":"operation-time-read","collection":"explicit-node-list-only"});
    result["partial"] = json!(partial);
    result["status"] = json!(if partial { "partial" } else { "complete" });
    if let Some(calibration) = input.calibration {
        result["calibration"] = calibrate(&result, &calibration);
    }
    result["displayContext"] = match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        server
            .services
            .upstream
            .refresh_bridge_context(&target.file_key),
    )
    .await
    {
        Ok(Ok(context)) => {
            json!({"state":"current","zoom":context.zoom,"uiDevicePixelRatio":context.ui_device_pixel_ratio,"observedAt":context.observed_at,
            "evidenceLimit":"DPR belongs to the plugin UI iframe; neither it nor zoom verifies screenshot-to-canvas scale."})
        }
        _ => {
            json!({"state":"unavailable","zoom":null,"uiDevicePixelRatio":null,"evidenceLimit":"The attached Bridge could not refresh display context. Logical node dimensions do not depend on zoom or DPR."})
        }
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn calibration_requires_consistent_known_dimensions_and_discloses_evidence() {
        let result = json!({"nodes":[{"nodeId":"1:1","width":100,"height":50}]});
        let mut input = super::super::tools::MeasureCalibration {
            node_id: "1:1".into(),
            screen_width_px: 200.0,
            screen_height_px: 100.0,
        };
        assert_eq!(calibrate(&result, &input)["uniformScale"], 2.0);
        input.screen_height_px = 200.0;
        assert_eq!(calibrate(&result, &input)["uniformScale"], Value::Null);
        input.node_id = "2:2".into();
        assert_eq!(calibrate(&result, &input)["state"], "unverified");
    }
    #[test]
    fn malformed_measured_nodes_are_refused_and_untrusted_extras_are_removed() {
        for width in [Value::Null, json!("167"), json!(-1)] {
            assert!(
                validated_node(
                    &json!({"nodeId":"1:1","status":"measured","width":width,"height":94}),
                    "1:1"
                )
                .is_err()
            );
        }
        let valid=validated_node(&json!({"nodeId":"1:1","status":"measured","width":167,"height":94,"code":"untrusted","image":"untrusted"}),"1:1").unwrap();
        assert!(valid.get("code").is_none());
        assert!(valid.get("image").is_none());
        assert!(validated_node(&json!({"nodeId":"2:2","status":"unavailable"}), "1:1").is_err());
    }
}
