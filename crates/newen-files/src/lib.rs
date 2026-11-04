//! A folder's files as Newen's trees list and change them, on the disk: run beside the
//! window, never on it. What the tree may do, after openADE's: every target is a plain path
//! under the folder, links are never followed, nothing is overwritten, and deleting only
//! moves to the desktop's trash. Messages are for the person, without absolute paths.

pub mod find;

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// When a file or folder last changed on the disk; `None` when it is not there.
pub type Stamp = Option<SystemTime>;

/// A folder's entry, as the tree shows it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    pub dir: bool,
}

/// When `path` last changed.
pub fn stamp(path: &Path) -> Stamp {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The entries of `dir` as git sees it (its `.gitignore` kept, a repository or not, and
/// `.git` itself left out): folders first, then by name.
pub fn list(dir: &Path) -> Vec<Entry> {
    let mut entries: Vec<Entry> = ignore::WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        // A folder that is not a repository still keeps its .gitignore.
        .require_git(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.depth() == 1)
        .map(|e| Entry {
            dir: e.file_type().is_some_and(|t| t.is_dir()),
            path: e.into_path(),
        })
        .collect();
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.path.cmp(&b.path)));
    entries
}

/// One operation; every path is from the folder.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Op {
    CreateFile {
        parent: PathBuf,
        name: String,
    },
    CreateDir {
        parent: PathBuf,
        name: String,
    },
    Rename {
        path: PathBuf,
        name: String,
    },
    Trash {
        path: PathBuf,
    },
    /// Copies `sources` into the folder `into`, or moves them there when `cut`.
    Paste {
        sources: Vec<PathBuf>,
        cut: bool,
        into: PathBuf,
    },
}

/// A copy stops rather than follow a folder without end.
const MAX_COPY_DEPTH: usize = 64;
const MAX_COPY_ENTRIES: usize = 20_000;

/// Moves one absolute path to the trash: the desktop's, or a test's stand-in.
pub type Trash = fn(&Path) -> Result<(), String>;

pub fn system_trash(path: &Path) -> Result<(), String> {
    // The crate's own message can name paths outside the folder.
    trash::delete(path).map_err(|_| "the desktop's trash is not available".to_owned())
}

/// Runs `op` under `root`, and says the paths it made or moved to, from the folder.
pub fn run(root: &Path, op: &Op, trash: Trash) -> Result<Vec<PathBuf>, String> {
    Ok(match op {
        Op::CreateFile { parent, name } => vec![create(root, parent, name, false)?],
        Op::CreateDir { parent, name } => vec![create(root, parent, name, true)?],
        Op::Rename { path, name } => vec![rename(root, path, name)?],
        Op::Trash { path } => {
            trash_path(root, path, trash)?;
            vec![]
        }
        Op::Paste { sources, cut, into } => {
            let mut made = vec![];
            for source in sources {
                made.push(paste(root, source, *cut, into, trash)?);
            }
            made
        }
    })
}

/// Where `path` is once `old` moved to `new`, or `Some(None)` once `old` is gone; `None` when
/// `path` is neither `old` nor under it.
pub fn moved(path: &Path, old: &Path, new: Option<&Path>) -> Option<Option<PathBuf>> {
    let rest = path.strip_prefix(old).ok()?;
    Some(new.map(|new| {
        if rest.as_os_str().is_empty() {
            new.to_path_buf()
        } else {
            new.join(rest)
        }
    }))
}

pub fn valid_name(name: &str) -> Result<(), String> {
    Err(if name.trim().is_empty() {
        "Type a name"
    } else if name == "." || name == ".." {
        "\u{201c}.\u{201d} and \u{201c}..\u{201d} are not names"
    } else if name.contains(['/', '\\', '\0']) {
        "A name has no slashes"
    } else {
        return Ok(());
    }
    .to_owned())
}

/// The cause of an error without the path the system may add to it, as a sentence.
fn reason(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => "It is not there anymore".into(),
        io::ErrorKind::AlreadyExists => "Something has that name already".into(),
        io::ErrorKind::PermissionDenied => "Permission denied".into(),
        io::ErrorKind::StorageFull => "The disk is full".into(),
        kind => format!("The system refused it ({kind:?})"),
    }
}

