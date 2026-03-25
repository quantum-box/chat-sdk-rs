use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use chat_sdk::model::SendMessage;
use chat_sdk::{ChatAdapter, MessageId};

#[derive(Parser)]
#[command(
    name = "chat-sdk",
    version,
    about = "Chat SDK CLI - manage messages across platforms"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Send a message to a channel
    Send {
        /// Target platform (slack, discord)
        #[arg(short, long)]
        platform: String,
        /// Channel name or ID
        #[arg(short, long)]
        channel: String,
        /// Message text
        message: String,
        /// Reply to a thread (message ID)
        #[arg(short, long)]
        thread: Option<String>,
    },
    /// List channels
    Channels {
        /// Target platform
        #[arg(short, long)]
        platform: String,
    },
    /// Authenticate with a platform via OAuth
    Auth {
        /// Target platform
        platform: String,
    },
}

fn build_adapter(platform: &str) -> Result<Box<dyn ChatAdapter>> {
    let token = std::env::var("CHAT_SDK_TOKEN")
        .map_err(|_| anyhow::anyhow!("CHAT_SDK_TOKEN environment variable is required"))?;

    match platform {
        "slack" => Ok(Box::new(chat_sdk::SlackAdapter::new(token))),
        "discord" => Ok(Box::new(chat_sdk::DiscordAdapter::new(token))),
        other => bail!("unsupported platform: {other} (supported: slack, discord)"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Send {
            platform,
            channel,
            message,
            thread,
        } => {
            let adapter = build_adapter(&platform)?;
            tracing::info!(
                platform = %platform,
                channel = %channel,
                thread = ?thread,
                "Sending message"
            );
            let mut msg = SendMessage::text(channel, message);
            msg.thread_id = thread.map(MessageId);
            let id = adapter.send_message(msg).await?;
            println!("{}", id.0);
        }
        Commands::Channels { platform } => {
            let adapter = build_adapter(&platform)?;
            tracing::info!(platform = %platform, "Listing channels");
            let channels = adapter.list_channels().await?;
            let json = serde_json::to_string_pretty(&channels)?;
            println!("{json}");
        }
        Commands::Auth { platform } => {
            tracing::info!(platform = %platform, "Starting OAuth flow");
            let (auth_url, token_url, scopes) = match platform.as_str() {
                "slack" => (
                    "https://slack.com/oauth/v2/authorize",
                    "https://slack.com/api/oauth.v2.access",
                    vec![
                        "channels:read",
                        "chat:write",
                        "reactions:read",
                        "reactions:write",
                    ],
                ),
                "discord" => (
                    "https://discord.com/api/oauth2/authorize",
                    "https://discord.com/api/oauth2/token",
                    vec!["bot", "identify"],
                ),
                other => bail!("unsupported platform: {other} (supported: slack, discord)"),
            };

            let client_id = std::env::var("CHAT_SDK_CLIENT_ID")
                .map_err(|_| anyhow::anyhow!("CHAT_SDK_CLIENT_ID env var is required"))?;
            let client_secret = std::env::var("CHAT_SDK_CLIENT_SECRET")
                .map_err(|_| anyhow::anyhow!("CHAT_SDK_CLIENT_SECRET env var is required"))?;
            let redirect_url = std::env::var("CHAT_SDK_REDIRECT_URL")
                .unwrap_or_else(|_| "http://localhost:8080/callback".to_string());

            let config = chat_sdk::OAuthConfig {
                client_id,
                client_secret,
                auth_url: auth_url.to_string(),
                token_url: token_url.to_string(),
                redirect_url,
                scopes: scopes.into_iter().map(String::from).collect(),
            };

            let auth = config.authorize_url()?;
            println!("Opening browser for authorization...");
            println!("{}", auth.url);
            let _ = open::that(&auth.url);

            println!("Waiting for callback...");
            let params = config.wait_for_callback(&auth.csrf_token).await?;

            println!("Exchanging authorization code for token...");
            let mut token = config.exchange_code(&params.code).await?;
            token.platform = platform.clone();

            let store = chat_sdk::TokenStore::new()?;
            store.save(&token)?;
            println!("Token saved for {platform}.");
        }
    }

    Ok(())
}
