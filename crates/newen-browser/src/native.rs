//! Chromium inside Newen's own process (docs/browser.md, "Native view"): each tab draws
//! without a window of its own, and its frames reach the pane's surface as they are painted,
//! with no screencast in between. The engine drives it over the same DevTools stream it
//! drives a headless Chromium with: a message for a page goes to that browser's own
//! DevTools, and the few browser-wide commands it sends (contexts, tabs, downloads) are
//! answered with CEF's own. Nothing of it is a window, so nothing of the platform's windows
//! (X11, Wayland, their scales and cursors) is involved, and Newen draws over a page as over
//! anything else of its own. Given the window's GPU, a tab's paints stay on it: Chromium's
//! buffer (a dmabuf) is copied into a texture of the window's device, which it draws as is.

use crate::frames::{Frames, Pixels};
use cef::*;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// DevTools ids Newen's own calls use, far above the engine's, which count from 1.
const OWN_IDS: i64 = 1 << 30;
/// The program run again as one of Chromium's own processes (`--type=renderer`…): it runs
/// that process and gives its exit code. Call it first thing in `main`.
pub fn subprocess() -> Option<i32> {
    if !std::env::args().any(|arg| arg.starts_with("--type=")) {
        return None;
    }
    library();
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();
    Some(execute_process(
        Some(args.as_main_args()),
        None,
        std::ptr::null_mut(),
    ))
}

/// Chromium running in Newen's process, and what it shows in Newen's window.
#[derive(Clone)]
pub struct Native {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for Native {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Native").finish_non_exhaustive()
    }
}

/// Where a tab's GPU paints are shown besides `Frames`: called on CEF's thread with the surface
/// and the tab's texture, the paint just copied into it. `true` says they were shown, and no
/// frame is published for the window.
pub type Present = Arc<dyn Fn(&str, &wgpu::Texture) -> bool + Send + Sync>;

struct Shared {
    frames: Frames,
    gpu: Option<Gpu>,
    present: Mutex<Option<Present>>,
    pages: Mutex<Pages>,
    shown: Mutex<Shown>,
    /// The engine's end of the DevTools stream, while one is open.
    stream: Mutex<Option<std::sync::mpsc::Sender<Vec<u8>>>>,
    own: AtomicI64,
    tokens: AtomicU64,
}

#[derive(Default)]
struct Pages {
    contexts: HashMap<String, RequestContext>,
    next_context: u32,
    /// Each tab by its target id, which is its page's main frame's.
    open: HashMap<String, Page>,
    /// Browsers made and not yet told their target id, by the token their client carries.
    made: HashMap<u64, Made>,
    /// The target id of each browser's token, once known.
    targets: HashMap<u64, String>,
    /// Each browser's size in CSS pixels, by its token: the tab's on screen, or the one the
    /// agent asked for.
    sizes: HashMap<u64, (u32, u32)>,
    own: HashMap<i64, Own>,
    downloads: Option<PathBuf>,
    finished: HashSet<u32>,
    /// `Browser.close` was asked: the stream ends once the last tab has closed.
    closing: bool,
}

struct Page {
    browser: Browser,
    context: String,
    url: String,
    title: String,
    _observer: Option<Registration>,
}

/// A browser on its way: what to answer, or announce, once its target id is known.
struct Made {
    reply: Option<i64>,
    opener: Option<String>,
    context: String,
    url: String,
    page: Option<Page>,
}

enum Own {
    TargetInfo { token: u64 },
}

/// The tab on screen: the surface its frames go to, and what it last painted, which a
/// `<select>`'s list, painted apart, is drawn over.
#[derive(Default)]
struct Shown {
    token: Option<u64>,
    surface: Option<String>,
    /// The scale Chromium was last told, to tell it again when the window's changes.
    scale: f32,
    /// The page's last paint, RGBA, kept only while a popup is open, and its size in pixels.
    page: Vec<u8>,
    size: (usize, usize),
    /// The open popup, where it is in CSS pixels, and its last paint, RGBA, with its width.
    popup: Option<Rect>,
    popup_pixels: Vec<u8>,
    popup_width: usize,
    /// On the GPU: the tab's texture, which its surface shows, and the popup's last paint.
    texture: Option<wgpu::Texture>,
    popup_texture: Option<wgpu::Texture>,
}

/// The window's device, which a tab's paints are copied into.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// A paint could not be imported once already: said once, not at every paint.
    failed: std::sync::atomic::AtomicBool,
}

