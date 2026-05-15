# Changelog

All notable changes to this project will be documented in this file.

This repository is distributed from GitHub. crates.io publishing is not part of
the current OSS release plan.

## [0.1.0] - 2026-05-16

Initial public release candidate for `chat-sdk-rs`.

### Added

- Core `ChatAdapter` trait for sending, reading, editing, reacting to, and
  threading chat messages through a common async interface.
- Type-safe chat models for messages, channels, users, threads, reactions, and
  send-message requests.
- Slack adapter support for posting messages, reading channel history, listing
  channels, reactions, thread replies, message edits, and card attachments.
- Discord adapter support for posting messages, reading channel history, listing
  guild text channels, reactions, thread-compatible message reads, message
  edits, and embeds.
- CLI binary (`chat-sdk`) for sending messages, listing channels, and running
  OAuth authentication flows against Slack or Discord.
- OAuth helpers for authorization URL generation, callback handling, token
  exchange, and local token storage.
- Webhook infrastructure for Slack Events API and Discord Interactions,
  including signature verification helpers and axum router/server utilities.
- Slash-command parsing and routing helpers.
- Platform-neutral rich cards with Slack Block Kit and Discord embed rendering.
- Platform-neutral message formatting for Slack mrkdwn and Discord Markdown.
- Event routing helpers for message, mention, reaction, update, and delete
  events.
- Streaming message helper based on the post-then-edit pattern.
- In-memory state adapter and optional Redis-backed state adapter.
- Dual MIT OR Apache-2.0 licensing, CI, security audit, cargo-deny policy, and
  example projects for Slack and Discord.
