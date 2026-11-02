//! quill's own updates, off the interface's thread (docs/UPDATES.md): a check a little after
//! quill starts and every six hours, one on demand, a download once a newer version is found
//! when Settings allow it (a mandatory version always), and an installation only when the
//! person asks for it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use quill_model::{Event, ReleaseNotes, UpdateState, WhatsNew};
use quill_update::install::Install;
use quill_update::whats_new::{self as wire, Kept, Kind};
use quill_update::{Error, Release, Source, Version};

use crate::Sink;

/// The first check waits this long, so the window is up before any request.
const FIRST_CHECK: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// Progress is told every this many bytes, and at the end.
const STEP: u64 = 256 * 1024;
/// The store's marks: the version whose announcement showed, and the one whose notes were
/// opened.
const SEEN: &str = "whats_new_seen";
const OPENED: &str = "whats_new_opened";
/// How many installed versions' notes are kept.
const KEPT_NOTES: usize = 10;

/// quill's update state, shared by the router and the tasks that move it along.
#[derive(Clone)]
pub struct Updates {
    shared: Arc<Mutex<Shared>>,
    ui: Sink,
    /// Whether quill looks for its own updates at all (`Config::updates`).
    enabled: bool,
    /// The record, where what the person saw of the notes is marked.
    store: Arc<Mutex<quill_store::Store>>,
    /// Where each release's notes and media are kept, a folder per version; `None` keeps none.
    notes: Option<PathBuf>,
}

struct Shared {
    state: UpdateState,
    /// Settings' `auto_update`.
    auto: bool,
    /// The newest release found, and where it is read from.
    found: Option<(Release, Source)>,
    /// Its download, once one is here.
    file: Option<PathBuf>,
}

impl Updates {
    /// Starts the checks, unless this copy is not one quill updates: a build from source or a
    /// binary `quill --install` never placed reports that and checks nothing. Off, nothing is
    /// reported or checked.
    pub fn start(
        ui: Sink,
        enabled: bool,
        auto: bool,
        store: Arc<Mutex<quill_store::Store>>,
        notes: Option<PathBuf>,
    ) -> Self {
        let unmanaged = if enabled { unmanaged() } else { None };
        let checks = enabled && unmanaged.is_none();
        let updates = Self {
            shared: Arc::new(Mutex::new(Shared {
                state: unmanaged.clone().unwrap_or(UpdateState::Idle),
                auto,
                found: None,
                file: None,
            })),
            ui,
            enabled,
            store,
            notes,
        };
        if let Some(state) = unmanaged {
            (updates.ui)(Event::Update(state));
        }
        if checks {
            let every = updates.clone();
            tokio::spawn(async move {
                tokio::time::sleep(FIRST_CHECK).await;
                loop {
                    every.check();
                    tokio::time::sleep(EVERY).await;
                }
            });
        }
        updates
    }

    /// Settings' `auto_update` as it is now: when on, a found version downloads at once.
    pub fn auto(&self, on: bool) {
        self.shared.lock().unwrap().auto = on;
        if on && matches!(self.state(), UpdateState::Available { .. }) {
            self.download();
        }
    }

    /// Looks for a newer version now, unless one is being looked for, fetched or installed.
    pub fn check(&self) {
        if !self.enabled {
            return;
        }
        match self.state() {
            UpdateState::Checking
            | UpdateState::Downloading { .. }
            | UpdateState::Ready { .. }
            | UpdateState::Installed { .. } => {}
            state @ (UpdateState::FromSource | UpdateState::NotInstalled) => self.set(state),
            _ => {
                self.set(UpdateState::Checking);
                let updates = self.clone();
                tokio::task::spawn_blocking(move || updates.run_check());
            }
        }
    }

    /// Downloads the newer version found, if it is not here yet.
    pub fn download(&self) {
        let updates = self.clone();
        tokio::task::spawn_blocking(move || updates.run_download());
    }

