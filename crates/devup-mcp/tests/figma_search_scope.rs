//! `devup_figma_search` and the `node-id` in the URL it was given.
//!
//! The tool tells its caller it is there to "locate the target before
//! `devup_figma_export`", and the caller hands it the link they were given -
//! which, in a hand-off, points at a Section. Searching the whole file for
//! that name answers with same-named frames from other pages, and frame ids
//! taken from that answer export the wrong screen. These lock the linked node
//! as the search scope.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

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

/// Two pages, four frames named `Loading`. Two of them sit inside the Section
/// the caller linked; the other two are elsewhere in the file. Node ids sort
/// so that the two out-of-scope frames rank ahead of the in-scope ones, which
/// is what made a small `limit` return only wrong answers.
#[derive(Default)]
struct TwoPageFile {
    search_limits: std::sync::Mutex<Vec<usize>>,
    calls: AtomicUsize,
}

#[async_trait]
impl FigmaUpstream for TwoPageFile {
    async fn list_tools(&self) -> Result<Vec<String>, DevupError> {
        Ok(vec!["use_figma".to_owned()])
    }

    async fn call_read_tool(&self, call: ReadToolCall) -> Result<UpstreamResult, DevupError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match call {
            ReadToolCall::PageCatalog { .. } => Ok(UpstreamResult {
                raw: page_catalog(),
            }),
            ReadToolCall::SearchSnapshot {
                node_id, options, ..
            } => {
                self.search_limits.lock().unwrap().push(options.limit);
                Ok(UpstreamResult {
                    raw: page_projection(&node_id),
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

fn page_catalog() -> Value {
    json!({
        "fileKey": "FileKey123",
        "version": null,
        "rootIds": ["0:1", "0:2"],
        "nodes": [
            node("0:1", "PAGE", "Phase2 Hand-off", Value::Null, &[]),
            node("0:2", "PAGE", "Components", Value::Null, &[]),
        ],
        "diagnostics": []
    })
}

fn page_projection(page_id: &str) -> Value {
    let nodes = if page_id == "0:1" {
        vec![
            node(
                "0:1",
                "PAGE",
                "Phase2 Hand-off",
                Value::Null,
                &["4279:7810", "3805:48672"],
            ),
            node(
                "4279:7810",
                "SECTION",
                "Loading screen improvements",
                json!("0:1"),
                &["3831:10708", "3831:10723"],
            ),
            node("3831:10708", "FRAME", "Loading", json!("4279:7810"), &[]),
            node("3831:10723", "FRAME", "Loading", json!("4279:7810"), &[]),
            node("3805:48672", "FRAME", "Loading", json!("0:1"), &[]),
        ]
    } else {
        vec![
            node("0:2", "PAGE", "Components", Value::Null, &["1690:33290"]),
            node("1690:33290", "FRAME", "Loading", json!("0:2"), &[]),
        ]
    };
    json!({
        "fileKey": "FileKey123",
        "version": null,
        "rootIds": [page_id],
        "nodes": nodes,
        "diagnostics": []
    })
}

fn node(id: &str, node_type: &str, name: &str, parent_id: Value, children: &[&str]) -> Value {
    json!({
        "id": id,
        "type": node_type,
        "fields": {
            "name": name,
            "parentId": parent_id,
            "childrenIds": children,
        },
        "extra": {},
        "fieldErrors": {}
    })
}

async fn search(arguments: Value) -> anyhow::Result<(Value, Arc<TwoPageFile>)> {
    let upstream = Arc::new(TwoPageFile::default());
    let server = DevupServer::new(Services::new(Arc::new(Auth), upstream.clone()));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let arguments: Map<String, Value> = arguments.as_object().cloned().unwrap();
    let result = client
        .call_tool(CallToolRequestParams::new("devup_figma_search").with_arguments(arguments))
        .await?;
    client.cancel().await?;
    task.await??;
    Ok((result.structured_content.unwrap(), upstream))
}

fn ids(output: &Value, key: &str) -> Vec<String> {
    output[key]
        .as_array()
        .map(|matches| {
            matches
                .iter()
                .map(|entry| entry["nodeId"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// D9. The reported reproduction: a Section link, `query: "Loading"`, and an
/// answer made entirely of frames from outside that Section.
#[tokio::test]
async fn a_node_id_in_the_url_scopes_the_search_to_that_subtree() -> anyhow::Result<()> {
    let (output, _) = search(json!({
        "url": "https://www.figma.com/design/FileKey123/Girok?node-id=4279-7810",
        "query": "Loading",
        "limit": 5
    }))
    .await?;

    // The linked Section is in its own scope; the two same-named frames on
    // other pages are not.
    assert_eq!(
        ids(&output, "matches"),
        ["3831:10708", "3831:10723", "4279:7810"]
    );
    assert_eq!(output["count"], 3);
    assert_eq!(output["scope"]["kind"], "node");
    assert_eq!(output["scope"]["nodeId"], "4279:7810");
    assert_eq!(output["scope"]["excludedOutOfScope"], 2);
    Ok(())
}

/// The filter runs before `limit` cuts the list, not after. Ranking alone put
/// both out-of-scope frames first, so filtering a already-truncated list
/// returned nothing at all for `limit: 1`.
#[tokio::test]
async fn a_small_limit_still_returns_in_scope_matches() -> anyhow::Result<()> {
    let (output, upstream) = search(json!({
        "url": "https://www.figma.com/design/FileKey123/Girok?node-id=4279-7810",
        "query": "Loading",
        "limit": 1
    }))
    .await?;

    assert_eq!(ids(&output, "matches"), ["3831:10708"]);
    assert_eq!(output["count"], 1);
    assert_eq!(output["scope"]["candidatesTruncated"], true);
    // The per-page projection is asked for its ceiling rather than the
    // caller's `limit`, or the in-scope frames never reach the filter.
    assert!(
        upstream
            .search_limits
            .lock()
            .unwrap()
            .iter()
            .all(|limit| *limit == 100)
    );
    Ok(())
}

/// A URL without a `node-id` is still a whole-file search - the scope is
/// taken from the link, never invented.
#[tokio::test]
async fn a_url_without_a_node_id_still_searches_the_whole_file() -> anyhow::Result<()> {
    let (output, _) = search(json!({
        "url": "https://www.figma.com/design/FileKey123/Girok",
        "query": "Loading",
        "limit": 5
    }))
    .await?;

    assert_eq!(
        ids(&output, "matches"),
        [
            "1690:33290",
            "3805:48672",
            "3831:10708",
            "3831:10723",
            "4279:7810"
        ]
    );
    assert_eq!(output["scope"]["kind"], "file");
    assert_eq!(output["scope"]["nodeId"], Value::Null);
    Ok(())
}

/// Nothing inside the linked node matches. The answer is empty rather than
/// wrong, and it carries what was found elsewhere plus the call that would
/// widen the search, so the caller is not left guessing whether the tool is
/// broken.
#[tokio::test]
async fn nothing_in_scope_reports_the_out_of_scope_matches_separately() -> anyhow::Result<()> {
    let (output, _) = search(json!({
        "url": "https://www.figma.com/design/FileKey123/Girok?node-id=3831-10708",
        "query": "Loading",
        "limit": 5
    }))
    .await?;

    // The anchor itself is in scope; its siblings are not.
    assert_eq!(ids(&output, "matches"), ["3831:10708"]);
    assert_eq!(output["scope"]["excludedOutOfScope"], 4);
    assert_eq!(
        ids(&output, "outOfScopeMatches"),
        Vec::<String>::new(),
        "out-of-scope matches are only listed when the scoped answer is empty"
    );

    let (empty, _) = search(json!({
        "url": "https://www.figma.com/design/FileKey123/Girok?node-id=1690-33290",
        "query": "Loading screen improvements",
        "limit": 5
    }))
    .await?;
    assert_eq!(empty["count"], 0);
    assert_eq!(ids(&empty, "outOfScopeMatches"), ["4279:7810"]);
    assert!(
        empty["scope"]["nextAction"]["example"]["url"]
            .as_str()
            .is_some_and(|url| !url.contains("node-id")),
        "the widening call must be a URL without a node-id: {}",
        empty["scope"]
    );
    Ok(())
}

/// The tool description is what a caller reads before deciding what the URL
/// means. It has to say that the link's `node-id` narrows the search.
#[tokio::test]
async fn the_tool_description_states_the_scope_rule() -> anyhow::Result<()> {
    let server = DevupServer::new(Services::new(
        Arc::new(Auth),
        Arc::new(TwoPageFile::default()),
    ));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let tools = client.list_all_tools().await?;
    let search = tools
        .iter()
        .find(|tool| tool.name == "devup_figma_search")
        .expect("devup_figma_search is published");
    let description = search.description.clone().unwrap_or_default();
    assert!(
        description.contains("node-id"),
        "the description must say what a node-id in the URL does: {description}"
    );
    client.cancel().await?;
    task.await??;
    Ok(())
}

/// An out-of-range `limit` is refused before any Figma call, the way
/// `devup_figma_explore` already refuses one.
#[tokio::test]
async fn an_out_of_range_limit_is_refused_before_collecting() -> anyhow::Result<()> {
    let upstream = Arc::new(TwoPageFile::default());
    let server = DevupServer::new(Services::new(Arc::new(Auth), upstream.clone()));
    let (server_transport, client_transport) = tokio::io::duplex(256 * 1024);
    let task = tokio::spawn(async move {
        server.serve(server_transport).await?.waiting().await?;
        anyhow::Ok(())
    });
    let client = ().serve(client_transport).await?;
    let refused = client
        .call_tool(
            CallToolRequestParams::new("devup_figma_search").with_arguments(
                json!({
                    "url": "https://www.figma.com/design/FileKey123/Girok",
                    "query": "Loading",
                    "limit": 101
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await;

    assert!(refused.is_err());
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    client.cancel().await?;
    task.await??;
    Ok(())
}
