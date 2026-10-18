use super::*;

#[tool_router(router = devup_screen_plan_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-screen-plan.md")]
    pub(super) async fn devup_screen_plan(
        &self,
        Parameters(input): Parameters<screen_plan::ScreenPlanInput>,
    ) -> Result<CallToolResult, ErrorData> {
        let result = match input.action.as_str() {
            "draft" => {
                let (screens, source) = self.plan_screens(&input).await.map_err(to_mcp_error)?;
                let mut assets = input.assets.clone();
                assets.extend(
                    source["assetCandidates"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|asset| serde_json::from_value(asset.clone()).ok()),
                );
                screen_plan::draft_with(&input, &screens, &assets, source, &self.output_policy)
                    .await
            }
            "confirm" => screen_plan::confirm(&input, &self.output_policy),
            "assume" => screen_plan::assume(&input, &self.output_policy),
            "status" => screen_plan::status(&input, &self.output_policy, &self.artifacts).await,
            "auditPlan" => {
                screen_plan::audit_plan(&input, &self.output_policy, &self.artifacts).await
            }
            "defer" => screen_plan::defer(&input, &self.output_policy),
            other => Err(DevupError::new(
                ErrorCode::DevupInvalidInput,
                format!(
                    "action must be draft, confirm, status, assume, auditPlan or defer, not {other}."
                ),
                false,
            )),
        }
        .map_err(to_mcp_error)?;
        Ok(tool_result(result))
    }
}