/// The absolute path of `relative` under the real root. Only plain names are taken, a linked
/// folder on the way is refused rather than followed, and the last name is not resolved.
fn locate(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    const OUTSIDE: &str = "That is outside the folder";
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(OUTSIDE.into());
    }
    let root = root.canonicalize().map_err(|e| reason(&e))?;
    if relative.as_os_str().is_empty() {
        return Ok(root);
    }
    let target = root.join(relative);
    let parent = target.parent().unwrap_or(&root);
    if parent.canonicalize().map_err(|e| reason(&e))? != parent {
        return Err(OUTSIDE.into());
    }
    Ok(target)
}

fn create(root: &Path, parent: &Path, name: &str, folder: bool) -> Result<PathBuf, String> {
    valid_name(name)?;
    let dir = locate(root, parent)?;
    // A link to a folder is a link, not a place to make things in.
    if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
        return Err("Its folder is not there anymore".into());
    }
    let target = dir.join(name);
    // Both fail on anything already there, a dangling link too.
    if folder {
        fs::create_dir(&target)
    } else {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map(drop)
    }
    .map_err(|e| reason(&e))?;
    Ok(parent.join(name))
}

fn rename(root: &Path, path: &Path, name: &str) -> Result<PathBuf, String> {
    valid_name(name)?;
    if path.file_name().is_none() {
        return Err("The folder itself is not renamed here".into());
    }
    let source = locate(root, path)?;
    fs::symlink_metadata(&source).map_err(|e| reason(&e))?;
    move_without_replacing(&source, &source.with_file_name(name)).map_err(|e| reason(&e))?;
    Ok(path.with_file_name(name))
}

fn trash_path(root: &Path, path: &Path, trash: Trash) -> Result<(), String> {
    if path.file_name().is_none() {
        return Err("The folder itself is not trashed here".into());
    }
    let target = locate(root, path)?;
    fs::symlink_metadata(&target).map_err(|e| reason(&e))?;
    trash(&target)
}

/// Pastes one entry into `into` and says where it went. A copy never replaces: a taken name
/// gets " copy", " copy 2"… A cut moves, and across disks copies and trashes the original.
fn paste(
    root: &Path,
    source: &Path,
    cut: bool,
    into: &Path,
    trash: Trash,
) -> Result<PathBuf, String> {
    let Some(name) = source.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Err("The folder itself is not pasted here".into());
    };
    let from = locate(root, source)?;
    let folder = locate(root, into)?;
    let metadata = fs::symlink_metadata(&from).map_err(|e| reason(&e))?;
    if !fs::symlink_metadata(&folder).is_ok_and(|m| m.is_dir()) {
        return Err("Its folder is not there anymore".into());
    }
    if metadata.is_dir() && into.starts_with(source) {
        return Err("A folder does not go inside itself".into());
    }
    if cut && source.parent() == Some(into) {
        // It is there already: moving it would only rename it.
        return Ok(source.to_path_buf());
    }
    let name = free_name(&folder, &name, metadata.is_dir())?;
    let (target, relative) = (folder.join(&name), into.join(&name));
    if cut {
        match move_without_replacing(&from, &target) {
            Ok(()) => return Ok(relative),
            Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {}
            Err(error) => return Err(reason(&error)),
        }
    }
    copy_entry(&from, &target, 0, &mut MAX_COPY_ENTRIES.clone()).map_err(|e| {
        match e.get_ref() {
            Some(limit) => limit.to_string(),
            None => reason(&e),
        }
    })?;
    if cut {
        trash(&from).map_err(|e| format!("Copied, but the original is still there: {e}"))?;
    }
    Ok(relative)
}

/// `name` when nothing in `folder` has it, else the first free of "name copy", "name copy 2"…,
/// kept before a file's extension.
fn free_name(folder: &Path, name: &str, folder_too: bool) -> Result<String, String> {
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if !folder_too && dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    (0..1000)
        .map(|attempt| match attempt {
            0 => name.to_owned(),
            1 => format!("{stem} copy{extension}"),
            _ => format!("{stem} copy {attempt}{extension}"),
        })
        .find(|candidate| fs::symlink_metadata(folder.join(candidate)).is_err())
        .ok_or_else(|| "There are too many copies of that name".to_owned())
}

