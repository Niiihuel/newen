//! quill's own updates, off the interface's thread (docs/UPDATES.md). Checks keep to the wall
//! clock: about every four hours (give or take 15 %), one at the start only when the last is
//! over an hour old, and one when the window comes back or the computer wakes and the last is
//! that old. A newer version found downloads when Settings allow (a mandatory one always) and
//! is staged: unpacked in its place, not the one that starts. The next start activates it
//! (main.rs), or a restart the person asks for does it now. A newer release found meanwhile
//! replaces it. Each step is kept in the update log.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use newen_model::{Event, ReleaseNotes, UpdateState, WhatsNew};
use newen_update::files;
use newen_update::install::{Install, Staged};
use newen_update::whats_new::{self as wire, Kept, Kind};
use newen_update::{Error, Manifest, Release, Source, Trust, VerifyingKey, Version};

use crate::Sink;

/// The first check waits this long, so the window is up before any request.
const FIRST_CHECK: Duration = Duration::from_secs(20);
/// Checks are this far apart, in seconds, give or take [`JITTER`] of it.
const EVERY: u64 = 4 * 60 * 60;
const JITTER: f64 = 0.15;
/// A start, a window coming back or a wake checks when the last check is this old.
const STALE: u64 = 60 * 60;
/// How often the schedule is looked at. It is kept on the wall clock, which a sleep moves.
const TICK: Duration = Duration::from_secs(60);
/// The wall clock moved this much more than the process's own clock between two looks: the
/// computer slept.
const WAKE: u64 = 5 * 60;
/// A version ready this long without a restart is announced, once.
const DAY: u64 = 24 * 60 * 60;
/// Progress is told every this many bytes, and at the end.
const STEP: u64 = 256 * 1024;
/// The store's marks: the version whose announcement showed, and the one whose notes were
/// opened.
const SEEN: &str = "whats_new_seen";
const OPENED: &str = "whats_new_opened";
/// The store's marks of updates: when the last check ended (seconds since 1970), the version
/// the person skipped, and the last one the system announced.
const LAST_CHECK: &str = "update_last_check";
const SKIPPED: &str = "update_skipped";
const NOTIFIED: &str = "update_notified";
/// The highest sequence a verified manifest had: a lower one is an old release served again.
const SEQUENCE: &str = "update_sequence";
/// The keys a rotation left, which replace the ones compiled in.
const KEYS: &str = "update_keys";
/// How many installed versions' notes are kept.
const KEPT_NOTES: usize = 10;

/// Shows a system notification (title, body); a click on it brings quill up.
pub type Notifier = Arc<dyn Fn(String, String) + Send + Sync>;

/// quill's update state, shared by the router and the tasks that move it along.
#[derive(Clone)]
pub struct Updates {
    shared: Arc<Mutex<Shared>>,
    ui: Sink,
    /// Whether quill looks for its own updates at all (`Config::updates`).
    enabled: bool,
    /// The record, where what the person saw of the notes is marked.
    store: Arc<Mutex<newen_store::Store>>,
    /// Where each release's notes and media are kept, a folder per version; `None` keeps none.
    notes: Option<PathBuf>,
    /// The installation this copy runs from; `None` for one quill does not update.
    install: Option<Install>,
    /// The update log (`Config::update_log`); `None` keeps none.
    log: Option<PathBuf>,
    notify: Notifier,
    /// Where releases are read and the key they are checked with; `None` is this copy's own
    /// (the tests give a loopback feed).
    feed: Option<(Source, VerifyingKey)>,
}

struct Shared {
    state: UpdateState,
    /// Settings' `auto_update`.
    auto: bool,
    /// The newest release found, and where it is read from.
    found: Option<Release>,
    /// Counts the releases found: a download of an older one stops when it moves on.
    generation: u64,
    /// A check is on its way.
    checking: bool,
    /// The window has the focus.
    focused: bool,
    /// When the last check ended, seconds since 1970.
    last_check: Option<u64>,
    /// When the schedule checks next; `None` until the first check's wait is over.
    next_check: Option<u64>,
    /// When the ready version was staged.
    ready_since: Option<u64>,
}

impl Shared {
    fn new(state: UpdateState, auto: bool) -> Self {
        Self {
            state,
            auto,
            found: None,
            generation: 0,
            checking: false,
            focused: true,
            last_check: None,
            next_check: None,
            ready_since: None,
        }
    }
}

impl Updates {
    /// Starts the checks, unless this copy is not one quill updates: a build from source or a
    /// binary `quill --install` never placed reports that and checks nothing. Off, nothing is
    /// reported or checked.
    pub fn start(
        ui: Sink,
        enabled: bool,
        auto: bool,
        store: Arc<Mutex<newen_store::Store>>,
        notes: Option<PathBuf>,
        log: Option<PathBuf>,
        notify: Notifier,
    ) -> Self {
        let unmanaged = if enabled { unmanaged() } else { None };
        let install = if enabled && unmanaged.is_none() {
            Install::locate()
        } else {
            None
        };
        let last_check = (store.lock().unwrap().marked(LAST_CHECK).ok().flatten())
            .and_then(|at| at.parse().ok());
        let mut shared = Shared::new(unmanaged.clone().unwrap_or(UpdateState::Idle), auto);
        shared.last_check = last_check;
        let updates = Self {
            shared: Arc::new(Mutex::new(shared)),
            ui,
            enabled,
            store,
            notes,
            install,
            log,
            notify,
            feed: None,
        };
        if let Some(state) = unmanaged {
            (updates.ui)(Event::Update(state));
        }
        if updates.install.is_some() {
            if let Some(at) = last_check {
                (updates.ui)(Event::UpdateChecked { at, error: None });
            }
            updates.schedule();
        }
        updates
    }