impl Native {
    /// Start Chromium, its tabs drawn into `frames` at the window's scale, keeping what it
    /// must in `cache`. Given the window's device, a tab's paints are copied into textures of
    /// it, if it can import Chromium's buffers (Vulkan's dmabufs) and Chromium has a display
    /// server to allocate them through; otherwise they reach the CPU. Call it on the main
    /// thread, once.
    pub fn start(
        cache: &Path,
        frames: Frames,
        gpu: Option<(wgpu::Device, wgpu::Queue)>,
    ) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        mac::load()?;
        library();
        let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
        // Chromium allocates its GPU buffers through a display server, with no window of its
        // own: the headless platform has none to give ("Unable to allocate frame"), and on
        // Wayland it paints at 1× whatever scale it is told. X11 (XWayland in a Wayland
        // session) takes the window's.
        // Off Linux there are no dmabufs to import (`Gpu::import`): paints reach the CPU.
        let x11 =
            cfg!(target_os = "linux") && std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty());
        let gpu = gpu.filter(|(device, _)| {
            let imports = device
                .features()
                .contains(wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF);
            if !(x11 && imports) {
                let why = if imports {
                    "no X11 display"
                } else {
                    "the window's GPU cannot import dmabufs (not Vulkan?)"
                };
                eprintln!("newen: the browser's frames go through the CPU: {why}");
            }
            x11 && imports
        });
        crate::clear_cache_if_asked(cache);
        std::fs::create_dir_all(cache).map_err(|e| e.to_string())?;
        let args = cef::args::Args::new();
        let mut settings = Settings {
            // macOS runs CEF's loop on the main thread only: the window's loop pumps it
            // (`Native::pump`).
            multi_threaded_message_loop: i32::from(!cfg!(target_os = "macos")),
            external_message_pump: i32::from(cfg!(target_os = "macos")),
            windowless_rendering_enabled: 1,
            root_cache_path: CefString::from(cache.to_string_lossy().as_ref()),
            log_file: CefString::from(cache.join("chromium.log").to_string_lossy().as_ref()),
            log_severity: LogSeverity::WARNING,
            // shortcut: CEF's sandbox library is linked on Linux only (Windows needs its
            // bootstrap executable, macOS a sandboxed helper); add them to sandbox pages there.
            no_sandbox: i32::from(
                !cfg!(target_os = "linux")
                    || std::env::var_os("NEWEN_NO_SANDBOX").is_some()
                    || std::env::var_os("QUILL_NO_SANDBOX").is_some()
                    || userns_restricted(),
            ),
            ..Default::default()
        };
        // In a macOS bundle the framework holds its resources and CEF finds them itself.
        if let Some(dir) = cef_dir() {
            settings.resources_dir_path = CefString::from(dir.to_string_lossy().as_ref());
            settings.locales_dir_path =
                CefString::from(dir.join("locales").to_string_lossy().as_ref());
        }
        let mut app = NewenApp::new(if gpu.is_some() { "x11" } else { "headless" });
        if initialize(
            Some(args.as_main_args()),
            Some(&settings),
            Some(&mut app),
            std::ptr::null_mut(),
        ) != 1
        {
            return Err(format!(
                "Chromium did not start; see {}",
                cache.join("chromium.log").display()
            ));
        }
        Ok(Self {
            shared: Arc::new(Shared {
                frames,
                gpu: gpu.map(|(device, queue)| Gpu {
                    device,
                    queue,
                    failed: Default::default(),
                }),
                present: Mutex::default(),
                pages: Mutex::default(),
                shown: Mutex::default(),
                stream: Mutex::default(),
                own: AtomicI64::new(OWN_IDS),
                tokens: AtomicU64::default(),
            }),
        })
    }

    /// Close every tab and stop Chromium. Call it on the thread `start` ran on, after the
    /// window's loop ended.
    pub fn shutdown(&self) {
        self.shared.post(Job::CloseAll);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline && !self.shared.pages.lock().unwrap().open.is_empty() {
            #[cfg(target_os = "macos")]
            do_message_loop_work();
            std::thread::sleep(Duration::from_millis(20));
        }
        shutdown();
    }

    /// A fresh DevTools stream for the engine, as a launched Chromium's pipe would be.
    pub(crate) fn pipe(&self) -> (impl Read + Send + 'static, impl Write + Send + 'static) {
        let (to_engine, from_cef) = std::sync::mpsc::channel();
        *self.shared.stream.lock().unwrap() = Some(to_engine);
        self.shared.pages.lock().unwrap().closing = false;
        (
            Incoming {
                from: from_cef,
                left: Vec::new(),
                at: 0,
            },
            Outgoing {
                shared: self.shared.clone(),
                buffer: Vec::new(),
            },
        )
    }

    /// The tab on screen, the surface its frames go to and its size in CSS pixels; `None`
    /// shows none.
    pub(crate) fn show(&self, shown: Option<(String, String, (u32, u32))>) {
        self.shared.post(Job::Show(shown));
    }

    /// Show the tabs' GPU paints through `present` as well, as soon as each is copied.
    pub fn present_with(&self, present: Present) {
        *self.shared.present.lock().unwrap() = Some(present);
    }

    /// Run CEF's pending work, on the main thread: on macOS its loop is the window's, so the
    /// window calls this each frame and whenever `wake` (see `pump_with`) asks. Elsewhere CEF
    /// has threads of its own and this does nothing.
    pub fn pump(&self) {
        #[cfg(target_os = "macos")]
        do_message_loop_work();
    }

    /// How CEF asks for `pump` to be called, after the given delay, from any thread.
    pub fn pump_with(&self, wake: Wake) {
        *PUMP.lock().unwrap() = Some(wake);
    }

    /// Draw the tab `target` at `size` CSS pixels.
    pub(crate) fn resize(&self, target: String, size: (u32, u32)) {
        self.shared.post(Job::Resize { target, size });
    }
}

/// CEF's premultiplied BGRA as egui's RGBA, in one pass that reads and writes each pixel
/// once, as a word.
fn to_rgba(bgra: &[u8]) -> Vec<u8> {
    // Little-endian BGRA is 0xAARRGGBB and RGBA 0xAABBGGRR: red and blue trade places.
    let swap = |p: u32| (p & 0xFF00_FF00) | (p & 0x00FF_00FF).rotate_left(16);
    let mut rgba = vec![0u8; bgra.len()];
    match (
        bytemuck::try_cast_slice::<u8, u32>(bgra),
        bytemuck::try_cast_slice_mut::<u8, u32>(&mut rgba),
    ) {
        (Ok(from), Ok(to)) => to
            .iter_mut()
            .zip(from)
            .for_each(|(to, from)| *to = swap(*from)),
        // Not aligned to a word: pixel by pixel.
        _ => {
            let (to, _) = rgba.as_chunks_mut::<4>();
            for (to, &[b, g, r, a]) in to.iter_mut().zip(bgra.as_chunks::<4>().0) {
                *to = [r, g, b, a];
            }
        }
    }
    rgba
}

/// The part of a popup of `size` pixels, its corner `at`, inside a page of `page` pixels:
/// where that part starts in the popup and in the page, and its size. `None` when none is.
fn clip(at: [i64; 2], size: [usize; 2], page: [usize; 2]) -> Option<[[usize; 2]; 3]> {
    let mut part = [[0; 2]; 3];
    for axis in 0..2 {
        let start = at[axis].max(0);
        let end = (at[axis] + size[axis] as i64).min(page[axis] as i64);
        if start >= end {
            return None;
        }
        part[0][axis] = (start - at[axis]) as usize;
        part[1][axis] = start as usize;
        part[2][axis] = (end - start) as usize;
    }
    Some(part)
}

/// `page`, RGBA rows `width` pixels wide, with `popup` (its RGBA, its width, where its corner
/// is) drawn over it, clipped to the page.
fn over(page: &[u8], width: usize, popup: Option<(&[u8], usize, [i64; 2])>) -> Vec<u8> {
    let mut out = page.to_vec();
    let Some((pixels, popup_width, at)) = popup.filter(|(_, w, _)| *w > 0) else {
        return out;
    };
    let size = [popup_width, pixels.len() / (popup_width * 4)];
    let page_size = [width, page.len() / (width * 4).max(1)];
    let Some([from, to, [span, rows]]) = clip(at, size, page_size) else {
        return out;
    };
    for row in 0..rows {
        let src = ((from[1] + row) * popup_width + from[0]) * 4;
        let dst = ((to[1] + row) * width + to[0]) * 4;
        out[dst..dst + span * 4].copy_from_slice(&pixels[src..src + span * 4]);
    }
    out
}

impl Gpu {
    /// Chromium's buffer as a texture of the window's device, to copy from while the paint
    /// lasts: the buffer goes back to Chromium when it returns.
    #[cfg(not(target_os = "linux"))]
    fn import(&self, _: &AcceleratedPaintInfo) -> Result<wgpu::Texture, String> {
        // Windows' shared handles and macOS's IOSurfaces are not imported: `start` keeps those
        // systems' paints on the CPU.
        Err("GPU paints are imported on Linux only".into())
    }

