fn generated_alias_fixture(label: &str, keys: &str) -> ScopedTempDir {
    let temp = ScopedTempDir::new(label);
    std::fs::write(temp.path().join("package.json"), "{}").unwrap();
    std::fs::create_dir(temp.path().join("df")).unwrap();
    std::fs::write(
        temp.path().join("openapi.json"),
        json!({"paths": {
            "/inquiries": {"post": {"operationId": "create_inquiry"}}
        }})
        .to_string(),
    )
    .unwrap();
    std::fs::write(temp.path().join("df/api.d.ts"), format!(
        "declare module '@devup-api/fetch' {{ interface DevupPrecomputedMethodKeys {{ 'openapi.json': {{ post: {keys}; get: never; }} }} }}"
    )).unwrap();
    temp
}

#[tokio::test]
async fn generated_camel_operation_alias_is_not_client_drift() {
    let temp = generated_alias_fixture("generated-alias", "'/inquiries' | 'createInquiry'");
    std::fs::write(
        temp.path().join("page.tsx"),
        r#"
        import { createApi } from '@devup-api/fetch';
        const client = createApi({});
        client.post('createInquiry', { body: {} });
        import { createInquiry } from '@devup-api/fetch/server';
        pathSchemas.POST['createInquiry'];
        api.post('missingInquiry');
    "#,
    )
    .unwrap();
    let result = run(
        Some(&temp.path().to_string_lossy()),
        &["openapi-client".into()],
    )
    .await
    .unwrap();
    let layer = &result["layers"]["openapi-client"];
    assert_eq!(layer["callsChecked"], 4, "{layer}");
    assert_eq!(layer["generatedOperationAliasMatches"], 3, "{layer}");
    assert_eq!(layer["drifts"].as_array().unwrap().len(), 1, "{layer}");
    assert_eq!(layer["drifts"][0]["identifier"], "missingInquiry");
}

#[tokio::test]
async fn stale_generated_keys_cannot_hide_removed_operations() {
    let temp = generated_alias_fixture("stale-alias", "'createInquiry' | 'deleteInquiry'");
    std::fs::write(
        temp.path().join("page.ts"),
        "api.post('createInquiry'); api.delete('deleteInquiry');",
    )
    .unwrap();
    let result = run(
        Some(&temp.path().to_string_lossy()),
        &["openapi-client".into()],
    )
    .await
    .unwrap();
    let layer = &result["layers"]["openapi-client"];
    assert_eq!(layer["generatedOperationAliasMatches"], 1, "{layer}");
    assert_eq!(layer["drifts"].as_array().unwrap().len(), 1, "{layer}");
    assert_eq!(layer["drifts"][0]["identifier"], "deleteInquiry");
}

#[tokio::test]
async fn operation_alias_requires_generated_evidence_and_respects_maintain_case() {
    let temp = generated_alias_fixture("maintain-alias", "'/inquiries' | 'create_inquiry'");
    std::fs::write(
        temp.path().join("page.ts"),
        "api.post('createInquiry'); api.post('create_inquiry');",
    )
    .unwrap();
    for remove_generated in [false, true] {
        if remove_generated {
            std::fs::remove_file(temp.path().join("df/api.d.ts")).unwrap();
        }
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["openapi-client".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["openapi-client"];
        assert_eq!(layer["generatedOperationAliasMatches"], 0, "{layer}");
        assert_eq!(layer["drifts"].as_array().unwrap().len(), 1, "{layer}");
        assert_eq!(layer["drifts"][0]["identifier"], "createInquiry");
    }
}

#[test]
fn generated_keys_are_scoped_to_the_spec_and_ignore_comment_examples() {
    let source = r#"
        // interface DevupPrecomputedMethodKeys { 'openapi.json': { post: 'commentAlias'; } }
        const example = "interface DevupPrecomputedMethodKeys { 'openapi.json': { post: 'stringAlias'; } }";
        interface Unrelated { 'openapi.json': { post: 'unrelatedAlias'; } }
        interface DevupPrecomputedMethodKeys {
            'other.json': { post: 'otherAlias'; };
            'openapi.json': { post: '/inquiries' | 'createInquiry'; get: never; };
        }
    "#;
    assert_eq!(
        parse::generated_client_keys(source, "openapi.json"),
        BTreeSet::from(["/inquiries".to_owned(), "createInquiry".to_owned(),])
    );
}

#[tokio::test]
async fn saved_exports_do_not_contribute_stale_client_calls() {
    let temp = generated_alias_fixture("saved-alias", "'createInquiry'");
    std::fs::write(temp.path().join("page.ts"), "api.post('createInquiry');").unwrap();
    for folder in [".devup/backup", ".agents/skills", ".opencode/skill"] {
        let dir = temp.path().join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("old-page.tsx"), "api.post('removedOperation');").unwrap();
    }
    let result = run(
        Some(&temp.path().to_string_lossy()),
        &["openapi-client".into()],
    )
    .await
    .unwrap();
    let layer = &result["layers"]["openapi-client"];
    assert_eq!(layer["callsChecked"], 1, "{layer}");
    assert_eq!(layer["filesScanned"], 1, "{layer}");
    assert!(layer["drifts"].as_array().unwrap().is_empty(), "{layer}");
}
