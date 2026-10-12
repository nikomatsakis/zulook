# zulook

Read-only access to Zulip conversations, channels, topics, and search. Works as a CLI, an MCP server, or an ACP proxy.

Give it a Zulip URL to read messages, or browse and search from a site URL.

```
$ zulook https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test

Messages from t-lang > test:

**Alice** (2025-01-15 14:32 UTC) [test]
Has anyone looked at the new RFC?

**Bob** (2025-01-15 14:35 UTC) [test]
Yes, I left some comments on the thread.

Older: https://rust-lang.zulipchat.com/#narrow/channel/213817-t-lang/topic/test/near/12340
Newer: https://rust-lang.zulipchat.com/#narrow/channel/213817-t-lang/topic/test/near/12345
```

## Setup

You need a Zulip API key. The easiest way:

1. Log in to your Zulip instance
2. Go to **Personal settings > Account & privacy**
3. Click **Manage your API key**, enter your password
4. Click **Download .zuliprc**
5. Move it to `~/zuliprc`:
   ```
   mv ~/Downloads/.zuliprc ~/zuliprc
   ```

The config path follows [zulip-terminal](https://github.com/zulip/zulip-terminal) convention: `~/zuliprc` (no dot).

Alternatively, set environment variables:
```
export ZULIP_URL=https://your-org.zulipchat.com
export ZULIP_EMAIL=you@example.com
export ZULIP_API_KEY=your_api_key
```

## Usage

### CLI

```
zulook <url> [limit=N]
zulook sites [format=json]
zulook channels <site_url> [name words] [format=json]
zulook topics <channel_url> [name words] [format=json]
zulook search <site_or_conversation_url> <query words> [scope=all|channels|dms|public] [limit=N] [before=ID] [format=json]
```

Pass any Zulip URL — streams, topics, DMs, group DMs, with or without `/near/` anchors. Default limit is 100 messages.

```
# Stream topic
zulook https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test

# DM conversation
zulook https://rust-lang.zulipchat.com/#narrow/dm/116009,116107-dm/near/572348733

# Fewer messages
zulook https://rust-lang.zulipchat.com/#narrow/stream/213817-t-lang/topic/test limit=20
```

The output includes **Older** and **Newer** URLs for paging through history. Run `zulook sites` to discover configured instances.

Use `channels` to discover visible channels by name or description. Use `topics` to list a channel's topics in recent activity order. Both include URLs that can be passed back to `zulook`. The optional name words filter matches without regard to case.

```
zulook channels https://rust-lang.zulipchat.com compiler
zulook topics https://rust-lang.zulipchat.com/#narrow/channel/213817-t-lang async
```

Use `search` to find messages. A site URL searches your message history across subscribed channels and DMs. `scope=channels` or `scope=dms` limits that history; `scope=public` searches accessible public channel history, including channels you have not joined. A channel, topic, or DM URL limits search to that conversation.

The URL is required as the first argument. Query words follow it. `scope=public` is a special option, not a query word; use it only with a site URL. Run `zulook search` or `zulook help search` for search-specific help.

```
zulook search https://rust-lang.zulipchat.com async drop scope=public limit=20
zulook search https://rust-lang.zulipchat.com/#narrow/channel/213817-t-lang trait solver
zulook search https://rust-lang.zulipchat.com/#narrow/dm/116009,116107-dm meeting
```

Search results are newest first and include a message URL. If the response gives a `before=ID` cursor, repeat the same search with that option to get older results. `format=json` returns structured data for scripts. The default search limit is 50; the maximum is 1000.

### MCP server

```
zulook mcp
```

Runs as an MCP server over stdio. Exposes five tools:

| Tool | Input | Result |
| --- | --- | --- |
| `zulip` | A command string, including a conversation URL or `"help"` | Formatted text |
| `zulip_sites` | `{}` | Configured site URLs |
| `zulip_channels` | `site`, optional `filter` | Structured channel list |
| `zulip_topics` | `channel`, optional `filter` | Structured topic list |
| `zulip_search` | `url`, `query`, optional `scope`, `limit`, `before` | Structured search hits and `next_before` cursor |

The same tools are available through the ACP proxy.

Claude Code config (`~/.claude/settings.json`):
```json
{
  "mcpServers": {
    "zulook": {
      "command": "/path/to/zulook",
      "args": ["mcp"]
    }
  }
}
```

### ACP proxy

```
zulook acp
```

Runs as an ACP proxy over stdio, for use with an ACP conductor.

## How it works

zulook parses Zulip narrow URLs to extract the channel, topic, DM participants, and message anchor from the URL fragment. It reads channels via `GET /api/v1/streams`, topics via `GET /api/v1/users/me/{stream_id}/topics`, and messages and search results via `GET /api/v1/messages` with narrow filters. Search and navigation only return data your Zulip credentials can access.

## Building

```
cargo build --release
```

Requires Rust 2024 edition.