    /// Chromium's buffer as a texture of the window's device, to copy from while the paint
    /// lasts: the buffer goes back to Chromium when it returns.
    #[cfg(target_os = "linux")]
    fn import(&self, info: &AcceleratedPaintInfo) -> Result<wgpu::Texture, String> {
        use std::os::fd::BorrowedFd;
        use wgpu::hal::api::Vulkan;
        // shortcut: one plane of BGRA, as Chromium paints a page; another layout is not shown.
        if info.plane_count != 1 || info.format != ColorType::BGRA_8888 {
            return Err(format!("{} planes of {:?}", info.plane_count, info.format));
        }
        let plane = &info.planes[0];
        // shortcut: on X11 Chromium names no modifier (DRM_FORMAT_MOD_INVALID); its capture
        // buffers are the ones it maps for the CPU too, so linear is taken. A tiled one would
        // show scrambled: then the modifier has to be asked of GBM.
        let modifier = match info.modifier {
            0x00ff_ffff_ffff_ffff => 0,
            modifier => modifier,
        };
        let size = wgpu::Extent3d {
            width: info.extra.coded_size.width.max(1) as u32,
            height: info.extra.coded_size.height.max(1) as u32,
            depth_or_array_layers: 1,
        };
        // Chromium keeps its descriptor; the driver takes a copy of it.
        let fd = unsafe { BorrowedFd::borrow_raw(plane.fd) }
            .try_clone_to_owned()
            .map_err(|e| e.to_string())?;
        let hal = unsafe {
            let device = self
                .device
                .as_hal::<Vulkan>()
                .ok_or("not a Vulkan device")?;
            let desc = wgpu::hal::TextureDescriptor {
                label: Some("chromium paint"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUses::COPY_SRC,
                memory_flags: wgpu::hal::MemoryFlags::empty(),
                view_formats: Vec::new(),
            };
            device
                .texture_from_dmabuf_fd(fd, &desc, modifier, plane.stride.into(), plane.offset)
                .map_err(|e| e.to_string())?
        };
        let desc = wgpu::TextureDescriptor {
            label: Some("chromium paint"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        };
        // Freshly made, its layout is undefined, which keeps a linear buffer's contents.
        Ok(unsafe {
            self.device.create_texture_from_hal::<Vulkan>(
                hal,
                &desc,
                wgpu::TextureUses::UNINITIALIZED,
            )
        })
    }

    /// `texture` if it is `width` by `height`, or a new one in its place.
    fn sized(&self, texture: &mut Option<wgpu::Texture>, width: u32, height: u32) -> wgpu::Texture {
        if let Some(texture) = texture
            .as_ref()
            .filter(|t| (t.width(), t.height()) == (width, height))
        {
            return texture.clone();
        }
        let made = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("native tab"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        *texture = Some(made.clone());
        made
    }
}

/// Copy `size` pixels from `from` at `at` into `to` at `into`.
fn copy(
    encoder: &mut wgpu::CommandEncoder,
    (from, at): (&wgpu::Texture, [u32; 2]),
    (to, into): (&wgpu::Texture, [u32; 2]),
    [width, height]: [u32; 2],
) {
    let place = |texture, [x, y]: [u32; 2]| wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d { x, y, z: 0 },
        aspect: wgpu::TextureAspect::All,
    };
    encoder.copy_texture_to_texture(
        place(from, at),
        place(to, into),
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

/// Where CEF's files are: `NEWEN_CEF_DIR`; else a release's, beside the program or, in a
/// Windows installation, in `versions\{version}` beside it (docs/UPDATES.md); else the CEF Newen
/// was built with. `None` in a macOS bundle, whose framework holds them.
fn cef_dir() -> Option<PathBuf> {
    // quill's name for it too, which a person's launcher may set (a NixOS wrapper, say).
    let set = std::env::var_os("NEWEN_CEF_DIR").or_else(|| std::env::var_os("QUILL_CEF_DIR"));
    if let Some(dir) = set.filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let program = std::env::current_exe().ok();
    #[cfg(target_os = "macos")]
    if program.as_deref().is_some_and(mac::bundled) {
        return None;
    }
    let beside = program
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_owned);
    let candidates = beside.iter().flat_map(|dir| {
        [
            dir.clone(),
            dir.join("versions").join(env!("CARGO_PKG_VERSION")),
        ]
    });
    candidates
        .chain([PathBuf::from(env!("NEWEN_CEF_DIR"))])
        .find(|dir| dir.join("icudtl.dat").is_file())
        .or_else(|| Some(PathBuf::from(env!("NEWEN_CEF_DIR"))))
}

/// Whether AppArmor keeps unprivileged programs from making user namespaces (Ubuntu 23.10 and
/// later), which Chromium's sandbox needs unless its `chrome-sandbox` is setuid root, as a
/// per-person installation's cannot be: then Chromium would not start at all with it.
fn userns_restricted() -> bool {
    let restricted =
        std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
            .is_ok_and(|value| value.trim() == "1");
    if restricted {
        eprintln!(
            "newen: the browser's pages run without Chromium's sandbox: AppArmor restricts \
             user namespaces here (kernel.apparmor_restrict_unprivileged_userns = 1)"
        );
    }
    restricted
}

/// Make libcef reachable before its first call. On Windows newen.exe loads libcef.dll late
/// (`/DELAYLOAD`, apps/newen/build.rs), so an installation's `versions\{version}` can hold it;
/// on Linux an rpath of `$ORIGIN` finds it beside the program, and macOS loads its framework in
/// `start`.
fn library() {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn SetDllDirectoryW(path: *const u16) -> i32;
        }
        if let Some(dir) = cef_dir().filter(|dir| dir.join("libcef.dll").is_file()) {
            let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
            // SAFETY: a NUL-terminated wide string that outlives the call.
            unsafe { SetDllDirectoryW(wide.as_ptr()) };
        }
    }
}

/// How CEF asks the window to pump its loop (macOS), after a delay, from any thread.
pub type Wake = Arc<dyn Fn(Duration) + Send + Sync>;

static PUMP: Mutex<Option<Wake>> = Mutex::new(None);

/// macOS: CEF is a framework inside Newen.app, loaded at run time, and Chromium's processes
/// run as the bundle's helper apps (`helper`); the application object must answer Chromium's
/// `CrAppProtocol`, which winit's does not, so its class is taught to.
#[cfg(target_os = "macos")]
mod mac {
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};

    const FRAMEWORK: &str = "Chromium Embedded Framework.framework";

    /// Whether `program` runs from `….app/Contents/MacOS` with the framework beside it.
    pub fn bundled(program: &Path) -> bool {
        program
            .parent()
            .and_then(Path::parent)
            .is_some_and(|contents| contents.join("Frameworks").join(FRAMEWORK).is_dir())
    }

    /// Load the framework of the bundle Newen runs from, once; it stays loaded.
    pub fn load() -> Result<(), String> {
        static LOADED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        LOADED
            .get_or_init(|| {
                let program = std::env::current_exe().map_err(|e| e.to_string())?;
                if !bundled(&program) {
                    return Err("not running from Newen.app: no CEF framework".into());
                }
                let loader = cef::library_loader::LibraryLoader::new(&program, false);
                if !loader.load() {
                    return Err("the CEF framework did not load".into());
                }
                // Dropping it would unload the framework.
                std::mem::forget(loader);
                answer_cr_app_protocol();
                Ok(())
            })
            .clone()
    }

    static HANDLING: AtomicBool = AtomicBool::new(false);

