use super::*;

/// Capture inventory only: never a verdict that styles or runtime states work.
pub(super) fn for_candidate(snapshot: &Snapshot, node: &RawNode) -> Option<Value> {
    if node.node_type != "INSTANCE" || !captured_default(node) {
        return None;
    }
    let view = node.typed_view();
    let set_id = view.string("mainComponentSetId").or_else(|| {
        let master = snapshot.nodes.get(view.string("mainComponentId")?)?;
        let parent = master.typed_view().string("parentId")?;
        (snapshot.nodes.get(parent)?.node_type == "COMPONENT_SET").then_some(parent)
    })?;
    let set = snapshot
        .nodes
        .get(set_id)
        .filter(|set| set.node_type == "COMPONENT_SET");
    let declared_known = set
        .and_then(|set| set.fields.get("childrenIds"))
        .is_some_and(Value::is_array);
    let declared = set
        .map(|set| set.typed_view().child_ids().collect::<Vec<_>>())
        .unwrap_or_default();
    let captured = declared
        .iter()
        .filter(|id| {
            snapshot
                .nodes
                .get(**id)
                .is_some_and(|n| n.node_type == "COMPONENT")
        })
        .count();
    let mut missing = set.is_none() || declared.is_empty() || captured != declared.len();
    let mut unreadable = false;
    let mut seen = BTreeSet::new();
    let mut pending = declared
        .iter()
        .map(|id| (*id).to_owned())
        .collect::<Vec<_>>();
    if let Some(set) = set {
        unreadable = !set.field_errors.is_empty();
    }
    while let Some(id) = pending.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        if let Some(node) = snapshot.nodes.get(&id) {
            unreadable |= !node.field_errors.is_empty();
            pending.extend(node.typed_view().child_ids().map(str::to_owned));
        } else {
            missing = true;
        }
    }
    let state = if set.is_none() {
        "missing"
    } else if unreadable || !declared_known {
        "unverified"
    } else if missing {
        "partial"
    } else {
        "captured"
    };
    let mut result = json!({"state":state,"sourceComponentSetId":set_id,
        "capturedVariantCount":captured,"declaredVariantCount":declared_known.then_some(declared.len()),
        "captureInventoryOnly":true,"isStyleError":false,"runtimeStateVerified":false,
        "evidenceLimit":"Declared variant capture only. A default page export or screenshot does not prove pseudo-state styling, semantic ownership or runtime behavior."});
    if state != "captured" {
        if local_document_set(snapshot, set_id) && safe_source_url(snapshot, set_id) {
            result["nextAction"] = json!({"tool":"devup_figma_export","arguments":{
                "url":format!("https://www.figma.com/design/{}?node-id={}",snapshot.file_key,set_id.replace(':',"-")),
                "outputs":["componentTsx"],"debug":true},
                "how":"Export the full local component set, then apply its captured selectors at the actual focusable control boundary while preserving instance geometry."});
        } else {
            result["lookupRequired"] = json!({"componentSetId":set_id,"output":"componentTsx",
                "reason":"Owning local/library file is not proven by this snapshot. Resolve the component set owner before constructing an export URL."});
        }
    }
    Some(result)
}

fn captured_default(node: &RawNode) -> bool {
    node.typed_view()
        .value("variantProperties")
        .and_then(Value::as_object)
        .is_some_and(|values| {
            values.values().any(|v| {
                v.as_str()
                    .is_some_and(|v| v.eq_ignore_ascii_case("default"))
            })
        })
        || node
            .typed_view()
            .value("componentProperties")
            .and_then(Value::as_object)
            .is_some_and(|values| {
                values.values().any(|v| {
                    v["type"] == "VARIANT"
                        && v["value"]
                            .as_str()
                            .is_some_and(|v| v.eq_ignore_ascii_case("default"))
                })
            })
}

