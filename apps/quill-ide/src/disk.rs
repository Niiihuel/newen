//! The `Job`s, on the disk: run on a thread of their own, never the window's.

use crate::ide::{Job, Loaded, Stamp};
use limn_editor::BufferSnapshot;
use quill_files::{list, stamp};
use std::io::{self, Write};
use std::path::Path;

pub fn run(job: Job) -> Loaded {
    match job {
        // Stamped before they are read, so a change while reading shows at the next look.
        Job::List(dir) => {
            let stamp = stamp(&dir);
            Loaded::Listed(dir.clone(), list(&dir), stamp)
        }
        Job::Read(path) => {
            let stamp = stamp(&path);
            let text = std::fs::read(&path)
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok());
            Loaded::Read(path, text, stamp)
        }
        Job::Write(path, save) => {
            let written = write(&path, &save.0).map_err(|e| e.to_string());
            Loaded::Written(path, save, written)
        }
        Job::SavePrefs(prefs) => {
            // shortcut: a failed save of Settings is only said on stderr; worth a line in
            // Settings if it ever happens.
            if let Err(error) = save_prefs(&prefs) {
                eprintln!("quill-ide: Settings not saved: {error}");
            }
            Loaded::PrefsSaved
        }
        Job::FindPacks(roots) => Loaded::Packs(quill_components::packs::find(&roots)),
        Job::LoadPack(json) => {
            let theme = quill_components::packs::load(&json);
            Loaded::Pack(json, theme)
        }
        Job::Branch(root) => {
            let head = head_file(&root);
            let branch = head.as_deref().and_then(branch_in);
            Loaded::Branch(
                branch,
                head.map(|h| {
                    let stamp = stamp(&h);
                    (h, stamp)
                }),
            )
        }
        Job::FindFonts => Loaded::Fonts(monospaced()),
        Job::FindFiles(root) => Loaded::Files(quill_files::find::files(&root)),
        Job::File(root, op) => {
            let done = quill_files::run(&root, &op, quill_files::system_trash);
            Loaded::File(op, done)
        }
        Job::ApplyEdits(path, edits) => {
            let done = apply_edits(&path, &edits);
            Loaded::EditsApplied(path, done)
        }
        Job::Analyze(path, save) => {
            let structure = crate::structure::analyze(&save.0, &path);
            Loaded::Analyzed(path, structure)
        }
        Job::Committed(path) => {
            let text = crate::marks::committed(&path);
            Loaded::Committed(path, text)
        }
        Job::Changes(path, committed, save) => {
            let now: String = save.0.chunks().collect();
            let changes = crate::marks::changes(&committed, &now);
            Loaded::Changes(path, save.0.revision(), changes)
        }
        Job::Grep(root, query, count) => {
            let hits = crate::grep::grep(&root, &quill_files::find::files(&root), &query);
            Loaded::Grep(count, hits)
        }
        Job::SaveSession(root, session) => {
            // shortcut: a session that fails to save is only said on stderr, as Settings.
            if let Err(error) = crate::session::save(&root, &session) {
                eprintln!("quill-ide: the open files not kept: {error}");
            }
            Loaded::SessionSaved
        }
        Job::LoadFont(family) => {
            let faces = faces(&family);
            Loaded::Font(family, faces)
        }
        Job::Stat(watched) => Loaded::Changed(
            watched
                .into_iter()
                .filter_map(|(path, known)| {
                    let now = stamp(&path);
                    (now != known).then_some((path, now))
                })
                .collect(),
        ),
    }
}

/// A language server's edits made to `path` on the disk, whole or not at all.
fn apply_edits(path: &Path, edits: &str) -> Result<Stamp, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let edits: serde_json::Value = serde_json::from_str(edits).map_err(|e| e.to_string())?;
    let mut buffer = limn_editor::TextBuffer::new(&text).map_err(|e| format!("{e:?}"))?;
    let edits =
        limn_language::lsp::text_edits(&buffer.snapshot(), &edits).map_err(|e| e.message)?;
    let caret = limn_editor::Selection::caret(0);
    buffer
        .apply_transaction(&edits, caret, limn_editor::EditKind::Replace, 0)
        .map_err(|e| format!("{e:?}"))?;
    write(path, &buffer.snapshot()).map_err(|e| e.to_string())
}

/// The HEAD file of the repository `root` is in: `.git/HEAD` in it or a folder above it, or
/// where a worktree's `.git` file points.
fn head_file(root: &Path) -> Option<std::path::PathBuf> {
    let git = root
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|git| git.exists())?;
    if git.is_dir() {
        return Some(git.join("HEAD"));
    }
    let pointer = std::fs::read_to_string(&git).ok()?;
    let dir = pointer.trim().strip_prefix("gitdir:")?.trim();
    Some(git.parent()?.join(dir).join("HEAD"))
}

/// The branch HEAD names, or the start of the commit it holds when none is checked out.
fn branch_in(head: &Path) -> Option<String> {
    let head = std::fs::read_to_string(head).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: refs/heads/") {
        Some(branch) => Some(branch.to_owned()),
        None => Some(head.get(..7)?.to_owned()),
    }
}

