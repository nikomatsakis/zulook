use std::sync::Arc;

use sacp::mcp_server::{McpConnectionTo, McpServer, McpTool};
use sacp::role::{self, Role};
use sacp::{Conductor, ConnectTo, Proxy};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::config::CredentialStore;
use crate::navigation::{
    ChannelsOutput, SearchOutput, SearchRequest, SearchScope, SitesOutput, TopicsOutput,
    list_channels, list_sites, list_topics, search,
};

#[derive(Debug, Deserialize, JsonSchema)]
struct ZulipInput {
    /// The command to run. Use "help" to see available commands.
    command: String,
}

struct ZulipTool {
    store: Arc<CredentialStore>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SitesInput {}

struct SitesTool {
    store: Arc<CredentialStore>,
}

impl<R: Role> McpTool<R> for SitesTool {
    type Input = SitesInput;
    type Output = SitesOutput;

    fn name(&self) -> String {
        "zulip_sites".to_string()
    }

    fn description(&self) -> String {
        "List configured Zulip sites so you can choose one for browsing or search.".to_string()
    }

    async fn call_tool(
        &self,
        _input: SitesInput,
        _cx: McpConnectionTo<R>,
    ) -> Result<SitesOutput, sacp::Error> {
        Ok(list_sites(&self.store))
    }
}

impl<R: Role> McpTool<R> for ZulipTool {
    type Input = ZulipInput;
    type Output = String;

    fn name(&self) -> String {
        "zulip".to_string()
    }

    fn description(&self) -> String {
        "Read Zulip conversations by URL; also accepts channels, topics, and search commands. Use \"help\" for syntax.".to_string()
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

#[derive(Debug, Deserialize, JsonSchema)]
struct ChannelsInput {
    /// Zulip site URL, for example https://example.zulipchat.com.
    site: String,
    /// Optional case-insensitive substring of the channel name or description.
    filter: Option<String>,
}

struct ChannelsTool {
    store: Arc<CredentialStore>,
}

impl<R: Role> McpTool<R> for ChannelsTool {
    type Input = ChannelsInput;
    type Output = ChannelsOutput;

    fn name(&self) -> String {
        "zulip_channels".to_string()
    }

    fn description(&self) -> String {
        "List visible Zulip channels with IDs, names, descriptions, privacy, and URLs.".to_string()
    }

    async fn call_tool(
        &self,
        input: ChannelsInput,
        _cx: McpConnectionTo<R>,
    ) -> Result<ChannelsOutput, sacp::Error> {
        list_channels(&self.store, &input.site, input.filter.as_deref())
            .await
            .map_err(tool_error)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct TopicsInput {
    /// Zulip channel URL, for example https://example.zulipchat.com/#narrow/channel/42-general.
    channel: String,
    /// Optional case-insensitive substring of the topic name.
    filter: Option<String>,
}

struct TopicsTool {
    store: Arc<CredentialStore>,
}

impl<R: Role> McpTool<R> for TopicsTool {
    type Input = TopicsInput;
    type Output = TopicsOutput;

    fn name(&self) -> String {
        "zulip_topics".to_string()
    }

    fn description(&self) -> String {
        "List a channel's accessible topics in recent activity order, with URLs.".to_string()
    }

    async fn call_tool(
        &self,
        input: TopicsInput,
        _cx: McpConnectionTo<R>,
    ) -> Result<TopicsOutput, sacp::Error> {
        list_topics(&self.store, &input.channel, input.filter.as_deref())
            .await
            .map_err(tool_error)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchInput {
    /// Zulip site URL to search your history, or channel, topic, or DM URL to restrict results.
    url: String,
    /// Zulip full-text search query.
    query: String,
    /// For site-wide searches: all (default), channels, dms, or public.
    #[serde(default)]
    scope: SearchScope,
    /// Maximum results, from 1 to 1000. Defaults to 50.
    limit: Option<u32>,
    /// Continue to older matches using next_before from the previous response.
    before: Option<i64>,
}

struct SearchTool {
    store: Arc<CredentialStore>,
}

impl<R: Role> McpTool<R> for SearchTool {
    type Input = SearchInput;
    type Output = SearchOutput;

    fn name(&self) -> String {
        "zulip_search".to_string()
    }

    fn description(&self) -> String {
        "Search Zulip channel and DM messages. Results include message URLs and a cursor for older matches.".to_string()
    }

    async fn call_tool(
        &self,
        input: SearchInput,
        _cx: McpConnectionTo<R>,
    ) -> Result<SearchOutput, sacp::Error> {
        search(
            &self.store,
            SearchRequest {
                url: input.url,
                query: input.query,
                scope: input.scope,
                limit: input.limit.unwrap_or(50),
                before: input.before,
            },
        )
        .await
        .map_err(tool_error)
    }
}

fn tool_error(error: anyhow::Error) -> sacp::Error {
    sacp::Error::internal_error().data(error.to_string())
}

pub async fn run_mcp(store: CredentialStore) -> anyhow::Result<()> {
    let store = Arc::new(store);
    let server = McpServer::<role::mcp::Client, _>::builder("zulook")
        .instructions("Read-only Zulip MCP server. Use zulip_sites to find configured instances, zulip_channels to discover channels, zulip_topics to browse topics, zulip_search to search messages, and zulip to read a conversation URL.")
        .tool(ZulipTool { store: store.clone() })
        .tool(SitesTool { store: store.clone() })
        .tool(ChannelsTool { store: store.clone() })
        .tool(TopicsTool { store: store.clone() })
        .tool(SearchTool { store })
        .build();

    ConnectTo::<role::mcp::Client>::connect_to(server, sacp_tokio::Stdio::new())
        .await
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;

    Ok(())
}

pub async fn run_acp(store: CredentialStore) -> anyhow::Result<()> {
    let store = Arc::new(store);
    let server = McpServer::<Conductor, _>::builder("zulook")
        .instructions("Read-only Zulip MCP server. Use zulip_sites to find configured instances, zulip_channels to discover channels, zulip_topics to browse topics, zulip_search to search messages, and zulip to read a conversation URL.")
        .tool(ZulipTool { store: store.clone() })
        .tool(SitesTool { store: store.clone() })
        .tool(ChannelsTool { store: store.clone() })
        .tool(TopicsTool { store: store.clone() })
        .tool(SearchTool { store })
        .build();

    Proxy
        .builder()
        .with_mcp_server(server)
        .connect_to(sacp_tokio::Stdio::new())
        .await
        .map_err(|e| anyhow::anyhow!("ACP proxy error: {e}"))?;

    Ok(())
}
