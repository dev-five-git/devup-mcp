

    #[tokio::test]
    async fn route_openapi_layer_flags_route_missing_from_spec() {
        let temp = ScopedTempDir::new("route-openapi");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api_root = temp.path().join("apis").join("api");
        let routes_dir = api_root.join("src").join("routes");
        std::fs::create_dir_all(&routes_dir).unwrap();
        std::fs::write(
            routes_dir.join("users.rs"),
            r##"
            #[vespera::route(get, path = "/{id}", tags = ["users"])]
            pub async fn get_user() -> Json<()> { todo!() }
            "##,
        )
        .unwrap();
        std::fs::write(api_root.join("openapi.json"), r##"{ "paths": {} }"##).unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["checked"], true);
        let drifts = layer["drifts"].as_array().unwrap();
        assert!(
            drifts
                .iter()
                .any(|drift| drift["kind"] == "route-missing-from-openapi")
        );
    }

    /// A project whose handlers are snake_case and whose `openapi.json`
    /// Vespera therefore wrote in kebab-case — including inside `{...}`
    /// parameters, and with a trailing slash on the module-root route.
    /// This is the shape of the real project in `devup-mcp-defects.md`
    /// D17, where 222 code routes and 222 spec routes with no actual drift
    /// produced ~158 mirror-image false positives.
    fn write_kebab_case_spec_project(root: &Path) {
        std::fs::write(root.join("package.json"), "{}").unwrap();
        let api_root = root.join("apis").join("api");
        let routes_dir = api_root.join("src").join("routes");
        std::fs::create_dir_all(&routes_dir).unwrap();
        std::fs::write(
            routes_dir.join("ai_character.rs"),
            r##"
            #[vespera::route(get, tags = ["ai_character"])]
            pub async fn list_ai_characters() -> Json<()> { todo!() }

            #[vespera::route(get, path = "/{id}", tags = ["ai_character"])]
            pub async fn get_ai_character() -> Json<()> { todo!() }
            "##,
        )
        .unwrap();
        std::fs::write(
            routes_dir.join("order.rs"),
            r##"
            #[vespera::route(get, path = "/{order_number}", tags = ["order"])]
            pub async fn get_order() -> Json<()> { todo!() }
            "##,
        )
        .unwrap();
        std::fs::write(
            api_root.join("openapi.json"),
            r##"{ "paths": {
                "/ai-character/": { "get": { "operationId": "listAiCharacters" } },
                "/ai-character/{id}": { "get": { "operationId": "getAiCharacter" } },
                "/order/{order-number}": { "get": { "operationId": "getOrder" } }
            } }"##,
        )
        .unwrap();
    }

    #[tokio::test]
    async fn route_openapi_layer_reports_no_drift_when_only_the_spelling_differs() {
        let temp = ScopedTempDir::new("route-openapi-kebab");
        write_kebab_case_spec_project(temp.path());

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        assert_eq!(layer["checked"], true);
        assert_eq!(layer["codeRouteCount"], 3);
        assert_eq!(layer["openapiRouteCount"], 3);
        assert_eq!(
            layer["drifts"].as_array().unwrap().len(),
            0,
            "snake_case handlers against Vespera's kebab_case spec are the \
             same routes, not drift: {layer}"
        );
    }

    #[tokio::test]
    async fn route_openapi_layer_says_how_many_routes_only_matched_on_spelling() {
        let temp = ScopedTempDir::new("route-openapi-kebab-note");
        write_kebab_case_spec_project(temp.path());

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        // All three differ in spelling: `/ai_character` vs `/ai-character/`,
        // `/ai_character/{id}` vs `/ai-character/{id}`, and
        // `/order/{order_number}` vs `/order/{order-number}`.
        assert_eq!(layer["spellingNormalizedMatches"], 3, "{layer}");
        assert!(
            layer["notes"][0].as_str().unwrap().contains("separator")
                || layer["notes"][0].as_str().unwrap().contains("spelling"),
            "the fold must be disclosed, not silent: {layer}"
        );
    }

    #[tokio::test]
    async fn route_openapi_layer_still_reports_a_route_the_spec_really_lacks() {
        let temp = ScopedTempDir::new("route-openapi-real-drift");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api_root = temp.path().join("apis").join("api");
        let routes_dir = api_root.join("src").join("routes");
        std::fs::create_dir_all(&routes_dir).unwrap();
        std::fs::write(
            routes_dir.join("story_comment.rs"),
            r##"
            #[vespera::route(get, path = "/{comment_id}", tags = ["story_comment"])]
            pub async fn get_comment() -> Json<()> { todo!() }

            #[vespera::route(delete, path = "/{comment_id}", tags = ["story_comment"])]
            pub async fn delete_comment() -> Json<()> { todo!() }
            "##,
        )
        .unwrap();
        // The spec has the GET in Vespera's kebab spelling but no DELETE.
        std::fs::write(
            api_root.join("openapi.json"),
            r##"{ "paths": {
                "/story-comment/{comment-id}": { "get": { "operationId": "getComment" } }
            } }"##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        let drifts = layer["drifts"].as_array().unwrap();
        let missing = drifts
            .iter()
            .find(|drift| drift["kind"] == "route-missing-from-openapi")
            .expect("the DELETE really is absent from the spec");
        let routes = missing["routes"].as_array().unwrap();
        assert_eq!(routes.len(), 1, "{layer}");
        assert_eq!(routes[0]["method"], "DELETE");
        assert_eq!(
            routes[0]["path"], "/story_comment/{comment_id}",
            "a drift must be reported in the spelling its own layer uses"
        );
        assert!(
            !drifts
                .iter()
                .any(|drift| drift["kind"] == "openapi-path-not-found-in-scanned-routes"),
            "the GET matched, so nothing may be reported from the spec side: {layer}"
        );
    }

    #[tokio::test]
    async fn stack_diff_skips_openapi_specs_inside_a_nested_worktree() {
        let temp = ScopedTempDir::new("stackdiff-worktree");
        write_kebab_case_spec_project(temp.path());
        let checkout = temp.path().join(".worktrees").join("revert-some-branch");
        std::fs::create_dir_all(checkout.join("apis").join("api")).unwrap();
        std::fs::write(checkout.join(".git"), "gitdir: ../../.git/worktrees/x").unwrap();
        std::fs::write(
            checkout.join("apis").join("api").join("openapi.json"),
            r##"{ "paths": { "/deleted-last-month": { "get": {} } } }"##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["route-openapi".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["route-openapi"];
        let specs = layer["openapiSpecsFound"].as_array().unwrap();
        assert_eq!(specs.len(), 1, "{layer}");
        assert!(
            !specs[0].as_str().unwrap().contains(".worktrees"),
            "{layer}"
        );
        assert_eq!(
            layer["drifts"].as_array().unwrap().len(),
            0,
            "a stale branch's endpoint is not this project's drift: {layer}"
        );
        assert_eq!(layer["excludedPaths"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn all_layers_report_nested_only_inputs_without_using_them() {
        let temp = ScopedTempDir::new("stackdiff-nested-only");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let checkout = temp.path().join(".worktrees/stale");
        std::fs::create_dir_all(checkout.join("models")).unwrap();
        std::fs::create_dir_all(checkout.join("routes")).unwrap();
        std::fs::write(checkout.join("openapi.json"), "not valid JSON").unwrap();
        let result = run(Some(&temp.path().to_string_lossy()), &[])
            .await
            .unwrap();
        for (layer, paths) in [
            ("db-entity", vec![".worktrees/stale/models"]),
            ("entity-route", vec![".worktrees/stale/models"]),
            (
                "route-openapi",
                vec![".worktrees/stale/routes", ".worktrees/stale/openapi.json"],
            ),
            ("openapi-client", vec![".worktrees/stale/openapi.json"]),
        ] {
            let layer = &result["layers"][layer];
            assert_eq!(layer["checked"], false, "{layer}");
            assert_eq!(layer["drifts"], json!([]), "{layer}");
            let expected = paths
                .into_iter()
                .map(|path| {
                    json!({
                        "path": path, "reason": "nested-checkout-directory"
                    })
                })
                .collect::<Vec<_>>();
            assert_eq!(layer["excludedPaths"], json!(expected), "{layer}");
        }
    }

    #[tokio::test]
    async fn openapi_client_layer_flags_unknown_operation_id() {
        let temp = ScopedTempDir::new("openapi-client");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("openapi.json"),
            r##"{ "paths": { "/users": { "get": { "operationId": "getUsers" } } } }"##,
        )
        .unwrap();
        let front = temp.path().join("apps").join("front").join("src");
        std::fs::create_dir_all(&front).unwrap();
        std::fs::write(
            front.join("page.tsx"),
            r##"const users = await api.get('getUsersThatDoesNotExist')"##,
        )
        .unwrap();

        let result = run(
            Some(&temp.path().to_string_lossy()),
            &["openapi-client".to_owned()],
        )
        .await
        .unwrap();
        let layer = &result["layers"]["openapi-client"];
        assert_eq!(layer["checked"], true);
        let drifts = layer["drifts"].as_array().unwrap();
        assert!(
            drifts
                .iter()
                .any(|drift| drift["identifier"] == "getUsersThatDoesNotExist")
        );
    }

    #[tokio::test]
    async fn missing_project_root_reports_guardrail() {
        let temp = ScopedTempDir::new("stackdiff-no-root");
        let nested = temp.path().join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        let result = run(Some(&nested.to_string_lossy()), &[]).await.unwrap();
        assert_eq!(result["found"], false);
        assert_eq!(result["guardrail"]["action"], "stop-and-report");
    }

    #[tokio::test]
    async fn invalid_layer_name_is_rejected() {
        let temp = ScopedTempDir::new("stackdiff-bad-layer");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let error = run(
            Some(&temp.path().to_string_lossy()),
            &["bogus-layer".to_owned()],
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    }
