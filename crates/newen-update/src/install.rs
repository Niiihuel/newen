//! Installing a release for this person, without administrator rights. A release is a bundle:
//! the program and Chromium's (CEF's) files beside it.
//!
//! Linux: the installation is a folder, `$XDG_DATA_HOME/quill/app` (`~/.local/share/quill/app`),
//! with each version's bundle in `versions/{version}/` (`quill`, `libcef.so`, the `.pak` files,
//! `locales/`…) and `current`, a link to the version that starts. A release is unpacked beside
//! the others under a temporary name, renamed into place, and made current by renaming a new link
//! over `current`: a start finds the old version or the new one, never neither. The version
//! before stays, so the one still running is never deleted under it; older ones go.
//!
//! Windows: the installation is a folder, `%LOCALAPPDATA%\Programs\quill`, holding `quill.exe`,
//! and the rest of each version's bundle in `versions\{version}\`, where quill.exe finds CEF. A
//! running program can be renamed but not overwritten, so the new one is written beside it, the
//! running one is renamed aside, and the new one takes its name: that rename is the commit point,
//! after the new version's folder is in place. The Windows path is not exercised on a machine yet
//! (docs/UPDATES.md).
//!
//! macOS: the installation is the `quill.app` bundle itself, in the folder it runs from (else
//! `~/Applications`). A release is unpacked beside it, its signature checked, and the bundle
//! renamed aside for the new one. Not exercised on a machine yet either.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::Version;

/// The program's file name in a bundle.
#[cfg(not(windows))]
pub const PROGRAM: &str = "quill";
#[cfg(windows)]
pub const PROGRAM: &str = "quill.exe";
/// The program's file name in a bundle of Newen, quill's next name, which an update installs
/// as well (docs/UPDATES.md, «El paso a Newen»).
#[cfg(not(windows))]
pub const NEWEN_PROGRAM: &str = "newen";
#[cfg(windows)]
pub const NEWEN_PROGRAM: &str = "newen.exe";

/// CEF's library: a folder holding it beside the program is a bundle.
#[cfg(not(any(windows, target_os = "macos")))]
const CEF: &str = "libcef.so";
#[cfg(windows)]
const CEF: &str = "libcef.dll";

/// The bundle's name on macOS.
const BUNDLE: &str = "quill.app";
/// Newen's bundle on macOS: staged, it takes [`BUNDLE`]'s name.
#[cfg(target_os = "macos")]
const NEWEN_BUNDLE: &str = "Newen.app";

/// Linux: the installation in the data folder `data`, `app/` in Newen's own folder
/// ([`newen_files::own_dir`]): `newen/app`, or quill's `quill/app` while its data was not moved.
#[cfg(any(test, not(any(windows, target_os = "macos"))))]
pub(crate) fn app_root(data: &Path) -> PathBuf {
    newen_files::own_dir(data).join("app")
}

/// The program in `dir`, a bundle's folder: Newen's when it is there, else quill's.
pub(crate) fn program_in(dir: &Path) -> PathBuf {
    let newen = dir.join(NEWEN_PROGRAM);
    if newen.is_file() {
        newen
    } else {
        dir.join(PROGRAM)
    }
}

/// macOS: the bundle a release unpacked into `dir` holds, Newen's when it is there.
#[cfg(target_os = "macos")]
pub(crate) fn bundle_in(dir: &Path) -> PathBuf {
    let newen = dir.join(NEWEN_BUNDLE);
    if newen.is_dir() {
        newen
    } else {
        dir.join(BUNDLE)
    }
}

/// Where quill is installed for this person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    root: PathBuf,
}

