use super::*;

#[tool_router(router = devup_stack_diff_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-stack-diff.md")]
    pub(super) async fn devup_stack_diff(
        &self,
        Parameters(input): Parameters<StackDiffInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut result = stack_diff::run(input.project_root.as_deref(), &input.layers)
            .await
            .map_err(to_mcp_error)?;
        let complete = skills::marks_completion("devup_stack_diff", "", &result);
        self.offer_retro(&mut result, input.project_root.as_deref(), complete);
        Ok(tool_result(result))
    }
}
