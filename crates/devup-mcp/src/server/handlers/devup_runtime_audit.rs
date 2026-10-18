use super::*;

#[tool_router(router = devup_runtime_audit_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-runtime-audit.md")]
    pub(super) async fn devup_runtime_audit(
        &self,
        Parameters(input): Parameters<runtime_audit::RuntimeAuditInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let invalid = |message: &str| {
            to_mcp_error(DevupError::new(
                ErrorCode::DevupInvalidInput,
                message,
                false,
            ))
        };
        let result = match input.action.as_str() {
            "plan" => {
                let plan = input
                    .plan
                    .ok_or_else(|| invalid("action plan needs plan."))?;
                runtime_audit::create_plan(
                    plan,
                    &self.output_policy,
                    input.output_path.as_deref(),
                    input.template_path.as_deref(),
                    &self.artifacts,
                )
                .await
            }
            "ingest" => {
                let (Some(plan_path), Some(report_path)) = (input.plan_path, input.report_path)
                else {
                    return Err(invalid("action ingest needs planPath and reportPath."));
                };
                runtime_audit::ingest_with_artifacts(
                    &self.output_policy,
                    &plan_path,
                    &report_path,
                    &self.artifacts,
                )
                .await
            }
            _ => return Err(invalid("action must be plan or ingest.")),
        }
        .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }
}