    /// The checks of the schedule, on the wall clock: one at the start when the last is an
    /// hour old (else when its interval ends), then every interval, and one after a sleep.
    fn schedule(&self) {
        let every = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(FIRST_CHECK).await;
            {
                let mut shared = every.shared.lock().unwrap();
                shared.next_check = Some(first_check(now(), shared.last_check, random()));
            }
            let mut before = (now(), Instant::now());
            loop {
                let (wall, ran) = (now(), Instant::now());
                let woke = slept(wall.saturating_sub(before.0), ran - before.1);
                before = (wall, ran);
                every.tick(wall, woke);
                tokio::time::sleep(TICK).await;
            }
        });
    }

    /// One look at the schedule at `now`; `woke` when the computer slept since the last.
    fn tick(&self, now: u64, woke: bool) {
        let (last, next) = {
            let shared = self.shared.lock().unwrap();
            (shared.last_check, shared.next_check)
        };
        if next.is_some_and(|next| due(now, last, next, woke)) {
            if woke {
                self.log("schedule: the computer woke");
            }
            self.check();
        }
        self.announce(now, false);
    }

    /// The window gained or lost the focus: coming back, a check long due is made.
    pub fn focused(&self, focused: bool) {
        let check = {
            let mut shared = self.shared.lock().unwrap();
            shared.focused = focused;
            focused && shared.next_check.is_some() && stale(now(), shared.last_check)
        };
        if check {
            self.check();
        }
    }

    /// Settings' `auto_update` as it is now: when on, a found version downloads at once.
    pub fn auto(&self, on: bool) {
        self.shared.lock().unwrap().auto = on;
        if on && matches!(self.state(), UpdateState::Available { .. }) {
            self.download();
        }
    }

    /// Looks for a newer version now, unless a check is on its way. What is downloading or
    /// ready stays in view meanwhile; the check's end is told apart (`Event::UpdateChecked`).
    pub fn check(&self) {
        if !self.enabled {
            return;
        }
        let mut shared = self.shared.lock().unwrap();
        if let state @ (UpdateState::FromSource | UpdateState::NotInstalled) = &shared.state {
            let state = state.clone();
            drop(shared);
            return self.set(state);
        }
        if shared.checking {
            return;
        }
        shared.checking = true;
        if !busy(&shared.state) {
            (self.ui)(Event::Update(UpdateState::Checking));
            shared.state = UpdateState::Checking;
        }
        drop(shared);
        let updates = self.clone();
        tokio::task::spawn_blocking(move || updates.run_check());
    }

    /// Downloads the newer version found, if it is not here yet.
    pub fn download(&self) {
        let updates = self.clone();
        tokio::task::spawn_blocking(move || updates.run_download());
    }

    /// The person skips the optional version offered: it is not offered again, a newer one is.
    pub fn skip(&self) {
        let updates = self.clone();
        tokio::task::spawn_blocking(move || updates.run_skip());
    }

    /// Activates the staged version and says where it runs from; the host starts it and
    /// closes this one.
    pub fn install(&self) {
        let version = match self.state() {
            UpdateState::Ready { version, .. } => Some(version),
            // A restart that failed may be tried again.
            UpdateState::Failed { .. } => None,
            _ => return,
        };
        let updates = self.clone();
        tokio::task::spawn_blocking(move || updates.run_install(version));
    }

    fn run_install(&self, version: Option<String>) {
        let Some(install) = self.install.clone() else {
            return self.set(failure(Error::NotInstalled));
        };
        let staged = install.staged();
        let Some(version) = (staged.as_ref())
            .map(|staged| staged.version.to_string())
            .or(version)
        else {
            return;
        };
        let activated = match &staged {
            Some(staged) => install.activate_staged(staged, &running()),
            // Another start of quill activated it already: restart into it.
            None => Ok(install.program()),
        };
        match activated {
            Ok(program) => {
                self.log(format!("activate: {version} starts from now on (restart)"));
                self.set(UpdateState::Installed { version, program })
            }
            Err(error) => {
                self.log(format!("activate: {version} failed: {error}"));
                self.set(failure(Error::Disk))
            }
        }
    }

    fn run_skip(&self) {
        let (version, at) = {
            let shared = self.shared.lock().unwrap();
            match &shared.state {
                UpdateState::Available {
                    version,
                    mandatory: false,
                    ..
                } => (version.clone(), shared.last_check.unwrap_or_else(now)),
                _ => return,
            }
        };
        // Kept before the interface hears it.
        if let Err(error) = self.store.lock().unwrap().mark(SKIPPED, &version) {
            return self.log(format!("skip: {version} not kept: {error}"));
        }
        self.log(format!("skip: {version}, until a newer one"));
        self.set(UpdateState::UpToDate { checked_at: at });
    }

    fn state(&self) -> UpdateState {
        self.shared.lock().unwrap().state.clone()
    }

    fn generation(&self) -> u64 {
        self.shared.lock().unwrap().generation
    }

    /// Sets the state and tells the interface, in that order for every caller.
    fn set(&self, state: UpdateState) {
        let mut shared = self.shared.lock().unwrap();
        (self.ui)(Event::Update(state.clone()));
        shared.state = state;
    }

    /// [`Self::set`], while `generation` is still the release found; else nothing.
    fn set_current(&self, generation: u64, state: UpdateState) -> bool {
        let mut shared = self.shared.lock().unwrap();
        if shared.generation != generation {
            return false;
        }
        (self.ui)(Event::Update(state.clone()));
        shared.state = state;
        true
    }

    /// [`Self::set`], unless a version is downloading or ready: a check does not hide it.
    fn set_unless_busy(&self, state: UpdateState) {
        let mut shared = self.shared.lock().unwrap();
        if !busy(&shared.state) {
            (self.ui)(Event::Update(state.clone()));
            shared.state = state;
        }
    }

    fn marked(&self, key: &str) -> Option<String> {
        self.store.lock().unwrap().marked(key).ok().flatten()
    }

    /// A line in the update log (and on stderr).
    fn log(&self, line: impl AsRef<str>) {
        eprintln!("quill: update: {}", line.as_ref());
        if let Some(path) = &self.log {
            newen_update::log::write(path, line.as_ref());
        }
    }

    /// The latest release's manifest. The highest sequence and the keys a rotation names are kept before it returns,
    /// except for a test feed of `QUILL_UPDATE_FEED`, which neither reads nor changes them.
    fn fetch(&self) -> Result<Manifest, Error> {
        let test_feed =
            std::env::var("QUILL_UPDATE_FEED").is_ok_and(|base| !base.trim().is_empty());
        let sequence = (self.marked(SEQUENCE))
            .filter(|_| !test_feed)
            .and_then(|kept| kept.parse().ok());
        let trust = Trust {
            keys: match &self.feed {
                Some((_, key)) => vec![*key],
                None => {
                    let stored = self
                        .marked(KEYS)
                        .and_then(|kept| newen_update::read_keys(&kept));
                    newen_update::trusted_keys(&stored.unwrap_or_default())
                }
            },
            sequence: sequence.unwrap_or(0),
        };
        let agent = newen_update::agent();
        let source = (self.feed.as_ref()).map_or_else(Source::locate, |(source, _)| source.clone());
        let checked = newen_update::check(&agent, &source, &trust);
        if let Some(manifest) = checked.as_ref().ok().filter(|_| !test_feed) {
            let store = self.store.lock().unwrap();
            if sequence.is_none_or(|kept| manifest.sequence > kept) {
                if let Err(error) = store.mark(SEQUENCE, &manifest.sequence.to_string()) {
                    eprintln!("quill: update: {error}");
                }
            }
            if let Some(keys) = &manifest.rotated {
                let kept = store.mark(KEYS, &newen_update::write_keys(keys));
                drop(store);
                match kept {
                    Ok(()) => {
                        self.log(format!("keys: rotated, {} trusted from now on", keys.len()))
                    }
                    Err(error) => self.log(format!("keys: rotation not kept: {error}")),
                }
            }
        }
        checked
    }

    fn run_check(&self) {
        let checked = self.fetch();
        let at = now();
        // Kept before the interface hears of it.
        if let Err(error) = (self.store.lock().unwrap()).mark(LAST_CHECK, &at.to_string()) {
            eprintln!("quill: update: {error}");
        }
        {
            let mut shared = self.shared.lock().unwrap();
            shared.checking = false;
            shared.last_check = Some(at);
            shared.next_check = Some(at + jittered(EVERY, random()));
        }
        (self.ui)(Event::UpdateChecked {
            at,
            error: checked.as_ref().err().map(|error| error.label()),
        });
        match checked {
            Ok(manifest) => self.found(manifest, at),
            Err(error) => {
                self.log(format!("check failed: {error}"));
                self.set_unless_busy(failure(error));
            }
        }
    }

    /// What a check found: a newer release replaces what was found before (its download
    /// stops), unless the person skipped it or it was rolled back here.
    fn found(&self, manifest: Manifest, at: u64) {
        let running = running();
        let latest = manifest.version.clone();
        let Some(release) = newen_update::offer(manifest, &running, &newen_update::platform())
        else {
            self.log(format!("check: up to date ({running}, latest {latest})"));
            return self.set_unless_busy(UpdateState::UpToDate { checked_at: at });
        };
        let version = release.version.to_string();
        let mandatory = release.mandatory;
        let install = self.install.as_ref();
        if install
            .and_then(Install::bad)
            .is_some_and(|bad| release.version <= bad)
        {
            self.log(format!(
                "check: {version} offered, refused: it was rolled back"
            ));
            return self.set_unless_busy(UpdateState::Failed {
                reason: format!(
                    "Version {version} did not start on this computer and was rolled back."
                ),
            });
        }
        if !mandatory && self.marked(SKIPPED).as_deref() == Some(version.as_str()) {
            self.log(format!("check: {version} offered, skipped"));
            return self.set_unless_busy(UpdateState::UpToDate { checked_at: at });
        }
        let what = if mandatory { " (mandatory)" } else { "" };
        self.log(format!("check: {version} offered{what} ({running} runs)"));
        let (generation, fetch) = {
            let mut shared = self.shared.lock().unwrap();
            let same =
                (shared.found.as_ref()).is_some_and(|found| found.version == release.version);
            if same && busy(&shared.state) {
                // On its way already, or here.
                return;
            }
            shared.generation += 1;
            shared.found = Some(release.clone());
            // A mandatory version is fetched whatever Settings say: it must be installed.
            (shared.generation, mandatory || shared.auto)
        };
        // Staged by an earlier run that never started it.
        if let Some(staged) = install
            .and_then(Install::staged)
            .filter(|staged| staged.version == release.version && staged.path.exists())
        {
            return self.ready(generation, &release, staged.since);
        }
        self.set_current(
            generation,
            UpdateState::Available {
                version,
                notes: release.notes.clone(),
                mandatory,
                size: (release.artifact.as_ref()).map_or(0, |artifact| artifact.size),
            },
        );
        if fetch {
            self.run_download();
        }
    }

    fn run_download(&self) {
        let (release, generation) = {
            let shared = self.shared.lock().unwrap();
            let Some(release) = shared.found.clone() else {
                return;
            };
            let version = release.version.to_string();
            if matches!(
                &shared.state,
                UpdateState::Downloading { version: on, .. } | UpdateState::Ready { version: on, .. }
                    if *on == version
            ) {
                return;
            }
            (release, shared.generation)
        };
        let version = release.version.to_string();
        let mandatory = release.mandatory;
        if release.artifact.is_none() && release.files.is_none() {
            self.log(format!("download: {version} has nothing for this system"));
            self.set_current(generation, failure(Error::NoArtifact));
            return;
        }
        let Some(install) = self.install.clone() else {
            self.set_current(generation, failure(Error::NotInstalled));
            return;
        };
        let downloading = |done, total| UpdateState::Downloading {
            version: version.clone(),
            done,
            total,
            mandatory,
        };
        // Told every STEP bytes and at the end, while this release is the one found.
        let report = |told: &mut u64, done: u64, total: u64| {
            if done == total || done.saturating_sub(*told) >= STEP {
                *told = done;
                return self.set_current(generation, downloading(done, total));
            }
            self.generation() == generation
        };
        let size = (release.artifact.as_ref()).map_or(0, |artifact| artifact.size);
        if !self.set_current(generation, downloading(0, size)) {
            return;
        }
        // File by file first: only what changed comes; the full archive when that fails.
        if let Some(index) = release.files.clone() {
            let (started, mut told, mut downloaded) = (Instant::now(), 0, 0);
            let agent = newen_update::agent();
            let staged =
                files::fetch_bundle(&agent, &index, &release.version, &newen_update::platform())
                    .and_then(|bundle| {
                        self.log(format!(
                            "download: {version} file by file, {} files",
                            bundle.files.len()
                        ));
                        let blobs = install.downloads().join("blobs");
                        files::stage(&agent, &install, &bundle, &blobs, |done, total| {
                            downloaded = done;
                            report(&mut told, done, total)
                        })
                    });
            if self.generation() != generation {
                return self.log(format!(
                    "download: {version} stopped: a newer release replaces it"
                ));
            }
            match staged {
                Ok(path) => {
                    self.log(format!(
                        "download: {version} done file by file, {downloaded} bytes in {:.1} s; every digest matches",
                        started.elapsed().as_secs_f32()
                    ));
                    return self.keep_ready(generation, &release, &install, path, index.sha256);
                }
                Err(error) => self.log(format!(
                    "download: {version} file by file failed: {error}; the full archive instead"
                )),
            }
        }
        let Some(artifact) = release.artifact.clone() else {
            self.log(format!("download: {version} has nothing for this system"));
            self.set_current(generation, failure(Error::NoArtifact));
            return;
        };
        if !self.set_current(generation, downloading(0, artifact.size)) {
            return;
        }
        self.log(format!(
            "download: {version} started, {} bytes",
            artifact.size
        ));
        let started = Instant::now();
        let mut told = 0;
        let downloaded = newen_update::download(
            &newen_update::agent(),
            &artifact,
            &install.downloads(),
            |done, total| report(&mut told, done, total),
        );
        if self.generation() != generation {
            return self.log(format!(
                "download: {version} stopped: a newer release replaces it"
            ));
        }
        let file = match downloaded {
            Ok(file) => file,
            Err(error) => {
                self.log(format!("download: {version} failed: {error}"));
                self.set_current(generation, failure(error));
                return;
            }
        };
        self.log(format!(
            "download: {version} done, {} bytes in {:.1} s; size and SHA-256 match",
            artifact.size,
            started.elapsed().as_secs_f32()
        ));
        match install.stage(&file, &release.version) {
            Ok(path) => {
                // A whole bundle with CEF: it is not kept once staged.
                let _ = std::fs::remove_file(&file);
                self.keep_ready(generation, &release, &install, path, artifact.sha256);
            }
            Err(error) => {
                self.log(format!("stage: {version} failed: {error}"));
                self.set_current(generation, failure(Error::Disk));
            }
        }
    }

    /// `release`, staged at `path` from what has `sha256`, is kept for the next start to
    /// activate, before the interface hears it is ready.
    fn keep_ready(
        &self,
        generation: u64,
        release: &Release,
        install: &Install,
        path: PathBuf,
        sha256: String,
    ) {
        let version = release.version.to_string();
        if self.generation() != generation {
            return;
        }
        let staged = Staged {
            version: release.version.clone(),
            sha256,
            path,
            since: now(),
        };
        if let Err(error) = install.keep_staged(&staged) {
            self.log(format!("stage: {version} failed: {error}"));
            self.set_current(generation, failure(Error::Disk));
            return;
        }
        self.log(format!("stage: {version} at {}", staged.path.display()));
        // Before the strip shows: its announcement is there when it is clicked.
        self.keep_whats_new(release);
        self.ready(generation, release, staged.since);
    }

    /// `release`, staged at `since`, is ready: the interface hears it, and the system too when
    /// the window is away.
    fn ready(&self, generation: u64, release: &Release, since: u64) {
        let ready = UpdateState::Ready {
            version: release.version.to_string(),
            mandatory: release.mandatory,
        };
        if self.set_current(generation, ready) {
            self.shared.lock().unwrap().ready_since = Some(since);
            self.announce(now(), true);
        }
    }

    /// Announces the ready version by the system, once for each version: as it becomes ready
    /// (`arriving`) while the window is away, or once it has waited a day.
    fn announce(&self, now: u64, arriving: bool) {
        let (version, since, focused) = {
            let shared = self.shared.lock().unwrap();
            match (&shared.state, shared.ready_since) {
                (UpdateState::Ready { version, .. }, Some(since)) => {
                    (version.clone(), since, shared.focused)
                }
                _ => return,
            }
        };
        let notified = self.marked(NOTIFIED);
        if !announces(arriving, focused, since, now, notified.as_deref(), &version) {
            return;
        }
        // Kept before it shows: never twice for a version, whatever happens next.
        if let Err(error) = self.store.lock().unwrap().mark(NOTIFIED, &version) {
            return self.log(format!("notify: {version} not kept: {error}"));
        }
        let why = if arriving {
            "the window is away"
        } else {
            "ready for a day"
        };
        self.log(format!("notify: {version} is ready ({why})"));
        (self.notify)(
            "quill".into(),
            format!("quill {version} is ready. Restart quill to update."),
        );
    }
}

