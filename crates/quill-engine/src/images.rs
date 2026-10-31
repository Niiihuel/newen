//! The images a message attaches, read and sized for the agent, and the pictures a tool gives
//! back, kept as files (docs/engine.md). Runs on a blocking thread: it decodes and encodes
//! whole files.

use image::{codecs::jpeg::JpegEncoder, DynamicImage, ImageFormat, RgbImage};
use quill_agent::{PromptImage, ToolContent};
use std::path::{Path, PathBuf};

/// Desktop's limits.
const MAX_FILE: u64 = 30 * 1024 * 1024;
const MAX_IMAGES: usize = 20;
const MAX_SIDE: u32 = 2000;
const MIN_SIDE: u32 = 500;
const MAX_BYTES: usize = 500 * 1024;

pub const FORMATS: &str = "Only PNG, JPEG, GIF, and WebP images are supported.";
pub const TOO_LARGE: &str = "Some images were too large to attach. Maximum size is 30 MB.";
pub const TOO_MANY: &str = "Maximum of 20 images allowed.";
pub const FAILED: &str = "Failed to process image. You can try again.";

/// The images a prompt carries, and the notices for those left out (each once).
pub fn attach(paths: &[PathBuf]) -> (Vec<PromptImage>, Vec<&'static str>) {
    let (mut images, mut notices) = (vec![], vec![]);
    for (i, path) in paths.iter().enumerate() {
        let notice = if i >= MAX_IMAGES {
            TOO_MANY
        } else if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_FILE) {
            TOO_LARGE
        } else {
            match std::fs::read(path) {
                Err(_) => FAILED,
                Ok(bytes) => match prepare(bytes) {
                    Ok(image) => {
                        images.push(image);
                        continue;
                    }
                    Err(notice) => notice,
                },
            }
        };
        if !notices.contains(&notice) {
            notices.push(notice);
        }
    }
    (images, notices)
}

/// The pictures a tool gave back, written to `dir` in the order they came, under names of
/// quill's own (what the agent sent never names a file). One that is not base64 of a PNG,
/// JPEG, GIF or WebP, or cannot be written, is let go.
pub fn keep(dir: &Path, pictures: Vec<ToolContent>) -> Vec<PathBuf> {
    use base64::Engine;
    let mut kept = vec![];
    for picture in pictures {
        let ToolContent::Image { data, .. } = picture else {
            continue;
        };
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(data) else {
            continue;
        };
        let extension = match image::guess_format(&bytes) {
            Ok(ImageFormat::Png) => "png",
            Ok(ImageFormat::Jpeg) => "jpg",
            Ok(ImageFormat::Gif) => "gif",
            Ok(ImageFormat::WebP) => "webp",
            _ => continue,
        };
        let path = dir.join(format!("{}.{extension}", uuid::Uuid::new_v4().simple()));
        if std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(&path, &bytes))
            .is_ok()
        {
            kept.push(path);
        }
    }
    kept
}

/// The image as it goes to the agent: as it is when it fits, otherwise re-encoded.
fn prepare(bytes: Vec<u8>) -> Result<PromptImage, &'static str> {
    let (format, media_type) = match image::guess_format(&bytes) {
        Ok(ImageFormat::Png) => (ImageFormat::Png, "image/png"),
        Ok(ImageFormat::Jpeg) => (ImageFormat::Jpeg, "image/jpeg"),
        Ok(ImageFormat::Gif) => (ImageFormat::Gif, "image/gif"),
        Ok(ImageFormat::WebP) => (ImageFormat::WebP, "image/webp"),
        _ => return Err(FORMATS),
    };
    let decoded = image::load_from_memory_with_format(&bytes, format).map_err(|_| FAILED)?;
    let (width, height) = (decoded.width(), decoded.height());
    if width <= MAX_SIDE && height <= MAX_SIDE && bytes.len() <= MAX_BYTES {
        return Ok(PromptImage {
            media_type: media_type.into(),
            data: bytes,
        });
    }
    let target = MAX_BYTES.min(bytes.len());
    let data = resize_to_fit(&decoded, target).ok_or(FAILED)?;
    // A re-encode that is no smaller keeps the original, when the original fits.
    if data.len() >= bytes.len() && bytes.len() <= MAX_BYTES {
        return Ok(PromptImage {
            media_type: media_type.into(),
            data: bytes,
        });
    }
    Ok(PromptImage {
        media_type: "image/jpeg".into(),
        data,
    })
}

