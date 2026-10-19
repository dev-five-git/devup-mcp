#[tokio::test]
async fn missing_sections_and_asset_files_fail_even_without_strict() -> anyhow::Result<()> {
    let result = validate(json!({
        "tsx": "<Box data-figma-node-id=\"section\"><Image src=\"/missing-photo.png\" /></Box>",
        "projectRoot": fixture_project_root(),
        "sectionCoverage": {"nodeIds":["section","picture"]}
    }))
    .await?;
    assert_eq!(result["ok"], false);
    assert_eq!(result["okReason"], "error-violations");
    let violations = result["violations"].as_array().unwrap();
    for rule in ["asset-missing", "section-coverage"] {
        assert!(
            violations
                .iter()
                .any(|v| v["rule"] == rule && v["severity"] == "error"),
            "{result}"
        );
        assert!(
            result["verificationScope"]["static"]["rules"]
                .as_array()
                .unwrap()
                .contains(&json!(rule))
        );
    }
    let result = validate(json!({
        "files":[
            {"path":"Section.tsx","content":"export function Section() { return <Box data-figma-node-id=\"section\" />; }"},
            {"path":"Picture.tsx","content":"export function Picture() { return <Box data-figma-node-id=\"picture\" />; }"}
        ],
        "sectionCoverage":{"nodeIds":["section","picture"]}
    })).await?;
    assert!(
        !result["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["rule"] == "section-coverage"),
        "{result}"
    );
    Ok(())
}
