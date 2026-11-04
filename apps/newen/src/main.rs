//! Newen's window: limn paints newen-ui's view; newen-engine runs the agents.
//! `newen [folder]` opens new sessions in `folder` (the current directory without one).

// A window, not a console program: Windows opens no console beside it (`--install` prints
// nothing there).
#![cfg_attr(windows, windows_subsystem = "windows")]

// The page presented beneath the window is Wayland's (dmabufs, a subsurface); elsewhere egui
// draws it and this stand-in never shows.
#[cfg(target_os = "linux")]
mod direct;
#[cfg(not(target_os = "linux"))]
mod direct {
    use std::sync::Arc;

    pub struct Direct;

    impl Direct {
        pub fn place(&self, _: Option<eframe::egui::Rect>, _: eframe::egui::Vec2, _: f32, _: bool) {
        }
        pub fn shows(&self) -> bool {
            false
        }
    }

    pub fn punch(_: &Arc<Direct>) -> eframe::egui::PaintCallback {
        unreachable!("a stand-in that shows nothing is never punched")
    }
}
mod editors;
mod images;
mod page;
mod video;

use eframe::egui;
use limn::{Element, Size, UiTree};
use newen_engine::Engine;
use newen_model::{Event, Intent, SessionId};
use newen_ui::{Pane, Store, TerminalRequest, Ui};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// An annotated copy and the picture it replaces: `None` when the copy could not be written.
type AnnotatedCopy = (PathBuf, Option<PathBuf>);
/// An image and its size in pixels, read off the window's thread.
type ImageSize = (PathBuf, (f32, f32));

struct App {
    tree: UiTree,
    store: Store,
    ui: Ui,
    engine: Engine,
    /// Events the engine reported since the last frame.
    inbox: Arc<Mutex<Vec<Event>>>,
    /// The folder the person chose in the platform's dialog, picked up on the next frame.
    picked: Arc<Mutex<Option<PathBuf>>>,
    /// The Chromium chosen in Settings › Browser, kept on the next frame.
    chromium: Arc<Mutex<Option<PathBuf>>>,
    /// A skill's file chosen in Customize, for what it was asked for, sent on the next frame.
    skill_file: Arc<Mutex<Option<(String, PathBuf)>>>,
    /// Files chosen for a page's file field, sent on the next frame.
    chosen: Arc<Mutex<Option<(SessionId, newen_model::BrowserOp)>>>,
    /// Files or a folder chosen from "+", to go into the message as @-mentions.
    attached: Arc<Mutex<Vec<PathBuf>>>,
    /// Images pasted from the clipboard, saved, waiting to join the composer's.
    pasted: Arc<Mutex<Vec<PathBuf>>>,
    /// The engine has retold the kept sessions; events from here on are news.
    restored: bool,
    /// Whether the window had the focus at the last frame, as the engine was told.
    focused: Option<bool>,
    /// Thumbnails decoded off the window's thread, and the images asked for so far (each once).
    thumbs_done: Arc<Mutex<Vec<(PathBuf, limn::RasterImage)>>>,
    thumbs_asked: std::collections::HashSet<PathBuf>,
    /// The link whose QR code was last drawn, so a link is drawn once.
    qr_for: Option<String>,
    /// The viewer's full-size pictures, decoded like the thumbnails, and the images asked for.
    full_done: Arc<Mutex<Vec<(PathBuf, limn::RasterImage)>>>,
    full_asked: std::collections::HashSet<PathBuf>,
    /// The clip of a release's announcement and Cisco's codec that plays it (video.rs).
    clips: video::Clips,
    /// The width over height of the picture the editor lays out, read from its file off the
    /// window's thread, and the images asked for so far.
    sizes_done: Arc<Mutex<Vec<ImageSize>>>,
    sizes_asked: std::collections::HashSet<PathBuf>,
    /// Annotated copies made off the window's thread, each with the picture it replaces; `None`
    /// when the copy could not be written.
    annotated: Arc<Mutex<Vec<AnnotatedCopy>>>,
    /// The terminal tabs of each folder, kept while the window lives.
    terminals: HashMap<PathBuf, newen_pty::Tabs>,
    fonts_attached: bool,
    /// The interface font and code font in force, as Settings chose them.
    fonts_in_force: (newen_model::InterfaceFont, String),
    /// The faces being built for Settings' fonts, taken on a frame once they are ready.
    fonts_coming:
        Option<std::sync::mpsc::Receiver<((newen_model::InterfaceFont, String), limn::Fonts)>>,
    /// The session on screen and how much its transcript held last frame.
    transcript: Option<(SessionId, usize, usize)>,
    /// The same of the session beside it, in the side pane.
    beside: Option<(SessionId, usize, usize)>,
    /// The scroll area of the helper agent's transcript shown last frame.
    helper: Option<String>,
    /// The motion Settings asked for that is in force.
    applied: Option<newen_model::Motion>,
    /// The theme the window's icon was last drawn for.
    icon_dark: Option<bool>,
    /// `NEWEN_THEME` pins the theme over Settings, for demos and captures.
    pinned_theme: bool,
    /// Where the browser leaves its pages' frames, presented each frame.
    frames: newen_engine::Frames,
    /// The page presented beneath the window, where the compositor allows (direct.rs).
    direct: Option<Arc<direct::Direct>>,
    /// Chromium in Newen's process, whose loop the window pumps on macOS.
    native: Option<newen_engine::Native>,
    /// The texture each surface shows from the GPU, as registered with egui's renderer.
    textures: HashMap<String, (egui::TextureId, eframe::wgpu::Texture)>,
    /// The page's share of the window's input.
    page: page::Page,
    /// The tab on screen and its size, as last told to the engine.
    browser_shown: Option<(SessionId, newen_model::BrowserView)>,
    /// What the address bar was last set to.
    address: Option<String>,
    /// Messages from newen-ide since the last frame, and the socket they come on while Newen
    /// listens on it.
    linked: Arc<Mutex<Vec<newen_link::Message>>>,
    _link: Option<newen_link::Listener>,
    /// The editors installed, once they are looked for off the window's thread, and the
    /// command each runs a link with, by scheme.
    editors: Arc<Mutex<Option<Vec<editors::Found>>>>,
    handlers: HashMap<String, Vec<String>>,
    /// When Settings' "Archive inactive sessions" was last looked at.
    archive_checked: Option<std::time::Instant>,
}

