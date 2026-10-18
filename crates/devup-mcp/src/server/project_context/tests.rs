

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct ScopedTempDir(PathBuf);

    impl ScopedTempDir {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "devup-mcp-context-test-{label}-{}-{unique}",
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

    #[tokio::test]
    async fn theme_scope_reads_real_devup_json_tokens() {
        let temp = ScopedTempDir::new("theme-ok");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("devup.json"),
            r##"{ "theme": { "colors": { "default": { "captionLight": "#999999" } } } }"##,
        )
        .unwrap();
        let result = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], true);
        assert_eq!(
            result["files"][0]["colors"]["default"]["captionLight"],
            "#999999"
        );
        assert_eq!(
            result["modeResolution"]["order"],
            "active mode -> selected default mode"
        );
        assert_eq!(
            result["modeResolution"]["defaultModeSelection"],
            json!(["default", "light", "first mode in lexical order"])
        );
        assert_eq!(
            result["responsiveArrayOrder"],
            json!(["mobile", "sm", "tablet", "lg", "PC"])
        );
        assert_eq!(result["propConventions"]["colorAndLength"], "$token");
        assert_eq!(
            result["propConventions"]["typography"],
            "bare token name, no $ prefix"
        );
    }

    #[tokio::test]
    async fn theme_scope_reports_not_found_guardrail_without_devup_json() {
        let temp = ScopedTempDir::new("theme-missing");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let result = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], false);
        assert_eq!(result["guardrail"]["action"], "stop-and-report");
        assert!(result.get("modeResolution").is_some());
    }

    #[tokio::test]
    async fn missing_project_root_reports_guardrail() {
        let temp = ScopedTempDir::new("no-root");
        // No package.json/devup.json/Cargo.toml/.git anywhere under temp.
        let nested = temp.path().join("deep").join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let result = run("theme", Some(&nested.to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], false);
        assert_eq!(result["guardrail"]["action"], "stop-and-report");
    }

    #[tokio::test]
    async fn api_scope_extracts_endpoints_and_required_fields() {
        let temp = ScopedTempDir::new("api-ok");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(
            temp.path().join("openapi.json"),
            r##"{
                "paths": {
                    "/users/{id}": {
                        "get": { "operationId": "getUser" }
                    }
                },
                "components": {
                    "schemas": {
                        "User": { "required": ["id", "email"], "properties": { "id": {}, "email": {}, "name": {} } }
                    }
                }
            }"##,
        )
        .unwrap();
        let result = run("api", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], true);
        assert_eq!(result["specs"][0]["endpoints"][0]["operationId"], "getUser");
        assert_eq!(result["specs"][0]["endpoints"][0]["method"], "GET");
        assert_eq!(result["specs"][0]["schemas"][0]["requiredFields"][0], "id");
    }

    #[tokio::test]
    async fn db_scope_extracts_columns_and_enum_values() {
        let temp = ScopedTempDir::new("db-ok");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let models = temp.path().join("apis").join("api").join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(
            models.join("user.json"),
            r##"{
                "name": "user",
                "columns": [
                    { "name": "id", "type": "uuid", "nullable": false, "primary_key": true },
                    { "name": "status", "type": { "kind": "enum", "name": "user_status", "values": ["pending", "active"] }, "nullable": false }
                ]
            }"##,
        )
        .unwrap();
        let result = run("db", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], true);
        let table = &result["tables"][0];
        assert_eq!(table["table"], "user");
        assert_eq!(table["columns"][0]["name"], "id");
        assert_eq!(table["columns"][0]["primaryKey"], true);
        assert_eq!(table["enums"][0]["values"][0], "pending");
    }

    #[tokio::test]
    async fn invalid_scope_is_rejected() {
        let temp = ScopedTempDir::new("bad-scope");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let error = run("bogus", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::DevupInvalidInput);
    }

    /// Writes the `.worktrees/<branch>/` shape `git worktree add` produces:
    /// a second copy of the project, carrying its own `.git` (a *file* for
    /// a worktree, which is why the check uses `exists()` and not
    /// `is_dir()`).
    fn write_nested_worktree(root: &Path, relative_file: &str, contents: &str) {
        let checkout = root.join(".worktrees").join("revert-some-branch");
        std::fs::create_dir_all(&checkout).unwrap();
        std::fs::write(checkout.join(".git"), "gitdir: ../../.git/worktrees/x").unwrap();
        let file = checkout.join(relative_file);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, contents).unwrap();
    }

    #[tokio::test]
    async fn theme_scope_skips_devup_json_inside_a_nested_worktree() {
        let temp = ScopedTempDir::new("theme-worktree");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let real = temp.path().join("apps").join("front");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(
            real.join("devup.json"),
            r##"{ "theme": { "colors": { "default": { "current": "#111111" } } } }"##,
        )
        .unwrap();
        write_nested_worktree(
            temp.path(),
            "apps/front/devup.json",
            r##"{ "theme": { "colors": { "default": { "stale": "#222222" } } } }"##,
        );

        // Both copies are accounted for by the shared scan, but only the
        // current project's copy can be used as an authoritative input.
        let (found, excluded) = find_files_named(temp.path(), "devup.json", 4);
        assert_eq!(found, vec![real.join("devup.json")]);
        assert_eq!(
            excluded,
            vec![json!({
                "path": ".worktrees/revert-some-branch/apps/front/devup.json",
                "reason": "nested-checkout-directory",
            })]
        );

        let result = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        let files = result["files"].as_array().unwrap();
        assert_eq!(files.len(), 1, "{result}");
        assert_eq!(files[0]["path"], "apps/front/devup.json");
        assert_eq!(files[0]["appliesTo"], "apps/front");
        assert_eq!(files[0]["authority"], "package-local");
        assert!(
            files[0]["colors"]["default"].get("stale").is_none(),
            "a stale branch's token must never be reported: {result}"
        );
        let excluded = result["excludedPaths"].as_array().unwrap();
        assert_eq!(excluded.len(), 1);
        assert!(
            excluded[0]["path"]
                .as_str()
                .unwrap()
                .contains(".worktrees/"),
            "{result}"
        );
    }

    #[tokio::test]
    async fn api_and_db_scopes_skip_nested_worktrees_too() {
        let temp = ScopedTempDir::new("api-db-worktree");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let api = temp.path().join("apis").join("api");
        std::fs::create_dir_all(api.join("models")).unwrap();
        std::fs::write(
            api.join("openapi.json"),
            r##"{ "paths": { "/current": { "get": { "operationId": "current" } } } }"##,
        )
        .unwrap();
        std::fs::write(
            api.join("models").join("user.json"),
            r##"{ "name": "user", "columns": [ { "name": "id", "type": "uuid" } ] }"##,
        )
        .unwrap();
        write_nested_worktree(
            temp.path(),
            "apis/api/openapi.json",
            r##"{ "paths": { "/stale": { "get": { "operationId": "stale" } } } }"##,
        );
        write_nested_worktree(
            temp.path(),
            "apis/api/models/ghost.json",
            r##"{ "name": "ghost", "columns": [ { "name": "id", "type": "uuid" } ] }"##,
        );

        let api_result = run("api", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        let specs = api_result["specs"].as_array().unwrap();
        assert_eq!(specs.len(), 1, "{api_result}");
        assert_eq!(specs[0]["path"], "apis/api/openapi.json");
        assert_eq!(specs[0]["endpoints"][0]["operationId"], "current");
        assert_eq!(api_result["excludedPaths"].as_array().unwrap().len(), 1);

        let db_result = run("db", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        let tables = db_result["tables"].as_array().unwrap();
        assert_eq!(tables.len(), 1, "{db_result}");
        assert_eq!(tables[0]["table"], "user");
        assert_eq!(db_result["excludedPaths"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn theme_for_validation_never_picks_a_stale_branch_over_the_real_one() {
        // `find_files_named` sorts, and `.worktrees` sorts ahead of `apps`,
        // so "the first devup.json found" used to be the stale branch's.
        let temp = ScopedTempDir::new("validation-worktree");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let real = temp.path().join("apps").join("front");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(
            real.join("devup.json"),
            r##"{ "theme": { "colors": { "default": { "current": "#111111" } } } }"##,
        )
        .unwrap();
        write_nested_worktree(
            temp.path(),
            "apps/front/devup.json",
            r##"{ "theme": { "colors": { "default": { "stale": "#222222" } } } }"##,
        );

        let lookup = theme_for_validation(Some(&temp.path().to_string_lossy())).unwrap();
        let theme = lookup.theme.expect("the project's own devup.json");
        assert!(theme.contains_token("current"));
        assert!(
            !theme.contains_token("stale"),
            "validated against a nested checkout's theme"
        );
    }

    #[tokio::test]
    async fn a_theme_that_exists_only_in_a_nested_checkout_says_so() {
        let temp = ScopedTempDir::new("only-worktree-theme");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        write_nested_worktree(
            temp.path(),
            "apps/front/devup.json",
            r##"{ "theme": { "colors": { "default": { "stale": "#222222" } } } }"##,
        );

        let result = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], false);
        assert!(
            result["guardrail"]["message"]
                .as_str()
                .unwrap()
                .contains("nested checkout"),
            "{result}"
        );
        assert_eq!(result["excludedPaths"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn nested_only_theme_reports_each_checkout_kind_without_parsing_it() {
        for (directory, git_marker, reason) in [
            (".worktrees/stale", None, "nested-checkout-directory"),
            (".worktree/stale", None, "nested-checkout-directory"),
            (".git-worktrees/stale", None, "nested-checkout-directory"),
            ("copies/repo", Some(true), "nested-git-checkout"),
            ("copies/linked", Some(false), "nested-git-checkout"),
        ] {
            let temp = ScopedTempDir::new("nested-only-kinds");
            std::fs::write(temp.path().join("package.json"), "{}").unwrap();
            let checkout = temp.path().join(directory);
            std::fs::create_dir_all(&checkout).unwrap();
            match git_marker {
                Some(true) => std::fs::create_dir(checkout.join(".git")).unwrap(),
                Some(false) => {
                    std::fs::write(checkout.join(".git"), "gitdir: elsewhere").unwrap();
                }
                None => {}
            }
            // Excluded files must be reported without reading/parsing their contents.
            std::fs::write(checkout.join("devup.json"), "not valid JSON").unwrap();
            let root = temp.path().to_string_lossy();
            let result = run("theme", Some(&root), None).await.unwrap();
            assert_eq!(result["found"], false, "{result}");
            assert_eq!(
                result["excludedPaths"],
                json!([{ "path": format!("{directory}/devup.json"), "reason": reason }]),
                "{result}"
            );
            let lookup = theme_for_validation(Some(&root)).unwrap();
            assert!(lookup.theme.is_none());
            assert!(
                lookup.guardrail.unwrap()["message"]
                    .as_str()
                    .unwrap()
                    .contains("nested checkout")
            );
        }
    }

    #[tokio::test]
    async fn one_theme_needs_no_authority_note_and_several_do() {
        let temp = ScopedTempDir::new("authority-note");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        let front = temp.path().join("apps").join("front");
        std::fs::create_dir_all(&front).unwrap();
        std::fs::write(front.join("devup.json"), r##"{"theme":{}}"##).unwrap();

        let single = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert!(single.get("authorityNote").is_none(), "{single}");
        assert!(single.get("excludedPaths").is_none(), "{single}");

        let admin = temp.path().join("apps").join("admin");
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(admin.join("devup.json"), r##"{"theme":{}}"##).unwrap();
        let several = run("theme", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert!(
            several["authorityNote"]
                .as_str()
                .unwrap()
                .contains("appliesTo"),
            "{several}"
        );
    }

    #[tokio::test]
    async fn all_scope_combines_every_axis() {
        let temp = ScopedTempDir::new("all-scope");
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::write(temp.path().join("devup.json"), r##"{"theme":{}}"##).unwrap();
        let result = run("all", Some(&temp.path().to_string_lossy()), None)
            .await
            .unwrap();
        assert_eq!(result["found"], true);
        assert!(result.get("theme").is_some());
        assert!(result.get("api").is_some());
        assert!(result.get("db").is_some());
    }
}

#[cfg(test)]
mod r17_tests {
    use super::*;
    #[tokio::test]
    async fn r17_context_distinguishes_total_matches_and_empty_categories() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/ground-truth-project");
        let all = run("theme", root.to_str(), None).await.unwrap();
        let filtered = run("theme", root.to_str(), Some("NO_MATCH")).await.unwrap();
        let f = &filtered["files"][0];
        assert_eq!(f["totalTokenCount"], all["files"][0]["tokenCount"]);
        assert_eq!(f["matchedTokenCount"], 0);
        assert_eq!(f["categories"]["colors"]["state"], "filtered-out");
        let schema =
            serde_json::to_value(schemars::schema_for!(super::super::ProjectContextInput)).unwrap();
        let description = schema["properties"]["filter"]["description"]
            .as_str()
            .unwrap_or("");
        assert!(
            description.contains("case-sensitive")
                && description.contains("substring")
                && description.contains("not a regular expression")
        );
    }
}
