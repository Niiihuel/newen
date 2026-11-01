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

use crate::Version;

/// The program's file name in a bundle.
#[cfg(not(windows))]
pub const PROGRAM: &str = "quill";
#[cfg(windows)]
pub const PROGRAM: &str = "quill.exe";

/// CEF's library: a folder holding it beside the program is a bundle.
#[cfg(not(any(windows, target_os = "macos")))]
const CEF: &str = "libcef.so";
#[cfg(windows)]
const CEF: &str = "libcef.dll";

/// The bundle's name on macOS.
const BUNDLE: &str = "quill.app";

/// Where quill is installed for this person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    root: PathBuf,
}

impl Install {
    /// `QUILL_UPDATE_ROOT` when set (a test installation), or this person's:
    /// `$XDG_DATA_HOME/quill/app` on Linux, `%LOCALAPPDATA%\Programs\quill` on Windows, and on
    /// macOS the folder holding the `quill.app` this program runs from, else `~/Applications`.
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
        let root = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|base| base.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })?
            .join("quill")
            .join("app");
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

    /// What a start runs: a path that stays the same across updates, for launchers.
    pub fn program(&self) -> PathBuf {
        if cfg!(windows) {
            self.root.join(PROGRAM)
        } else if cfg!(target_os = "macos") {
            self.root.join(BUNDLE).join("Contents/MacOS").join(PROGRAM)
        } else {
            self.root.join("current").join(PROGRAM)
        }
    }

    /// The version `current` names, if any.
    #[cfg(not(any(windows, target_os = "macos")))]
    pub fn current(&self) -> Option<String> {
        let target = fs::read_link(self.root.join("current")).ok()?;
        Some(target.file_name()?.to_str()?.to_owned())
    }

    /// Install `version` from the artifact at `artifact` (verified already) and make it current:
    /// on Linux a gzipped tar of the bundle, on Windows a zip of it, on macOS a zip holding
    /// `quill.app`. Returns [`Self::program`].
    pub fn apply(&self, artifact: &Path, version: &Version) -> io::Result<PathBuf> {
        #[cfg(not(any(windows, target_os = "macos")))]
        return self.place(version, |staging| {
            run(Command::new("tar")
                .arg("-xzf")
                .arg(artifact)
                .arg("-C")
                .arg(staging))
        });
        #[cfg(windows)]
        return self.place(version, |staging| {
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
        return self.place(version, |staging| {
            run(Command::new("/usr/bin/ditto")
                .args(["-x", "-k"])
                .arg(artifact)
                .arg(staging))?;
            // A bundle that is not intact and signed as released would not open.
            run(Command::new("/usr/bin/codesign")
                .args(["--verify", "--deep", "--strict"])
                .arg(staging.join(BUNDLE)))
        });
    }

    /// Install `program`, this very file, as `version` (`quill --install`): with CEF's files
    /// beside it when its folder is a bundle (on macOS, the `quill.app` it is in).
    pub fn adopt(&self, program: &Path, version: &Version) -> io::Result<PathBuf> {
        if self.runs(program) {
            return Ok(self.program());
        }
        #[cfg(not(target_os = "macos"))]
        return self.place(version, |staging| {
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
        });
        #[cfg(target_os = "macos")]
        return self.place(version, |staging| {
            let from = bundle_root(program).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "quill --install runs from quill.app",
                )
            })?;
            run(Command::new("/usr/bin/ditto")
                .arg(from.join(BUNDLE))
                .arg(staging.join(BUNDLE)))
        });
    }

    /// Fill a temporary folder with `version`'s bundle by `fill`, move it into place and make it
    /// current, or leave everything as it was.
    #[cfg(not(any(windows, target_os = "macos")))]
    fn place(
        &self,
        version: &Version,
        fill: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<PathBuf> {
        let name = version.to_string();
        if self.current().as_deref() == Some(name.as_str()) {
            return Ok(self.program());
        }
        let versions = self.root.join("versions");
        fs::create_dir_all(&versions)?;
        let staging = versions.join(format!(".{name}.{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir(&staging)?;
        let staged = fill(&staging)
            // A release for another system would never start: refuse it before it replaces
            // one that does.
            .and_then(|()| native(&staging.join(PROGRAM), "the release is not a Linux program"))
            .and_then(|()| sync_tree(&staging));
        let target = versions.join(&name);
        let moved = staged.and_then(|()| {
            // A copy an earlier try left, never current.
            let _ = fs::remove_dir_all(&target);
            fs::rename(&staging, &target)
        });
        if let Err(error) = moved {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        let previous = self.current();
        let link = self.root.join("current.new");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(Path::new("versions").join(&name), &link)?;
        fs::rename(&link, self.root.join("current"))?;
        // The rename reaches the disk before anything is deleted.
        if let Ok(folder) = fs::File::open(&self.root) {
            let _ = folder.sync_all();
        }
        self.prune(&name, previous.as_deref());
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
    /// `versions\{version}`. Then the running program is renamed to `quill.exe.old` (kept until
    /// the next update replaces it) and the new one takes its name. Versions that are neither
    /// the new one nor this running one's are deleted.
    #[cfg(windows)]
    fn place(
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
        let new = self.root.join(format!("{PROGRAM}.{name}.new"));
        let target = versions.join(&name);
        let placed = fill(&staging).and_then(|()| {
            let _ = fs::remove_file(&new);
            fs::rename(staging.join(PROGRAM), &new)?;
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
        let program = self.program();
        let aside = self.root.join(format!("{PROGRAM}.old"));
        let _ = fs::remove_file(&aside);
        if program.exists() {
            fs::rename(&program, &aside)?;
        }
        if let Err(error) = fs::rename(&new, &program) {
            let _ = fs::rename(&aside, &program);
            let _ = fs::remove_file(&new);
            return Err(error);
        }
        for entry in fs::read_dir(&versions).into_iter().flatten().flatten() {
            let found = entry.file_name();
            let found = found.to_string_lossy();
            if found != name && found != crate::VERSION {
                let _ = discard(&entry.path());
            }
        }
        Ok(program)
    }

    /// macOS: the bundle `fill` leaves in `.quill.app.{version}.{pid}/` replaces `quill.app`,
    /// which is renamed to `.quill.app.old` (deleted by the next update).
    #[cfg(target_os = "macos")]
    fn place(
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
        let new = staging.join(BUNDLE);
        let staged = fill(&staging)
            .and_then(|()| {
                native(
                    &new.join("Contents/MacOS").join(PROGRAM),
                    "the release is not a macOS program",
                )
            })
            .and_then(|()| sync_tree(&staging));
        let bundle = self.root.join(BUNDLE);
        let aside = self.root.join(format!(".{BUNDLE}.old"));
        let moved = staged.and_then(|()| {
            let _ = fs::remove_dir_all(&aside);
            if bundle.exists() {
                fs::rename(&bundle, &aside)?;
            }
            fs::rename(&new, &bundle).inspect_err(|_| {
                let _ = fs::rename(&aside, &bundle);
            })
        });
        let _ = fs::remove_dir_all(&staging);
        moved.map(|()| self.program())
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
fn run(command: &mut Command) -> io::Result<()> {
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
