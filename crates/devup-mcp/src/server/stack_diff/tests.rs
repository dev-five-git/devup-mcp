
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn route_arguments_ignore_comment_paths() {
        for source in [
            "#[route(get, /* path = \"/fake\" */ tags = [\"/tag\"])]\npub async fn real() {}",
            "#[route(get, // path = \"/fake\"\n)]\npub async fn real() {}",
            "#[route(get, /* outer /* path = \"/fake\" */ end */)]\npub async fn real() {}",
        ] {
            assert_eq!(
                extract_vespera_route_attributes(source),
                vec![("get".into(), None)]
            );
        }
    }

    #[test]
    fn route_arguments_ignore_doc_comment_and_macro_examples() {
        for source in [
            "/// #[route(get, path = \"/fake\")]\npub async fn real() {}",
            "/* #[route(get, path = \"/fake\")] */\npub async fn real() {}",
            "unrelated! { #[route(get, path = \"/fake\")]\npub async fn real() {} }",
        ] {
            assert!(
                extract_vespera_route_attributes(source).is_empty(),
                "{source}"
            );
        }
    }

    #[test]
    fn route_arguments_decode_rust_string_literals() {
        for (literal, expected) in [
            (r##"r"/raw""##, "/raw"),
            (r##"r#"/raw"quote"#"##, "/raw\"quote"),
            (r#""/escaped\"quote""#, "/escaped\"quote"),
            ("\"/continued\\\n    /path\"", "/continued/path"),
            (r#""/\u{61}\x62""#, "/ab"),
        ] {
            let source =
                format!("#[vespera::route(get, path = {literal})]\npub async fn real() {{}}");
            assert_eq!(
                extract_vespera_route_attributes(&source),
                vec![("get".into(), Some(expected.into()))]
            );
        }
    }

    #[test]
    fn route_arguments_multiline_reversed_and_two_handlers() {
        let source = r#"
            #[vespera :: route (
                path = "/first", tags = ["path = fake"], get,
            )]
            #[allow(
                dead_code
            )]
            pub async fn first() {}
            #[route(post, path = "/second")]
            pub async fn second() {}
        "#;
        assert_eq!(
            extract_vespera_route_attributes(source),
            vec![
                ("get".into(), Some("/first".into())),
                ("post".into(), Some("/second".into())),
            ]
        );
    }

    #[test]
    fn route_arguments_reject_unsupported_or_ambiguous_paths() {
        for args in [
            "get, \"/positional\"",
            "get, path = PATH",
            "get, path = concat!(\"/x\")",
            "get, path = \"/one\", path = \"/two\"",
            "get, post",
            "get, path = b\"/bytes\"",
        ] {
            let source = format!("#[route({args})]\npub async fn real() {{}}");
            assert!(
                extract_vespera_route_attributes(&source).is_empty(),
                "{source}"
            );
        }
    }

    #[tokio::test]
    async fn route_arguments_cfg_handlers_are_conditional_with_ownership() {
        let temp = ScopedTempDir::new("route-cfg");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                "#[route(get)]\n#[cfg(feature = \"optional\")]\npub async fn user() {}",
            )],
        );
        std::fs::write(temp.path().join("openapi.json"), r#"{"paths":{}}"#).unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["codeRouteCount"], 0);
        let findings = layer["drifts"].as_array().unwrap();
        let conditional = findings
            .iter()
            .find(|f| f["kind"] == "conditional-route-openapi")
            .unwrap();
        assert_eq!(conditional["confidence"], "low");
        assert!(conditional["sourceOwnership"].is_object());
        assert!(
            !findings
                .iter()
                .any(|f| f["kind"] == "route-missing-from-openapi")
        );
    }

    #[tokio::test]
    async fn route_arguments_unresolved_file_keeps_low_confidence_fallback() {
        let temp = ScopedTempDir::new("route-fallback");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                "#[route(get, path = PATH)]\npub async fn user() {}",
            )],
        );
        std::fs::write(temp.path().join("openapi.json"), r#"{"paths":{}}"#).unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["codeRouteCount"], 0);
        let fallback = layer["drifts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["kind"] == "route-openapi-unresolved-fallback")
            .unwrap();
        assert_eq!(fallback["confidence"], "low");
        assert!(fallback["sourceOwnership"].is_object());
    }

    #[test]
    fn route_arguments_do_not_promote_cfg_modules_or_cfg_attr() {
        for source in [
            "#![cfg(feature = \"optional\")]\n#[route(get)]\npub async fn real() {}",
            "#[cfg(feature = \"optional\")] mod inner { #[route(get)]\npub async fn real() {} }",
            "#[cfg_attr(feature = \"optional\", cfg(unix))]\n#[route(get)]\npub async fn real() {}",
        ] {
            assert!(
                extract_vespera_route_attributes(source).is_empty(),
                "{source}"
            );
        }
    }

    #[test]
    fn route_arguments_do_not_extract_other_namespaces_or_function_bodies() {
        for source in [
            "#[other::route(get, path = \"/fake\")]\npub async fn real() {}",
            "pub async fn outer() { #[route(get)]\npub async fn inner() {} }",
            "#[route(get)]\nfn private() {}\npub async fn unrelated() {}",
        ] {
            assert!(
                extract_vespera_route_attributes(source).is_empty(),
                "{source}"
            );
        }
    }

    #[tokio::test]
    async fn route_arguments_malformed_file_preserves_fallback_evidence() {
        let temp = ScopedTempDir::new("route-malformed");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                "#[route(get, path = \"/known\")]\npub async fn user() {}\nfn broken(",
            )],
        );
        std::fs::write(temp.path().join("openapi.json"), r#"{"paths":{}}"#).unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["codeRouteCount"], 0);
        let fallback = layer["drifts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["kind"] == "route-openapi-unresolved-fallback")
            .unwrap();
        assert_eq!(fallback["confidence"], "low");
        assert_eq!(fallback["routes"][0]["path"], "/user/known");
    }

    #[tokio::test]
    async fn route_arguments_unfinished_unicode_attribute_does_not_panic() {
        let temp = ScopedTempDir::new("route-unicode");
        mapping_project(temp.path(), &[("user.rs", "#[route(경로")]);
        std::fs::write(temp.path().join("openapi.json"), r#"{"paths":{}}"#).unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["codeRouteCount"], 0);
        assert_eq!(
            layer["drifts"][0]["kind"],
            "route-openapi-unresolved-fallback"
        );
    }

    #[tokio::test]
    async fn route_arguments_cfg_module_declaration_gates_child_file() {
        let temp = ScopedTempDir::new("route-module-cfg");
        mapping_project(
            temp.path(),
            &[
                ("mod.rs", "#[cfg(feature = \"optional\")]\npub mod user;"),
                ("user.rs", "#[route(get)]\npub async fn user() {}"),
            ],
        );
        std::fs::write(
            temp.path().join("openapi.json"),
            r#"{"paths":{"/user/":{"get":{}}}}"#,
        )
        .unwrap();
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".into()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["codeRouteCount"], 0);
        assert_eq!(layer["spellingNormalizedMatches"], 0);
        let findings = layer["drifts"].as_array().unwrap();
        assert!(
            findings
                .iter()
                .any(|f| f["kind"] == "conditional-route-openapi")
        );
        assert!(
            !findings
                .iter()
                .any(|f| f["kind"] == "openapi-path-not-found-in-scanned-routes")
        );
    }
    fn mapping_project(root: &Path, routes: &[(&str, &str)]) {
        std::fs::write(root.join("package.json"), "{}").unwrap();
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::create_dir_all(root.join("src/routes")).unwrap();
        for table in ["user", "team"] {
            std::fs::write(
                root.join(format!("models/{table}.json")),
                json!({
                    "name": table, "columns": [{"name":"id"}, {"name":"email"}, {"name":"secret"}]
                })
                .to_string(),
            )
            .unwrap();
        }
        for (name, source) in routes {
            std::fs::write(root.join("src/routes").join(name), source).unwrap();
        }
    }

    #[tokio::test]
    async fn parsed_route_mapping_ignores_comments_strings_and_other_models() {
        let temp = ScopedTempDir::new("exact-map");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                r#"
            schema_type!(User from crate::models::user::Model, pick = [id]);
            schema_type!(Team from crate::models::team::Model, omit = [secret]);
            // email secret crate::models::user::Column::Email
            const TEXT: &str = "crate::models::user::Model";
            fn unrelated() { let email_secret = 1; }
        "#,
            )],
        );
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".into()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["entity-route"]["drifts"]
            .as_array()
            .unwrap();
        let columns = drifts
            .iter()
            .map(|d| (d["table"].as_str().unwrap(), d["column"].as_str().unwrap()))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            columns,
            BTreeSet::from([("user", "email"), ("user", "secret"), ("team", "secret")])
        );
        assert!(
            drifts
                .iter()
                .all(|d| d["kind"] == "column-never-referenced-in-routes")
        );
    }

    #[tokio::test]
    async fn parsed_route_mapping_direct_references_are_table_scoped() {
        let temp = ScopedTempDir::new("direct-map");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                r#"
            fn route() { crate::models::user::Column::Email; crate::models::team::Entity::find(); }
        "#,
            )],
        );
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".into()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["entity-route"]["drifts"]
            .as_array()
            .unwrap();
        assert_eq!(drifts.len(), 2, "{result}");
        assert!(
            drifts
                .iter()
                .all(|d| d["table"] == "user" && d["column"] != "email")
        );
    }

    #[tokio::test]
    async fn unresolved_route_files_keep_separate_low_confidence_fallback() {
        let temp = ScopedTempDir::new("fallback-map");
        mapping_project(temp.path(), &[("user.rs", "fn route() { let id = 1; }")]);
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".into()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["entity-route"]["drifts"]
            .as_array()
            .unwrap();
        assert_eq!(drifts.len(), 4);
        assert!(drifts.iter().all(|d| d["kind"]
            == "column-never-referenced-in-routes-text-fallback"
            && d["confidence"] == "low"));
    }

    #[test]
    fn extracts_all_literal_client_forms() {
        for source in [
            r#"<ApiForm api={api} method="post" path="missingOperation" />"#,
            r#"<ApiForm path='/users/{id}' method='patch' api={api} />"#,
            "import { missingOperation as localName, anotherOperation } from '@devup-api/fetch/server';",
            "pathSchemas.GET['missingOperation']",
            "queryClient.getQueryKey('GET', 'missingOperation')",
            "useSuspenseQuery('get', 'missingOperation')",
            "useInfiniteQuery('get', 'missingOperation')",
            "useQueries([['get', 'missingOperation', { nested: ['get', 'notACall'] }], ['post', 'anotherOperation']])",
        ] {
            let calls = extract_devup_api_calls(source);
            assert!(!calls.is_empty(), "unrecognised form: {source}");
            assert!(!calls.iter().any(|(_, id)| id == "notACall"));
        }
    }

    #[test]
    fn client_extraction_preserves_literals_and_ignores_lookalikes() {
        let source = r##"
            // pathSchemas.GET['comment']
            const example = "queryClient.getQueryKey('get', 'string')";
            <OtherForm method="post" path="notAForm" />;
            <ApiForm method="get" path="notAMutation" />;
            <ApiForm method="delete" path={dynamicPath} />;
            import { wrongModule } from 'another/server';
            pathSchemas.NOT_A_METHOD['wrongMethod'];
            longeruseQueries([['get', 'wrongFunction']]);
            queryClient.getQueryKey('GET', '/users/{userId}/');
            useQueries([['get', 'one'], ['post', 'two', { nested: ['get', 'notACall'] }]]);
        "##;
        let ids = extract_devup_api_calls(source)
            .into_iter()
            .map(|(_, id)| id)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            ids,
            BTreeSet::from(["/users/{userId}/".into(), "one".into(), "two".into()])
        );
    }

    #[tokio::test]
    async fn mixed_route_resolution_preserves_fallback_without_cross_model_leaks() {
        let temp = ScopedTempDir::new("mixed-map");
        mapping_project(
            temp.path(),
            &[
                (
                    "explicit.rs",
                    "schema_type!(User from crate::models::user::Model, pick = [id]); // secret",
                ),
                ("unresolved.rs", "fn handler() { let email = 1; }"),
            ],
        );
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".into()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["entity-route"]["drifts"]
            .as_array()
            .unwrap();
        assert_eq!(drifts.len(), 3, "{result}");
        assert!(
            drifts
                .iter()
                .all(|d| d["kind"] == "column-never-referenced-in-routes-text-fallback")
        );
        assert!(
            drifts
                .iter()
                .any(|d| d["table"] == "user" && d["column"] == "secret")
        );
    }

    #[tokio::test]
    async fn schema_mapping_handles_omit_raw_identifiers_and_grouped_model_imports() {
        let temp = ScopedTempDir::new("schema-map");
        mapping_project(
            temp.path(),
            &[(
                "user.rs",
                r##"
            vespera::schema_type! {
                User from crate :: models :: user :: Model, omit = [r#secret]
            }
            use crate::models::team::{Entity, Model, Column};
            const EXAMPLE: &str = r#"crate::models::user::Model"#;
            /* nested /* crate::models::user::Model */ ignored */
        "##,
            )],
        );
        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".into()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["entity-route"]["drifts"]
            .as_array()
            .unwrap();
        assert_eq!(drifts.len(), 1, "{result}");
        assert_eq!(drifts[0]["table"], "user");
        assert_eq!(drifts[0]["column"], "secret");
        assert_eq!(drifts[0]["kind"], "column-never-referenced-in-routes");
    }

    #[test]
    fn incomplete_rust_syntax_keeps_unresolved_mapping_without_panicking() {
        for source in [
            "r#",
            "schema_type!(User from crate::models::user::Model, pick = [id]",
            "schema_type!(User from crate::models::user::Model, unsupported = [id]);",
        ] {
            let mapping = parse::route_mapping(source);
            assert!(mapping.unresolved, "{source}");
            assert!(!mapping.mentions("user", "secret"), "{source}");
        }
    }

    #[tokio::test]
    async fn new_client_forms_share_normalization_and_crud_tag_resolution() {
        let temp = ScopedTempDir::new("client-forms");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("openapi.json"),
            json!({"paths": {
                "/users/{user-id}": {"get": {"operationId":"getUser", "tags":["devup:user:one"]},
                    "post": {"operationId":"createUser", "tags":["devup:user:create"]},
                    "put": {"operationId":"editUser", "tags":["devup:user:edit"]},
                    "patch": {"operationId":"fixUser", "tags":["devup:user:fix"]}}
            }})
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            temp.path().join("page.tsx"),
            r#"
            <ApiForm api={api} method="post" path="/users/{userId}/" />
            pathSchemas.GET['/users/{userId}'];
            queryClient.getQueryKey('GET', '/users/{userId}');
            useQueries([['get', '/users/{userId}'], ['post', '/users/{userId}/missing']]);
            import { getUser as local, missingServerOperation } from '@devup-api/fetch/server';
            <ApiCrud config={crudConfigs.user} />
            useApiCrud({ config: crudConfigs.user });
            <ApiCrud config={crudConfigs.missing} />
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
        assert_eq!(layer["spellingNormalizedMatches"], 4, "{layer}");
        assert_eq!(layer["callsChecked"], 16, "{layer}");
        let drifts = layer["drifts"].as_array().unwrap();
        assert_eq!(drifts.len(), 3, "{layer}");
        assert!(
            drifts
                .iter()
                .any(|d| d["identifier"] == "/users/{userId}/missing")
        );
        assert!(
            drifts
                .iter()
                .any(|d| d["identifier"] == "missingServerOperation")
        );
        assert!(
            drifts
                .iter()
                .any(|d| d["kind"] == "client-crud-config-not-in-openapi")
        );
    }

    #[tokio::test]
    async fn each_new_client_form_reports_unknown_identifiers_verbatim() {
        let temp = ScopedTempDir::new("each-client-form");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("openapi.json"),
            r#"{"paths":{"/users/{user-id}":{"get":{"operationId":"getUser"}}}}"#,
        )
        .unwrap();
        for template in [
            "<ApiForm api={api} method=\"post\" path=\"IDENTIFIER\" />",
            "pathSchemas.GET['IDENTIFIER']",
            "queryClient.getQueryKey('GET', 'IDENTIFIER')",
            "useSuspenseQuery('get', 'IDENTIFIER')",
            "useInfiniteQuery('get', 'IDENTIFIER')",
            "useQueries([['get', 'IDENTIFIER']])",
        ] {
            for (identifier, expected_drifts, normalized) in [
                ("/users/{userId}/", 0, 1),
                ("getUser", 0, 0),
                ("/users/{otherId}", 1, 0),
                ("/users/{userId}/extra", 1, 0),
                ("missingOperation", 1, 0),
            ] {
                std::fs::write(
                    temp.path().join("page.tsx"),
                    template.replace("IDENTIFIER", identifier),
                )
                .unwrap();
                let result = run(
                    Some(&temp.path().to_string_lossy()),
                    &["openapi-client".into()],
                )
                .await
                .unwrap();
                let layer = &result["layers"]["openapi-client"];
                assert_eq!(layer["callsChecked"], 1, "{template}: {layer}");
                assert_eq!(
                    layer["spellingNormalizedMatches"], normalized,
                    "{template}: {layer}"
                );
                assert_eq!(
                    layer["drifts"].as_array().unwrap().len(),
                    expected_drifts,
                    "{template}: {layer}"
                );
                if expected_drifts > 0 {
                    assert_eq!(layer["drifts"][0]["identifier"], identifier);
                }
            }
        }
    }

    #[tokio::test]
    async fn crud_resolution_requires_a_recognized_devup_role() {
        let temp = ScopedTempDir::new("crud-tags");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(temp.path().join("openapi.json"), json!({"paths": {
            "/users": {"get": {"tags":["devup:valid:one", "devup:wrong:list", "devup:extra:one:more", "ordinary"]}}
        }}).to_string()).unwrap();
        std::fs::write(
            temp.path().join("page.tsx"),
            r#"
            <ApiCrud config={crudConfigs.valid} />
            useApiCrud({ config: crudConfigs.wrong });
            <ApiCrud config={crudConfigs.extra} />
            <OtherCrud config={crudConfigs.ignored} />
            const unusedConfig = crudConfigs.unused;
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
        assert_eq!(layer["callsChecked"], 3, "{layer}");
        let names = layer["drifts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["identifier"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(names, BTreeSet::from(["wrong", "extra"]));
    }

    #[tokio::test]
    async fn every_drift_identifies_ownership_and_upstream_repair() {
        let temp = ScopedTempDir::new("ownership");
        mapping_project(
            temp.path(),
            &[("user.rs", "#[vespera::route(get)]\npub async fn user() {}")],
        );
        std::fs::write(
            temp.path().join("openapi.json"),
            r#"{"paths":{"/old":{"post":{}}}}"#,
        )
        .unwrap();
        std::fs::write(temp.path().join("page.ts"), "api.get('missing')").unwrap();
        let result = run(Some(&temp.path().to_string_lossy()), &[])
            .await
            .unwrap();
        for (name, layer) in result["layers"].as_object().unwrap() {
            let drifts = layer["drifts"].as_array().unwrap();
            assert!(!drifts.is_empty(), "{name}");
            for drift in drifts {
                let ownership = &drift["sourceOwnership"];
                assert!(ownership["humanAuthored"].is_array(), "{drift}");
                assert!(ownership["generated"].is_array(), "{drift}");
                let repair = ownership["repairDirection"]
                    .as_str()
                    .expect("repair direction");
                assert!(!repair.contains("edit openapi.json"));
                assert!(!repair.contains("edit the entity"));
                assert!(matches!(
                    drift["confidence"].as_str(),
                    Some("low" | "medium")
                ));
            }
        }
    }

    struct ScopedTempDir(PathBuf);

    impl ScopedTempDir {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devup-mcp-stackdiff-test-{label}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create scoped temp dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for ScopedTempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn extracts_model_struct_fields_ignoring_derive_attributes() {
        let source = r##"
            use sea_orm::entity::prelude::*;

            #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
            #[sea_orm(table_name = "user")]
            pub struct Model {
                #[sea_orm(primary_key, auto_increment = false)]
                pub id: Uuid,
                #[sea_orm(unique)]
                pub email: String,
                pub name: String,
                pub avatar_url: Option<String>,
            }

            #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
            pub enum Relation {}
        "##;
        let fields = extract_model_struct_fields(source);
        assert_eq!(
            fields,
            BTreeSet::from([
                "id".to_owned(),
                "email".to_owned(),
                "name".to_owned(),
                "avatar_url".to_owned(),
            ])
        );
    }

    #[test]
    fn a_relation_accessor_is_not_a_column_and_r_hash_is_not_part_of_the_name() {
        // The shape `#[sea_orm::model]` produces: relation accessors live
        // in the `Model` struct beside the columns, a Rust keyword column
        // is escaped, and `column_name` names the column outright.
        let source = r##"
            #[sea_orm::model]
            #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
            #[sea_orm(table_name = "alarm")]
            pub struct Model {
                #[sea_orm(primary_key)]
                pub id: i64,
                #[sea_orm(indexed)]
                pub user_id: i64,
                /// The alarm kind.
                #[sea_orm(indexed, default_value = "message", column_name = "type")]
                pub r#type: AlarmType,
                #[sea_orm(belongs_to, relation_enum = "User", from = "user_id", to = "id")]
                pub user: HasOne<super::user::Entity>,
                #[sea_orm(has_many)]
                pub projects: HasMany<super::project::Entity>,
            }
        "##;
        assert_eq!(
            extract_model_struct_fields(source),
            BTreeSet::from(["id".to_owned(), "user_id".to_owned(), "type".to_owned()])
        );
    }

    #[tokio::test]
    async fn db_entity_layer_does_not_call_a_relation_a_missing_column() {
        let temp = ScopedTempDir::new("db-entity-relations");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api_root = temp.path().join("apis").join("api");
        let models_dir = api_root.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();
        std::fs::write(
            models_dir.join("alarm.json"),
            r##"{ "name": "alarm", "columns": [
                { "name": "id", "type": "bigint" },
                { "name": "user_id", "type": "bigint" },
                { "name": "type", "type": "text" }
            ] }"##,
        )
        .unwrap();
        let entity_dir = api_root.join("src").join("models");
        std::fs::create_dir_all(&entity_dir).unwrap();
        std::fs::write(
            entity_dir.join("alarm.rs"),
            r##"
            pub struct Model {
                #[sea_orm(primary_key)]
                pub id: i64,
                pub user_id: i64,
                #[sea_orm(column_name = "type")]
                pub r#type: AlarmType,
                #[sea_orm(belongs_to, from = "user_id", to = "id")]
                pub user: HasOne<super::user::Entity>,
            }
            "##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["db-entity".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["db-entity"];
        assert_eq!(layer["tablesChecked"], 1);
        assert_eq!(
            layer["drifts"].as_array().unwrap().len(),
            0,
            "the entity and the model agree; `user` is a relation and \
             `r#type` is the `type` column: {layer}"
        );
    }

    #[tokio::test]
    async fn entity_route_layer_ignores_a_models_directory_with_no_vespertide_models() {
        let temp = ScopedTempDir::new("entity-route-empty-models");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api_root = temp.path().join("apis").join("api");
        // The real Vespertide models, with a routes tree beside them.
        std::fs::create_dir_all(api_root.join("models")).unwrap();
        std::fs::write(
            api_root.join("models").join("user.json"),
            r##"{ "name": "user", "columns": [ { "name": "email", "type": "text" } ] }"##,
        )
        .unwrap();
        let routes_dir = api_root.join("src").join("routes");
        std::fs::create_dir_all(&routes_dir).unwrap();
        std::fs::write(
            routes_dir.join("user.rs"),
            r##"
            #[vespera::route(get, tags = ["user"])]
            pub async fn list_users() -> Json<()> { let _ = "email"; todo!() }
            "##,
        )
        .unwrap();
        // Generated sea-orm entities, and another app's models: both are
        // called `models` and neither holds a Vespertide model.
        std::fs::create_dir_all(api_root.join("src").join("models")).unwrap();
        std::fs::write(
            api_root.join("src").join("models").join("user.rs"),
            "pub struct Model { pub email: String, }",
        )
        .unwrap();
        let other_app = temp.path().join("apis").join("ai").join("app");
        std::fs::create_dir_all(other_app.join("models")).unwrap();
        std::fs::write(other_app.join("models").join("schema.py"), "").unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["entity-route".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["entity-route"];
        assert_eq!(layer["checked"], true);
        assert!(
            !layer["drifts"]
                .as_array()
                .unwrap()
                .iter()
                .any(|drift| drift["kind"] == "no-routes-dir"),
            "a directory with no Vespertide model has no correspondence to \
             report on: {layer}"
        );
    }

    #[tokio::test]
    async fn openapi_client_layer_matches_a_camel_case_parameter_to_the_kebab_case_spec() {
        let temp = ScopedTempDir::new("openapi-client-camel");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("openapi.json"),
            r##"{ "paths": {
                "/order/{order-number}": { "get": { "operationId": "getOrder" } },
                "/delivery/projects/{project-id}": { "get": { "operationId": "getDelivery" } }
            } }"##,
        )
        .unwrap();
        let front = temp.path().join("apps").join("front").join("src");
        std::fs::create_dir_all(&front).unwrap();
        std::fs::write(
            front.join("page.tsx"),
            r##"
            const order = await api.get('/order/{orderNumber}')
            const delivery = await api.get('/delivery/projects/{projectId}')
            const nope = await api.get('/order/{orderNumber}/refund')
            "##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["openapi-client".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["openapi-client"];
        assert_eq!(layer["callsChecked"], 3);
        assert_eq!(layer["spellingNormalizedMatches"], 2, "{layer}");
        let drifts = layer["drifts"].as_array().unwrap();
        assert_eq!(drifts.len(), 1, "{layer}");
        assert_eq!(drifts[0]["identifier"], "/order/{orderNumber}/refund");
    }

    #[test]
    fn route_url_prefix_matches_vespera_file_structure_convention() {
        assert_eq!(route_url_prefix(Path::new("mod.rs")), "");
        assert_eq!(route_url_prefix(Path::new("users.rs")), "/users");
        assert_eq!(route_url_prefix(Path::new("admin/mod.rs")), "/admin");
        assert_eq!(
            route_url_prefix(Path::new("admin/stats.rs")),
            "/admin/stats"
        );
    }

    #[test]
    fn extracts_vespera_route_attributes_and_matches_path() {
        let source = r##"
            #[vespera::route(get, path = "/{id}", tags = ["users"])]
            pub async fn get_user(Path(id): Path<u32>) -> Json<User> { todo!() }

            #[vespera::route(post, tags = ["users"])]
            pub async fn create_user() -> Json<User> { todo!() }
        "##;
        let routes = extract_vespera_route_attributes(source);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].0, "get");
        assert_eq!(routes[0].1.as_deref(), Some("/{id}"));
        assert_eq!(routes[1].0, "post");
        assert_eq!(routes[1].1, None);
    }

    #[test]
    fn extracts_devup_api_client_calls() {
        let source = r##"
            const user = await api.get('getUser', { params: { id: '1' } })
            await api.put('/users/{id}', { params: { id: '1' } })
            queryClient.useQuery('get', '/users/{id}', { params: { id: userId } })
        "##;
        let calls = extract_devup_api_calls(source);
        let identifiers = calls.iter().map(|(_, id)| id.as_str()).collect::<Vec<_>>();
        assert!(identifiers.contains(&"getUser"));
        assert!(identifiers.contains(&"/users/{id}"));
    }

    #[tokio::test]
    async fn db_entity_layer_flags_missing_entity_field() {
        let temp = ScopedTempDir::new("db-entity");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api_root = temp.path().join("apis").join("api");
        let models_dir = api_root.join("models");
        std::fs::create_dir_all(&models_dir).unwrap();
        std::fs::write(
            models_dir.join("user.json"),
            r##"{ "name": "user", "columns": [
                { "name": "id", "type": "uuid", "nullable": false },
                { "name": "phone_number", "type": "text", "nullable": true }
            ] }"##,
        )
        .unwrap();
        let entity_dir = api_root.join("src").join("models");
        std::fs::create_dir_all(&entity_dir).unwrap();
        std::fs::write(
            entity_dir.join("user.rs"),
            r##"
            pub struct Model {
                pub id: Uuid,
            }
            "##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["db-entity".to_owned()],
        )
        .await
        .unwrap();
        let drifts = result["layers"]["db-entity"]["drifts"].as_array().unwrap();
        assert!(!drifts.is_empty());
        let drift = &drifts[0];
        assert_eq!(drift["columnsMissingInEntity"][0], "phone_number");
    }