//! quill's built-in browser (docs/browser.md): one Chromium, launched when first needed and
//! driven over the DevTools protocol, with a browser context per session so cookies stay
//! apart. The person sees the tab on screen through a screencast into a limn surface and
//! drives it with the pointer and keys; the agent drives the same tabs with its tools.

mod cdp;
mod chromium;
mod frames;
mod input;
pub mod native;
pub mod tools;

pub use cdp::Error;
pub use frames::{decode, Frame, Frames, Pixels};
pub use native::{Native, Present};

use cdp::{Cdp, Message};
use newen_model::{BrowserInput, BrowserOp, BrowserTab, BrowserUpdate, BrowserView, PointerButton};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Whose tabs: quill's session id.
pub type Owner = u64;

/// Where updates go, for the engine to pass to the UI.
pub type Updates = Arc<dyn Fn(Owner, BrowserUpdate) + Send + Sync>;

/// The person's answer to an agent wanting a site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grant {
    Once,
    /// From now on: the site joins the allowed ones.
    Always,
    No,
}

/// Asks the person whether `owner`'s agent may open or use the site `host` (`url` is what it
/// wants); the answer comes on the receiver. Desktop's card: "Allow once",
/// "Always allow for this website", "Don't allow".
pub type Permit =
    Arc<dyn Fn(Owner, SiteAsk) -> tokio::sync::oneshot::Receiver<Grant> + Send + Sync>;

#[derive(Clone, Debug, PartialEq)]
pub struct SiteAsk {
    pub host: String,
    pub url: String,
    /// Opening the site, or acting in a tab already on it.
    pub opening: bool,
}

/// Desktop stops at nine tabs counting its seed tab.
pub const MAX_TABS: usize = 8;
/// How long Chromium runs with no tab open before it is closed, as Desktop destroys a hidden
/// pane's views after 5 minutes.
const IDLE: Duration = Duration::from_secs(5 * 60);
/// A tab's view until the pane shows it.
const DEFAULT_VIEW: (u32, u32) = (1280, 800);
/// How much of a page's console and network a tab keeps for the agent.
const KEPT: usize = 300;

pub struct Options {
    /// The Chromium Settings names; `NEWEN_CHROME` and `PATH` otherwise.
    pub program: Option<PathBuf>,
    /// Chromium's profile: what it keeps between launches, which is not cookies (they live in
    /// each session's browser context, in memory).
    pub profile: PathBuf,
    /// Where downloads are saved.
    pub downloads: PathBuf,
    pub frames: Frames,
    /// Asks the person before the agent opens or uses a site that is not local nor allowed.
    pub permit: Permit,
    /// Chromium in quill's own process, when the host started it: tabs paint into their
    /// surfaces directly, and no Chromium is launched.
    pub native: Option<Native>,
}

#[derive(Clone)]
pub struct Browser {
    inner: Arc<Inner>,
}

struct Inner {
    options: Options,
    updates: Updates,
    run: Mutex<Option<Arc<Run>>>,
    /// Held while Chromium is launched, so two callers never launch two.
    launching: tokio::sync::Mutex<()>,
    /// What the person did, carried out in order (`Browser::send`).
    ops: tokio::sync::mpsc::UnboundedSender<(Owner, BrowserOp)>,
    /// Sites the agent may use without asking, from Settings and "Always allow".
    allowed: Mutex<Vec<String>>,
    /// The Chromium Settings names, for the next launch.
    program: Mutex<Option<PathBuf>>,
    /// Each session's tabs from when quill last ran, opened again with its browser.
    remembered: Mutex<HashMap<Owner, Vec<String>>>,
    /// Since when no session has a tab open, while Chromium runs.
    idle_since: Mutex<Option<Instant>>,
}

/// A running Chromium.
pub(crate) struct Run {
    pub(crate) cdp: Cdp,
    /// Closed on purpose: its going is not a failure to report.
    closing: std::sync::atomic::AtomicBool,
    pub(crate) state: Mutex<State>,
    /// The launched Chromium; none inside quill's window.
    child: Mutex<Option<std::process::Child>>,
    native: Option<Native>,
    decode: std::sync::mpsc::Sender<Shot>,
}

#[derive(Default)]
pub(crate) struct State {
    pub(crate) owners: HashMap<Owner, Owned>,
    /// The tab the pane shows, whose frames are drawn.
    shown: Option<(Owner, u32)>,
    /// Downloads under way, by Chromium's id: whose, and the file's name.
    downloads: HashMap<String, (Owner, String)>,
}

pub(crate) struct Owned {
    context: String,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: Option<u32>,
    next: u32,
    /// What closed tabs showed, newest last, for Ctrl+Shift+T.
    closed: Vec<String>,
    /// The tabs' addresses as last told to be kept.
    kept: Vec<String>,
}

pub(crate) struct Tab {
    pub(crate) id: u32,
    target: String,
    /// The page's DevTools session.
    pub(crate) session: String,
    pub(crate) url: String,
    pub(crate) title: String,
    loading: bool,
    back: bool,
    forward: bool,
    /// The pane's size for it, and the one the agent asked for, which wins.
    view: (u32, u32),
    pub(crate) agent_view: Option<(u32, u32)>,
    /// The agent set `agent_view`, not the person.
    by_agent: bool,
    held: Option<PointerButton>,
    cursor_asked: Option<Instant>,
    pub(crate) console: VecDeque<String>,
    pub(crate) network: VecDeque<Request>,
    /// The page opened it: the agent may not act in it (Desktop's rule).
    pub(crate) popup: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub(crate) id: String,
    pub(crate) method: String,
    pub(crate) url: String,
    pub(crate) status: Option<String>,
}

/// A frame to decode and the acknowledgement Chromium waits for before sending the next.
struct Shot {
    surface: String,
    data: String,
    ack: Value,
    session: String,
    scale: f32,
}

impl Tab {
    fn info(&self) -> BrowserTab {
        BrowserTab {
            id: self.id,
            url: self.url.clone(),
            title: self.title.clone(),
            loading: self.loading,
            back: self.back,
            forward: self.forward,
            emulated: self.agent_view,
            emulated_by_agent: self.by_agent && self.agent_view.is_some(),
        }
    }

    fn size(&self) -> (u32, u32) {
        self.agent_view.unwrap_or(self.view)
    }
}

/// The surface a tab's frames are presented on.
pub fn surface(owner: Owner, tab: u32) -> String {
    format!("browser:{owner}:{tab}")
}