    /// Installs the downloaded version and says where it runs from; the host starts it and
    /// closes this one.
    pub fn install(&self) {
        let (version, file) = {
            let shared = self.shared.lock().unwrap();
            match (&shared.state, &shared.file, &shared.found) {
                (
                    UpdateState::Ready { .. } | UpdateState::Failed { .. },
                    Some(file),
                    Some((release, _)),
                ) => (release.version.to_string(), file.clone()),
                _ => return,
            }
        };
        let updates = self.clone();
        tokio::task::spawn_blocking(move || {
            let installed = Install::locate()
                .ok_or(Error::NotInstalled)
                .and_then(|install| {
                    let parsed = Version::parse(&version).map_err(|_| Error::Manifest)?;
                    install.apply(&file, &parsed).map_err(|error| {
                        eprintln!("quill: update: {error}");
                        Error::Disk
                    })
                });
            match installed {
                Ok(program) => {
                    // A whole bundle with CEF: it is not kept once installed.
                    let _ = std::fs::remove_file(&file);
                    updates.set(UpdateState::Installed { version, program })
                }
                Err(error) => updates.set(failure(error)),
            }
        });
    }

    fn state(&self) -> UpdateState {
        self.shared.lock().unwrap().state.clone()
    }

    /// Sets the state and tells the interface, in that order for every caller.
    fn set(&self, state: UpdateState) {
        let mut shared = self.shared.lock().unwrap();
        (self.ui)(Event::Update(state.clone()));
        shared.state = state;
    }

    fn run_check(&self) {
        let key = quill_update::public_key();
        let checked = match Source::locate() {
            None => Err(Error::Access),
            Some(source) => {
                let agent = quill_update::agent();
                quill_update::check(&agent, &source, key.as_ref())
                    .map(|manifest| (manifest, source))
            }
        };
        let (manifest, source) = match checked {
            Ok(checked) => checked,
            Err(error) => return self.set(failure(error)),
        };
        let installed = Version::parse(quill_update::VERSION).expect("the workspace's version");
        let Some(release) = quill_update::offer(manifest, &installed, &quill_update::platform())
        else {
            return self.set(UpdateState::UpToDate { checked_at: now() });
        };
        let available = UpdateState::Available {
            version: release.version.to_string(),
            notes: release.notes.clone(),
            mandatory: release.mandatory,
            size: release
                .artifact
                .as_ref()
                .map_or(0, |artifact| artifact.size),
        };
        // A mandatory version is fetched whatever Settings say: it must be installed.
        let fetch = release.mandatory || self.shared.lock().unwrap().auto;
        {
            let mut shared = self.shared.lock().unwrap();
            shared.file = None;
            shared.found = Some((release, source));
        }
        self.set(available);
        if fetch {
            self.run_download();
        }
    }

