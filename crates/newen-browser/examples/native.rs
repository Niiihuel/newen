//! The native view end to end, as `tests/chromium.rs` checks the headless one: CEF can only
//! run in a program of its own (its processes are this one run again), so it is an example.
//! `cargo run -p quill-browser --example native [out-dir]` from shell.nix; it saves the frames
//! it saw as PPM in `out-dir` when given one. The frames stay on a GPU device of its own, as
//! they do on the window's, where its driver allows; `NEWEN_NO_GPU=1` checks the CPU's path.

use newen_browser::{Browser, Frame, Frames, Grant, Native, Options, Permit, Pixels};
use newen_model::{BrowserInput, BrowserOp, BrowserUpdate, BrowserView, Modifiers, PointerButton};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PAGE: &str = "<!doctype html><title>Acme</title><body style='margin:0;background:#fff'>\
<input id=email style='position:absolute;left:10px;top:10px;width:200px;height:30px'>\
<select id=pick style='position:absolute;left:10px;top:60px;width:150px'>\
<option>one<option>two<option>three</select>\
<p style='position:absolute;top:100px;left:10px;font:20px sans-serif'>hello quill</p></body>";

fn main() {
    if let Some(code) = newen_browser::native::subprocess() {
        std::process::exit(code);
    }
    let out = std::env::args().nth(1).map(std::path::PathBuf::from);
    let dir = std::env::temp_dir().join(format!("quill-native-{}", std::process::id()));
    let frames = Frames::default();
    frames.set_scale(2.0);
    let gpu = std::env::var_os("NEWEN_NO_GPU")
        .is_none()
        .then(gpu)
        .flatten();
    println!("frames on the GPU: {}", gpu.is_some());
    let native = Native::start(&dir.join("cef"), frames.clone(), gpu.clone()).expect("CEF starts");
    let run = {
        let (native, dir) = (native.clone(), dir.clone());
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(check(native, frames, dir, out, gpu))
        })
    };
    let result = run.join();
    native.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    if result.is_err() {
        std::process::exit(1);
    }
    println!("native view: ok");
}

async fn check(
    native: Native,
    frames: Frames,
    dir: std::path::PathBuf,
    out: Option<std::path::PathBuf>,
    gpu: Gpu,
) {
    let rgba = |frame: &Frame| rgba(frame, &gpu);
    let heard: Arc<Mutex<Vec<(u64, BrowserUpdate)>>> = Arc::default();
    let log = heard.clone();
    let allow: Permit = Arc::new(|_, _| {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = tx.send(Grant::Once);
        rx
    });
    let browser = Browser::new(
        Options {
            program: None,
            profile: dir.join("profile"),
            downloads: dir.join("downloads"),
            frames: frames.clone(),
            permit: allow,
            native: Some(native),
        },
        Arc::new(move |owner, update| log.lock().unwrap().push((owner, update))),
    );
    let port = serve();
    browser
        .apply(
            7,
            BrowserOp::Open {
                url: Some(format!("localhost:{port}/")),
            },
        )
        .await;
    let tab = until("the page's title", || {
        heard
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|(_, update)| match update {
                BrowserUpdate::Tabs { tabs, .. } => {
                    tabs.iter().find(|t| t.title == "Acme").map(|t| t.id)
                }
                _ => None,
            })
    })
    .await;
    let view = BrowserView {
        tab,
        width: 400,
        height: 300,
    };
    browser.apply(7, BrowserOp::Show { view: Some(view) }).await;
    let surface = format!("browser:7:{tab}");
    // Painted at the pane's size and the window's scale: twice the CSS pixels each way.
    let frame = until("a frame at the pane's size", || {
        frames
            .take()
            .into_iter()
            .find(|(id, f)| *id == surface && f.width == 800)
            .map(|(_, f)| f)
    })
    .await;
    let pixels = rgba(&frame);
    save(&out, "page", &frame, &pixels);
    assert_eq!((frame.height, frame.scale), (600, 2.0));
    // The page's own white, and the text's dark pixels somewhere on its line.
    assert_eq!(&pixels[..4], &[255, 255, 255, 255]);
    let line = &pixels[frame.width * 4 * 210..frame.width * 4 * 250];
    assert!(line.chunks(4).any(|p| p[0] < 100), "the text is painted");
    // The pane's clicks and keys reach it, as the headless one's do.
    let keys = Modifiers::default();
    let click = |x: f32, y: f32| {
        [
            BrowserInput::Down {
                x,
                y,
                button: PointerButton::Left,
                clicks: 1,
                keys,
            },
            BrowserInput::Up {
                x,
                y,
                button: PointerButton::Left,
                keys,
            },
        ]
    };
    for input in click(50.0, 25.0)
        .into_iter()
        .chain([BrowserInput::Text("alex@acme.test".into())])
    {
        browser.apply(7, BrowserOp::Input { tab, input }).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let typed = browser.evaluate(7, tab, "email.value").await.unwrap();
    assert_eq!(typed.as_str(), Some("alex@acme.test"));
    // A <select>'s list is painted apart and drawn over the page, below the field.
    for input in click(50.0, 70.0) {
        browser.apply(7, BrowserOp::Input { tab, input }).await;
    }
    let (open, pixels) = until("the list over the page", || {
        frames.take().into_iter().find_map(|(id, f)| {
            let pixels = rgba(&f);
            (id == surface && pixels[(f.width * 2 * 100 + 2 * 30) * 4..][..3] != [255, 255, 255])
                .then_some((f, pixels))
        })
    })
    .await;
    save(&out, "select", &open, &pixels);
    // A size the agent asks for is the one painted.
    browser
        .apply(
            7,
            BrowserOp::Show {
                view: Some(BrowserView { width: 300, ..view }),
            },
        )
        .await;
    let frame = until("the narrower frame", || {
        frames
            .take()
            .into_iter()
            .find(|(id, f)| *id == surface && f.width == 600)
            .map(|(_, f)| f)
    })
    .await;
    assert_eq!(frame.height, 600);
    // The wheel scrolls it, a pane's size at a time.
    browser
        .evaluate(7, tab, "document.body.style.height='20000px'")
        .await
        .unwrap();
    for _ in 0..10 {
        let input = BrowserInput::Wheel {
            x: 100.0,
            y: 100.0,
            dx: 0.0,
            dy: 100.0,
        };
        browser.apply(7, BrowserOp::Input { tab, input }).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let y = browser.evaluate(7, tab, "scrollY").await.unwrap();
    assert!(
        y.as_f64().is_some_and(|y| y > 0.0),
        "the wheel scrolls: {y}"
    );
    latency(&browser, &frames, tab, &surface).await;
    browser.shutdown().await;
}

fn save(out: &Option<std::path::PathBuf>, name: &str, frame: &Frame, rgba: &[u8]) {
    if let Some(out) = out {
        let mut ppm = format!("P6\n{} {}\n255\n", frame.width, frame.height).into_bytes();
        ppm.extend(rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]));
        std::fs::write(out.join(format!("{name}.ppm")), ppm).unwrap();
    }
}

