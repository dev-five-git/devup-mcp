mod tools;

use rmcp::{
    ServerHandler,
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
};

pub use tools::{AuthInput, FigmaToJsonInput, FigmaToUiInput};

#[derive(Clone)]
pub struct DevupServer {
    tool_router: ToolRouter<Self>,
}

impl Default for DevupServer {
    fn default() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl DevupServer {
    #[tool(description = "Authenticate devup-mcp with Figma Remote MCP")]
    async fn devup_figma_auth(
        &self,
        rmcp::handler::server::wrapper::Parameters(input): rmcp::handler::server::wrapper::Parameters<
            AuthInput,
        >,
    ) -> String {
        format!("Figma auth action '{}' is not connected yet", input.action)
    }

    #[tool(description = "Convert a Figma design link to DevupUI TypeScript")]
    async fn devup_figma_to_ui(
        &self,
        rmcp::handler::server::wrapper::Parameters(input): rmcp::handler::server::wrapper::Parameters<
            FigmaToUiInput,
        >,
    ) -> String {
        format!(
            "Figma UI conversion for '{}' is not connected yet",
            input.url
        )
    }

    #[tool(description = "Convert Figma variables to devup.json")]
    async fn devup_figma_to_json(
        &self,
        rmcp::handler::server::wrapper::Parameters(input): rmcp::handler::server::wrapper::Parameters<
            FigmaToJsonInput,
        >,
    ) -> String {
        format!(
            "Figma theme conversion for '{}' is not connected yet",
            input.url
        )
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for DevupServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Read Figma designs and generate DevupUI artifacts")
    }
}
