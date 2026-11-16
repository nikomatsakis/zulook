# zulook

Read-only access to Zulip conversations, recent messages, DM discovery, channels, topics, and search. Works as a CLI, an MCP server, or an ACP proxy.

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
zulook dms <site_url> [name words] [limit=N] [before=ID] [format=json]
zulook recent <site_or_conversation_url> [scope=all|channels|dms] [unread_only=true] [mentions_only=true] [limit=N] [before=ID] [format=json]
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

Use `dms` to discover recent one-to-one and group DM conversations without knowing participant IDs or URLs. Results include participant names and IDs, the latest message ID, and a conversation URL. Optional name words filter participant names without regard to case.

Zulip supplies a recent subset of DM conversations, not a complete history. Paging through `dms` only covers this subset. Use `recent` with `scope=dms` to scan older DM messages.

Use `recent` to scan messages without search keywords. A site URL scans your combined message history; `scope=dms` or `scope=channels` narrows it. A channel, topic, or DM URL restricts the scan to that conversation; omit `scope` in that case. Recent scans start with the newest messages even when the URL has a `/near/` anchor.

Set `unread_only=true` for unread messages or `mentions_only=true` for messages that mention you, including group and wildcard mentions. Combine them to fetch unread mentions. Reading messages does not mark them read.

```
# Discover DM conversations, optionally by participant name
zulook dms https://rust-lang.zulipchat.com
zulook dms https://rust-lang.zulipchat.com Alice format=json

# Scan your recent DMs or unread mentions
zulook recent https://rust-lang.zulipchat.com scope=dms limit=20
zulook recent https://rust-lang.zulipchat.com unread_only=true mentions_only=true

# Continue toward older messages with the cursor from the previous response
zulook recent https://rust-lang.zulipchat.com scope=dms before=572348733 format=json
```

Recent messages are newest first and include content, sender, location, timestamp, personal message flags, and message URL. Both commands default to a limit of 50, with a maximum of 1000. If the response gives a `before=ID` cursor (`next_before` in JSON), repeat the same command and filters with `before=ID` to fetch the next page. A null `next_before` means the current result set is exhausted.

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

Runs as an MCP server over stdio. Exposes seven tools:

| Tool | Input | Result |
| --- | --- | --- |
| `zulip` | A command string, including a conversation URL or `"help"` | Formatted text |
| `zulip_sites` | `{}` | Configured site URLs |
| `zulip_channels` | `site`, optional `filter` | Structured channel list |
| `zulip_topics` | `channel`, optional `filter` | Structured topic list |
| `zulip_dms` | `site`, optional `filter`, `limit`, `before` | Recent DM conversations with participants, URLs, and `next_before` cursor |
| `zulip_recent` | `url`, optional `scope`, `unread_only`, `mentions_only`, `limit`, `before` | Recent messages with personal flags and `next_before` cursor |
| `zulip_search` | `url`, `query`, optional `scope`, `limit`, `before` | Structured search hits and `next_before` cursor |

For an agent workflow such as "scan my unread DMs", discover the instance with `zulip_sites`, then call `zulip_recent` with:

```json
{
  "url": "https://rust-lang.zulipchat.com",
  "scope": "dms",
  "unread_only": true,
  "limit": 50
}
```

Use `next_before` as `before` in the next call, keeping the same filters. To browse by person, call `zulip_dms` with a site and optional participant-name `filter`, then pass a conversation URL to `zulip` or `zulip_recent`. DM discovery results include a note explaining their limited historical coverage.

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

DM discovery fetches a snapshot of recent conversations and user names via `POST /api/v1/register`, subscribes to no events, and immediately removes the temporary event queue via `DELETE /api/v1/events`. It does not change conversations or message read state.

## Building

```
cargo build --release
```

Requires Rust 2024 edition.
