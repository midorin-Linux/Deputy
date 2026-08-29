//! 添付画像の前処理。ハニーポットには悪意ある投稿が集中するため、
//! ダウンロードサイズの上限と縮小・再エンコードでメモリとAPIコストを抑える。

use std::io::Cursor;

use serenity::all::Message;
use tracing::warn;

use crate::features::honeypot::agent::ImageAttachment;

/// 画像をそのままAIへ送るサイズ上限（バイト）。これを超えたら縮小・再圧縮する。
const DOWNSCALE_THRESHOLD_BYTES: u64 = 5 * 1024 * 1024;
/// ダウンロード自体を諦めるサイズ上限（バイト）。巨大ファイルによるメモリ枯渇を防ぐ。
const MAX_DOWNLOAD_BYTES: u64 = 25 * 1024 * 1024;
/// 縮小後の画像の最大辺（ピクセル）。スパム判定に必要な解像度は高くない。
const MAX_DIMENSION: u32 = 1024;

/// メッセージの画像添付をダウンロードし、必要なら縮小して返す。
/// 個々の添付の失敗はスキップして続行する（1枚の失敗で判定自体を落とさない）。
pub async fn download_image_attachments(msg: &Message) -> Vec<ImageAttachment> {
    let mut images = Vec::new();

    for attachment in &msg.attachments {
        let Some(content_type) = &attachment.content_type else {
            continue;
        };

        if !content_type.starts_with("image/") {
            continue;
        }

        if u64::from(attachment.size) > MAX_DOWNLOAD_BYTES {
            warn!(
                attachment_id = %attachment.id,
                size = attachment.size,
                "skipping oversized image attachment"
            );
            continue;
        }

        let data = match attachment.download().await {
            Ok(data) => data,
            Err(err) => {
                warn!(
                    error = %err,
                    attachment_id = %attachment.id,
                    "failed to download image attachment"
                );
                continue;
            }
        };

        // 再圧縮に失敗した場合は元データにフォールバックする。
        let (data, content_type) = if u64::from(attachment.size) > DOWNSCALE_THRESHOLD_BYTES {
            downscale(&data).unwrap_or_else(|| {
                warn!(
                    attachment_id = %attachment.id,
                    "failed to downscale large image; using original"
                );
                (data, content_type.clone())
            })
        } else {
            (data, content_type.clone())
        };

        images.push(ImageAttachment { data, content_type });
    }

    images
}

/// 画像をデコードし、最大辺が`MAX_DIMENSION`を超える場合は縮小してJPEGで再エンコードする。
/// デコード/エンコードに失敗した場合は`None`。
fn downscale(data: &[u8]) -> Option<(Vec<u8>, String)> {
    let reader = image::ImageReader::new(Cursor::new(data))
        .with_guessed_format()
        .ok()?;

    let img = reader.decode().ok()?;

    let img = if img.width() > MAX_DIMENSION || img.height() > MAX_DIMENSION {
        img.resize(
            MAX_DIMENSION,
            MAX_DIMENSION,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };

    let mut buf = Cursor::new(Vec::new());
    // JPEGはアルファチャンネルを扱えないためRGB8へ変換してからエンコードする。
    image::DynamicImage::ImageRgb8(img.to_rgb8())
        .write_to(&mut buf, image::ImageFormat::Jpeg)
        .ok()?;

    Some((buf.into_inner(), "image/jpeg".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_of(width: u32, height: u32) -> Vec<u8> {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(width, height));
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png)
            .expect("png encoding must succeed");
        buf.into_inner()
    }

    #[test]
    fn oversized_image_is_resized_within_max_dimension() {
        let (data, content_type) =
            downscale(&png_of(MAX_DIMENSION * 2, MAX_DIMENSION)).expect("must downscale");

        assert_eq!(content_type, "image/jpeg");

        let decoded = image::load_from_memory(&data).expect("result must decode");
        assert!(decoded.width() <= MAX_DIMENSION);
        assert!(decoded.height() <= MAX_DIMENSION);
    }

    #[test]
    fn small_image_keeps_its_dimensions_but_is_reencoded() {
        let (data, content_type) = downscale(&png_of(64, 32)).expect("must re-encode");

        assert_eq!(content_type, "image/jpeg");

        let decoded = image::load_from_memory(&data).expect("result must decode");
        assert_eq!((decoded.width(), decoded.height()), (64, 32));
    }

    #[test]
    fn undecodable_data_yields_none() {
        assert!(downscale(b"not an image at all").is_none());
    }
}