impl Install {
    /// `QUILL_UPDATE_ROOT` when set (a test installation), or this person's:
    /// `$XDG_DATA_HOME/quill/app` on Linux ([`app_root`]), `%LOCALAPPDATA%\Programs\quill` on
    /// Windows, and on macOS the folder holding the `quill.app` this program runs from, else
    /// `~/Applications`.
    pub fn locate() -> Option<Self> {
        if let Some(root) = std::env::var_os("QUILL_UPDATE_ROOT").filter(|root| !root.is_empty()) {
            return Some(Self::at(root));
        }
        #[cfg(windows)]
        let root = PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("Programs")
            .join("quill");
        #[cfg(target_os = "macos")]
        let program = std::env::current_exe().ok();
        #[cfg(target_os = "macos")]
        let root = program
            .as_deref()
            .and_then(bundle_root)
            // Opened where it was unzipped, macOS runs a read-only copy: no installation.
            .filter(|dir| {
                !dir.components()
                    .any(|part| part.as_os_str() == "AppTranslocation")
            })
            .map(Path::to_path_buf)
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join("Applications")))?;
        #[cfg(not(any(windows, target_os = "macos")))]
        let root = app_root(
            &std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|base| base.is_absolute())
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
                })?,
        );
        Some(Self::at(root))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether `program`, this one as `current_exe` names it (links resolved), runs from this
    /// installation.
    pub fn runs(&self, program: &Path) -> bool {
        // Resolved as `program` is (and on Windows with its `\\?\` prefix).
        let root = fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        if cfg!(windows) {
            program.parent() == Some(root.as_path())
        } else if cfg!(target_os = "macos") {
            program.starts_with(root.join(BUNDLE))
        } else {
            program.starts_with(root.join("versions"))
        }
    }

    /// Where downloads wait until they are installed.
    pub fn downloads(&self) -> PathBuf {
        if cfg!(any(windows, target_os = "macos")) {
            std::env::temp_dir().join("quill-updates")
        } else {
            self.root.join("downloads")
        }
    }

    /// What a start runs: a path that stays the same across updates, for launchers. On Linux
    /// and macOS, Newen's program when the version that starts is Newen's; on Windows its
    /// program is placed under quill's name ([`Self::stage_with`]).
    pub fn program(&self) -> PathBuf {
        if cfg!(windows) {
            self.root.join(PROGRAM)
        } else if cfg!(target_os = "macos") {
            program_in(&self.root.join(BUNDLE).join("Contents/MacOS"))
        } else {
            program_in(&self.root.join("current"))
        }
    }

    /// Where this installation holds the file a release's bundle names `path` (`/` between
    /// folders), in the version that runs: Linux's `current/`; Windows' program beside the
    /// versions and the rest in `versions\{this version}\`; macOS' `quill.app/…` where it is.
    pub fn installed(&self, path: &str) -> PathBuf {
        let base = if cfg!(windows) {
            if path == PROGRAM {
                return self.root.join(PROGRAM);
            }
            self.root.join("versions").join(crate::VERSION)
        } else if cfg!(target_os = "macos") {
            self.root.clone()
        } else {
            self.root.join("current")
        };
        path.split('/').fold(base, |at, part| at.join(part))
    }

    /// The version `current` names, if any.
    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn current(&self) -> Option<String> {
        let target = fs::read_link(self.root.join("current")).ok()?;
        Some(target.file_name()?.to_str()?.to_owned())
    }

    /// Install `version` from the artifact at `artifact` (verified already) and make it current
    /// at once: [`Self::stage`] then [`Self::activate`]. Returns [`Self::program`].
    pub fn apply(&self, artifact: &Path, version: &Version) -> io::Result<PathBuf> {
        self.stage(artifact, version)?;
        self.activate(version)
    }

    /// Unpack `version` from the artifact at `artifact` (verified already) into its place, ready
    /// but not the one that starts: on Linux a gzipped tar of the bundle, on Windows a zip of
    /// it, on macOS a zip holding `quill.app`. Returns where it is staged ([`staged_path`]).
    pub fn stage(&self, artifact: &Path, version: &Version) -> io::Result<PathBuf> {
        #[cfg(not(any(windows, target_os = "macos")))]
        return self.stage_with(version, |staging| {
            run(Command::new("tar")
                .arg("-xzf")
                .arg(artifact)
                .arg("-C")
                .arg(staging))
        });
        #[cfg(windows)]
        return self.stage_with(version, |staging| {
            use std::os::windows::process::CommandExt;
            // Windows' own bsdtar reads zip; another `tar` on the PATH (Git's) may not.
            let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            run(
                Command::new(Path::new(&system).join("System32").join("tar.exe"))
                    .arg("-xf")
                    .arg(artifact)
                    .arg("-C")
                    .arg(staging)
                    // CREATE_NO_WINDOW: no console flashes.
                    .creation_flags(0x0800_0000),
            )
        });
        #[cfg(target_os = "macos")]
        return self.stage_with(version, |staging| {
            run(Command::new("/usr/bin/ditto")
                .args(["-x", "-k"])
                .arg(artifact)
                .arg(staging))?;
            // A bundle that is not intact and signed as released would not open.
            run(Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(bundle_in(staging)))
        });
    }

    /// Install `program`, this very file, as `version` (`quill --install`): with CEF's files
    /// beside it when its folder is a bundle (on macOS, the `quill.app` it is in).
    pub fn adopt(&self, program: &Path, version: &Version) -> io::Result<PathBuf> {
        if self.runs(program) {
            return Ok(self.program());
        }
        #[cfg(not(target_os = "macos"))]
        self.stage_with(version, |staging| {
            let folder = program.parent().unwrap_or(Path::new("."));
            // shortcut: a bundle's files are the folder's own and `locales/`, what a release
            // holds; a bundle with other folders would need them listed here.
            if folder.join(CEF).is_file() {
                copy_files(folder, staging, program)?;
                let locales = folder.join("locales");
                if locales.is_dir() {
                    copy_files(&locales, &staging.join("locales"), program)?;
                }
            }
            fs::copy(program, staging.join(PROGRAM)).map(drop)
        })?;
        #[cfg(target_os = "macos")]
        self.stage_with(version, |staging| {
            let from = bundle_root(program).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "quill --install runs from quill.app",
                )
            })?;
            run(Command::new("/usr/bin/ditto")
                .arg(from.join(BUNDLE))
                .arg(staging.join(BUNDLE)))
        })?;
        self.activate(version)
    }

    /// Stage `version` from what `fill` leaves in an empty temporary folder: the bundle as a
    /// release holds it (Linux and Windows: the program and CEF's files at its root; macOS:
    /// `quill.app/`), quill's or Newen's ([`program_in`]). It is checked to be a program for
    /// this system, synced to the disk and moved to [`staged_path`], or nothing is left.
    /// Nothing that starts changes: that is [`Self::activate`]'s. A per-file stager plugs in
    /// here as its own `fill`. On Linux, Newen's `newen` without a `quill` gets a `quill` link
    /// to it, so launchers that start `current/quill` start it.
    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn stage_with(
        &self,
        version: &Version,
        fill: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<PathBuf> {
        let name = version.to_string();
        let versions = self.root.join("versions");
        let target = versions.join(&name);
        if self.current().as_deref() == Some(name.as_str()) {
            return Ok(target);
        }
        fs::create_dir_all(&versions)?;
        let staging = versions.join(format!(".{name}.{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir(&staging)?;
        let staged = fill(&staging)
            // A release for another system would never start: refuse it before it replaces
            // one that does.
            .and_then(|()| native(&program_in(&staging), "the release is not a Linux program"))
            .and_then(|()| {
                let quill = staging.join(PROGRAM);
                // Past the check, a bundle without `quill` has Newen's `newen`.
                if fs::symlink_metadata(&quill).is_err() {
                    std::os::unix::fs::symlink(NEWEN_PROGRAM, quill)?;
                }
                Ok(())
            })
            .and_then(|()| sync_tree(&staging));
        let moved = staged.and_then(|()| {
            // A copy an earlier try left, never current.
            let _ = fs::remove_dir_all(&target);
            fs::rename(&staging, &target)?;
            fs::File::open(&versions)?.sync_all()
        });
        if let Err(error) = moved {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        Ok(target)
    }

    /// Make the staged `version` the one that starts, by renaming a new link over `current`: a
    /// start finds the old version or the new one, never neither. Every version but the new
    /// one and the one before goes. When the new one is Newen's, the one before, quill's, gets
    /// a `newen` link to its `quill`: launchers Newen writes start `current/newen`, and Newen's
    /// rollback returns to that folder. Returns [`Self::program`].
    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn activate(&self, version: &Version) -> io::Result<PathBuf> {
        let name = version.to_string();
        if self.current().as_deref() == Some(name.as_str()) {
            return Ok(self.program());
        }
        let versions = self.root.join("versions");
        if !program_in(&versions.join(&name)).is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{name} is not staged"),
            ));
        }
        let previous = self.current();
        if let Some(before) = previous.as_deref().map(|before| versions.join(before)) {
            if versions.join(&name).join(NEWEN_PROGRAM).is_file()
                && before.join(PROGRAM).is_file()
                && fs::symlink_metadata(before.join(NEWEN_PROGRAM)).is_err()
            {
                // Best effort: without it, only Newen's launchers miss after a rollback.
                let _ = std::os::unix::fs::symlink(PROGRAM, before.join(NEWEN_PROGRAM));
            }
        }
        self.switch(&name)?;
        self.prune(&name, previous.as_deref());
        Ok(self.program())
    }

    /// `current` names `versions/{name}` from now on, on the disk.
    #[cfg(not(any(windows, target_os = "macos")))]
    fn switch(&self, name: &str) -> io::Result<()> {
        let link = self.root.join("current.new");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(Path::new("versions").join(name), &link)?;
        fs::rename(&link, self.root.join("current"))?;
        // The rename reaches the disk before anything is deleted.
        if let Ok(folder) = fs::File::open(&self.root) {
            let _ = folder.sync_all();
        }
        Ok(())
    }

    /// Back to `trial.previous`, which the activation of `trial.version` kept.
    #[cfg(not(any(windows, target_os = "macos")))]
    fn roll_back(&self, trial: &Trial) -> io::Result<PathBuf> {
        let name = trial.previous.to_string();
        if !program_in(&self.root.join("versions").join(&name)).is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{name} is no longer here"),
            ));
        }
        self.switch(&name)?;
        Ok(self.program())
    }

    /// Delete every version but `current` and `previous`, and what earlier tries left. Whatever
    /// cannot be deleted stays.
    #[cfg(not(any(windows, target_os = "macos")))]
    fn prune(&self, current: &str, previous: Option<&str>) {
        let Ok(entries) = fs::read_dir(self.root.join("versions")) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name != current && Some(name.as_ref()) != previous {
                let _ = fs::remove_dir_all(entry.path());
            }
        }
    }

    /// Windows: the bundle `fill` leaves in `versions\.{version}.{pid}` gives up its program,
    /// written beside the running one as `quill.exe.{version}.new`; the rest becomes
    /// `versions\{version}`. Newen's `newen.exe` is placed so too, under the name launchers and
    /// the Run key start.
    #[cfg(windows)]
    pub fn stage_with(
        &self,
        version: &Version,
        fill: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<PathBuf> {
        let name = version.to_string();
        let versions = self.root.join("versions");
        fs::create_dir_all(&versions)?;
        let staging = versions.join(format!(".{name}.{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir(&staging)?;
        let new = staged_path("windows", &self.root, version);
        let target = versions.join(&name);
        let placed = fill(&staging).and_then(|()| {
            let _ = fs::remove_file(&new);
            fs::rename(program_in(&staging), &new)?;
            native(&new, "the release is not a Windows program")?;
            sync_tree(&staging)?;
            fs::OpenOptions::new().write(true).open(&new)?.sync_all()?;
            // A copy an earlier try left, or this version run from elsewhere: then in use, and
            // it stays.
            if target.exists() {
                discard(&target)?;
            }
            fs::rename(&staging, &target)
        });
        if let Err(error) = placed {
            let _ = fs::remove_dir_all(&staging);
            let _ = fs::remove_file(&new);
            return Err(error);
        }
        Ok(new)
    }

    /// Windows: the running program is renamed to `quill.exe.old` (kept until the next update
    /// replaces it) and the staged one takes its name: that rename is the commit point.
    /// Versions that are neither the new one nor this running one's are deleted, and the
    /// programs earlier stages and rollbacks left.
    #[cfg(windows)]
    pub fn activate(&self, version: &Version) -> io::Result<PathBuf> {
        let name = version.to_string();
        let new = staged_path("windows", &self.root, version);
        if !new.is_file() || !self.root.join("versions").join(&name).is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{name} is not staged"),
            ));
        }
        let program = self.program();
        let aside = self.root.join(format!("{PROGRAM}.old"));
        let _ = fs::remove_file(&aside);
        if program.exists() {
            fs::rename(&program, &aside)?;
        }
        if let Err(error) = fs::rename(&new, &program) {
            let _ = fs::rename(&aside, &program);
            return Err(error);
        }
        for entry in fs::read_dir(self.root.join("versions"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let found = entry.file_name();
            let found = found.to_string_lossy();
            if found != name && found != crate::VERSION {
                let _ = discard(&entry.path());
            }
        }
        self.sweep(&format!("{PROGRAM}."), &[&format!("{PROGRAM}.old")]);
        Ok(program)
    }

    /// Windows: the program on trial is renamed aside as `quill.exe.{version}.bad` (a running
    /// program can be renamed) and `quill.exe.old`, the one before, takes its name again.
    #[cfg(windows)]
    fn roll_back(&self, trial: &Trial) -> io::Result<PathBuf> {
        let program = self.program();
        let aside = self.root.join(format!("{PROGRAM}.old"));
        if !aside.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the version before is no longer here",
            ));
        }
        let bad = self.root.join(format!("{PROGRAM}.{}.bad", trial.version));
        let _ = fs::remove_file(&bad);
        fs::rename(&program, &bad)?;
        if let Err(error) = fs::rename(&aside, &program) {
            let _ = fs::rename(&bad, &program);
            return Err(error);
        }
        Ok(program)
    }

    /// macOS: the bundle `fill` leaves in `.quill.app.{version}.{pid}/` (`quill.app` or Newen's
    /// `Newen.app`) is checked and moved to `.quill.app.{version}`, beside `quill.app`.
    #[cfg(target_os = "macos")]
    pub fn stage_with(
        &self,
        version: &Version,
        fill: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.root)?;
        let staging = self
            .root
            .join(format!(".{BUNDLE}.{version}.{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir(&staging)?;
        let target = staged_path("macos", &self.root, version);
        let staged = fill(&staging)
            .and_then(|()| {
                native(
                    &program_in(&bundle_in(&staging).join("Contents/MacOS")),
                    "the release is not a macOS program",
                )
            })
            .and_then(|()| sync_tree(&staging))
            .and_then(|()| {
                let _ = fs::remove_dir_all(&target);
                fs::rename(bundle_in(&staging), &target)
            });
        let _ = fs::remove_dir_all(&staging);
        staged.map(|()| target)
    }

    /// macOS: `quill.app` is renamed to `.quill.app.old` (deleted by the next update) and the
    /// staged bundle takes its name.
    #[cfg(target_os = "macos")]
    pub fn activate(&self, version: &Version) -> io::Result<PathBuf> {
        let new = staged_path("macos", &self.root, version);
        if !new.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{version} is not staged"),
            ));
        }
        let bundle = self.root.join(BUNDLE);
        let aside = self.root.join(format!(".{BUNDLE}.old"));
        let _ = fs::remove_dir_all(&aside);
        if bundle.exists() {
            fs::rename(&bundle, &aside)?;
        }
        fs::rename(&new, &bundle).inspect_err(|_| {
            let _ = fs::rename(&aside, &bundle);
        })?;
        self.sweep(
            &format!(".{BUNDLE}."),
            &[&format!(".{BUNDLE}.old"), &format!(".{BUNDLE}.update")],
        );
        Ok(self.program())
    }

    /// macOS: the bundle on trial is renamed aside as `.quill.app.{version}.bad` and
    /// `.quill.app.old`, the one before, takes its name again.
    #[cfg(target_os = "macos")]
    fn roll_back(&self, trial: &Trial) -> io::Result<PathBuf> {
        let bundle = self.root.join(BUNDLE);
        let aside = self.root.join(format!(".{BUNDLE}.old"));
        if !aside.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the version before is no longer here",
            ));
        }
        let bad = self.root.join(format!(".{BUNDLE}.{}.bad", trial.version));
        let _ = fs::remove_dir_all(&bad);
        fs::rename(&bundle, &bad)?;
        fs::rename(&aside, &bundle).inspect_err(|_| {
            let _ = fs::rename(&bad, &bundle);
        })?;
        Ok(self.program())
    }

    /// Windows and macOS: delete what earlier stages and rollbacks left beside the program,
    /// every entry named `{prefix}…` but `keep`. One in use stays.
    #[cfg(any(windows, target_os = "macos"))]
    fn sweep(&self, prefix: &str, keep: &[&str]) {
        for entry in fs::read_dir(&self.root).into_iter().flatten().flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with(prefix) || keep.contains(&name.as_ref()) {
                continue;
            }
            let _ = if entry.path().is_dir() {
                fs::remove_dir_all(entry.path())
            } else {
                fs::remove_file(entry.path())
            };
        }
    }
}