/// The monospaced families fontconfig knows, each by its first name, sorted.
fn monospaced() -> Vec<String> {
    let Ok(out) = quill_files::command("fc-list")
        // Faces for writing in, not monospaced emoji or symbol sets.
        .args([":spacing=100:lang=en", "family"])
        .output()
    else {
        return vec![];
    };
    let mut families: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.split(',').next())
        .map(|family| family.trim().to_owned())
        .filter(|family| !family.is_empty())
        .collect();
    families.sort();
    families.dedup();
    families
}

/// `family`'s regular and bold faces as fontconfig finds them; the system's monospace for
/// an empty one. A family without a bold takes its regular face for it.
fn faces(family: &str) -> Result<crate::ide::Faces, String> {
    let family = if family.is_empty() {
        "monospace"
    } else {
        family
    };
    let face = |pattern: String| -> Result<limn::Font, String> {
        let out = quill_files::command("fc-match")
            .args(["-f", "%{file}", &pattern])
            .output()
            .map_err(|e| e.to_string())?;
        let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
        limn::Font::new(bytes).map_err(|e| format!("{path}: {e:?}"))
    };
    let regular = face(family.to_owned())?;
    let bold = face(format!("{family}:bold")).unwrap_or_else(|_| regular.clone());
    Ok(crate::ide::Faces(regular, bold))
}

/// Settings whole or not at all, as a file is saved.
fn save_prefs(prefs: &crate::prefs::Prefs) -> io::Result<()> {
    let path = crate::prefs::path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension(format!("json.{}", std::process::id()));
    let json = serde_json::to_vec_pretty(prefs).map_err(io::Error::other)?;
    std::fs::write(&temporary, json)?;
    std::fs::rename(&temporary, &path)
}

/// Write `snapshot` over `path` whole or not at all: to a file beside it, then renamed over
/// it with its permissions. A link is followed to the file it names, and stays a link.
fn write(path: &Path, snapshot: &BufferSnapshot) -> io::Result<Stamp> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let (Some(dir), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(io::Error::other("not a file's path"));
    };
    let temporary = dir.join(format!(
        ".{}.quill-ide-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let written = (|| {
        let mut out = io::BufWriter::new(std::fs::File::create(&temporary)?);
        snapshot.write_to(&mut out)?;
        out.flush()?;
        out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        if let Ok(meta) = std::fs::metadata(&target) {
            std::fs::set_permissions(&temporary, meta.permissions())?;
        }
        std::fs::rename(&temporary, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written.map(|()| stamp(&target))
}

/// `dir`'s entries as git sees them: dotfiles shown, .gitignore'd ones and .git left out,
/// folders first, then by name.
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::ide::Save;

    #[test]
    fn the_branch_is_read_from_head_in_a_repository_and_in_a_worktree() {
        let dir = std::env::temp_dir().join(format!("quill-ide-head-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (main, linked) = (dir.join("main"), dir.join("linked"));
        std::fs::create_dir_all(main.join(".git/worktrees/linked")).unwrap();
        std::fs::create_dir_all(linked.join("src")).unwrap();
        std::fs::write(main.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let linked_head = main.join(".git/worktrees/linked/HEAD");
        std::fs::write(&linked_head, "0123456789abcdef\n").unwrap();
        let gitdir = format!("gitdir: {}\n", main.join(".git/worktrees/linked").display());
        std::fs::write(linked.join(".git"), gitdir).unwrap();
        let read = |root: &Path| head_file(root).as_deref().and_then(branch_in);
        let found = (read(&main.join(".git/..")), read(&linked.join("src")));
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found, (Some("main".into()), Some("0123456".into())));
    }

    #[test]
    fn a_folder_lists_as_git_sees_it() {
        let dir = std::env::temp_dir().join(format!("quill-ide-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["src", ".git", "target"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        for (f, body) in [(".gitignore", "target\n"), ("b.rs", ""), ("a.rs", "")] {
            std::fs::write(dir.join(f), body).unwrap();
        }
        let names: Vec<String> = list(&dir)
            .iter()
            .map(|e| e.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(names, ["src", ".gitignore", "a.rs", "b.rs"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_save_through_a_link_replaces_the_file_whole_and_keeps_its_mode_and_the_link() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = std::env::temp_dir().join(format!("quill-ide-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (file, link) = (dir.join("run.sh"), dir.join("link.sh"));
        std::fs::write(&file, "old").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        symlink(&file, &link).unwrap();
        let known = stamp(&file);
        let snapshot = limn_editor::TextBuffer::new("new").unwrap().snapshot();
        let Loaded::Written(_, _, Ok(written)) = run(Job::Write(link.clone(), Save(snapshot)))
        else {
            panic!("the save failed");
        };
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        let is_link = std::fs::symlink_metadata(&link).unwrap().is_symlink();
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        let Loaded::Changed(changed) = run(Job::Stat(vec![(file.clone(), known)])) else {
            panic!("a stat finds what changed");
        };
        let content = std::fs::read_to_string(&file).unwrap();
        let now = stamp(&file);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            (content.as_str(), mode, is_link, left.len()),
            ("new", 0o755, true, 2)
        );
        assert_eq!(written, now);
        assert_eq!(changed.len(), usize::from(known != written));
    }
}
