/// A repository of several apps: each app's `devup.json` and its sources.
/// `$bg` is a front token and `$adminBg` an admin one; `shared/` belongs to
/// neither app.
fn monorepo(label: &str) -> anyhow::Result<std::path::PathBuf> {
    let root = std::env::temp_dir().join(format!("devup-token-scopes-{label}-{}", rand::random::<u64>()));
    for (path, content) in [
        ("package.json", "{}"),
        ("apps/front/package.json", "{}"),
        ("apps/front/devup.json", r##"{"theme":{"colors":{"default":{"bg":"#F7F3EC"}}}}"##),
        ("apps/front/src/Hero.tsx", r#"export function Hero() { return <Box bg="$bg" /> }"#),
        ("apps/admin/package.json", "{}"),
        ("apps/admin/devup.json", r##"{"theme":{"colors":{"default":{"adminBg":"#000000"}}}}"##),
        ("apps/admin/src/Panel.tsx", r#"export function Panel() { return <Box bg="$bg" color="$adminBg" /> }"#),
        ("shared/Badge.tsx", r#"export function Badge() { return <Box bg="$bg" /> }"#),
    ] {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap())?;
        std::fs::write(file, content)?;
    }
    Ok(root)
}

fn unknown_tokens(output: &Value) -> Vec<(String, String)> {
    output["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["rule"] == "unknown-token")
        .map(|finding| (finding["path"].as_str().unwrap().to_owned(), finding["snippet"].as_str().unwrap().to_owned()))
        .collect()
}

/// One call over the repository root used to skip every token check (several
/// themes, none at the root). Each file is now checked against its own app's
/// theme: `$bg` passes in front and is unknown in admin.
#[tokio::test]
async fn each_app_is_validated_against_its_own_devup_json() -> anyhow::Result<()> {
    let root = monorepo("apps")?;
    let result = validate(json!({"paths":["apps"],"projectRoot":root})).await;
    std::fs::remove_dir_all(&root)?;
    let out = result?;
    assert_eq!(unknown_tokens(&out), [("apps/admin/src/Panel.tsx".to_owned(), "\"$bg\"".to_owned())], "{out}");
    assert_eq!(out["tokens"]["verification"], "verified", "{out}");
    assert_eq!(out["tokens"]["unknownTokenCheckRan"], true, "{out}");
    assert_eq!(out["themeAvailable"], true, "{out}");
    assert!(out["themeGuardrail"].is_null(), "{out}");
    assert_eq!(
        out["tokens"]["scopes"],
        json!([
            {"status":"verified","devupJson":"apps/admin/devup.json","files":1,"referencedByTsx":2,"definedByTheme":1},
            {"status":"verified","devupJson":"apps/front/devup.json","files":1,"referencedByTsx":1,"definedByTheme":1},
        ]),
        "{out}"
    );
    assert_eq!(out["okReason"], "error-violations", "{out}");
    Ok(())
}

/// A file no `devup.json` governs is reported as unverified. The run is
/// `partial`, never `verified`, and the files are named.
#[tokio::test]
async fn files_without_a_resolvable_devup_json_are_reported_as_unverified() -> anyhow::Result<()> {
    let root = monorepo("partial")?;
    let full = validate(json!({"paths":["apps/front","shared"],"projectRoot":root})).await;
    let summary = validate(json!({"paths":["apps/front","shared"],"projectRoot":root,"detail":"summary"})).await;
    let only_shared = validate(json!({"paths":["shared"],"projectRoot":root})).await;
    std::fs::remove_dir_all(&root)?;
    let full = full?;
    assert!(unknown_tokens(&full).is_empty(), "{full}");
    for out in [full, summary?] {
        assert_eq!(out["tokens"]["verification"], "partial", "{out}");
        assert_eq!(out["tokens"]["unknownTokenCheckRan"], false, "{out}");
        assert_eq!(out["tokens"]["unverifiedReferences"], 1, "{out}");
        assert_eq!(out["tokens"]["unverifiedFiles"], 1, "{out}");
        assert_eq!(out["themeAvailable"], false, "{out}");
        assert_eq!(out["themeGuardrail"]["unverifiedPaths"], json!(["shared/Badge.tsx"]), "{out}");
        // The project-wide answer still names the themes to choose from.
        assert_eq!(out["themeGuardrail"]["candidateProjectRoots"].as_array().map(Vec::len), Some(2), "{out}");
        assert_ne!(out["okReason"], "clean", "{out}");
        let scopes = out["tokens"]["scopes"].as_array().unwrap();
        assert_eq!(scopes[0]["devupJson"], "apps/front/devup.json", "{out}");
        assert_eq!(scopes[1]["status"], "unverified", "{out}");
        assert_eq!(scopes[1]["paths"], json!(["shared/Badge.tsx"]), "{out}");
    }
    let out = only_shared?;
    assert_eq!(out["tokens"]["verification"], "skipped", "{out}");
    assert_eq!(out["themeAvailable"], false, "{out}");
    assert_eq!(out["tokens"]["definedByTheme"], 0, "{out}");
    assert_ne!(out["okReason"], "clean", "{out}");
    Ok(())
}

/// With one theme in the project, a file outside every package still uses it
/// (the single-theme behaviour), but another package's files do not borrow it.
#[tokio::test]
async fn a_single_project_theme_does_not_cross_into_another_package() -> anyhow::Result<()> {
    let root = monorepo("boundary")?;
    std::fs::remove_file(root.join("apps/admin/devup.json"))?;
    let result = validate(json!({"paths":["apps/admin","shared"],"projectRoot":root})).await;
    std::fs::remove_dir_all(&root)?;
    let out = result?;
    // `shared/` has no package of its own: front's theme, the only one, applies.
    // `apps/admin` is its own package without a theme: `$adminBg` is not
    // called unknown against front's theme, it is unverified.
    assert!(unknown_tokens(&out).is_empty(), "{out}");
    assert_eq!(out["tokens"]["verification"], "partial", "{out}");
    assert_eq!(out["themeGuardrail"]["unverifiedPaths"], json!(["apps/admin/src/Panel.tsx"]), "{out}");
    assert_eq!(out["tokens"]["unverifiedReferences"], 2, "{out}");
    let scopes = out["tokens"]["scopes"].as_array().unwrap();
    assert_eq!(scopes[0]["status"], "verified", "{out}");
    assert_eq!(scopes[0]["files"], 1, "{out}");
    assert!(scopes[0]["devupJson"].as_str().unwrap().replace('\\', "/").ends_with("apps/front/devup.json"), "{out}");
    Ok(())
}

/// A supplied bundle with two themes used to be refused. It is matched per
/// directory like the disk case, without reading the filesystem.
#[tokio::test]
async fn a_bundle_with_several_themes_is_matched_per_directory() -> anyhow::Result<()> {
    let out = validate(json!({"files":[
        {"path":"apps/front/devup.json","content":r##"{"theme":{"colors":{"default":{"bg":"#F7F3EC"}}}}"##},
        {"path":"apps/admin/devup.json","content":r##"{"theme":{"colors":{"default":{"adminBg":"#000000"}}}}"##},
        {"path":"apps/front/src/Hero.tsx","content":r#"export function Hero() { return <Box bg="$bg" /> }"#},
        {"path":"apps/admin/src/Panel.tsx","content":r#"export function Panel() { return <Box bg="$bg" /> }"#},
        {"path":"shared/Badge.tsx","content":r#"export function Badge() { return <Box bg="$bg" /> }"#},
    ]}))
    .await?;
    assert_eq!(unknown_tokens(&out), [("apps/admin/src/Panel.tsx".to_owned(), "\"$bg\"".to_owned())], "{out}");
    assert_eq!(out["tokens"]["verification"], "partial", "{out}");
    assert_eq!(out["themeGuardrail"]["unverifiedPaths"], json!(["shared/Badge.tsx"]), "{out}");
    assert!(out["themeGuardrail"]["message"].as_str().unwrap().contains("never reads the filesystem"), "{out}");
    Ok(())
}
