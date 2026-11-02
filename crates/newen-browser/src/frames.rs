//! The frames a page draws, decoded off the UI thread and left for the host to present: only
//! the newest frame of each surface is kept, so a slow window skips frames instead of
//! queueing them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// A page's pixels, `width` by `height`, rendered for `scale` device pixels per CSS pixel.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub scale: f32,
    pub pixels: Pixels,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pixels {
    /// Premultiplied RGBA rows top to bottom: egui's own form, so the window takes them
    /// without converting.
    Rgba(Vec<u8>),
    /// A texture of the window's own device, BGRA, that a native tab's paints are copied
    /// into on the GPU: the window draws it as it is. It is written again by the next paint.
    Texture(wgpu::Texture),
}

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Shared between the browser, which publishes, and the host, which takes.
#[derive(Clone, Default)]
pub struct Frames {
    fresh: Arc<Mutex<HashMap<String, Frame>>>,
    wake: Arc<Mutex<Option<Wake>>>,
    /// The window's scale as f32 bits; Chromium is launched at it.
    scale: Arc<AtomicU32>,
}

impl std::fmt::Debug for Frames {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frames")
            .field("scale", &self.scale())
            .finish_non_exhaustive()
    }
}

impl Frames {
    /// Call `wake` (the window's repaint) whenever a frame is published.
    pub fn on_frame(&self, wake: impl Fn() + Send + Sync + 'static) {
        *self.wake.lock().unwrap() = Some(Arc::new(wake));
    }

    /// The frames published since the last call, newest of each surface.
    pub fn take(&self) -> Vec<(String, Frame)> {
        self.fresh.lock().unwrap().drain().collect()
    }

    /// The host tells the window's device pixels per logical pixel, every frame.
    pub fn set_scale(&self, scale: f32) {
        self.scale.store(scale.to_bits(), Ordering::Relaxed);
    }

    pub fn scale(&self) -> f32 {
        match f32::from_bits(self.scale.load(Ordering::Relaxed)) {
            scale if scale.is_finite() && scale > 0.0 => scale,
            _ => 1.0,
        }
    }

    pub(crate) fn publish(&self, surface: String, frame: Frame) {
        self.fresh.lock().unwrap().insert(surface, frame);
        let wake = self.wake.lock().unwrap().clone();
        if let Some(wake) = wake {
            wake();
        }
    }
}

/// A screencast frame's JPEG as opaque RGBA.
pub fn decode(jpeg: &[u8], scale: f32) -> Option<Frame> {
    use zune_core::bytestream::ZCursor;
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(jpeg), options);
    let rgba = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (info.width as usize, info.height as usize);
    (rgba.len() == width * height * 4).then_some(Frame {
        width,
        height,
        scale,
        pixels: Pixels::Rgba(rgba),
    })
}