/// JPEG at 2000 px at most, stepping quality down and size down by ×0.75 (to 500 px) until it
/// fits `target` bytes; the last resort is 500 px at quality 70. quill uses JPEG where Desktop
/// prefers WebP: the `image` crate encodes only lossless WebP, which is larger (inferred).
fn resize_to_fit(decoded: &DynamicImage, target: usize) -> Option<Vec<u8>> {
    let longest = decoded.width().max(decoded.height());
    let mut side = longest.min(MAX_SIDE);
    loop {
        let resized;
        let scaled = if side < longest {
            resized = decoded.resize(side, side, image::imageops::FilterType::Triangle);
            &resized
        } else {
            decoded
        };
        for quality in [90, 70] {
            let out = encode(scaled, quality)?;
            if out.len() <= target || (side <= MIN_SIDE && quality == 70) {
                return Some(out);
            }
        }
        side = ((side as f32 * 0.75) as u32).max(MIN_SIDE);
    }
}

/// `image` has no alpha in JPEG, so a transparent pixel is painted on white.
fn encode(image: &DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let rgba = image.to_rgba8();
    let flat = RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [r, g, b, a] = rgba.get_pixel(x, y).0;
        let over = |c: u8| ((c as u32 * a as u32 + 255 * (255 - a as u32)) / 255) as u8;
        image::Rgb([over(r), over(g), over(b)])
    });
    let mut out = vec![];
    flat.write_with_encoder(JpegEncoder::new_with_quality(&mut out, quality))
        .ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageEncoder, Rgba, RgbaImage};

    fn png(width: u32, height: u32, noisy: bool) -> Vec<u8> {
        let img = RgbaImage::from_fn(width, height, |x, y| {
            let v = if noisy {
                ((x * 7919 + y * 104729) % 256) as u8
            } else {
                40
            };
            Rgba([v, v.wrapping_mul(3), 255 - v, 255])
        });
        let mut out = vec![];
        image::codecs::png::PngEncoder::new(&mut out)
            .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgba8)
            .unwrap();
        out
    }

    fn write(name: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("quill-images-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_small_png_goes_as_it_is() {
        let bytes = png(40, 30, false);
        let (images, notices) = attach(&[write("small.png", &bytes)]);
        assert!(notices.is_empty());
        assert_eq!(
            images,
            vec![PromptImage {
                media_type: "image/png".into(),
                data: bytes
            }]
        );
    }

    #[test]
    fn a_wide_png_is_resized_to_a_small_jpeg() {
        let (images, notices) = attach(&[write("wide.png", &png(3000, 1000, true))]);
        assert!(notices.is_empty());
        let image = &images[0];
        assert_eq!(image.media_type, "image/jpeg");
        assert!(image.data.len() <= MAX_BYTES);
        let decoded = image::load_from_memory(&image.data).unwrap();
        assert!(decoded.width() <= MAX_SIDE && decoded.height() <= MAX_SIDE);
    }

    #[test]
    fn a_text_file_is_rejected_with_the_formats_message() {
        let (images, notices) = attach(&[write("notes.txt", b"just words\n")]);
        assert!(images.is_empty());
        assert_eq!(notices, vec![FORMATS]);
    }

    #[test]
    fn a_tool_s_pictures_are_kept_as_files_and_what_is_not_one_is_let_go() {
        use base64::Engine;
        let base64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        let image = |data: String| ToolContent::Image {
            mime: "image/png".into(),
            data,
        };
        let dir = std::env::temp_dir().join(format!("quill-keep-{}", std::process::id()));
        let shot = png(4, 3, false);
        let kept = keep(
            &dir,
            vec![
                image(base64(&shot)),
                image(base64(b"not a picture")),
                image("not base64!".into()),
                ToolContent::Text("words".into()),
            ],
        );
        assert_eq!(kept.len(), 1);
        assert!(kept[0].starts_with(&dir) && kept[0].extension().unwrap() == "png");
        assert_eq!(std::fs::read(&kept[0]).unwrap(), shot);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_file_is_reported_as_failed_never_sent() {
        let (images, notices) = attach(&[PathBuf::from("/nonexistent/quill/image.png")]);
        assert!(images.is_empty());
        assert_eq!(notices, vec![FAILED]);
    }
}
