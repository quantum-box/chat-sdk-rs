//! Platform-agnostic rich message (Card) abstraction.
//!
//! Unifies Slack Block Kit and Discord Embeds behind a single [`Card`] type.
//! Cards are serialized to each platform's native format when sent.
//!
//! # Example
//! ```
//! use chat_sdk::card::{Card, CardBuilder, Color};
//!
//! let card = CardBuilder::new()
//!     .title("Deploy Notification")
//!     .description("Service **api-gateway** deployed to production.")
//!     .color(Color::Good)
//!     .field("Version", "v2.3.1", true)
//!     .field("Region", "ap-northeast-1", true)
//!     .footer("Deployed by CI/CD")
//!     .build();
//!
//! // Serialize to Slack Block Kit
//! let slack_json = card.to_slack_blocks();
//!
//! // Serialize to Discord Embed
//! let discord_json = card.to_discord_embed();
//! ```

use serde::{Deserialize, Serialize};

/// A platform-agnostic rich message card.
///
/// Maps to Slack Block Kit attachments/blocks and Discord embeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    /// Card title (header).
    pub title: Option<String>,
    /// Main body text (supports platform markdown).
    pub description: Option<String>,
    /// Sidebar color.
    pub color: Option<Color>,
    /// Structured key-value fields.
    pub fields: Vec<Field>,
    /// Author information shown at the top.
    pub author: Option<Author>,
    /// Image URL displayed in the card body.
    pub image_url: Option<String>,
    /// Small thumbnail image URL.
    pub thumbnail_url: Option<String>,
    /// Footer text at the bottom.
    pub footer: Option<String>,
    /// Footer icon URL.
    pub footer_icon_url: Option<String>,
}

/// A key-value field within a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    /// Field label.
    pub name: String,
    /// Field value text.
    pub value: String,
    /// Whether this field should be displayed inline (side-by-side).
    pub inline: bool,
}

/// Author block shown at the top of a card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    /// Author display name.
    pub name: String,
    /// Optional URL the author name links to.
    pub url: Option<String>,
    /// Optional author icon URL.
    pub icon_url: Option<String>,
}

/// Semantic color for the card sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    /// Green – success / positive.
    Good,
    /// Yellow – warning / notice.
    Warning,
    /// Red – error / danger.
    Danger,
    /// Custom hex color (stored as 0xRRGGBB integer).
    Custom(u32),
}

impl Color {
    /// Return the color as a `#RRGGBB` hex string.
    pub fn to_hex(self) -> String {
        let rgb = match self {
            Color::Good => 0x2EB67D,
            Color::Warning => 0xECB22E,
            Color::Danger => 0xE01E5A,
            Color::Custom(v) => v,
        };
        format!("#{rgb:06X}")
    }

    /// Return the color as a decimal integer (used by Discord).
    pub fn to_decimal(self) -> u32 {
        match self {
            Color::Good => 0x2EB67D,
            Color::Warning => 0xECB22E,
            Color::Danger => 0xE01E5A,
            Color::Custom(v) => v,
        }
    }
}

// ── Serialization to platform-native formats ──────────────────────

