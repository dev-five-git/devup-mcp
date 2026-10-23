use super::*;

fn plan(scope: &str, columns: Value) -> Value {
    json!({"facts":{"parts":"API"},"features":[{"id":"catalog","scope":scope,"entities":["post"],"screens":["1:1"]}],
        "entities":[{"name":"post","columns":columns}]})
}

#[test]
fn resource_paths_pluralize_the_last_word() {
    for (name, path) in [("inquiry","/inquiries"),("category","/categories"),("news","/news"),("series","/series"),("posts","/posts"),("status","/statuses"),("box","/boxes"),("press_release","/press-releases")] {
        let mut plan = plan("list", json!([]));
        plan["entities"][0]["name"] = json!(name);
        plan["features"][0]["entities"] = json!([name]);
        wiring::draft(&mut plan);
        assert_eq!(plan["endpointContracts"][0]["path"], path, "{name}");
    }
}

#[test]
fn list_contracts_draft_queries_only_from_relevant_evidence() {
    let mut plan = plan("List pagination sort search filter", json!([
        {"name":"title","type":"text","nullable":false},
        {"name":"created_at","type":"timestamptz","nullable":false},
        {"name":"views","type":"integer","nullable":false},
        {"name":"body","type":"text","nullable":false},
        {"name":"category_id","type":"integer","nullable":false},
        {"name":"state","type":{"kind":"enum","values":["draft","published"]},"nullable":false}
    ]));
    wiring::draft(&mut plan);
    let queries = array(&plan["endpointContracts"][0]["queryParams"]);
    for name in ["page","pageSize","sort","q","category_id","state"] {
        assert!(queries.iter().any(|query| query["name"] == name), "{name}: {queries:?}");
    }
    let sort = queries.iter().find(|query| query["name"] == "sort").unwrap();
    assert_eq!(sort["allowedKeys"], json!(["title","created_at","views","category_id"]));
    let mut plain = plan.clone();
    plain["features"][0]["scope"] = json!("List");
    wiring::draft(&mut plain);
    assert_eq!(plain["endpointContracts"][0]["queryParams"], json!([]));
}

#[test]
fn write_contracts_include_validation_and_delete_has_no_response_fields() {
    let mut plan = plan("Create edit CMS", json!([
        {"name":"email","type":{"kind":"varchar","length":120},"nullable":false},
        {"name":"phone","type":"text","nullable":true},
        {"name":"website_link","type":"text","nullable":true},
        {"name":"privacy_consent","type":"boolean","nullable":false},
        {"name":"state","type":{"kind":"enum","values":["draft","sent"]},"nullable":false}
    ]));
    wiring::draft(&mut plan);
    for operation in ["create","update"] {
        let contract = array(&plan["endpointContracts"]).iter().find(|c| c["operation"] == operation).unwrap();
        let fields = array(&contract["requestFields"]);
        let email = fields.iter().find(|field| field["name"] == "email").unwrap();
        assert_eq!(email["rules"]["maxLength"], 120);
        assert_eq!(email["rules"]["format"], "email");
        assert_eq!(email["rules"]["required"], true);
        assert_eq!(fields[1]["rules"]["format"], "phone");
        assert_eq!(fields[2]["rules"]["format"], "url");
        assert_eq!(fields[3]["rules"]["const"], true);
        assert_eq!(fields[4]["rules"]["enum"], json!(["draft","sent"]));
    }
    let delete = array(&plan["endpointContracts"]).iter().find(|c| c["operation"] == "delete").unwrap();
    assert_eq!(delete["method"], "DELETE");
    assert_eq!(delete["path"], "/posts/{id}");
    assert_eq!(delete["responseFields"], json!([]));
}

#[test]
fn structural_screens_supply_operations_and_form_field_candidates() {
    let mut plan = plan("Catalog", json!([{"name":"email_address","type":"text","nullable":false}]));
    plan["facts"]["screens"] = json!([{"nodeId":"1:1","url":"/catalog/[slug]","parts":["Tile","Tile","Tile"],
        "children":[{"type":"TEXT_INPUT","label":"EmailAddress"},{"name":"TextField","label":"Unknown"}]}]);
    wiring::draft(&mut plan);
    let mapping = &plan["dataMappings"][0];
    for op in ["list","get","create"] {
        assert!(array(&mapping["requiredOperations"]).contains(&json!(op)), "{mapping}");
        assert!(!array(&mapping["operationEvidence"][op]).is_empty());
    }
    assert!(array(&mapping["formFields"]).iter().any(|field| field["label"] == "EmailAddress" && field["column"] == "email_address"));
    assert!(array(&mapping["formFields"]).iter().any(|field| field["label"] == "Unknown" && field["status"] == "unmatched"));
}

fn report_contracts(contracts: Value, doc: Value) -> Value {
    let dir = TestDir::new();
    std::fs::write(dir.path().join("openapi.json"), doc.to_string()).unwrap();
    wiring::report(&json!({"endpointContracts":contracts}), &dir.path().to_string_lossy())
}

