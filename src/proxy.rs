use std::sync::Arc;

use sacp::mcp_server::{McpConnectionTo, McpServer, McpTool};
use sacp::role::{self, Role};
use sacp::{Conductor, ConnectTo, Proxy};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::config::CredentialStore;
use crate::navigation::{
    ChannelsOutput, DmsOutput, DmsRequest, RecentOutput, RecentRequest, RecentScope, SearchOutput,
    SearchRequest, SearchScope, SitesOutput, TopicsOutput, list_channels, list_dms, list_sites,
    list_topics, recent, search,
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
        "Read Zulip conversations by URL; also accepts channels, topics, dms, recent, and search commands. Use \"help\" for syntax.".to_string()
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

#[derive(Debug, Deserialize, JsonSchema)]
struct RecentInput {
    /// Zulip site URL for your combined history, or a channel, topic, or DM URL. Starts with newest messages even if the URL has a near anchor.
    url: String,
    /// For site-wide scans: all (default), channels, or dms. Omit for a conversation URL.
    #[serde(default)]
    scope: RecentScope,
    /// Return only messages you have not read. Reading with this tool does not mark them read.
    #[serde(default)]
    unread_only: bool,
    /// Return only messages that mention you, including group and wildcard mentions.
    #[serde(default)]
    mentions_only: bool,
    /// Maximum messages, from 1 to 1000. Defaults to 50.
    limit: Option<u32>,
    /// Continue to older messages using next_before, keeping the same filters.
    before: Option<i64>,
}

struct RecentTool {
    store: Arc<CredentialStore>,
}

impl<R: Role> McpTool<R> for RecentTool {
    type Input = RecentInput;
    type Output = RecentOutput;

    fn name(&self) -> String {
        "zulip_recent".to_string()
    }

    fn description(&self) -> String {
        "Scan recent Zulip messages without search keywords, newest first. Use scope=dms for direct messages, unread_only for unread messages, or mentions_only for mentions of you; both filters can be combined. Returns content, senders, locations, timestamps, personal flags, message URLs, and an older-message cursor. Does not mark messages read.".to_string()
    }

    async fn call_tool(
        &self,
        input: RecentInput,
        _cx: McpConnectionTo<R>,
    ) -> Result<RecentOutput, sacp::Error> {
        recent(
            &self.store,
            RecentRequest {
                url: input.url,
                scope: input.scope,
                unread_only: input.unread_only,
                mentions_only: input.mentions_only,
                limit: input.limit.unwrap_or(50),
                before: input.before,
            },
        )
        .await
        .map_err(tool_error)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DmsInput {
    /// Zulip site URL, for example https://example.zulipchat.com.
    site: String,
    /// Optional case-insensitive substring of a participant's name.
    filter: Option<String>,
    /// Maximum conversations, from 1 to 1000. Defaults to 50.
    limit: Option<u32>,
    /// Continue within the recent subset using next_before, keeping the same filter.
    before: Option<i64>,
}

struct DmsTool {
    store: Arc<CredentialStore>,
}

impl<R: Role> McpTool<R> for DmsTool {
    type Input = DmsInput;
    type Output = DmsOutput;

    fn name(&self) -> String {
        "zulip_dms".to_string()
    }

    fn description(&self) -> String {
        "Discover recent one-to-one and group DM conversations, newest activity first, with participant IDs, names, latest message IDs, and conversation URLs. Zulip returns a recent subset, not a complete history; use zulip_recent with scope=dms to scan older messages. Pass a returned URL to zulip or zulip_recent to read that conversation.".to_string()
    }

    async fn call_tool(
        &self,
        input: DmsInput,
        _cx: McpConnectionTo<R>,
    ) -> Result<DmsOutput, sacp::Error> {
        list_dms(
            &self.store,
            DmsRequest {
                site: input.site,
                filter: input.filter,
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

const SERVER_INSTRUCTIONS: &str = "Read-only Zulip MCP server. Use zulip_sites to find configured instances, zulip_recent to scan recent messages without keywords (scope=dms for DMs, unread_only or mentions_only to triage), zulip_dms to discover recent DM conversations, zulip_channels to discover channels, zulip_topics to browse topics, zulip_search to search messages, and zulip to read a conversation URL. Reading does not mark messages read. Recent DM discovery is a subset; use zulip_recent with scope=dms and next_before to scan older DM history.";

pub async fn run_mcp(store: CredentialStore) -> anyhow::Result<()> {
    let store = Arc::new(store);
    let server = McpServer::<role::mcp::Client, _>::builder("zulook")
        .instructions(SERVER_INSTRUCTIONS)
        .tool(ZulipTool {
            store: store.clone(),
        })
        .tool(SitesTool {
            store: store.clone(),
        })
        .tool(ChannelsTool {
            store: store.clone(),
        })
        .tool(TopicsTool {
            store: store.clone(),
        })
        .tool(SearchTool {
            store: store.clone(),
        })
        .tool(RecentTool {
            store: store.clone(),
        })
        .tool(DmsTool { store })
        .build();

    ConnectTo::<role::mcp::Client>::connect_to(server, sacp_tokio::Stdio::new())
        .await
        .map_err(|e| anyhow::anyhow!("MCP server error: {e}"))?;

    Ok(())
}

pub async fn run_acp(store: CredentialStore) -> anyhow::Result<()> {
    let store = Arc::new(store);
    let server = McpServer::<Conductor, _>::builder("zulook")
        .instructions(SERVER_INSTRUCTIONS)
        .tool(ZulipTool {
            store: store.clone(),
        })
        .tool(SitesTool {
            store: store.clone(),
        })
        .tool(ChannelsTool {
            store: store.clone(),
        })
        .tool(TopicsTool {
            store: store.clone(),
        })
        .tool(SearchTool {
            store: store.clone(),
        })
        .tool(RecentTool {
            store: store.clone(),
        })
        .tool(DmsTool { store })
        .build();

    Proxy
        .builder()
        .with_mcp_server(server)
        .connect_to(sacp_tokio::Stdio::new())
        .await
        .map_err(|e| anyhow::anyhow!("ACP proxy error: {e}"))?;

    Ok(())
}
