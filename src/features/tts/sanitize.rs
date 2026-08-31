//! 読み上げ前の本文整形。`regex`クレートは依存に無いため手書きのパーサで済ませる。

/// 投稿本文を読み上げ用に整形する。整形後に空文字になった場合は`None`。
///
/// - コードブロック（```〜```）・インラインコード（`〜`）の中身は除去する
/// - URL（http(s)://から次の空白まで）は「URL」に置換する
/// - ユーザー/ロール/チャンネルメンションは除去する
/// - カスタム絵文字はその名前に置換する
/// - 連続する空白・改行は単一のスペースへ畳み、前後をトリムする
/// - `max_chars`（char単位）を超える分は切り捨てる
pub fn sanitize(content: &str, max_chars: usize) -> Option<String> {
    let s = strip_delimited(content, "```");
    let s = strip_delimited(&s, "`");
    let s = replace_urls(&s);
    let s = replace_mentions_and_emoji(&s);
    let s = collapse_whitespace(&s);

    if s.is_empty() {
        return None;
    }

    Some(s.chars().take(max_chars).collect())
}

/// `delim`で囲まれた区間（区切り文字自体も含む）を丸ごと除去する。
/// 閉じられていない場合は、開始位置以降を丸ごと捨てる。
fn strip_delimited(input: &str, delim: &str) -> String {
    let mut out = String::new();
    let mut rest = input;

    while let Some(start) = rest.find(delim) {
        out.push_str(&rest[.. start]);
        let after_open = &rest[start + delim.len() ..];

        match after_open.find(delim) {
            Some(end) => rest = &after_open[end + delim.len() ..],
            None => return out,
        }
    }

    out.push_str(rest);
    out
}

fn replace_urls(input: &str) -> String {
    let mut out = String::new();
    let mut rest = input;

    loop {
        let candidates = [rest.find("http://"), rest.find("https://")];
        let start = candidates.into_iter().flatten().min();

        let Some(start) = start else {
            out.push_str(rest);
            break;
        };

        out.push_str(&rest[.. start]);
        out.push_str("URL");

        let after = &rest[start ..];
        let end = after.find(char::is_whitespace).unwrap_or(after.len());
        rest = &after[end ..];
    }

    out
}

/// メンション（`<@id>` `<@!id>` `<@&id>` `<#id>`）を除去し、
/// カスタム絵文字（`<:name:id>` `<a:name:id>`）はその名前へ置換する。
/// どちらでもない`<...>`はそのまま残す。
fn replace_mentions_and_emoji(input: &str) -> String {
    let mut out = String::new();
    let mut rest = input;

    while let Some(start) = rest.find('<') {
        out.push_str(&rest[.. start]);
        let after = &rest[start + 1 ..];

        match after.find('>') {
            Some(end) => {
                let token = &after[.. end];

                if let Some(name) = custom_emoji_name(token) {
                    out.push_str(name);
                } else if !is_mention(token) {
                    out.push('<');
                    out.push_str(token);
                    out.push('>');
                }

                rest = &after[end + 1 ..];
            }
            None => {
                out.push('<');
                rest = after;
            }
        }
    }

    out.push_str(rest);
    out
}

fn is_mention(token: &str) -> bool {
    match token.strip_prefix('@') {
        Some(rest) => {
            let rest = rest
                .strip_prefix('!')
                .or_else(|| rest.strip_prefix('&'))
                .unwrap_or(rest);
            !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
        }
        None => match token.strip_prefix('#') {
            Some(rest) => !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()),
            None => false,
        },
    }
}

/// `<:name:id>`（token: `:name:id`）または`<a:name:id>`（token: `a:name:id`）から`name`を取り出す。
fn custom_emoji_name(token: &str) -> Option<&str> {
    let body = token
        .strip_prefix("a:")
        .or_else(|| token.strip_prefix(':'))?;
    let (name, id) = body.split_once(':')?;

    if !name.is_empty() && !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
        Some(name)
    } else {
        None
    }
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(sanitize("hello world", 100).as_deref(), Some("hello world"));
    }

    #[test]
    fn code_block_content_is_removed() {
        assert_eq!(
            sanitize("before ```let x = 1;``` after", 100).as_deref(),
            Some("before after")
        );
    }

    #[test]
    fn unterminated_code_block_drops_the_rest() {
        assert_eq!(sanitize("before ```oops", 100).as_deref(), Some("before"));
    }

    #[test]
    fn inline_code_content_is_removed() {
        assert_eq!(
            sanitize("run `cargo test` now", 100).as_deref(),
            Some("run now")
        );
    }

    #[test]
    fn url_is_replaced_with_placeholder() {
        assert_eq!(
            sanitize("see https://example.com/path?q=1 for details", 100).as_deref(),
            Some("see URL for details")
        );
    }

    #[test]
    fn plain_http_url_is_also_replaced() {
        assert_eq!(
            sanitize("http://example.com end", 100).as_deref(),
            Some("URL end")
        );
    }

    #[test]
    fn user_mention_is_removed() {
        assert_eq!(
            sanitize("hi <@123> there", 100).as_deref(),
            Some("hi there")
        );
    }

    #[test]
    fn nickname_user_mention_is_removed() {
        assert_eq!(
            sanitize("hi <@!123> there", 100).as_deref(),
            Some("hi there")
        );
    }

    #[test]
    fn role_mention_is_removed() {
        assert_eq!(
            sanitize("hi <@&123> there", 100).as_deref(),
            Some("hi there")
        );
    }

    #[test]
    fn channel_mention_is_removed() {
        assert_eq!(
            sanitize("see <#123> there", 100).as_deref(),
            Some("see there")
        );
    }

    #[test]
    fn custom_emoji_is_replaced_with_its_name() {
        assert_eq!(
            sanitize("nice <:pog:123456> emote", 100).as_deref(),
            Some("nice pog emote")
        );
    }

    #[test]
    fn animated_custom_emoji_is_replaced_with_its_name() {
        assert_eq!(
            sanitize("nice <a:pog:123456> emote", 100).as_deref(),
            Some("nice pog emote")
        );
    }

    #[test]
    fn unrecognized_angle_bracket_token_is_kept() {
        assert_eq!(sanitize("a <b> c", 100).as_deref(), Some("a <b> c"));
    }

    #[test]
    fn consecutive_whitespace_and_newlines_collapse_to_one_space() {
        assert_eq!(
            sanitize("a\n\n  b\t\tc   d", 100).as_deref(),
            Some("a b c d")
        );
    }

    #[test]
    fn leading_and_trailing_whitespace_is_trimmed() {
        assert_eq!(sanitize("   hello   ", 100).as_deref(), Some("hello"));
    }

    #[test]
    fn empty_after_sanitizing_yields_none() {
        assert_eq!(sanitize("```only code```", 100), None);
        assert_eq!(sanitize("<@123>", 100), None);
        assert_eq!(sanitize("   \n\t  ", 100), None);
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        // 日本語はUTF-8で1文字が複数バイトになるため、バイト単位で切ると文字化けする。
        let result = sanitize("こんにちは世界", 5).unwrap();
        assert_eq!(result, "こんにちは");
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn combined_rules_apply_together() {
        let input = "```code``` hi <@1> see https://x.test <:e:2> `inline`  done";
        assert_eq!(sanitize(input, 100).as_deref(), Some("hi see URL e done"));
    }
}
