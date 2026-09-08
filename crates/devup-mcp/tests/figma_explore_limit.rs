//! What `limit` limits in `devup_figma_explore`, and what `truncated` means.
//!
//! `limit` reads as "how many candidates I want back". It was instead
//! multiplied by four and clamped into the projection budget the collection
//! script spends, and `includeTextPreview` spent that same budget on text -
//! so raising `limit` from 30 to 33 while turning previews on returned
//! *fewer* screens, and `truncated` could not say which of the two had cut
//! the list. These lock `limit` to the answer and split the two truncations.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use devup_mcp::server::{DevupAuth, DevupServer, Services};
use devup_mcp_figma::{
    AuthStatus, DevupError, ErrorCode, ExploreReadOptions, FigmaUpstream, ReadToolCall,
    UpstreamResult,
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

struct HeadingWithThreeScreens {
    projection_truncated: bool,
    options: std::sync::Mutex<Vec<ExploreReadOptions>>,
    calls: AtomicUsize,
}

impl HeadingWithThreeScreens {
    fn new(projection_truncated: bool) -> Self {
        Self {
            projection_truncated,
            options: std::sync::Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl FigmaUpstream for HeadingWithThreeScreens {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match call {
            ReadToolCall::ExploreSnapshot { options, .. } => {
                self.options.lock().unwrap().push(options);
                Ok(UpstreamResult {
                    raw: projection(self.projection_truncated),
                })
            }
            _ => Err(DevupError::new(
                ErrorCode::DevupSnapshotUnsupported,
                "unexpected test call",
                false,
            )),
        }
    }
}

fn projection(truncated: bool) -> Value {
    json!({
        "fileKey": "FileKey123",
        "version": null,
        "rootIds": ["0:1"],
        "nodes": [
            {
                "id": "0:1", "type": "PAGE",
                "fields": {
                    "name": "Phase2", "parentId": null, "childrenIds": [],
                    "x": 0, "y": 0, "width": 1400, "height": 1600, "childCount": 20,
                    "textPreview": "", "projectionTruncated": truncated
                },
                "extra": {}, "fieldErrors": {}
            },
            heading("1:1"),
            screen("1:2", 0.0),
            screen("1:3", 400.0),
            screen("1:4", 800.0),
        ],
        "diagnostics": []
    })
}

fn heading(id: &str) -> Value {
    json!({
        "id": id, "type": "FRAME",
        "fields": {
            "name": "[FR-026] Loading", "parentId": "0:1", "childrenIds": [],
            "x": 0, "y": 0, "width": 1200, "height": 80, "childCount": 1,
            "textPreview": "Loading"
        },
        "extra": {}, "fieldErrors": {}
    })
}

fn screen(id: &str, x: f64) -> Value {
    json!({
        "id": id, "type": "FRAME",
        "fields": {
            "name": "Loading", "parentId": "0:1", "childrenIds": [],
            "x": x, "y": 120, "width": 360, "height": 740, "childCount": 12,
            "textPreview": "Writing your entry"
        },
        "extra": {}, "fieldErrors": {}
    })
}

async fn explore(
    upstream: Arc<HeadingWithThreeScreens>,
    arguments: Value,
) -> anyhow::Result<Value> {
    let server = DevupServer::new(Services::new(Arc::new(Auth), upstream));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("devup_figma_explore").with_arguments(arguments))
        .await?;
    client.cancel().await?;
    task.await??;
    Ok(result.structured_content.unwrap())
}

fn ids(output: &Value) -> Vec<String> {
    output["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| candidate["node"]["nodeId"].as_str().unwrap().to_owned())
        .collect()
}

fn call(limit: u64, include_text_preview: bool) -> Value {
    json!({
        "url": "https://www.figma.com/design/FileKey123/Fixture?node-id=1-1",
        "limit": limit,
        "includeTextPreview": include_text_preview
    })
}

/// D5. `limit` is the number of candidates returned and nothing else, so how
/// much of the design gets read cannot change with it - the one thing that
/// let a larger `limit` answer with a shorter list.
#[tokio::test]
async fn the_projection_budget_does_not_move_with_limit_or_text_preview() -> anyhow::Result<()> {
    let mut asked = Vec::new();
    for (limit, preview) in [(1, false), (30, false), (33, true), (100, true)] {
        let upstream = Arc::new(HeadingWithThreeScreens::new(false));
        explore(upstream.clone(), call(limit, preview)).await?;
        asked.extend(upstream.options.lock().unwrap().iter().cloned());
    }

    let first = asked
        .first()
        .cloned()
        .expect("one explore call per request");
    assert!(
        asked
            .iter()
            .all(|options| options.projection_limit == first.projection_limit),
        "the projection budget must not depend on limit or includeTextPreview: {asked:?}"
    );
    Ok(())
}

/// Raising `limit` may never shorten the answer.
#[tokio::test]
async fn raising_the_limit_never_returns_fewer_candidates() -> anyhow::Result<()> {
    let mut previous: Option<Vec<String>> = None;
    for limit in 1..=4 {
        let upstream = Arc::new(HeadingWithThreeScreens::new(false));
        let output = explore(upstream, call(limit, true)).await?;
        let current = ids(&output);
        assert_eq!(
            output["count"].as_u64().unwrap() as usize,
            current.len(),
            "count must describe the list that was returned"
        );
        if let Some(previous) = &previous {
            assert!(
                current.len() >= previous.len(),
                "limit {limit} returned fewer candidates than limit {}: {current:?} vs {previous:?}",
                limit - 1
            );
            assert_eq!(
                &current[..previous.len()],
                previous.as_slice(),
                "a larger limit must extend the same ranked list"
            );
        }
        previous = Some(current);
    }
    assert_eq!(
        previous.unwrap(),
        ["1:2", "1:3", "1:4"],
        "three screens sit under this heading"
    );
    Ok(())
}

/// `truncated: true` alone never said which cut happened. The two are now
/// reported apart, and the one caused by `limit` carries the call that undoes
/// it.
#[tokio::test]
async fn truncation_says_whether_the_limit_or_the_snapshot_cut_the_list() -> anyhow::Result<()> {
    let by_limit = explore(Arc::new(HeadingWithThreeScreens::new(false)), call(2, true)).await?;
    assert_eq!(ids(&by_limit), ["1:2", "1:3"]);
    assert_eq!(by_limit["truncated"], true);
    assert_eq!(by_limit["truncation"]["candidates"], true);
    assert_eq!(by_limit["truncation"]["projection"], false);
    assert_eq!(by_limit["truncation"]["returned"], 2);
    assert_eq!(by_limit["truncation"]["found"], 3);
    assert_eq!(by_limit["truncation"]["nextAction"]["limit"], 3);

    let complete = explore(
        Arc::new(HeadingWithThreeScreens::new(false)),
        call(50, true),
    )
    .await?;
    assert_eq!(complete["truncated"], false);
    assert_eq!(complete["truncation"]["candidates"], false);
    assert_eq!(complete["truncation"]["projection"], false);

    let by_projection =
        explore(Arc::new(HeadingWithThreeScreens::new(true)), call(50, true)).await?;
    assert_eq!(ids(&by_projection), ["1:2", "1:3", "1:4"]);
    assert_eq!(by_projection["truncated"], true);
    assert_eq!(by_projection["truncation"]["candidates"], false);
    assert_eq!(by_projection["truncation"]["projection"], true);
    assert!(
        by_projection["truncation"]["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty()),
        "a truncated answer has to say why: {}",
        by_projection["truncation"]
    );
    Ok(())
}

/// The tool description has to say what `limit` counts, because the number a
/// caller picks is chosen from that sentence alone.
#[tokio::test]
async fn the_tool_description_states_what_limit_counts() -> anyhow::Result<()> {
    let server = DevupServer::new(Services::new(
        Arc::new(Auth),
        Arc::new(HeadingWithThreeScreens::new(false)),
    ));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let tools = client.list_all_tools().await?;
    let explore = tools
        .iter()
        .find(|tool| tool.name == "devup_figma_explore")
        .expect("devup_figma_explore is published");
    let description = explore.description.clone().unwrap_or_default();
    assert!(
        description.contains("limit"),
        "the description must say what limit counts: {description}"
    );
    client.cancel().await?;
    task.await??;
    Ok(())
}