/// A version is downloading, ready or being installed: a check leaves it in view.
fn busy(state: &UpdateState) -> bool {
    matches!(
        state,
        UpdateState::Downloading { .. } | UpdateState::Ready { .. } | UpdateState::Installed { .. }
    )
}

fn running() -> Version {
    Version::parse(newen_update::VERSION).expect("the workspace's version")
}

/// `base` seconds, moved by up to [`JITTER`] of it either way as `unit` (0 to 1) says.
fn jittered(base: u64, unit: f64) -> u64 {
    (base as f64 * (1.0 - JITTER + 2.0 * JITTER * unit.clamp(0.0, 1.0))) as u64
}

/// A number from 0 to 1, different each time: std's random hasher keys, no crate for it.
fn random() -> f64 {
    use std::hash::BuildHasher as _;
    let bits = std::collections::hash_map::RandomState::new().hash_one(0u8);
    bits as f64 / u64::MAX as f64
}

/// The last check ended an hour ago or more, or there was none.
fn stale(now: u64, last: Option<u64>) -> bool {
    last.is_none_or(|last| now.saturating_sub(last) >= STALE)
}

/// When a start checks: now when the last check is stale, else when its interval ends.
fn first_check(now: u64, last: Option<u64>, unit: f64) -> u64 {
    match last {
        Some(last) if !stale(now, Some(last)) => last + jittered(EVERY, unit),
        _ => now,
    }
}

