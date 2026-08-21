use serenity::all::{Colour, CreateEmbed, CreateEmbedFooter, Timestamp};

/// 通常のログ用Embed（青系）。
pub fn log_embed(title: impl Into<String>, description: impl Into<String>) -> CreateEmbed {
    base_embed(title, description, Colour::BLURPLE)
}

/// 警告用Embed（新規垢など、注意喚起したいログ向け）。
pub fn warn_embed(title: impl Into<String>, description: impl Into<String>) -> CreateEmbed {
    base_embed(title, description, Colour::ORANGE)
}

fn base_embed(
    title: impl Into<String>,
    description: impl Into<String>,
    colour: Colour,
) -> CreateEmbed {
    CreateEmbed::new()
        .title(title)
        .description(description)
        .colour(colour)
        .timestamp(Timestamp::now())
        .footer(CreateEmbedFooter::new("Deputy"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(embed: &CreateEmbed, key: &str) -> serde_json::Value {
        serde_json::to_value(embed).unwrap()[key].clone()
    }

    #[test]
    fn log_embed_uses_blurple_and_given_text() {
        let embed = log_embed("title", "desc");
        assert_eq!(field(&embed, "title"), "title");
        assert_eq!(field(&embed, "description"), "desc");
        assert_eq!(field(&embed, "color"), Colour::BLURPLE.0);
    }

    #[test]
    fn warn_embed_uses_orange_and_given_text() {
        let embed = warn_embed("title", "desc");
        assert_eq!(field(&embed, "title"), "title");
        assert_eq!(field(&embed, "description"), "desc");
        assert_eq!(field(&embed, "color"), Colour::ORANGE.0);
    }

    #[test]
    fn log_and_warn_embeds_use_different_colours() {
        assert_ne!(
            field(&log_embed("t", "d"), "color"),
            field(&warn_embed("t", "d"), "color")
        );
    }
}