impl App {
    /// Start, size and read the shells of the folder on screen while the pane is open, and do
    /// what the person asked of them.
    fn sync_terminal(&mut self, ctx: &egui::Context, window: Size) {
        let folder = newen_ui::folder(&self.store, &self.ui);
        let (cols, rows) = newen_ui::view::terminal_grid(&self.store, &self.ui, window);
        if !self.ui.pane_open(Pane::Terminal) {
            self.ui.terminal.screen = None;
            self.ui.terminal.requests.clear();
            return;
        }
        let pane = &mut self.ui.terminal;
        pane.screen = None;
        let requests = std::mem::take(&mut pane.requests);
        let tabs = self.terminals.entry(folder.clone()).or_default();
        let had = !tabs.is_empty();
        let wake = || {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        for request in requests {
            match request {
                TerminalRequest::New => {
                    if let Err(error) = tabs.open(&folder, cols, rows, wake()) {
                        eprintln!("newen: no terminal in {}: {error}", folder.display());
                    }
                }
                TerminalRequest::Select(tab) => tabs.select(tab),
                TerminalRequest::Close(tab) => tabs.close(tab),
                TerminalRequest::CloseOthers(tab) => tabs.close_others(tab),
                TerminalRequest::Rename(tab, name) => tabs.rename(tab, name),
                TerminalRequest::Clear(tab) => tabs.clear(tab),
                TerminalRequest::Move { from, to } => tabs.move_tab(from, to),
                // The output of a command the agent left running, followed as it grows.
                TerminalRequest::Output { name, path } => {
                    let path = path.display().to_string();
                    let follow: Vec<String> = if cfg!(windows) {
                        let quoted = path.replace('\'', "''");
                        ["powershell", "-NoProfile", "-Command"]
                            .map(String::from)
                            .into_iter()
                            .chain([format!("Get-Content -Wait -Path '{quoted}'")])
                            .collect()
                    } else {
                        ["tail", "-n", "+1", "-F"]
                            .map(String::from)
                            .into_iter()
                            .chain([path])
                            .collect()
                    };
                    let opened =
                        tabs.open_running(&folder, &follow, Some(name), cols, rows, wake());
                    if let Err(error) = opened {
                        eprintln!("newen: no terminal for that output: {error}");
                    }
                }
            }
        }
        tabs.reap();
        if tabs.is_empty() && !had {
            if let Err(error) = tabs.open(&folder, cols, rows, wake()) {
                eprintln!("newen: no terminal in {}: {error}", folder.display());
            }
        }
        // Closing the last tab, or `exit` in it, hides the pane; opening it again starts fresh.
        let Some(terminal) = tabs.active_mut() else {
            pane.tabs.clear();
            self.ui.close_pane(Pane::Terminal);
            return;
        };
        terminal.resize(cols, rows);
        pane.screen = Some(terminal.screen());
        pane.tabs = tabs
            .list()
            .into_iter()
            .map(|tab| newen_ui::TerminalTab {
                title: tab.title,
                busy: tab.busy,
                named: tab.named,
            })
            .collect();
        pane.active = tabs.active();
        pane.closing = pane.closing.filter(|closing| match closing {
            newen_ui::Closing::One(tab) | newen_ui::Closing::Others(tab) => *tab < pane.tabs.len(),
        });
    }

    /// Keys typed while the terminal has the focus go to its shell, not to the window.
    fn typed_into_terminal(&mut self, event: &limn::UiEvent) -> bool {
        if self.tree.focused_id() != Some(newen_ui::view::TERMINAL) {
            return false;
        }
        let folder = newen_ui::folder(&self.store, &self.ui);
        let terminal = self.terminals.get_mut(&folder).and_then(|t| t.active_mut());
        if let limn::UiEvent::Paste(text) = event {
            if let Some(terminal) = terminal {
                terminal.paste(text);
            }
            return true;
        }
        let bytes = match event {
            limn::UiEvent::Text(text) => text.as_bytes().to_vec(),
            limn::UiEvent::Key {
                key, shift, ctrl, ..
            } => {
                match newen_pty::key_bytes(*key, *ctrl, *shift) {
                    Some(bytes) => bytes,
                    // Plain letters arrive again as Text.
                    None => return true,
                }
            }
            _ => return false,
        };
        if let Some(terminal) = terminal {
            terminal.write(&bytes);
        }
        true
    }

    /// The page's rectangle in the window and whether it is on top under each position:
    /// what of this frame's input is the page's.
    fn route_page(
        &mut self,
        ctx: &egui::Context,
        origin: egui::Pos2,
    ) -> (Vec<page::Ask>, Vec<usize>) {
        let view = newen_ui::view::BROWSER_VIEW;
        let frame = self.tree.frame();
        let rect = |r: limn::Rect| {
            egui::Rect::from_min_size(origin + egui::vec2(r.x, r.y), egui::vec2(r.width, r.height))
        };
        let page = frame
            .regions
            .iter()
            .find(|r| r.id == view)
            .map(|r| rect(r.rect));
        let over = |pos: egui::Pos2| {
            let at = limn::Point::new(pos.x - origin.x, pos.y - origin.y);
            frame
                .press_region(at)
                .filter(|r| r.id == view)
                .map(|r| rect(r.rect))
        };
        let focused = self.tree.focused_id() == Some(view);
        let events = ctx.input(|i| i.events.clone());
        self.page.route(&events, over, page, focused)
    }

    /// Carry out what the page asked; the browser's buttons are clicked as the person would.
    fn page_asked(&mut self, ask: page::Ask, window: Size) -> Vec<limn::UiAction> {
        use newen_ui::view::*;
        use page::Ask;
        let clicked = |id: String| vec![limn::UiAction::Clicked { id }];
        let Some(session) = self.ui.selected else {
            return vec![];
        };
        let browser = self
            .store
            .session(session)
            .map(|s| s.browser.clone())
            .unwrap_or_default();
        let Some(tab) = browser.active else {
            return match ask {
                Ask::ToggleBrowser => clicked(BROWSER_TOGGLE.into()),
                _ => vec![],
            };
        };
        let send = |op| Intent::Browser { session, op };
        match ask {
            Ask::Input(input) => {
                self.dispatch(vec![send(newen_model::BrowserOp::Input { tab, input })]);
                vec![]
            }
            Ask::Copy { cut } => {
                let mut ops = vec![send(newen_model::BrowserOp::Copy { tab })];
                if cut {
                    ops.push(send(newen_model::BrowserOp::Input {
                        tab,
                        input: newen_model::BrowserInput::Key {
                            key: "Delete".into(),
                            code: "Delete".into(),
                            key_code: 46,
                            text: String::new(),
                            keys: Default::default(),
                        },
                    }));
                }
                self.dispatch(ops);
                vec![]
            }
            Ask::Focus => {
                self.tree.focus_from_pointer(BROWSER_VIEW);
                vec![]
            }
            Ask::FocusAddress => {
                self.tree.layout(window);
                self.tree.focus(BROWSER_URL);
                self.tree.handle_event(limn::UiEvent::Key {
                    key: limn::Key::A,
                    ctrl: true,
                    shift: false,
                    alt: false,
                });
                vec![]
            }
            Ask::NewTab => clicked(BROWSER_NEW_TAB.into()),
            Ask::CloseTab => clicked(format!("browser:close:{tab}")),
            Ask::Reload => clicked(BROWSER_RELOAD.into()),
            Ask::ToggleBrowser => clicked(BROWSER_TOGGLE.into()),
            Ask::NextTab(forward) => {
                let at = browser.tabs.iter().position(|t| t.id == tab).unwrap_or(0);
                let count = browser.tabs.len().max(1);
                let next = if forward {
                    (at + 1) % count
                } else {
                    (at + count - 1) % count
                };
                browser
                    .tabs
                    .get(next)
                    .map(|t| clicked(format!("browser:tab:{}", t.id)))
                    .unwrap_or_default()
            }
        }
    }

    /// The address bar shows the tab's address unless the person is typing in it.
    fn sync_address(&mut self) {
        if self.tree.focused_id() == Some(newen_ui::view::BROWSER_URL) {
            return;
        }
        let tab = self
            .ui
            .selected
            .and_then(|s| self.store.session(s))
            .and_then(|s| s.browser.tab().map(|t| (t.id, t.url.clone())));
        let url = match &tab {
            Some((_, url)) if url != "about:blank" => url.clone(),
            _ => String::new(),
        };
        if self.address.as_ref() != Some(&url) {
            self.tree
                .set_input_value(newen_ui::view::BROWSER_URL, url.clone());
            // An address longer than the field shows its start, as a browser's does.
            self.tree.set_input_cursor(newen_ui::view::BROWSER_URL, 0);
            self.address = Some(url);
        }
    }

    /// After painting: the engine hears which tab is on screen and at what size, the pointer
    /// takes the page's shape over it, and an input method follows the page's focus.
    fn page_frame(&mut self, ctx: &egui::Context, frame: &limn::Frame, origin: egui::Pos2) {
        let view = newen_ui::view::BROWSER_VIEW;
        let region = frame.regions.iter().find(|r| r.id == view).map(|r| r.rect);
        let shown = (|| {
            let session = self.ui.selected?;
            let tab = self.store.session(session)?.browser.active?;
            let rect = region.filter(|_| self.ui.pane_open(Pane::Browser))?;
            Some((
                session,
                newen_model::BrowserView {
                    tab,
                    width: rect.width.round() as u32,
                    height: rect.height.round() as u32,
                },
            ))
        })()
        .filter(|(_, view)| view.width > 0 && view.height > 0);
        // While the panes slide, the page keeps its size and takes the new one once they rest.
        let sliding = shown.is_some() && self.ui.animating();
        if shown != self.browser_shown && !sliding {
            let intent = match (shown, self.browser_shown) {
                (Some((session, view)), _) => Intent::Browser {
                    session,
                    op: newen_model::BrowserOp::Show { view: Some(view) },
                },
                (None, Some((session, _))) => Intent::Browser {
                    session,
                    op: newen_model::BrowserOp::Show { view: None },
                },
                (None, None) => unreachable!("they differ"),
            };
            self.engine.send(intent);
            self.browser_shown = shown;
        }
        let page = region.map(|rect| {
            egui::Rect::from_min_size(
                origin + egui::vec2(rect.x, rect.y),
                egui::vec2(rect.width, rect.height),
            )
        });
        if let Some(direct) = &self.direct {
            let (window, fullscreen) = ctx.input(|i| {
                (
                    i.content_rect().size(),
                    i.viewport().fullscreen == Some(true),
                )
            });
            let room = page.filter(|_| self.browser_shown.is_some());
            direct.place(room, window, ctx.pixels_per_point(), fullscreen);
        }
        let Some(rect) = page else {
            return;
        };
        let hovered = ctx
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| rect.contains(p));
        if hovered {
            let cursor = self
                .ui
                .selected
                .and_then(|s| self.store.session(s))
                .map(|s| s.browser.cursor.as_str())
                .unwrap_or_default();
            ctx.set_cursor_icon(match cursor {
                "pointer" => egui::CursorIcon::PointingHand,
                "text" => egui::CursorIcon::Text,
                "move" => egui::CursorIcon::Move,
                "grab" => egui::CursorIcon::Grab,
                "grabbing" => egui::CursorIcon::Grabbing,
                "not-allowed" => egui::CursorIcon::NotAllowed,
                "crosshair" => egui::CursorIcon::Crosshair,
                "col-resize" | "ew-resize" => egui::CursorIcon::ResizeHorizontal,
                "row-resize" | "ns-resize" => egui::CursorIcon::ResizeVertical,
                "wait" | "progress" => egui::CursorIcon::Wait,
                _ => egui::CursorIcon::Default,
            });
        }
        // Text typed through an input method lands in the page.
        if self.tree.focused_id() == Some(view) {
            ctx.output_mut(|output| {
                output.ime = Some(egui::output::IMEOutput {
                    purpose: egui::IMEPurpose::Normal,
                    rect,
                    cursor_rect: egui::Rect::from_min_size(rect.min, egui::vec2(1.0, 20.0)),
                    should_interrupt_composition: false,
                });
            });
        }
    }

    /// The id egui's renderer draws `texture` by, for the surface `id`: a texture it already
    /// has is drawn again as is, as a page's paints are written into it.
    fn register(
        &mut self,
        render: &eframe::egui_wgpu::RenderState,
        id: &str,
        texture: eframe::wgpu::Texture,
    ) -> egui::TextureId {
        if let Some((known, _)) = self.textures.get(id).filter(|(_, t)| *t == texture) {
            return *known;
        }
        let view = texture.create_view(&Default::default());
        let filter = eframe::wgpu::FilterMode::Linear;
        let mut renderer = render.renderer.write();
        let known = match self.textures.get(id) {
            Some((known, _)) => {
                renderer.update_egui_texture_from_wgpu_texture(
                    &render.device,
                    &view,
                    filter,
                    *known,
                );
                *known
            }
            None => renderer.register_native_texture(&render.device, &view, filter),
        };
        self.textures.insert(id.to_owned(), (known, texture));
        known
    }

    /// The session on screen, its number of items and the length of a reply being written.
    fn transcript_mark(&self, id: Option<SessionId>) -> Option<(SessionId, usize, usize)> {
        let session = self.store.session(id?)?;
        let tail = match session.items.last() {
            Some(newen_ui::Item::Assistant { text, .. }) => text.len(),
            _ => 0,
        };
        Some((session.id, session.items.len(), tail))
    }

    /// Put `prompt` in the composer, its editor and the UI's copy alike, and give it the keys.
    /// Code from newen-ide: its folder's session on screen, or a new one there, and the file
    /// and lines added to the message, for the person to say what they want of them.
    fn ask_from_editor(
        &mut self,
        root: &std::path::Path,
        path: &std::path::Path,
        lines: Option<(u32, u32)>,
        text: &str,
    ) {
        let session = newen_ui::session_for(&self.store, &self.ui, root);
        let id = session.map_or(newen_ui::view::NEW_SESSION.to_owned(), |s| {
            format!("session:{s}")
        });
        let intents = newen_ui::update(&self.store, &mut self.ui, &limn::UiAction::Clicked { id });
        self.dispatch(intents);
        if session.is_none() {
            self.ui.cwd = root.to_path_buf();
        }
        let mut prompt = self.ui.prompt.trim_end().to_owned();
        if !prompt.is_empty() {
            prompt.push_str(if lines.is_some() { "\n\n" } else { " " });
        }
        prompt.push_str(&newen_ui::mention(root, path, lines, text));
        self.set_prompt(prompt);
    }

    fn set_prompt(&mut self, prompt: String) {
        self.tree.set_input_value(newen_ui::view::COMPOSER, &prompt);
        self.ui.prompt = prompt;
        self.tree.focus(newen_ui::view::COMPOSER);
    }

