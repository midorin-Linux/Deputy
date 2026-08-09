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
