# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Discover configured Zulip sites, list accessible channels, and list a channel's topics from the CLI and MCP/ACP tools.
- Search channel and DM messages with conversation filters, pagination, and structured results.
- Scan recent messages without keywords, with DM/channel scopes, unread and mention filters, personal flags, and pagination through `zulip_recent` and the `recent` CLI command.
- Discover recent one-to-one and group DM conversations with participant names, conversation URLs, filtering, and pagination through `zulip_dms` and the `dms` CLI command.

## [1.0.2](https://github.com/nikomatsakis/zulook/compare/v1.0.1...v1.0.2) - 2026-05-31

### Other

- add recent work
- fmt
- release-plz token