#[test]
fn openapi_matches_normalized_fields_structural_paths_and_common_prefix() {
    let report = report_contracts(json!([
        {"path":"/posts/{id}","method":"GET","responseFields":[{"name":"image_url","type":"text"}]},
        {"path":"/posts","method":"GET","operation":"list","responseFields":[{"name":"image-url","type":"text"}]}
    ]), json!({"paths":{
        "/api/posts/{postId}/":{"get":{"responses":{"200":{"content":{"application/json":{"schema":{"properties":{"ImageUrl":{"type":"string"}}}}}}}}},
        "/api/posts":{"get":{"responses":{"200":{"content":{"application/json":{"schema":{"type":"array","items":{"properties":{"imageUrl":{"type":"string"}}}}}}}}}}
    }}));
    assert_eq!(report["contractMissing"], json!([]), "{report}");
    assert_eq!(report["contractMatches"][0]["prefix"], "/api");
    assert_eq!(report["contractMatches"][0]["fields"][0]["openapiName"], "ImageUrl");
}

#[test]
fn openapi_rejects_ambiguous_or_inconsistent_prefixes() {
    for paths in [json!({"/api/posts":{"get":{}},"/cms/posts":{"get":{}}}), json!({"/api/posts":{"get":{}},"/cms/users":{"get":{}}})] {
        let report = report_contracts(json!([{"path":"/posts","method":"GET"},{"path":"/users","method":"GET"}]), json!({"paths":paths}));
        assert!(!array(&report["contractMissing"]).is_empty());
    }
}

#[test]
fn openapi_checks_path_and_operation_query_parameters_with_refs() {
    let report = report_contracts(json!([{"path":"/posts","method":"GET","queryParams":[{"name":"page"},{"name":"pageSize","aliases":["size"]},{"name":"q"},{"name":"sort"}]}]),
        json!({"paths":{"/posts":{"parameters":[{"$ref":"#/components/parameters/Page"}],"get":{"parameters":[{"in":"query","name":"size"},{"in":"header","name":"q"}]}}},
            "components":{"parameters":{"Page":{"in":"query","name":"page"}}}}));
    let reason = text(&report["contractMissing"][0]["reason"]);
    assert!(reason.contains("queryParams missing q") && reason.contains("queryParams missing sort"), "{report}");
    assert!(!reason.contains("missing page"));
}

#[test]
fn openapi_reads_composites_wrappers_refs_and_form_bodies() {
    for media in ["multipart/form-data", "application/x-www-form-urlencoded"] {
        let mut doc = json!({"paths":{"/posts":{"post":{"requestBody":{"content":{}},"responses":{"200":{"$ref":"#/components/responses/List"}}}}},
            "components":{"schemas":{"Post":{"allOf":[{"properties":{"id":{"type":"integer"}}},{"anyOf":[{"properties":{"title":{"$ref":"#/components/schemas/Title"}}},{"properties":{"publishedAt":{"type":["string","null"],"format":"date-time"}}}]}],"required":["id","title"]},"Title":{"type":"string"}},
            "responses":{"List":{"content":{"application/json":{"schema":{"properties":{"data":{"type":"array","items":{"$ref":"#/components/schemas/Post"}}}}}}}}}});
        doc["paths"]["/posts"]["post"]["requestBody"]["content"][media] = json!({"schema":{"oneOf":[{"properties":{"title":{"type":"string"}},"required":["title"]}]}});
        let report = report_contracts(json!([{"path":"/posts","method":"POST","operation":"list","requestFields":[{"name":"title","type":"text","required":true,"nullable":false}],
            "responseFields":[{"name":"id","type":"big_int","required":true,"nullable":false},{"name":"title","type":{"kind":"varchar","length":100},"required":true,"nullable":false},{"name":"published_at","type":"timestamptz","nullable":true}]}]), doc);
        assert_eq!(report["contractMissing"], json!([]), "{report}");
    }
}

#[test]
fn openapi_reports_type_format_nullability_and_required_mismatches() {
    for (field, schema, required, fragment) in [
        (json!({"name":"value","type":"integer"}), json!({"type":"string"}), json!([]), "type mismatch value: expected integer got string"),
        (json!({"name":"value","type":"date"}), json!({"type":"string","format":"date-time"}), json!([]), "expected string(date) got string(date-time)"),
        (json!({"name":"value","type":"boolean","nullable":false}), json!({"type":["boolean","null"]}), json!([]), "nullable mismatch value"),
        (json!({"name":"value","type":"text","required":true}), json!({"type":"string"}), json!([]), "required mismatch value")
    ] {
        let report = report_contracts(json!([{"path":"/posts","method":"GET","responseFields":[field]}]), json!({"paths":{"/posts":{"get":{"responses":{"200":{"content":{"application/json":{"schema":{"properties":{"value":schema},"required":required}}}}}}}}}));
        assert!(text(&report["contractMissing"][0]["reason"]).contains(fragment), "{report}");
    }
}
