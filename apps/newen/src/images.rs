//! Pasted images and the thumbnails of attached ones. Disk and decoding run on
//! threads; the window only takes their results on a later frame.

use eframe::egui;
use limn::{RasterImage, RasterImageFormat, Size, SketchMark};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Where pasted images are saved: `attachments/` beside quill's database.
fn attachments_dir() -> PathBuf {
    newen_engine::Config::default_database()
        .database
        .and_then(|db| db.parent().map(|dir| dir.join("attachments")))
        .unwrap_or_else(|| std::env::temp_dir().join("quill-attachments"))
}

/// Saves a pasted picture (RGBA, as egui-winit read it off the clipboard) as PNG on a thread
/// and gives its path to `pasted`.
pub fn paste(
    pasted: Arc<Mutex<Vec<PathBuf>>>,
    ctx: egui::Context,
    (width, height, rgba): (usize, usize, Arc<[u8]>),
) {
    std::thread::spawn(move || {
        let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
            return;
        };
        let Some(rgba) = image::RgbaImage::from_raw(width, height, rgba.to_vec()) else {
            return;
        };
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis());
        let dir = attachments_dir();
        let path = dir.join(format!("pasted-{millis}.png"));
        let saved = std::fs::create_dir_all(&dir).and_then(|()| {
            rgba.save_with_format(&path, image::ImageFormat::Png)
                .map_err(std::io::Error::other)
        });
        if let Err(error) = saved {
            eprintln!("quill: the pasted image was not saved: {error}");
            return;
        }
        pasted.lock().unwrap().push(path);
        ctx.request_repaint();
    });
}

/// The thumbnail of an image as limn takes it: 256 px at most, PNG. `None` when the file does
/// not decode, and the tile keeps its file icon.
pub fn thumbnail(path: &Path) -> Option<RasterImage> {
    scaled(path, 256)
}

/// What limn decodes of a picture: 4096 px a side, 12 MP and 8 MiB of bytes at most.
const LIMN_SIDE: u32 = 4096;
const LIMN_PIXELS: u64 = 12 * 1024 * 1024;
const LIMN_CAP: usize = 8 * 1024 * 1024;

/// The picture the viewer shows, at its own size: a PNG or JPEG file within limn's limits as it
/// is, any other one decoded and, past those limits, made smaller with a Lanczos filter. Then
/// PNG, or JPEG at quality 90 when the PNG would pass the 8 MiB cap.
pub fn full(path: &Path) -> Option<RasterImage> {
    let bytes = std::fs::read(path).ok()?;
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()?;
    let format = match reader.format()? {
        image::ImageFormat::Png => RasterImageFormat::Png,
        image::ImageFormat::Jpeg => RasterImageFormat::Jpeg,
        _ => return large(path),
    };
    let (width, height) = reader.into_dimensions().ok()?;
    if within_limn(width, height) && bytes.len() <= LIMN_CAP {
        let alt = path.file_name()?.to_string_lossy().into_owned();
        return raster(format, &bytes, alt);
    }
    large(path)
}

fn within_limn(width: u32, height: u32) -> bool {
    width.max(height) <= LIMN_SIDE && u64::from(width) * u64::from(height) <= LIMN_PIXELS
}

/// A picture that does not go to limn as it is: decoded, made smaller to limn's limits with a
/// Lanczos filter (a screenshot's text stays legible), and encoded again.
fn large(path: &Path) -> Option<RasterImage> {
    let decoded = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let (width, height) = (decoded.width(), decoded.height());
    let decoded = if within_limn(width, height) {
        decoded
    } else {
        let by_side = LIMN_SIDE as f64 / f64::from(width.max(height));
        let by_pixels = (LIMN_PIXELS as f64 / (f64::from(width) * f64::from(height))).sqrt();
        let scale = by_side.min(by_pixels);
        let fit = |side: u32| ((f64::from(side) * scale).floor() as u32).max(1);
        decoded.resize_exact(
            fit(width),
            fit(height),
            image::imageops::FilterType::Lanczos3,
        )
    };
    encoded(decoded, path)
}