    /// Files the picker or a drop gave: images join the composer's, and the others go into the
    /// message as @-mentions the agent reads.
    fn add_files(&mut self, files: Vec<PathBuf>) {
        let (images, others): (Vec<PathBuf>, Vec<PathBuf>) =
            files.into_iter().partition(|p| newen_ui::is_image(p));
        if !images.is_empty() {
            newen_ui::add_images(&mut self.ui, images);
        }
        if others.is_empty() {
            return;
        }
        // Paths inside the folder are written from it, as the agent reads them.
        let base = newen_ui::folder(&self.store, &self.ui);
        let mentions: Vec<String> = others
            .iter()
            .map(|p| format!("@{}", p.strip_prefix(&base).unwrap_or(p).display()))
            .collect();
        let mut prompt = self.ui.prompt.trim_end().to_owned();
        if !prompt.is_empty() {
            prompt.push(' ');
        }
        prompt.push_str(&mentions.join(" "));
        prompt.push(' ');
        self.set_prompt(prompt);
    }

    /// Ask the platform for files or a folder beside the window; they land in `attached`.
    fn attach(&self, ctx: &egui::Context, folder: bool) {
        let (attached, start, ctx) = (
            self.attached.clone(),
            newen_ui::folder(&self.store, &self.ui),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new().set_directory(start);
            let paths = if folder {
                dialog.pick_folder().into_iter().collect()
            } else {
                dialog.pick_files().unwrap_or_default()
            };
            if !paths.is_empty() {
                *attached.lock().unwrap() = paths;
                ctx.request_repaint();
            }
        });
    }

    /// The theme and the motion Settings chose, in force from this frame: "System" follows the
    /// desktop's theme as it changes, and its motion preference where it can be read.
    fn apply_look(&mut self, ctx: &egui::Context) {
        use newen_model::{Motion, Theme};
        if !self.pinned_theme {
            let dark = match self.ui.settings.theme {
                Theme::Light => false,
                Theme::Dark => true,
                Theme::System => ctx.system_theme() != Some(egui::Theme::Light),
            };
            if dark != newen_ui::look::is_dark() {
                ctx.set_theme(if dark {
                    egui::Theme::Dark
                } else {
                    egui::Theme::Light
                });
                newen_ui::look::set_dark(dark);
            }
        }
        let dark = newen_ui::look::is_dark();
        if self.icon_dark != Some(dark) {
            self.icon_dark = Some(dark);
            ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc::new(window_icon(
                dark,
            )))));
        }
        let motion = self.ui.settings.motion;
        if self.applied != Some(motion) {
            self.applied = Some(motion);
            let reduced = match motion {
                Motion::Reduced => true,
                Motion::Full => false,
                Motion::System => limn_egui::prefers_reduced_motion().unwrap_or(false),
            };
            self.tree.set_reduced_motion(reduced);
            self.ui.reduced_motion = reduced;
        }
    }

    /// Settings' fonts, built on a thread since fontconfig blocks, then taken: the next frame
    /// re-measures the text in them.
    fn apply_fonts(&mut self, ctx: &egui::Context) {
        if let Some((wanted, fonts)) = self.fonts_coming.as_ref().and_then(|rx| rx.try_recv().ok())
        {
            self.fonts_coming = None;
            self.fonts_in_force = wanted;
            limn::set_fonts(fonts);
            limn_egui::install_fonts(ctx);
            self.fonts_attached = false;
        }
        let settings = &self.ui.settings;
        let wanted = (settings.interface_font, settings.code_font.clone());
        if self.fonts_coming.is_none() && wanted != self.fonts_in_force {
            let (interface, code) = wanted.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let repaint = ctx.clone();
            std::thread::spawn(move || {
                let fonts =
                    newen_components::fonts(interface == newen_model::InterfaceFont::System, &code);
                let _ = tx.send((wanted, fonts));
                repaint.request_repaint();
            });
            self.fonts_coming = Some(rx);
        }
    }

    fn dispatch(&mut self, intents: Vec<Intent>) {
        for intent in intents {
            newen_ui::decided(&mut self.store, &intent);
            self.engine.send(intent);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, host: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(native) = &self.native {
            native.pump();
        }
        self.apply_fonts(&ctx);
        if !self.fonts_attached {
            limn_egui::attach_fonts(&mut self.tree, &ctx);
            self.fonts_attached = true;
        }
        // Desktop opens a session at its end and follows it while the person stays there.
        let at_end = self
            .transcript
            .is_none_or(|(id, ..)| self.tree.is_scroll_at_end(&format!("transcript:{id}"), 8.0));
        self.ui.at_end = at_end;
        let events = std::mem::take(&mut *self.inbox.lock().unwrap());
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(true);
        // The updates check when the window comes back, and announce by the system only away.
        if self.focused != Some(focused) {
            self.focused = Some(focused);
            self.dispatch(vec![Intent::WindowFocused { focused }]);
        }
        for event in events {
            // A session that needs the person, or finished, while it is not in view shows a
            // banner and flags Newen, as Desktop's notifications and "Draw attention on
            // notifications".
            // What the engine retells of kept sessions at start is history, not news.
            if matches!(event, Event::Restored) {
                self.restored = true;
            }
            // The new version is in place: it starts, and this window closes (update.rs).
            if let Event::Update(newen_model::UpdateState::Installed { program, .. }) = &event {
                match restart(program) {
                    Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(error) => eprintln!("newen: could not start the new version: {error}"),
                }
            }
            if let Some((level, banner)) = self
                .restored
                .then(|| newen_ui::notice(&self.store, &self.ui, &event, focused))
                .flatten()
            {
                if level == newen_model::NotifyLevel::Banner {
                    self.dispatch(vec![banner]);
                }
                if self.ui.settings.attention {
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                        egui::UserAttentionType::Informational,
                    ));
                }
            }
            // A click on a banner brings the window up on its session.
            if let Event::NotificationClicked { session } = &event {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                let intents = newen_ui::banner_clicked(&self.store, &mut self.ui, *session);
                self.dispatch(intents);
            }
            // What the page had selected goes on the clipboard.
            if let Event::Browser {
                update: newen_model::BrowserUpdate::Copied(text),
                ..
            } = &event
            {
                ctx.copy_text(text.clone());
            }
            // A page's file field: the desktop's dialog, beside the window as it blocks.
            if let Event::Browser {
                session,
                update:
                    newen_model::BrowserUpdate::ChooseFiles {
                        tab,
                        node,
                        multiple,
                    },
            } = &event
            {
                let (chosen, ctx, session, tab, node, multiple) = (
                    self.chosen.clone(),
                    ctx.clone(),
                    *session,
                    *tab,
                    *node,
                    *multiple,
                );
                std::thread::spawn(move || {
                    let dialog = rfd::FileDialog::new();
                    let paths = if multiple {
                        dialog.pick_files().unwrap_or_default()
                    } else {
                        dialog.pick_file().into_iter().collect()
                    };
                    let op = newen_model::BrowserOp::Files { tab, node, paths };
                    *chosen.lock().unwrap() = Some((session, op));
                    ctx.request_repaint();
                });
            }
            // An element picked in the page joins the message being written.
            if let Event::Browser {
                update: newen_model::BrowserUpdate::Selected(block),
                ..
            } = &event
            {
                let mut prompt = self.ui.prompt.trim_end().to_owned();
                if !prompt.is_empty() {
                    prompt.push_str("\n\n");
                }
                prompt.push_str(block);
                self.set_prompt(prompt);
            }
            let mut intents = self.ui.applied(&event);
            self.store.apply(event);
            intents.extend(newen_ui::settled(&self.store, &mut self.ui));
            self.dispatch(intents);
        }
        self.apply_look(&ctx);
        // Context menus read as Newen's other menus, in the theme in force.
        self.tree
            .set_context_menu_palette(newen_ui::look::menu_palette());
        self.tree
            .set_context_menu_sizing(newen_components::pieces::menu_sizing(240.0));
        if let Some(found) = self.editors.lock().unwrap().take() {
            for found in found {
                self.handlers
                    .insert(found.editor.key.clone(), found.command);
                self.ui.editors.push(found.editor);
            }
        }
        let linked = std::mem::take(&mut *self.linked.lock().unwrap());
        for message in linked {
            if let newen_link::Message::Ask {
                root,
                path,
                lines,
                text,
            } = message
            {
                self.ask_from_editor(&root, &path, lines, &text);
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        let mut files = std::mem::take(&mut *self.attached.lock().unwrap());
        files.extend(ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect::<Vec<_>>()
        }));
        self.add_files(files);
        let pasted = std::mem::take(&mut *self.pasted.lock().unwrap());
        if !pasted.is_empty() {
            newen_ui::add_images(&mut self.ui, pasted);
        }
        // Thumbnails for the images on screen, each decoded once, the new ones in one thread,
        // with their sizes: a step's strip draws each at its shape.
        let todo: Vec<PathBuf> = newen_ui::image_paths(&self.store, &self.ui)
            .into_iter()
            .filter(|p| !self.ui.thumbs.contains_key(p) && self.thumbs_asked.insert(p.clone()))
            .collect();
        if !todo.is_empty() {
            self.sizes_asked.extend(todo.iter().cloned());
            let (done, sizes) = (self.thumbs_done.clone(), self.sizes_done.clone());
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                for path in todo {
                    if let Some(size) = images::size(&path) {
                        sizes.lock().unwrap().push((path.clone(), size));
                    }
                    // An image that does not decode keeps its file icon.
                    if let Some(thumb) = images::thumbnail(&path) {
                        done.lock().unwrap().push((path, thumb));
                        ctx.request_repaint();
                    }
                }
            });
        }
        for (path, thumb) in std::mem::take(&mut *self.thumbs_done.lock().unwrap()) {
            self.ui.thumbs.insert(path, thumb);
        }
        // The QR code of the link the Remote Control dialog shows: a millisecond's work, once.
        let link = newen_ui::remote::shown_url(&self.store, &self.ui).map(str::to_owned);
        if link != self.qr_for {
            self.ui.qr = (link.as_deref())
                .and_then(images::qr)
                .zip(link.clone())
                .map(|(picture, link)| (link, picture));
            self.qr_for = link;
        }
        self.ui.pixels_per_point = ctx.pixels_per_point();
        // The image the viewer shows and the picture of a release's announcement, at full
        // size, read once off the window's thread.
        let viewed = self.ui.viewer.as_ref().map(|viewer| viewer.path.clone());
        for path in viewed
            .into_iter()
            .chain(newen_ui::whats_new::media(&self.ui))
        {
            if !self.ui.full.contains_key(&path) && self.full_asked.insert(path.clone()) {
                let (done, ctx) = (self.full_done.clone(), ctx.clone());
                std::thread::spawn(move || {
                    if let Some(full) = images::full(&path) {
                        done.lock().unwrap().push((path, full));
                        ctx.request_repaint();
                    }
                });
            }
        }
        self.clips.tick(&mut self.ui, &ctx);
        for (path, full) in std::mem::take(&mut *self.full_done.lock().unwrap()) {
            self.ui.full.insert(path, full);
        }
        // A whole picture weighs megabytes: past a few, only the one shown is kept.
        if self.ui.full.len() > 8 {
            let shown = self.ui.viewer.as_ref().map(|viewer| viewer.path.clone());
            let media = newen_ui::whats_new::media(&self.ui);
            self.ui
                .full
                .retain(|path, _| Some(path) == shown.as_ref() || media.contains(path));
            let full = &self.ui.full;
            self.full_asked.retain(|path| full.contains_key(path));
        }
        // The viewer and the editor lay the picture out at the file's size, read off the
        // window's thread.
        let shown =
            (self.ui.viewer.as_ref().map(|v| v.path.clone())).or(self.ui.annotating.clone());
        if let Some(path) = shown {
            if !self.ui.sizes.contains_key(&path) && self.sizes_asked.insert(path.clone()) {
                let (done, ctx) = (self.sizes_done.clone(), ctx.clone());
                std::thread::spawn(move || {
                    if let Some(size) = images::size(&path) {
                        done.lock().unwrap().push((path, size));
                        ctx.request_repaint();
                    }
                });
            }
        }
        for (path, size) in std::mem::take(&mut *self.sizes_done.lock().unwrap()) {
            self.ui.sizes.insert(path, size);
        }
        // An annotated copy made: it takes the picture's place, or the editor says it failed.
        for (original, copy) in std::mem::take(&mut *self.annotated.lock().unwrap()) {
            newen_ui::annotate::finish_save(&mut self.ui, original, copy);
        }
        if let Some(folder) = self.picked.lock().unwrap().take() {
            self.ui.cwd = folder;
        }
        if let Some((session, op)) = self.chosen.lock().unwrap().take() {
            self.engine.send(Intent::Browser { session, op });
        }
        let skill_file = self.skill_file.lock().unwrap().take();
        if let Some((purpose, path)) = skill_file {
            let intents = newen_ui::customize::file_chosen(&mut self.ui, &purpose, path);
            self.dispatch(intents);
        }
        let chromium = self.chromium.lock().unwrap().take();
        if let Some(program) = chromium {
            let mut settings = self.ui.settings.clone();
            settings.browser_program = Some(program.display().to_string());
            self.dispatch(vec![Intent::SaveSettings(settings)]);
        }
        limn_egui::normalize_keyboard_input(&ctx);
        let area = ui.max_rect();
        let window = Size::new(area.width(), area.height());
        self.ui.window = window;
        // New output keeps the terminal at its end only if the person left it there.
        let shown = self.ui.terminal.screen.clone();
        let follow = shown.is_none()
            || self
                .tree
                .is_scroll_at_end(newen_ui::view::TERMINAL_SCROLL, 4.0);
        self.sync_terminal(&ctx, window);
        // The hidden sidebar peeks while the pointer is on its button or the window's edge.
        self.ui.pointer = ctx
            .input(|i| i.pointer.hover_pos())
            .map(|p| limn::Point::new(p.x - area.min.x, p.y - area.min.y));
        self.ui.hovered = self.tree.hovered_id().map(str::to_owned);
        self.ui.tick(&self.store);
        let mut due = self.ui.due();
        due.extend(self.ui.logos_due(&self.store));
        self.dispatch(due);
        // Inactive sessions are put away every ten minutes, as Settings asks; not before it
        // asks, which may come a moment after start.
        if self.ui.settings.archive_after_days > 0
            && self
                .archive_checked
                .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(600))
        {
            self.archive_checked = Some(std::time::Instant::now());
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let days = self.ui.settings.archive_after_days;
            let archive = newen_ui::inactive(&self.store, days, now)
                .into_iter()
                .map(|session| newen_model::Intent::Archive {
                    session,
                    archived: true,
                })
                .collect();
            self.dispatch(archive);
        }
        // What came in may have grown a transcript kept at its end: this layout, which input is
        // read against, keeps it there as well, else the end would move away from it before
        // the frame's own follow below.
        let pressed = ctx.input(|i| i.pointer.primary_down());
        let scrolls = [
            self.transcript.map(|(id, ..)| format!("transcript:{id}")),
            self.beside.map(|(id, ..)| format!("side:{id}:scroll")),
            self.helper.clone(),
        ];
        for scroll in scrolls.iter().flatten() {
            if !pressed && self.tree.is_scroll_at_end(scroll, 0.5) {
                self.tree.scroll_to_end(scroll);
            }
        }
        self.tree
            .set_root(newen_ui::view(&self.store, &self.ui, window));
        self.tree.layout(window);
        let mut actions = vec![];
        let mut focus_terminal = false;
        let was_renaming = self.ui.terminal.renaming.is_some();
        let was_titling = self.ui.title_edit.is_some();
        // Ctrl+, opens Settings, as in Desktop.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Comma)) {
            actions.push(limn::UiAction::Clicked {
                id: newen_ui::view::SETTINGS_OPEN.into(),
            });
        }
        // Ctrl+` shows and hides the terminal, as in Desktop; limn has no key for it.
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Backtick)) {
            actions.push(limn::UiAction::Clicked {
                id: newen_ui::view::TERMINAL_TOGGLE.into(),
            });
        }
        // The browser's frames, presented where the view asks for them.
        self.frames.set_scale(ctx.pixels_per_point());
        for (id, frame) in self.frames.take() {
            let size = [frame.width, frame.height];
            match frame.pixels {
                newen_engine::Pixels::Rgba(rgba) => {
                    if let Some(surface) = surface_frame(size, rgba, frame.scale) {
                        limn_egui::present_surface(&ctx, &id, surface);
                    }
                }
                newen_engine::Pixels::Texture(texture) => {
                    if let Some(render) = host.wgpu_render_state() {
                        let texture = self.register(render, &id, texture);
                        limn_egui::present_surface_texture(&ctx, &id, texture, size, frame.scale);
                    }
                }
            }
        }
        // A page presented beneath the window shows through a hole in it.
        let direct = self.direct.as_ref().filter(|d| d.shows());
        if let (Some(direct), Some((session, view))) = (direct, self.browser_shown) {
            let surface = newen_ui::view::browser_surface(session, view.tab);
            limn_egui::present_surface_paint(&ctx, &surface, direct::punch(direct));
        }
        let (asks, taken) = self.route_page(&ctx, area.min);
        for ask in asks {
            actions.extend(self.page_asked(ask, window));
        }
        for item in limn_egui::events_indexed(&ctx, area.min) {
            if item
                .source_index
                .is_some_and(|index| taken.contains(&index))
            {
                continue;
            }
            let event = item.event;
            // The pointer over a row of the "/" or "@" list chooses it, as Desktop's.
            if let limn::UiEvent::PointerMove(position) = &event {
                if let Some(id) = self.tree.frame().hit_test(*position) {
                    newen_ui::palette_hover(&self.store, &mut self.ui, id);
                }
            }
            // A right-click on a link of the conversation opens its own menu there.
            if let limn::UiEvent::ContextMenu { position } = &event {
                let url = (self.tree.frame().hit_test(*position))
                    .and_then(newen_ui::view::link_url)
                    .map(str::to_owned);
                if let Some(url) = url {
                    self.ui.link_menu = Some((url, *position));
                    continue;
                }
            }
            // Permission keys work wherever the focus is, the composer included.
            // Ctrl+U adds files; a digit picks that row of an open composer menu, as in Desktop.
            if matches!(
                &event,
                limn::UiEvent::Key {
                    key: limn::Key::U,
                    ctrl: true,
                    shift: false,
                    ..
                }
            ) {
                actions.push(limn::UiAction::Clicked {
                    id: newen_ui::view::ATTACH_FILES.into(),
                });
                continue;
            }
            // Ctrl+N starts a new session, as Desktop's sidebar says.
            if matches!(
                &event,
                limn::UiEvent::Key {
                    key: limn::Key::N,
                    ctrl: true,
                    shift: false,
                    ..
                }
            ) {
                actions.push(limn::UiAction::Clicked {
                    id: newen_ui::view::NEW_SESSION.into(),
                });
                continue;
            }
            // Ctrl+O cycles the transcript view, as Desktop's keymap does (docs/ui.md §2.12).
            if matches!(
                &event,
                limn::UiEvent::Key {
                    key: limn::Key::O,
                    ctrl: true,
                    shift: false,
                    ..
                }
            ) {
                actions.push(limn::UiAction::Clicked {
                    id: newen_ui::view::TRANSCRIPT_VIEW_CYCLE.into(),
                });
                continue;
            }
            // Ctrl+B hides the sidebar and Ctrl+K opens the search, as Desktop's tooltips say;
            // neither reaches past a dialog.
            let dialog = self.ui.deleting.is_some() || self.ui.terminal.closing.is_some();
            if let limn::UiEvent::Key {
                key: key @ (limn::Key::B | limn::Key::K),
                ctrl: true,
                shift: false,
                ..
            } = &event
            {
                if dialog {
                    continue;
                }
                let id = if *key == limn::Key::B {
                    newen_ui::view::SIDEBAR_TOGGLE
                } else {
                    newen_ui::view::SEARCH
                };
                actions.push(limn::UiAction::Clicked { id: id.into() });
                continue;
            }
            // Ctrl+Shift+I and Ctrl+Shift+M open the model and mode menus, as in Desktop.
            if let limn::UiEvent::Key {
                key,
                ctrl: true,
                shift: true,
                ..
            } = &event
            {
                let menu = match key {
                    limn::Key::I => Some(newen_ui::view::MODEL_MENU),
                    limn::Key::M => Some(newen_ui::view::MODE_MENU),
                    limn::Key::D => Some(newen_ui::view::DIFF_TOGGLE),
                    // The browser, as Desktop's Ctrl+Shift+B.
                    limn::Key::B => Some(newen_ui::view::BROWSER_TOGGLE),
                    // The Files pane, as an editor's Ctrl+Shift+E.
                    limn::Key::E => Some(newen_ui::view::FILES_TOGGLE),
                    _ => None,
                };
                if let Some(id) = menu {
                    actions.push(limn::UiAction::Clicked { id: id.into() });
                    continue;
                }
            }
            if let limn::UiEvent::Text(typed) = &event {
                if let Some(id) = newen_ui::menu_digit(&self.store, &self.ui, typed) {
                    actions.push(limn::UiAction::Clicked { id });
                    continue;
                }
            }
            let focused = self.tree.focused_id().map(str::to_owned);
            // The "/" palette over the composer takes the arrows, Enter, Tab and Escape while open.
            if let Some(intents) =
                newen_ui::slash_key(&self.store, &mut self.ui, focused.as_deref(), &event)
            {
                self.dispatch(intents);
                continue;
            }
            // The "@" list takes the same keys over the composer; only one of the two opens.
            if newen_ui::mention_key(&self.store, &mut self.ui, focused.as_deref(), &event) {
                continue;
            }
            // An empty composer's ArrowUp takes the last queued message back for editing.
            if let Some(intents) =
                newen_ui::queue_key(&self.store, &self.ui, focused.as_deref(), &event)
            {
                self.dispatch(intents);
                continue;
            }
            // ...and its Tab or → takes the agent's suggested prompt.
            if newen_ui::suggestion_key(&self.store, &mut self.ui, focused.as_deref(), &event) {
                continue;
            }
            // On a row of the Files pane, the arrows and nvim-tree's letters walk and change it.
            if let Some(intents) = newen_ui::files_key(&mut self.ui, focused.as_deref(), &event) {
                self.dispatch(intents);
                continue;
            }
            if let limn::UiEvent::Key {
                key, shift, ctrl, ..
            } = &event
            {
                let shortcut = newen_ui::shortcut(&self.store, &self.ui, *key, *ctrl, *shift)
                    .or_else(|| newen_ui::annotate::key(&self.ui, *key, *ctrl, *shift));
                if let Some(id) = shortcut {
                    actions.push(limn::UiAction::Clicked { id });
                    continue;
                }
                let focused = self.tree.focused_id().map(str::to_owned);
                if newen_ui::terminal_key(&mut self.ui, focused.as_deref(), *key, *ctrl, *shift) {
                    continue;
                }
            }
            if !self.typed_into_terminal(&event) {
                newen_ui::composer_key(&mut self.ui, self.tree.focused_id(), &event);
                actions.extend(self.tree.handle_event(event));
            }
        }
        actions.extend(limn_egui::apply_access_requests(&ctx, &mut self.tree));
        for action in actions {
            // The clip's play and pause button is its player's.
            if (self.ui.video.as_mut()).is_some_and(|player| player.handle(&action)) {
                continue;
            }
            if let limn::UiAction::CopyRequested { text } = &action {
                ctx.copy_text(text.clone());
            }
            // An image copied with no text, pasted into the composer: saved off this thread.
            if let Some((width, height, rgba)) = newen_ui::pasted_image(&action) {
                images::paste(
                    self.pasted.clone(),
                    ctx.clone(),
                    (width, height, rgba.clone()),
                );
            }
            if matches!(&action, limn::UiAction::Clicked { id } if id == newen_ui::view::PICK_FOLDER)
            {
                // The dialog blocks until the person answers, so it runs beside the window.
                let (picked, start, ctx) = (self.picked.clone(), self.ui.cwd.clone(), ctx.clone());
                std::thread::spawn(move || {
                    if let Some(folder) = rfd::FileDialog::new().set_directory(start).pick_folder()
                    {
                        *picked.lock().unwrap() = Some(folder);
                        ctx.request_repaint();
                    }
                });
            }
            if let limn::UiAction::Clicked { id } = &action {
                if let Some(words) = newen_ui::copied_text(&self.store, id) {
                    ctx.copy_text(words);
                }
                // A link: its address copied, or opened on the computer, from its menu or, for
                // what Newen's browser and Files do not open (mail, another app's), its click.
                if let Some(url) = id.strip_prefix(newen_ui::view::LINK_COPY) {
                    ctx.copy_text(url.to_owned());
                }
                // A page whose link opens on the computer, as Settings asks.
                let web_out = newen_ui::view::link_url(id)
                    .filter(|url| newen_ui::opens_outside(&self.ui.settings, url));
                let elsewhere = newen_ui::view::link_url(id)
                    .filter(|url| {
                        url.contains("://")
                            && !url.starts_with("http")
                            && !url.starts_with("file://")
                    })
                    .or_else(|| {
                        newen_ui::view::link_url(id).filter(|url| url.starts_with("mailto:"))
                    });
                if let Some(url) = id
                    .strip_prefix(newen_ui::view::LINK_EXTERNAL)
                    .or(elsewhere)
                    .or(web_out)
                {
                    open_external(url);
                }
                if let Some(code) = id.strip_prefix(newen_ui::view::COPY_CODE) {
                    ctx.copy_text(code.to_owned());
                }
                // "Open in": a session's folder, or the one on screen ("here"), in an editor,
                // the desktop's default app or its file manager.
                if let Some((place, target)) = id
                    .strip_prefix("open-in:")
                    .and_then(|rest| rest.split_once(':'))
                {
                    let folder = target
                        .parse()
                        .ok()
                        .and_then(|s| self.store.session(s))
                        .map_or_else(
                            || newen_ui::folder(&self.store, &self.ui),
                            |s| s.cwd.clone(),
                        );
                    match place {
                        // The desktop's answer is waited for beside the window.
                        "files" => {
                            std::thread::spawn(move || show_in_file_manager(&folder));
                        }
                        "default" => {
                            open_external(&folder);
                        }
                        editor => {
                            open_in_editor(editor, self.handlers.get(editor), &folder, None, None)
                        }
                    }
                }
                // A changed file from its Open in menu: at its line, in an editor, or the
                // desktop's default app, or shown in its folder.
                if let Some((place, (line, path))) = id
                    .strip_prefix(newen_ui::view::OPEN_FILE_IN)
                    .and_then(|rest| rest.split_once(':'))
                    .and_then(|(place, rest)| Some((place, rest.split_once(':')?)))
                {
                    let root = newen_ui::folder(&self.store, &self.ui);
                    let path = root.join(path);
                    match place {
                        "files" => {
                            let folder = path.parent().map_or(root.clone(), |p| p.to_path_buf());
                            std::thread::spawn(move || show_in_file_manager(&folder));
                        }
                        "default" => {
                            open_external(&path);
                        }
                        editor => open_in_editor(
                            editor,
                            self.handlers.get(editor),
                            &root,
                            Some(&path),
                            line.parse().ok(),
                        ),
                    }
                }
                // Customize: a skill's file to add or put in another's place, or where to
                // save one, from the platform's dialog.
                if let Some(rest) = id.strip_prefix("czp:file:") {
                    let (skill_file, ctx, purpose) =
                        (self.skill_file.clone(), ctx.clone(), id.clone());
                    let saving = rest.strip_prefix("download:").map(|n| format!("{n}.zip"));
                    std::thread::spawn(move || {
                        let dialog = rfd::FileDialog::new();
                        let path = match saving {
                            Some(name) => dialog.set_file_name(name).save_file(),
                            None => dialog
                                .add_filter("Skill", &["skill", "zip", "md"])
                                .pick_file(),
                        };
                        if let Some(path) = path {
                            *skill_file.lock().unwrap() = Some((purpose, path));
                            ctx.request_repaint();
                        }
                    });
                }
                // Settings › Browser: the Chromium to run, from the platform's dialog.
                if id == newen_ui::view::BROWSER_PROGRAM_CHOOSE {
                    let (chromium, ctx) = (self.chromium.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        if let Some(path) = rfd::FileDialog::new().pick_file() {
                            *chromium.lock().unwrap() = Some(path);
                            ctx.request_repaint();
                        }
                    });
                }
                // Settings' data folder shows in the desktop's own file manager.
                if id == newen_ui::view::SETTINGS_STORAGE_OPEN {
                    if let Some((path, _)) = &self.ui.storage {
                        open_external(path);
                    }
                }
                match id.as_str() {
                    newen_ui::view::ATTACH_FILES => self.attach(&ctx, false),
                    newen_ui::view::ATTACH_FOLDER => self.attach(&ctx, true),
                    newen_ui::view::SLASH if !self.ui.prompt.starts_with('/') => {
                        self.set_prompt(format!("/{}", self.ui.prompt));
                    }
                    // Desktop's "See all" opens the plan's usage page.
                    newen_ui::view::USAGE_ALL => {
                        open_external("https://claude.ai/settings/usage");
                    }
                    newen_ui::view::SCROLL_END => {
                        if let Some(session) = self.ui.selected {
                            self.tree.scroll_to_end(&format!("transcript:{session}"));
                        }
                    }
                    // The filter's clear empties it; the field keeps its own text.
                    newen_ui::view::FILTER_CLEAR => {
                        self.tree.set_input_value(newen_ui::view::FILTER_FIELD, "");
                    }
                    newen_ui::view::SEND if !self.ui.prompt.trim().is_empty() => {
                        self.tree.set_input_value(newen_ui::view::COMPOSER, "");
                    }
                    _ => {}
                }
            }
            // An address opened, the page takes the keys, as a browser's does.
            if matches!(&action, limn::UiAction::Submitted { id, .. } if id == newen_ui::view::BROWSER_URL)
            {
                self.address = None;
                self.tree.focus_from_pointer(newen_ui::view::BROWSER_VIEW);
            }
            // The editor keeps its own text; a sent message empties it.
            if let limn::UiAction::Submitted { id, value } = &action {
                let field = [newen_ui::view::COMPOSER, newen_ui::view::SIDE_FIELD]
                    .into_iter()
                    .find(|f| id == f);
                if let Some(field) = field.filter(|_| !value.trim().is_empty()) {
                    self.tree.set_input_value(field, "");
                }
            }
            let opened = !self.ui.pane_open(Pane::Terminal);
            // A tile opens the viewer, which grows its picture from the tile: only the frame knows where it is.
            let tile = match &action {
                limn::UiAction::Clicked { id }
                    if ["image:open:", "image:msg:", "image:tool:", "image:step:"]
                        .iter()
                        .any(|prefix| id.starts_with(prefix)) =>
                {
                    self.tree
                        .frame()
                        .regions
                        .iter()
                        .find(|r| &r.id == id)
                        .map(|r| r.rect)
                }
                _ => None,
            };
            // Save: the copy is made off the window's thread and answered on a later frame.
            if matches!(&action, limn::UiAction::Clicked { id } if id == newen_ui::annotate::SAVE) {
                if let Some((original, marks, window)) =
                    newen_ui::annotate::start_save(&mut self.ui)
                {
                    let (done, ctx) = (self.annotated.clone(), ctx.clone());
                    std::thread::spawn(move || {
                        let copy = images::annotate(&original, &marks, window);
                        done.lock().unwrap().push((original, copy));
                        ctx.request_repaint();
                    });
                }
            }
            let intents = newen_ui::update(&self.store, &mut self.ui, &action);
            if let (Some(viewer), Some(rect)) = (self.ui.viewer.as_mut(), tile) {
                viewer.origin = Some(rect);
            }
            self.dispatch(intents);
            // A pane just opened, or a tab just picked or made, takes the keys.
            focus_terminal |= (opened && self.ui.pane_open(Pane::Terminal))
                || self.ui.terminal.requests.iter().any(|r| {
                    !matches!(r, TerminalRequest::Close(_) | TerminalRequest::Move { .. })
                });
        }
        // Each conversation keeps its own draft: the one switched to fills the box.
        if std::mem::take(&mut self.ui.draft_switched) {
            self.tree
                .set_input_value(newen_ui::view::COMPOSER, &self.ui.prompt);
        }
        // A message rewound or forked from goes back in the box, as Desktop's does.
        if let Some(prompt) = self.ui.restore_prompt.take() {
            self.set_prompt(prompt);
        }
        // What the Files pane asked for: a control to focus, and words to copy.
        if let Some(id) = self.ui.files_focus.take() {
            self.tree.focus(&id);
        }
        // Kept until the editor is laid out: a file just read has none yet.
        if let Some(id) = self.ui.files_reveal.take() {
            if !self.tree.reveal_code_caret(&id) {
                self.ui.files_reveal = Some(id);
                ctx.request_repaint();
            }
        }
        if let Some(words) = self.ui.files_copy.take() {
            ctx.copy_text(words);
        }
        self.sync_terminal(&ctx, window);
        let changed = match (&shown, &self.ui.terminal.screen) {
            (Some(before), Some(now)) => !Arc::ptr_eq(before, now),
            (None, Some(_)) => true,
            _ => false,
        };
        if (follow && changed) || focus_terminal {
            self.tree.scroll_to_end(newen_ui::view::TERMINAL_SCROLL);
        }
        // At its end, the transcript stays there every frame, not only when a message grows:
        // the text is revealed over frames, and thumbnails and steps open after it. Whether it
        // is at its end is read after this frame's input reached the tree, to the pixel, so the
        // person moving it up at all lets go of the end: a wheel's smallest step (Hyprland's
        // smooth scroll sends many), the bar or the paging keys. Bringing it back down to the
        // end takes it up again; a press, as when selecting its text, holds it where it is.
        let kept = |tree: &limn::UiTree, id: &str| !pressed && tree.is_scroll_at_end(id, 0.5);
        let mark = self.transcript_mark(self.ui.selected);
        let opened = mark.map(|m| m.0) != self.transcript.map(|m| m.0);
        if let Some((id, ..)) = mark {
            let scroll = format!("transcript:{id}");
            if opened || kept(&self.tree, &scroll) {
                self.tree.scroll_to_end(&scroll);
            }
        }
        self.transcript = mark;
        // The session beside opens at its end too, and follows it the same way.
        let mark = self.transcript_mark(self.ui.side_chat);
        let opened = mark.map(|m| m.0) != self.beside.map(|m| m.0);
        if let Some((id, ..)) = mark {
            let scroll = format!("side:{id}:scroll");
            if opened || kept(&self.tree, &scroll) {
                self.tree.scroll_to_end(&scroll);
            }
        }
        self.beside = mark;
        // A helper agent's transcript opens at its end and follows it the same way.
        let scroll = self.ui.helper_scroll();
        if let Some(id) = &scroll {
            if scroll != self.helper || kept(&self.tree, id) {
                self.tree.scroll_to_end(id);
            }
        }
        self.helper = scroll;
        self.sync_address();
        self.ui.tick(&self.store);
        self.tree
            .set_root(newen_ui::view(&self.store, &self.ui, window));
        let mut frame = self.tree.layout(window);
        // A tour step whose control is not in view moves on, so no bubble waits on nothing; the
        // view is built again until a step is found (each move ends or advances the tour).
        while let Some(target) = frame.missing_coach_target().map(str::to_owned) {
            let Some(intents) = newen_ui::tour::skip_missing(&mut self.ui, &target) else {
                break;
            };
            self.dispatch(intents);
            self.tree
                .set_root(newen_ui::view(&self.store, &self.ui, window));
            frame = self.tree.layout(window);
        }
        // A pill that just turned into a name field takes the keys.
        if !was_renaming && self.ui.terminal.renaming.is_some() {
            self.tree.focus(newen_ui::view::TERMINAL_RENAME);
            frame = self.tree.layout(window);
        }
        // The title's field takes the keys with its text selected, as Desktop's does.
        if !was_titling && self.ui.title_edit.is_some() {
            self.tree.focus(newen_ui::view::TITLE_FIELD);
            self.tree.handle_event(limn::UiEvent::Key {
                key: limn::Key::A,
                ctrl: true,
                shift: false,
                alt: false,
            });
            frame = self.tree.layout(window);
        }
        // A pane just opened takes the keys; focus needs the laid-out tree.
        if focus_terminal && self.tree.focus(newen_ui::view::TERMINAL) {
            frame = self.tree.layout(window);
        }
        // The clip's frame that is due, and a repaint when the next is: only while it plays
        // in view.
        if let Some(player) = self.ui.video.as_mut() {
            player.update(&ctx, &frame);
        }
        limn_egui::paint_at(ui, &frame, area.min);
        limn_egui::set_cursor(&ctx, self.tree.cursor());
        self.page_frame(&ctx, &frame, area.min);
        if let Some(after) = self.tree.animation_repaint_after() {
            ctx.request_repaint_after(after);
        }
        // The effort thumb's spring and the Ultracode field are Newen's own, not the tree's.
        if let Some(after) = self.ui.repaint_after() {
            ctx.request_repaint_after(after);
        }
    }
}

