//! What the server says about itself: the guidance it publishes and the
//! staleness it reports.
//!
//! Both changes are about cost and honesty rather than features. The guidance
//! moved from `initialize` - where every client paid for Figma prose even when
//! it only called `devup_stack_diff` - into a resource the caller pulls. The
//! staleness signal rides the identity every response already carries, and it
//! is cache-only so it can never delay a call.

use devup_mcp::server::DevupServer;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, ReadResourceRequestParams, ResourceContents};
use serde_json::Value;

async fn connect() -> anyhow::Result<(
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tokio::task::JoinHandle<anyhow::Result<()>>,
)> {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);
    let server = tokio::spawn(async move {
        DevupServer::production_with_output_roots(vec![std::env::current_dir()?])?
            .serve(server_transport)
            .await?
            .waiting()
            .await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    Ok((client, server))
}

/// The instructions a client cannot avoid must stay small, and must still point
/// at everything that moved out of them.
#[tokio::test]
async fn initialize_instructions_are_small_and_name_the_guide() -> anyhow::Result<()> {
    let (client, server) = connect().await?;
    let info = client.peer_info().expect("server info");
    let instructions = info
        .instructions
        .as_deref()
        .expect("the server still publishes instructions");

    assert!(
        instructions.len() < 1_200,
        "instructions are {} bytes; every client pays this on every session",
        instructions.len()
    );
    assert!(
        instructions.contains("devup://guide/usage"),
        "instructions must name the guide, or the moved rules are unreachable"
    );
    // Identity is about reading every response, so it does not move behind a
    // fetch.
    assert!(instructions.contains("server.commit/buildId"));
    assert!(instructions.contains("server.updateAvailable"));
    // The one rule that changes the very next action stays too.
    assert!(instructions.contains("devup_figma_export"));
    // ...and the bulk of the Figma detail is gone from the hot path.
    assert!(
        !instructions.contains("WQUW-120"),
        "output sizing measurements belong in the pulled guide, not in instructions"
    );
    assert!(!instructions.contains("assetManifest"));

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

/// The guide has to be readable in a session that has produced no outputs at
/// all, which is exactly the session in which a client needs it.
#[tokio::test]
async fn the_guide_resource_lists_and_reads_in_a_fresh_session() -> anyhow::Result<()> {
    let (client, server) = connect().await?;

    let listed = client.list_all_resources().await?;
    let guide = listed
        .iter()
        .find(|resource| resource.uri == "devup://guide/usage")
        .expect("the guide is listed");
    assert_eq!(guide.mime_type.as_deref(), Some("text/markdown"));

    let read = client
        .read_resource(ReadResourceRequestParams::new("devup://guide/usage"))
        .await?;
    let text = read
        .contents
        .iter()
        .filter_map(|content| match content {
            ResourceContents::TextResourceContents { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<String>();

    // The relocation must not lose a rule. These are the ones that left the
    // instructions string.
    for moved in [
        "WQUW-120",
        "assetManifest",
        "selection_required",
        "get_design_context",
        "devup://artifact/",
    ] {
        assert!(text.contains(moved), "the guide lost {moved}");
    }
    assert!(
        text.len() > 2_000,
        "the guide should hold the bulk of the guidance, got {} bytes",
        text.len()
    );

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

/// A missing guide URI must still fail as a resource lookup, not as a panic or
/// a success with empty contents.
#[tokio::test]
async fn an_unknown_guide_topic_is_not_found() -> anyhow::Result<()> {
    let (client, server) = connect().await?;
    let result = client
        .read_resource(ReadResourceRequestParams::new(
            "devup://guide/does-not-exist",
        ))
        .await;
    assert!(result.is_err(), "{result:?}");
    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

/// Every tool response already carried build identity. It now also carries
/// whether a newer release exists - and in a test there is no populated cache,
/// so the honest answer is `unknown` rather than a guess.
#[tokio::test]
async fn every_tool_response_reports_staleness_without_blocking() -> anyhow::Result<()> {
    let (client, server) = connect().await?;

    // A tool that needs no network and no Figma credentials.
    let arguments = serde_json::json!({ "scope": "theme" })
        .as_object()
        .expect("object")
        .clone();
    let result = client
        .call_tool(CallToolRequestParams::new("devup_project_context").with_arguments(arguments))
        .await?;
    let structured: Value = result
        .structured_content
        .expect("devup tools return structured content");

    let identity = &structured["server"];
    assert_eq!(identity["version"], env!("CARGO_PKG_VERSION"));
    let update = &identity["updateAvailable"];
    assert_eq!(update["current"], env!("CARGO_PKG_VERSION"));
    let state = update["state"].as_str().expect("a state string");
    assert!(
        matches!(state, "unknown" | "known" | "disabled"),
        "unexpected state {state}"
    );
    // With no lookup completed, nothing may claim a verdict.
    if state == "unknown" {
        assert!(
            update.get("isStale").is_none(),
            "an unknown check must not report isStale: {update}"
        );
    }
    assert!(
        update["note"].as_str().is_some_and(|note| !note.is_empty()),
        "the state must explain itself: {update}"
    );

    client.cancel().await?;
    let _ = server.await;
    Ok(())
}

/// `--self-check` is documented as making no network call, and the release
/// lookup must not have changed that. The check starts from `initialize`, not
/// from server construction, and `self_check` constructs a server on a thread
/// with no Tokio runtime.
#[test]
fn self_check_stays_network_free_and_synchronous() {
    let report = devup_mcp::self_check();
    let value = serde_json::to_value(&report).expect("serializable");
    assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
    assert!(
        value.get("updateAvailable").is_none(),
        "self-check must not grow a release lookup: {value}"
    );
}
