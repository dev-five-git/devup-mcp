use super::*;

#[tool_router(router = devup_visual_compare_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-visual-compare.md")]
    pub(super) async fn devup_visual_compare(
        &self,
        Parameters(input): Parameters<tools::VisualCompareInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = visual_compare::compare(input, &self.output_policy, &self.artifacts)
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }
}