/// Where [`Install::stage_with`] leaves `version` on `os` (`std::env::consts::OS`) under the
/// installation `root`: what [`Install::activate`] switches to.
pub fn staged_path(os: &str, root: &Path, version: &Version) -> PathBuf {
    match os {
        "windows" => root.join(format!("quill.exe.{version}.new")),
        "macos" => root.join(format!(".{BUNDLE}.{version}")),
        _ => root.join("versions").join(version.to_string()),
    }
}

/// A version staged: unpacked in its place and checked, not yet the one that starts. Kept in
/// the installation ([`Install::keep_staged`]) so the next start activates it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Staged {
    pub version: Version,
    /// The SHA-256 of the artifact, or of the list of files, it came from.
    pub sha256: String,
    /// [`staged_path`].
    pub path: PathBuf,
    /// When it was staged, in seconds since 1970.
    pub since: u64,
}

/// A version just activated, on trial until it has run a minute ([`Install::healthy`]):
/// `attempts` counts its starts.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trial {
    pub version: Version,
    /// The version it replaced, which a rollback returns to.
    pub previous: Version,
    pub attempts: u32,
}

/// A version on trial that started this many times without running a minute is rolled back.
pub const ATTEMPTS: u32 = 2;

/// What a start did before its window ([`Install::launch`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// Nothing to change: this program goes on.
    Go,
    /// The staged `version` is now the one that starts: start `program` in place of this one.
    Activated { version: Version, program: PathBuf },
    /// `version` failed to start [`ATTEMPTS`] times: `previous` is back, start `program`.
    RolledBack {
        version: Version,
        previous: Version,
        program: PathBuf,
    },
}

