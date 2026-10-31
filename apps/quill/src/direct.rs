//! The browser's page presented straight to the compositor (docs/browser.md §8, "Direct"): a
//! Wayland subsurface of quill's own, beneath its window, shows a native tab's GPU paints in
//! buffers quill allocates (GBM), and the window is cleared to transparent where the page is
//! (`Punch`), so the page shows through and whatever quill draws there stays above it. A paint
//! reaches the screen without waiting for the window's next frame.

use eframe::egui;
use eframe::egui_wgpu::{self, RenderState};
use eframe::wgpu;
use std::os::fd::AsFd;
use std::sync::{Arc, Mutex};
use wayland_client::protocol::{
    wl_buffer::{self, WlBuffer},
    wl_compositor::WlCompositor,
    wl_registry::{self, WlRegistry},
    wl_subcompositor::WlSubcompositor,
    wl_subsurface::WlSubsurface,
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1::{self, ZwpLinuxBufferParamsV1},
    zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

/// BGRA in memory, as Chromium paints and wgpu's `Bgra8Unorm` holds it; opaque.
const DRM_FORMAT_XRGB8888: u32 = 0x3432_5258;
/// Paints in flight at most: one shown, one queued, one being drawn.
const BUFFERS: usize = 3;
/// How far the subsurface reaches past the hole under quill's opaque pixels, in logical
/// pixels: a compositor that rounds every surface's corners (Hyprland) rounds them out of
/// sight, not inside the hole.
const MARGIN: i32 = 32;

pub struct Direct {
    inner: Mutex<Inner>,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

struct Inner {
    connection: Connection,
    events: EventQueue<Wl>,
    wl: Wl,
    surface: WlSurface,
    subsurface: WlSubsurface,
    viewport: WpViewport,
    gbm: gbm::Device<std::fs::File>,
    buffers: Vec<Buffer>,
    /// Where the page is laid out; `None` while it is not on screen.
    at: Option<Place>,
    /// The window draws the page itself for now: compositors take a fullscreen window as
    /// opaque, and the page beneath it would not show through.
    window_draws: bool,
    /// The size in pixels of the paint last attached.
    painted: Option<[u32; 2]>,
    /// What of it the compositor shows, in the window's logical pixels: x, y, width and height.
    /// The window is punched there and nowhere else.
    shown: Option<[i32; 4]>,
    /// How far the subsurface reaches past it, left, top, right and bottom, in logical pixels.
    reach: [i32; 4],
    /// The border around the page in each buffer, in pixels.
    border: u32,
}

/// The page's place in the window, in logical pixels.
#[derive(Clone, Copy, PartialEq)]
struct Place {
    at: [i32; 2],
    /// The room limn laid out for the page, and the window's own size.
    room: [i32; 2],
    window: [i32; 2],
    scale: f32,
}

impl Inner {
    /// What of the last paint fits in its room and in the window, from its top left corner; a
    /// paint of the size before a resize is cut, never stretched.
    fn visible(&self) -> Option<[i32; 4]> {
        let place = self.at?;
        let painted = self.painted?;
        let fit = |axis: usize| {
            let paint = (painted[axis] as f32 / place.scale).floor() as i32;
            paint
                .min(place.room[axis])
                .min(place.window[axis] - place.at[axis])
        };
        let [width, height] = [fit(0), fit(1)];
        (width > 0 && height > 0).then_some([place.at[0], place.at[1], width, height])
    }

    /// Crop the paint to what is visible, reaching past it under quill wherever the window
    /// has room, or unmap it when nothing is visible; `true` if that changed what is shown,
    /// which takes a commit.
    fn show_visible(&mut self) -> bool {
        let visible = self.visible();
        let reach = match (visible, self.at) {
            (Some([x, y, width, height]), Some(place)) => [
                x,
                y,
                place.window[0] - x - width,
                place.window[1] - y - height,
            ]
            .map(|room| room.clamp(0, MARGIN)),
            _ => [0; 4],
        };
        if visible == self.shown && reach == self.reach {
            return false;
        }
        match visible {
            Some([x, y, width, height]) => {
                let [left, top, right, bottom] = reach;
                let scale = f64::from(self.at.map_or(1.0, |p| p.scale));
                let border = f64::from(self.border);
                let [w, h] = [left + width + right, top + height + bottom];
                self.subsurface.set_position(x - left, y - top);
                self.viewport.set_source(
                    border - f64::from(left) * scale,
                    border - f64::from(top) * scale,
                    f64::from(w) * scale,
                    f64::from(h) * scale,
                );
                self.viewport.set_destination(w, h);
            }
            None => {
                self.surface.attach(None, 0, 0);
                self.painted = None;
            }
        }
        self.shown = visible;
        self.reach = reach;
        true
    }
}

/// GBM's memory, seen by wgpu as a texture and by the compositor as a `wl_buffer`.
struct Buffer {
    _bo: gbm::BufferObject<()>,
    texture: wgpu::Texture,
    wl: WlBuffer,
    busy: bool,
}

#[derive(Default)]
struct Wl {
    compositor: Option<WlCompositor>,
    subcompositor: Option<WlSubcompositor>,
    dmabuf: Option<ZwpLinuxDmabufV1>,
    viewporter: Option<WpViewporter>,
    /// Buffers the compositor gave back, by index.
    released: Vec<usize>,
}

impl Direct {
    /// The subsurface under the window's `parent` surface, on the window's own connection
    /// (`display`), drawn by `render`'s device. Fails where any of it is missing; the page is
    /// then drawn by egui as before.
    pub fn new(
        display: *mut std::ffi::c_void,
        parent: *mut std::ffi::c_void,
        render: &RenderState,
    ) -> Result<Arc<Self>, String> {
        let backend =
            unsafe { wayland_backend::client::Backend::from_foreign_display(display.cast()) };
        let connection = Connection::from_backend(backend);
        let mut events = connection.new_event_queue();
        let qh = events.handle();
        let _registry = connection.display().get_registry(&qh, ());
        let mut wl = Wl::default();
        events.roundtrip(&mut wl).map_err(|e| e.to_string())?;
        let parent = unsafe {
            let id =
                wayland_backend::client::ObjectId::from_ptr(WlSurface::interface(), parent.cast())
                    .map_err(|e| e.to_string())?;
            WlSurface::from_id(&connection, id).map_err(|e| e.to_string())?
        };
        let missing = |name: &str| format!("the compositor has no {name}");
        let surface = (wl.compositor.as_ref())
            .ok_or_else(|| missing("wl_compositor"))?
            .create_surface(&qh, ());
        let subsurface = (wl.subcompositor.as_ref())
            .ok_or_else(|| missing("wl_subcompositor"))?
            .get_subsurface(&surface, &parent, &qh, ());
        let viewport = (wl.viewporter.as_ref())
            .ok_or_else(|| missing("wp_viewporter"))?
            .get_viewport(&surface, &qh, ());
        wl.dmabuf
            .as_ref()
            .ok_or_else(|| missing("zwp_linux_dmabuf_v1"))?;
        // Beneath the window, which shows it through a hole; its own frames, not the window's.
        subsurface.place_below(&parent);
        subsurface.set_desync();
        // Nothing of it takes the pointer: the window's own surface, above, gets it all.
        let empty = (wl.compositor.as_ref().unwrap()).create_region(&qh, ());
        surface.set_input_region(Some(&empty));
        empty.destroy();
        // The render node of the GPU wgpu draws with, which the compositor must read from.
        let bus = render.adapter.get_info().device_pci_bus_id;
        let node = format!("/dev/dri/by-path/pci-{bus}-render");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&node)
            .map_err(|e| format!("{node}: {e}"))?;
        let gbm = gbm::Device::new(file).map_err(|e| e.to_string())?;
        connection.flush().map_err(|e| e.to_string())?;
        Ok(Arc::new(Self {
            inner: Mutex::new(Inner {
                connection,
                events,
                wl,
                surface,
                subsurface,
                viewport,
                gbm,
                buffers: Vec::new(),
                at: None,
                window_draws: false,
                painted: None,
                shown: None,
                reach: [0; 4],
                border: 0,
            }),
            device: render.device.clone(),
            queue: render.queue.clone(),
        }))
    }

    /// Where the page is laid out this frame, in the window's logical pixels, in a window of
    /// `window` logical pixels at `scale`; `None` when it is not on screen. A move lands with
    /// the window's commit of this same frame. While the window is `fullscreen` it draws the
    /// page itself (`Direct::shows`).
    pub fn place(
        &self,
        room: Option<egui::Rect>,
        window: egui::Vec2,
        scale: f32,
        fullscreen: bool,
    ) {
        let mut inner = self.inner.lock().unwrap();
        inner.window_draws = fullscreen;
        let room = room.filter(|_| !fullscreen);
        let at = room.map(|r| {
            let [x, y] = [r.min.x.round() as i32, r.min.y.round() as i32];
            Place {
                at: [x, y],
                room: [r.max.x.round() as i32 - x, r.max.y.round() as i32 - y],
                window: [window.x.floor() as i32, window.y.floor() as i32],
                scale,
            }
        });
        if inner.at == at {
            return;
        }
        inner.at = at;
        if inner.show_visible() {
            inner.surface.commit();
        }
        let _ = inner.connection.flush();
    }

    /// Whether the page is presented beneath the window, which then punches a hole for it;
    /// otherwise the window draws it as a texture. Only a paint made on the GPU reaches the
    /// compositor: when Chromium paints on the CPU (its GPU process could not start), nothing
    /// is attached here and the window draws those paints.
    pub fn shows(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        !inner.window_draws && inner.painted.is_some()
    }

    /// What the compositor shows of the page, in the window's logical pixels.
    fn shown(&self) -> Option<[i32; 4]> {
        self.inner.lock().unwrap().shown
    }

    /// Show `page`, a tab's texture just painted; `false` if it could not be, so the window
    /// draws it instead. Called on CEF's thread.
    pub fn present(&self, page: &wgpu::Texture) -> bool {
        let mut guard = self.inner.lock().unwrap();
        let inner = &mut *guard;
        if inner.window_draws {
            return false;
        }
        let Some(place) = inner.at else {
            // Not on screen: nothing to show, and nothing for the window to draw either.
            return true;
        };
        if inner.events.dispatch_pending(&mut inner.wl).is_err() {
            return false;
        }
        for index in inner.wl.released.drain(..) {
            if let Some(buffer) = inner.buffers.get_mut(index) {
                buffer.busy = false;
            }
        }
        let size = (page.width(), page.height());
        let border = (MARGIN as f32 * place.scale).ceil() as u32;
        let buffer_size = (size.0 + 2 * border, size.1 + 2 * border);
        let sized = inner
            .buffers
            .first()
            .map(|b| (b.texture.width(), b.texture.height()));
        if sized != Some(buffer_size) {
            inner.border = border;
            match allocate(inner, &self.device, buffer_size) {
                Ok(buffers) => inner.buffers = buffers,
                Err(error) => {
                    eprintln!("quill: the page is drawn by the window: {error}");
                    return false;
                }
            }
        }
        // shortcut: all three still with the compositor means it is behind; this paint is
        // dropped, and the next one shows. A paint that is the last for a while would be lost.
        let Some(index) = inner.buffers.iter().position(|b| !b.busy) else {
            return true;
        };
        let buffer = &mut inner.buffers[index];
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_texture(
            page.as_image_copy(),
            wgpu::TexelCopyTextureInfo {
                texture: &buffer.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: border,
                    y: border,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            page.size(),
        );
        let copied = self.queue.submit([encoder.finish()]);
        // The compositor reads the buffer as soon as it is committed.
        let _ = self.device.poll(wgpu::PollType::Wait {
            submission_index: Some(copied),
            timeout: Some(std::time::Duration::from_secs(1)),
        });
        inner.painted = Some([size.0, size.1]);
        if inner.visible().is_none() {
            // No room for it: a buffer attached and taken back before a commit would never be
            // released.
            return true;
        }
        let buffer = &mut inner.buffers[index];
        inner.surface.attach(Some(&buffer.wl), 0, 0);
        inner
            .surface
            .damage_buffer(0, 0, buffer_size.0 as i32, buffer_size.1 as i32);
        buffer.busy = true;
        inner.show_visible();
        inner.surface.commit();
        inner.connection.flush().is_ok()
    }
}

/// Three buffers of `width` by `height` pixels: GBM's linear memory, imported into `device` to
/// copy into, and handed to the compositor as dmabufs.
fn allocate(
    inner: &mut Inner,
    device: &wgpu::Device,
    (width, height): (u32, u32),
) -> Result<Vec<Buffer>, String> {
    use wgpu::hal::api::Vulkan;
    let qh = inner.events.handle();
    let dmabuf = inner.wl.dmabuf.as_ref().ok_or("no zwp_linux_dmabuf_v1")?;
    let mut buffers = Vec::with_capacity(BUFFERS);
    for index in 0..BUFFERS {
        let bo = inner
            .gbm
            .create_buffer_object::<()>(
                width,
                height,
                gbm::Format::Xrgb8888,
                gbm::BufferObjectFlags::RENDERING | gbm::BufferObjectFlags::LINEAR,
            )
            .map_err(|e| e.to_string())?;
        let fd = bo.fd_for_plane(0).map_err(|e| format!("{e:?}"))?;
        let (stride, offset) = (bo.stride_for_plane(0), bo.offset(0));
        // Asked for LINEAR by flag, GBM names no modifier (DRM_FORMAT_MOD_INVALID).
        let modifier = match u64::from(bo.modifier()) {
            0x00ff_ffff_ffff_ffff => 0,
            modifier => modifier,
        };
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let hal = unsafe {
            let hal_device = device.as_hal::<Vulkan>().ok_or("not a Vulkan device")?;
            let desc = wgpu::hal::TextureDescriptor {
                label: Some("direct page"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8Unorm,
                usage: wgpu::TextureUses::COPY_DST,
                memory_flags: wgpu::hal::MemoryFlags::empty(),
                view_formats: Vec::new(),
            };
            let fd = fd.try_clone().map_err(|e| e.to_string())?;
            hal_device
                .texture_from_dmabuf_fd(fd, &desc, modifier, stride.into(), offset.into())
                .map_err(|e| e.to_string())?
        };
        let texture = unsafe {
            device.create_texture_from_hal::<Vulkan>(
                hal,
                &wgpu::TextureDescriptor {
                    label: Some("direct page"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    usage: wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::TextureUses::UNINITIALIZED,
            )
        };
        let params = dmabuf.create_params(&qh, ());
        params.add(
            fd.as_fd(),
            0,
            offset,
            stride,
            (modifier >> 32) as u32,
            modifier as u32,
        );
        let wl = params.create_immed(
            width as i32,
            height as i32,
            DRM_FORMAT_XRGB8888,
            zwp_linux_buffer_params_v1::Flags::empty(),
            &qh,
            index,
        );
        params.destroy();
        buffers.push(Buffer {
            _bo: bo,
            texture,
            wl,
            busy: false,
        });
    }
    Ok(buffers)
}

/// The paint limn adds where the page is: the window cleared to transparent where the
/// compositor shows the page, exactly, replacing what is under it rather than blending over it.
/// Where the page does not reach (a paint still of the size before a resize), quill's own
/// pixels stay, never the desktop.
pub fn punch(direct: &Arc<Direct>) -> egui::PaintCallback {
    egui_wgpu::Callback::new_paint_callback(egui::Rect::NOTHING, Punch(direct.clone()))
}

/// Keeps egui's target format for the punch's pipeline; call it once the window is made.
pub fn prepare(render: &RenderState) {
    (render.renderer.write().callback_resources).insert(Target(render.target_format));
}

struct Punch(Arc<Direct>);
struct Target(wgpu::TextureFormat);
struct PunchPipeline(wgpu::RenderPipeline);

impl egui_wgpu::CallbackTrait for Punch {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if resources.get::<PunchPipeline>().is_none() {
            let format = resources
                .get::<Target>()
                .map_or(wgpu::TextureFormat::Bgra8Unorm, |t| t.0);
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("punch"),
                source: wgpu::ShaderSource::Wgsl(
                    "@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
                        let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
                        return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
                    }
                    @fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }"
                        .into(),
                ),
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("punch"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    // No blending: the window's pixels there become transparent.
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            });
            resources.insert(PunchPipeline(pipeline));
        }
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let (Some(PunchPipeline(pipeline)), Some([x, y, width, height])) =
            (resources.get(), self.0.shown())
        else {
            return;
        };
        // The page's pixels, as the compositor places them, within what limn lets this paint.
        let scale = info.pixels_per_point;
        let px = |v: i32| (v as f32 * scale).round() as i32;
        let clip = info.clip_rect_in_pixels();
        let left = px(x).max(clip.left_px);
        let top = px(y).max(clip.top_px);
        let right = px(x + width).min(clip.left_px + clip.width_px);
        let bottom = px(y + height).min(clip.top_px + clip.height_px);
        if right <= left || bottom <= top {
            return;
        }
        pass.set_scissor_rect(
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        );
        pass.set_pipeline(pipeline);
        pass.draw(0..3, 0..1);
    }
}

impl Dispatch<WlRegistry, ()> for Wl {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version.min(4), qh, ()))
                }
                "wl_subcompositor" => state.subcompositor = Some(registry.bind(name, 1, qh, ())),
                "zwp_linux_dmabuf_v1" => {
                    state.dmabuf = Some(registry.bind(name, version.min(3), qh, ()))
                }
                "wp_viewporter" => state.viewporter = Some(registry.bind(name, 1, qh, ())),
                _ => {}
            }
        }
    }
}

impl Dispatch<WlBuffer, usize> for Wl {
    fn event(
        state: &mut Self,
        _: &WlBuffer,
        event: wl_buffer::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            state.released.push(*index);
        }
    }
}

/// Objects whose events quill has no use for.
macro_rules! ignore {
    ($($t:ty),*) => {$(
        impl Dispatch<$t, ()> for Wl {
            fn event(_: &mut Self, _: &$t, _: <$t as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore!(
    WlCompositor,
    WlSubcompositor,
    WlSurface,
    WlSubsurface,
    ZwpLinuxDmabufV1,
    ZwpLinuxBufferParamsV1,
    WpViewporter,
    WpViewport,
    wayland_client::protocol::wl_region::WlRegion
);