/// Cisco's OpenH264 release for this system, which the engine downloads from Cisco once the
/// person allows it; limn-video names it, as it loads only that release.
fn codec_release() -> Option<newen_engine::CodecRelease> {
    limn_video::cisco::current().map(|release| newen_engine::CodecRelease {
        version: limn_video::cisco::VERSION.into(),
        url: release.url.into(),
        file_name: release.file_name.into(),
        sha256: release.sha256.into(),
    })
}

/// The local time zone's offset from UTC in seconds, as `date +%z` says it (`-0300`).
fn utc_offset() -> i64 {
    let out = newen_files::command("date").arg("+%z").output().ok();
    let zone = out
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    let zone = zone.trim();
    let (sign, digits) = match zone.split_at_checked(1) {
        Some(("-", d)) => (-1, d),
        Some(("+", d)) => (1, d),
        _ => return 0,
    };
    let hours: i64 = digits.get(0..2).and_then(|h| h.parse().ok()).unwrap_or(0);
    let minutes: i64 = digits.get(2..4).and_then(|m| m.parse().ok()).unwrap_or(0);
    sign * (hours * 3_600 + minutes * 60)
}

/// The person's first name for the greeting: the account's full name if the system has one,
/// else the login name, capitalized.
fn first_name() -> Option<String> {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()?;
    let full = newen_files::command("getent")
        .args(["passwd", &user])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|line| line.split(':').nth(4).map(str::to_owned))
        .filter(|gecos| !gecos.trim().is_empty());
    let name = full
        .as_deref()
        .unwrap_or(&user)
        .split([' ', ','])
        .next()?
        .to_owned();
    let mut chars = name.chars();
    Some(chars.next()?.to_uppercase().chain(chars).collect())
}

