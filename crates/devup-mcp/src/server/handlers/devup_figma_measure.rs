use super::*;

#[tool_router(router = devup_figma_measure_router, vis = "pub(super)")]
impl DevupServer {

    #[tool(
        output_schema = permissive_object_output_schema())]
    #[doc = include_str!("../text/devup-figma-measure.md")]
    pub(super) async fn devup_figma_measure(
        &self,
        Parameters(input): Parameters<tools::FigmaMeasureInput>,
    ) -> Result<CallToolResult, ErrorData> {
        measurement::run(self, input)
            .await
            .map(tool_result)
            .map_err(to_mcp_error)
    }
}
