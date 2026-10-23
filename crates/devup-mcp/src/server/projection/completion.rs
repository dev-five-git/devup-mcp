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
        attach_node_page(source, payload);
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
    // The page the user has open in Figma - not the page the node lives on.
    // `nodePage` (when the snapshot reaches it) says where the node is.
    source["viewerPageName"] = json!(served.page_name);
    source["bridgeReads"] = json!(served.reads);
}

/// The page the requested node actually belongs to, walked up from the
/// snapshot's parent links, with a note when it is not the page the viewer has
/// open or when its name says trash/archive/draft.
fn attach_node_page(source: &mut Value, payload: &CollectedPayload) {
    let Some(start) = payload.target.node_id.as_deref() else {
        return;
    };
    if let Some(reason) = devup_mcp_figma::node_excluded_reason(&payload.snapshot, start) {
        source["excluded"] = json!(reason);
    }
    let mut current = Some(start.to_owned());
    let mut visited = std::collections::BTreeSet::new();
    let mut page: Option<(String, String)> = None;
    while let Some(id) = current {
        if !visited.insert(id.clone()) {
            break;
        }
        let Some(node) = payload.snapshot.nodes.get(&id) else {
            break;
        };
        if node.node_type == "PAGE" {
            page = Some((
                node.id.clone(),
                node.typed_view().name().unwrap_or_default().to_owned(),
            ));
            break;
        }
        current = node.typed_view().string("parentId").map(str::to_owned);
    }
    let Some((id, name)) = page else {
        return;
    };
    source["nodePage"] = json!({"id": id, "name": name});
    let lowered = name.to_lowercase();
    if let Some(keyword) = [
        "trash", "archive", "draft", "copy", "pdf", "export", "휴지통", "보관", "초안", "복사",
        "저장용", "출력",
    ]
    .iter()
    .find(|keyword| lowered.contains(*keyword))
    {
        source["nodePage"]["excluded"] = json!(format!("{keyword} ({name})"));
        source["nodePageNote"] = json!(format!(
            "The node lives on the page \"{name}\", whose name marks it as trash, archive or draft."
        ));
    } else if source
        .get("viewerPageName")
        .and_then(Value::as_str)
        .is_some_and(|viewer| viewer != name)
    {
        source["nodePageNote"] = json!(format!(
            "viewerPageName is the page open in Figma; the node itself lives on \"{name}\"."
        ));
    }
}

/// The file key an answer may state: the plugin's own report when the bridge
/// read the file, and the requested key when Figma itself served it.
fn vouched_file_key(payload: &CollectedPayload) -> Option<&str> {
    match &payload.stats.bridge {
        Some(served) => served.file_key.as_deref(),
        None => Some(payload.target.file_key.as_str()),
    }
}


#[cfg(test)]
mod node_page_tests {
    use super::*;
    use devup_mcp_figma::{
        CollectionScope, CollectionStats, FigmaTarget, PayloadCompleteness, RawNode, Snapshot,
    };

    fn payload_on_page(page_name: &str) -> CollectedPayload {
        let nodes = [("0:1", "PAGE", page_name, None), ("1:1", "FRAME", "Home", Some("0:1"))]
            .into_iter()
            .map(|(id, node_type, name, parent)| {
                (
                    id.to_owned(),
                    RawNode {
                        id: id.to_owned(),
                        node_type: node_type.to_owned(),
                        fields: serde_json::from_value(json!({"name": name, "parentId": parent}))
                            .unwrap(),
                        extra: Default::default(),
                        field_errors: Default::default(),
                    },
                )
            })
            .collect();
        CollectedPayload {
            target: FigmaTarget {
                file_key: "k".into(),
                node_id: Some("1:1".into()),
                branch_key: None,
            },
            scope: CollectionScope::Node,
            metadata: json!({}),
            snapshot: Snapshot {
                file_key: "k".into(),
                version: None,
                roots: vec!["0:1".into()],
                nodes,
                diagnostics: vec![],
            },
            variables: None,
            styles: None,
            completeness: PayloadCompleteness::ResolvedValuesOnly,
            source_version: None,
            stats: CollectionStats::default(),
            assets: vec![],
            reference_png: None,
            failures: vec![],
        }
    }

    #[test]
    fn a_trash_page_is_named_and_flagged() {
        let mut source = json!({"viewerPageName": "Trash old"});
        attach_node_page(&mut source, &payload_on_page("Trash old"));
        assert_eq!(
            source["nodePage"],
            json!({"id": "0:1", "name": "Trash old", "excluded": "trash (Trash old)"})
        );
        assert!(source["nodePageNote"].as_str().unwrap().contains("trash, archive or draft"));
    }

    #[test]
    fn a_trash_page_sets_the_excluded_marker_on_node_page() {
        let mut source = json!({});
        attach_node_page(&mut source, &payload_on_page("PDF 저장용"));
        assert!(source["nodePage"]["excluded"].as_str().unwrap().contains("pdf"));
        let mut source = json!({});
        attach_node_page(&mut source, &payload_on_page("Screens"));
        assert!(source["nodePage"].get("excluded").is_none());
    }

    #[test]
    fn a_node_off_the_viewer_page_says_so() {
        let mut source = json!({"viewerPageName": "Cover"});
        attach_node_page(&mut source, &payload_on_page("Screens"));
        assert_eq!(source["nodePage"]["name"], "Screens");
        assert!(source["nodePageNote"].as_str().unwrap().contains("lives on \"Screens\""));
    }

    #[test]
    fn the_viewer_page_needs_no_note() {
        let mut source = json!({"viewerPageName": "Screens"});
        attach_node_page(&mut source, &payload_on_page("Screens"));
        assert!(source.get("nodePageNote").is_none());
    }
}