    fn run_download(&self) {
        let (release, source) = {
            let shared = self.shared.lock().unwrap();
            if matches!(
                shared.state,
                UpdateState::Downloading { .. } | UpdateState::Ready { .. }
            ) {
                return;
            }
            let Some(found) = shared.found.clone() else {
                return;
            };
            found
        };
        let version = release.version.to_string();
        let mandatory = release.mandatory;
        let Some(artifact) = release.artifact.clone() else {
            return self.set(failure(Error::NoArtifact));
        };
        let Some(install) = Install::locate() else {
            return self.set(failure(Error::NotInstalled));
        };
        self.set(UpdateState::Downloading {
            version: version.clone(),
            done: 0,
            total: artifact.size,
            mandatory,
        });
        let agent = quill_update::agent();
        let mut told = 0;
        let downloaded = quill_update::download(
            &agent,
            &source,
            &artifact,
            &install.downloads(),
            |done, total| {
                if done == total || done - told >= STEP {
                    told = done;
                    self.set(UpdateState::Downloading {
                        version: version.clone(),
                        done,
                        total,
                        mandatory,
                    });
                }
            },
        );
        match downloaded {
            Ok(path) => {
                self.shared.lock().unwrap().file = Some(path);
                // Before the strip shows: its announcement is there when it is clicked.
                self.keep_whats_new(&release, &source);
                self.set(UpdateState::Ready { version, mandatory });
            }
            Err(error) => self.set(failure(error)),
        }
    }
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
            let kept = (updates.store.lock().unwrap()).mark(OPENED, quill_update::VERSION);
            if let Err(error) = kept {
                eprintln!("quill: what's new: {error}");
            }
            updates.tell_whats_new(false);
        });
    }

    /// Downloads `release`'s notes and media into its folder; a failure leaves the update as
    /// it is, without them.
    fn keep_whats_new(&self, release: &Release, source: &Source) {
        let Some(folder) = (self.notes.as_ref()).filter(|_| release.whats_new.is_some()) else {
            return;
        };
        let folder = folder.join(release.version.to_string());
        let agent = quill_update::agent();
        let kept =
            wire::fetch_whats_new(&agent, source, release, &folder).and_then(|(notes, media)| {
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
        let running = Version::parse(quill_update::VERSION).expect("the workspace's version");
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
            current.is_some() && marked(OPENED).as_deref() != Some(quill_update::VERSION);
        let announce = announce
            && current.is_some_and(|(_, notes)| notes.announcement.is_some())
            && marked(SEEN).as_deref() != Some(quill_update::VERSION)
            && (store.mark(SEEN, quill_update::VERSION))
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
        announcement: notes.announcement.map(|a| quill_model::Announcement {
            title: cut(&a.title, TITLE),
            body: cut(&a.body, BODY),
            bullets: (a.bullets.iter())
                .take(BULLETS)
                .map(|b| cut(b, LINE))
                .collect(),
            media: media.map(|kept| match (kept.kind, kept.poster) {
                (Kind::Video, Some(poster)) => quill_model::Media::Video {
                    poster,
                    clip: kept.file,
                },
                _ => quill_model::Media::Image { path: kept.file },
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

/// The state an error leaves: without access to the private repository, that is said as such.
fn failure(error: Error) -> UpdateState {
    match error {
        Error::Access => UpdateState::NeedsAccess,
        other => UpdateState::Failed {
            reason: other.label(),
        },
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
        let updates = Updates::start(sink, true, true, store(), None);
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

    fn store() -> Arc<Mutex<quill_store::Store>> {
        Arc::new(Mutex::new(quill_store::Store::in_memory().unwrap()))
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
        let running = Version::parse(quill_update::VERSION).unwrap();
        let newer = Version::new(running.major, running.minor, running.patch + 1).to_string();
        for (version, title) in [
            (quill_update::VERSION, Some("A browser beside the chat")),
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
        );
        let last = || match told.lock().unwrap().last().cloned() {
            Some(Event::WhatsNew(whats_new)) => whats_new,
            other => panic!("{other:?}"),
        };
        updates.tell_whats_new(true);
        let first = last();
        assert!(first.announce && first.unopened);
        let versions: Vec<_> = first.releases.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, [quill_update::VERSION, "0.0.1"]);
        assert_eq!(first.ready.unwrap().version, newer);
        // A line past its length is cut.
        assert_eq!(first.releases[0].new[0].chars().count(), LINE);
        assert_eq!(
            store.lock().unwrap().marked(SEEN).unwrap().as_deref(),
            Some(quill_update::VERSION)
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
        let updates = Updates::start(sink, false, false, store(), Some(folder.path().into()));
        updates.tell_whats_new(true);
        assert_eq!(
            *told.lock().unwrap(),
            [Event::WhatsNew(WhatsNew::default())]
        );
    }

    #[test]
    fn a_refused_token_is_about_access_and_other_failures_say_why() {
        assert_eq!(failure(Error::Access), UpdateState::NeedsAccess);
        assert_eq!(
            failure(Error::Digest),
            UpdateState::Failed {
                reason: Error::Digest.label()
            }
        );
    }
}
