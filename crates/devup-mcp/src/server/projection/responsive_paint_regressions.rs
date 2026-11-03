#[tokio::test]
async fn r13_known_empty_desktop_paint_is_verified_and_strict_accepts() {
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { all_screens, strict, .. } = &mut op {
        *all_screens = true;
        *strict = true;
    }
    let mut data = responsive_payload();
    // This positive strict fixture declares its size policy too; the generic
    // payload deliberately leaves dimensions unexplained for other audits.
    for node in data.snapshot.nodes.values_mut() {
        node.fields.extend(serde_json::from_value::<std::collections::BTreeMap<String, Value>>(
            json!({"layoutMode":"HORIZONTAL","layoutSizingHorizontal":"FIXED",
                "layoutSizingVertical":"FIXED","parentType":"SECTION"}),
        ).unwrap());
    }
    data.snapshot.nodes.get_mut("1:1").unwrap().fields.insert(
        "fills".into(), json!([{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]),
    );
    let result = project(data, op).await.unwrap();
    assert_eq!(result["mappingComplete"], true, "{result}");
    assert_eq!(result["outputResults"]["responsiveTsx"]["fidelity"]["mergedMappingVerified"], true);
    let code: String = result["responsiveTsx"].as_str().unwrap().split_whitespace().collect();
    assert!(code.contains("bg={[\"#F00\",null,null,null,\"initial\"]}"), "{code}");
}

#[tokio::test]
async fn r13_responsive_mapping_gap_is_explicit_and_strict_rejects() {
    let mut op = operation(&["responsiveTsx"]);
    if let PendingOperation::Export { all_screens, .. } = &mut op {
        *all_screens = true;
    }
    // Desktop paint is unreadable, so a mobile fill cannot be safely
    // cleared or accepted as a verified match at that width.
    let mut data = responsive_payload();
    data.snapshot.nodes.get_mut("1:1").unwrap().fields.insert(
        "fills".into(),
        json!([{"type":"SOLID","color":{"r":1,"g":0,"b":0}}]),
    );
    data.snapshot.nodes.get_mut("1:2").unwrap().fields.insert(
        "fills".into(), json!({"$unsupported":"paint read failed"}),
    );
    let result = project(data.clone(), op.clone()).await.unwrap();
    assert!(result["responsiveTsx"].is_string());
    assert_eq!(result["mappingComplete"], false);
    let merge = result["projectionIssues"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|d| {
            d["details"]["unmappedProperties"]
                .as_array()
                .into_iter()
                .flatten()
        })
        .find(|p| p["reason"] == "unverified-responsive-merge")
        .unwrap();
    assert!(
        merge["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap["property"] == "bg" && gap["slot"] == 4 && gap["widthNodeId"] == "1:2"),
        "{merge}"
    );
    assert_ne!(
        result["outputResults"]["responsiveTsx"]["projection"],
        "exact"
    );
    assert!(
        result["projectionIssues"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "DEVUP_CODEGEN_PROPERTY_UNMAPPED"
                && d["details"]["output"] == "responsiveTsx"
                && d["fidelityImpact"] == "none")
    );
    if let PendingOperation::Export { strict, .. } = &mut op {
        *strict = true;
    }
    assert!(project(data, op).await.is_err());
}