    extern "C-unwind" fn is_handling_send_event(
        _: &objc2::runtime::AnyObject,
        _: objc2::runtime::Sel,
    ) -> objc2::runtime::Bool {
        HANDLING.load(Ordering::Relaxed).into()
    }

    extern "C-unwind" fn set_handling_send_event(
        _: &objc2::runtime::AnyObject,
        _: objc2::runtime::Sel,
        value: objc2::runtime::Bool,
    ) {
        HANDLING.store(value.as_bool(), Ordering::Relaxed);
    }

    /// Add `isHandlingSendEvent` and `setHandlingSendEvent:` to the class of the running
    /// NSApplication and declare the protocols Chromium checks it for.
    fn answer_cr_app_protocol() {
        use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Imp, Sel};
        use objc2::{class, msg_send, sel};
        // SAFETY: called on the main thread once the window (and so NSApp) exists; the added
        // methods' signatures match their type encodings.
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
            let Some(app) = app.as_ref() else { return };
            let class = app.class() as *const AnyClass as *mut objc2::ffi::objc_class;
            let methods: [(Sel, Imp, &std::ffi::CStr); 2] = [
                (
                    sel!(isHandlingSendEvent),
                    std::mem::transmute::<
                        extern "C-unwind" fn(&AnyObject, Sel) -> objc2::runtime::Bool,
                        Imp,
                    >(is_handling_send_event),
                    c"c@:",
                ),
                (
                    sel!(setHandlingSendEvent:),
                    std::mem::transmute::<
                        extern "C-unwind" fn(&AnyObject, Sel, objc2::runtime::Bool),
                        Imp,
                    >(set_handling_send_event),
                    c"v@:c",
                ),
            ];
            for (name, imp, types) in methods {
                objc2::ffi::class_addMethod(class.cast(), name, imp, types.as_ptr());
            }
            for name in [c"CrAppProtocol", c"CrAppControlProtocol", c"CefAppProtocol"] {
                if let Some(protocol) = AnyProtocol::get(name) {
                    objc2::ffi::class_addProtocol(class.cast(), protocol);
                }
            }
        }
    }

    /// A helper app's process: load the framework from the helper's place in the bundle and
    /// run the process Chromium asked for.
    pub fn helper() -> i32 {
        let Ok(program) = std::env::current_exe() else {
            return 1;
        };
        let loader = cef::library_loader::LibraryLoader::new(&program, true);
        if !loader.load() {
            return 1;
        }
        let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
        let args = cef::args::Args::new();
        cef::execute_process(Some(args.as_main_args()), None, std::ptr::null_mut())
    }
}

/// The process of one of Newen.app's helper apps (macOS), which Chromium runs its renderers,
/// GPU and utility processes as: its exit code.
#[cfg(target_os = "macos")]
pub fn helper() -> i32 {
    mac::helper()
}

/// Work for CEF's UI thread, where its browsers must be driven.
#[derive(Clone)]
enum Job {
    Deliver(String),
    Show(Option<(String, String, (u32, u32))>),
    Resize { target: String, size: (u32, u32) },
    Repaint { target: String },
    CloseAll,
}

wrap_task! {
    struct Run {
        shared: Arc<Shared>,
        job: Job,
    }

    impl Task {
        fn execute(&self) {
            self.shared.run(self.job.clone());
        }
    }
}

impl Shared {
    fn post(self: &Arc<Self>, job: Job) {
        let mut task = Run::new(self.clone(), job);
        post_task(ThreadId::UI, Some(&mut task));
    }

    fn post_later(self: &Arc<Self>, job: Job, ms: i64) {
        let mut task = Run::new(self.clone(), job);
        post_delayed_task(ThreadId::UI, Some(&mut task), ms);
    }

    fn run(self: &Arc<Self>, job: Job) {
        match job {
            Job::Deliver(message) => {
                if let Ok(message) = serde_json::from_str::<Value>(&message) {
                    self.deliver(message);
                }
            }
            Job::Show(shown) => {
                let (target, surface, size) = match shown {
                    Some((target, surface, size)) => (Some(target), Some(surface), Some(size)),
                    None => (None, None, None),
                };
                let token = target.as_deref().and_then(|t| self.token_of(t));
                let mut shown = self.shown.lock().unwrap();
                if shown.token != token {
                    *shown = Shown::default();
                }
                shown.token = token;
                shown.surface = surface;
                let scale = self.frames.scale();
                let rescaled = shown.scale != scale;
                shown.scale = scale;
                drop(shown);
                if let (Some(target), Some(size)) = (target, size) {
                    if let Some(host) = self.host(&target) {
                        if rescaled {
                            host.notify_screen_info_changed();
                        }
                        // A tab only paints what changes: the one shown now paints it all, at
                        // its new size if it has one.
                        if !self.resize(&target, size) {
                            host.invalidate(PaintElementType::VIEW);
                        }
                    }
                }
            }
            Job::Resize { target, size } => {
                self.resize(&target, size);
            }
            Job::Repaint { target } => {
                if let Some(host) = self.host(&target) {
                    host.invalidate(PaintElementType::VIEW);
                }
            }
            Job::CloseAll => {
                let mut pages = self.pages.lock().unwrap();
                pages.closing = true;
                let hosts: Vec<BrowserHost> = pages
                    .open
                    .values()
                    .filter_map(|p| p.browser.host())
                    .collect();
                let empty = pages.open.is_empty();
                drop(pages);
                for host in hosts {
                    host.close_browser(1);
                }
                if empty {
                    self.stream.lock().unwrap().take();
                }
            }
        }
    }

    fn token_of(&self, target: &str) -> Option<u64> {
        let pages = self.pages.lock().unwrap();
        pages
            .targets
            .iter()
            .find(|(_, t)| t.as_str() == target)
            .map(|(token, _)| *token)
    }

    /// Whether `target` had another size, and was resized.
    fn resize(&self, target: &str, size: (u32, u32)) -> bool {
        let Some(token) = self.token_of(target) else {
            return false;
        };
        let size = (size.0.max(1), size.1.max(1));
        let resized = self.pages.lock().unwrap().sizes.insert(token, size) != Some(size);
        if resized {
            if let Some(host) = self.host(target) {
                host.was_resized();
            }
        }
        resized
    }

    fn host(&self, target: &str) -> Option<BrowserHost> {
        self.pages
            .lock()
            .unwrap()
            .open
            .get(target)
            .and_then(|p| p.browser.host())
    }

    /// Send the engine one message of the stream.
    fn emit(&self, message: Value) {
        if let Some(stream) = self.stream.lock().unwrap().as_ref() {
            let mut bytes = message.to_string().into_bytes();
            bytes.push(0);
            let _ = stream.send(bytes);
        }
    }

    fn reply(&self, id: i64, result: Result<Value, String>) {
        self.emit(match result {
            Ok(result) => json!({ "id": id, "result": result }),
            Err(message) => json!({ "id": id, "error": { "code": -32000, "message": message } }),
        });
    }

    fn event(&self, method: &str, params: Value) {
        self.emit(json!({ "method": method, "params": params }));
    }

