# Contributing

Thanks for taking the time to improve `chat-sdk-rs`.

This project is in its first public OSS release cycle. Small, focused changes
are easiest to review: bug fixes, documentation improvements, examples, tests,
and narrowly scoped adapter improvements are all welcome.

## Development Setup

Requirements:

- Rust 1.88.0 or later
- A recent nightly toolchain for `rustfmt` checks

Clone and verify the workspace:

```bash
git clone https://github.com/quantum-box/chat-sdk-rs.git
cd chat-sdk-rs
cargo build --all-features
cargo test --all-features
```

The repository is consumed from GitHub. Do not add crates.io publish steps or
crates.io installation instructions unless that release policy changes.

## Before Opening a PR

Run the checks that match CI:

```bash
cargo +nightly fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo test --no-default-features
cargo check --all-features
```

If your change touches examples, also run:

```bash
cargo check --manifest-path examples/slack_minimal/Cargo.toml
cargo check --manifest-path examples/discord_minimal/Cargo.toml
```

## Secrets

Never commit bot tokens, client secrets, signing secrets, webhook payloads with
real credentials, or private channel/user identifiers. Examples and tests should
use environment variables and placeholder values.

Before submitting, run a quick scan:

```bash
rg -n "xox[baprs]-|Bot [A-Za-z0-9._-]+|client_secret|signing_secret|WEBHOOK_SECRET|TOKEN|SECRET|PASSWORD|PRIVATE_KEY"
```

Expected matches should be documentation, environment variable names, or test
placeholders only.

## Adding Platform Support

When adding or extending an adapter:

- Keep the public API platform-neutral where practical.
- Add or update feature flags in `crates/chat-sdk/Cargo.toml`.
- Map platform-specific errors into `ChatError`.
- Add focused tests for parsing, formatting, and error mapping.
- Update README, CHANGELOG, and examples when behavior changes.

## Code of Conduct

Participation in this project is covered by the [Code of Conduct](CODE_OF_CONDUCT.md).