/// A page, a file or a folder in what the computer opens it with: its default browser for
/// an address, on Linux, Windows and macOS alike.
fn open_external(target: impl AsRef<std::ffi::OsStr>) {
    let target = target.as_ref();
    let command = newen_files::open_in_system(target);
    if let Err(error) = command {
        eprintln!(
            "newen: could not open {}: {error}",
            target.to_string_lossy()
        );
    }
}

/// `newen --open-url URL`: the agents' `$BROWSER` (the engine's bridge). A local page opens in
/// the session's browser; anything else, or no Newen to answer, in the desktop's own.
/// `newen --install`: this copy becomes the installed version, which `current` names from then
/// on (newen-update's install.rs). Returns where it is.
fn install_self() -> Result<String, String> {
    let install = newen_update::install::Install::locate()
        .ok_or("no folder to install Newen in (set HOME or NEWEN_UPDATE_ROOT)")?;
    let program = std::env::current_exe().map_err(|error| error.to_string())?;
    let version = newen_update::Version::parse(newen_update::VERSION).map_err(|e| e.to_string())?;
    let placed = install
        .adopt(&program, &version)
        .map_err(|error| error.to_string())?;
    Ok(placed.display().to_string())
}

/// Before the window, in an installed copy Newen updates: a start of the version on trial is
/// counted, and after two that never ran a minute it is rolled back to the one before; else a
/// newer version staged is activated. Either way the installed program then starts in place of
/// this one. Returns the version on trial, to end it once this start proves healthy.
fn launch_update() -> Option<(newen_update::install::Install, newen_update::Version)> {
    use newen_update::install::{Install, Launch, ATTEMPTS};
    if newen_files::env("NO_UPDATE").is_some_and(|value| !value.is_empty()) {
        return None;
    }
    let install = Install::locate()?;
    if !install.runs(&std::env::current_exe().ok()?) {
        return None;
    }
    let running = newen_update::Version::parse(newen_update::VERSION).ok()?;
    let start = |program: &std::path::Path| {
        if let Err(error) = restart(program) {
            update_log(format!(
                "start: could not start {}: {error}",
                program.display()
            ));
            return;
        }
        std::process::exit(0);
    };
    match install.launch(&running) {
        Ok(Launch::Go) => {}
        Ok(Launch::Activated { version, program }) => {
            update_log(format!(
                "activate: {version} in place of {running}, at the start"
            ));
            start(&program);
        }
        Ok(Launch::RolledBack {
            version,
            previous,
            program,
        }) => {
            update_log(format!(
                "rollback: {version} started {ATTEMPTS} times without running a minute; back to {previous}, and {version} is refused"
            ));
            start(&program);
        }
        Err(error) => update_log(format!("start: {error}")),
    }
    let trial = install.trial().filter(|trial| trial.version == running)?;
    update_log(format!(
        "start: {running} on trial after {}, start {}",
        trial.previous, trial.attempts
    ));
    Some((install, running))
}