impl Browser {
    /// A browser that launches Chromium when first needed. Call it inside a tokio runtime:
    /// it keeps a task carrying out what the person does, in order.
    pub fn new(options: Options, updates: Updates) -> Self {
        let (ops, mut queue) = tokio::sync::mpsc::unbounded_channel();
        let program = Mutex::new(options.program.clone());
        let browser = Self {
            inner: Arc::new(Inner {
                options,
                updates,
                run: Mutex::new(None),
                launching: tokio::sync::Mutex::new(()),
                ops,
                allowed: Mutex::default(),
                program,
                remembered: Mutex::default(),
                idle_since: Mutex::default(),
            }),
        };
        // Chromium goes once nothing has used it for a while.
        let weak = Arc::downgrade(&browser.inner);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let idle = inner
                    .idle_since
                    .lock()
                    .unwrap()
                    .is_some_and(|since| since.elapsed() >= IDLE);
                if idle {
                    *inner.idle_since.lock().unwrap() = None;
                    Browser { inner }.shutdown().await;
                }
            }
        });
        let weak = Arc::downgrade(&browser.inner);
        tokio::spawn(async move {
            while let Some((owner, op)) = queue.recv().await {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let browser = Browser { inner };
                // Loading a page may take long; it does not hold up what comes after it.
                if matches!(
                    op,
                    BrowserOp::Open { .. }
                        | BrowserOp::NewTab { .. }
                        | BrowserOp::Navigate { .. }
                        | BrowserOp::Back { .. }
                        | BrowserOp::Forward { .. }
                        | BrowserOp::Reload { .. }
                ) {
                    tokio::spawn(async move { browser.apply(owner, op).await });
                } else {
                    browser.apply(owner, op).await;
                }
            }
        });
        browser
    }

    /// `owner`'s tabs from when quill last ran: they open again the first time its browser
    /// does without an address.
    pub fn remember(&self, owner: Owner, urls: Vec<String>) {
        if !urls.is_empty() {
            self.inner.remembered.lock().unwrap().insert(owner, urls);
        }
    }

    /// The Chromium Settings names; it runs from the next launch.
    pub fn use_program(&self, program: Option<PathBuf>) {
        *self.inner.program.lock().unwrap() = program;
    }

    /// The sites the agent may use without asking, as Settings has them.
    pub fn allow_sites(&self, sites: Vec<String>) {
        *self.inner.allowed.lock().unwrap() = sites;
    }

    /// May the agent open or use `url`? Local pages always; another site when it, or a
    /// domain it is under, is allowed, or when the person says so now. `file:` and other
    /// schemes never: the agent's pages are web pages.
    pub(crate) async fn may(&self, owner: Owner, url: &str, opening: bool) -> Result<(), String> {
        let host = match site(url) {
            Site::Local => return Ok(()),
            Site::Other(scheme) => {
                return Err(format!(
                    "The agent's browser opens web pages only, not {scheme}: addresses."
                ))
            }
            Site::Web(host) => host,
        };
        let allowed = self
            .inner
            .allowed
            .lock()
            .unwrap()
            .iter()
            .any(|site| host == *site || host.ends_with(&format!(".{site}")));
        if allowed {
            return Ok(());
        }
        let ask = SiteAsk {
            host: host.clone(),
            url: url.to_owned(),
            opening,
        };
        match (self.inner.options.permit)(owner, ask).await {
            Ok(Grant::Once) => Ok(()),
            Ok(Grant::Always) => {
                self.inner.allowed.lock().unwrap().push(host);
                Ok(())
            }
            _ => Err(format!(
                "The user did not allow the browser on {host}. Do not try it again unless they ask."
            )),
        }
    }

    /// Carry out `op` after what the person did before it; input goes to the page at once.
    pub fn send(&self, owner: Owner, op: BrowserOp) {
        if let BrowserOp::Input { tab, input } = op {
            if let Some(run) = self.running() {
                self.input(&run, owner, tab, input);
            }
            return;
        }
        let _ = self.inner.ops.send((owner, op));
    }

    fn running(&self) -> Option<Arc<Run>> {
        self.inner
            .run
            .lock()
            .unwrap()
            .clone()
            .filter(|run| run.cdp.alive())
    }

    fn tell(&self, owner: Owner, update: BrowserUpdate) {
        (self.inner.updates)(owner, update);
    }

    /// The running Chromium, launched now if there is none or the last one stopped.
    pub(crate) async fn run(&self) -> Result<Arc<Run>, String> {
        let _launching = self.inner.launching.lock().await;
        if let Some(run) = self.running() {
            return Ok(run);
        }
        let options = &self.inner.options;
        let scale = options.frames.scale();
        let (events, heard) = tokio::sync::mpsc::unbounded_channel();
        // Inside quill's window the same stream is answered by the Chromium already running.
        let (cdp, child) = match &options.native {
            Some(native) => {
                let (reader, writer) = native.pipe();
                (Cdp::start(reader, writer, events), None)
            }
            None => {
                let configured = self.inner.program.lock().unwrap().clone();
                let program = chromium::find(configured.as_deref()).ok_or_else(|| {
                    "No Chromium found. Install chromium or google-chrome, or set its path in Settings."
                        .to_owned()
                })?;
                std::fs::create_dir_all(&options.profile).map_err(|e| e.to_string())?;
                let process = chromium::launch(&program, &options.profile, scale)
                    .map_err(|e| format!("The built-in browser couldn't start: {e}"))?;
                (
                    Cdp::start(process.reader, process.writer, events),
                    Some(process.child),
                )
            }
        };
        let (decoder, shots) = std::sync::mpsc::channel::<Shot>();
        let (frames, acks) = (options.frames.clone(), cdp.clone());
        std::thread::Builder::new()
            .name("quill-browser-frames".into())
            .spawn(move || {
                use base64::Engine;
                while let Ok(shot) = shots.recv() {
                    // Only the newest waiting frame of a burst is worth decoding.
                    let shot = std::iter::from_fn(|| shots.try_recv().ok())
                        .last()
                        .unwrap_or(shot);
                    let jpeg = base64::engine::general_purpose::STANDARD.decode(&shot.data);
                    if let Some(frame) = jpeg.ok().and_then(|jpeg| decode(&jpeg, shot.scale)) {
                        frames.publish(shot.surface, frame);
                    }
                    acks.send(
                        "Page.screencastFrameAck",
                        json!({ "sessionId": shot.ack }),
                        Some(&shot.session),
                    );
                }
            })
            .map_err(|e| e.to_string())?;
        let run = Arc::new(Run {
            cdp,
            closing: Default::default(),
            state: Mutex::default(),
            child: Mutex::new(child),
            native: options.native.clone(),
            decode: decoder,
        });
        // Titles and addresses as they change, and the tabs pages open.
        run.cdp
            .call(
                "Target.setDiscoverTargets",
                json!({ "discover": true }),
                None,
            )
            .await
            .map_err(|e| format!("The built-in browser couldn't start: {e}"))?;
        tokio::spawn(listen(self.clone(), run.clone(), heard, scale));
        *self.inner.run.lock().unwrap() = Some(run.clone());
        Ok(run)
    }

    /// Carry out what the person did in `owner`'s browser.
    pub async fn apply(&self, owner: Owner, op: BrowserOp) {
        if let Err(error) = self.apply_inner(owner, op).await {
            self.tell(owner, BrowserUpdate::Failed(error));
        }
    }

    async fn apply_inner(&self, owner: Owner, op: BrowserOp) -> Result<(), String> {
        // Only opening starts Chromium; the rest acts on what is there.
        let run = match &op {
            BrowserOp::Open { .. } | BrowserOp::NewTab { .. } => self.run().await?,
            _ => match self.running() {
                Some(run) => run,
                None => return Ok(()),
            },
        };
        match op {
            BrowserOp::Open { url } => {
                let blank = {
                    let state = run.state.lock().unwrap();
                    state.owners.get(&owner).map(|o| {
                        let active = o.tabs.iter().find(|t| Some(t.id) == o.active);
                        active.filter(|t| t.url == "about:blank").map(|t| t.id)
                    })
                };
                match (url, blank) {
                    (Some(url), Some(Some(tab))) => self.navigate(&run, owner, tab, &url).await?,
                    (Some(url), _) => drop(self.new_tab(&run, owner, Some(&url)).await?),
                    (None, Some(_)) => {}
                    (None, None) => {
                        // A session's tabs from before quill restarted come back.
                        let before = self.inner.remembered.lock().unwrap().remove(&owner);
                        let urls = before.unwrap_or_default();
                        if urls.is_empty() {
                            self.new_tab(&run, owner, None).await?;
                        }
                        for url in urls.iter().take(MAX_TABS) {
                            self.new_tab(&run, owner, Some(url)).await?;
                        }
                    }
                }
            }
            BrowserOp::NewTab { url } => drop(self.new_tab(&run, owner, url.as_deref()).await?),
            BrowserOp::Navigate { tab, url } => self.navigate(&run, owner, tab, &url).await?,
            BrowserOp::CloseTab { tab } => self.close_tab(&run, owner, tab).await,
            BrowserOp::SelectTab { tab } => {
                if let Some(owned) = run.state.lock().unwrap().owners.get_mut(&owner) {
                    if owned.tabs.iter().any(|t| t.id == tab) {
                        owned.active = Some(tab);
                    }
                }
                self.publish(&run, owner);
            }
            BrowserOp::Back { tab } => self.history(&run, owner, tab, -1).await?,
            BrowserOp::Forward { tab } => self.history(&run, owner, tab, 1).await?,
            BrowserOp::Reload { tab } => {
                if let Some(session) = run.session(owner, tab) {
                    run.cdp
                        .call("Page.reload", json!({}), Some(&session))
                        .await
                        .map_err(|e| e.0)?;
                }
            }
            BrowserOp::Stop { tab } => {
                if let Some(session) = run.session(owner, tab) {
                    run.cdp.send("Page.stopLoading", json!({}), Some(&session));
                }
            }
            BrowserOp::Show { view } => self.show(&run, owner, view).await,
            BrowserOp::Input { tab, input } => self.input(&run, owner, tab, input),
            BrowserOp::Copy { tab } => {
                if let Some(session) = run.session(owner, tab) {
                    let text = run
                        .evaluate(&session, "String(getSelection())")
                        .await
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default();
                    if !text.is_empty() {
                        self.tell(owner, BrowserUpdate::Copied(text));
                    }
                }
            }
            BrowserOp::Viewport { tab, size } => {
                self.emulate(&run, owner, tab, size, false).await?;
                self.publish(&run, owner);
            }
            BrowserOp::Files { tab, node, paths } => {
                let Some(session) = run.session(owner, tab) else {
                    return Ok(());
                };
                if !paths.is_empty() {
                    let files: Vec<String> =
                        paths.iter().map(|p| p.display().to_string()).collect();
                    run.cdp
                        .call(
                            "DOM.setFileInputFiles",
                            json!({ "files": files, "backendNodeId": node }),
                            Some(&session),
                        )
                        .await
                        .map_err(|e| e.0)?;
                }
            }
            // Chromium's own inspect mode highlights what is under the pointer and reports
            // the element pressed (`Overlay.inspectNodeRequested`).
            BrowserOp::Select { tab, on } => {
                let Some(session) = run.session(owner, tab) else {
                    return Ok(());
                };
                if on {
                    for method in ["DOM.enable", "Overlay.enable"] {
                        run.cdp
                            .call(method, json!({}), Some(&session))
                            .await
                            .map_err(|e| e.0)?;
                    }
                    run.cdp
                        .call(
                            "Overlay.setInspectMode",
                            json!({ "mode": "searchForNode", "highlightConfig": {
                                "showInfo": true,
                                "contentColor": { "r": 37, "g": 125, "b": 224, "a": 0.18 },
                                "borderColor": { "r": 37, "g": 125, "b": 224, "a": 0.9 },
                            } }),
                            Some(&session),
                        )
                        .await
                        .map_err(|e| e.0)?;
                } else {
                    stop_inspecting(&run, &session);
                }
                self.tell(owner, BrowserUpdate::Selecting(on));
            }
        }
        Ok(())
    }

    /// A new tab for `owner`, at `url` or blank, made the active one.
    pub(crate) async fn new_tab(
        &self,
        run: &Arc<Run>,
        owner: Owner,
        url: Option<&str>,
    ) -> Result<u32, String> {
        let context = self.context(run, owner).await?;
        if run.state.lock().unwrap().owners[&owner].tabs.len() >= MAX_TABS {
            return Err(format!("{MAX_TABS} tabs are open: close one first."));
        }
        let url = url.map(address).unwrap_or_else(|| "about:blank".into());
        let target = run
            .cdp
            .call(
                "Target.createTarget",
                json!({ "url": "about:blank", "browserContextId": context,
                        "width": DEFAULT_VIEW.0, "height": DEFAULT_VIEW.1 }),
                None,
            )
            .await
            .map_err(|e| e.0)?["targetId"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let tab = self.adopt(run, owner, &target, false).await?;
        if url != "about:blank" {
            self.navigate(run, owner, tab, &url).await?;
        }
        Ok(tab)
    }

    /// Attach to `target` and keep it as one of `owner`'s tabs, the active one.
    async fn adopt(
        &self,
        run: &Arc<Run>,
        owner: Owner,
        target: &str,
        popup: bool,
    ) -> Result<u32, String> {
        let session = run
            .cdp
            .call(
                "Target.attachToTarget",
                json!({ "targetId": target, "flatten": true }),
                None,
            )
            .await
            .map_err(|e| e.0)?["sessionId"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let mut setup = vec![
            ("Page.enable", json!({})),
            ("Runtime.enable", json!({})),
            ("Network.enable", json!({})),
            // A page's file field asks quill, which shows the desktop's dialog.
            (
                "Page.setInterceptFileChooserDialog",
                json!({ "enabled": true }),
            ),
        ];
        // A page that thinks it has the focus shows its caret and :focus styles.
        setup.push((
            "Emulation.setFocusEmulationEnabled",
            json!({ "enabled": true }),
        ));
        // A native tab is drawn at its own size, which an emulated one would fight.
        if run.native.is_none() {
            setup.push((
                "Emulation.setDeviceMetricsOverride",
                json!({ "width": DEFAULT_VIEW.0, "height": DEFAULT_VIEW.1,
                        "deviceScaleFactor": 0, "mobile": false }),
            ));
        }
        for (method, params) in setup {
            run.cdp
                .call(method, params, Some(&session))
                .await
                .map_err(|e| e.0)?;
        }
        let id = {
            let mut state = run.state.lock().unwrap();
            let Some(owned) = state.owners.get_mut(&owner) else {
                return Err("the session's browser closed".into());
            };
            owned.next += 1;
            let id = owned.next;
            owned.tabs.push(Tab {
                id,
                target: target.to_owned(),
                session,
                url: "about:blank".into(),
                title: String::new(),
                loading: false,
                back: false,
                forward: false,
                view: DEFAULT_VIEW,
                agent_view: None,
                by_agent: false,
                held: None,
                cursor_asked: None,
                console: VecDeque::new(),
                network: VecDeque::new(),
                popup,
            });
            owned.active = Some(id);
            id
        };
        self.publish(run, owner);
        Ok(id)
    }

    /// `owner`'s browser context, made now if it has none.
    async fn context(&self, run: &Arc<Run>, owner: Owner) -> Result<String, String> {
        if let Some(owned) = run.state.lock().unwrap().owners.get(&owner) {
            return Ok(owned.context.clone());
        }
        let context = run
            .cdp
            .call("Target.createBrowserContext", json!({}), None)
            .await
            .map_err(|e| e.0)?["browserContextId"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        // Downloads land in the person's folder and are reported.
        let downloads = self.inner.options.downloads.display().to_string();
        let _ = run
            .cdp
            .call(
                "Browser.setDownloadBehavior",
                json!({ "behavior": "allow", "browserContextId": context,
                        "downloadPath": downloads, "eventsEnabled": true }),
                None,
            )
            .await;
        run.state.lock().unwrap().owners.insert(
            owner,
            Owned {
                context: context.clone(),
                tabs: Vec::new(),
                active: None,
                next: 0,
                closed: Vec::new(),
                kept: Vec::new(),
            },
        );
        Ok(context)
    }

    pub(crate) async fn navigate(
        &self,
        run: &Arc<Run>,
        owner: Owner,
        tab: u32,
        url: &str,
    ) -> Result<(), String> {
        let Some(session) = run.session(owner, tab) else {
            return Err("That tab is closed.".into());
        };
        let url = address(url);
        let reply = run
            .cdp
            .call("Page.navigate", json!({ "url": url }), Some(&session))
            .await
            .map_err(|e| e.0)?;
        match reply["errorText"].as_str() {
            Some(error) if !error.is_empty() => Err(format!("Couldn't open {url}: {error}")),
            _ => Ok(()),
        }
    }

    async fn history(
        &self,
        run: &Arc<Run>,
        owner: Owner,
        tab: u32,
        step: i64,
    ) -> Result<(), String> {
        let Some(session) = run.session(owner, tab) else {
            return Ok(());
        };
        let history = run
            .cdp
            .call("Page.getNavigationHistory", json!({}), Some(&session))
            .await
            .map_err(|e| e.0)?;
        let at = history["currentIndex"].as_i64().unwrap_or(0) + step;
        if let Some(entry) = history["entries"]
            .get(at.max(0) as usize)
            .filter(|_| at >= 0)
        {
            run.cdp
                .call(
                    "Page.navigateToHistoryEntry",
                    json!({ "entryId": entry["id"] }),
                    Some(&session),
                )
                .await
                .map_err(|e| e.0)?;
        }
        Ok(())
    }

    async fn close_tab(&self, run: &Arc<Run>, owner: Owner, tab: u32) {
        let target = {
            let mut state = run.state.lock().unwrap();
            if state.shown == Some((owner, tab)) {
                state.shown = None;
            }
            let Some(owned) = state.owners.get_mut(&owner) else {
                return;
            };
            let Some(at) = owned.tabs.iter().position(|t| t.id == tab) else {
                return;
            };
            let closed = owned.tabs.remove(at);
            if closed.url != "about:blank" {
                owned.closed.push(closed.url);
            }
            if owned.active == Some(tab) {
                owned.active = owned
                    .tabs
                    .get(at.saturating_sub(1))
                    .or(owned.tabs.first())
                    .map(|t| t.id);
            }
            closed.target
        };
        let _ = run
            .cdp
            .call("Target.closeTarget", json!({ "targetId": target }), None)
            .await;
        self.publish(run, owner);
    }

    /// The pane shows `view`, or nothing: the tab on screen is sized to it and screencast;
    /// any other stops.
    async fn show(&self, run: &Arc<Run>, owner: Owner, view: Option<BrowserView>) {
        let wanted = view.map(|v| (owner, v.tab));
        let (before, session, size) = {
            let mut state = run.state.lock().unwrap();
            let before = state.shown;
            // Another session's pane being hidden says nothing about this one.
            if view.is_none() && before.is_some_and(|(o, _)| o != owner) {
                return;
            }
            state.shown = wanted;
            let (session, size) = match view {
                Some(view) => match state.tab_mut(owner, view.tab) {
                    Some(tab) => {
                        tab.view = (view.width.max(1), view.height.max(1));
                        (Some(tab.session.clone()), tab.size())
                    }
                    None => {
                        state.shown = None;
                        (None, (0, 0))
                    }
                },
                None => (None, (0, 0)),
            };
            let before = before
                .filter(|b| Some(*b) != wanted)
                .and_then(|(o, t)| state.session(o, t));
            (before, session, size)
        };
        if let (Some(before), None) = (before, &run.native) {
            run.cdp
                .send("Page.stopScreencast", json!({}), Some(&before));
        }
        let Some(session) = session else {
            if let Some(native) = &run.native {
                native.show(None);
            }
            return;
        };
        // A native tab paints into its surface itself, at the size it is given.
        if let Some(native) = &run.native {
            native.show(view.map(|v| (session, surface(owner, v.tab), size)));
            return;
        }
        let _ = run
            .cdp
            .call(
                "Emulation.setDeviceMetricsOverride",
                json!({ "width": size.0, "height": size.1, "deviceScaleFactor": 0, "mobile": false }),
                Some(&session),
            )
            .await;
        // Optional keys are left out: a null one stops Chromium sending frames at all.
        let _ = run
            .cdp
            .call(
                "Page.startScreencast",
                json!({ "format": "jpeg", "quality": 85, "everyNthFrame": 1 }),
                Some(&session),
            )
            .await;
    }

    fn input(&self, run: &Arc<Run>, owner: Owner, tab: u32, input: BrowserInput) {
        let (session, ask_cursor) = {
            let mut state = run.state.lock().unwrap();
            let Some(page) = state.tab_mut(owner, tab) else {
                return;
            };
            for (method, params) in input::commands(&input, &mut page.held) {
                run.cdp.send(method, params, Some(&page.session));
            }
            // The page's own cursor is asked for now and then while the pointer moves:
            // DevTools does not report it.
            let due = page
                .cursor_asked
                .is_none_or(|at| at.elapsed() > Duration::from_millis(120));
            let ask = matches!(input, BrowserInput::Move { .. }) && due;
            if ask {
                page.cursor_asked = Some(Instant::now());
            }
            (page.session.clone(), ask.then_some(input))
        };
        if let Some(BrowserInput::Move { x, y }) = ask_cursor {
            let (this, run) = (self.clone(), run.clone());
            tokio::spawn(async move {
                let script = format!(
                    "(e=>{{if(!e)return'default';const c=getComputedStyle(e).cursor;if(c!=='auto')return c;\
                     return(e.tagName==='INPUT'||e.tagName==='TEXTAREA'||e.isContentEditable)?'text':'default'}})\
                     (document.elementFromPoint({x},{y}))"
                );
                if let Ok(Value::String(cursor)) = run.evaluate(&session, &script).await {
                    this.tell(owner, BrowserUpdate::Cursor(cursor));
                }
            });
        }
    }

    /// Draw `tab` at `size`, or at the pane's size again, as the agent asked or the person
    /// chose. A phone's width is emulated as a phone, so the page's viewport meta applies.
    pub(crate) async fn emulate(
        &self,
        run: &Arc<Run>,
        owner: Owner,
        tab: u32,
        size: Option<(u32, u32)>,
        by_agent: bool,
    ) -> Result<(u32, u32), String> {
        let (session, shown) = {
            let mut state = run.state.lock().unwrap();
            let page = state.tab_mut(owner, tab).ok_or("That tab is closed.")?;
            page.agent_view = size.filter(|(w, h)| *w > 0 && *h > 0);
            page.by_agent = by_agent;
            (page.session.clone(), page.size())
        };
        let mobile = size.is_some_and(|(w, _)| w <= 480);
        // A native tab is drawn at the size asked for, or the pane's; only a phone is emulated.
        if let Some(native) = &run.native {
            native.resize(session.clone(), shown);
            if !mobile {
                run.cdp
                    .call(
                        "Emulation.clearDeviceMetricsOverride",
                        json!({}),
                        Some(&session),
                    )
                    .await
                    .map_err(|e| e.0)?;
                return Ok(shown);
            }
        }
        run.cdp
            .call(
                "Emulation.setDeviceMetricsOverride",
                json!({ "width": shown.0, "height": shown.1, "deviceScaleFactor": 0, "mobile": mobile }),
                Some(&session),
            )
            .await
            .map_err(|e| e.0)?;
        Ok(shown)
    }

    /// The value of `expression` in one of `owner`'s tabs.
    pub async fn evaluate(
        &self,
        owner: Owner,
        tab: u32,
        expression: &str,
    ) -> Result<Value, String> {
        let run = self.running().ok_or("the browser is not running")?;
        let session = run.session(owner, tab).ok_or("That tab is closed.")?;
        run.evaluate(&session, expression).await.map_err(|e| e.0)
    }

    /// Forget `owner`'s tabs and cookies: its session was archived or deleted.
    pub async fn forget(&self, owner: Owner) {
        let Some(run) = self.running() else {
            return;
        };
        let owned = {
            let mut state = run.state.lock().unwrap();
            if state.shown.is_some_and(|(o, _)| o == owner) {
                state.shown = None;
            }
            state.owners.remove(&owner)
        };
        if let Some(owned) = owned {
            let _ = run
                .cdp
                .call(
                    "Target.disposeBrowserContext",
                    json!({ "browserContextId": owned.context }),
                    None,
                )
                .await;
        }
    }

    /// Close Chromium, if it runs.
    pub async fn shutdown(&self) {
        let Some(run) = self.inner.run.lock().unwrap().take() else {
            return;
        };
        run.closing.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = run
            .cdp
            .call_within("Browser.close", json!({}), None, Duration::from_secs(2))
            .await;
        let mut child = run.child.lock().unwrap();
        if let Some(child) = child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub(crate) fn publish(&self, run: &Arc<Run>, owner: Owner) {
        let (update, kept) = {
            let mut state = run.state.lock().unwrap();
            // The addresses are kept when they change, to open again after a restart.
            let kept = state.owners.get_mut(&owner).and_then(|owned| {
                let urls: Vec<String> = owned
                    .tabs
                    .iter()
                    .map(|t| t.url.clone())
                    .filter(|u| u != "about:blank" && !u.is_empty())
                    .collect();
                (urls != owned.kept).then(|| {
                    owned.kept = urls.clone();
                    urls
                })
            });
            let open = state.owners.values().any(|o| !o.tabs.is_empty());
            let mut idle = self.inner.idle_since.lock().unwrap();
            *idle = if open {
                None
            } else {
                idle.or(Some(Instant::now()))
            };
            let Some(owned) = state.owners.get(&owner) else {
                return;
            };
            let update = BrowserUpdate::Tabs {
                tabs: owned.tabs.iter().map(Tab::info).collect(),
                active: owned.active,
            };
            (update, kept)
        };
        self.tell(owner, update);
        if let Some(urls) = kept {
            self.tell(owner, BrowserUpdate::Kept(urls));
        }
    }
}

/// Chromium stops with the browser that launched it.
impl Drop for Run {
    fn drop(&mut self) {
        if let Ok(Some(child)) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Run {
    pub(crate) fn session(&self, owner: Owner, tab: u32) -> Option<String> {
        self.state.lock().unwrap().session(owner, tab)
    }

    /// The value of `expression` in the page, awaited if it is a promise.
    pub(crate) async fn evaluate(&self, session: &str, expression: &str) -> Result<Value, Error> {
        let reply = self
            .cdp
            .call(
                "Runtime.evaluate",
                json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
                Some(session),
            )
            .await?;
        if let Some(exception) = reply.get("exceptionDetails") {
            let text = exception["exception"]["description"]
                .as_str()
                .or(exception["text"].as_str())
                .unwrap_or("the script failed");
            return Err(Error(text.to_owned()));
        }
        Ok(reply["result"]["value"].clone())
    }
}

impl State {
    fn session(&self, owner: Owner, tab: u32) -> Option<String> {
        let owned = self.owners.get(&owner)?;
        owned
            .tabs
            .iter()
            .find(|t| t.id == tab)
            .map(|t| t.session.clone())
    }

    pub(crate) fn tab_mut(&mut self, owner: Owner, tab: u32) -> Option<&mut Tab> {
        self.owners
            .get_mut(&owner)?
            .tabs
            .iter_mut()
            .find(|t| t.id == tab)
    }

    /// The tab of DevTools session `session`, with its owner.
    fn by_session(&mut self, session: &str) -> Option<(Owner, &mut Tab)> {
        self.owners.iter_mut().find_map(|(owner, owned)| {
            owned
                .tabs
                .iter_mut()
                .find(|t| t.session == session)
                .map(|t| (*owner, t))
        })
    }

    fn by_target(&mut self, target: &str) -> Option<(Owner, &mut Tab)> {
        self.owners.iter_mut().find_map(|(owner, owned)| {
            owned
                .tabs
                .iter_mut()
                .find(|t| t.target == target)
                .map(|t| (*owner, t))
        })
    }

    fn by_context(&self, context: &str) -> Option<Owner> {
        self.owners
            .iter()
            .find(|(_, o)| o.context == context)
            .map(|(owner, _)| *owner)
    }
}

/// What kind of place a URL is, for the agent's permissions.
#[derive(Debug, PartialEq)]
pub(crate) enum Site {
    /// This computer, or a page with no site (about:blank, data:).
    Local,
    Web(String),
    /// A scheme the agent may not open, such as `file`.
    Other(String),
}

/// Whether `url` is a page of this computer (localhost, 127.0.0.1…) with no credentials in
/// it: what a session's tools may open in its browser through `$BROWSER`.
pub fn is_local(url: &str) -> bool {
    let authority = url.split_once("://").map_or("", |(_, rest)| rest);
    let authority = authority.split(['/', '?', '#']).next().unwrap_or_default();
    (url.starts_with("http://") || url.starts_with("https://"))
        && !authority.contains('@')
        && site(url) == Site::Local
}

pub(crate) fn site(url: &str) -> Site {
    let url = address(url);
    let Some((scheme, rest)) = url.split_once(':') else {
        return Site::Local;
    };
    match scheme {
        "about" | "data" | "blob" => return Site::Local,
        "http" | "https" => {}
        other => return Site::Other(other.to_owned()),
    }
    let rest = rest.trim_start_matches('/');
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = if host.starts_with('[') {
        host.split(']')
            .next()
            .map(|h| format!("{h}]"))
            .unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or_default().to_owned()
    }
    .to_lowercase();
    let local = host == "localhost"
        || host.ends_with(".localhost")
        || host == "[::1]"
        || host == "0.0.0.0"
        || host.starts_with("127.");
    if local {
        Site::Local
    } else {
        Site::Web(host.strip_prefix("www.").unwrap_or(&host).to_owned())
    }
}

/// What the person typed in the address bar, as a URL: a scheme is kept, a local address
/// gets http, something with a dot gets https, anything else is searched for.
pub fn address(typed: &str) -> String {
    let typed = typed.trim();
    if typed.contains("://") || typed.starts_with("about:") || typed.starts_with("data:") {
        return typed.to_owned();
    }
    let host = typed.split(['/', '?', '#']).next().unwrap_or_default();
    let local = ["localhost", "127.0.0.1", "[::1]", "0.0.0.0"]
        .iter()
        .any(|l| host == *l || host.starts_with(&format!("{l}:")));
    if local {
        format!("http://{typed}")
    } else if !typed.contains(char::is_whitespace) && host.contains('.') {
        format!("https://{typed}")
    } else {
        let query: String = typed
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                b' ' => "+".into(),
                _ => format!("%{b:02X}"),
            })
            .collect();
        format!("https://duckduckgo.com/?q={query}")
    }
}

/// What Chromium reports, kept and passed on until it stops.
async fn listen(
    browser: Browser,
    run: Arc<Run>,
    mut heard: tokio::sync::mpsc::UnboundedReceiver<Message>,
    scale: f32,
) {
    while let Some(message) = heard.recv().await {
        on_event(&browser, &run, message, scale);
    }
    // Chromium stopped: every session's tabs went with it.
    let owners: Vec<Owner> = run
        .state
        .lock()
        .unwrap()
        .owners
        .drain()
        .map(|(o, _)| o)
        .collect();
    for owner in owners {
        browser.tell(
            owner,
            BrowserUpdate::Tabs {
                tabs: vec![],
                active: None,
            },
        );
        // Closed on purpose, there is nothing to tell beyond that.
        if !run.closing.load(std::sync::atomic::Ordering::SeqCst) {
            browser.tell(owner, BrowserUpdate::Failed("The browser stopped.".into()));
        }
    }
}

fn on_event(browser: &Browser, run: &Arc<Run>, message: Message, scale: f32) {
    let params = &message.params;
    let mut changed = None;
    match message.method.as_str() {
        "Target.targetInfoChanged" => {
            let info = &params["targetInfo"];
            let mut state = run.state.lock().unwrap();
            if let Some((owner, tab)) =
                state.by_target(info["targetId"].as_str().unwrap_or_default())
            {
                tab.url = info["url"].as_str().unwrap_or_default().to_owned();
                tab.title = info["title"].as_str().unwrap_or_default().to_owned();
                changed = Some(owner);
            }
        }
        // A page opened another: it becomes a tab of the same session.
        "Target.targetCreated" => {
            let info = &params["targetInfo"];
            let opener = info["openerId"].as_str().unwrap_or_default();
            let target = info["targetId"].as_str().unwrap_or_default().to_owned();
            let owner = run
                .state
                .lock()
                .unwrap()
                .by_context(info["browserContextId"].as_str().unwrap_or_default());
            if info["type"] == "page" && !opener.is_empty() {
                if let Some(owner) = owner {
                    let (browser, run) = (browser.clone(), run.clone());
                    tokio::spawn(async move {
                        if let Err(error) = browser.adopt(&run, owner, &target, true).await {
                            browser.tell(owner, BrowserUpdate::Notice(error));
                        }
                    });
                }
            }
        }
        "Target.targetDestroyed" | "Target.detachedFromTarget" => {
            let target = params["targetId"].as_str().unwrap_or_default();
            let mut state = run.state.lock().unwrap();
            let owner = state.owners.iter_mut().find_map(|(owner, owned)| {
                let at = owned.tabs.iter().position(|t| t.target == target)?;
                let gone = owned.tabs.remove(at);
                if owned.active == Some(gone.id) {
                    owned.active = owned.tabs.last().map(|t| t.id);
                }
                Some(*owner)
            });
            changed = owner;
        }
        "Page.frameStartedLoading" | "Page.frameStoppedLoading" => {
            let loading = message.method.ends_with("StartedLoading");
            let frame = params["frameId"].as_str().unwrap_or_default();
            let mut state = run.state.lock().unwrap();
            // A page's main frame has the target's id.
            if let Some((owner, tab)) = state.by_target(frame) {
                tab.loading = loading;
                changed = Some(owner);
                // The title a page sets as it loads is not always reported as a change.
                if !loading {
                    let (browser, run, session) =
                        (browser.clone(), run.clone(), tab.session.clone());
                    tokio::spawn(async move { refresh(&browser, &run, &session).await });
                }
            }
        }
        // Back and forward are asked again after any navigation; a frame's own is cheap to
        // count along.
        "Page.frameNavigated" | "Page.navigatedWithinDocument" => {
            if let Some(session) = message.session.clone() {
                let (browser, run) = (browser.clone(), run.clone());
                tokio::spawn(async move { refresh(&browser, &run, &session).await });
            }
        }
        "Page.screencastFrame" => {
            let session = message.session.unwrap_or_default();
            let mut state = run.state.lock().unwrap();
            let shown = state.shown;
            let surface = state
                .by_session(&session)
                .filter(|(owner, tab)| shown == Some((*owner, tab.id)))
                .map(|(owner, tab)| surface(owner, tab.id));
            let ack = params["sessionId"].clone();
            match surface {
                Some(surface) => {
                    let _ = run.decode.send(Shot {
                        surface,
                        data: params["data"].as_str().unwrap_or_default().to_owned(),
                        ack,
                        session,
                        scale,
                    });
                }
                None => run.cdp.send(
                    "Page.screencastFrameAck",
                    json!({ "sessionId": ack }),
                    Some(&session),
                ),
            }
        }
        "Page.fileChooserOpened" => {
            let session = message.session.unwrap_or_default();
            let found = run
                .state
                .lock()
                .unwrap()
                .by_session(&session)
                .map(|(o, t)| (o, t.id));
            if let Some((owner, tab)) = found {
                browser.tell(
                    owner,
                    BrowserUpdate::ChooseFiles {
                        tab,
                        node: params["backendNodeId"].as_i64().unwrap_or_default(),
                        multiple: params["mode"] == "selectMultiple",
                    },
                );
            }
        }
        "Overlay.inspectNodeRequested" => {
            let (browser, run) = (browser.clone(), run.clone());
            let session = message.session.unwrap_or_default();
            let node = params["backendNodeId"].as_i64().unwrap_or_default();
            tokio::spawn(async move { picked(&browser, &run, &session, node).await });
        }
        // The person sees what a page says; nothing waits on an answer (Desktop suppresses
        // dialogs for the agent).
        "Page.javascriptDialogOpening" => {
            let session = message.session.unwrap_or_default();
            let kind = params["type"].as_str().unwrap_or_default();
            run.cdp.send(
                "Page.handleJavaScriptDialog",
                json!({ "accept": kind == "alert" || kind == "beforeunload" }),
                Some(&session),
            );
            let owner = run
                .state
                .lock()
                .unwrap()
                .by_session(&session)
                .map(|(o, _)| o);
            let text = params["message"].as_str().unwrap_or_default();
            if let (Some(owner), false) = (owner, text.is_empty()) {
                browser.tell(
                    owner,
                    BrowserUpdate::Notice(format!("The page said: {text}")),
                );
            }
        }
        "Runtime.consoleAPICalled" | "Runtime.exceptionThrown" => {
            let line = console_line(&message.method, params);
            let mut state = run.state.lock().unwrap();
            if let Some((_, tab)) = state.by_session(message.session.as_deref().unwrap_or_default())
            {
                if tab.console.len() == KEPT {
                    tab.console.pop_front();
                }
                tab.console.push_back(line);
            }
        }
        "Network.requestWillBeSent" | "Network.responseReceived" | "Network.loadingFailed" => {
            let mut state = run.state.lock().unwrap();
            if let Some((_, tab)) = state.by_session(message.session.as_deref().unwrap_or_default())
            {
                note_request(tab, &message.method, params);
            }
        }
        // A download starts in a tab's frame, whose id is the tab's target.
        "Browser.downloadWillBegin" => {
            let mut state = run.state.lock().unwrap();
            let frame = params["frameId"].as_str().unwrap_or_default();
            if let Some((owner, _)) = state.by_target(frame) {
                let name = params["suggestedFilename"]
                    .as_str()
                    .unwrap_or("a file")
                    .to_owned();
                let guid = params["guid"].as_str().unwrap_or_default().to_owned();
                state.downloads.insert(guid, (owner, name));
            }
        }
        "Browser.downloadProgress" if params["state"] != "inProgress" => {
            let guid = params["guid"].as_str().unwrap_or_default();
            let done = run.state.lock().unwrap().downloads.remove(guid);
            if let Some((owner, name)) = done {
                let notice = if params["state"] == "completed" {
                    format!(
                        "Saved {name} in {}",
                        browser.inner.options.downloads.display()
                    )
                } else {
                    format!("Couldn't save {name}.")
                };
                browser.tell(owner, BrowserUpdate::Notice(notice));
            }
        }
        _ => {}
    }
    if let Some(owner) = changed {
        browser.publish(run, owner);
    }
}

fn stop_inspecting(run: &Run, session: &str) {
    run.cdp.send(
        "Overlay.setInspectMode",
        json!({ "mode": "none", "highlightConfig": {} }),
        Some(session),
    );
}

/// The element the person picked, for the message they write: what it is, its HTML and a
/// picture of it, as Desktop's pick carries; the picture is a file the agent
/// reads, as the message is text.
async fn picked(browser: &Browser, run: &Arc<Run>, session: &str, node: i64) {
    let owner = run
        .state
        .lock()
        .unwrap()
        .by_session(session)
        .map(|(o, t)| (o, t.url.clone()));
    let Some((owner, url)) = owner else {
        return;
    };
    stop_inspecting(run, session);
    browser.tell(owner, BrowserUpdate::Selecting(false));
    let Ok(object) = run
        .cdp
        .call(
            "DOM.resolveNode",
            json!({ "backendNodeId": node }),
            Some(session),
        )
        .await
    else {
        return;
    };
    let read = "function() { const r = this.getBoundingClientRect(); return {         x: r.left + scrollX, y: r.top + scrollY, w: r.width, h: r.height,         tag: this.tagName.toLowerCase(), text: (this.innerText || '').trim().slice(0, 200),         html: this.outerHTML } }";
    let Ok(info) = run
        .cdp
        .call(
            "Runtime.callFunctionOn",
            json!({ "objectId": object["object"]["objectId"], "functionDeclaration": read,
                    "returnByValue": true }),
            Some(session),
        )
        .await
    else {
        return;
    };
    let info = &info["result"]["value"];
    let html: String = info["html"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(3000)
        .collect();
    let mut block = format!(
        "<selected_element page=\"{url}\">\n{} {}\n```html\n{html}\n```\n",
        info["tag"].as_str().unwrap_or("element"),
        info["text"]
            .as_str()
            .map(|t| format!("\"{t}\""))
            .unwrap_or_default(),
    );
    let (w, h) = (
        info["w"].as_f64().unwrap_or(0.0),
        info["h"].as_f64().unwrap_or(0.0),
    );
    if w >= 1.0 && h >= 1.0 {
        let shot = run
            .cdp
            .call(
                "Page.captureScreenshot",
                json!({ "format": "png", "clip": { "x": info["x"], "y": info["y"],
                        "width": w, "height": h, "scale": 1 } }),
                Some(session),
            )
            .await;
        if let Some(path) = shot.ok().and_then(|shot| keep_pick(shot["data"].as_str()?)) {
            block.push_str(&format!("A picture of it: @{}\n", path.display()));
        }
    }
    block.push_str("</selected_element>\n");
    browser.tell(owner, BrowserUpdate::Selected(block));
}

/// A picked element's picture, kept where the agent can read it.
fn keep_pick(png: &str) -> Option<PathBuf> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(png).ok()?;
    let dir = std::env::temp_dir().join(format!("quill-picks-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let path = dir.join(format!("element-{stamp}.png"));
    std::fs::write(&path, bytes).ok()?;
    Some(path)
}

/// A tab's address, title and history asked again, for the person's toolbar.
async fn refresh(browser: &Browser, run: &Arc<Run>, session: &str) {
    let Ok(history) = run
        .cdp
        .call("Page.getNavigationHistory", json!({}), Some(session))
        .await
    else {
        return;
    };
    let at = history["currentIndex"].as_i64().unwrap_or(0);
    let count = history["entries"].as_array().map_or(0, Vec::len) as i64;
    let current = &history["entries"][at.max(0) as usize];
    let owner = {
        let mut state = run.state.lock().unwrap();
        state.by_session(session).map(|(owner, tab)| {
            tab.back = at > 0;
            tab.forward = at + 1 < count;
            if let Some(url) = current["url"].as_str() {
                tab.url = url.to_owned();
            }
            if let Some(title) = current["title"].as_str() {
                tab.title = title.to_owned();
            }
            owner
        })
    };
    if let Some(owner) = owner {
        browser.publish(run, owner);
    }
}

/// A console line as the agent reads it: "[error] message".
fn console_line(method: &str, params: &Value) -> String {
    if method == "Runtime.exceptionThrown" {
        let details = &params["exceptionDetails"];
        let text = details["exception"]["description"]
            .as_str()
            .or(details["text"].as_str())
            .unwrap_or("uncaught exception");
        return format!("[exception] {text}");
    }
    let words: Vec<String> = params["args"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|arg| match &arg["value"] {
            Value::String(text) => text.clone(),
            Value::Null => arg["description"]
                .as_str()
                .unwrap_or(arg["type"].as_str().unwrap_or(""))
                .to_owned(),
            value => value.to_string(),
        })
        .collect();
    format!(
        "[{}] {}",
        params["type"].as_str().unwrap_or("log"),
        words.join(" ")
    )
}

fn note_request(tab: &mut Tab, method: &str, params: &Value) {
    let id = params["requestId"].as_str().unwrap_or_default();
    match method {
        "Network.requestWillBeSent" => {
            if tab.network.len() == KEPT {
                tab.network.pop_front();
            }
            tab.network.push_back(Request {
                id: id.to_owned(),
                method: params["request"]["method"]
                    .as_str()
                    .unwrap_or("GET")
                    .to_owned(),
                url: params["request"]["url"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                status: None,
            });
        }
        _ => {
            let status = match method {
                "Network.responseReceived" => params["response"]["status"].to_string(),
                _ => format!(
                    "failed: {}",
                    params["errorText"].as_str().unwrap_or("error")
                ),
            };
            if let Some(request) = tab.network.iter_mut().rev().find(|r| r.id == id) {
                request.status = Some(status);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_addresses_become_urls() {
        assert_eq!(
            address("localhost:3000/checkout"),
            "http://localhost:3000/checkout"
        );
        assert_eq!(address("127.0.0.1:8080"), "http://127.0.0.1:8080");
        assert_eq!(address("example.com/a?b"), "https://example.com/a?b");
        assert_eq!(address("https://acme.test"), "https://acme.test");
        assert_eq!(address("about:blank"), "about:blank");
        assert_eq!(
            address("rust borrow checker"),
            "https://duckduckgo.com/?q=rust+borrow+checker"
        );
    }

    #[test]
    fn only_local_web_pages_come_through_browser() {
        assert!(is_local("http://localhost:5173/"));
        assert!(is_local("http://127.0.0.1:8000/a?b"));
        assert!(!is_local("https://github.com/login/device"));
        assert!(!is_local("http://user:pw@localhost:3000/"));
        assert!(!is_local("file:///tmp/x.html"));
    }

    #[test]
    fn local_pages_never_ask_and_other_schemes_are_refused() {
        assert_eq!(site("localhost:3000/a"), Site::Local);
        assert_eq!(site("http://127.0.0.1:8080"), Site::Local);
        assert_eq!(site("http://app.localhost/"), Site::Local);
        assert_eq!(site("about:blank"), Site::Local);
        assert_eq!(
            site("https://www.Example.com/x"),
            Site::Web("example.com".into())
        );
        assert_eq!(
            site("https://user:pw@docs.rs/serde"),
            Site::Web("docs.rs".into())
        );
        assert_eq!(site("file:///etc/passwd"), Site::Other("file".into()));
    }
}
