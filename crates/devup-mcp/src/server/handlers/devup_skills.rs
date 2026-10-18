use super::*;

#[tool_router(router = devup_skills_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-skills.md")]
    pub(super) async fn devup_skills(
        &self,
        Parameters(input): Parameters<SkillsInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let lookup = skills::Lookup::new(
            input.project_root.as_deref(),
            self.output_policy.primary_root(),
            self.client_name(),
        );
        match input.action.as_str() {
            "status" => Ok(tool_result(skills::report(&lookup))),
            "install" => {
                let outcome = skills::install(&self.output_policy, &lookup, &input.names)
                    .await
                    .map_err(to_mcp_error)?;
                // The state after the write, from the same reader `status`
                // uses. An install that reports what it meant to do rather than
                // what is now on disk is the report that cannot be trusted.
                let mut result = outcome;
                result["state"] = skills::report(&lookup);
                Ok(tool_result(result))
            }
            other => Err(to_mcp_error(DevupError::new(
                ErrorCode::DevupInvalidInput,
                format!("action must be status or install, not {other}."),
                false,
            ))),
        }
    }
}