/// Newen's first start on a computer quill used moves quill's data to Newen's names
/// (`newen_engine::migrate`), when `allowed` (a release, or a debug build asked to) and no copy
/// of quill answers on `quill_socket`: one alive would go on writing where it is. A step that
/// fails, or one that panics, leaves quill's names in use for this start. What happened, a line
/// each.
fn migrate_from_quill(
    allowed: bool,
    quill_socket: &std::path::Path,
    data: &std::path::Path,
    config: &std::path::Path,
) -> Vec<String> {
    if !allowed {
        return vec![];
    }
    if newen_link::answers(quill_socket) {
        return vec![
            "a copy of quill runs: its data stays where it is until a start without it".into(),
        ];
    }
    std::panic::catch_unwind(|| newen_engine::migrate::from_quill(data, config))
        .unwrap_or_else(|_| vec!["stopped by an error: quill's data stays in use".into()])
}

/// A line in the update log (and on stderr).
fn update_log(line: String) {
    eprintln!("newen: update: {line}");
    if let Some(path) = newen_update::log::path() {
        newen_update::log::write(&path, &line);
    }
}

/// Where a restart names the process the program it starts waits for, `NEWEN_RESTART_AFTER`:
/// quill 0.1.9 reads it too when Newen goes back to it, and quill sets `QUILL_RESTART_AFTER`
/// when it starts Newen ([`newen_files::env`]).
const RESTART_AFTER: &str = "RESTART_AFTER";