    /// One message from the engine: for a page, to its DevTools; for the browser, answered here.
    fn deliver(self: &Arc<Self>, message: Value) {
        let id = message["id"].as_i64().unwrap_or_default();
        let method = message["method"].as_str().unwrap_or_default();
        let params = message["params"].clone();
        match message["sessionId"].as_str() {
            Some(target) => match self.host(target) {
                Some(host) => {
                    let mut call = json!({ "id": id, "method": method });
                    if params.is_object() {
                        call["params"] = params;
                    }
                    host.send_dev_tools_message(Some(call.to_string().as_bytes()));
                }
                None => self.reply(id, Err("No session with given id".into())),
            },
            None => self.browser(id, method, params),
        }
    }

    /// The browser-wide commands the engine sends, in CEF's terms.
    fn browser(self: &Arc<Self>, id: i64, method: &str, params: Value) {
        let text = |key: &str| params[key].as_str().unwrap_or_default().to_owned();
        match method {
            "Target.setDiscoverTargets" => self.reply(id, Ok(json!({}))),
            "Target.createBrowserContext" => {
                let mut pages = self.pages.lock().unwrap();
                pages.next_context += 1;
                let name = format!("context-{}", pages.next_context);
                // No cache path: cookies and storage live in memory, one session's apart.
                if let Some(context) =
                    request_context_create_context(Some(&RequestContextSettings::default()), None)
                {
                    pages.contexts.insert(name.clone(), context);
                }
                drop(pages);
                self.reply(id, Ok(json!({ "browserContextId": name })));
            }
            "Target.disposeBrowserContext" => {
                let context = text("browserContextId");
                let mut pages = self.pages.lock().unwrap();
                pages.contexts.remove(&context);
                let hosts: Vec<BrowserHost> = pages
                    .open
                    .values()
                    .filter(|p| p.context == context)
                    .filter_map(|p| p.browser.host())
                    .collect();
                drop(pages);
                for host in hosts {
                    host.close_browser(1);
                }
                self.reply(id, Ok(json!({})));
            }
            "Target.createTarget" => {
                let size = (
                    params["width"].as_u64().unwrap_or(1280) as u32,
                    params["height"].as_u64().unwrap_or(800) as u32,
                );
                self.make(Some(id), None, text("browserContextId"), text("url"), size);
            }
            "Target.attachToTarget" => {
                let target = text("targetId");
                let known = self.pages.lock().unwrap().open.contains_key(&target);
                self.reply(
                    id,
                    if known {
                        Ok(json!({ "sessionId": target }))
                    } else {
                        Err("No target with given id found".into())
                    },
                );
            }
            "Target.closeTarget" => {
                if let Some(host) = self.host(&text("targetId")) {
                    host.close_browser(1);
                }
                self.reply(id, Ok(json!({ "success": true })));
            }
            "Browser.setDownloadBehavior" => {
                let path = text("downloadPath");
                self.pages.lock().unwrap().downloads = (!path.is_empty()).then(|| path.into());
                self.reply(id, Ok(json!({})));
            }
            "Browser.close" => {
                self.reply(id, Ok(json!({})));
                self.run(Job::CloseAll);
            }
            other => self.reply(id, Err(format!("'{other}' wasn't found"))),
        }
    }

    /// Make a tab in `context` showing `url`: answered to `reply`, or announced as opened by
    /// `opener`, once its target id is known.
    fn make(
        self: &Arc<Self>,
        reply: Option<i64>,
        opener: Option<String>,
        context: String,
        url: String,
        size: (u32, u32),
    ) {
        let token = self.tokens.fetch_add(1, Ordering::Relaxed) + 1;
        let mut request = {
            let mut pages = self.pages.lock().unwrap();
            pages.sizes.insert(token, size);
            pages.made.insert(
                token,
                Made {
                    reply,
                    opener,
                    context: context.clone(),
                    url: url.clone(),
                    page: None,
                },
            );
            pages.contexts.get(&context).cloned()
        };
        // Alloy: the page alone, without Chrome's tab and window machinery.
        // No parent window: an X11 id on Linux, a pointer (HWND, NSView) elsewhere.
        #[cfg(target_os = "linux")]
        let parent = 0;
        #[cfg(windows)]
        let parent = sys::HWND(std::ptr::null_mut());
        #[cfg(target_os = "macos")]
        let parent = std::ptr::null_mut();
        let mut info = WindowInfo::default().set_as_windowless(parent);
        info.shared_texture_enabled = i32::from(self.gpu.is_some());
        let mut client = PageClient::new(self.clone(), token);
        let url = if url.is_empty() {
            "about:blank".to_owned()
        } else {
            url
        };
        browser_host_create_browser(
            Some(&info),
            Some(&mut client),
            Some(&CefString::from(url.as_str())),
            Some(&BrowserSettings {
                // A 120 Hz screen's: a steady scroll paints ~100 frames a second, not 60.
                windowless_frame_rate: 120,
                // Opaque, as a window's page is: a page with no background of its own is white.
                background_color: 0xFFFF_FFFF,
                ..Default::default()
            }),
            None,
            request.as_mut(),
        );
    }

    /// A browser was made: it is asked for its target id.
    fn made(self: &Arc<Self>, token: u64, browser: Browser) {
        let Some(host) = browser.host() else {
            return;
        };
        let mut observer = Observer::new(self.clone(), token);
        let registration = host.add_dev_tools_message_observer(Some(&mut observer));
        let mut pages = self.pages.lock().unwrap();
        let Some(made) = pages.made.get_mut(&token) else {
            return;
        };
        made.page = Some(Page {
            browser,
            context: made.context.clone(),
            url: made.url.clone(),
            title: String::new(),
            _observer: registration,
        });
        let id = self.own.fetch_add(1, Ordering::Relaxed);
        pages.own.insert(id, Own::TargetInfo { token });
        drop(pages);
        let call = json!({ "id": id, "method": "Target.getTargetInfo" });
        host.send_dev_tools_message(Some(call.to_string().as_bytes()));
    }

