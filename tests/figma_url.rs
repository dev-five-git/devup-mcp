use devup_mcp::figma::{ErrorCode, FigmaTarget};

#[test]
fn parses_design_link_and_normalizes_dash_node_id() {
    let target = FigmaTarget::parse(
        "https://www.figma.com/design/85CgSws3o5XsLv7aAwWJyS/%EA%B8%B0%EB%A1%9D?node-id=3879-35481&t=secret",
    )
    .expect("valid Figma design link");

    assert_eq!(target.file_key, "85CgSws3o5XsLv7aAwWJyS");
    assert_eq!(target.node_id.as_deref(), Some("3879:35481"));
    assert_eq!(target.branch_key, None);
}

#[test]
fn accepts_file_and_branch_routes() {
    let file = FigmaTarget::parse("https://figma.com/file/Abc_123-xyz/Name")
        .expect("legacy file links remain valid");
    assert_eq!(file.file_key, "Abc_123-xyz");

    let branch = FigmaTarget::parse(
        "https://www.figma.com/branch/FileKey123/BranchKey456/Name?node-id=1%3A2",
    )
    .expect("branch link");
    assert_eq!(branch.file_key, "FileKey123");
    assert_eq!(branch.branch_key.as_deref(), Some("BranchKey456"));
    assert_eq!(branch.node_id.as_deref(), Some("1:2"));
}

#[test]
fn rejects_non_figma_and_deceptive_hosts() {
    for url in [
        "http://www.figma.com/design/GoodKey123/Name",
        "https://figma.com.evil.test/design/GoodKey123/Name",
        "https://evil.test/design/GoodKey123/Name",
    ] {
        let error = FigmaTarget::parse(url).expect_err("host or scheme must be rejected");
        assert_eq!(error.code, ErrorCode::DevupFigmaUnsupportedFile);
        assert!(!error.retryable);
    }
}

#[test]
fn rejects_malformed_keys_and_node_ids() {
    for url in [
        "https://figma.com/design/a/Name",
        "https://figma.com/design/key.with.dot/Name",
        "https://figma.com/design/GoodKey123/Name?node-id=abc",
        "https://figma.com/design/GoodKey123/Name?node-id=1%3A2%3A3",
    ] {
        let error = FigmaTarget::parse(url).expect_err("malformed link must be rejected");
        assert_eq!(error.code, ErrorCode::DevupFigmaUnsupportedFile);
    }
}

#[test]
fn serializes_the_stable_public_error_code() {
    let error =
        FigmaTarget::parse("https://evil.test/design/GoodKey123/Name").expect_err("invalid host");

    assert_eq!(
        serde_json::to_value(error.code).expect("serialize error code"),
        "DEVUP_FIGMA_UNSUPPORTED_FILE"
    );
}

#[test]
fn errors_do_not_echo_query_secrets() {
    let error = FigmaTarget::parse(
        "https://evil.test/design/GoodKey123/Name?access_token=top-secret&code=oauth-code",
    )
    .expect_err("invalid host");

    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains("top-secret"));
    assert!(!rendered.contains("oauth-code"));
    assert!(!rendered.contains("access_token"));
}