/// Starts the installed program with this one's arguments. It waits for this process to go
/// (`wait_for_restarter`): the link socket is this window's until then.
fn restart(program: &std::path::Path) -> std::io::Result<()> {
    restart_command(program).spawn().map(drop)
}

fn restart_command(program: &std::path::Path) -> std::process::Command {
    let mut command = newen_files::command(program);
    command.args(std::env::args_os().skip(1));
    command.env(
        format!("NEWEN_{RESTART_AFTER}"),
        std::process::id().to_string(),
    );
    command
}

/// The process a restart said to wait for.
fn restarter(var: Option<std::ffi::OsString>) -> Option<u32> {
    var?.to_str()?.parse().ok()
}

/// When a restarted copy starts, waits (up to ten seconds) for the process that restarted it
/// to exit, so the window and the link socket are free.
fn wait_for_restarter() {
    let Some(pid) = restarter(newen_files::env(RESTART_AFTER)) else {
        return;
    };
    for prefix in ["NEWEN", "QUILL"] {
        std::env::remove_var(format!("{prefix}_{RESTART_AFTER}"));
    }
    // shortcut: only Linux can ask whether a pid lives without FFI; elsewhere a moment's grace.
    if !std::path::Path::new("/proc/self").exists() {
        std::thread::sleep(std::time::Duration::from_secs(1));
        return;
    }
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::path::Path::new(&format!("/proc/{pid}")).exists()
        && std::time::Instant::now() < until
    {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

fn open_url(url: &str) -> bool {
    // The bridge to Newen's own browser is a Unix socket; elsewhere the system opens it.
    #[cfg(unix)]
    let handed = (|| -> Option<bool> {
        use std::io::{BufRead, Write};
        let socket = std::env::var_os("NEWEN_BROWSER_BRIDGE")?;
        let key = std::env::var("NEWEN_BROWSER_KEY").ok()?;
        let mut stream = std::os::unix::net::UnixStream::connect(socket).ok()?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .ok()?;
        writeln!(stream, "{}", serde_json::json!({ "key": key, "url": url })).ok()?;
        let mut reply = String::new();
        std::io::BufReader::new(stream).read_line(&mut reply).ok()?;
        Some(reply.trim() == "pane")
    })()
    .unwrap_or(false);
    #[cfg(not(unix))]
    let handed = false;
    handed
        || newen_files::open_in_system(url)
            .and_then(|mut opener| opener.wait())
            .is_ok_and(|status| status.success())
}

/// Show `path` of the folder `root`, or the folder, in `editor` at `line` when given: newen-ide
/// in its window of that folder when one is open, else a new one; the others by their URL
/// link, as Desktop opens them: `vscode://file/<path>:<line>`.
fn open_in_editor(
    editor: &str,
    handler: Option<&Vec<String>>,
    root: &std::path::Path,
    path: Option<&std::path::Path>,
    line: Option<u32>,
) {
    let target = path.unwrap_or(root);
    if editor != newen_ui::view::NEWEN_IDE {
        let shown = target.display().to_string();
        // As Desktop: a path the URL would have to escape is not sent at all.
        if shown.contains(['%', '#', '?', '\\']) || shown.chars().any(char::is_whitespace) {
            eprintln!("newen: {shown} cannot go to {editor} as a link");
            return;
        }
        let at = line.map_or(String::new(), |line| format!(":{line}"));
        let url = format!("{editor}://file{shown}{at}");
        // The command the editor's own `.desktop` file runs for its links, which needs no
        // default handler set; without one, the desktop's.
        let command = handler
            .map(|exec| editors::command(exec, &url))
            .filter(|c| !c.is_empty());
        let opened = match command {
            Some(command) => newen_files::command(&command[0])
                .args(&command[1..])
                .spawn(),
            None => newen_files::open_in_system(url),
        };
        if let Err(error) = opened {
            eprintln!("newen: {editor} did not open: {error}");
        }
        return;
    }
    let open = newen_link::Message::Open {
        root: root.to_path_buf(),
        path: target.to_path_buf(),
        line,
        column: None,
    };
    if newen_link::send(&newen_link::ide_socket(root), &open).is_ok() {
        return;
    }
    // Beside Newen when they were built together, else the one on PATH.
    let program = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(newen_ui::view::NEWEN_IDE)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| newen_ui::view::NEWEN_IDE.into());
    let mut command = newen_files::command(program);
    command.arg(root);
    if let Some(path) = path {
        command.arg(match line {
            Some(line) => format!("{}:{line}", path.display()),
            None => path.display().to_string(),
        });
    }
    if let Err(error) = command.spawn() {
        eprintln!("newen: newen-ide did not start: {error}");
    }
}

/// The folder shown, picked, in the desktop's file manager, through its common interface;
/// without one, the folder it is in.
fn show_in_file_manager(folder: &std::path::Path) {
    // Explorer and Finder select the item themselves (explorer exits 1 even then).
    if cfg!(windows) {
        let _ = newen_files::command("explorer")
            .arg(format!("/select,{}", folder.display()))
            .spawn();
        return;
    }
    if cfg!(target_os = "macos") {
        let _ = newen_files::command("open").arg("-R").arg(folder).spawn();
        return;
    }
    let shown = newen_files::command("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.FileManager1",
            "--type=method_call",
            "/org/freedesktop/FileManager1",
            "org.freedesktop.FileManager1.ShowItems",
        ])
        .arg(format!("array:string:file://{}", folder.display()))
        .arg("string:")
        .status()
        .is_ok_and(|s| s.success());
    if !shown {
        open_external(folder.parent().unwrap_or(folder));
    }
}

/// Newen's own icon for its window in the theme in force (X11, Windows); a Wayland desktop
/// takes it by the app id from the icon theme instead (`scripts/install-desktop.sh`).
fn window_icon(dark: bool) -> egui::IconData {
    let png: &[u8] = if dark {
        include_bytes!("../assets/icons/newen-dark-256.png")
    } else {
        include_bytes!("../assets/icons/newen-256.png")
    };
    let decoded = image::load_from_memory(png)
        .map(|image| image.to_rgba8())
        .unwrap_or_default();
    egui::IconData {
        width: decoded.width(),
        height: decoded.height(),
        rgba: decoded.into_raw(),
    }
}