impl Card {
    /// Serialize to Slack Block Kit attachment JSON.
    ///
    /// Returns a JSON value suitable for the `attachments` array
    /// in `chat.postMessage`.
    pub fn to_slack_blocks(&self) -> serde_json::Value {
        let mut attachment = serde_json::Map::new();

        // Color
        if let Some(ref color) = self.color {
            attachment.insert("color".into(), serde_json::Value::String(color.to_hex()));
        }

        let mut blocks = Vec::new();

        // Header block
        if let Some(ref title) = self.title {
            blocks.push(serde_json::json!({
                "type": "header",
                "text": {
                    "type": "plain_text",
                    "text": title,
                }
            }));
        }

        // Author context block
        if let Some(ref author) = self.author {
            let mut elements = Vec::new();
            if let Some(ref icon_url) = author.icon_url {
                elements.push(serde_json::json!({
                    "type": "image",
                    "image_url": icon_url,
                    "alt_text": &author.name,
                }));
            }
            let name_text = if let Some(ref url) = author.url {
                format!("<{}|{}>", url, author.name)
            } else {
                author.name.clone()
            };
            elements.push(serde_json::json!({
                "type": "mrkdwn",
                "text": name_text,
            }));
            blocks.push(serde_json::json!({
                "type": "context",
                "elements": elements,
            }));
        }

        // Description section
        if let Some(ref desc) = self.description {
            blocks.push(serde_json::json!({
                "type": "section",
                "text": {
                    "type": "mrkdwn",
                    "text": desc,
                }
            }));
        }

        // Thumbnail as accessory on description or standalone
        if let Some(ref thumb) = self.thumbnail_url {
            if let Some(last_section) = blocks.iter_mut().rev().find(|b| {
                b.get("type").and_then(|v| v.as_str()) == Some("section")
            }) {
                last_section["accessory"] = serde_json::json!({
                    "type": "image",
                    "image_url": thumb,
                    "alt_text": "thumbnail",
                });
            }
        }

        // Fields – group into section blocks (max 10 fields per section in Slack)
        for chunk in self.fields.chunks(10) {
            let field_values: Vec<serde_json::Value> = chunk
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "type": "mrkdwn",
                        "text": format!("*{}*\n{}", f.name, f.value),
                    })
                })
                .collect();
            blocks.push(serde_json::json!({
                "type": "section",
                "fields": field_values,
            }));
        }

        // Image block
        if let Some(ref image_url) = self.image_url {
            blocks.push(serde_json::json!({
                "type": "image",
                "image_url": image_url,
                "alt_text": "image",
            }));
        }

        // Footer context block
        if self.footer.is_some() || self.footer_icon_url.is_some() {
            let mut elements = Vec::new();
            if let Some(ref icon_url) = self.footer_icon_url {
                elements.push(serde_json::json!({
                    "type": "image",
                    "image_url": icon_url,
                    "alt_text": "footer icon",
                }));
            }
            if let Some(ref footer) = self.footer {
                elements.push(serde_json::json!({
                    "type": "mrkdwn",
                    "text": footer,
                }));
            }
            blocks.push(serde_json::json!({
                "type": "context",
                "elements": elements,
            }));
        }

        attachment.insert("blocks".into(), serde_json::Value::Array(blocks));

        serde_json::Value::Object(attachment)
    }

    /// Serialize to Discord embed JSON.
    ///
    /// Returns a JSON value suitable for the `embeds` array
    /// in Discord's Create Message endpoint.
    pub fn to_discord_embed(&self) -> serde_json::Value {
        let mut embed = serde_json::Map::new();

        if let Some(ref title) = self.title {
            embed.insert("title".into(), serde_json::Value::String(title.clone()));
        }

        if let Some(ref desc) = self.description {
            embed.insert(
                "description".into(),
                serde_json::Value::String(desc.clone()),
            );
        }

        if let Some(ref color) = self.color {
            embed.insert(
                "color".into(),
                serde_json::Value::Number(color.to_decimal().into()),
            );
        }

        if !self.fields.is_empty() {
            let fields: Vec<serde_json::Value> = self
                .fields
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "name": f.name,
                        "value": f.value,
                        "inline": f.inline,
                    })
                })
                .collect();
            embed.insert("fields".into(), serde_json::Value::Array(fields));
        }

        if let Some(ref author) = self.author {
            let mut author_obj = serde_json::Map::new();
            author_obj.insert("name".into(), serde_json::Value::String(author.name.clone()));
            if let Some(ref url) = author.url {
                author_obj.insert("url".into(), serde_json::Value::String(url.clone()));
            }
            if let Some(ref icon_url) = author.icon_url {
                author_obj.insert(
                    "icon_url".into(),
                    serde_json::Value::String(icon_url.clone()),
                );
            }
            embed.insert("author".into(), serde_json::Value::Object(author_obj));
        }

        if let Some(ref image_url) = self.image_url {
            embed.insert(
                "image".into(),
                serde_json::json!({ "url": image_url }),
            );
        }

        if let Some(ref thumb) = self.thumbnail_url {
            embed.insert(
                "thumbnail".into(),
                serde_json::json!({ "url": thumb }),
            );
        }

        if self.footer.is_some() || self.footer_icon_url.is_some() {
            let mut footer_obj = serde_json::Map::new();
            if let Some(ref text) = self.footer {
                footer_obj.insert("text".into(), serde_json::Value::String(text.clone()));
            }
            if let Some(ref icon) = self.footer_icon_url {
                footer_obj.insert("icon_url".into(), serde_json::Value::String(icon.clone()));
            }
            embed.insert("footer".into(), serde_json::Value::Object(footer_obj));
        }

        serde_json::Value::Object(embed)
    }
}