/// A set ID alone can refer to a remote library. Require its actual parent
/// chain and child edges to reach a captured document/page root in this file.
fn local_document_set(snapshot: &Snapshot, set_id: &str) -> bool {
    let mut current = set_id;
    let mut seen = BTreeSet::new();
    while seen.insert(current.to_owned()) {
        let Some(node) = snapshot.nodes.get(current) else {
            return false;
        };
        if snapshot.roots.iter().any(|root| root == current)
            && matches!(node.node_type.as_str(), "PAGE" | "DOCUMENT")
        {
            return true;
        }
        if node.field_errors.contains_key("parentId") {
            return false;
        }
        let Some(parent_id) = node.typed_view().string("parentId") else {
            return false;
        };
        let Some(parent) = snapshot.nodes.get(parent_id) else {
            return false;
        };
        if parent.field_errors.contains_key("childrenIds") {
            return false;
        }
        if !parent.typed_view().child_ids().any(|id| id == current) {
            return false;
        }
        current = parent_id;
    }
    false
}
fn safe_source_url(snapshot: &Snapshot, set_id: &str) -> bool {
    !snapshot.file_key.is_empty()
        && snapshot.file_key.chars().all(|c| c.is_ascii_alphanumeric())
        && set_id.split_once(':').is_some_and(|(left, right)| {
            !left.is_empty()
                && !right.is_empty()
                && left.chars().all(|c| c.is_ascii_digit())
                && right.chars().all(|c| c.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cta() -> Snapshot {
        serde_json::from_str(include_str!(
            "../../../../../fixtures/feedback/cta-default-state-capture.json"
        ))
        .unwrap()
    }
    fn full() -> Snapshot {
        let mut source: Snapshot = serde_json::from_str(include_str!(
            "../../../../../fixtures/feedback/button-state-component-set.json"
        ))
        .unwrap();
        source.nodes.extend(cta().nodes);
        source
    }
    #[test]
    fn actual_cta_default_warns_capture_missing_without_inventing_library_url() {
        let source = cta();
        let value = for_candidate(&source, &source.nodes["45:2075"]).unwrap();
        assert_eq!(value["state"], "missing");
        assert_eq!(value["sourceComponentSetId"], "23:1613");
        assert_eq!(value["runtimeStateVerified"], false);
        assert_eq!(value["isStyleError"], false);
        assert!(value["nextAction"].is_null());
        assert!(value["lookupRequired"].is_object());
    }
    #[test]
    fn actual_full_button_set_is_captured_but_not_runtime_verified() {
        let source = full();
        let value = for_candidate(&source, &source.nodes["45:2075"]).unwrap();
        assert_eq!(value["state"], "captured");
        assert_eq!(value["declaredVariantCount"], 32);
        assert_eq!(value["capturedVariantCount"], 32);
        assert_eq!(value["runtimeStateVerified"], false);
        assert!(value["nextAction"].is_null());
    }
    #[test]
    fn incomplete_local_document_set_has_explicit_same_file_export_action() {
        let mut source = full();
        source.nodes.remove("24:1856");
        source
            .nodes
            .get_mut("23:1613")
            .unwrap()
            .fields
            .insert("parentId".into(), json!("local-page"));
        source.nodes.insert(
            "local-page".into(),
            serde_json::from_value(
                json!({"id":"local-page","type":"PAGE","fields":{"childrenIds":["23:1613"]}}),
            )
            .unwrap(),
        );
        source.roots = vec!["local-page".into()];
        let value = for_candidate(&source, &source.nodes["45:2075"]).unwrap();
        assert_eq!(value["state"], "partial");
        assert_eq!(
            value["nextAction"]["arguments"]["outputs"],
            json!(["componentTsx"])
        );
        let url = value["nextAction"]["arguments"]["url"].as_str().unwrap();
        assert!(url.contains(&source.file_key) && url.ends_with("23-1613"));
    }
    #[test]
    fn business_selection_and_non_instance_do_not_become_style_failures() {
        let mut source = cta();
        let node = source.nodes.get_mut("45:2075").unwrap();
        node.fields
            .insert("variantProperties".into(), json!({"state":"selected"}));
        node.fields.remove("componentProperties");
        assert!(for_candidate(&source, &source.nodes["45:2075"]).is_none());
        source.nodes.get_mut("45:2075").unwrap().node_type = "FRAME".into();
        assert!(for_candidate(&source, &source.nodes["45:2075"]).is_none());
    }
    #[test]
    fn unavailable_declared_children_is_unverified_not_a_zero_variant_claim() {
        let mut source = full();
        source
            .nodes
            .get_mut("23:1613")
            .unwrap()
            .fields
            .remove("childrenIds");
        let value = for_candidate(&source, &source.nodes["45:2075"]).unwrap();
        assert_eq!(value["state"], "unverified");
        assert!(value["declaredVariantCount"].is_null());
        assert!(value["nextAction"].is_null());
        assert_eq!(value["isStyleError"], false);
    }
    #[test]
    fn evidence_candidate_carries_capture_inventory() {
        let source = cta();
        let output = super::super::evidence(&source, &source.roots);
        let candidate = output["interactiveCandidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["nodeId"] == "45:2075")
            .unwrap();
        assert_eq!(
            candidate["states"]["fullInteractiveStateCapture"]["state"],
            "missing"
        );
        assert_eq!(candidate["states"]["runtimeStateVerified"], false);
    }
}