/// Copies a file, a folder with what it holds, or a link as a link. Nothing is followed and
/// nothing there is written over.
fn copy_entry(from: &Path, to: &Path, depth: usize, budget: &mut usize) -> io::Result<()> {
    let limit = |message: &'static str| io::Error::other(message);
    *budget = budget
        .checked_sub(1)
        .ok_or_else(|| limit("The folder holds too many things"))?;
    let metadata = fs::symlink_metadata(from)?;
    let kind = metadata.file_type();
    if kind.is_symlink() {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(fs::read_link(from)?, to);
        // shortcut: Windows links need a privilege and a file/folder distinction; not copied.
        #[cfg(not(unix))]
        return Err(limit("Links are not copied on this system"));
    }
    if kind.is_dir() {
        if depth >= MAX_COPY_DEPTH {
            return Err(limit("The folder is too deep"));
        }
        fs::create_dir(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_entry(
                &entry.path(),
                &to.join(entry.file_name()),
                depth + 1,
                budget,
            )?;
        }
    } else if kind.is_file() {
        let mut source = fs::OpenOptions::new();
        source.read(true);
        // A file swapped for a link after the check above is not followed.
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::custom_flags(&mut source, libc::O_NOFOLLOW);
        let mut target = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(to)?;
        io::copy(&mut source.open(from)?, &mut target)?;
    } else {
        return Err(limit("Only files, folders and links are copied"));
    }
    // Last, so a read-only folder still takes what it holds.
    fs::set_permissions(to, metadata.permissions())
}

/// Rename `source` to `target` only if nothing is there: atomically on Linux.
fn move_without_replacing(source: &Path, target: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let c_path = |path: &Path| {
            std::ffi::CString::new(path.as_os_str().as_bytes())
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
        };
        let (from, to) = (c_path(source)?, c_path(target)?);
        // SAFETY: both pointers are NUL-terminated paths that outlive the call.
        let moved = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if moved == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        // A filesystem without the flag takes the checked rename below.
        if !matches!(error.raw_os_error(), Some(libc::EINVAL | libc::ENOSYS)) {
            return Err(error);
        }
    }
    // shortcut: between this check and the rename another process can make `target` and lose
    // it; only Linux has the atomic form above.
    if fs::symlink_metadata(target).is_ok() {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    fs::rename(source, target)
}

/// The person's home folder, on any system.
pub fn home_dir() -> Option<PathBuf> {
    std::env::home_dir().filter(|home| !home.as_os_str().is_empty())
}

/// Where an app keeps its data and its settings: `$XDG_DATA_HOME` / `$XDG_CONFIG_HOME` (or
/// `~/.local/share`, `~/.config`) on Linux, `~/Library/Application Support` on macOS,
/// `%LOCALAPPDATA%` / `%APPDATA%` on Windows. Inferred from each system's convention.
pub fn data_dir() -> Option<PathBuf> {
    app_dir("XDG_DATA_HOME", ".local/share", "LOCALAPPDATA")
}

/// See [`data_dir`].
pub fn config_dir() -> Option<PathBuf> {
    app_dir("XDG_CONFIG_HOME", ".config", "APPDATA")
}

/// The environment variable `NEWEN_<name>`, else `QUILL_<name>`, its name before Newen: for
/// those a person may have set themselves, or a copy of quill hands on (docs/UPDATES.md, «Venir
/// de quill»). Newen sets only its own.
pub fn env(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(format!("NEWEN_{name}")).or_else(|| std::env::var_os(format!("QUILL_{name}")))
}

/// Whether quill, Newen's name before, kept its data in the data folder `data`: its database is
/// in `quill/`, or in `newen/` until Newen's first start renames it, last. Another app's folder
/// of that name (Windows and macOS find a `Quill` as `quill`) has none.
pub fn quill_was_here(data: &Path) -> bool {
    data.join("quill/quill.db").is_file() || data.join("newen/quill.db").is_file()
}

/// Newen's own folder in `base`, the data folder `data` or the settings folder beside it
/// ([`data_dir`], [`config_dir`]): `newen/`; `quill/`, the name it had, while that is there,
/// `newen/` is not, and quill's data is in `data` ([`quill_was_here`], [`named_dir`]).
pub fn own_dir(base: &Path, data: &Path) -> PathBuf {
    if quill_was_here(data) {
        named_dir(base, "newen", "quill")
    } else {
        base.join("newen")
    }
}

/// `base/name`; `base/legacy`, the folder's name before Newen, while that is a folder and
/// `base/name` is not there. A first start moves it (docs/UPDATES.md, «Venir de quill»); until
/// then, or when it could not, the data stays where it is read, and nothing starts empty
/// beside it.
pub fn named_dir(base: &Path, name: &str, legacy: &str) -> PathBuf {
    let (dir, old) = (base.join(name), base.join(legacy));
    if fs::symlink_metadata(&dir).is_err() && old.is_dir() {
        old
    } else {
        dir
    }
}