/// The wall clock moved `wall` seconds while the process's own clock moved `ran`: the computer
/// slept between.
fn slept(wall: u64, ran: Duration) -> bool {
    wall > ran.as_secs() + WAKE
}

/// A check is due at `now`: its time came, or the computer woke and the last one is stale.
fn due(now: u64, last: Option<u64>, next: u64, woke: bool) -> bool {
    now >= next || (woke && stale(now, last))
}

/// Whether the system announces `version`, ready since `since`, at `now`: never twice
/// (`notified`), and only as it arrives with the window away, or after a day.
fn announces(
    arriving: bool,
    focused: bool,
    since: u64,
    now: u64,
    notified: Option<&str>,
    version: &str,
) -> bool {
    notified != Some(version) && ((arriving && !focused) || now >= since + DAY)
}

impl Updates {
    /// Tells the interface the notes kept, at a start: when this version came by an update
    /// and `announce` (Settings' `whats_new`) allows, its announcement shows, once.
    pub fn whats_new(&self, announce: bool) {
        let updates = self.clone();
        tokio::task::spawn_blocking(move || updates.tell_whats_new(announce));
    }

    /// The person opened the notes: this version's button goes once that is kept.
    pub fn opened(&self) {
        let updates = self.clone();
        tokio::task::spawn_blocking(move || {
            let kept = (updates.store.lock().unwrap()).mark(OPENED, newen_update::VERSION);
            if let Err(error) = kept {
                eprintln!("quill: what's new: {error}");
            }
            updates.tell_whats_new(false);
        });
    }

    /// Downloads `release`'s notes and media into its folder; a failure leaves the update as
    /// it is, without them.
    fn keep_whats_new(&self, release: &Release) {
        let Some(folder) = (self.notes.as_ref()).filter(|_| release.whats_new.is_some()) else {
            return;
        };
        let folder = folder.join(release.version.to_string());
        let agent = newen_update::agent();
        let kept = wire::fetch_whats_new(&agent, release, &folder).and_then(|(notes, media)| {
            let json = serde_json::to_vec_pretty(&release_notes(notes, media))
                .map_err(|_| Error::Manifest)?;
            std::fs::write(folder.join(NOTES), json).map_err(|error| {
                eprintln!("quill: what's new: {error}");
                Error::Disk
            })
        });
        match kept {
            Ok(()) => self.tell_whats_new(false),
            Err(error) => eprintln!("quill: what's new: not kept: {error}"),
        }
    }