type Gpu = Option<(wgpu::Device, wgpu::Queue)>;

/// A Vulkan device that can import Chromium's buffers, as quill asks eframe for; `None`
/// without one.
fn gpu() -> Gpu {
    let wanted = wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let wait = tokio::runtime::Runtime::new().unwrap();
    let adapter = wait
        .block_on(instance.request_adapter(&Default::default()))
        .ok()?;
    let descriptor = wgpu::DeviceDescriptor {
        required_features: adapter.features() & wanted,
        ..Default::default()
    };
    let (device, queue) = wait.block_on(adapter.request_device(&descriptor)).ok()?;
    device
        .features()
        .contains(wanted)
        .then_some((device, queue))
}

/// A frame's pixels as RGBA rows, read back from the GPU when they are there.
fn rgba(frame: &Frame, gpu: &Gpu) -> Vec<u8> {
    let texture = match &frame.pixels {
        Pixels::Rgba(rgba) => return rgba.clone(),
        Pixels::Texture(texture) => texture,
    };
    let (device, queue) = gpu.as_ref().expect("a texture is of a device");
    let width = texture.width() as usize * 4;
    let row = width.next_multiple_of(256);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * texture.height() as usize) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    let layout = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(row as u32),
        rows_per_image: None,
    };
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout,
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    // BGRA rows, each padded to 256 bytes.
    mapped
        .chunks(row)
        .flat_map(|line| {
            line[..width]
                .chunks(4)
                .flat_map(|p| [p[2], p[1], p[0], p[3]])
        })
        .collect()
}

/// A one-page server on a free local port.
fn serve() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{PAGE}",
                PAGE.len()
            );
        }
    });
    port
}

async fn until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "waited too long for {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// How long a wheel step takes to reach the surface as a new frame, at a real pane's size:
/// printed, as what it measures depends on the machine.
async fn latency(browser: &Browser, frames: &Frames, tab: u32, surface: &str) {
    let stripes = "document.body.style.background=\
        'repeating-linear-gradient(#fff 0 20px, #36c 20px 40px)'";
    browser.evaluate(7, tab, stripes).await.unwrap();
    let view = BrowserView {
        tab,
        width: 800,
        height: 1100,
    };
    browser.apply(7, BrowserOp::Show { view: Some(view) }).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    frames.take();
    let mut took = Vec::new();
    for _ in 0..30 {
        let input = BrowserInput::Wheel {
            x: 100.0,
            y: 100.0,
            dx: 0.0,
            dy: 30.0,
        };
        let sent = Instant::now();
        eprintln!(
            "T sent {}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros()
        );
        browser.apply(7, BrowserOp::Input { tab, input }).await;
        while sent.elapsed() < Duration::from_secs(2) {
            if frames.take().iter().any(|(id, _)| id == surface) {
                eprintln!(
                    "T got {}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_micros()
                );
                took.push(sent.elapsed());
                break;
            }
            tokio::time::sleep(Duration::from_micros(500)).await;
        }
        // Whatever else the step painted, before the next one.
        tokio::time::sleep(Duration::from_millis(100)).await;
        frames.take();
    }
    // A steady scroll, a step every 8 ms (a 120 Hz trackpad) for a second.
    let (start, mut shown) = (Instant::now(), 0);
    while start.elapsed() < Duration::from_secs(1) {
        let input = BrowserInput::Wheel {
            x: 100.0,
            y: 100.0,
            dx: 0.0,
            dy: 10.0,
        };
        browser.apply(7, BrowserOp::Input { tab, input }).await;
        tokio::time::sleep(Duration::from_millis(8)).await;
        shown += frames.take().iter().filter(|(id, _)| id == surface).count();
    }
    println!("a steady scroll: {shown} frames in a second");
    took.sort();
    let at = |q: usize| took.get(took.len() * q / 100).copied().unwrap_or_default();
    println!(
        "wheel to frame, {} of 30: median {:?}, p90 {:?}, max {:?}",
        took.len(),
        at(50),
        at(90),
        took.last().copied().unwrap_or_default()
    );
}
