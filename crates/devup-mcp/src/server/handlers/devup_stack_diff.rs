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
        let result = stack_diff::run(input.project_root.as_deref(), &input.layers)
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }
}
