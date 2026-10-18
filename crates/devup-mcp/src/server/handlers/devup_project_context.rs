use super::*;

#[tool_router(router = devup_project_context_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-project-context.md")]
    pub(super) async fn devup_project_context(
        &self,
        Parameters(input): Parameters<ProjectContextInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = project_context::run(
            &input.scope,
            input.project_root.as_deref(),
            input.filter.as_deref(),
        )
        .await
        .map_err(to_mcp_error)?;
        Ok(tool_result(with_context_skill_gap(
            result,
            &input.scope,
            &skills::Lookup::new(
                input.project_root.as_deref(),
                self.output_policy.primary_root(),
                self.client_name(),
            ),
        )))
    }
}