    /// What a page's DevTools sent: Newen's own answers are kept, the rest goes to the engine
    /// as from the page's session.
    fn heard(self: &Arc<Self>, token: u64, raw: &[u8]) {
        let Ok(mut message) = serde_json::from_slice::<Value>(raw) else {
            return;
        };
        if let Some(id) = message["id"].as_i64().filter(|id| *id >= OWN_IDS) {
            let purpose = self.pages.lock().unwrap().own.remove(&id);
            match purpose {
                Some(Own::TargetInfo { token }) => {
                    let target = message["result"]["targetInfo"]["targetId"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    self.named(token, target);
                }
                None => {}
            }
            return;
        }
        let target = self.pages.lock().unwrap().targets.get(&token).cloned();
        if let Some(target) = target {
            message["sessionId"] = json!(target);
            self.emit(message);
        }
    }

    /// The browser of `token` is the page `target`: a tab now, answered or announced.
    fn named(self: &Arc<Self>, token: u64, target: String) {
        let mut pages = self.pages.lock().unwrap();
        let Some(mut made) = pages.made.remove(&token) else {
            return;
        };
        let Some(page) = made.page.take() else {
            return;
        };
        let (url, context) = (page.url.clone(), page.context.clone());
        pages.targets.insert(token, target.clone());
        pages.open.insert(target.clone(), page);
        drop(pages);
        if let Some(reply) = made.reply {
            self.reply(reply, Ok(json!({ "targetId": target })));
        }
        if let Some(opener) = made.opener {
            self.event(
                "Target.targetCreated",
                json!({ "targetInfo": { "targetId": target, "type": "page", "url": url, "title": "",
                         "attached": false, "openerId": opener, "browserContextId": context } }),
            );
        }
    }

    /// The tab of `token` painted `pixels`, BGRA, `width` by `height`: its page, or the
    /// popup over it. Only the tab on screen is shown.
    fn painted(
        self: &Arc<Self>,
        token: u64,
        popup: bool,
        pixels: &[u8],
        width: usize,
        height: usize,
    ) {
        let mut shown = self.shown.lock().unwrap();
        if shown.token != Some(token) || pixels.len() < width * height * 4 {
            return;
        }
        if !popup && self.stale(token, width, height, shown.scale) {
            return;
        }
        // shortcut: the copy is on CEF's UI thread; Chromium's shared textures
        // (OnAcceleratedPaint, dmabuf) avoid it if it shows.
        let rgba = to_rgba(&pixels[..width * height * 4]);
        if popup {
            shown.popup_pixels = rgba;
            shown.popup_width = width;
            self.present(&shown);
            return;
        }
        shown.size = (width, height);
        // The page is kept only while a popup is drawn over it, as either may paint again.
        if shown.popup.is_some() {
            shown.page = rgba;
            self.present(&shown);
            return;
        }
        shown.page = Vec::new();
        if let Some(surface) = shown.surface.clone() {
            let scale = shown.scale;
            let frame = crate::Frame {
                width,
                height,
                scale,
                pixels: Pixels::Rgba(rgba),
            };
            self.frames.publish(surface, frame);
        }
    }

    /// Whether a paint of the tab of `token`, `width` by `height` pixels, is still at its size
    /// before a resize. It would stand in for the new one, as nothing may paint again: only
    /// the size asked for is shown. Chromium now and then keeps the old size until it paints
    /// again, so it is asked to, a moment later.
    fn stale(self: &Arc<Self>, token: u64, width: usize, height: usize, scale: f32) -> bool {
        let size = self.pages.lock().unwrap().sizes.get(&token).copied();
        let near = |pixels: usize, css: u32| (pixels as f32 - css as f32 * scale).abs() <= 1.0;
        if !size.is_some_and(|(w, h)| !near(width, w) || !near(height, h)) {
            return false;
        }
        let target = self.pages.lock().unwrap().targets.get(&token).cloned();
        if let Some(target) = target {
            self.post_later(Job::Repaint { target }, 100);
        }
        true
    }

    /// The tab of `token` painted on the GPU, its page or the popup over it: copied into the
    /// tab's texture, the popup's last paint over the page's, and waited for, as Chromium
    /// takes its buffer back when this returns. Only the tab on screen is shown.
    fn accelerated(self: &Arc<Self>, token: u64, popup: bool, info: &AcceleratedPaintInfo) {
        let Some(gpu) = &self.gpu else {
            return;
        };
        let visible = &info.extra.visible_rect;
        let (width, height) = (visible.width.max(0) as u32, visible.height.max(0) as u32);
        let mut shown = self.shown.lock().unwrap();
        if shown.token != Some(token) || width == 0 || height == 0 {
            return;
        }
        if !popup && self.stale(token, width as usize, height as usize, shown.scale) {
            return;
        }
        let paint = match gpu.import(info) {
            Ok(paint) => paint,
            Err(error) => {
                if !gpu.failed.swap(true, Ordering::Relaxed) {
                    eprintln!("newen: a page's frame could not be imported: {error}");
                }
                return;
            }
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let painted = (&paint, [visible.x.max(0) as u32, visible.y.max(0) as u32]);
        let target = if popup {
            &mut shown.popup_texture
        } else {
            &mut shown.texture
        };
        let into = gpu.sized(target, width, height);
        copy(&mut encoder, painted, (&into, [0, 0]), [width, height]);
        let Some(page) = shown.texture.clone() else {
            // A popup with no page to go over yet: the page paints again.
            drop(encoder);
            self.repaint(shown.token);
            return;
        };
        if let (Some(over), Some(at)) = (&shown.popup_texture, &shown.popup) {
            let at = [at.x, at.y].map(|v| (v as f32 * shown.scale).round() as i64);
            let size = [over.width(), over.height()].map(|v| v as usize);
            let page_size = [page.width(), page.height()].map(|v| v as usize);
            if let Some([from, to, extent]) = clip(at, size, page_size) {
                let [from, to, extent] = [from, to, extent].map(|p| p.map(|v| v as u32));
                copy(&mut encoder, (over, from), (&page, to), extent);
            }
        }
        let submitted = gpu.queue.submit([encoder.finish()]);
        let _ = gpu.device.poll(wgpu::PollType::Wait {
            submission_index: Some(submitted),
            timeout: Some(Duration::from_secs(1)),
        });
        let present = self.present.lock().unwrap().clone();
        if let (Some(present), Some(surface)) = (present, shown.surface.as_deref()) {
            if present(surface, &page) {
                return;
            }
        }
        if let Some(surface) = shown.surface.clone() {
            let frame = crate::Frame {
                width: page.width() as usize,
                height: page.height() as usize,
                scale: shown.scale,
                pixels: Pixels::Texture(page),
            };
            self.frames.publish(surface, frame);
        }
    }

    fn repaint(self: &Arc<Self>, token: Option<u64>) {
        let target = token.and_then(|t| self.pages.lock().unwrap().targets.get(&t).cloned());
        if let Some(target) = target {
            self.post(Job::Repaint { target });
        }
    }

    /// The tab on screen as it last painted, its popup over it, in its surface.
    fn present(self: &Arc<Self>, shown: &Shown) {
        let Some(surface) = shown.surface.clone() else {
            return;
        };
        let (width, height) = shown.size;
        // No page kept to draw the popup over yet: it paints again.
        if shown.page.len() != width * height * 4 || width == 0 {
            self.repaint(shown.token);
            return;
        }
        let scale = shown.scale;
        let popup = shown
            .popup
            .clone()
            .filter(|_| !shown.popup_pixels.is_empty())
            .map(|at| {
                let x = (at.x as f32 * scale).round() as i64;
                let y = (at.y as f32 * scale).round() as i64;
                (shown.popup_pixels.as_slice(), shown.popup_width, [x, y])
            });
        self.frames.publish(
            surface,
            crate::Frame {
                width,
                height,
                scale,
                pixels: Pixels::Rgba(over(&shown.page, width, popup)),
            },
        );
    }

    fn popup(self: &Arc<Self>, token: u64, at: Option<Rect>) {
        let mut shown = self.shown.lock().unwrap();
        if shown.token != Some(token) {
            return;
        }
        match at {
            // On the GPU the page has the popup drawn into it: it paints again, with the
            // popup where it is now.
            Some(_) if self.gpu.is_some() => {
                shown.popup = at;
                self.repaint(shown.token);
            }
            Some(_) => {
                shown.popup = at;
                self.present(&shown);
            }
            // The page paints again without it.
            None => {
                shown.popup = None;
                shown.popup_pixels.clear();
                shown.popup_texture = None;
                shown.page = Vec::new();
                self.repaint(shown.token);
            }
        }
    }

    fn target_of(&self, browser: &Browser) -> Option<String> {
        let id = browser.identifier();
        let pages = self.pages.lock().unwrap();
        pages
            .open
            .iter()
            .find(|(_, page)| page.browser.identifier() == id)
            .map(|(target, _)| target.clone())
    }

    /// A tab's address or title changed, as Chromium reports a target's.
    fn changed(&self, browser: &Browser, url: Option<String>, title: Option<String>) {
        let Some(target) = self.target_of(browser) else {
            return;
        };
        let mut pages = self.pages.lock().unwrap();
        let Some(page) = pages.open.get_mut(&target) else {
            return;
        };
        if let Some(url) = url {
            page.url = url;
        }
        if let Some(title) = title {
            page.title = title;
        }
        let info = json!({ "targetInfo": { "targetId": target, "type": "page", "url": page.url,
                           "title": page.title, "attached": true, "browserContextId": page.context } });
        drop(pages);
        self.event("Target.targetInfoChanged", info);
    }

    fn closed(&self, browser: &Browser) {
        let Some(target) = self.target_of(browser) else {
            return;
        };
        let mut pages = self.pages.lock().unwrap();
        pages.open.remove(&target);
        let gone: Vec<u64> = pages
            .targets
            .iter()
            .filter(|(_, t)| **t == target)
            .map(|(token, _)| *token)
            .collect();
        for token in &gone {
            pages.targets.remove(token);
            pages.sizes.remove(token);
        }
        let last = pages.closing && pages.open.is_empty();
        drop(pages);
        {
            let mut shown = self.shown.lock().unwrap();
            if shown.token.is_some_and(|token| gone.contains(&token)) {
                *shown = Shown::default();
            }
        }
        self.event(
            "Target.detachedFromTarget",
            json!({ "sessionId": target, "targetId": target }),
        );
        self.event("Target.targetDestroyed", json!({ "targetId": target }));
        if last {
            self.stream.lock().unwrap().take();
        }
    }
}

/// The engine's side of the stream: what CEF answers and reports, NUL-delimited.
struct Incoming {
    from: std::sync::mpsc::Receiver<Vec<u8>>,
    left: Vec<u8>,
    at: usize,
}

impl Read for Incoming {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.at >= self.left.len() {
            match self.from.recv() {
                Ok(next) => {
                    self.left = next;
                    self.at = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.left.len() - self.at);
        out[..n].copy_from_slice(&self.left[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// What the engine writes, a message at a NUL, carried to CEF's UI thread.
struct Outgoing {
    shared: Arc<Shared>,
    buffer: Vec<u8>,
}

impl Write for Outgoing {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend_from_slice(data);
        while let Some(end) = self.buffer.iter().position(|b| *b == 0) {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            if let Ok(message) = std::str::from_utf8(&line[..end]) {
                self.shared.post(Job::Deliver(message.to_owned()));
            }
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// `platform`: Chromium's display platform; `headless` paints on the CPU only.
wrap_app! {
    struct NewenApp {
        platform: &'static str,
    }

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(NewenProcess::new())
        }

        fn on_before_command_line_processing(&self, kind: Option<&CefString>, line: Option<&mut CommandLine>) {
            // The browser process's switches reach the others.
            let (None, Some(line)) = (kind.map(|k| k.to_string()).filter(|k| !k.is_empty()), line) else {
                return;
            };
            let switch = |name: &str, value: &str| {
                line.append_switch_with_value(Some(&CefString::from(name)), Some(&CefString::from(value)));
            };
            // No window of Chromium's own: a display server only to allocate GPU buffers
            // through, or none. The GPU through EGL, which ANGLE's default (an X11 display)
            // would not find without one.
            // Ozone and EGL are Linux's; Windows and macOS keep their own (D3D11, Metal).
            if cfg!(target_os = "linux") {
                switch("ozone-platform", self.platform);
                switch("use-angle", "gl-egl");
            }
            // As the headless Chromium runs: no keyring prompt, no background chatter.
            switch("password-store", "basic");
            line.append_switch(Some(&CefString::from("use-mock-keychain")));
            switch("disable-features", "Translate,MediaRouter,OptimizationHints");
        }
    }
}

// Only macOS's external pump asks for work to be scheduled (`Native::pump_with`).
wrap_browser_process_handler! {
    struct NewenProcess;

    impl BrowserProcessHandler {
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            if let Some(wake) = PUMP.lock().unwrap().as_ref() {
                wake(Duration::from_millis(delay_ms.max(0).unsigned_abs()));
            }
        }
    }
}

wrap_client! {
    struct PageClient {
        shared: Arc<Shared>,
        token: u64,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(Life::new(self.shared.clone(), self.token))
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(Display::new(self.shared.clone()))
        }

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(Downloads::new(self.shared.clone()))
        }

        fn render_handler(&self) -> Option<RenderHandler> {
            Some(Render::new(self.shared.clone(), self.token))
        }
    }
}

wrap_render_handler! {
    struct Render {
        shared: Arc<Shared>,
        token: u64,
    }

    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            let size = self.shared.pages.lock().unwrap().sizes.get(&self.token).copied();
            let (width, height) = size.unwrap_or((1280, 800));
            if let Some(rect) = rect {
                *rect = Rect { x: 0, y: 0, width: width as i32, height: height as i32 };
            }
        }

        fn screen_info(&self, _browser: Option<&mut Browser>, info: Option<&mut ScreenInfo>) -> ::std::os::raw::c_int {
            let Some(info) = info else {
                return 0;
            };
            info.device_scale_factor = self.shared.frames.scale();
            1
        }

        fn on_popup_show(&self, _browser: Option<&mut Browser>, show: ::std::os::raw::c_int) {
            if show == 0 {
                self.shared.popup(self.token, None);
            }
        }

        fn on_popup_size(&self, _browser: Option<&mut Browser>, rect: Option<&Rect>) {
            self.shared.popup(self.token, rect.cloned());
        }

        fn on_accelerated_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            info: Option<&AcceleratedPaintInfo>,
        ) {
            if let Some(info) = info {
                self.shared.accelerated(self.token, type_ == PaintElementType::POPUP, info);
            }
        }

        fn on_paint(
            &self,
            _browser: Option<&mut Browser>,
            type_: PaintElementType,
            _dirty_rects: Option<&[Rect]>,
            buffer: *const u8,
            width: ::std::os::raw::c_int,
            height: ::std::os::raw::c_int,
        ) {
            let (width, height) = (width.max(0) as usize, height.max(0) as usize);
            if buffer.is_null() || width == 0 || height == 0 {
                return;
            }
            // CEF's buffer: BGRA rows, `width` by `height`, alive for this call.
            let pixels = unsafe { std::slice::from_raw_parts(buffer, width * height * 4) };
            self.shared.painted(self.token, type_ == PaintElementType::POPUP, pixels, width, height);
        }
    }
}

wrap_life_span_handler! {
    struct Life {
        shared: Arc<Shared>,
        token: u64,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                self.shared.made(self.token, browser.clone());
            }
        }