/// `base/legacy` moved to `base/name` by a rename (the same folder: never a copy, all or
/// nothing), leaving on Unix a link `legacy -> name` for what still looks there (quill after a
/// rollback, a launcher). Nothing to do, false, when `base/name` is there or `base/legacy` is
/// not a folder. An error leaves everything where it was.
pub fn move_legacy(base: &Path, name: &str, legacy: &str) -> io::Result<bool> {
    let (dir, old) = (base.join(name), base.join(legacy));
    if fs::symlink_metadata(&dir).is_ok() || !old.is_dir() {
        return Ok(false);
    }
    fs::rename(&old, &dir)?;
    // Best effort: quill 0.1.9 finds `name` without it.
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(name, &old);
    Ok(true)
}

fn app_dir(xdg: &str, linux: &str, windows: &str) -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os(windows).map(PathBuf::from);
    }
    if cfg!(target_os = "macos") {
        return Some(home_dir()?.join("Library/Application Support"));
    }
    std::env::var_os(xdg)
        .map(PathBuf::from)
        .or_else(|| Some(home_dir()?.join(linux)))
}

/// A page, a file or a folder in what the computer opens it with (its default browser for an
/// address), on Linux, Windows and macOS alike. Windows goes through `rundll32`, not
/// `cmd /C start`: cmd would split a sign-in address at its `&`.
pub fn open_in_system(target: impl AsRef<std::ffi::OsStr>) -> std::io::Result<std::process::Child> {
    let (program, args): (&str, &[&str]) = if cfg!(target_os = "windows") {
        ("rundll32", &["url.dll,FileProtocolHandler"])
    } else if cfg!(target_os = "macos") {
        ("open", &[])
    } else {
        ("xdg-open", &[])
    };
    command(program)
        .args(args)
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
}

/// A process to start, the same on every system: on Windows a bare name is found through
/// `PATH` and `PATHEXT` (npm's CLIs are `claude.cmd`, which `Command::new` alone never finds;
/// std runs a `.cmd` it is given by path through cmd.exe, safely), and a console program
/// opens no console window of its own (`CREATE_NO_WINDOW`).
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let program = program.as_ref();
        let found =
            std::env::var_os("PATH").and_then(|path| find_program(program, &path, &pathext()));
        let mut command =
            std::process::Command::new(found.as_deref().map_or(program, Path::as_os_str));
        command.creation_flags(0x0800_0000);
        command
    }
    #[cfg(not(windows))]
    std::process::Command::new(program)
}

