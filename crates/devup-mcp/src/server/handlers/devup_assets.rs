use super::*;

#[tool_router(router = devup_assets_router, vis = "pub(super)")]
impl DevupServer {
    #[tool(output_schema = permissive_object_output_schema())]
    #[doc = include_str!("../text/devup-assets.md")]
    pub(super) async fn devup_assets(
        &self,
        Parameters(input): Parameters<assets::AssetsInput>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(tool_result(
            assets::run(input, &self.output_policy).map_err(to_mcp_error)?,
        ))
    }
}
