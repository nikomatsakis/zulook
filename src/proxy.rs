use sacp::mcp_server::{McpConnectionTo, McpServer, McpTool};
use sacp::role::{self, Role};
use sacp::{Conductor, ConnectTo, Proxy};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::config::CredentialStore;

#[derive(Debug, Deserialize, JsonSchema)]
struct ZulipInput {
    /// The command to run. Use "help" to see available commands.
    command: String,
}

struct ZulipTool {
    store: CredentialStore,
}

impl<R: Role> McpTool<R> for ZulipTool {
    type Input = ZulipInput;
    type Output = String;

    fn name(&self) -> String {
        "zulip".to_string()
    }

    fn description(&self) -> String {
        "Read Zulip conversations. Give a Zulip URL to fetch nearby messages, or \"help\" for more options.".to_string()
    }

    async fn call_tool(
        &self,
        input: ZulipInput,
        _cx: McpConnectionTo<R>,
    ) -> Result<String, sacp::Error> {
        crate::dispatch::dispatch(&self.store, input.command.trim())
            .await
            .map_err(|e| sacp::Error::internal_error().data(e.to_string()))
    }
}

pub async fn run_mcp(store: CredentialStore) -> anyhow::Result<()> {
    let server = McpServer::<role::mcp::Client, _>::builder("zulook")
        .instructions("Read-only Zulip MCP server. Give a Zulip URL to fetch nearby messages, or \"help\" for more options.")
        .tool(ZulipTool { store })
        .build();

    ConnectTo::<role::mcp::Client>::connect_to(server, sacp_tokio::Stdio::new())
        .await
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;

    Ok(())
}

pub async fn run_acp(store: CredentialStore) -> anyhow::Result<()> {
    let server = McpServer::<Conductor, _>::builder("zulook")
        .instructions("Read-only Zulip MCP server. Give a Zulip URL to fetch nearby messages, or \"help\" for more options.")
        .tool(ZulipTool { store })
        .build();

    Proxy
        .builder()
        .with_mcp_server(server)
        .connect_to(sacp_tokio::Stdio::new())
        .await
        .map_err(|e| anyhow::anyhow!("ACP proxy error: {e}"))?;

    Ok(())
}
