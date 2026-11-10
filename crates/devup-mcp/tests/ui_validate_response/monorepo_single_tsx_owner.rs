
/// A repository root holding two apps and a file neither owns. Each app has
/// its own theme: `$brand` is `a`'s token and `$accent` is `b`'s.
fn two_apps(label: &str) -> anyhow::Result<std::path::PathBuf> {
    let root = std::env::temp_dir().join(format!("devup-token-owner-{label}-{}", rand::random::<u64>()));
    for (path, content) in [
        ("package.json", "{}"),
        ("apps/a/package.json", "{}"),
        ("apps/a/devup.json", r##"{"theme":{"colors":{"default":{"brand":"#0066FF"}}}}"##),
        ("apps/a/src/components/Hero.tsx", A_HERO),
        ("apps/a/src/components/Shared.tsx", SHARED),
        ("apps/b/package.json", "{}"),
        ("apps/b/devup.json", r##"{"theme":{"colors":{"default":{"accent":"#FF6600"}}}}"##),
        ("apps/b/src/components/Banner.tsx", B_BANNER),
        ("apps/b/src/components/Shared.tsx", SHARED),
        ("tools/Orphan.tsx", ORPHAN),
    ] {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap())?;
        std::fs::write(file, content)?;
    }
    Ok(root)
}

const A_HERO: &str = "export function Hero() {\n  return <Box bg=\"$brand\" color=\"$accent\" />\n}\n";
const B_BANNER: &str = "export function Banner() {\n  return <Box bg=\"$brand\" color=\"$accent\" />;\n}\n";
const SHARED: &str = "export function Shared() { return <Box bg=\"$brand\" /> }\n";
const ORPHAN: &str = "export function Orphan() { return <Box bg=\"$brand\" /> }\n";

fn unknown(output: &Value) -> Vec<String> {
    output["violations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["rule"] == "unknown-token")
        .map(|finding| finding["snippet"].as_str().unwrap().to_owned())
        .collect()
}

/// One `tsx` at a monorepo root is checked against the theme of the app that
/// owns the file, without the caller choosing a projectRoot per app: by its
/// path (relative or absolute), by the tail of its name, or by its text.
#[tokio::test]
async fn a_single_tsx_at_a_monorepo_root_is_checked_against_its_owning_app() -> anyhow::Result<()> {
    let root = two_apps("owned")?;
    let absolute = root.join("apps").join("a").join("src").join("components").join("Hero.tsx");
    let runs = [
        // apps/a: by absolute path, by a path relative to the app, by bare name, by content alone.
        (validate(json!({"tsx":A_HERO,"projectRoot":root,"sourceName":absolute})).await, "a", "sourceName"),
        (validate(json!({"tsx":A_HERO,"projectRoot":root,"sourceName":"src/components/Hero.tsx"})).await, "a", "sourceName-suffix"),
        (validate(json!({"tsx":A_HERO,"projectRoot":root,"sourceName":"Hero.tsx"})).await, "a", "sourceName-suffix"),
        (validate(json!({"tsx":A_HERO,"projectRoot":root})).await, "a", "content-match"),
        // The editor sent CRLF line ends and no trailing newline: still the same file.
        (validate(json!({"tsx":A_HERO.trim_end().replace('\n', "\r\n"),"projectRoot":root})).await, "a", "content-match"),
        // apps/b: the same three ways.
        (validate(json!({"tsx":B_BANNER,"projectRoot":root,"sourceName":"apps/b/src/components/Banner.tsx"})).await, "b", "sourceName"),
        (validate(json!({"tsx":B_BANNER,"projectRoot":root,"sourceName":"components/Banner.tsx"})).await, "b", "sourceName-suffix"),
        (validate(json!({"tsx":B_BANNER,"projectRoot":root})).await, "b", "content-match"),
        // A new file that is not on disk yet, placed by its path.
        (validate(json!({"tsx":B_BANNER,"projectRoot":root,"sourceName":"apps/b/src/components/New.tsx"})).await, "b", "sourceName"),
    ];
    std::fs::remove_dir_all(&root)?;
    for (result, app, resolved_from) in runs {
        let out = result?;
        // The same two references in both files: each app knows one of them.
        let (known, missing) = if app == "a" { ("$brand", "$accent") } else { ("$accent", "$brand") };
        assert_eq!(unknown(&out), [format!("\"{missing}\"")], "{app}/{resolved_from}: {out}");
        assert!(!out.to_string().contains(&format!("\"snippet\":\"\\\"{known}\\\"\"")), "{out}");
        assert_eq!(out["themeAvailable"], true, "{out}");
        assert_eq!(out["tokens"]["unknownTokenCheckRan"], true, "{out}");
        assert_eq!(out["tokens"]["devupJson"], format!("apps/{app}/devup.json"), "{out}");
        assert_eq!(out["tokens"]["owner"]["resolvedFrom"], resolved_from, "{out}");
        assert_eq!(out["tokens"]["owner"]["devupJson"], format!("apps/{app}/devup.json"), "{out}");
        assert!(out["themeGuardrail"].is_null(), "{out}");
        assert_eq!(out["okReason"], "error-violations", "{out}");
    }
    Ok(())
}

/// Unverified is reported only when no owner can be resolved: a file no theme
/// governs, a file that exists under two themes, and text found nowhere.
#[tokio::test]
async fn a_single_tsx_with_no_resolvable_owner_stays_unverified() -> anyhow::Result<()> {
    let root = two_apps("orphan")?;
    let orphan_by_path = validate(json!({"tsx":ORPHAN,"projectRoot":root,"sourceName":"tools/Orphan.tsx"})).await;
    let orphan_by_content = validate(json!({"tsx":ORPHAN,"projectRoot":root})).await;
    let ambiguous_by_name = validate(json!({"tsx":SHARED,"projectRoot":root,"sourceName":"Shared.tsx"})).await;
    let ambiguous_by_content = validate(json!({"tsx":SHARED,"projectRoot":root})).await;
    let nowhere = validate(json!({"tsx":"export function Draft() { return <Box bg=\"$brand\" /> }","projectRoot":root})).await;
    let outside = validate(json!({"tsx":A_HERO,"projectRoot":root.join("apps").join("b"),"sourceName":root.join("apps").join("a").join("src").join("components").join("Hero.tsx")})).await;
    // The ambiguity ends once the caller says which copy it is.
    let chosen = validate(json!({"tsx":SHARED,"projectRoot":root,"sourceName":"apps/b/src/components/Shared.tsx"})).await;
    std::fs::remove_dir_all(&root)?;

    for (result, candidates) in [
        (orphan_by_path, Some(vec!["tools/Orphan.tsx"])),
        (orphan_by_content, Some(vec!["tools/Orphan.tsx"])),
        (ambiguous_by_name, Some(vec!["apps/a/src/components/Shared.tsx", "apps/b/src/components/Shared.tsx"])),
        (ambiguous_by_content, Some(vec!["apps/a/src/components/Shared.tsx", "apps/b/src/components/Shared.tsx"])),
        (nowhere, None),
    ] {
        let out = result?;
        assert!(unknown(&out).is_empty(), "{out}");
        assert_eq!(out["themeAvailable"], false, "{out}");
        assert_eq!(out["tokens"]["unknownTokenCheckRan"], false, "{out}");
        assert_eq!(out["okReason"], "clean-unverified-tokens", "{out}");
        assert!(out["tokens"]["devupJson"].is_null() && out["tokens"]["owner"].is_null(), "{out}");
        match candidates {
            // The files that were found are named, so the caller can choose.
            Some(paths) => assert_eq!(out["themeGuardrail"]["ownerCandidates"], json!(paths), "{out}"),
            None => assert!(out["themeGuardrail"]["ownerCandidates"].is_null(), "{out}"),
        }
        // The repository-wide answer still names the themes there are.
        assert_eq!(out["themeGuardrail"]["candidateProjectRoots"].as_array().map(Vec::len), Some(2), "{out}");
    }
    // A path outside projectRoot is not followed: apps/b's own theme applies.
    let out = outside?;
    assert_eq!(out["tokens"]["devupJson"].as_str().map(|path| path.replace('\\', "/").ends_with("apps/b/devup.json")), Some(true), "{out}");
    assert!(out["tokens"]["owner"].is_null(), "{out}");
    let out = chosen?;
    assert_eq!(unknown(&out), ["\"$brand\""], "{out}");
    assert_eq!(out["tokens"]["owner"]["resolvedFrom"], "sourceName", "{out}");
    Ok(())
}
