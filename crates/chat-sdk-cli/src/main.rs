use anyhow::Result;
use clap::{Parser, Subcommand};

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
            tracing::info!(
                platform = %platform,
                channel = %channel,
                thread = ?thread,
                "Sending message"
            );
            println!("TODO: Send to {platform}#{channel}: {message}");
        }
        Commands::Channels { platform } => {
            tracing::info!(platform = %platform, "Listing channels");
            println!("TODO: List channels for {platform}");
        }
        Commands::Auth { platform } => {
            tracing::info!(platform = %platform, "Starting OAuth flow");
            println!("TODO: OAuth flow for {platform}");
        }
    }

    Ok(())
}
