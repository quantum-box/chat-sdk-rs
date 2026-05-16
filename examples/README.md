# Examples

The example projects are intentionally small and use environment variables for
all credentials. Do not put bot tokens, client secrets, or signing secrets in
source files.

## Slack Minimal

```bash
export CHAT_SDK_TOKEN="<slack-bot-token>"
export CHAT_SDK_CHANNEL="C0123456789"
cargo run --manifest-path examples/slack_minimal/Cargo.toml
```

The token must have permission to post to the selected channel, for example
`chat:write`.

## Discord Minimal

```bash
export CHAT_SDK_TOKEN="..."
export CHAT_SDK_CHANNEL="123456789012345678"
cargo run --manifest-path examples/discord_minimal/Cargo.toml
```

The channel must be a Discord text channel ID that the bot can send messages to.
