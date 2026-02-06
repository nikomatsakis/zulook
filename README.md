# zulook

Read-only access to Zulip conversations. Works as a CLI, an MCP server, or an ACP proxy.

Give it a Zulip URL, get back the messages. That's it.

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

The output includes **Older** and **Newer** URLs for paging through history.

### MCP server

```
zulook mcp
```

Runs as an MCP server over stdio. Exposes a single `zulip` tool — give it a URL or `"help"`.

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

zulook parses Zulip narrow URLs to extract the stream/channel, topic, DM participants, and message anchor from the URL fragment. It then calls the Zulip REST API (`GET /api/v1/messages`) with the corresponding narrow filters and returns formatted messages.

The URL is the only interface. There's one tool, one command pattern, and pagination is just following URLs — the same ones you'd see in your browser.

## Building

```
cargo build --release
```

Requires Rust 2024 edition.
