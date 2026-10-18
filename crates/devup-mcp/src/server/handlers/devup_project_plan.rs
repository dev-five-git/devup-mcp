use super::*;

#[tool_router(router = devup_project_plan_router, vis = "pub(super)")]
impl DevupServer {

    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-project-plan.md")]
    pub(super) async fn devup_project_plan(
        &self,
        Parameters(input): Parameters<project_plan::ProjectPlanInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = project_plan::run(&input, &self.output_policy)
            .await
            .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }
}
