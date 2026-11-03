use super::*;

struct Fixture {
    root: PathBuf,
    policy: Option<OutputPolicy>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("devup-asset-tool-{}", rand::random::<u64>()));
        std::fs::create_dir_all(root.join("public/images")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
        Self {
            root,
            policy: Some(policy),
        }
    }
    fn write(&self, path: &str, bytes: &[u8]) {
        let target = self.root.join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, bytes).unwrap();
    }
    fn input(&self, value: Value) -> AssetsInput {
        let mut base =
            json!({"projectRoot":self.root,"publicRoots":["public"],"referencePaths":["src"]});
        base.as_object_mut()
            .unwrap()
            .extend(value.as_object().unwrap().clone());
        serde_json::from_value(base).unwrap()
    }
    fn call(&self, value: Value) -> Result<Value, DevupError> {
        run(self.input(value), self.policy.as_ref().unwrap())
    }
    fn create_plan(&self, extra: Value) -> Value {
        let mut input = json!({"action":"plan","dryRun":false,"renames":[{"from":"/images/opaque.png","to":"/images/laboratory.png"}]});
        input
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        self.call(input).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.policy.take();
        assert!(self.root.starts_with(std::env::temp_dir()));
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn inventory_collects_all_formats_exact_references_duplicates_and_dynamic_unknowns() {
    let f = Fixture::new();
    f.write("public/images/opaque.png", b"one");
    f.write("public/images/copy.webp", b"one");
    f.write("public/data.bin", b"two");
    f.write(
        "src/page.tsx",
        br#"export const Page=()=> <><img src="/images/opaque.png"/><img src={record.image}/></>;"#,
    );
    let result = f.call(json!({"action":"inventory"})).unwrap();
    assert_eq!(result["inventory"]["assets"].as_array().unwrap().len(), 3);
    assert_eq!(
        result["inventory"]["references"][0]["url"],
        "/images/opaque.png"
    );
    assert_eq!(
        result["inventory"]["dynamicReferences"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(result["duplicateGroups"].as_array().unwrap().len(), 1);
    assert_eq!(result["dynamicUsageProven"], false);
    assert_eq!(result["liveDatabaseVerified"], false);
}

#[test]
fn plan_dry_run_apply_and_guarded_rollback_keep_files_and_registry_consistent() {
    let f = Fixture::new();
    f.write("public/images/opaque.png", b"photograph");
    let original=br#"export const Page=()=> <img src="/images/opaque.png"/>; // /images/opaque.png stays a comment"#;
    f.write("src/page.tsx", original);
    let dry=f.call(json!({"action":"plan","renames":[{"from":"/images/opaque.png","to":"/images/laboratory.png"}]})).unwrap();
    assert_eq!(dry["planPersisted"], false);
    assert!(!f.root.join(".devup").exists());
    let plan = f.create_plan(json!({}));
    let path = plan["planPath"].as_str().unwrap();
    let dry = f.call(json!({"action":"apply","planPath":path})).unwrap();
    assert_eq!(dry["localChangesReady"], true);
    assert!(!f.root.join("public/images/laboratory.png").exists());
    let applied = f
        .call(json!({"action":"apply","planPath":path,"dryRun":false}))
        .unwrap();
    assert_eq!(applied["state"], "applied");
    assert_eq!(
        std::fs::read(f.root.join("public/images/laboratory.png")).unwrap(),
        b"photograph"
    );
    assert!(f.root.join("public/images/opaque.png").exists());
    let code = std::fs::read_to_string(f.root.join("src/page.tsx")).unwrap();
    assert!(code.contains("src=\"/images/laboratory.png\""));
    assert!(code.contains("// /images/opaque.png stays a comment"));
    let registry: Value =
        serde_json::from_slice(&std::fs::read(f.root.join("docs/public-assets.json")).unwrap())
            .unwrap();
    assert!(
        registry["assets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["path"] == "/images/laboratory.png")
    );
    assert_eq!(
        f.call(json!({"action":"apply","planPath":path,"dryRun":false}))
            .unwrap()["state"],
        "already-applied"
    );
    assert_eq!(
        f.call(json!({"action":"rollback","planPath":path,"dryRun":false}))
            .unwrap()["state"],
        "rolled-back"
    );
    assert_eq!(
        std::fs::read(f.root.join("src/page.tsx")).unwrap(),
        original
    );
    assert!(!f.root.join("public/images/laboratory.png").exists());
    assert!(!f.root.join("docs/public-assets.json").exists());
    assert_eq!(
        f.call(json!({"action":"rollback","planPath":path,"dryRun":false}))
            .unwrap()["state"],
        "already-rolled-back"
    );
}

#[test]
fn source_retirement_is_explicit_static_and_reversible() {
    let f = Fixture::new();
    f.write("public/images/opaque.png", b"photo");
    f.write("src/seed.json", br#"{"hero":"/images/opaque.png"}"#);
    let plan = f.create_plan(json!({"retireSources":true}));
    let path = plan["planPath"].as_str().unwrap();
    f.call(json!({"action":"apply","planPath":path,"dryRun":false}))
        .unwrap();
    assert!(!f.root.join("public/images/opaque.png").exists());
    f.call(json!({"action":"rollback","planPath":path,"dryRun":false}))
        .unwrap();
    assert_eq!(
        std::fs::read(f.root.join("public/images/opaque.png")).unwrap(),
        b"photo"
    );
    assert_eq!(
        std::fs::read(f.root.join("src/seed.json")).unwrap(),
        br#"{"hero":"/images/opaque.png"}"#
    );
}

#[test]
fn stale_plan_and_edited_rollback_target_never_overwrite_user_changes() {
    let f = Fixture::new();
    f.write("public/images/opaque.png", b"photo");
    f.write("src/page.tsx", br#"const x="/images/opaque.png";"#);
    let plan = f.create_plan(json!({}));
    let path = plan["planPath"].as_str().unwrap();
    f.write("src/page.tsx", b"const changed=true;");
    assert!(
        f.call(json!({"action":"apply","planPath":path,"dryRun":false}))
            .is_err()
    );
    assert!(!f.root.join("public/images/laboratory.png").exists());
    f.write("src/page.tsx", br#"const x="/images/opaque.png";"#);
    f.call(json!({"action":"apply","planPath":path,"dryRun":false}))
        .unwrap();
    f.write("src/page.tsx", b"user edit after migration");
    assert!(
        f.call(json!({"action":"rollback","planPath":path,"dryRun":false}))
            .is_err()
    );
    assert_eq!(
        std::fs::read(f.root.join("src/page.tsx")).unwrap(),
        b"user edit after migration"
    );
    assert!(f.root.join("public/images/laboratory.png").exists());
}

#[test]
fn different_target_bytes_and_dynamic_retirement_are_refused() {
    let f = Fixture::new();
    f.write("public/images/opaque.png", b"photo");
    f.write("public/images/laboratory.png", b"other");
    assert!(f.call(json!({"action":"plan","renames":[{"from":"/images/opaque.png","to":"/images/laboratory.png"}]})).is_err());
    std::fs::remove_file(f.root.join("public/images/laboratory.png")).unwrap();
    f.write(
        "src/page.tsx",
        br#"const Page=()=> <img src={record.url}/>;"#,
    );
    assert!(f.call(json!({"action":"plan","retireSources":true,"renames":[{"from":"/images/opaque.png","to":"/images/laboratory.png"}]})).is_err());
}

#[test]
fn database_adapter_generates_guarded_sql_without_pretending_to_apply_live_db() {
    let f = Fixture::new();
    f.write("public/images/opaque.png", b"photo");
    f.write(
        "snapshots/products.json",
        br#"[{"key":"row'1","url":"/images/opaque.png"}]"#,
    );
    let descriptor = json!([{"snapshotPath":"snapshots/products.json","table":"products","keyColumn":"id","urlColumn":"image_url"}]);
    let plan = f.create_plan(json!({"databaseReferences":descriptor}));
    let path = plan["planPath"].as_str().unwrap();
    assert_eq!(plan["plan"]["databaseChanges"].as_array().unwrap().len(), 1);
    let applied = f
        .call(json!({"action":"apply","planPath":path,"dryRun":false}))
        .unwrap();
    assert_eq!(applied["liveDatabaseApplied"], false);
    assert_eq!(applied["databaseChangesPending"], true);
    let sql = std::fs::read_to_string(
        f.root
            .join(applied["databaseSql"]["migration"].as_str().unwrap()),
    )
    .unwrap();
    assert!(sql.contains("row''1"));
    assert!(sql.contains("GET DIAGNOSTICS changed = ROW_COUNT"));
    assert!(sql.contains("BEGIN;"));
    assert!(sql.contains("COMMIT;"));
    assert_eq!(
        std::fs::read(f.root.join("snapshots/products.json")).unwrap(),
        br#"[{"key":"row'1","url":"/images/opaque.png"}]"#
    );
    let rollback = f
        .call(json!({"action":"rollback","planPath":path,"dryRun":false}))
        .unwrap();
    assert_eq!(rollback["retainedCanonicalAssetsAndRegistry"], true);
    assert!(f.root.join("public/images/laboratory.png").exists());
    assert!(f.root.join("docs/public-assets.json").exists());
    let illegal = json!([{"snapshotPath":"snapshots/products.json","table":"products; DROP TABLE x","keyColumn":"id","urlColumn":"image_url"}]);
    assert!(
        f.call(json!({"action":"inventory","databaseReferences":illegal}))
            .is_err()
    );
}

#[test]
fn css_mask_seed_and_query_references_migrate_without_changing_their_meaning() {
    let f = Fixture::new();
    f.write("public/images/opaque.png", b"photo");
    f.write("src/page.tsx",br#"export const Page=()=> <Box maskImage="url(/images/opaque.png)"><img src="/images/opaque.png?v=2#view"/></Box>;"#);
    f.write(
        "src/style.css",
        b"a { background-image: url(/images/opaque.png); }",
    );
    f.write(
        "src/seed.sql",
        b"INSERT INTO images(url) VALUES('/images/opaque.png');",
    );
    let plan = f.create_plan(json!({}));
    let path = plan["planPath"].as_str().unwrap();
    f.call(json!({"action":"apply","planPath":path,"dryRun":false}))
        .unwrap();
    let code = std::fs::read_to_string(f.root.join("src/page.tsx")).unwrap();
    assert!(code.contains("url(/images/laboratory.png)"));
    assert!(code.contains("/images/laboratory.png?v=2#view"));
    assert!(references::scan("src/page.tsx", &code).2.is_empty());
    assert!(
        std::fs::read_to_string(f.root.join("src/style.css"))
            .unwrap()
            .contains("url(/images/laboratory.png)")
    );
    assert!(
        std::fs::read_to_string(f.root.join("src/seed.sql"))
            .unwrap()
            .contains("'/images/laboratory.png'")
    );
}

#[test]
fn inventory_pages_have_totals_and_continuation_without_silent_row_loss() {
    let f = Fixture::new();
    for name in ["laboratory", "office", "research"] {
        f.write(&format!("public/images/{name}.png"), name.as_bytes());
    }
    let first = f
        .call(json!({"action":"inventory","responseLimit":1}))
        .unwrap();
    assert_eq!(first["inventory"]["totals"]["assets"], 3);
    assert_eq!(first["inventory"]["assets"].as_array().unwrap().len(), 1);
    assert_eq!(first["inventory"]["paging"]["hasMore"], true);
    assert_eq!(first["nextPage"]["arguments"]["responseOffset"], 1);
    let second = f
        .call(json!({"action":"inventory","responseLimit":1,"responseOffset":1}))
        .unwrap();
    assert_ne!(
        first["inventory"]["assets"][0]["path"],
        second["inventory"]["assets"][0]["path"]
    );
}

#[test]
fn paths_cannot_escape_workspace_or_use_anonymous_destinations() {
    let f = Fixture::new();
    f.write("public/images/opaque.png", b"photo");
    for to in [
        "/../private.png",
        "/images/%2e%2e/private.png",
        "/images/image-123.png",
    ] {
        assert!(
            f.call(json!({"action":"plan","renames":[{"from":"/images/opaque.png","to":to}]}))
                .is_err()
        );
    }
    assert!(
        f.call(json!({"action":"inventory","referencePaths":["../outside"]}))
            .is_err()
    );
}
