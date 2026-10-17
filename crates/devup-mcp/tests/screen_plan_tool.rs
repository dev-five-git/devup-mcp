//! `devup_screen_plan` and the export gate through the real tool surface: a
//! draft hands back every open question at once, confirm refuses a partial set
//! of answers, and a page write without a confirmed plan is answered with the
//! plan to make before Figma is read.

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, DevupError, ErrorCode, FigmaUpstream, ReadToolCall, UpstreamResult,
};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Map, Value, json};

struct Auth;

#[async_trait]
impl DevupAuth for Auth {
    async fn status(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }
    async fn login(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Connected)
    }
    async fn logout(&self) -> Result<AuthStatus, DevupError> {
        Ok(AuthStatus::Disconnected)
    }
}

struct NoFigma;

#[async_trait]
impl FigmaUpstream for NoFigma {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec![])
    }
    async fn call_read_tool(&self, _call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        Err(DevupError::new(
            ErrorCode::DevupSnapshotUnsupported,
            "this test does not reach Figma",
            false,
        ))
    }
}

async fn call(root: &Path, tool: &'static str, arguments: Value) -> anyhow::Result<(bool, Value)> {
    let server = DevupServer::with_output_roots(
        Services::new(Arc::new(Auth), Arc::new(NoFigma)),
        vec![root.to_path_buf()],
    )?;
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments))
        .await?;
    client.cancel().await?;
    task.await??;
    Ok((
        result.is_error == Some(true),
        result.structured_content.unwrap_or_default(),
    ))
}

fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("devup-screen-plan-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/app/about")).unwrap();
    std::fs::write(dir.join("package.json"), "{}").unwrap();
    std::fs::write(
        dir.join("src/app/page.tsx"),
        "export default function Page() { return <main />; }",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/app/about/page.tsx"),
        "export default function Page() { return <main />; }",
    )
    .unwrap();
    dunce::canonicalize(dir).unwrap()
}

const FILE: &str = "https://www.figma.com/design/FileKey123/Site";

#[tokio::test]
async fn draft_asks_everything_at_once_and_confirm_needs_every_answer() -> anyhow::Result<()> {
    let root = project("flow");
    let root_text = root.to_string_lossy().into_owned();
    let (failed, draft) = call(
        &root,
        "devup_screen_plan",
        json!({"action": "draft", "projectRoot": root_text, "url": format!("{FILE}?node-id=1-1"),
        "screens": [
            {"nodeId": "1:2", "name": "Main - 1440", "width": 1440},
            {"nodeId": "1:3", "name": "Main - 390", "width": 390},
            {"nodeId": "1:4", "name": "About", "width": 1440},
            {"nodeId": "1:5", "name": "Pricing", "width": 1440}
        ]}),
    )
    .await?;
    assert!(!failed, "{draft}");
    assert_eq!(
        draft["askInstruction"],
        "Ask all of these in ONE call to your question tool; do not ask in prose; do not implement before confirm."
    );
    let ids = draft["questions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|question| question["id"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        ["collision:1:2", "collision:1:4", "route:1:5", "responsive"],
        "{draft}"
    );
    assert_eq!(draft["screens"][0]["proposedRoute"], "/");
    assert_eq!(draft["screens"][0]["drawnWidths"], json!([1440, 390]));
    assert_eq!(draft["screens"][2]["routeEvidence"], "UNVERIFIED");
    let plan_id = draft["planId"].as_str().unwrap().to_owned();

    let (failed, refused) = call(
        &root,
        "devup_screen_plan",
        json!({"action": "confirm", "projectRoot": root_text, "planId": plan_id,
            "answers": {"route:1:5": "/pricing (Recommended)"}}),
    )
    .await?;
    assert!(failed);
    let refusal = refused.to_string();
    assert!(
        refusal.contains("collision:1:4") && refusal.contains("responsive"),
        "{refusal}"
    );

    let (failed, gated) = call(
        &root,
        "devup_figma_export",
        json!({"url": format!("{FILE}?node-id=1-5"), "projectRoot": root_text,
            "outputs": ["pageScaffold"], "pageScaffold": {"route": "/pricing", "write": true},
            "outputPaths": {"pageScaffold": root_text}}),
    )
    .await?;
    assert!(!failed, "{gated}");
    assert_eq!(gated["status"], "plan_required", "{gated}");

    let (failed, confirmed) = call(
        &root,
        "devup_screen_plan",
        json!({"action": "confirm", "projectRoot": root_text, "planId": plan_id,
            "answers": {"route:1:5": "/pricing (Recommended)",
                "collision:1:2": "Merge into the existing page (Recommended)",
                "collision:1:4": "Merge into the existing page (Recommended)",
                "responsive": "One responsive route (Recommended)"}}),
    )
    .await?;
    assert!(!failed, "{confirmed}");
    assert_eq!(confirmed["status"], "confirmed");
    assert_eq!(confirmed["routes"][2]["url"], "/pricing");
    assert_eq!(confirmed["routes"][2]["file"], "src/app/pricing/page.tsx");

    let (failed, mismatch) = call(
        &root,
        "devup_figma_export",
        json!({"url": format!("{FILE}?node-id=1-5"), "projectRoot": root_text,
            "outputs": ["pageScaffold"], "pageScaffold": {"route": "/plans", "write": true},
            "outputPaths": {"pageScaffold": root_text}}),
    )
    .await?;
    assert!(failed, "{mismatch}");
    assert!(mismatch.to_string().contains("routeMismatch"), "{mismatch}");

    let (failed, status) = call(
        &root,
        "devup_screen_plan",
        json!({"action": "status", "projectRoot": root_text}),
    )
    .await?;
    assert!(!failed, "{status}");
    assert_eq!(
        status["summary"],
        "3 URLs: 2 implemented, 0 verified at all widths; no runtime audit report, so every width is not-run"
    );
    std::fs::remove_dir_all(root).ok();
    Ok(())
}

#[tokio::test]
async fn a_single_component_export_is_not_gated() -> anyhow::Result<()> {
    let root = project("component");
    let (failed, result) = call(
        &root,
        "devup_figma_export",
        json!({"url": format!("{FILE}?node-id=1-2"), "projectRoot": root.to_string_lossy(),
            "outputs": ["tsx"]}),
    )
    .await?;
    // It goes on to read Figma, which this test refuses: the refusal is the
    // proof that the gate let it through.
    assert!(failed, "{result}");
    assert_ne!(result["status"], "plan_required");
    assert!(
        result
            .to_string()
            .contains("this test does not reach Figma"),
        "{result}"
    );
    std::fs::remove_dir_all(root).ok();
    Ok(())
}