// ── Builder ───────────────────────────────────────────────────────

/// Fluent builder for constructing a [`Card`].
#[derive(Debug, Default)]
pub struct CardBuilder {
    title: Option<String>,
    description: Option<String>,
    color: Option<Color>,
    fields: Vec<Field>,
    author: Option<Author>,
    image_url: Option<String>,
    thumbnail_url: Option<String>,
    footer: Option<String>,
    footer_icon_url: Option<String>,
}

impl CardBuilder {
    /// Create a new empty card builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the card title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the card description / body text.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the sidebar color.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Add a key-value field.
    pub fn field(mut self, name: impl Into<String>, value: impl Into<String>, inline: bool) -> Self {
        self.fields.push(Field {
            name: name.into(),
            value: value.into(),
            inline,
        });
        self
    }

    /// Set the author block.
    pub fn author(mut self, name: impl Into<String>) -> Self {
        self.author = Some(Author {
            name: name.into(),
            url: None,
            icon_url: None,
        });
        self
    }

    /// Set the author block with URL and icon.
    pub fn author_full(
        mut self,
        name: impl Into<String>,
        url: Option<String>,
        icon_url: Option<String>,
    ) -> Self {
        self.author = Some(Author {
            name: name.into(),
            url,
            icon_url,
        });
        self
    }

    /// Set the main image URL.
    pub fn image(mut self, url: impl Into<String>) -> Self {
        self.image_url = Some(url.into());
        self
    }

    /// Set the thumbnail image URL.
    pub fn thumbnail(mut self, url: impl Into<String>) -> Self {
        self.thumbnail_url = Some(url.into());
        self
    }

    /// Set the footer text.
    pub fn footer(mut self, text: impl Into<String>) -> Self {
        self.footer = Some(text.into());
        self
    }

    /// Set the footer icon URL.
    pub fn footer_icon(mut self, url: impl Into<String>) -> Self {
        self.footer_icon_url = Some(url.into());
        self
    }

    /// Consume the builder and produce a [`Card`].
    pub fn build(self) -> Card {
        Card {
            title: self.title,
            description: self.description,
            color: self.color,
            fields: self.fields,
            author: self.author,
            image_url: self.image_url,
            thumbnail_url: self.thumbnail_url,
            footer: self.footer,
            footer_icon_url: self.footer_icon_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_produces_card() {
        let card = CardBuilder::new()
            .title("Test Card")
            .description("A test description")
            .color(Color::Good)
            .field("Key", "Value", true)
            .footer("footer text")
            .build();

        assert_eq!(card.title.as_deref(), Some("Test Card"));
        assert_eq!(card.description.as_deref(), Some("A test description"));
        assert_eq!(card.color, Some(Color::Good));
        assert_eq!(card.fields.len(), 1);
        assert_eq!(card.fields[0].name, "Key");
        assert_eq!(card.fields[0].value, "Value");
        assert!(card.fields[0].inline);
        assert_eq!(card.footer.as_deref(), Some("footer text"));
    }

    #[test]
    fn color_hex() {
        assert_eq!(Color::Good.to_hex(), "#2EB67D");
        assert_eq!(Color::Warning.to_hex(), "#ECB22E");
        assert_eq!(Color::Danger.to_hex(), "#E01E5A");
        assert_eq!(Color::Custom(0xFF5500).to_hex(), "#FF5500");
    }

    #[test]
    fn color_decimal() {
        assert_eq!(Color::Good.to_decimal(), 0x2EB67D);
        assert_eq!(Color::Custom(0xABCDEF).to_decimal(), 0xABCDEF);
    }

    #[test]
    fn slack_blocks_basic() {
        let card = CardBuilder::new()
            .title("Deploy")
            .description("Deployed *api*")
            .color(Color::Good)
            .field("Version", "v1.0", true)
            .field("Env", "prod", true)
            .footer("CI/CD")
            .build();

        let json = card.to_slack_blocks();
        assert_eq!(json["color"], "#2EB67D");
        let blocks = json["blocks"].as_array().unwrap();

        // header, description section, fields section, footer context
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0]["type"], "header");
        assert_eq!(blocks[0]["text"]["text"], "Deploy");
        assert_eq!(blocks[1]["type"], "section");
        assert_eq!(blocks[1]["text"]["text"], "Deployed *api*");
        assert_eq!(blocks[2]["type"], "section");
        assert_eq!(blocks[2]["fields"].as_array().unwrap().len(), 2);
        assert_eq!(blocks[3]["type"], "context");
    }

