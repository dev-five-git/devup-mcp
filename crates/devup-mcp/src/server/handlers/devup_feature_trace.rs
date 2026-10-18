use super::*;

#[tool_router(router = devup_feature_trace_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-feature-trace.md")]
    pub(super) async fn devup_feature_trace(
        &self,
        Parameters(input): Parameters<tools::FeatureTraceInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = feature_trace::run(input, &self.artifacts)
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }
}
