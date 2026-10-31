//! Pasted images and the thumbnails of attached ones. Clipboard, disk and decoding run on
//! threads; the window only takes their results on a later frame.

use eframe::egui;
use limn::{RasterImage, RasterImageFormat, Size, SketchMark};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Where pasted images are saved: `attachments/` beside quill's database.
fn attachments_dir() -> PathBuf {
    quill_engine::Config::default_database()
        .database
        .and_then(|db| db.parent().map(|dir| dir.join("attachments")))
        .unwrap_or_else(|| std::env::temp_dir().join("quill-attachments"))
}

/// Reads an image from the clipboard, saves it as PNG and gives its path to `pasted`. Nothing
/// happens when the clipboard holds no image (text pastes go on as they do).
pub fn paste(pasted: Arc<Mutex<Vec<PathBuf>>>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let Some(clip) = arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.get_image())
            .ok()
        else {
            return;
        };
        let (Ok(width), Ok(height)) = (u32::try_from(clip.width), u32::try_from(clip.height))
        else {
            return;
        };
        let Some(rgba) = image::RgbaImage::from_raw(width, height, clip.bytes.into_owned()) else {
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

/// The picture the viewer shows: 2048 px at most on its long side. PNG, or JPEG at quality 90
/// when the PNG would pass limn's 8 MiB cap.
pub fn full(path: &Path) -> Option<RasterImage> {
    scaled(path, 2048)
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
    let shown = quill_ui::annotate::shown(aspect, window).width;
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
    const LIMN_CAP: usize = 8 * 1024 * 1024;
    let decoded = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?
        .thumbnail(side, side);
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
}
