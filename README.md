# chat-sdk-rs

Rust Chat Service Abstraction Library with adapter pattern.

## Features

- **Multi-platform**: Unified trait interface for Slack, Discord, and more
- **CLI**: `chat-sdk send --platform slack --channel general "Hello"`
- **OAuth**: `chat-sdk auth slack` for interactive browser authorization
- **AI Agent ready**: Expose chat operations to AI agents via MCP Tools

## Architecture

```
chat-sdk (library)
├── ChatAdapter trait     # Core abstraction
├── model                 # Message, Channel, User, Thread, Reaction
├── oauth                 # OAuth2 authorization flow
└── error                 # Unified error types

chat-sdk-cli (binary)
├── send                  # Send messages
├── channels              # List channels
└── auth                  # OAuth flow
```

## Supported Platforms

| Platform | Status |
|----------|--------|
| Slack    | Planned |
| Discord  | Planned |
| Teams    | Future |
| LINE     | Future |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
