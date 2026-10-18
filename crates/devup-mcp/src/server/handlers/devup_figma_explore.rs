use super::*;

#[tool_router(router = devup_figma_explore_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-figma-explore.md")]
    pub(super) async fn devup_figma_explore(
        &self,
        Parameters(input): Parameters<FigmaExploreInput>,
    ) -> Result<CallToolResult, ErrorData> {
        self.explore(input)
            .await
            .map(tool_result)
            .map_err(to_mcp_error)
    }
}
