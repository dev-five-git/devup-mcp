use super::*;
#[tool_router(router=devup_source_sync_router,vis="pub(super)")]
impl DevupServer {
    #[doc = include_str!("../text/devup-source-sync.md")]
    #[tool(output_schema=permissive_object_output_schema())]
    pub(super) async fn devup_source_sync(
        &self,
        Parameters(input): Parameters<source_sync::SourceSyncInput>,
    ) -> Result<CallToolResult, ErrorData> {
        source_sync::run(input, &self.output_policy)
            .map(tool_result)
            .map_err(to_mcp_error)
    }
}
