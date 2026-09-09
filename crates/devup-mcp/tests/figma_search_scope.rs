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
    if !matches!(page_id, "0:1" | "0:2") {
        let nodes = match page_id {
            "4279:7810" => vec![
                node(
                    "0:1",
                    "PAGE",
                    "Phase2 Hand-off",
                    Value::Null,
                    &["4279:7810"],
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
            ],
            "3831:10708" => vec![
                node(
                    "0:1",
                    "PAGE",
                    "Phase2 Hand-off",
                    Value::Null,
                    &["4279:7810"],
                ),
                node(
                    "4279:7810",
                    "SECTION",
                    "Loading screen improvements",
                    json!("0:1"),
                    &["3831:10708"],
                ),
                node("3831:10708", "FRAME", "Loading", json!("4279:7810"), &[]),
            ],
            "1690:33290" => vec![
                node("0:2", "PAGE", "Components", Value::Null, &["1690:33290"]),
                node("1690:33290", "FRAME", "Loading", json!("0:2"), &[]),
            ],
            _ => panic!("unexpected search root: {page_id}"),
        };
        let root_id = if page_id == "1690:33290" {
            "0:2"
        } else {
            "0:1"
        };
        return json!({"fileKey": "FileKey123", "version": null, "rootIds": [root_id], "nodes": nodes, "diagnostics": []});
    }
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
        .call_tool(
            CallToolRequestParams::new("devup_figma_search").with_arguments(arguments.clone()),
        )
        .await?;
    let output = result.structured_content.unwrap();
    let calls = upstream.calls.load(Ordering::SeqCst);
    let cached = client
        .call_tool(CallToolRequestParams::new("devup_figma_search").with_arguments(arguments))
        .await?
        .structured_content
        .unwrap();
    assert_eq!(cached["cache"]["cacheHit"], true);
    assert_eq!(cached["scope"], output["scope"]);
    assert_eq!(cached["matches"], output["matches"]);
    assert_eq!(
        cached["scope"]["collectionScope"],
        cached["cache"]["capabilities"]["collectionScope"]
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), calls);
    client.cancel().await?;
    task.await??;
    Ok((output, upstream))
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
    let (output, upstream) = search(json!({
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
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);
    assert_eq!(output["scope"]["collectionScope"], "node");
    assert_eq!(output["cache"]["capabilities"]["collectionScope"], "node");
    assert_eq!(output["scope"]["matchedInScope"], 3);
    assert_eq!(output["scope"]["excludedOutOfScope"], 0);
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
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);
    // Collect up to the projection ceiling before applying the response limit.
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
    let (output, upstream) = search(json!({
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
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 3);
    assert_eq!(output["scope"]["collectionScope"], "file");
    assert_eq!(output["cache"]["capabilities"]["collectionScope"], "file");
    Ok(())
}

/// Direct FRAME reads exclude siblings and preserve an empty scoped answer
/// without collecting matches elsewhere in the file.
#[tokio::test]
async fn frame_scope_does_not_collect_siblings_even_when_nothing_matches() -> anyhow::Result<()> {
    let (output, upstream) = search(json!({
        "url": "https://www.figma.com/design/FileKey123/Girok?node-id=3831-10708",
        "query": "Loading",
        "match": "exact",
        "limit": 5
    }))
    .await?;

    // The anchor itself is in scope; its siblings are not.
    assert_eq!(ids(&output, "matches"), ["3831:10708"]);
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);
    assert_eq!(output["scope"]["collectionScope"], "node");
    assert_eq!(output["scope"]["excludedOutOfScope"], 0);
    assert_eq!(
        ids(&output, "outOfScopeMatches"),
        Vec::<String>::new(),
        "out-of-scope matches are only listed when the scoped answer is empty"
    );

    let (empty, upstream) = search(json!({
        "url": "https://www.figma.com/design/FileKey123/Girok?node-id=1690-33290",
        "query": "Loading screen improvements",
        "limit": 5
    }))
    .await?;
    assert_eq!(empty["count"], 0);
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);
    assert_eq!(empty["scope"]["collectionScope"], "node");
    assert_eq!(empty["scope"]["matchedInScope"], 0);
    assert_eq!(empty["scope"]["excludedOutOfScope"], 0);
    assert!(ids(&empty, "outOfScopeMatches").is_empty());
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
