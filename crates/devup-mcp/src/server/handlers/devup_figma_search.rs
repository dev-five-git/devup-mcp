use super::*;

#[tool_router(router = devup_figma_search_router, vis = "pub(super)")]
impl DevupServer {


    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-figma-search.md")]
    pub(super) async fn devup_figma_search(
        &self,
        Parameters(input): Parameters<FigmaSearchInput>,
    ) -> Result<CallToolResult, ErrorData> {
        // Refused here rather than after collecting, the way the explore tool
        // already refuses one: the projection below asks for the ceiling when
        // the search is scoped, and would otherwise swallow the rejection that
        // an out-of-range limit is owed.
        if !(1..=MATCH_CEILING).contains(&input.limit) {
            return Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupFigmaResponseTooLarge,
                "The search limit must be between 1 and 100 inclusive.",
                false,
            )));
        }
        let target = self
            .bind_to_bridge(
                parse_link(input.url.as_deref()).map_err(to_mcp_error)?,
                Unnamed::File,
                &json!({ "tool": "devup_figma_search", "arguments": { "query": input.query } }),
            )
            .await
            .map_err(to_mcp_error)?;
        let scope = SearchScope {
            node_id: target.node_id.clone(),
            limit: input.limit,
            file_url: target.link(None),
        };
        // Scope the upstream read itself to the linked subtree. Keep the ranked
        // projection above the caller's limit so ancestry filtering happens
        // before the response is cut.
        let collection_scope = if scope.node_id.is_some() {
            CollectionScope::Node
        } else {
            CollectionScope::File
        };
        let projected_limit = if scope.node_id.is_some() {
            MATCH_CEILING
        } else {
            input.limit
        };
        let mut request = CollectionRequest::new(target, collection_scope);
        request.search = Some(SearchReadOptions {
            query: input.query.clone(),
            node_types: input.node_types.clone(),
            match_kind: input.match_kind.clone(),
            limit: projected_limit,
        });
        let result = self
            .start_operation_scoped(
                PendingOperation::Search {
                    query: input.query,
                    node_types: input.node_types,
                    match_kind: input.match_kind,
                    limit: projected_limit,
                },
                request,
                false,
                Some(&scope),
            )
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }
}
