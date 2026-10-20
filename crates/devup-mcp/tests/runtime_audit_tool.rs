//! `devup_runtime_audit` through the real tool surface: a plan is written
//! only under an allowed root, a report bound to it is ingested, and anything
//! that is not declarative data is refused before a file is written.

use std::{path::Path, sync::Arc};

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, DevupError, ErrorCode, FigmaUpstream, ReadToolCall, UpstreamResult,
};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

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
            "this tool does not reach Figma",
            false,
        ))
    }
}

async fn call(root: &Path, arguments: Value) -> anyhow::Result<(bool, Value)> {
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
        .call_tool(CallToolRequestParams::new("devup_runtime_audit").with_arguments(arguments))
        .await?;
    client.cancel().await?;
    task.await??;
    Ok((
        result.is_error == Some(true),
        result.structured_content.unwrap_or_default(),
    ))
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([255, 255, 255, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn plan() -> Value {
    json!({
        "baseUrl": "http://127.0.0.1:4173",
        "allowedOrigins": ["http://127.0.0.1:4173"],
        "viewports": [{"width": 360, "height": 740}],
        "screens": [{"id": "today", "routePath": "/today",
            "checks": [{"kind": "no-horizontal-overflow"}]}]
    })
}

#[tokio::test]
async fn a_plan_is_written_and_a_report_bound_to_it_is_ingested() -> anyhow::Result<()> {
    let root =
        std::env::temp_dir().join(format!("devup-runtime-audit-tool-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let root = dunce::canonicalize(&root)?;

    let (failed, planned) = call(
        &root,
        json!({"action": "plan", "plan": plan(), "outputPath": "audit/plan.json"}),
    )
    .await?;
    assert!(!failed, "{planned}");
    let id = planned["planId"].as_str().unwrap().to_owned();
    assert_eq!(planned["expected"]["checkItems"], 1);
    let plan_bytes = std::fs::read(root.join("audit/plan.json"))?;

    let shot = png(360, 900);
    std::fs::write(root.join("audit/360-today.png"), &shot)?;
    let report = json!({
        "schemaVersion": 1, "kind": "devup-runtime-audit-report", "planId": id,
        "planSha256": sha256(&plan_bytes), "status": "completed",
        "executor": {"name": "test"},
        "runs": [{"viewport": {"width": 360, "height": 740, "deviceScaleFactor": 1},
            "environmentErrors": [],
            "screens": [{"id": "today", "status": "completed",
                "checks": [{"index": 0, "status": "pass", "measured": {"scrollWidth": 360}}],
                "screenshot": {"path": "360-today.png", "sha256": sha256(&shot), "bytes": shot.len()}}]}],
        "summary": {"pass": 1, "fail": 0, "inconclusive": 0, "skipped": 0}
    });
    std::fs::write(root.join("audit/report.json"), serde_json::to_vec(&report)?)?;
    let (failed, ingested) = call(
        &root,
        json!({"action": "ingest", "planPath": "audit/plan.json", "reportPath": "audit/report.json"}),
    )
    .await?;
    assert!(!failed, "{ingested}");
    assert_eq!(ingested["verdict"], "pass", "{ingested}");
    assert_eq!(ingested["runs"][0]["screenshots"]["verified"], 1);

    let (failed, refused) = call(
        &root,
        json!({"action": "plan", "outputPath": "audit/other.json",
            "plan": {"baseUrl": "http://127.0.0.1:4173", "allowedOrigins": ["http://127.0.0.1:4173"],
                "viewports": [{"width": 360, "height": 740}],
                "screens": [{"id": "x", "routePath": "/", "script": "process.exit()"}]}}),
    )
    .await?;
    assert!(failed, "{refused}");
    assert!(!root.join("audit/other.json").exists());

    let (failed, outside) = call(
        &root,
        json!({"action": "plan", "plan": plan(), "outputPath": "../escaped.json"}),
    )
    .await?;
    assert!(failed, "{outside}");
    std::fs::remove_dir_all(&root)?;
    Ok(())
}

#[tokio::test]
async fn consumer_template_is_allowlisted_and_missing_acquisitions_never_pass() -> anyhow::Result<()>
{
    let root = std::env::temp_dir().join(format!("devup-runtime-template-{}", std::process::id()));
    std::fs::create_dir_all(&root)?;
    let root = dunce::canonicalize(root)?;
    let widths = [1440, 1100, 858, 390];
    let plan = json!({
        "baseUrl": "http://127.0.0.1:4173", "allowedOrigins": ["http://127.0.0.1:4173"],
        "viewports": widths.iter().map(|width| json!({"width": width, "height": 100, "deviceScaleFactor": 2})).collect::<Vec<_>>(),
        "screens": [{"id": "fixture", "routePath": "/", "checks": [{"kind": "dimensions",
            "subject": {"testId": "target"}, "tolerancePx": 0.5,
            "figmaDimensions": widths.iter().map(|width| json!({"viewportWidth": width, "artifactId": "missing", "nodeId": "target"})).collect::<Vec<_>>()
        }]}]
    });
    let (failed, planned) = call(
        &root,
        json!({"action": "plan", "plan": plan,
        "outputPath": "plan.json", "templatePath": "consumer-audit"}),
    )
    .await?;
    assert!(!failed, "{planned}");
    assert_eq!(planned["template"]["executed"], false);
    assert_eq!(planned["dimensionReferences"][0]["status"], "unverified");
    assert_eq!(planned["template"]["files"].as_array().unwrap().len(), 6);
    let runner = std::fs::read_to_string(root.join("consumer-audit/run.mjs"))?;
    assert!(runner.contains("runPlan"));
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("consumer-audit/package.json"))?)?;
    assert_eq!(manifest["dependencies"]["playwright"], "1.63.0");

    let plan_bytes = std::fs::read(root.join("plan.json"))?;
    let runs = widths.iter().map(|width| {
        let bytes = png(width * 2, 200);
        let name = format!("{width}.png");
        std::fs::write(root.join(&name), &bytes).unwrap();
        json!({"viewport": {"width": width, "height": 100, "deviceScaleFactor": 2}, "environmentErrors": [],
            "screens": [{"id": "fixture", "status": "completed", "checks": [{"index": 0, "status": "pass", "measured": {
                "units": "css-px", "subjectCount": 1, "subjectVisible": true,
                "subject": {"width": 100, "height": 80}}}],
                "screenshot": {"path": name, "sha256": sha256(&bytes), "bytes": bytes.len()}}]})
    }).collect::<Vec<_>>();
    let mut report = json!({"schemaVersion": 1, "kind": "devup-runtime-audit-report", "planId": planned["planId"],
        "planSha256": sha256(&plan_bytes), "status": "completed", "runs": runs});
    std::fs::write(root.join("report.json"), serde_json::to_vec(&report)?)?;
    let (failed, ingested) = call(
        &root,
        json!({"action": "ingest", "planPath": "plan.json", "reportPath": "report.json"}),
    )
    .await?;
    assert!(!failed, "{ingested}");
    assert_eq!(ingested["verdict"], "inconclusive");
    assert_eq!(ingested["summary"]["inconclusive"], 4);
    assert_eq!(ingested["widthTable"].as_array().unwrap().len(), 4);
    assert_eq!(ingested["toleranceExceedances"], json!([]));

    report["planSha256"] = json!("edited");
    std::fs::write(root.join("report.json"), serde_json::to_vec(&report)?)?;
    let (_, refused) = call(
        &root,
        json!({"action": "ingest", "planPath": "plan.json", "reportPath": "report.json"}),
    )
    .await?;
    assert_eq!(refused["reason"], "plan-file-differs");
    for request in [
        json!({"action": "plan", "plan": plan, "outputPath": "other.json", "templatePath": "../outside"}),
        json!({"action": "plan", "plan": plan, "templatePath": "unbound"}),
        json!({"action": "plan", "plan": plan, "outputPath": "other.json", "command": "node run.mjs"}),
    ] {
        assert!(call(&root, request).await?.0);
        assert!(!root.join("other.json").exists());
    }
    let mut invalid = plan;
    invalid["screens"][0]["checks"][0]["tolerancePx"] = json!(-1);
    assert!(
        call(
            &root,
            json!({"action": "plan", "plan": invalid, "outputPath": "other.json"})
        )
        .await?
        .0
    );
    std::fs::remove_dir_all(root)?;
    Ok(())
}
