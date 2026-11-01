//! quill's own updates, off the interface's thread (docs/UPDATES.md): a check a little after
//! quill starts and every six hours, one on demand, a download once a newer version is found
//! when Settings allow it (a mandatory version always), and an installation only when the
//! person asks for it.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use quill_model::{Event, UpdateState};
use quill_update::install::Install;
use quill_update::{Error, Release, Source, Version};

use crate::Sink;

/// The first check waits this long, so the window is up before any request.
const FIRST_CHECK: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// Progress is told every this many bytes, and at the end.
const STEP: u64 = 256 * 1024;

/// quill's update state, shared by the router and the tasks that move it along.
#[derive(Clone)]
pub struct Updates {
    shared: Arc<Mutex<Shared>>,
    ui: Sink,
    /// Whether quill looks for its own updates at all (`Config::updates`).
    enabled: bool,
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
    pub fn start(ui: Sink, enabled: bool, auto: bool) -> Self {
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
                self.set(UpdateState::Ready { version, mandatory });
            }
            Err(error) => self.set(failure(error)),
        }
    }
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
        let updates = Updates::start(sink, true, true);
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