/// What a start of `running` does, given the trial and the staged version kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Go,
    /// This start of the version on trial is counted.
    Count(Trial),
    /// The version on trial has failed its starts.
    RollBack(Trial),
    Activate(Staged),
    /// Staged, but not newer than what runs: it goes.
    Forget,
}

pub(crate) fn step(running: &Version, trial: Option<&Trial>, staged: Option<&Staged>) -> Step {
    if let Some(trial) = trial.filter(|trial| trial.version == *running) {
        return if trial.attempts >= ATTEMPTS {
            Step::RollBack(trial.clone())
        } else {
            Step::Count(Trial {
                attempts: trial.attempts + 1,
                ..trial.clone()
            })
        };
    }
    match staged {
        Some(staged) if staged.version > *running => Step::Activate(staged.clone()),
        Some(_) => Step::Forget,
        None => Step::Go,
    }
}

const STAGED: &str = "staged.json";
const TRIAL: &str = "trial.json";
const BAD: &str = "bad-version";

impl Install {
    /// Where the installation keeps what is staged, on trial and refused: the installation
    /// itself; on macOS, whose installation is the Applications folder, a hidden folder there.
    fn marks(&self) -> PathBuf {
        if cfg!(target_os = "macos") {
            self.root.join(format!(".{BUNDLE}.update"))
        } else {
            self.root.clone()
        }
    }

