//! What Newen keeps on this computer, by kind, for Settings › Storage (docs/ui.md §2.11e):
//! measured on a blocking thread, and freed on the person's word. A session goes through
//! `Intent::Delete`, as from its own menu; here go a folder's file history, a worktree and the
//! caches.
use crate::{size_of, Config};
use newen_model::{
    SessionId, StorageClear, StorageReport, StoredHistory, StoredWorktree, STORAGE_IDLE_DAYS,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Where Newen keeps each kind of thing, from its configuration.
pub struct Places {
    root: PathBuf,
    pub database: PathBuf,
    pictures: Option<PathBuf>,
    snapshots: Option<PathBuf>,
    logos: Option<PathBuf>,
    avatars: Option<PathBuf>,
    tools: Option<PathBuf>,
    codecs: Option<PathBuf>,
    whats_new: Option<PathBuf>,
    update_log: Option<PathBuf>,
    /// The built-in browser's two profiles: Chromium beside the window (`chromium`, as the app
    /// places it) and the one it launches.
    browsers: Vec<PathBuf>,
    /// What the installation takes (`Install::footprint`): its folder on Linux and Windows,
    /// its bundle on macOS, where it shares a folder with other apps.
    app: Vec<PathBuf>,
}

impl Places {
    /// `None` without a database: nothing is kept on disk then.
    pub fn of(config: &Config) -> Option<Places> {
        let database = config.database.clone()?;
        let root = database.parent()?.to_path_buf();
        Some(Places {
            browsers: [Some(root.join("chromium")), config.browser.clone()]
                .into_iter()
                .flatten()
                .collect(),
            app: newen_update::install::Install::locate()
                .map(|install| install.footprint())
                .unwrap_or_default(),
            pictures: config.pictures.clone(),
            snapshots: config.snapshots.clone(),
            logos: config.logos.clone(),
            avatars: config.avatars.clone(),
            tools: config.tools.clone(),
            codecs: config.codecs.clone(),
            whats_new: config.whats_new.clone(),
            update_log: config.update_log.clone(),
            database,
            root,
        })
    }
}

/// A path as the file system names it: links followed and, on Windows, with the `\\?\` prefix
/// that git's own paths get too once resolved. One that is not there now keeps its own name.
fn real(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// A path as the person reads it: on Windows without the `\\?\` prefix a resolved one has (a
/// network share's `\\?\UNC\` keeps it). Clearing resolves it again.
fn plain(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if cfg!(windows) && !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

/// A name to compare as the file system does: without case on Windows and macOS.
fn compared(name: &Path) -> String {
    let name = name.to_string_lossy();
    if cfg!(any(windows, target_os = "macos")) {
        name.to_lowercase()
    } else {
        name.into_owned()
    }
}

/// The sessions' folders, resolved once, each with whether an active session works in it.
pub struct Folders(Vec<(PathBuf, bool)>);

impl Folders {
    pub fn of(folders: &[(PathBuf, bool)]) -> Folders {
        Folders(
            folders
                .iter()
                .map(|(f, active)| (real(f), *active))
                .collect(),
        )
    }

    /// An active session works in `path`, or in a folder inside it, or around it: an agent
    /// makes its worktrees inside the folder it works in.
    fn worked_in(&self, path: &Path) -> bool {
        (self.0.iter()).any(|(folder, active)| {
            *active && (folder.starts_with(path) || path.starts_with(folder))
        })
    }
}

/// What has been measured, so that nothing is walked twice: the data folder's total is made of
/// the parts.
#[derive(Default)]
struct Sizes(HashMap<PathBuf, u64>);

impl Sizes {
    fn of(&mut self, path: &Path) -> u64 {
        *self
            .0
            .entry(path.to_path_buf())
            .or_insert_with(|| size_of(path))
    }

    fn of_any(&mut self, path: &Option<PathBuf>) -> u64 {
        path.as_deref().map_or(0, |path| self.of(path))
    }

    /// A folder whose parts were measured one by one.
    fn of_parts(&mut self, dir: &Path) -> u64 {
        let total = entries(dir).map(|entry| self.of(&entry)).sum();
        self.0.insert(dir.to_path_buf(), total);
        total
    }
}

fn entries(dir: &Path) -> impl Iterator<Item = PathBuf> {
    (std::fs::read_dir(dir).into_iter().flatten().flatten()).map(|entry| entry.path())
}

/// Everything Newen keeps, by kind. `sizes` is each session's events (`newen_store::sizes`).
pub fn measure(places: &Places, sizes: &[(SessionId, u64)], folders: &Folders) -> StorageReport {
    let mut measured = Sizes::default();
    let database: u64 = ["", "-wal", "-shm"]
        .iter()
        .map(|end| {
            let mut name = places.database.clone().into_os_string();
            name.push(end);
            measured.of(Path::new(&name))
        })
        .sum();
    let pictures = places.pictures.as_deref();
    let pictures_total = pictures.map_or(0, |dir| measured.of_parts(dir));
    let by_session = (sizes.iter())
        .map(|(session, bytes)| {
            let own = pictures.map_or(0, |dir| measured.of(&dir.join(session.to_string())));
            (*session, bytes + own)
        })
        .collect();
    let history: Vec<StoredHistory> = (shadows(places, folders).into_iter())
        .map(|(path, name, used)| StoredHistory {
            bytes: measured.of(&path),
            name,
            used,
        })
        .collect();
    if let Some(snapshots) = &places.snapshots {
        measured.of_parts(snapshots);
    }
    // A cache folder is walked again inside its profile's walk: they are small beside it.
    let browser_cache: u64 = (places.browsers.iter())
        .flat_map(|profile| newen_browser::cache_dirs(profile))
        .map(|dir| size_of(&dir))
        .sum();
    let pending = (places.browsers.iter()).any(|p| p.join(newen_browser::CLEAR_CACHE).exists());
    let browser_logs: u64 = (places.browsers.iter())
        .map(|profile| size_of(&profile.join("chromium.log")))
        .sum();
    for profile in &places.browsers {
        measured.of(profile);
    }
    let logos = measured.of_any(&places.logos);
    let app_bytes: u64 = places.app.iter().map(|part| measured.of(part)).sum();
    let named = |items: Vec<(&str, u64)>| -> Vec<(String, u64)> {
        (items.into_iter())
            .filter(|(_, bytes)| *bytes > 0)
            .map(|(name, bytes)| (name.to_owned(), bytes))
            .collect()
    };
    let app = named(vec![
        ("Newen's installed versions", app_bytes),
        ("The GitHub CLI", measured.of_any(&places.tools)),
        ("The video codec", measured.of_any(&places.codecs)),
        ("Release notes", measured.of_any(&places.whats_new)),
    ]);
    let caches = named(vec![
        ("Logos", logos),
        ("Your GitHub picture", measured.of_any(&places.avatars)),
        (
            if pending {
                "The browser's cache, cleared the next time the browser starts"
            } else {
                "The browser's cache"
            },
            browser_cache,
        ),
        ("Logs", measured.of_any(&places.update_log) + browser_logs),
    ]);
    // The data folder's top, each part as measured above, and the installation when it lives
    // elsewhere (on Windows and macOS).
    let total = entries(&places.root)
        .map(|entry| measured.of(&entry))
        .sum::<u64>()
        + (places.app.iter())
            .filter(|part| !part.starts_with(&places.root))
            .map(|part| measured.of(part))
            .sum::<u64>();
    StorageReport {
        root: places.root.clone(),
        total,
        sessions: database + pictures_total,
        by_session,
        history,
        worktrees: (classify(folders).into_iter())
            .map(|w| StoredWorktree {
                bytes: size_of(&w.tree.path),
                path: plain(w.tree.path),
                branch: w.tree.branch,
                clean: w.clean,
                idle_days: w.idle_days,
                unused: w.unused,
            })
            .collect(),
        app,
        caches,
        clearable_caches: logos + browser_cache,
        failed: vec![],
    }
}

/// Each folder's shadow repository: where it is, the folder's name when a session works in
/// it (else the repository's own), and whether one does. A session's folder that is not there
/// now still names its own: its history is not unused.
fn shadows(places: &Places, folders: &Folders) -> Vec<(PathBuf, String, bool)> {
    let Some(root) = &places.snapshots else {
        return vec![];
    };
    let ours: Vec<(String, String)> = (folders.0.iter())
        .map(|(folder, _)| {
            let shadow = compared(&newen_git::shadow_dir(root, folder));
            (shadow, plain(folder.clone()).display().to_string())
        })
        .collect();
    let mut all: Vec<(PathBuf, String, bool)> = entries(root)
        .filter(|path| path.extension().is_some_and(|e| e == "git"))
        .map(|path| {
            let folder = ours.iter().find(|(shadow, _)| *shadow == compared(&path));
            let own = || {
                let name = path.file_name().unwrap_or_default();
                name.to_string_lossy().into_owned()
            };
            let name = folder.map_or_else(own, |(_, name)| name.clone());
            (path.clone(), name, folder.is_some())
        })
        .collect();
    all.sort_by(|a, b| a.1.cmp(&b.1));
    all
}

/// A worktree with what decides whether it can go.
struct Classified {
    tree: newen_git::Worktree,
    clean: bool,
    idle_days: u64,
    unused: bool,
}

/// The worktrees of the repositories the sessions work in, each repository once: clean,
/// unlocked, its commits held by a ref, no active session in it or around it, and no change in
/// its git state for `STORAGE_IDLE_DAYS` make it unused. Nothing here measures their size.
fn classify(folders: &Folders) -> Vec<Classified> {
    let mut repos = vec![];
    let mut all = vec![];
    for (folder, _) in &folders.0 {
        let Some(common) = newen_git::common_dir(folder) else {
            continue;
        };
        if repos.contains(&common) {
            continue;
        }
        repos.push(common);
        for mut tree in newen_git::worktrees(folder) {
            if !tree.path.is_dir() {
                continue;
            }
            // git names it by its real path, in its own form (`C:/…` on Windows).
            tree.path = real(&tree.path);
            let idle_days = newen_git::changed_at(&tree.path)
                .and_then(|at| SystemTime::now().duration_since(at).ok())
                .map_or(0, |idle| idle.as_secs() / 86_400);
            let clean = newen_git::is_clean(&tree.path);
            let unused = clean
                && !tree.locked
                && idle_days >= STORAGE_IDLE_DAYS
                && !folders.worked_in(&tree.path)
                && newen_git::is_kept(&tree.path, &tree.head);
            all.push(Classified {
                tree,
                clean,
                idle_days,
                unused,
            });
        }
    }
    all
}

/// Free what `clear` names; what could not go, and why. A worktree goes only if it is still
/// unused, classified again now.
pub fn clear(places: &Places, clear: &StorageClear, folders: &Folders) -> Vec<String> {
    let mut failed = vec![];
    match clear {
        StorageClear::History => {
            for (path, name, used) in shadows(places, folders) {
                if !used {
                    if let Err(error) = std::fs::remove_dir_all(&path) {
                        failed.push(format!("The file history of {name}: {error}"));
                    }
                }
            }
        }
        StorageClear::Worktrees(paths) => {
            let asked: Vec<PathBuf> = paths.iter().map(|p| real(p)).collect();
            for worktree in classify(folders) {
                if worktree.unused && asked.contains(&worktree.tree.path) {
                    if let Err(error) = newen_git::remove_worktree(&worktree.tree.path) {
                        let path = worktree.tree.path.display();
                        failed.push(format!("The worktree {path}: {error}"));
                    }
                }
            }
        }
        StorageClear::Caches => {
            // A logo that is not there is looked up again when it is shown.
            if let Some(logos) = &places.logos {
                if let Err(error) = std::fs::remove_dir_all(logos) {
                    if error.kind() != std::io::ErrorKind::NotFound {
                        failed.push(format!("The logos: {error}"));
                    }
                }
                let _ = std::fs::create_dir_all(logos);
            }
            for profile in places.browsers.iter().filter(|p| p.is_dir()) {
                let _ = std::fs::write(profile.join(newen_browser::CLEAR_CACHE), "");
            }
        }
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn places(root: &Path) -> Places {
        Places {
            database: root.join("newen.db"),
            pictures: Some(root.join("pictures")),
            snapshots: Some(root.join("snapshots")),
            logos: Some(root.join("logos")),
            avatars: Some(root.join("avatars")),
            tools: None,
            codecs: None,
            whats_new: None,
            update_log: Some(root.join("updates.log")),
            browsers: vec![root.join("chromium")],
            app: vec![root.join("app")],
            root: root.to_path_buf(),
        }
    }

    fn put(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    fn fresh(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("newen-storage-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_resolved_windows_path_reads_plain() {
        let path = PathBuf::from(r"\\?\C:\Users\a");
        let share = PathBuf::from(r"\\?\UNC\server\a");
        if cfg!(windows) {
            assert_eq!(plain(path), PathBuf::from(r"C:\Users\a"));
        } else {
            assert_eq!(plain(path.clone()), path);
        }
        assert_eq!(plain(share.clone()), share);
    }

    /// Each kind measured where it is kept: the sessions (their pictures each its own), the
    /// history of a folder a session works in, of one that is not there now and of one no
    /// session has, Newen, the caches; then only the history no session has and the caches go,
    /// the browser's at its next start.
    #[test]
    fn what_newen_keeps_is_measured_by_kind_and_freed() {
        let root = fresh("kinds");
        let places = places(&root);
        let folder = root.join("work");
        std::fs::create_dir_all(&folder).unwrap();
        let shadows = root.join("snapshots");
        let ours = newen_git::shadow_dir(&shadows, &real(&folder));
        let away = root.join("unmounted");
        let theirs = newen_git::shadow_dir(&shadows, &away);
        put(&root.join("newen.db"), 1000);
        put(&root.join("pictures/7/a.png"), 300);
        put(&ours.join("objects/x"), 50);
        put(&theirs.join("objects/z"), 60);
        put(&shadows.join("gone-folder.git/objects/y"), 70);
        put(&root.join("app/versions/0.2.2/newen"), 4000);
        put(&root.join("logos/svgl/a.png"), 20);
        put(&root.join("chromium/Default/Cache/data"), 500);
        put(&root.join("chromium/Default/Cookies"), 9);
        put(&root.join("updates.log"), 5);
        let folders = Folders::of(&[(folder.clone(), true), (away.clone(), false)]);
        let report = measure(&places, &[(7, 100), (8, 40)], &folders);
        assert_eq!(report.sessions, 1300);
        assert_eq!(report.by_session, [(7, 400), (8, 40)]);
        let used: Vec<_> = report.history.iter().filter(|h| h.used).collect();
        assert_eq!(used.len(), 2, "{:?}", report.history);
        let gone = report.history.iter().find(|h| !h.used).unwrap();
        assert_eq!((gone.name.as_str(), gone.bytes), ("gone-folder.git", 70));
        assert_eq!(
            report.app,
            [("Newen's installed versions".to_owned(), 4000)]
        );
        assert!(report
            .caches
            .contains(&("The browser's cache".to_owned(), 500)));
        assert_eq!(report.clearable_caches, 520);
        let everything = 1000 + 300 + 50 + 60 + 70 + 4000 + 20 + 500 + 9 + 5;
        assert_eq!(report.total, everything);
        assert!(clear(&places, &StorageClear::History, &folders).is_empty());
        assert!(clear(&places, &StorageClear::Caches, &folders).is_empty());
        let after = measure(&places, &[], &folders);
        assert!(after.history.iter().all(|h| h.used) && ours.exists() && theirs.exists());
        assert!(!after.caches.iter().any(|(name, _)| name == "Logos"));
        assert!(after
            .caches
            .iter()
            .any(|(name, _)| name.contains("next time")));
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Worktrees by what decides it: a clean old one is unused, unless an active session works
    /// around it (reached through a link) or it has changes; asked to go, only an unused one
    /// does, and it goes only if still unused.
    #[cfg(unix)]
    #[test]
    fn only_an_unused_worktree_goes() {
        let root = fresh("worktrees");
        let git = |dir: &Path, args: &[&str]| {
            let done = Command::new("git")
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(["-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap();
            assert!(done.status.success(), "{args:?}");
        };
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "a"]);
        let (old, busy, dirty) = (root.join("old"), repo.join("inside"), root.join("dirty"));
        for (path, branch) in [(&old, "old"), (&busy, "busy"), (&dirty, "dirty")] {
            git(
                &repo,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    branch,
                    path.to_str().unwrap(),
                ],
            );
        }
        put(&dirty.join("notes.txt"), 3);
        // Forty days without a change in each one's git state.
        let then = SystemTime::now() - std::time::Duration::from_secs(40 * 86_400);
        for gitdir in entries(&repo.join(".git/worktrees")) {
            for name in ["HEAD", "index", "logs/HEAD"] {
                if let Ok(file) = std::fs::File::options().write(true).open(gitdir.join(name)) {
                    file.set_modified(then).unwrap();
                }
            }
        }
        // The session works at the repository's top, named through a link.
        let link = root.join("link");
        std::os::unix::fs::symlink(&repo, &link).unwrap();
        let folders = Folders::of(&[(link.clone(), false)]);
        let by_name = |all: &[Classified], name: &str| {
            let tree = all.iter().find(|w| w.tree.path.ends_with(name)).unwrap();
            (tree.clean, tree.unused)
        };
        let all = classify(&folders);
        assert_eq!(by_name(&all, "old"), (true, true));
        assert_eq!(by_name(&all, "inside"), (true, true));
        assert_eq!(by_name(&all, "dirty"), (false, false));
        // Active, its session keeps the one inside its folder.
        let active = Folders::of(&[(link, true)]);
        let all = classify(&active);
        assert_eq!(by_name(&all, "inside"), (true, false));
        assert_eq!(by_name(&all, "old"), (true, true));
        let asked = StorageClear::Worktrees(vec![old.clone(), busy.clone(), dirty.clone()]);
        assert!(clear(&places(&root), &asked, &active).is_empty());
        assert!(!old.exists() && busy.exists() && dirty.join("notes.txt").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