    /// Reads the notes kept and tells the interface; with `announce`, this version's
    /// announcement is marked as seen first, and shows only once that is kept.
    fn tell_whats_new(&self, announce: bool) {
        let Some(folder) = &self.notes else {
            return;
        };
        let running = Version::parse(newen_update::VERSION).expect("the workspace's version");
        let mut kept = read_notes(folder);
        kept.sort_by(|a, b| b.0.cmp(&a.0));
        let ready = (kept.iter())
            .find(|(version, _)| *version > running)
            .map(|(_, notes)| notes.clone());
        let mut releases: Vec<(Version, ReleaseNotes)> = (kept.into_iter())
            .filter(|(version, _)| *version <= running)
            .collect();
        // The oldest go, folder and all.
        for (version, _) in releases.drain(KEPT_NOTES.min(releases.len())..) {
            let _ = std::fs::remove_dir_all(folder.join(version.to_string()));
        }
        let current = (releases.first()).filter(|(version, _)| *version == running);
        let store = self.store.lock().unwrap();
        let marked = |key| store.marked(key).ok().flatten();
        let unopened =
            current.is_some() && marked(OPENED).as_deref() != Some(newen_update::VERSION);
        let announce = announce
            && current.is_some_and(|(_, notes)| notes.announcement.is_some())
            && marked(SEEN).as_deref() != Some(newen_update::VERSION)
            && (store.mark(SEEN, newen_update::VERSION))
                .inspect_err(|error| eprintln!("quill: what's new: {error}"))
                .is_ok();
        drop(store);
        (self.ui)(Event::WhatsNew(WhatsNew {
            releases: releases.into_iter().map(|(_, notes)| notes).collect(),
            ready,
            announce,
            unopened,
        }));
    }
}

/// A release's notes as its folder keeps them, in the interface's terms.
const NOTES: &str = "notes.json";

/// Every release's notes kept under `folder`, each with its version; one unreadable is skipped.
fn read_notes(folder: &std::path::Path) -> Vec<(Version, ReleaseNotes)> {
    let entries = std::fs::read_dir(folder).into_iter().flatten().flatten();
    entries
        .filter_map(|entry| {
            let version = Version::parse(entry.file_name().to_str()?).ok()?;
            let json = std::fs::read(entry.path().join(NOTES)).ok()?;
            Some((version, serde_json::from_slice(&json).ok()?))
        })
        .collect()
}

/// The longest a title, a body and a line of the notes may be; longer ones are cut, so the
/// announcement and the dialog keep their shape.
const TITLE: usize = 60;
const BODY: usize = 280;
const LINE: usize = 160;
/// The most points an announcement lists.
const BULLETS: usize = 5;

/// `notes` as the interface shows them, with the medium that matched.
fn release_notes(notes: wire::WhatsNew, media: Option<Kept>) -> ReleaseNotes {
    let lines = |lines: Vec<String>| lines.iter().map(|line| cut(line, LINE)).collect();
    ReleaseNotes {
        version: notes.version.to_string(),
        date: notes.date,
        new: lines(notes.notes.new),
        improved: lines(notes.notes.improved),
        fixed: lines(notes.notes.fixed),
        announcement: notes.announcement.map(|a| newen_model::Announcement {
            title: cut(&a.title, TITLE),
            body: cut(&a.body, BODY),
            bullets: (a.bullets.iter())
                .take(BULLETS)
                .map(|b| cut(b, LINE))
                .collect(),
            media: media.map(|kept| match (kept.kind, kept.poster) {
                (Kind::Video, Some(poster)) => newen_model::Media::Video {
                    poster,
                    clip: kept.file,
                },
                _ => newen_model::Media::Image { path: kept.file },
            }),
            try_it: a.try_it,
        }),
    }
}

/// `text` within `most` characters, ending in an ellipsis when it was longer; the log says so.
fn cut(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    eprintln!("quill: what's new: cut to {most} characters: {text}");
    let mut cut: String = text.chars().take(most - 1).collect();
    cut.push('\u{2026}');
    cut
}

/// Why this copy is not one quill updates: built from source (under a `target` folder, or with
/// `QUILL_NO_UPDATE` set), or never placed by `quill --install`.
fn unmanaged() -> Option<UpdateState> {
    if std::env::var_os("QUILL_NO_UPDATE").is_some_and(|value| !value.is_empty()) {
        return Some(UpdateState::FromSource);
    }
    let program = std::env::current_exe().ok()?;
    if Install::locate().is_some_and(|install| install.runs(&program)) {
        return None;
    }
    if program
        .components()
        .any(|part| part.as_os_str() == "target")
    {
        return Some(UpdateState::FromSource);
    }
    Some(UpdateState::NotInstalled)
}