/// An image's size in pixels, from its header; the viewer fits it and the editor lays it out.
pub fn size(path: &Path) -> Option<(f32, f32)> {
    let (width, height) = image::image_dimensions(path).ok()?;
    (width > 0 && height > 0).then_some((width as f32, height as f32))
}

/// The annotated copy of an image: the original at full size with `marks` painted over it,
/// saved as PNG beside the pasted images as `<name>-annotated.png`, or `-annotated-2` and on
/// when that is taken. `window` is the window the editor showed the picture in, which sets the
/// scale of the marks' widths.
pub fn annotate(original: &Path, marks: &[SketchMark], window: Size) -> Option<PathBuf> {
    let source = image::open(original).ok()?.to_rgba8();
    let aspect = source.width() as f32 / source.height() as f32;
    let shown = newen_ui::annotate::shown(aspect, window).width;
    let png = annotated_png(&source, marks, shown)?;
    let dir = attachments_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let stem = original.file_stem()?.to_string_lossy().into_owned();
    let path = free_name(&dir, &format!("{stem}-annotated"));
    std::fs::write(&path, png).ok()?;
    Some(path)
}

/// `dir/<base>.png`, or the first `dir/<base>-<n>.png` from 2 that is not taken.
fn free_name(dir: &Path, base: &str) -> PathBuf {
    let mut path = dir.join(format!("{base}.png"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{base}-{n}.png"));
        n += 1;
    }
    path
}

/// The picture with `marks` painted over it, as PNG. `shown` is the width the editor drew the
/// picture at, so a mark's width scales to the picture's pixels. `None` when `shown` is not
/// a positive width or the PNG cannot be written.
pub fn annotated_png(
    source: &image::RgbaImage,
    marks: &[SketchMark],
    shown: f32,
) -> Option<Vec<u8>> {
    if !(shown.is_finite() && shown > 0.0) {
        return None;
    }
    let (width, height) = source.dimensions();
    let mut rgba = source.as_raw().clone();
    limn::rasterize_sketch(marks, width, height, &mut rgba, width as f32 / shown);
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_raw(width, height, rgba)?)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// The image at most `side` px on each side, as limn takes it.
fn scaled(path: &Path, side: u32) -> Option<RasterImage> {
    let decoded = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?
        .thumbnail(side, side);
    encoded(decoded, path)
}

/// `decoded` as limn takes it: PNG, or JPEG at quality 90 past the 8 MiB cap.
fn encoded(decoded: image::DynamicImage, path: &Path) -> Option<RasterImage> {
    let mut png = Vec::new();
    decoded
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    let (format, bytes) = if png.len() <= LIMN_CAP {
        (RasterImageFormat::Png, png)
    } else {
        let mut jpeg = std::io::Cursor::new(Vec::new());
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90);
        decoded.to_rgb8().write_with_encoder(encoder).ok()?;
        (RasterImageFormat::Jpeg, jpeg.into_inner())
    };
    raster(
        format,
        &bytes,
        path.file_name()?.to_string_lossy().into_owned(),
    )
}

/// Encoded bytes as limn takes them: with their digest, in base64.
fn raster(format: RasterImageFormat, bytes: &[u8], alt: String) -> Option<RasterImage> {
    use base64::Engine;
    use sha2::Digest;
    let digest: String = sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    RasterImage::new(
        digest,
        format,
        u32::try_from(bytes.len()).ok()?,
        data.into(),
        alt,
    )
}

