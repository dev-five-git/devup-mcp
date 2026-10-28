#[tokio::test]
async fn summary_compacts_and_merges_findings() -> anyhow::Result<()> {
    let output = validate(json!({"detail":"summary","tsx":"<><Text maxLength={1} /><Text maxLength={1} /></>"})).await?;
    assert_eq!(output["detail"], "summary");
    assert!(output.get("navigation").is_none());
    assert!(output.get("verificationScope").is_none());
    assert!(output.get("checkedTokens").is_none());
    assert!(output["violationCounts"]["error"].as_u64().unwrap() >= 2);
    let rows = output["violations"].as_array().unwrap();
    assert!(rows.iter().any(|row| row["count"] == 2));
    assert!(rows.iter().all(|row| row.get("context").is_none() && row.get("byteRange").is_none()));
    Ok(())
}

#[tokio::test]
async fn summary_caps_distinct_findings_without_losing_error_counts() -> anyhow::Result<()> {
    let elements = (0..105).map(|index| format!("<Text invalidProp{index}={{1}} />")).collect::<String>();
    let output = validate(json!({"detail":"summary","tsx":format!("<>\n{elements}</>")})).await?;
    assert_eq!(output["violations"].as_array().unwrap().len(), 100);
    assert_eq!(output["violationsTruncated"], json!({"shown":100,"total":105}));
    assert_eq!(output["violationCounts"]["error"], 105);
    assert!(output["violations"].as_array().unwrap().iter().all(|row| row["line"] == 2));
    Ok(())
}

#[tokio::test]
async fn summary_groups_hardcoded_values_across_files() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("devup-validation-group-{}", rand::random::<u64>()));
    std::fs::create_dir_all(root.join("src"))?;
    std::fs::write(root.join("devup.json"), r#"{"theme":{"length":{"default":{"space8":"8px"}}}}"#)?;
    std::fs::write(root.join("src/A.tsx"), "export function A() { return <Box gap=\"8px\" p=\"8px\" left=\"0px\" /> }")?;
    std::fs::write(root.join("src/B.tsx"), "export function B() {\n  return <Box gap=\"8px\" />\n}")?;
    let output = validate(json!({"paths":["src"],"projectRoot":root,"detail":"summary"})).await?;
    let rows = output["violations"].as_array().unwrap();
    let grouped = rows.iter().filter(|row| row["value"] == "8px").collect::<Vec<_>>();
    assert_eq!(grouped.len(), 1, "{rows:?}");
    let row = grouped[0];
    assert_eq!(row["count"], 3);
    assert_eq!(row["tokens"], json!(["$space8"]));
    assert_eq!(row["props"], json!(["gap", "p"]));
    assert_eq!(row["files"].as_array().unwrap().len(), 2);
    assert!(!rows.iter().any(|row| row["value"] == "0px"));
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[tokio::test]
async fn paths_expands_sources_and_uses_disk_theme() -> anyhow::Result<()> {
    let root = std::env::temp_dir().join(format!("devup-validation-paths-{}", rand::random::<u64>()));
    std::fs::create_dir_all(root.join("src/app"))?;
    std::fs::create_dir_all(root.join("src/components"))?;
    std::fs::create_dir_all(root.join("src/node_modules"))?;
    std::fs::create_dir_all(root.join("src/.next"))?;
    std::fs::write(root.join("devup.json"), r##"{"theme":{"colors":{"default":{"bg":"#ffffff"}}}}"##)?;
    std::fs::write(root.join("src/app/page.tsx"), "export default function HomePage() { return <Box bg=\"$bg\" /> }")?;
    std::fs::write(root.join("src/components/Card.tsx"), "export function Card() { return <Text maxLength={2} /> }")?;
    std::fs::write(root.join("src/node_modules/ignored.tsx"), "invalid")?;
    std::fs::write(root.join("src/.next/ignored.tsx"), "invalid")?;
    for detail in ["summary", "full"] {
        let output = validate(json!({"paths":["src"],"projectRoot":root,"detail":detail})).await?;
        assert_eq!(output["pathsRead"], 2);
        assert_eq!(output["themeAvailable"], true);
        assert!(!output["violations"].as_array().unwrap().iter().any(|row| row["rule"] == "unknown-token"));
        assert!(output["violations"].as_array().unwrap().iter().any(|row| row["sourceName"] == "src/components/Card.tsx" || row["path"] == "src/components/Card.tsx"));
    }
    assert!(validate(json!({"paths":["src"],"tsx":"<Box />","projectRoot":root})).await.is_err());
    assert!(validate(json!({"paths":[root.parent().unwrap()],"projectRoot":root})).await.is_err());
    std::fs::remove_dir_all(root)?;
    Ok(())
}
