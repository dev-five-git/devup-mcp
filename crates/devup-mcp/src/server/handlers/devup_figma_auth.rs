use super::*;

#[tool_router(router = devup_figma_auth_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(
        output_schema = permissive_object_output_schema()
    )]
    #[doc = include_str!("../text/devup-figma-auth.md")]
    pub(super) async fn devup_figma_auth(
        &self,
        Parameters(input): Parameters<AuthInput>,
    ) -> Result<CallToolResult, ErrorData> {
        match input.action.as_str() {
            "status" => {}
            "doctor" => {
                let status = self.services.auth.status().await.map_err(to_mcp_error)?;
                let direct = self
                    .services
                    .auth
                    .direct_path_snapshot()
                    .await
                    .map_err(to_mcp_error)?;
                let bridge = self.services.upstream.bridge_path_snapshot().await;
                return Ok(tool_result(
                    diagnostics::doctor_report(status, direct, bridge).await,
                ));
            }
            "configure" => {
                let client_id = input.client_id.ok_or_else(|| {
                    to_mcp_error(DevupError::new(
                        ErrorCode::DevupInvalidInput,
                        "configure requires clientId.",
                        false,
                    ))
                })?;
                self.services
                    .auth
                    .configure_client_credentials(client_id, input.client_secret)
                    .await
                    .map_err(to_mcp_error)?;
                return Ok(tool_result(json!({ "status": "configured" })));
            }
            "login" => {
                // Warned rather than refused: the bridge serves every read,
                // but the direct path re-reads what the plugin answers short,
                // and whoever asks may want that. But reaching for the metered
                // path while the free one is attached is the mistake this
                // answer exists to catch, so it says so.
                let bridge_attached = self
                    .services
                    .upstream
                    .bridge_path_snapshot()
                    .await
                    .is_some_and(|bridge| bridge.available());
                self.services.auth.login().await.map_err(to_mcp_error)?;
                let mut report = self.connection_report().await.map_err(to_mcp_error)?;
                if bridge_attached {
                    report["warning"] =
                        json!(include_str!("../text/mod-devup_figma_auth-444c97f4.md"));
                }
                return Ok(tool_result(report));
            }
            "logout" => {
                self.services.auth.logout().await.map_err(to_mcp_error)?;
            }
            _ => {
                return Err(to_mcp_error(DevupError::new(
                    ErrorCode::DevupAuthRequired,
                    "action must be status, login, logout, configure, or doctor.",
                    false,
                )));
            }
        }
        Ok(tool_result(
            self.connection_report().await.map_err(to_mcp_error)?,
        ))
    }
}