    #[test]
    fn slack_blocks_with_author() {
        let card = CardBuilder::new()
            .author_full(
                "Bot",
                Some("https://example.com".into()),
                Some("https://example.com/icon.png".into()),
            )
            .description("Hello")
            .build();

        let json = card.to_slack_blocks();
        let blocks = json["blocks"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "context"); // author context
        let elements = blocks[0]["elements"].as_array().unwrap();
        assert_eq!(elements.len(), 2); // icon + text
        assert_eq!(elements[0]["type"], "image");
    }

    #[test]
    fn slack_blocks_with_image() {
        let card = CardBuilder::new()
            .image("https://example.com/img.png")
            .build();

        let json = card.to_slack_blocks();
        let blocks = json["blocks"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "image");
        assert_eq!(blocks[0]["image_url"], "https://example.com/img.png");
    }

    #[test]
    fn slack_blocks_thumbnail_as_accessory() {
        let card = CardBuilder::new()
            .description("Body text")
            .thumbnail("https://example.com/thumb.png")
            .build();

        let json = card.to_slack_blocks();
        let blocks = json["blocks"].as_array().unwrap();
        let section = &blocks[0];
        assert_eq!(section["type"], "section");
        assert_eq!(
            section["accessory"]["image_url"],
            "https://example.com/thumb.png"
        );
    }

    #[test]
    fn discord_embed_basic() {
        let card = CardBuilder::new()
            .title("Alert")
            .description("Something happened")
            .color(Color::Danger)
            .field("Status", "500", false)
            .footer("monitoring")
            .build();

        let json = card.to_discord_embed();
        assert_eq!(json["title"], "Alert");
        assert_eq!(json["description"], "Something happened");
        assert_eq!(json["color"], Color::Danger.to_decimal());
        let fields = json["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0]["name"], "Status");
        assert_eq!(fields[0]["value"], "500");
        assert!(!fields[0]["inline"].as_bool().unwrap());
        assert_eq!(json["footer"]["text"], "monitoring");
    }

    #[test]
    fn discord_embed_with_author() {
        let card = CardBuilder::new()
            .author_full("Bot", Some("https://example.com".into()), None)
            .build();

        let json = card.to_discord_embed();
        assert_eq!(json["author"]["name"], "Bot");
        assert_eq!(json["author"]["url"], "https://example.com");
        assert!(json["author"].get("icon_url").is_none());
    }

    #[test]
    fn discord_embed_images() {
        let card = CardBuilder::new()
            .image("https://example.com/img.png")
            .thumbnail("https://example.com/thumb.png")
            .build();

        let json = card.to_discord_embed();
        assert_eq!(json["image"]["url"], "https://example.com/img.png");
        assert_eq!(json["thumbnail"]["url"], "https://example.com/thumb.png");
    }

    #[test]
    fn empty_card() {
        let card = CardBuilder::new().build();
        let slack = card.to_slack_blocks();
        assert!(slack["blocks"].as_array().unwrap().is_empty());

        let discord = card.to_discord_embed();
        assert!(discord.as_object().unwrap().is_empty());
    }

    #[test]
    fn card_serialization_roundtrip() {
        let card = CardBuilder::new()
            .title("Test")
            .color(Color::Custom(0x123456))
            .build();

        let json_str = serde_json::to_string(&card).unwrap();
        let deserialized: Card = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.title, card.title);
        assert_eq!(deserialized.color, card.color);
    }
}