    fn read<T: serde::de::DeserializeOwned>(&self, name: &str) -> Option<T> {
        serde_json::from_slice(&fs::read(self.marks().join(name)).ok()?).ok()
    }

    /// `value` kept as `name`, written beside it and renamed, on the disk before it returns.
    fn keep(&self, name: &str, value: &impl Serialize) -> io::Result<()> {
        let marks = self.marks();
        fs::create_dir_all(&marks)?;
        let next = marks.join(format!("{name}.new"));
        let mut file = fs::File::create(&next)?;
        io::Write::write_all(&mut file, &serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        fs::rename(&next, marks.join(name))
    }

    fn forget(&self, name: &str) {
        let _ = fs::remove_file(self.marks().join(name));
    }

    /// The version staged for the next start, if any.
    pub fn staged(&self) -> Option<Staged> {
        self.read(STAGED)
    }

    /// Keep `staged` as the version the next start activates; it replaces any other.
    pub fn keep_staged(&self, staged: &Staged) -> io::Result<()> {
        self.keep(STAGED, staged)
    }

    /// The version on trial, if any.
    pub fn trial(&self) -> Option<Trial> {
        self.read(TRIAL)
    }

    /// The newest version that was rolled back: no release up to it is installed again.
    pub fn bad(&self) -> Option<Version> {
        Version::parse(fs::read_to_string(self.marks().join(BAD)).ok()?.trim()).ok()
    }

    /// Activate the staged `staged` in place of `running`, on trial: the trial is kept before
    /// the switch, and the staged mark goes after it. Returns [`Self::program`].
    pub fn activate_staged(&self, staged: &Staged, running: &Version) -> io::Result<PathBuf> {
        self.keep(
            TRIAL,
            &Trial {
                version: staged.version.clone(),
                previous: running.clone(),
                attempts: 0,
            },
        )?;
        match self.activate(&staged.version) {
            Ok(program) => {
                self.forget(STAGED);
                Ok(program)
            }
            Err(error) => {
                self.forget(TRIAL);
                Err(error)
            }
        }
    }

    /// What a start of `running` does first, before its window: count a start of the version
    /// on trial and roll it back after [`ATTEMPTS`] failed ones (it is then refused for good,
    /// [`Self::bad`]), or activate a newer staged version.
    pub fn launch(&self, running: &Version) -> io::Result<Launch> {
        match step(running, self.trial().as_ref(), self.staged().as_ref()) {
            Step::Go => Ok(Launch::Go),
            Step::Forget => {
                self.forget(STAGED);
                Ok(Launch::Go)
            }
            Step::Count(trial) => self.keep(TRIAL, &trial).map(|()| Launch::Go),
            Step::Activate(staged) => {
                let program = self.activate_staged(&staged, running)?;
                Ok(Launch::Activated {
                    version: staged.version,
                    program,
                })
            }
            Step::RollBack(trial) => {
                // Whatever happens, this trial is over: a rollback that cannot be done is not
                // tried at every start.
                self.forget(TRIAL);
                let program = self.roll_back(&trial)?;
                let refused = self
                    .bad()
                    .map_or(trial.version.clone(), |bad| bad.max(trial.version.clone()));
                fs::write(self.marks().join(BAD), refused.to_string())?;
                Ok(Launch::RolledBack {
                    version: trial.version,
                    previous: trial.previous,
                    program,
                })
            }
        }
    }

    /// `running` has run a minute with its window up: its trial is over.
    pub fn healthy(&self, running: &Version) {
        if self.trial().is_some_and(|trial| trial.version == *running) {
            self.forget(TRIAL);
        }
    }
}

/// The folder holding the `quill.app` that `program` runs from, if it runs from one:
/// `<folder>/quill.app/Contents/MacOS/<program>`.
#[cfg(any(test, target_os = "macos"))]
pub(crate) fn bundle_root(program: &Path) -> Option<&Path> {
    let macos = program.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    (macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.file_name()? == BUNDLE)
        .then_some(bundle.parent()?)
}

/// Whether `head`, a program's first bytes, is how `os`'s programs start.
pub(crate) fn starts_native(os: &str, head: &[u8]) -> bool {
    match os {
        "windows" => head.starts_with(b"MZ"),
        // Mach-O 64-bit, little-endian; or a universal (fat) program.
        "macos" => {
            head.starts_with(&[0xcf, 0xfa, 0xed, 0xfe])
                || head.starts_with(&[0xca, 0xfe, 0xba, 0xbe])
        }
        _ => head.starts_with(b"\x7fELF"),
    }
}

/// The program at `path` is one for this system, or the error says `refused`.
fn native(path: &Path, refused: &str) -> io::Result<()> {
    let mut head = Vec::with_capacity(4);
    fs::File::open(path)?.take(4).read_to_end(&mut head)?;
    if starts_native(std::env::consts::OS, &head) {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidData, refused))
    }
}

