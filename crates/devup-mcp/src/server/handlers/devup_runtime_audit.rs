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
        let completion_root = input
            .completion
            .as_ref()
            .map(|completion| completion.project_root.clone());
        let mut result = match input.action.as_str() {
            "plan" => {
                let mut plan = input
                    .plan
                    .ok_or_else(|| invalid("action plan needs plan."))?;
                if let Some(root) = input.project_root.as_deref() {
                    let (digest, _) = super::completion::source_digest(
                        std::path::Path::new(root),
                        &self.output_policy,
                    )
                    .map_err(to_mcp_error)?;
                    if plan
                        .source_digest
                        .as_ref()
                        .is_some_and(|supplied| *supplied != digest)
                    {
                        return Err(invalid(
                            "Supplied sourceDigest differs from current project sources.",
                        ));
                    }
                    plan.source_digest = Some(digest);
                    plan.project_root = Some(root.to_owned());
                }
                runtime_audit::create_plan(
                    plan,
                    &self.output_policy,
                    input.output_path.as_deref(),
                    input.template_path.as_deref(),
                    &self.artifacts,
                )
                .await
            }
            "completion" => {
                let completion = input
                    .completion
                    .ok_or_else(|| invalid("action completion requires completion inputs."))?;
                super::completion::write(
                    &completion,
                    &self.output_policy,
                    &self.artifacts,
                    input.output_path.as_deref(),
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
            _ => return Err(invalid("action must be plan, ingest or completion.")),
        }
        .map_err(to_mcp_error)?;
        // Only the completion receipt can say the application is finished.
        let complete = skills::marks_completion("devup_runtime_audit", &input.action, &result);
        self.offer_retro(&mut result, completion_root.as_deref(), complete);
        Ok(tool_result(result))
    }
}
