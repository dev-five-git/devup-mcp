pub(super) async fn complete_operation(
    operation: PendingOperation,
    payload: &CollectedPayload,
    source_kind: &str,
    artifact: &ArtifactLookup,
    output_policy: &OutputPolicy,
    artifact_store: &ArtifactStore,
) -> Result<Value, DevupError> {
    let project_root = match &operation {
        PendingOperation::Export { theme_options, .. } => theme_options.project_root.clone(),
        _ => None,
    };
    let mut response = project_operation(
        operation,
        payload,
        source_kind,
        artifact,
        output_policy,
        artifact_store,
    )
    .await?;
    if let Some(source) = response
        .get_mut("source")
        .filter(|source| source.is_object())
    {
        attach_bridge_source(source, payload);
    }
    // What this export wrote for each marked node, so the audit can tell
    // code drift from a converter bug later.
    if let Some(root) = project_root.as_deref()
        && response.get("figmaNodeIds").is_some()
    {
        let root_node = payload.target.node_id.clone().unwrap_or_default();
        match super::export_record::write(
            output_policy,
            root,
            &payload.target.file_key,
            &root_node,
            &response,
        ) {
            Ok(path) => response["exportRecord"] = json!(path),
            Err(error) => response["exportRecordError"] = json!(error.message),
        }
    }
    Ok(response)
}

/// Names the Devup Bridge plugin when it carried the design, and never claims
/// a file key the plugin did not report.
///
/// Every answer used to say `kind: "direct"` whatever carried it, and repeated
/// the request's key as the file's - including a key an agent had invented to
/// get past the url field, which the plugin served only because it was the one
/// attached. So the agent could not tell which path had served it, and was
/// told its own invention was the file.
fn attach_bridge_source(source: &mut Value, payload: &CollectedPayload) {
    let fallbacks = &payload.stats.direct_fallbacks;
    if !fallbacks.is_empty() {
        let reasons: std::collections::BTreeSet<&str> =
            fallbacks.iter().map(String::as_str).collect();
        let hints: Vec<&str> = reasons
            .iter()
            .filter_map(|reason| match *reason {
                "file-out-of-sight" => Some(include_str!(
                    "../text/projection-attach_bridge_source-e2f65c04.md"
                )),
                "plugin-older-than-server" => Some(include_str!(
                    "../text/projection-attach_bridge_source-65001189.md"
                )),
                _ => None,
            })
            .collect();
        source["directFallbacks"] = json!(fallbacks.len());
        source["directFallbackReasons"] = json!(reasons);
        if !hints.is_empty() {
            source["directFallbackHint"] = json!(hints.join(" "));
        }
    }
    let Some(served) = &payload.stats.bridge else {
        return;
    };
    // A reused acquisition stays `artifact` - this call read nothing - and
    // says where the acquisition came from.
    if source["kind"] == "direct" {
        source["kind"] = json!("bridge");
    } else {
        source["origin"] = json!("bridge");
    }
    source["bridgePort"] = json!(served.port);
    source["fileKey"] = json!(served.file_key);
    if served.file_key.is_none() {
        source["fileKeyReason"] = json!(
            "The Devup Bridge plugin that served this could not report its file key (figma.fileKey is empty, as in Dev Mode), so no file key is claimed."
        );
        if !payload.target.is_bridge_only() {
            source["requestedFileKey"] = json!(payload.target.file_key);
        }
    }
    source["fileName"] = json!(served.file_name);
    source["pageName"] = json!(served.page_name);
    source["bridgeReads"] = json!(served.reads);
}

/// The file key an answer may state: the plugin's own report when the bridge
/// read the file, and the requested key when Figma itself served it.
fn vouched_file_key(payload: &CollectedPayload) -> Option<&str> {
    match &payload.stats.bridge {
        Some(served) => served.file_key.as_deref(),
        None => Some(payload.target.file_key.as_str()),
    }
}