        // A page opening another opens a tab of the same session, as Chromium's targets do.
        fn on_before_popup(
            &self,
            browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            _user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let opener = browser.and_then(|b| self.shared.target_of(b));
            if let Some(opener) = opener {
                let context = self.shared.pages.lock().unwrap().open.get(&opener).map(|p| p.context.clone());
                let url = target_url.map(|u| u.to_string()).unwrap_or_default();
                let size = self.shared.pages.lock().unwrap().sizes.get(&self.token).copied();
                if let Some(context) = context {
                    self.shared.make(None, Some(opener), context, url, size.unwrap_or((1280, 800)));
                }
            }
            1
        }

        fn on_before_close(&self, browser: Option<&mut Browser>) {
            if let Some(browser) = browser {
                self.shared.closed(browser);
            }
        }
    }
}

wrap_display_handler! {
    struct Display {
        shared: Arc<Shared>,
    }

    impl DisplayHandler {
        fn on_address_change(&self, browser: Option<&mut Browser>, frame: Option<&mut Frame>, url: Option<&CefString>) {
            if let (Some(browser), Some(frame)) = (browser, frame) {
                if frame.is_main() != 0 {
                    self.shared.changed(browser, url.map(|u| u.to_string()), None);
                }
            }
        }

        fn on_title_change(&self, browser: Option<&mut Browser>, title: Option<&CefString>) {
            if let Some(browser) = browser {
                self.shared.changed(browser, None, title.map(|t| t.to_string()));
            }
        }
    }
}

