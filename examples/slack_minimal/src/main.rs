use chat_sdk::{ChatAdapter, SendMessage, SlackAdapter};

#[tokio::main]
async fn main() -> chat_sdk::ChatResult<()> {
    let token = std::env::var("CHAT_SDK_TOKEN")
        .map_err(|_| chat_sdk::ChatError::Other("CHAT_SDK_TOKEN is required".into()))?;
    let channel = std::env::var("CHAT_SDK_CHANNEL")
        .map_err(|_| chat_sdk::ChatError::Other("CHAT_SDK_CHANNEL is required".into()))?;

    let adapter = SlackAdapter::new(token);
    let message_id = adapter
        .send_message(SendMessage::text(
            channel,
            "Hello from chat-sdk-rs Slack example.",
        ))
        .await?;

    println!("sent Slack message: {}", message_id.0);
    Ok(())
}
