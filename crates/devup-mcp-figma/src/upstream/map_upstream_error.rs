

fn map_upstream_error<E: std::fmt::Display>(
    context: UpstreamFailureContext,
    error: E,
) -> DevupError {
    upstream_failure_error(context, None, &error.to_string())
}

fn upstream_timeout_error(context: UpstreamFailureContext) -> DevupError {
    let phase = match context {
        UpstreamFailureContext::Connect => "connect",
        UpstreamFailureContext::ListTools => "listTools",
        UpstreamFailureContext::CallTool => "callTool",
        UpstreamFailureContext::RegisterClient => "registerClient",
        UpstreamFailureContext::Decode => "decode",
    };
    let mut error = UpstreamFailureKind::Transport.into_devup_error(None);
    error.details = json!({
        "source": "direct",
        "status": null,
        "reason": "timeout",
        "phase": phase,
    });
    error
}

impl BuiltinScript {
    /// 플러그인이 아는 이름.
    ///
    /// 코드젠이 파일명에서 만든 것과 같아야 한다(`fast_snapshot.js` →
    /// `fastSnapshot`). 어긋나면 플러그인이 `DEVUP_BRIDGE_UNKNOWN_SCRIPT` 를
    /// 돌려주며 아는 이름을 함께 알려 준다.
    pub fn plugin_name(self) -> &'static str {
        match self {
            Self::MeasureNodes => "measureNodes",
            Self::BridgeContext => "bridgeContext",
            Self::NodeSnapshot => "snapshot",
            // 두 변형이 같은 원본을 쓴다. 나뉘는 곳은 넘기는 rootIds 뿐이다.
            Self::FastSnapshotEnvelope | Self::MultiRootSnapshotEnvelope => "fastSnapshot",
            Self::FastThemeEnvelope => "fastTheme",
            Self::PageCatalog => "pageCatalog",
            Self::SearchSnapshot => "search",
            Self::VariableCatalog => "variableCatalog",
            Self::LocalVariables => "variables",
            Self::UsedResources => "usedResources",
            Self::ExploreSnapshot => "explore",
            Self::SectionIndex => "sectionIndex",
            Self::LargeValue => "largeValue",
            Self::AssetExport => "assets",
        }
    }
}

impl ReadToolCall {
    /// An asset rendition or a fragment of one, rather than a document field.
    pub fn is_asset_read(&self) -> bool {
        matches!(self, Self::AssetExport { .. })
            || matches!(self, Self::LargeValue { options, .. } if options.field.starts_with("$export:"))
    }
    /// 이 읽기가 향하는 파일.
    pub fn file_key(&self) -> &str {
        match self {
            Self::Metadata { file_key, .. }
            | Self::VariableDefs { file_key, .. }
            | Self::DesignContext { file_key, .. }
            | Self::CodeConnectMap { file_key, .. }
            | Self::Screenshot { file_key, .. }
            | Self::Snapshot { file_key, .. }
            | Self::SearchSnapshot { file_key, .. }
            | Self::PageCatalog { file_key }
            | Self::ExploreSnapshot { file_key, .. }
            | Self::FastTheme { file_key, .. }
            | Self::LargeValue { file_key, .. }
            | Self::AssetExport { file_key, .. } => file_key,
        }
    }