wrap_download_handler! {
    struct Downloads {
        shared: Arc<Shared>,
    }

    impl DownloadHandler {
        fn on_before_download(
            &self,
            browser: Option<&mut Browser>,
            item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> ::std::os::raw::c_int {
            let (Some(browser), Some(item), Some(callback)) = (browser, item, callback) else {
                return 0;
            };
            let name = suggested_name.map(|n| n.to_string()).unwrap_or_else(|| "download".into());
            let folder = self.shared.pages.lock().unwrap().downloads.clone();
            let Some(folder) = folder else {
                return 0;
            };
            let path = folder.join(&name);
            callback.cont(Some(&CefString::from(path.to_string_lossy().as_ref())), 0);
            let frame = self.shared.target_of(browser).unwrap_or_default();
            let url = CefString::from(&item.url()).to_string();
            self.shared.event(
                "Browser.downloadWillBegin",
                json!({ "frameId": frame, "guid": item.id().to_string(), "url": url, "suggestedFilename": name }),
            );
            1
        }

        fn on_download_updated(&self, _browser: Option<&mut Browser>, item: Option<&mut DownloadItem>, _callback: Option<&mut DownloadItemCallback>) {
            let Some(item) = item else {
                return;
            };
            let state = if item.is_complete() != 0 {
                "completed"
            } else if item.is_canceled() != 0 || item.is_interrupted() != 0 {
                "canceled"
            } else {
                return;
            };
            if !self.shared.pages.lock().unwrap().finished.insert(item.id()) {
                return;
            }
            self.shared.event(
                "Browser.downloadProgress",
                json!({ "guid": item.id().to_string(), "state": state,
                        "totalBytes": item.total_bytes(), "receivedBytes": item.received_bytes() }),
            );
        }
    }
}

wrap_dev_tools_message_observer! {
    struct Observer {
        shared: Arc<Shared>,
        token: u64,
    }

    impl DevToolsMessageObserver {
        fn on_dev_tools_message(&self, _browser: Option<&mut Browser>, message: Option<&[u8]>) -> ::std::os::raw::c_int {
            if let Some(message) = message {
                self.shared.heard(self.token, message);
            }
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_becomes_rgba_aligned_or_not() {
        let bgra = [1, 2, 3, 4, 5, 6, 7, 8, 9];
        assert_eq!(to_rgba(&bgra[..8]), [3, 2, 1, 4, 7, 6, 5, 8]);
        assert_eq!(to_rgba(&bgra[1..]), [4, 3, 2, 5, 8, 7, 6, 9]);
    }

    #[test]
    fn a_popup_is_drawn_over_the_page_where_it_is_and_clipped_to_it() {
        // A 3×2 page of zeros, a 2×2 popup of ones at (2, 1): one pixel of it is inside.
        let page = vec![0; 3 * 2 * 4];
        let popup = vec![1; 2 * 2 * 4];
        let out = over(&page, 3, Some((&popup, 2, [2, 1])));
        let lit: Vec<usize> = (0..6).filter(|i| out[i * 4] == 1).collect();
        assert_eq!(lit, [5]);
        assert_eq!(over(&page, 3, None), page);
        // Off the page to the left and above, the part inside still shows.
        let out = over(&page, 3, Some((&popup, 2, [-1, -1])));
        let lit: Vec<usize> = (0..6).filter(|i| out[i * 4] == 1).collect();
        assert_eq!(lit, [0]);
    }

    #[test]
    fn a_popup_s_part_inside_the_page_is_where_it_starts_in_each_and_its_size() {
        assert_eq!(clip([2, 1], [2, 2], [3, 2]), Some([[0, 0], [2, 1], [1, 1]]));
        assert_eq!(
            clip([-1, -1], [2, 2], [3, 2]),
            Some([[1, 1], [0, 0], [1, 1]])
        );
        assert_eq!(clip([0, 0], [5, 5], [3, 2]), Some([[0, 0], [0, 0], [3, 2]]));
        assert_eq!(clip([3, 0], [2, 2], [3, 2]), None);
        assert_eq!(clip([-2, 0], [2, 2], [3, 2]), None);
    }

    #[test]
    fn the_stream_reads_what_cef_sent_in_order_and_ends_with_it() {
        let (to_engine, from_cef) = std::sync::mpsc::channel();
        let mut incoming = Incoming {
            from: from_cef,
            left: Vec::new(),
            at: 0,
        };
        to_engine.send(b"{\"id\":1}\0".to_vec()).unwrap();
        to_engine.send(b"{\"id\":2}\0".to_vec()).unwrap();
        drop(to_engine);
        let mut read = Vec::new();
        let mut chunk = [0; 3];
        loop {
            match incoming.read(&mut chunk).unwrap() {
                0 => break,
                n => read.extend_from_slice(&chunk[..n]),
            }
        }
        assert_eq!(read, b"{\"id\":1}\0{\"id\":2}\0");
    }
}