/// The state an error leaves.
fn failure(error: Error) -> UpdateState {
    UpdateState::Failed {
        reason: error.label(),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test binary runs from `target/`, so it is a build from source: it reports that and
    /// checks nothing, whatever the environment says.
    #[tokio::test]
    async fn a_build_from_source_says_so_and_checks_nothing() {
        let told: Arc<Mutex<Vec<Event>>> = Arc::default();
        let sink: Sink = {
            let told = told.clone();
            Arc::new(move |event| told.lock().unwrap().push(event))
        };
        let updates = Updates::start(sink, true, true, store(), None, None, quiet());
        updates.check();
        assert_eq!(updates.state(), UpdateState::FromSource);
        assert_eq!(
            *told.lock().unwrap(),
            [
                Event::Update(UpdateState::FromSource),
                Event::Update(UpdateState::FromSource),
            ]
        );
    }

    fn store() -> Arc<Mutex<newen_store::Store>> {
        Arc::new(Mutex::new(newen_store::Store::in_memory().unwrap()))
    }

    fn quiet() -> Notifier {
        Arc::new(|_, _| {})
    }

    /// What a test's updates told: the interface's events, and the system's notices.
    #[derive(Clone, Default)]
    struct Told {
        events: Arc<Mutex<Vec<Event>>>,
        notices: Arc<Mutex<Vec<String>>>,
    }

    /// Updates managing `install`, reading the loopback `server`'s feed with `key`, its log in
    /// `log`. `probe` looks at each event as it is told.
    fn managed(
        server: &newen_update::serve::Server,
        key: VerifyingKey,
        install: Option<Install>,
        auto: bool,
        log: PathBuf,
        probe: impl Fn(&Event) + Send + Sync + 'static,
    ) -> (Updates, Told) {
        let told = Told::default();
        let ui: Sink = {
            let events = told.events.clone();
            Arc::new(move |event| {
                probe(&event);
                events.lock().unwrap().push(event)
            })
        };
        let store = store();
        let notify: Notifier = {
            let (notices, store) = (told.notices.clone(), store.clone());
            Arc::new(move |_, body| {
                // The version announced is kept before the notice shows.
                let kept = store.lock().unwrap().marked(NOTIFIED).unwrap();
                assert!(kept.is_some_and(|version| body.contains(&version)));
                notices.lock().unwrap().push(body)
            })
        };
        let updates = Updates {
            shared: Arc::new(Mutex::new(Shared::new(UpdateState::Idle, auto))),
            ui,
            enabled: true,
            store,
            notes: None,
            install,
            log: Some(log),
            notify,
            feed: Some((Source::at(server.base()), key)),
        };
        (updates, told)
    }

    /// `running`'s next patch, `by` patches on.
    fn next(by: u64) -> Version {
        let running = running();
        Version::new(running.major, running.minor, running.patch + by)
    }

    /// Publish `version` on `server`, with `artifact` (bytes) for this system when given.
    fn release(
        server: &newen_update::serve::Server,
        version: &Version,
        mandatory: bool,
        artifact: Option<&[u8]>,
    ) -> VerifyingKey {
        use sha2::Digest as _;
        let name = format!("quill-{version}.tar.gz");
        let mut platforms = std::collections::BTreeMap::new();
        if let Some(bytes) = artifact {
            server.put(&format!("/{name}"), bytes.to_vec());
            platforms.insert(
                newen_update::platform(),
                newen_update::Artifact {
                    url: format!("{}/{name}", server.base()),
                    size: bytes.len() as u64,
                    sha256: newen_update::hex(&sha2::Sha256::digest(bytes)),
                },
            );
        }
        newen_update::serve::publish(
            server,
            &Manifest {
                version: version.clone(),
                date: "2026-11-02T12:00:00Z".into(),
                notes: String::new(),
                minimum_version: mandatory.then(|| version.clone()),
                platforms,
                ..Default::default()
            },
        )
    }

    /// A Linux bundle of `version` as a release's `.tar.gz`, built in `folder`.
    #[cfg(target_os = "linux")]
    fn bundle(folder: &std::path::Path, version: &Version) -> Vec<u8> {
        use std::os::unix::fs::PermissionsExt;
        let dir = folder.join(format!("bundle-{version}"));
        std::fs::create_dir_all(dir.join("locales")).unwrap();
        std::fs::write(
            dir.join("quill"),
            [b"\x7fELF".as_slice(), version.to_string().as_bytes()].concat(),
        )
        .unwrap();
        std::fs::set_permissions(dir.join("quill"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        std::fs::write(dir.join("libcef.so"), "cef").unwrap();
        let out = folder.join(format!("{version}.tar.gz"));
        let status = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&out)
            .arg("-C")
            .arg(&dir)
            .args(["quill", "libcef.so", "locales"])
            .status()
            .unwrap();
        assert!(status.success());
        std::fs::read(out).unwrap()
    }

    /// A release found downloads, is staged and kept before the interface hears it is ready;
    /// a newer one found while it waits replaces it; the next start activates the newest. The
    /// log tells each step, and no address.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_release_is_staged_before_it_is_ready_and_a_newer_one_found_replaces_it() {
        let folder = tempfile::tempdir().unwrap();
        let install = Install::at(folder.path().join("app"));
        let running = running();
        let built = folder.path().join("built");
        std::fs::create_dir_all(&built).unwrap();
        std::fs::write(built.join("quill"), b"\x7fELF running").unwrap();
        let program = install.adopt(&built.join("quill"), &running).unwrap();
        let server = newen_update::serve::serve(0).unwrap();
        let (one, two) = (next(1), next(2));
        let key = release(&server, &one, false, Some(&bundle(folder.path(), &one)));
        let staged_when_ready: Arc<Mutex<Vec<Option<Version>>>> = Arc::default();
        let probe = {
            let (seen, install) = (staged_when_ready.clone(), install.clone());
            move |event: &Event| {
                if let Event::Update(UpdateState::Ready { .. }) = event {
                    seen.lock()
                        .unwrap()
                        .push(install.staged().map(|staged| staged.version));
                }
            }
        };
        let log = folder.path().join("updates.log");
        let (updates, told) = managed(
            &server,
            key,
            Some(install.clone()),
            true,
            log.clone(),
            probe,
        );
        updates.run_check();
        let ready = |version: &Version| UpdateState::Ready {
            version: version.to_string(),
            mandatory: false,
        };
        assert_eq!(updates.state(), ready(&one));
        assert_eq!(*staged_when_ready.lock().unwrap(), [Some(one.clone())]);
        // Staged, not started: this version still starts.
        assert_eq!(install.current(), Some(running.to_string()));
        assert!(install
            .root()
            .join(format!("versions/{one}/quill"))
            .is_file());
        // Downloading showed on its way.
        assert!(told.events.lock().unwrap().iter().any(|event| matches!(
            event,
            Event::Update(UpdateState::Downloading { done, total, .. }) if done == total
        )));

        // Checked again while it waits: the same version is nothing new.
        let before = told.events.lock().unwrap().len();
        updates.run_check();
        let after: Vec<Event> = told.events.lock().unwrap()[before..].to_vec();
        assert!(
            matches!(&after[..], [Event::UpdateChecked { error: None, .. }]),
            "{after:?}"
        );

        // A newer one is published: it replaces the staged one.
        release(&server, &two, false, Some(&bundle(folder.path(), &two)));
        updates.run_check();
        assert_eq!(updates.state(), ready(&two));
        assert_eq!(
            install.staged().map(|staged| staged.version),
            Some(two.clone())
        );
        assert_eq!(
            *staged_when_ready.lock().unwrap(),
            [Some(one.clone()), Some(two.clone())]
        );
        // The next start activates it.
        assert_eq!(
            install.launch(&running).unwrap(),
            newen_update::install::Launch::Activated {
                version: two.clone(),
                program
            }
        );
        let text = std::fs::read_to_string(&log).unwrap();
        for step in [
            format!("check: {one} offered"),
            format!("download: {one} started"),
            format!("download: {one} done"),
            format!("stage: {one} at"),
            format!("check: {two} offered"),
            format!("stage: {two} at"),
        ] {
            assert!(text.contains(&step), "{step} in {text}");
        }
        assert!(!text.contains("http"), "{text}");
        assert!(
            told.notices.lock().unwrap().is_empty(),
            "the window has the focus"
        );
    }

    /// Publish `version` on `server` with its archive and its list of files, as a release does;
    /// with `corrupt`, the program's blob is not the program.
    #[cfg(target_os = "linux")]
    fn release_with_files(
        server: &newen_update::serve::Server,
        folder: &std::path::Path,
        version: &Version,
        corrupt: bool,
    ) -> VerifyingKey {
        use sha2::Digest as _;
        let archive = bundle(folder, version);
        let name = format!("quill-{version}.tar.gz");
        server.put(&format!("/{name}"), archive.clone());
        let tree = folder.join(format!("bundle-{version}"));
        let platform = newen_update::platform();
        let list = newen_update::serve::publish_files(server, &tree, version, &platform);
        if corrupt {
            let program = std::fs::read(tree.join("quill")).unwrap();
            let blob = files::blob_name(&newen_update::hex(&sha2::Sha256::digest(&program)));
            // The same size, every byte flipped: it no longer unpacks to the program.
            server.configure(|files| {
                let packed = files.files.get_mut(&format!("/{blob}")).unwrap();
                *packed = Arc::new(packed.iter().map(|byte| !byte).collect());
            });
        }
        let archive = newen_update::Artifact {
            url: format!("{}/{name}", server.base()),
            size: archive.len() as u64,
            sha256: newen_update::hex(&sha2::Sha256::digest(&archive)),
        };
        newen_update::serve::publish(
            server,
            &Manifest {
                version: version.clone(),
                date: "2026-11-02T12:00:00Z".into(),
                platforms: [(platform.clone(), archive)].into(),
                files: [(platform, list)].into(),
                sequence: version.patch,
                ..Default::default()
            },
        )
    }

    /// A release with a list of files is staged file by file: only the program, which changed,
    /// is downloaded; CEF's library comes from the version that runs. A release whose blob is
    /// bad falls back on its full archive and is staged all the same.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_release_is_staged_file_by_file_and_falls_back_on_its_archive() {
        let folder = tempfile::tempdir().unwrap();
        let install = Install::at(folder.path().join("app"));
        let running = running();
        bundle(folder.path(), &running);
        let built = folder.path().join(format!("bundle-{running}"));
        install.adopt(&built.join("quill"), &running).unwrap();
        let server = newen_update::serve::serve(0).unwrap();
        let (one, two) = (next(1), next(2));
        let key = release_with_files(&server, folder.path(), &one, false);
        let log = folder.path().join("updates.log");
        let (updates, _told) = managed(
            &server,
            key,
            Some(install.clone()),
            true,
            log.clone(),
            |_| {},
        );
        updates.run_check();
        let ready = |version: &Version| UpdateState::Ready {
            version: version.to_string(),
            mandatory: false,
        };
        assert_eq!(updates.state(), ready(&one));
        let staged = install.root().join(format!("versions/{one}"));
        assert_eq!(
            std::fs::read(staged.join("quill")).unwrap(),
            std::fs::read(folder.path().join(format!("bundle-{one}/quill"))).unwrap()
        );
        assert_eq!(std::fs::read(staged.join("libcef.so")).unwrap(), b"cef");
        let fetched: Vec<String> = (server.requests().into_iter())
            .map(|(path, _)| path)
            .filter(|path| path.starts_with("/f-") || path.ends_with(".tar.gz"))
            .collect();
        assert_eq!(fetched.len(), 1, "the program's blob only: {fetched:?}");
        // The sequence seen is kept.
        assert_eq!(updates.marked(SEQUENCE), Some(one.patch.to_string()));
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(
            text.contains(&format!("download: {one} done file by file")),
            "{text}"
        );
        assert!(
            !text.contains(&format!("download: {one} started")),
            "{text}"
        );

        release_with_files(&server, folder.path(), &two, true);
        updates.run_check();
        assert_eq!(updates.state(), ready(&two));
        assert_eq!(
            install.staged().map(|staged| staged.version),
            Some(two.clone())
        );
        let text = std::fs::read_to_string(&log).unwrap();
        for step in [
            format!("download: {two} file by file failed"),
            format!("download: {two} done,"),
            format!("stage: {two} at"),
        ] {
            assert!(text.contains(&step), "{step} in {text}");
        }
        // A newer version with a lower sequence: an old manifest served again, refused.
        release(&server, &next(3), false, None);
        updates.run_check();
        assert_eq!(updates.state(), ready(&two));
        assert_eq!(updates.marked(SEQUENCE), Some(two.patch.to_string()));
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(
            text.contains(&format!("check failed: {}", Error::Replayed)),
            "{text}"
        );
    }

    /// A version the person skipped is not offered again until a newer one comes; the skip is
    /// kept before the interface hears it. A mandatory version cannot be skipped.
    #[test]
    fn a_skipped_version_is_offered_again_only_when_a_newer_one_comes() {
        let folder = tempfile::tempdir().unwrap();
        let server = newen_update::serve::serve(0).unwrap();
        let (one, two) = (next(1), next(2));
        let key = release(&server, &one, false, None);
        let log = folder.path().join("updates.log");
        let (updates, _told) = managed(&server, key, None, false, log, |_| {});
        updates.run_check();
        let available = |version: &Version| {
            matches!(
                updates.state(),
                UpdateState::Available { version: found, mandatory: false, .. } if found == version.to_string()
            )
        };
        assert!(available(&one));
        updates.run_skip();
        assert_eq!(updates.marked(SKIPPED), Some(one.to_string()));
        assert!(matches!(updates.state(), UpdateState::UpToDate { .. }));
        updates.run_check();
        assert!(matches!(updates.state(), UpdateState::UpToDate { .. }));
        release(&server, &two, false, None);
        updates.run_check();
        assert!(available(&two));
        // Mandatory: nothing is skipped.
        updates.set(UpdateState::Available {
            version: two.to_string(),
            notes: String::new(),
            mandatory: true,
            size: 0,
        });
        updates.run_skip();
        assert_eq!(updates.marked(SKIPPED), Some(one.to_string()));
    }

    /// The system announces a ready version once: as it arrives while the window is away, or
    /// after it waited a day; the version is kept before the notice shows (`managed`).
    #[test]
    fn a_ready_version_is_announced_once_when_the_window_is_away_or_after_a_day() {
        assert!(announces(true, false, 0, 0, None, "1.0.0"));
        assert!(!announces(true, true, 0, 0, None, "1.0.0"));
        assert!(!announces(false, false, 0, DAY - 1, None, "1.0.0"));
        assert!(announces(false, true, 0, DAY, None, "1.0.0"));
        assert!(!announces(true, false, 0, DAY, Some("1.0.0"), "1.0.0"));
        assert!(announces(true, false, 0, 0, Some("0.9.0"), "1.0.0"));

        let folder = tempfile::tempdir().unwrap();
        let server = newen_update::serve::serve(0).unwrap();
        let key = release(&server, &next(1), false, None);
        let (updates, told) = managed(&server, key, None, false, folder.path().join("log"), |_| {});
        let make_ready = |version: &Version, since| {
            updates.set(UpdateState::Ready {
                version: version.to_string(),
                mandatory: false,
            });
            updates.shared.lock().unwrap().ready_since = Some(since);
        };
        // Away as it arrives: one notice, and never another for it.
        updates.focused(false);
        make_ready(&next(1), 100);
        updates.announce(100, true);
        updates.announce(100, true);
        updates.announce(100 + DAY, false);
        assert_eq!(told.notices.lock().unwrap().len(), 1);
        // In view as it arrives: none, until it waited a day.
        updates.focused(true);
        make_ready(&next(2), 1_000);
        updates.announce(1_000, true);
        updates.announce(1_000 + DAY - 1, false);
        assert_eq!(told.notices.lock().unwrap().len(), 1);
        updates.announce(1_000 + DAY, false);
        updates.announce(1_000 + 2 * DAY, false);
        let notices = told.notices.lock().unwrap().clone();
        assert_eq!(notices.len(), 2);
        assert!(notices[1].contains(&next(2).to_string()));
    }

    /// Checks keep to the wall clock: four hours apart give or take 15 %, one at the start only
    /// when the last is an hour old, and one after a sleep or when the window comes back if
    /// the last is that old.
    #[test]
    fn checks_are_jittered_and_caught_up_after_a_sleep_by_the_wall_clock() {
        assert_eq!(jittered(EVERY, 0.0), (EVERY as f64 * 0.85) as u64);
        assert_eq!(jittered(EVERY, 1.0), (EVERY as f64 * 1.15) as u64);
        assert_eq!(jittered(EVERY, 0.5), EVERY);
        for _ in 0..50 {
            let at = jittered(EVERY, random());
            assert!((EVERY * 85 / 100..=EVERY * 115 / 100).contains(&at), "{at}");
        }
        let now = 1_000_000;
        // At the start: now when stale or never checked; else the interval's remainder.
        assert_eq!(first_check(now, None, 0.5), now);
        assert_eq!(first_check(now, Some(now - STALE), 0.5), now);
        assert_eq!(first_check(now, Some(now - 600), 0.5), now - 600 + EVERY);
        // The wall clock ran hours while the process's ran a minute: it slept.
        assert!(slept(3 * 3600, Duration::from_secs(60)));
        assert!(!slept(61, Duration::from_secs(60)));
        let next = now + 3600;
        assert!(!due(now, Some(now - 600), next, false));
        assert!(due(next, Some(now - 600), next, false));
        // Woken: when the last check is stale only.
        assert!(!due(now, Some(now - 600), next, true));
        assert!(due(now, Some(now - STALE - 1), next, true));
    }

    /// The window coming back checks when the last check is stale, once the start's wait is
    /// over; a check's end is kept before it is told, and the log never holds an address.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_focus_after_a_stale_check_checks_and_the_log_holds_no_address() {
        let folder = tempfile::tempdir().unwrap();
        let server = newen_update::serve::serve(0).unwrap();
        let key = release(&server, &next(1), false, None);
        let kept_when_told: Arc<Mutex<Vec<bool>>> = Arc::default();
        let log = folder.path().join("updates.log");
        let (mut updates, told) = managed(&server, key, None, false, log.clone(), |_| {});
        // A feed with no release at all.
        updates.feed = Some((Source::at(format!("{}/nothing", server.base())), key));
        let probe = {
            let (seen, store, events) = (
                kept_when_told.clone(),
                updates.store.clone(),
                told.events.clone(),
            );
            Arc::new(move |event: Event| {
                if let Event::UpdateChecked { at, .. } = &event {
                    let kept = store.lock().unwrap().marked(LAST_CHECK).unwrap();
                    seen.lock().unwrap().push(kept == Some(at.to_string()));
                }
                events.lock().unwrap().push(event)
            })
        };
        updates.ui = probe;
        // Before the start's wait is over, the focus checks nothing.
        updates.focused(true);
        assert_eq!(updates.state(), UpdateState::Idle);
        updates.shared.lock().unwrap().next_check = Some(u64::MAX);
        updates.focused(false);
        assert_eq!(updates.state(), UpdateState::Idle);
        updates.focused(true);
        for _ in 0..200 {
            if !kept_when_told.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(*kept_when_told.lock().unwrap(), [true]);
        for _ in 0..200 {
            if updates.state() == failure(Error::Status(404)) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(updates.state(), failure(Error::Status(404)));
        // Just checked: coming back again checks nothing.
        let checks = told.events.lock().unwrap().len();
        updates.focused(false);
        updates.focused(true);
        assert_eq!(told.events.lock().unwrap().len(), checks);
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains("check failed"), "{text}");
        assert!(!text.contains("http"), "{text}");
    }

    /// This version came by an update whose notes have an announcement: it is announced at
    /// the first start only, marked before it is told; the button stays until the notes are
    /// opened. A newer version downloaded is the ready one, and older notes come after.
    #[tokio::test]
    async fn this_versions_announcement_is_told_once_and_its_button_until_the_notes_open() {
        let told: Arc<Mutex<Vec<Event>>> = Arc::default();
        let sink: Sink = {
            let told = told.clone();
            Arc::new(move |event| told.lock().unwrap().push(event))
        };
        let folder = tempfile::tempdir().unwrap();
        let running = Version::parse(newen_update::VERSION).unwrap();
        let newer = Version::new(running.major, running.minor, running.patch + 1).to_string();
        for (version, title) in [
            (newen_update::VERSION, Some("A browser beside the chat")),
            ("0.0.1", None),
            (newer.as_str(), Some("Later")),
        ] {
            let notes = wire::WhatsNew {
                version: Version::parse(version).unwrap(),
                date: "2026-11-10".into(),
                notes: wire::Notes {
                    new: vec!["x".repeat(400)],
                    ..Default::default()
                },
                announcement: title.map(|title| wire::Announcement {
                    title: title.into(),
                    body: String::new(),
                    bullets: vec![],
                    media: vec![],
                    try_it: None,
                }),
            };
            let at = folder.path().join(version);
            std::fs::create_dir_all(&at).unwrap();
            let json = serde_json::to_vec(&release_notes(notes, None)).unwrap();
            std::fs::write(at.join(NOTES), json).unwrap();
        }
        let store = store();
        let updates = Updates::start(
            sink,
            false,
            false,
            store.clone(),
            Some(folder.path().into()),
            None,
            quiet(),
        );
        let last = || match told.lock().unwrap().last().cloned() {
            Some(Event::WhatsNew(whats_new)) => whats_new,
            other => panic!("{other:?}"),
        };
        updates.tell_whats_new(true);
        let first = last();
        assert!(first.announce && first.unopened);
        let versions: Vec<_> = first.releases.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, [newen_update::VERSION, "0.0.1"]);
        assert_eq!(first.ready.unwrap().version, newer);
        // A line past its length is cut.
        assert_eq!(first.releases[0].new[0].chars().count(), LINE);
        assert_eq!(
            store.lock().unwrap().marked(SEEN).unwrap().as_deref(),
            Some(newen_update::VERSION)
        );
        // The next start: not again.
        updates.tell_whats_new(true);
        assert!(!last().announce && last().unopened);
        updates.opened();
        for _ in 0..100 {
            if !last().unopened {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!last().unopened && !last().announce);
    }

    /// A fresh copy has no notes of its own version: nothing is announced, no button.
    #[tokio::test]
    async fn a_fresh_copy_announces_nothing() {
        let told: Arc<Mutex<Vec<Event>>> = Arc::default();
        let sink: Sink = {
            let told = told.clone();
            Arc::new(move |event| told.lock().unwrap().push(event))
        };
        let folder = tempfile::tempdir().unwrap();
        let updates = Updates::start(
            sink,
            false,
            false,
            store(),
            Some(folder.path().into()),
            None,
            quiet(),
        );
        updates.tell_whats_new(true);
        assert_eq!(
            *told.lock().unwrap(),
            [Event::WhatsNew(WhatsNew::default())]
        );
    }
}