fn main() -> eframe::Result {
    // Chromium's own processes are Newen run again; they do their work and leave.
    if let Some(code) = newen_engine::native::subprocess() {
        std::process::exit(code);
    }
    newen_files::add_local_bin_to_path();
    wait_for_restarter();
    let args: Vec<String> = std::env::args().collect();
    if let [_, flag, url, ..] = args.as_slice() {
        if flag == "--open-url" {
            std::process::exit(if open_url(url) { 0 } else { 1 });
        }
    }
    if let [_, flag] = args.as_slice() {
        if flag == "--install" {
            std::process::exit(match install_self() {
                Ok(placed) => {
                    println!("Newen {} is installed at {placed}", newen_update::VERSION);
                    0
                }
                Err(error) => {
                    eprintln!("newen: --install: {error}");
                    1
                }
            });
        }
    }
    let trial = launch_update();
    if let (Some(data), Some(config)) = (newen_files::data_dir(), newen_files::config_dir()) {
        let allowed = newen_engine::migrate::allowed(
            cfg!(debug_assertions),
            std::env::var_os("NEWEN_MIGRATE").as_deref(),
        );
        let socket = newen_link::legacy_socket();
        for line in migrate_from_quill(allowed, &socket, &data, &config) {
            update_log(format!("migrate: {line}"));
        }
    }
    // Found again: the installation may have moved with the data folder.
    let trial = trial.map(|(install, running)| {
        (
            newen_update::install::Install::locate().unwrap_or(install),
            running,
        )
    });
    let cwd = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| ".".into());
    let cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    let native = native_browser(&saved_settings());
    let started: Arc<Mutex<Option<newen_engine::Native>>> = Arc::default();
    let keep = started.clone();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: wgpu_setup(native),
            ..Default::default()
        },
        viewport: egui::ViewportBuilder::default()
            .with_title("Newen")
            .with_app_id("newen")
            .with_icon(window_icon(false))
            .with_inner_size([1408.0, 844.0])
            .with_min_inner_size([720.0, 480.0])
            // Where the page is presented beneath it (Wayland), the window is see-through.
            .with_transparent(native && cfg!(target_os = "linux")),
        ..Default::default()
    };
    let result = eframe::run_native(
        "newen",
        options,
        Box::new(move |creation| {
            limn::set_fonts(newen_components::fonts(true, ""));
            limn_egui::install_fonts(&creation.egui_ctx);
            // `NEWEN_THEME=dark|light`, else the system's, else dark.
            let dark = match newen_files::env("THEME")
                .as_deref()
                .and_then(|v| v.to_str())
            {
                Some("light") => false,
                Some("dark") => true,
                _ => creation.egui_ctx.system_theme() != Some(egui::Theme::Light),
            };
            creation.egui_ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            newen_ui::look::set_dark(dark);
            let inbox: Arc<Mutex<Vec<Event>>> = Arc::default();
            let (events, ctx) = (inbox.clone(), creation.egui_ctx.clone());
            // The engine's thread wakes the window for every event it reports.
            let sink = move |event| {
                events.lock().unwrap().push(event);
                ctx.request_repaint();
            };
            // `NEWEN_PROGRAMS=claude=/path/fake-agent` runs another program for an agent, as
            // the engine's tests do, with sessions kept in memory only: a replay to look at.
            // The browser's frames wake the window too.
            let frames = newen_engine::Frames::default();
            let ctx = creation.egui_ctx.clone();
            frames.on_frame(move || ctx.request_repaint());
            let gpu = creation
                .wgpu_render_state
                .as_ref()
                .map(|render| (render.device.clone(), render.queue.clone()));
            let native = native.then(|| start_native(&frames, gpu)).flatten();
            if let Some(native) = &native {
                let ctx = creation.egui_ctx.clone();
                native.pump_with(Arc::new(move |delay| ctx.request_repaint_after(delay)));
            }
            *keep.lock().unwrap() = native.clone();
            let direct = native
                .as_ref()
                .and_then(|native| start_direct(creation, native));
            // A stand-in agent keeps no settings: the welcome would show at every start.
            let skip_welcome = std::env::var_os("NEWEN_PROGRAMS").is_some()
                && std::env::var_os("NEWEN_WELCOME").is_none();
            let engine = match std::env::var("NEWEN_PROGRAMS") {
                Ok(programs) => Engine::start_with(
                    sink,
                    newen_engine::Config {
                        programs: programs
                            .split(',')
                            .filter_map(|p| p.split_once('='))
                            .map(|(agent, program)| (agent.to_owned(), program.to_owned()))
                            .collect(),
                        frames: frames.clone(),
                        native: native.clone(),
                        // The stand-in's sign-in page opens too, so the flow can be watched.
                        open_pages: true,
                        // GitHub's picture is kept like the real app's, so the foot can show it.
                        avatars: newen_engine::Config::default_database().avatars,
                        // The "All agents" layer is kept, but its secrets never reach the
                        // real keychain while programs are stand-ins.
                        extensions: newen_engine::Config::default_database().extensions,
                        // A replay's pictures last as long as its sessions do.
                        pictures: Some(std::env::temp_dir().join("newen-pictures")),
                        secrets: Some(Arc::new(newen_engine::MemorySecrets::default())),
                        ..Default::default()
                    },
                ),
                Err(_) => Engine::start_with(
                    sink,
                    newen_engine::Config {
                        frames: frames.clone(),
                        native: native.clone(),
                        codec: codec_release(),
                        ..newen_engine::Config::default_database()
                    },
                ),
            };
            // A version on trial that ran a minute with its window up and its engine started
            // is healthy: its trial ends, and no later start rolls it back.
            if let Some((install, running)) = trial {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    if install
                        .trial()
                        .is_some_and(|trial| trial.version == running)
                    {
                        install.healthy(&running);
                        update_log(format!("start: {running} ran a minute, its trial is over"));
                    }
                });
            }
            // newen-ide sends code to ask about; a second Newen leaves the socket to the first.
            let linked: Arc<Mutex<Vec<newen_link::Message>>> = Arc::default();
            let link = {
                let (linked, ctx) = (linked.clone(), creation.egui_ctx.clone());
                newen_link::listen(&newen_link::newen_socket(), move |message| {
                    linked.lock().unwrap().push(message);
                    ctx.request_repaint();
                })
                .ok()
            };
            // Which editors are installed is read from disk once, beside the window.
            let editors: Arc<Mutex<Option<Vec<editors::Found>>>> = Arc::default();
            {
                let (editors, ctx) = (editors.clone(), creation.egui_ctx.clone());
                std::thread::spawn(move || {
                    *editors.lock().unwrap() = Some(editors::find());
                    ctx.request_repaint();
                });
            }
            Ok(Box::new(App {
                editors,
                handlers: HashMap::new(),
                linked,
                _link: link,
                tree: UiTree::new(Element::spacer()),
                store: Store::default(),
                ui: Ui {
                    // Tests and demos pin a cheap model; the picker changes it per session.
                    model: std::env::var("NEWEN_MODEL").ok(),
                    name: first_name(),
                    utc_offset: utc_offset(),
                    skip_welcome,
                    ..Ui::new(cwd)
                },
                engine,
                inbox,
                picked: Arc::default(),
                chosen: Arc::default(),
                chromium: Arc::default(),
                skill_file: Arc::default(),
                attached: Arc::default(),
                pasted: Arc::default(),
                restored: false,
                focused: None,
                thumbs_done: Arc::default(),
                thumbs_asked: Default::default(),
                qr_for: None,
                full_done: Arc::default(),
                full_asked: Default::default(),
                clips: Default::default(),
                sizes_done: Arc::default(),
                sizes_asked: Default::default(),
                annotated: Arc::default(),
                terminals: HashMap::new(),
                transcript: None,
                beside: None,
                helper: None,
                applied: None,
                icon_dark: None,
                pinned_theme: newen_files::env("THEME").is_some(),
                frames,
                textures: HashMap::new(),
                direct,
                native: native.clone(),
                page: page::Page::default(),
                browser_shown: None,
                address: None,
                fonts_attached: false,
                fonts_in_force: (newen_model::InterfaceFont::System, String::new()),
                fonts_coming: None,
                archive_checked: None,
            }))
        }),
    );
    // Chromium stops on the thread it started on, its tabs closed first.
    if let Some(native) = started.lock().unwrap().take() {
        native.shutdown();
    }
    result
}

/// A page's frame as egui holds pixels: they are already in its form, so the buffer is
/// taken as it is rather than converted again on the UI thread.
fn surface_frame(size: [usize; 2], rgba: Vec<u8>, scale: f32) -> Option<limn_egui::SurfaceFrame> {
    let pixels: Vec<egui::Color32> = bytemuck::allocation::try_cast_vec(rgba)
        .unwrap_or_else(|(_, rgba)| bytemuck::cast_slice(&rgba).to_vec());
    if pixels.len() != size[0] * size[1] {
        return None;
    }
    limn_egui::SurfaceFrame::from_image(egui::ColorImage::new(size, pixels), scale)
}

/// eframe's device; for native tabs, one that can take in Chromium's GPU buffers (dmabufs),
/// where the driver can, so their paints never leave the GPU.
fn wgpu_setup(native: bool) -> eframe::egui_wgpu::WgpuSetup {
    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    if native {
        // The GPU the compositor reads from (the integrated one on a laptop with two), so the
        // buffers the page is presented in are its to read.
        setup.power_preference = eframe::wgpu::PowerPreference::LowPower;
        let base = setup.device_descriptor.clone();
        setup.device_descriptor = Arc::new(move |adapter| {
            let mut descriptor = base(adapter);
            descriptor.required_features |=
                adapter.features() & eframe::wgpu::Features::VULKAN_EXTERNAL_MEMORY_DMA_BUF;
            descriptor
        });
    }
    eframe::egui_wgpu::WgpuSetup::CreateNew(setup)
}

/// The page presented beneath the window, straight to the compositor, where it is Wayland and
/// takes dmabufs; `None` keeps it drawn by egui.
#[cfg(not(target_os = "linux"))]
fn start_direct(
    _: &eframe::CreationContext,
    _: &newen_engine::Native,
) -> Option<Arc<direct::Direct>> {
    None
}

#[cfg(target_os = "linux")]
fn start_direct(
    creation: &eframe::CreationContext,
    native: &newen_engine::Native,
) -> Option<Arc<direct::Direct>> {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    let render = creation.wgpu_render_state.as_ref()?;
    let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(window)) = (
        creation.display_handle().ok()?.as_raw(),
        creation.window_handle().ok()?.as_raw(),
    ) else {
        return None;
    };
    match direct::Direct::new(display.display.as_ptr(), window.surface.as_ptr(), render) {
        Ok(direct) => {
            direct::prepare(render);
            let shown = direct.clone();
            native.present_with(Arc::new(move |_, page| shown.present(page)));
            Some(direct)
        }
        Err(error) => {
            eprintln!("newen: the page is drawn by the window: {error}");
            None
        }
    }
}

/// Whether the browser's pages are drawn by Chromium in Newen's own process, rather than by
/// a headless one's screencast: when Settings asks. `NEWEN_BROWSER=native` or `headless`
/// decides instead.
fn native_browser(settings: &newen_model::Settings) -> bool {
    match newen_files::env("BROWSER")
        .as_deref()
        .and_then(|v| v.to_str())
    {
        Some("native") => true,
        Some("headless") => false,
        // Windows has no headless Chromium to drive (chromium.rs) and both it and macOS ship
        // CEF in the bundle: there it is the browser.
        _ => settings.browser_native || !cfg!(target_os = "linux"),
    }
}

/// The person's settings as the engine keeps them, read before the window opens.
fn saved_settings() -> newen_model::Settings {
    newen_engine::Config::default_database()
        .settings
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Start Chromium in Newen's process, for native tabs, its paints copied on the window's
/// `gpu` where it can; `None` keeps the headless one.
fn start_native(
    frames: &newen_engine::Frames,
    gpu: Option<(eframe::wgpu::Device, eframe::wgpu::Queue)>,
) -> Option<newen_engine::Native> {
    let data = newen_engine::Config::default_database()
        .database
        .and_then(|db| db.parent().map(|dir| dir.join("chromium")))
        .unwrap_or_else(|| std::env::temp_dir().join("newen-chromium"));
    match newen_engine::Native::start(&data, frames.clone(), gpu) {
        Ok(native) => Some(native),
        Err(error) => {
            eprintln!("newen: the browser draws without native tabs: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// quill's data moves only when allowed and when no copy of quill answers on its socket;
    /// then it does, once.
    #[test]
    fn quill_s_data_moves_only_when_allowed_and_no_copy_of_quill_runs() {
        let base = std::env::temp_dir().join(format!("newen-first-start-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (data, config, socket) = (base.join("data"), base.join("config"), base.join("q.sock"));
        std::fs::create_dir_all(data.join("quill")).unwrap();
        std::fs::write(data.join("quill/quill.db"), "").unwrap();
        let listening = newen_link::listen(&socket, |_| {}).unwrap();
        let said = migrate_from_quill(true, &socket, &data, &config);
        assert!(said[0].starts_with("a copy of quill runs"), "{said:?}");
        assert!(data.join("quill").is_dir() && !data.join("newen").exists());
        drop(listening);
        assert!(migrate_from_quill(false, &socket, &data, &config).is_empty());
        assert!(!data.join("newen").exists(), "not allowed: nothing moves");
        let said = migrate_from_quill(true, &socket, &data, &config);
        assert_eq!(said.first().map(String::as_str), Some("data folder: done"));
        assert!(data.join("newen").is_dir());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A restart names this process in Newen's variable alone (quill 0.1.9 reads it too); the
    /// pid read is the variable's, from Newen's or from quill's when quill started this one.
    #[test]
    fn a_restart_names_this_process_in_newen_s_variable() {
        let command = restart_command(std::path::Path::new("/nowhere/newen"));
        let pid = std::process::id().to_string();
        let envs: Vec<_> = command.get_envs().collect();
        assert_eq!(
            envs,
            [(
                std::ffi::OsStr::new("NEWEN_RESTART_AFTER"),
                Some(std::ffi::OsStr::new(&pid))
            )]
        );
        assert_eq!(restarter(Some("42".into())), Some(42));
        assert_eq!(restarter(None), None);
        assert_eq!(restarter(Some("not a pid".into())), None);
    }
}