/// Run a system tool to its end; what it wrote to stderr is the error.
pub(crate) fn run(command: &mut Command) -> io::Result<()> {
    let output = command.stdin(Stdio::null()).output()?;
    if output.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{}: {}",
        command.get_program().to_string_lossy(),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

/// Everything in `dir` reaches the disk. Links are not followed.
fn sync_tree(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_dir() {
            sync_tree(&entry.path())?;
        } else if kind.is_file() {
            // Windows flushes only a file open for writing; Unix any, read-only ones too.
            let file = if cfg!(windows) {
                fs::OpenOptions::new().write(true).open(entry.path())
            } else {
                fs::File::open(entry.path())
            };
            file?.sync_all()?;
        }
    }
    if cfg!(unix) {
        fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// The files of the folder `from` (not its folders, nor `skip`) copied into `to`.
#[cfg(not(target_os = "macos"))]
fn copy_files(from: &Path, to: &Path, skip: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() && path != skip {
            fs::copy(&path, to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// Delete the folder `dir` whole or not at all: Windows refuses to rename a folder while a file
/// in it is open, so the files of a quill still running stay.
#[cfg(windows)]
fn discard(dir: &Path) -> io::Result<()> {
    let name = dir.file_name().unwrap_or_default().to_string_lossy();
    if name.starts_with('.') {
        return fs::remove_dir_all(dir);
    }
    let aside = dir.with_file_name(format!(".{name}.{}.old", std::process::id()));
    fs::rename(dir, &aside)?;
    fs::remove_dir_all(aside)
}
