use super::*;

fn asset(id: &str, path: &str, body: &[u8], format: &str) -> devup_mcp_figma::AssetManifestEntry {
    serde_json::from_value(json!({"assetId":id,"nodeId":id,"field":"node","sourceKind":"node","status":"exported","format":format,"path":path,"mimeType":if format=="svg"{"image/svg+xml"}else{"image/png"},"byteLength":body.len(),"sha256":sha256_hex(body),"dataBase64":STANDARD.encode(body)})).unwrap()
}
fn manifest(assets: Vec<devup_mcp_figma::AssetManifestEntry>) -> AssetManifest {
    AssetManifest {
        version: 1,
        assets,
        diagnostics: Vec::new(),
    }
}
fn result(code: &str) -> Map<String, Value> {
    json!({"tsx":code}).as_object().unwrap().clone()
}
fn svg(size: usize, color: &str) -> Vec<u8> {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{size}\" height=\"{size}\" viewBox=\"0 0 {size} {size}\"><path d=\"M0 0 L{size} 0 L{size} {size} Z\" fill=\"{color}\"/></svg>").into_bytes()
}

#[test]
fn mask_state_colors_and_sizes_share_one_semantic_asset() {
    let mut manifest = manifest(vec![
        asset("a", "/icons/arrow.svg", &svg(40, "#0057bb"), "svg"),
        asset("b", "/icons/arrow-selected.svg", &svg(80, "#949caa"), "svg"),
    ]);
    let path = std::env::temp_dir();
    let policy = OutputPolicy::from_roots(vec![path]).unwrap();
    let resolved=reconcile(&mut manifest,&mut BTreeMap::new(),None,None,&policy,&result(r#"<><Box boxSize="40px" maskImage="url(/icons/arrow.svg)"/><Box boxSize="80px" maskImage="url(/icons/arrow-selected.svg)"/></>"#)).unwrap();
    assert_eq!(manifest.assets[0].path, manifest.assets[1].path);
    assert_eq!(
        manifest.assets[0].data_base64,
        manifest.assets[1].data_base64
    );
    assert_eq!(resolved.registry["assets"].as_array().unwrap().len(), 1);
    assert!(resolved.filters.is_empty());
}

#[test]
fn flat_gray_image_reuses_color_original_with_derived_filter() {
    let mut manifest = manifest(vec![
        asset("color", "/icons/logo.svg", &svg(40, "#0057bb"), "svg"),
        asset("gray", "/icons/logo-muted.svg", &svg(40, "#aaaaaa"), "svg"),
    ]);
    let policy = OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap();
    let mut result = Value::Object(result(
        r#"<><Image w="40px" h="40px" src="/icons/logo.svg"/><Image w="40px" h="40px" src="/icons/logo-muted.svg"/></>"#,
    ));
    let contract=devup_mcp_devup_ui::provenance::attributes::generated_attributes(result["tsx"].as_str().unwrap()).into_iter().map(|attr|json!({"elementIndex":attr.element,"tag":attr.tag,"generatedProperty":attr.source,"verified":true})).collect::<Vec<_>>();
    result["outputResults"] = json!({"tsx":{"_propertyContract":contract}});
    let resolved = reconcile(
        &mut manifest,
        &mut BTreeMap::new(),
        None,
        None,
        &policy,
        result.as_object().unwrap(),
    )
    .unwrap();
    assert_eq!(manifest.assets[0].path, manifest.assets[1].path);
    apply_filters(&mut result, &resolved.filters);
    assert!(
        result["tsx"]
            .as_str()
            .unwrap()
            .contains("grayscale(1) brightness(0) invert(")
    );
    assert!(result["tsx"].as_str().unwrap().contains("w=\"40px\""));
    assert!(
        devup_mcp_devup_ui::provenance::attributes::uncovered_attributes(
            result["tsx"].as_str().unwrap(),
            result["outputResults"]["tsx"]["_propertyContract"]
                .as_array()
                .unwrap()
        )
        .is_empty()
    );
}

#[test]
fn unsafe_svg_and_real_geometry_differences_are_not_merged() {
    assert!(
        super::super::svg_identity::identity(
            br#"<svg viewBox="0 0 40 40"><script>alert(1)</script></svg>"#,
            false
        )
        .is_none()
    );
    assert!(
        super::super::svg_identity::identity(
            br#"<svg viewBox="0 0 40 40"><use href="https://outside.test/a.svg#x"/></svg>"#,
            false
        )
        .is_none()
    );
    let triangle = super::super::svg_identity::identity(&svg(40, "#0057bb"), false).unwrap();
    let square = super::super::svg_identity::identity(
        br##"<svg viewBox="0 0 40 40"><rect width="40" height="40" fill="#0057bb"/></svg>"##,
        false,
    )
    .unwrap();
    assert_ne!(triangle.key, square.key);
}

#[test]
fn svg_ids_are_normalized_but_mask_paint_semantics_are_preserved() {
    let a=br##"<svg viewBox="0 0 20 20"><defs><mask id="figma-1"><rect width="20" height="20" fill="white"/></mask></defs><path mask="url(#figma-1)" d="M0 0L20 20" fill="#0057bb"/></svg>"##;
    let b = String::from_utf8(a.to_vec())
        .unwrap()
        .replace("figma-1", "figma-2");
    assert_eq!(
        super::super::svg_identity::identity(a, true).unwrap().key,
        super::super::svg_identity::identity(b.as_bytes(), true)
            .unwrap()
            .key
    );
    let c = b.replace("white", "black");
    assert_ne!(
        super::super::svg_identity::identity(a, true).unwrap().key,
        super::super::svg_identity::identity(c.as_bytes(), true)
            .unwrap()
            .key
    );
}

#[test]
fn repeated_exports_reuse_the_registry_and_preserve_user_files() {
    let root =
        std::env::temp_dir().join(format!("devup-canonical-assets-{}", rand::random::<u64>()));
    std::fs::create_dir_all(root.join("public")).unwrap();
    let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
    let public = root.join("public");
    let make = |id: &str, path: &str| manifest(vec![asset(id, path, b"one raster", "png")]);
    let mut first = make("a", "/images/hero.png");
    let mut outputs = BTreeMap::from([(
        "a".into(),
        public.join("images/hero.png").display().to_string(),
    )]);
    let resolved = reconcile(
        &mut first,
        &mut outputs,
        Some(&public),
        Some(root.to_str().unwrap()),
        &policy,
        &result("<Image src=\"/images/hero.png\" />"),
    )
    .unwrap();
    let mut transaction = OutputTransaction::new();
    transaction
        .stage_asset(
            "asset:a",
            policy.resolve(&outputs["a"]).unwrap(),
            b"one raster",
        )
        .unwrap();
    let (path, bytes, expected) = resolved.registry_write.unwrap();
    transaction
        .stage_registry(
            "assetRegistry",
            policy.resolve(&path).unwrap(),
            &bytes,
            expected,
        )
        .unwrap();
    transaction.commit().unwrap();
    let mut second = make("another-instance", "/images/mobile-hero.png");
    let mut outputs = BTreeMap::from([(
        "another-instance".into(),
        public.join("images/mobile-hero.png").display().to_string(),
    )]);
    let resolved = reconcile(
        &mut second,
        &mut outputs,
        Some(&public),
        Some(root.to_str().unwrap()),
        &policy,
        &result("<Image src=\"/images/mobile-hero.png\" />"),
    )
    .unwrap();
    assert_eq!(second.assets[0].path, first.assets[0].path);
    assert_eq!(resolved.registry["assets"].as_array().unwrap().len(), 1);
    std::fs::write(public.join("images/hero.png"), b"user edited").unwrap();
    let mut third = make("a", "/images/hero.png");
    let mut outputs = BTreeMap::from([(
        "a".into(),
        public.join("images/hero.png").display().to_string(),
    )]);
    reconcile(
        &mut third,
        &mut outputs,
        Some(&public),
        Some(root.to_str().unwrap()),
        &policy,
        &result("<Image src=\"/images/hero.png\" />"),
    )
    .unwrap();
    assert_ne!(third.assets[0].path, first.assets[0].path);
    assert_eq!(
        std::fs::read(public.join("images/hero.png")).unwrap(),
        b"user edited"
    );
    drop(policy);
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn late_asset_collision_rolls_back_code_and_registry_without_overwriting_assets() {
    let root =
        std::env::temp_dir().join(format!("devup-assets-rollback-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
    std::fs::write(root.join("page.tsx"), b"original code").unwrap();
    std::fs::write(root.join("registry.json"), b"original registry").unwrap();
    let mut transaction = OutputTransaction::new();
    transaction
        .stage("tsx", policy.resolve("page.tsx").unwrap(), b"new code")
        .unwrap();
    transaction
        .stage(
            "registry",
            policy.resolve("registry.json").unwrap(),
            b"new registry",
        )
        .unwrap();
    transaction
        .stage_asset(
            "asset:x",
            policy.resolve("picture.svg").unwrap(),
            b"exported picture",
        )
        .unwrap();
    std::fs::write(root.join("picture.svg"), b"concurrent user file").unwrap();
    assert!(transaction.commit().is_err());
    assert_eq!(
        std::fs::read(root.join("page.tsx")).unwrap(),
        b"original code"
    );
    assert_eq!(
        std::fs::read(root.join("registry.json")).unwrap(),
        b"original registry"
    );
    assert_eq!(
        std::fs::read(root.join("picture.svg")).unwrap(),
        b"concurrent user file"
    );
    let mut reuse = OutputTransaction::new();
    reuse
        .stage_asset(
            "asset:x",
            policy.resolve("picture.svg").unwrap(),
            b"concurrent user file",
        )
        .unwrap();
    reuse.commit().unwrap();
    drop(policy);
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn registry_paths_cannot_escape_public_root() {
    let root = std::env::temp_dir().join(format!("devup-assets-unsafe-{}", rand::random::<u64>()));
    std::fs::create_dir_all(root.join("public")).unwrap();
    std::fs::create_dir_all(root.join(".devup")).unwrap();
    std::fs::write(root.join(".devup/public-assets.json"),br#"{"version":1,"assets":[{"path":"/../private.json","identity":"x","sha256":"x","mimeType":"image/svg+xml"}]}"#).unwrap();
    let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
    assert!(
        reconcile(
            &mut manifest(Vec::new()),
            &mut BTreeMap::new(),
            Some(&root.join("public")),
            None,
            &policy,
            &Map::new()
        )
        .is_err()
    );
    drop(policy);
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn shared_assets_and_registry_updates_work_with_scaffold_no_clobber_and_stale_registry_is_refused()
{
    let root =
        std::env::temp_dir().join(format!("devup-assets-registry-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&root).unwrap();
    let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
    std::fs::write(root.join("logo.svg"), b"same").unwrap();
    std::fs::write(root.join("registry.json"), b"initial").unwrap();
    let mut transaction = OutputTransaction::new();
    transaction.refuse_existing();
    transaction
        .stage_asset("asset:x", policy.resolve("logo.svg").unwrap(), b"same")
        .unwrap();
    transaction
        .stage_registry(
            "assetRegistry",
            policy.resolve("registry.json").unwrap(),
            b"updated",
            Some(sha256_hex(b"initial")),
        )
        .unwrap();
    transaction.commit().unwrap();
    assert_eq!(
        std::fs::read(root.join("registry.json")).unwrap(),
        b"updated"
    );
    let mut stale = OutputTransaction::new();
    stale
        .stage_registry(
            "assetRegistry",
            policy.resolve("registry.json").unwrap(),
            b"stale",
            Some(sha256_hex(b"initial")),
        )
        .unwrap();
    assert!(stale.commit().is_err());
    assert_eq!(
        std::fs::read(root.join("registry.json")).unwrap(),
        b"updated"
    );
    drop(policy);
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn anonymous_names_and_collisions_require_review_and_cannot_be_final() {
    let mut pictures = manifest(vec![
        asset("a", "/images/image-123.png", b"one", "png"),
        asset("b", "/images/image-123.png", b"two", "png"),
    ]);
    let policy = OutputPolicy::from_roots(vec![std::env::temp_dir()]).unwrap();
    // Distinct pictures cannot share one source-code placeholder.
    assert!(
        reconcile(
            &mut pictures,
            &mut BTreeMap::new(),
            None,
            None,
            &policy,
            &Map::new()
        )
        .is_err()
    );
    let mut pictures = manifest(vec![asset("a", "/images/rectangle123.png", b"one", "png")]);
    let resolved = reconcile(
        &mut pictures,
        &mut BTreeMap::new(),
        None,
        None,
        &policy,
        &Map::new(),
    )
    .unwrap();
    assert_eq!(resolved.registry["namingComplete"], false);
    let mut value = json!({"status":"complete","assetRegistry":resolved.registry,"deliverable":{"isFinal":true}});
    enforce_naming_review(&mut value);
    assert_eq!(value["status"], "partial");
    assert_eq!(value["deliverable"]["isFinal"], false);
    assert_eq!(value["deliverable"]["assetNamingState"], "review-required");
}

#[test]
fn nested_unicode_paths_survive_registry_replay_and_code_only_projection() {
    let root = std::env::temp_dir().join(format!("devup-assets-unicode-{}", rand::random::<u64>()));
    std::fs::create_dir_all(root.join("public")).unwrap();
    let public = root.join("public");
    let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
    let path = "/images/기업소개/연구실.png";
    let mut first = manifest(vec![asset("a", path, b"lab photograph", "png")]);
    let mut outputs = BTreeMap::from([(
        "a".into(),
        public
            .join("images/기업소개/연구실.png")
            .display()
            .to_string(),
    )]);
    let resolved = reconcile(
        &mut first,
        &mut outputs,
        Some(&public),
        None,
        &policy,
        &Map::new(),
    )
    .unwrap();
    let encoded = super::super::super::validation::public_asset_url(
        &public,
        &public.join("images/기업소개/연구실.png"),
    )
    .unwrap();
    assert_eq!(first.assets[0].path.as_deref(), Some(encoded.as_str()));
    assert_eq!(resolved.registry["namingComplete"], true);
    let mut transaction = OutputTransaction::new();
    transaction
        .stage_asset(
            "asset:a",
            policy.resolve(&outputs["a"]).unwrap(),
            b"lab photograph",
        )
        .unwrap();
    let (registry, bytes, expected) = resolved.registry_write.unwrap();
    transaction
        .stage_registry(
            "assetRegistry",
            policy.resolve(&registry).unwrap(),
            &bytes,
            expected,
        )
        .unwrap();
    transaction.commit().unwrap();
    let mut second = manifest(vec![asset(
        "b",
        "/images/lab-mobile.png",
        b"lab photograph",
        "png",
    )]);
    let resolved = reconcile(
        &mut second,
        &mut BTreeMap::new(),
        Some(&public),
        None,
        &policy,
        &Map::new(),
    )
    .unwrap();
    assert_eq!(second.assets[0].path.as_deref(), Some(encoded.as_str()));
    let mut value = json!({"tsx":"<><Image src=\"/images/lab-mobile.png\"/><Image src=\"/icons/brand/콘드로젠.svg\" /></>"});
    super::super::rewrite_result_asset_references(&mut value, &resolved.replacements);
    assert!(value["tsx"].as_str().unwrap().contains(&encoded));
    assert!(
        value["tsx"]
            .as_str()
            .unwrap()
            .contains("/icons/brand/콘드로젠.svg")
    );
    assert_eq!(
        super::super::with_host_safe_asset_paths(value["tsx"].as_str().unwrap()),
        value["tsx"].as_str().unwrap()
    );
    drop(policy);
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn public_root_only_scope_preserves_explicit_filename_and_scopes_registry() {
    let root = std::env::temp_dir().join(format!(
        "devup-assets-public-scope-{}",
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let policy = OutputPolicy::from_roots(vec![root.clone()]).unwrap();
    let mut manifest = manifest(vec![asset("a", "/images/brand.png", b"brand", "png")]);
    let requested = root.join("icons/BI icon.png");
    let mut outputs = BTreeMap::from([("a".into(), requested.display().to_string())]);
    let resolved = reconcile(
        &mut manifest,
        &mut outputs,
        Some(&root),
        None,
        &policy,
        &Map::new(),
    )
    .unwrap();
    assert_eq!(
        manifest.assets[0].path.as_deref(),
        Some("/icons/BI%20icon.png")
    );
    assert_eq!(
        std::path::Path::new(&outputs["a"]),
        policy
            .resolve(&requested.display().to_string())
            .unwrap()
            .display_path()
    );
    let (registry, bytes, expected) = resolved.registry_write.unwrap();
    assert!(std::path::Path::new(&registry).ends_with(".devup/public-assets.json"));
    assert!(std::path::Path::new(&registry).starts_with(policy.primary_root()));
    let mut transaction = OutputTransaction::new();
    transaction
        .stage_asset("asset:a", policy.resolve(&outputs["a"]).unwrap(), b"brand")
        .unwrap();
    transaction
        .stage_registry(
            "assetRegistry",
            policy.resolve(&registry).unwrap(),
            &bytes,
            expected,
        )
        .unwrap();
    transaction.commit().unwrap();
    assert_eq!(std::fs::read(&requested).unwrap(), b"brand");
    drop(policy);
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn svg_comparison_never_rounds_geometry_or_guesses_implicit_paint() {
    let exact = |bytes: &[u8]| super::super::svg_identity::identity(bytes, false).unwrap();
    let a = br##"<svg viewBox="0 0 40 40"><path d="M0 0L40 0L40 40Z" fill="#aaaaaa"/></svg>"##;
    let b = String::from_utf8(a.to_vec())
        .unwrap()
        .replace("L40 40Z", "L40 39.999999999999Z");
    assert_ne!(exact(a).key, exact(b.as_bytes()).key);
    let implicit=br##"<svg viewBox="0 0 40 40"><path d="M0 0L20 20" fill="#aaaaaa"/><path d="M0 0L40 40"/></svg>"##;
    assert!(exact(implicit).grayscale_filter.is_none());
    let duplicate=br##"<svg viewBox="0 0 40 40"><path id="same" d="M0 0L20 20"/><path id="same" d="M0 0L40 40"/></svg>"##;
    assert!(super::super::svg_identity::identity(duplicate, true).is_none());
    let external=br##"<svg viewBox="0 0 40 40"><path d="M0 0L20 20" fill="url(https://outside.test/paint.svg#p)"/></svg>"##;
    assert!(super::super::svg_identity::identity(external, true).is_none());
}

#[test]
fn derived_filters_only_touch_the_exact_asset_url() {
    let mut value = json!({"tsx":"<><Image src=\"/icons/logo-muted.svg\"/><Image src=\"/icons/logo-muted.svg-other\"/></>"});
    apply_filters(
        &mut value,
        &BTreeMap::from([("/icons/logo-muted.svg".into(), "grayscale(1)".into())]),
    );
    assert_eq!(value["tsx"].as_str().unwrap().matches("filter=").count(), 1);
}