    /// 브리지가 대신할 수 있는 읽기면 스크립트 이름과 값을, 아니면 `None`.
    ///
    /// 값이 놓이는 자리는 [`BuiltinScript::source`] 의 치환 자리와 1:1 이다. 한쪽만
    /// 고치면 브리지 경로에서만 빈 값이 들어가 조용히 다른 결과가 나온다.
    ///
    /// 공식 도구 이름으로 가는 읽기는 디코더가 두 경로를 같은 타입으로 환원할 때만
    /// 브리지가 맡는다. 그렇지 않은 것은 `None` 으로 두어 원격이 처리하게 한다.
    pub fn bridge_job(&self) -> Option<crate::bridge::BridgeJob> {
        use crate::bridge::BridgeJob;

        let job = match self {
            // `metadata.js` 의 JSON 과 공식 경로의 XML 은 `find_metadata` 에서
            // 같은 `MetadataDocument` 로 환원된다.
            Self::Metadata {
                node_id: Some(node_id),
                ..
            } => BridgeJob {
                script: "metadata",
                params: json!({ "nodeId": node_id }),
            },

            // 노드 없는 읽기는 최상위 페이지 목록이다. 플러그인의 페이지 목록을 브리지
            // 클라이언트가 공식 `get_metadata` 와 같은 글로 감싼다.
            Self::Metadata { node_id: None, .. } => BridgeJob {
                script: BuiltinScript::PageCatalog.plugin_name(),
                params: json!({}),
            },

            // 공식 `get_screenshot` 대신 플러그인이 노드를 PNG 로 export 한다.
            // 브리지 클라이언트가 그 답을 공식 도구와 같은 이미지 블록으로 감싼다.
            Self::Screenshot { node_id, .. } => BridgeJob {
                script: "referencePng",
                params: json!({ "nodeId": node_id }),
            },

            Self::VariableDefs { .. }
            | Self::DesignContext { .. }
            | Self::CodeConnectMap { .. } => {
                return None;
            }

            Self::Snapshot {
                node_id,
                script,
                resources,
                snapshot,
                root_ids,
                ..
            } => BridgeJob {
                script: script.plugin_name(),
                params: json!({
                    "nodeId": node_id,
                    // `source` 와 같은 기본값. 단일 루트 읽기는 대상 노드 하나다.
                    "rootIds": root_ids.clone()
                        .unwrap_or_else(|| vec![node_id.clone()]),
                    "snapshot": snapshot.clone().unwrap_or_default(),
                    // `source` 의 `empty_resources` 와 같은 빈 값. 스크립트는 두
                    // 배열이 있다고 전제하므로 없는 채로 두면 거기서 죽는다.
                    "resources": resources.clone().unwrap_or(ResourceBatch {
                        variable_ids: Vec::new(),
                        styles: Vec::new(),
                    }),
                }),
            },

            Self::SearchSnapshot {
                node_id, options, ..
            } => BridgeJob {
                script: BuiltinScript::SearchSnapshot.plugin_name(),
                params: json!({ "nodeId": node_id, "search": options }),
            },

            Self::PageCatalog { .. } => BridgeJob {
                script: BuiltinScript::PageCatalog.plugin_name(),
                params: json!({}),
            },

            Self::ExploreSnapshot {
                node_id, options, ..
            } => BridgeJob {
                script: BuiltinScript::ExploreSnapshot.plugin_name(),
                params: json!({ "nodeId": node_id, "explore": options }),
            },

            Self::FastTheme { offset, .. } => BridgeJob {
                script: BuiltinScript::FastThemeEnvelope.plugin_name(),
                params: json!({ "theme": { "offset": offset } }),
            },

            Self::LargeValue { options, .. } => {
                let mut params = json!({ "nodeId": options.node_id, "largeValue": options });
                if self.is_asset_read() {
                    params["largeValue"]["timeoutMs"] =
                        json!(crate::asset_timeout().as_millis() as u64);
                }
                BridgeJob {
                    script: BuiltinScript::LargeValue.plugin_name(),
                    params,
                }
            }

            Self::AssetExport {
                version, request, ..
            } => BridgeJob {
                script: BuiltinScript::AssetExport.plugin_name(),
                params: json!({
                    "nodeId": request.node_id,
                    "asset": {
                        "transport": "bridge",
                        "assetId": request.asset_id,
                    "renderMode": request.render_mode,
                    "usageAspectRatio":request.usage_aspect_ratio,"maxWidth":request.max_width,"srcset":request.srcset,"visibleScopeNodeId":request.visible_scope_node_id,
                        "nodeId": request.node_id,
                        "field": request.field,
                        "imageHash": request.image_hash,
                        "format": request.format,
                        "quality": request.quality,
                        "lossless": request.lossless,
                        "scale": request.scale,
                        "timeoutMs": crate::asset_timeout().as_millis() as u64,
                        "version": version,
                    },
                }),
            },
        };
        Some(job)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rmcp::{
        ErrorData, RoleServer, ServerHandler, ServiceExt,
        model::{
            CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
            PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
        },
        service::RequestContext,
    };

    use crate::ErrorCode;

    use super::*;

    #[derive(Clone)]
    struct CountingToolServer {
        list_calls: Arc<AtomicUsize>,
        tool_calls: Arc<AtomicUsize>,
    }

    #[derive(Clone)]
    struct StalledToolServer {
        stall_list: bool,
        stall_call: bool,
        fail_call: bool,
    }

    impl ServerHandler for StalledToolServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, ErrorData> {
            if self.stall_list {
                std::future::pending().await
            }
            Ok(ListToolsResult {
                tools: vec![Tool::new("get_metadata", "read metadata", Map::new())],
                ..ListToolsResult::default()
            })
        }

        async fn call_tool(
            &self,
            _request: CallToolRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> Result<CallToolResponse, ErrorData> {
            if self.stall_call {
                std::future::pending().await
            }
            if self.fail_call {
                return Err(ErrorData::new(
                    rmcp::model::ErrorCode::INTERNAL_ERROR,
                    "test failure",
                    None,
                ));
            }
            Ok(CallToolResult::success(vec![ContentBlock::text("ok")]).into())
        }
    }

    impl ServerHandler for CountingToolServer {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        }

        async fn list_tools(
            &self,
            _request: Option<PaginatedRequestParams>,
            _context: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, ErrorData> {
            self.list_calls.fetch_add(1, Ordering::SeqCst);
            Ok(ListToolsResult {
                tools: vec![
                    Tool::new("get_metadata", "read metadata", Map::new()),
                    Tool::new("delete_node", "write-like server tool", Map::new()),
                ],
                ..ListToolsResult::default()
            })
        }

        async fn call_tool(
            &self,
            _request: CallToolRequestParams,
            _context: RequestContext<RoleServer>,
        ) -> Result<CallToolResponse, ErrorData> {
            self.tool_calls.fetch_add(1, Ordering::SeqCst);
            Ok(CallToolResult::success(vec![ContentBlock::text("ok")]).into())
        }
    }

    async fn connect_counting_server(
        connections: Arc<AtomicUsize>,
        list_calls: Arc<AtomicUsize>,
        tool_calls: Arc<AtomicUsize>,
    ) -> Result<RunningFigmaClient, DevupError> {
        connections.fetch_add(1, Ordering::SeqCst);
        let (server_transport, client_transport) = tokio::io::duplex(4_096);
        tokio::spawn(async move {
            let server = CountingToolServer {
                list_calls,
                tool_calls,
            }
            .serve(server_transport)
            .await
            .expect("test MCP server starts");
            let _ = server.waiting().await;
        });
        ().serve(client_transport)
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::Connect, error))
    }

    async fn connect_stalled_server(
        connections: Arc<AtomicUsize>,
        stall_list: bool,
        stall_call: bool,
        fail_call: bool,
    ) -> Result<RunningFigmaClient, DevupError> {
        connections.fetch_add(1, Ordering::SeqCst);
        let (server_transport, client_transport) = tokio::io::duplex(4_096);
        tokio::spawn(async move {
            let server = StalledToolServer {
                stall_list,
                stall_call,
                fail_call,
            }
            .serve(server_transport)
            .await
            .expect("test MCP server starts");
            let _ = server.waiting().await;
        });
        ().serve(client_transport)
            .await
            .map_err(|error| map_upstream_error(UpstreamFailureContext::Connect, error))
    }

    #[tokio::test]
    async fn bounded_session_reuses_one_connection_and_one_filtered_capability_catalog() {
        let connections = Arc::new(AtomicUsize::new(0));
        let list_calls = Arc::new(AtomicUsize::new(0));
        let tool_calls = Arc::new(AtomicUsize::new(0));
        let cache = RemoteSessionCache::new(Duration::from_secs(30));

        let connect = || {
            let connections = connections.clone();
            let list_calls = list_calls.clone();
            let tool_calls = tool_calls.clone();
            connect_counting_server(connections, list_calls, tool_calls)
        };

        let first = cache.get_or_connect(connect).await.unwrap();
        let second = cache.get_or_connect(connect).await.unwrap();
        assert!(Arc::ptr_eq(&first.client, &second.client));
        assert_eq!(
            first
                .capabilities
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["get_metadata"]
        );
        assert!(!first.capabilities.contains("delete_node"));
        for session in [&first, &second] {
            session
                .client
                .call_tool(CallToolRequestParams::new("get_metadata").with_arguments(Map::new()))
                .await
                .unwrap();
        }

        assert_eq!(connections.load(Ordering::SeqCst), 1);
        assert_eq!(list_calls.load(Ordering::SeqCst), 1);
        assert_eq!(tool_calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn expired_session_reconnects_and_reverifies_capabilities() {
        let connections = Arc::new(AtomicUsize::new(0));
        let list_calls = Arc::new(AtomicUsize::new(0));
        let tool_calls = Arc::new(AtomicUsize::new(0));
        let cache = RemoteSessionCache::new(Duration::ZERO);

        let connect =
            || connect_counting_server(connections.clone(), list_calls.clone(), tool_calls.clone());
        let first = cache.get_or_connect(connect).await.unwrap();
        let second = cache.get_or_connect(connect).await.unwrap();

        assert!(!Arc::ptr_eq(&first.client, &second.client));
        assert_eq!(connections.load(Ordering::SeqCst), 2);
        assert_eq!(list_calls.load(Ordering::SeqCst), 2);
        assert!(!second.capabilities.contains("delete_node"));
    }

    #[tokio::test]
    async fn stalled_capability_catalog_returns_a_bounded_safe_timeout() {
        let connections = Arc::new(AtomicUsize::new(0));
        let cache = RemoteSessionCache::with_timeouts(
            Duration::from_secs(30),
            Duration::from_millis(100),
            Duration::from_millis(10),
        );

        let result = tokio::time::timeout(
            Duration::from_millis(250),
            cache
                .get_or_connect(|| connect_stalled_server(connections.clone(), true, false, false)),
        )
        .await
        .expect("product deadline must complete before the test guard");
        let Err(result) = result else {
            panic!("stalled capability catalog must time out")
        };

        assert_eq!(result.code, ErrorCode::DevupFigmaDirectUnavailable);
        assert!(result.retryable);
        assert_eq!(result.details["reason"], "timeout");
        assert_eq!(result.details["phase"], "listTools");
    }

    #[tokio::test]
    async fn stalled_connection_returns_a_bounded_safe_timeout() {
        let cache = RemoteSessionCache::with_timeouts(
            Duration::from_secs(30),
            Duration::from_millis(10),
            Duration::from_millis(100),
        );

        let result = tokio::time::timeout(
            Duration::from_millis(250),
            cache.get_or_connect(|| {
                std::future::pending::<Result<RunningFigmaClient, DevupError>>()
            }),
        )
        .await
        .expect("product deadline must complete before the test guard");
        let Err(result) = result else {
            panic!("stalled connection must time out")
        };

        assert_eq!(result.code, ErrorCode::DevupFigmaDirectUnavailable);
        assert!(result.retryable);
        assert_eq!(result.details["reason"], "timeout");
        assert_eq!(result.details["phase"], "connect");
    }

    #[tokio::test]
    async fn stalled_tool_call_times_out_and_evicts_its_remote_session() {
        let connections = Arc::new(AtomicUsize::new(0));
        let cache = RemoteSessionCache::with_timeouts(
            Duration::from_secs(30),
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
        let session = cache
            .get_or_connect(|| connect_stalled_server(connections.clone(), false, true, false))
            .await
            .expect("initial session");
        let call = ReadToolCall::metadata("FileKey123", Some("1:2"));

        let error = tokio::time::timeout(
            Duration::from_millis(250),
            cache.call_tool(&session, &call, Duration::from_millis(10)),
        )
        .await
        .expect("product deadline must complete before the test guard")
        .expect_err("stalled tool call must time out");
        assert_eq!(error.code, ErrorCode::DevupFigmaDirectUnavailable);
        assert!(error.retryable);
        assert_eq!(error.details["reason"], "timeout");
        assert_eq!(error.details["phase"], "callTool");

        let replacement = cache
            .get_or_connect(|| connect_stalled_server(connections.clone(), false, false, false))
            .await
            .expect("replacement session");
        assert!(!Arc::ptr_eq(&session.client, &replacement.client));
        assert_eq!(connections.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn ordinary_tool_error_evicts_its_remote_session() {
        let connections = Arc::new(AtomicUsize::new(0));
        let cache = RemoteSessionCache::with_timeouts(
            Duration::from_secs(30),
            Duration::from_millis(100),
            Duration::from_millis(100),
        );
        let session = cache
            .get_or_connect(|| connect_stalled_server(connections.clone(), false, false, true))
            .await
            .expect("initial session");
        let call = ReadToolCall::metadata("FileKey123", Some("1:2"));

        let error = cache
            .call_tool(&session, &call, Duration::from_millis(100))
            .await
            .expect_err("ordinary remote tool error must be classified");
        assert_eq!(error.code, ErrorCode::DevupFigmaDirectUnavailable);
        assert!(error.retryable);

        let replacement = cache
            .get_or_connect(|| connect_stalled_server(connections.clone(), false, false, false))
            .await
            .expect("replacement session");
        assert!(!Arc::ptr_eq(&session.client, &replacement.client));
        assert_eq!(connections.load(Ordering::SeqCst), 2);
    }
}