/// The QR code of `link` as a PNG: dark modules of 8 px on white, inside the four-module
/// margin a camera needs.
pub fn qr(link: &str) -> Option<RasterImage> {
    const MODULE: u32 = 8;
    const MARGIN: u32 = 4;
    let code = qrcode::QrCode::new(link).ok()?;
    let (width, colors) = (code.width() as u32, code.to_colors());
    let side = (width + 2 * MARGIN) * MODULE;
    let picture = image::GrayImage::from_fn(side, side, |x, y| {
        let (x, y) = (x / MODULE, y / MODULE);
        let inside = (MARGIN..MARGIN + width).contains(&x) && (MARGIN..MARGIN + width).contains(&y);
        let dark =
            inside && colors[((y - MARGIN) * width + x - MARGIN) as usize] == qrcode::Color::Dark;
        image::Luma([if dark { 0 } else { 255 }])
    });
    let mut png = Vec::new();
    picture
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    raster(RasterImageFormat::Png, &png, "QR code of the link".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use limn::{Color, Point};

    /// eframe 0.37 made the system clipboard a feature of its own: without it egui-winit keeps
    /// a clipboard inside the window, and Ctrl+V pastes nothing copied in another program.
    #[test]
    fn the_window_reads_the_system_clipboard() {
        let manifest = include_str!("../../../Cargo.toml");
        let eframe = manifest
            .lines()
            .find(|l| l.starts_with("eframe ="))
            .unwrap();
        assert!(eframe.contains("\"clipboard\""), "{eframe}");
    }

    #[test]
    fn a_red_stroke_is_painted_into_the_saved_pixels() {
        let white = image::RgbaImage::from_pixel(100, 50, image::Rgba([255, 255, 255, 255]));
        let marks = [SketchMark::Line {
            from: Point::new(0.1, 0.5),
            to: Point::new(0.9, 0.5),
            color: Color::rgb(0xE0, 0x31, 0x31),
            width: 4.0,
        }];
        let png = annotated_png(&white, &marks, 100.0).expect("the marks are saved");
        let out = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(out.dimensions(), (100, 50));
        let [r, g, b, _] = out.get_pixel(50, 25).0;
        assert!(r > 200 && g < 100 && b < 100, "pixel {r} {g} {b}");
        assert_eq!(out.get_pixel(50, 5).0, [255, 255, 255, 255]);
    }

    #[test]
    fn a_mark_is_scaled_to_the_size_the_picture_was_shown_at() {
        let white = image::RgbaImage::from_pixel(200, 100, image::Rgba([255, 255, 255, 255]));
        let marks = [SketchMark::Line {
            from: Point::new(0.1, 0.5),
            to: Point::new(0.9, 0.5),
            color: Color::rgb(0, 0, 0),
            width: 4.0,
        }];
        // Shown at half its width, the 4 px stroke is 8 px thick on the 200 px picture.
        let png = annotated_png(&white, &marks, 100.0).unwrap();
        let out = image::load_from_memory(&png).unwrap().to_rgba8();
        assert!(out.get_pixel(100, 46).0[0] < 100);
        assert!(out.get_pixel(100, 53).0[0] < 100);
        assert_eq!(out.get_pixel(100, 40).0, [255, 255, 255, 255]);
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let dir = std::env::temp_dir().join(format!("quill-free-name-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(free_name(&dir, "a-annotated"), dir.join("a-annotated.png"));
        std::fs::write(dir.join("a-annotated.png"), b"").unwrap();
        assert_eq!(
            free_name(&dir, "a-annotated"),
            dir.join("a-annotated-2.png")
        );
        std::fs::write(dir.join("a-annotated-2.png"), b"").unwrap();
        assert_eq!(
            free_name(&dir, "a-annotated"),
            dir.join("a-annotated-3.png")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The pixels of what limn would be handed.
    fn pixels(picture: &RasterImage) -> (u32, u32) {
        use base64::Engine;
        use image::GenericImageView;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(picture.data_base64())
            .unwrap();
        image::load_from_memory(&bytes).unwrap().dimensions()
    }

    #[test]
    fn the_viewer_gets_a_screenshot_at_its_own_size_and_a_huge_one_within_limn() {
        let dir = std::env::temp_dir().join(format!("quill-full-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // A 2880 by 1800 screenshot goes as its own file, not its 256 px thumbnail.
        let shot = dir.join("shot.png");
        image::RgbImage::from_fn(2880, 1800, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 0])
        })
        .save(&shot)
        .unwrap();
        let whole = full(&shot).unwrap();
        assert_eq!(
            whole.size_bytes() as u64,
            std::fs::metadata(&shot).unwrap().len()
        );
        assert_eq!(pixels(&whole), (2880, 1800));
        assert_eq!(pixels(&thumbnail(&shot).unwrap()), (256, 160));
        // Past 4096 px a side it is made smaller to fit, keeping its shape.
        let huge = dir.join("huge.png");
        image::GrayImage::new(6000, 1000).save(&huge).unwrap();
        assert_eq!(pixels(&full(&huge).unwrap()), (4096, 682));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
