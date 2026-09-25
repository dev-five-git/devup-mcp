use devup_mcp_figma::{ErrorCode, FigmaTarget};

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

/// The placeholder is the file the attached bridge plugin has open, so it
/// parses to a key only the bridge can serve and keeps the node it names.
#[test]
fn the_bridge_link_parses_to_a_bridge_only_target() {
    let current = FigmaTarget::parse("figma-bridge://current").expect("bare bridge link");
    assert!(current.is_bridge_only());
    assert_eq!(current, FigmaTarget::bridge_current());

    let node = FigmaTarget::parse("figma-bridge://current?node-id=1-2").expect("with a node");
    assert!(node.is_bridge_only());
    assert_eq!(node.node_id.as_deref(), Some("1:2"));

    for url in [
        "figma-bridge://other",
        "figma-bridge://current/path",
        "figma-bridge://current?node-id=abc",
    ] {
        let error = FigmaTarget::parse(url).expect_err("only figma-bridge://current is a link");
        assert_eq!(error.code, ErrorCode::DevupFigmaUnsupportedFile, "{url}");
    }
}

/// A link back to a target has to route to the same place. A Figma file keeps
/// its Figma URL; a file only the bridge can read keeps the bridge link, never
/// a Figma URL around a key no Figma file has.
#[test]
fn a_link_routes_back_to_its_own_target() {
    let figma = FigmaTarget::parse("https://www.figma.com/design/FileKey123/Name?node-id=1-2")
        .expect("Figma link");
    assert!(!figma.is_bridge_only());
    assert_eq!(
        figma.link(Some("3:4")),
        "https://www.figma.com/design/FileKey123/devup?node-id=3-4"
    );
    assert_eq!(
        FigmaTarget::parse(&figma.link(Some("3:4"))).expect("round trip"),
        FigmaTarget {
            node_id: Some("3:4".to_owned()),
            ..figma
        }
    );

    let bridge = FigmaTarget {
        file_key: "bridge:7".to_owned(),
        node_id: None,
        branch_key: None,
    };
    assert!(bridge.is_bridge_only());
    assert_eq!(bridge.link(None), "figma-bridge://current");
    assert_eq!(
        bridge.link(Some("3:4")),
        "figma-bridge://current?node-id=3-4"
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