#[cfg(windows)]
fn pathext() -> std::ffi::OsString {
    std::env::var_os("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
}

/// Whether [`command`] finds `program`: a path to a file, or a bare name in one of `PATH`'s
/// folders (on Windows with one of `PATHEXT`'s extensions, too).
pub fn installed(program: impl AsRef<std::ffi::OsStr>) -> bool {
    let program = Path::new(program.as_ref());
    if program.parent() != Some(Path::new("")) {
        return program.is_file();
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    #[cfg(windows)]
    if find_program(program.as_os_str(), &path, &pathext()).is_some() {
        return true;
    }
    std::env::split_paths(&path).any(|dir| dir.join(program).is_file())
}

/// Puts `~/.local/bin` on `PATH` when it is not: a desktop launcher's `PATH` often lacks it,
/// and the agents' installers put their programs there. Called first thing, before threads.
pub fn add_local_bin_to_path() {
    let Some(bin) = home_dir().map(|home| home.join(".local").join("bin")) else {
        return;
    };
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    if dirs.contains(&bin) {
        return;
    }
    dirs.push(bin);
    if let Ok(joined) = std::env::join_paths(dirs) {
        std::env::set_var("PATH", joined);
    }
}

/// The first `name` + one of `pathext`'s extensions (`;`-separated) that is a file in one of
/// `path`'s folders, in that order, as cmd.exe looks; `None` for a name with a folder or an
/// extension of its own.
#[cfg(any(windows, test))]
fn find_program(
    name: &std::ffi::OsStr,
    path: &std::ffi::OsStr,
    pathext: &std::ffi::OsStr,
) -> Option<PathBuf> {
    let bare = Path::new(name);
    if bare.extension().is_some() || bare.parent() != Some(Path::new("")) {
        return None;
    }
    let pathext = pathext.to_str()?;
    std::env::split_paths(path)
        .flat_map(|dir| {
            pathext
                .split(';')
                .filter(|ext| !ext.is_empty())
                .map(move |ext| {
                    let mut file = name.to_os_string();
                    file.push(ext);
                    dir.join(file)
                })
        })
        .find(|file| file.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Newen's variable first, quill's when Newen's is not set.
    #[test]
    fn newen_s_variable_is_read_and_quill_s_when_it_is_not_set() {
        let name = format!("ENV_TEST_{}", std::process::id());
        assert_eq!(env(&name), None);
        std::env::set_var(format!("QUILL_{name}"), "quill's");
        assert_eq!(env(&name).as_deref(), Some("quill's".as_ref()));
        std::env::set_var(format!("NEWEN_{name}"), "newen's");
        assert_eq!(env(&name).as_deref(), Some("newen's".as_ref()));
    }

    /// A folder is Newen's own once it is there; before, quill's while quill's data is there:
    /// never an empty one beside the person's data, nor another app's folder named quill. A
    /// link left in quill's place changes nothing. The settings folder follows the data's.
    #[test]
    fn newen_s_folder_is_used_and_quill_s_only_while_it_was_not_moved() {
        let base = std::env::temp_dir().join(format!("newen-own-dir-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let config = base.join("config");
        fs::create_dir_all(config.join("quill")).unwrap();
        let own = || (own_dir(&base, &base), own_dir(&config, &base));
        let newen = (base.join("newen"), config.join("newen"));
        assert_eq!(own(), newen, "a first install");
        fs::write(base.join("quill"), "a file, not a folder").unwrap();
        assert_eq!(own(), newen);
        fs::remove_file(base.join("quill")).unwrap();
        fs::create_dir(base.join("quill")).unwrap();
        assert_eq!(own(), newen, "another app's: no database of quill's");
        fs::write(base.join("quill/quill.db"), "").unwrap();
        let quill = (base.join("quill"), config.join("quill"));
        assert_eq!(own(), quill, "not moved yet");
        fs::create_dir(base.join("newen")).unwrap();
        assert_eq!(own_dir(&base, &base), base.join("newen"), "both: Newen's");
        fs::remove_dir(base.join("newen")).unwrap();
        fs::rename(base.join("quill"), base.join("newen")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("newen", base.join("quill")).unwrap();
        assert_eq!(
            own(),
            (base.join("newen"), config.join("quill")),
            "the data moved, its database not renamed yet: the settings are still quill's"
        );
        fs::rename(base.join("newen/quill.db"), base.join("newen/newen.db")).unwrap();
        assert_eq!(own_dir(&base, &base), base.join("newen"), "moved");
        assert_eq!(
            named_dir(&base, "newen-ide", "quill-ide"),
            base.join("newen-ide")
        );
        let _ = fs::remove_dir_all(&base);
    }

    /// quill's folder is moved to Newen's once, a link left in its place on Unix; never over a
    /// folder of Newen's, nor when there is no folder of quill's.
    #[test]
    fn quill_s_folder_is_moved_once_and_never_over_newen_s() {
        let base = std::env::temp_dir().join(format!("newen-move-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("quill")).unwrap();
        fs::write(base.join("quill/kept"), "x").unwrap();
        assert!(move_legacy(&base, "newen", "quill").unwrap());
        assert_eq!(fs::read_to_string(base.join("newen/kept")).unwrap(), "x");
        #[cfg(unix)]
        assert_eq!(
            fs::read_link(base.join("quill")).unwrap(),
            Path::new("newen")
        );
        assert!(!move_legacy(&base, "newen", "quill").unwrap(), "once");
        assert!(!move_legacy(&base, "newen-ide", "quill-ide").unwrap());
        fs::create_dir_all(base.join("quill-ide")).unwrap();
        fs::create_dir_all(base.join("newen-ide")).unwrap();
        assert!(!move_legacy(&base, "newen-ide", "quill-ide").unwrap());
        assert!(base.join("quill-ide").is_dir(), "left alone");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_bare_program_name_is_found_through_path_and_pathext() {
        let root = std::env::temp_dir().join(format!("newen-files-path-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (first, second) = (root.join("first"), root.join("second"));
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::write(first.join("tool.CMD"), "").unwrap();
        fs::write(second.join("tool.EXE"), "").unwrap();
        let path = std::env::join_paths([&first, &second]).unwrap();
        let find = |name: &str| find_program(name.as_ref(), &path, ".COM;.EXE;.BAT;.CMD".as_ref());
        // The first folder wins, whatever the extension's rank.
        assert_eq!(find("tool"), Some(first.join("tool.CMD")));
        assert_eq!(find("missing"), None);
        // A name with an extension or a folder is left as it is.
        assert_eq!(find("tool.CMD"), None);
        assert_eq!(find("first/tool"), None);
        fs::remove_dir_all(&root).unwrap();
    }

    /// A program is installed when its path is a file, or its bare name is on `PATH`.
    #[test]
    fn a_program_is_installed_by_path_or_on_path() {
        let me = std::env::current_exe().unwrap();
        assert!(installed(&me));
        assert!(!installed(me.with_file_name("newen-no-such-program")));
        assert!(!installed("newen-no-such-program"));
        assert!(!installed(""));
        #[cfg(unix)]
        assert!(installed("sh"));
    }

    fn kept(path: &Path) -> Result<(), String> {
        let dir = path.parent().unwrap().join(".trashed");
        fs::create_dir_all(&dir).unwrap();
        fs::rename(path, dir.join(path.file_name().unwrap())).map_err(|e| e.to_string())
    }

    #[test]
    fn files_are_made_renamed_and_trashed_without_overwriting_or_leaving_the_folder() {
        let root = std::env::temp_dir().join(format!("newen-ide-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let op = |op| run(&root, &op, kept);
        let file = |parent: &str, name: &str| Op::CreateFile {
            parent: parent.into(),
            name: name.into(),
        };
        assert_eq!(
            op(Op::CreateDir {
                parent: "".into(),
                name: "src".into()
            }),
            Ok(vec!["src".into()])
        );
        assert_eq!(op(file("src", "a.rs")), Ok(vec!["src/a.rs".into()]));
        fs::write(root.join("src/a.rs"), "kept").unwrap();
        assert_eq!(
            op(file("src", "a.rs")),
            Err("Something has that name already".into())
        );
        assert_eq!(op(file("src", "x/y")), Err("A name has no slashes".into()));
        assert_eq!(
            op(file("../out", "z")),
            Err("That is outside the folder".into())
        );
        op(file("src", "b.rs")).unwrap();
        let rename = |path: &str, name: &str| Op::Rename {
            path: path.into(),
            name: name.into(),
        };
        assert_eq!(
            op(rename("src/b.rs", "a.rs")),
            Err("Something has that name already".into())
        );
        assert_eq!(fs::read_to_string(root.join("src/a.rs")).unwrap(), "kept");
        assert_eq!(op(rename("src/b.rs", "c.rs")), Ok(vec!["src/c.rs".into()]));
        assert_eq!(
            op(Op::Trash {
                path: "src/c.rs".into()
            }),
            Ok(vec![])
        );
        let left = root.join("src/.trashed/c.rs").exists() && !root.join("src/c.rs").exists();
        // Pasted copies take free names, and a folder does not go inside itself.
        let paste = |sources: &[&str], cut, into: &str| Op::Paste {
            sources: sources.iter().map(PathBuf::from).collect(),
            cut,
            into: into.into(),
        };
        assert_eq!(
            op(paste(&["src/a.rs"], false, "src")),
            Ok(vec!["src/a copy.rs".into()])
        );
        assert_eq!(
            op(paste(&["src/a.rs"], false, "src")),
            Ok(vec!["src/a copy 2.rs".into()])
        );
        assert_eq!(
            op(paste(&["src"], false, "src")),
            Err("A folder does not go inside itself".into())
        );
        op(Op::CreateDir {
            parent: "".into(),
            name: "lib".into(),
        })
        .unwrap();
        assert_eq!(
            op(paste(&["src/a copy.rs"], true, "lib")),
            Ok(vec!["lib/a copy.rs".into()])
        );
        let went = !root.join("src/a copy.rs").exists() && root.join("lib/a copy.rs").exists();
        assert_eq!(
            fs::read_to_string(root.join("src/a copy 2.rs")).unwrap(),
            "kept"
        );
        fs::remove_dir_all(&root).unwrap();
        assert!(left && went);
        let at = Path::new("src/a.rs");
        assert_eq!(
            moved(at, Path::new("src"), Some(Path::new("lib"))),
            Some(Some("lib/a.rs".into()))
        );
        assert_eq!(moved(at, Path::new("src/a.rs"), None), Some(None));
        assert_eq!(moved(at, Path::new("docs"), None), None);
    }
}
