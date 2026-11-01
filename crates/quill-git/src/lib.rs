//! A shadow git repository per folder, outside the folder (spec.md D4): a snapshot of the
//! work tree before and after each request tells what that request created, changed,
//! removed or moved, whoever did it, and puts files back for a rewind. The project's own
//! `.git` is never touched. Built on the `git` binary (spec.md D9); calls block.
//!
//! shortcut: every snapshot runs `git add -A` over the whole folder; for very large
//! folders switch to snapshots per tool from the agent's hooks (engine.md §6).

use quill_model::{ChangeKind, FileEffect};
use std::io;
use std::path::{Path, PathBuf};

/// A patch longer than this is cut, so one generated file cannot flood the transcript.
const PATCH_LIMIT: usize = 200_000;

pub struct Shadow {
    git_dir: PathBuf,
    work: PathBuf,
}

impl Shadow {
    /// The shadow of `work`, kept under `root` (created on first use).
    pub fn open(root: &Path, work: &Path) -> io::Result<Self> {
        let work = work.canonicalize()?;
        let name: String = work
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let git_dir = root.join(format!("{}.git", name.trim_matches('-')));
        let shadow = Self { git_dir, work };
        if !shadow.git_dir.join("HEAD").exists() {
            std::fs::create_dir_all(&shadow.git_dir)?;
            // `init --bare` takes no work tree; the shadow is told its folder on each command.
            let status = quill_files::command("git")
                .args(["init", "-q", "--bare"])
                .arg(&shadow.git_dir)
                .status()?;
            if !status.success() {
                return Err(io::Error::other(
                    "git init failed for the shadow repository",
                ));
            }
        }
        Ok(shadow)
    }

    fn git(&self, args: &[&str]) -> io::Result<String> {
        let output = quill_files::command("git")
            .env("GIT_DIR", &self.git_dir)
            .env("GIT_WORK_TREE", &self.work)
            .env("GIT_INDEX_FILE", self.git_dir.join("index"))
            .args(["-c", "core.quotePath=false", "-c", "core.autocrlf=false"])
            .args(args)
            .current_dir(&self.work)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// The folder as it is now, as a tree id.
    pub fn snapshot(&self) -> io::Result<String> {
        self.git(&["add", "-A", "--", "."])?;
        Ok(self.git(&["write-tree"])?.trim().to_owned())
    }

    /// What changed from tree `from` to tree `to`, file by file, with each file's patch.
    pub fn changes(&self, from: &str, to: &str) -> io::Result<Vec<FileEffect>> {
        let diff = self.git(&["diff", "-M", "--no-color", "--raw", "-p", from, to])?;
        effects(&diff, |paths| {
            let mut args = vec!["diff", "-M", "--no-color", from, to, "--"];
            args.extend(paths);
            self.git(&args)
        })
    }

    /// Put the folder's files back as they were in tree `tree`: changed files return, files
    /// created since are removed. Ignored files are left alone.
    pub fn restore(&self, tree: &str) -> io::Result<()> {
        self.snapshot()?;
        self.git(&["read-tree", "-u", "--reset", tree])?;
        Ok(())
    }
}

/// The files a `git diff --raw -p` names, with their patches: its raw lines
/// (":<modes> <ids> <status>\t<paths>") list them, and its patch is cut at each file's
/// "diff --git" line, so a thousand files cost one git process. `patch` writes the patch of
/// the paths it is given, asked file by file only when the cut does not give one part per
/// listed file (as with an external diff tool).
fn effects(
    diff: &str,
    patch: impl Fn(&[&str]) -> io::Result<String>,
) -> io::Result<Vec<FileEffect>> {
    let mut listed = vec![];
    for line in diff.lines().map_while(|line| line.strip_prefix(':')) {
        let Some(line) = line.splitn(5, ' ').nth(4) else {
            continue;
        };
        let parts: Vec<&str> = line.split('\t').collect();
        listed.push(match parts.as_slice() {
            [status, path] if status.starts_with('A') => (ChangeKind::Added, vec![*path]),
            [status, path] if status.starts_with('D') => (ChangeKind::Deleted, vec![*path]),
            [status, from_path, path] if status.starts_with('R') => (
                ChangeKind::Renamed {
                    from: from_path.to_string(),
                },
                vec![*from_path, *path],
            ),
            [_, path] => (ChangeKind::Modified, vec![*path]),
            _ => continue,
        });
    }
    // A blank line parts the raw lines from the patch.
    let all = diff.split_once("\n\n").map_or("", |(_, all)| all);
    let mut parts = split(all, listed.len()).map(Vec::into_iter);
    let mut files = vec![];
    for (kind, paths) in listed {
        let mut patch = match parts.as_mut() {
            Some(parts) => parts.next().unwrap_or_default(),
            None => patch(&paths)?,
        };
        let (added, removed) = counts(&patch);
        cut(&mut patch);
        files.push(FileEffect {
            path: paths.last().unwrap().to_string(),
            kind,
            added,
            removed,
            patch,
        });
    }
    Ok(files)
}

/// The patch `all` of `count` files cut into each file's own, in the order git lists them:
/// a part begins at a "diff --git" line, but a type change writes its deletion and its
/// creation under the same one. `None` when that does not give `count` parts.
fn split(all: &str, count: usize) -> Option<Vec<String>> {
    let mut parts: Vec<String> = vec![];
    let mut header = "";
    for line in all.split_inclusive('\n') {
        if line.starts_with("diff --git ") && line != header {
            header = line;
            parts.push(String::new());
        }
        parts.last_mut()?.push_str(line);
    }
    (parts.len() == count).then_some(parts)
}

/// Cut a patch past [`PATCH_LIMIT`], saying so.
fn cut(patch: &mut String) {
    if patch.len() > PATCH_LIMIT {
        let cut = patch
            .char_indices()
            .nth(PATCH_LIMIT)
            .map_or(patch.len(), |(i, _)| i);
        patch.truncate(cut);
        patch.push_str("\n… (cut)\n");
    }
}

/// What is uncommitted in `dir`'s own repository, as Desktop's diff shows "master → working
/// tree": tracked files against HEAD, each with its patch, then the files git neither tracks
/// nor ignores, as added ones ([`untracked`]). Paths are from the repository's top, whichever
/// of its folders `dir` is.
pub fn working_changes(dir: &Path) -> io::Result<Vec<FileEffect>> {
    let run = |dir: &Path, args: &[&str]| -> io::Result<String> {
        let output = quill_files::command("git")
            .args(["-c", "core.quotePath=false"])
            .args(args)
            .current_dir(dir)
            .output()?;
        // `diff --no-index` answers 1 when the files differ.
        if !output.status.success() && output.status.code() != Some(1) {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let top = run(dir, &["rev-parse", "--show-toplevel"])?;
    let top = Path::new(top.trim_end_matches(['\n', '\r']));
    let git = |args: &[&str]| run(top, args);
    let tracked = git(&["diff", "HEAD", "-M", "--no-color", "--raw", "-p"])?;
    let mut files = effects(&tracked, |paths| {
        let mut args = vec!["diff", "HEAD", "-M", "--no-color", "--"];
        args.extend(paths);
        git(&args)
    })?;
    let others = git(&["ls-files", "--others", "--exclude-standard", "-z"])?;
    // A file taken out of the index but kept is the tracked diff's already.
    let known: std::collections::HashSet<String> = files.iter().map(|f| f.path.clone()).collect();
    let others = others
        .split('\0')
        .filter(|path| !path.is_empty() && !path.ends_with('/') && !known.contains(*path));
    files.extend(untracked(top, others));
    Ok(files)
}

/// How many untracked files are read, how many more are listed unread, and how large and how
/// long one is read in full: Desktop's limits, as its diff shows them (inferred).
const UNTRACKED_READ: usize = 200;
const UNTRACKED_LISTED: usize = 400;
const UNTRACKED_SIZE: u64 = 1 << 20;
const UNTRACKED_LINES: u32 = 2000;

/// Untracked files under `top` as added ones, in the patch git writes for a new file. The
/// first [`UNTRACKED_READ`] are read: one over [`UNTRACKED_SIZE`] has no lines, and one with
/// a NUL byte is a binary file, with git's line for one; a text file of up to
/// [`UNTRACKED_LINES`] lines has all of them, and a longer one its count and no hunk. The
/// next ones are listed unread, with no lines, up to [`UNTRACKED_LISTED`]; any more are left
/// out.
fn untracked<'a>(top: &Path, paths: impl Iterator<Item = &'a str>) -> Vec<FileEffect> {
    let file = |at: usize, path: &str| {
        let full = top.join(path);
        let meta = std::fs::metadata(&full).ok();
        let mode = if meta.as_ref().is_some_and(executable) {
            "100755"
        } else {
            "100644"
        };
        let mut patch = format!("diff --git a/{path} b/{path}\nnew file mode {mode}\n");
        let bytes = meta
            .filter(|meta| at < UNTRACKED_READ && meta.len() <= UNTRACKED_SIZE)
            .and_then(|_| std::fs::read(&full).ok())
            .unwrap_or_default();
        let mut added = 0;
        if bytes.contains(&0) {
            patch.push_str(&format!("Binary files /dev/null and b/{path} differ\n"));
        } else if !bytes.is_empty() {
            let text = String::from_utf8_lossy(&bytes);
            added = text.lines().count() as u32;
            if added <= UNTRACKED_LINES {
                // As git writes a hunk of one line: "+1", not "+1,1".
                let lines = if added == 1 {
                    String::new()
                } else {
                    format!(",{added}")
                };
                patch.push_str(&format!(
                    "--- /dev/null\n+++ b/{path}\n@@ -0,0 +1{lines} @@\n"
                ));
                for line in text.split_inclusive('\n') {
                    patch.push('+');
                    patch.push_str(line);
                }
                if !text.ends_with('\n') {
                    patch.push_str("\n\\ No newline at end of file\n");
                }
                cut(&mut patch);
            }
        }
        FileEffect {
            path: path.to_owned(),
            kind: ChangeKind::Added,
            added,
            removed: 0,
            patch,
        }
    };
    paths
        .take(UNTRACKED_LISTED)
        .enumerate()
        .map(|(at, path)| file(at, path))
        .collect()
}

/// Whether git keeps the file as executable (mode 100755).
fn executable(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o100 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// Lines added and removed in a unified diff: those of its hunks, not the "---" and "+++"
/// lines a file's part begins with, which a removed "-- note" or an added "++i" looks like.
fn counts(patch: &str) -> (u32, u32) {
    let mut hunk = false;
    patch.lines().fold((0, 0), |(a, r), line| {
        if line.starts_with("diff --git ") {
            hunk = false;
        } else if line.starts_with("@@") {
            hunk = true;
        }
        match line.as_bytes().first() {
            Some(b'+') if hunk => (a + 1, r),
            Some(b'-') if hunk => (a, r + 1),
            _ => (a, r),
        }
    })
}

/// The repository `dir` is in, if it is in one: its branch, `origin` and the branch
/// `origin` names as its own.
pub fn repo(dir: &Path) -> Option<quill_model::Repo> {
    let git = |args: &[&str]| {
        let output = quill_files::command("git")
            .args(args)
            .current_dir(dir)
            .output()
            .ok()?;
        let out = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (output.status.success() && !out.is_empty()).then_some(out)
    };
    Some(quill_model::Repo {
        branch: branch(dir)?,
        remote: git(&["remote", "get-url", "origin"]),
        base: git(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
            .map(|head| head.trim_start_matches("origin/").to_owned()),
    })
}

/// The branch checked out in `dir`'s own repository, if it is one.
pub fn branch(dir: &Path) -> Option<String> {
    let output = quill_files::command("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !name.is_empty()).then_some(name)
}

/// The repository `dir` belongs to, the same for each of its worktrees: its common git
/// folder, absolute.
pub fn common_dir(dir: &Path) -> Option<PathBuf> {
    let output = quill_files::command("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(dir)
        .output()
        .ok()?;
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !path.is_empty()).then(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn folder(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("quill-git-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let (root, work) = (base.join("shadow"), base.join("work"));
        fs::create_dir_all(&work).unwrap();
        (root, work)
    }

    #[test]
    fn a_request_made_with_a_shell_is_attributed_and_rewound() {
        let (root, work) = folder("attribute");
        fs::write(work.join("keep.txt"), "one\n").unwrap();
        fs::write(work.join("notes.txt"), "hello\n").unwrap();
        fs::write(work.join("old.txt"), "same\n").unwrap();
        let shadow = Shadow::open(&root, &work).unwrap();
        let before = shadow.snapshot().unwrap();

        // What an agent might do with Edit and Bash: change, create, remove, move.
        fs::write(work.join("keep.txt"), "one\ntwo\n").unwrap();
        fs::write(work.join("new.txt"), "fresh\n").unwrap();
        fs::remove_file(work.join("notes.txt")).unwrap();
        fs::rename(work.join("old.txt"), work.join("moved.txt")).unwrap();
        let after = shadow.snapshot().unwrap();

        let files = shadow.changes(&before, &after).unwrap();
        let kind = |p: &str| files.iter().find(|f| f.path == p).map(|f| f.kind.clone());
        assert_eq!(kind("keep.txt"), Some(ChangeKind::Modified));
        assert_eq!(kind("new.txt"), Some(ChangeKind::Added));
        assert_eq!(kind("notes.txt"), Some(ChangeKind::Deleted));
        assert_eq!(
            kind("moved.txt"),
            Some(ChangeKind::Renamed {
                from: "old.txt".into()
            })
        );
        let keep = files.iter().find(|f| f.path == "keep.txt").unwrap();
        assert_eq!((keep.added, keep.removed), (1, 0));
        assert!(keep.patch.contains("+two"));

        shadow.restore(&before).unwrap();
        assert_eq!(fs::read_to_string(work.join("keep.txt")).unwrap(), "one\n");
        assert_eq!(
            fs::read_to_string(work.join("notes.txt")).unwrap(),
            "hello\n"
        );
        assert!(
            work.join("old.txt").exists()
                && !work.join("moved.txt").exists()
                && !work.join("new.txt").exists()
        );
    }

    #[test]
    fn the_projects_own_git_and_ignored_files_are_left_alone() {
        let (root, work) = folder("ignore");
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&work)
            .status()
            .unwrap();
        fs::write(work.join(".gitignore"), "secret.env\n").unwrap();
        fs::write(work.join("secret.env"), "TOKEN=1\n").unwrap();
        let shadow = Shadow::open(&root, &work).unwrap();
        let before = shadow.snapshot().unwrap();
        fs::write(work.join("secret.env"), "TOKEN=2\n").unwrap();
        let after = shadow.snapshot().unwrap();
        assert!(shadow.changes(&before, &after).unwrap().is_empty());
        assert!(work.join(".git/HEAD").exists());
    }

    #[test]
    fn the_working_tree_is_what_is_uncommitted_new_files_too() {
        let (_, work) = folder("working");
        let git = |args: &[&str]| {
            assert!(Command::new("git")
                .args(args)
                .current_dir(&work)
                .output()
                .unwrap()
                .status
                .success());
        };
        git(&["init", "-q"]);
        fs::write(work.join("a.py"), "print(1)\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=q",
            "-c",
            "user.email=q@q",
            "commit",
            "-qm",
            "a",
        ]);
        fs::write(work.join("a.py"), "print(2)\n").unwrap();
        fs::write(work.join("new.txt"), "hi\n").unwrap();
        let files = working_changes(&work).unwrap();
        let summary: Vec<_> = files
            .iter()
            .map(|f| (f.path.as_str(), f.kind.clone(), f.added, f.removed))
            .collect();
        assert_eq!(
            summary,
            [
                ("a.py", ChangeKind::Modified, 1, 1),
                ("new.txt", ChangeKind::Added, 1, 0)
            ]
        );
    }

    #[test]
    fn one_diff_cut_per_file_reads_as_a_diff_per_file() {
        let (root, work) = folder("cut");
        let git = |args: &[&str]| -> String {
            let output = Command::new("git")
                .args(["-c", "core.quotePath=false"])
                .args(["-c", "user.name=q", "-c", "user.email=q@q"])
                .args(args)
                .current_dir(&work)
                .output()
                .unwrap();
            assert!(output.status.success(), "{args:?}: {output:?}");
            String::from_utf8_lossy(&output.stdout).into_owned()
        };
        git(&["init", "-q"]);
        fs::create_dir_all(work.join("sub")).unwrap();
        let mut names: Vec<String> = (0..40).map(|i| format!("f{i:02}.txt")).collect();
        names.extend(
            [
                "keep.txt",
                "gone.txt",
                "old.txt",
                "mode.sh",
                "link",
                "with space.txt",
                "ñandú.txt",
                "sub/b.txt",
                "sub-c.txt",
                "sub.d.txt",
            ]
            .map(String::from),
        );
        for name in &names {
            fs::write(work.join(name), format!("{name}\nsame\n")).unwrap();
        }
        fs::write(work.join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        let shadow = Shadow::open(&root, &work).unwrap();
        let before = shadow.snapshot().unwrap();

        // What an agent leaves: edits, a file made and added, one removed, one moved, a binary
        // changed, a mode and a type changed.
        let edited = [
            "keep.txt",
            "with space.txt",
            "ñandú.txt",
            "sub/b.txt",
            "sub-c.txt",
        ];
        for name in names
            .iter()
            .take(40)
            .step_by(3)
            .chain(&edited.map(String::from))
        {
            fs::write(work.join(name), format!("{name}\nmore\nlines\n")).unwrap();
        }
        fs::write(work.join("new.txt"), "fresh\n").unwrap();
        git(&["add", "new.txt"]);
        fs::remove_file(work.join("gone.txt")).unwrap();
        git(&["mv", "old.txt", "moved.txt"]);
        fs::write(work.join("bin.dat"), [0u8, 9, 9, 9]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(work.join("mode.sh"), fs::Permissions::from_mode(0o755)).unwrap();
            fs::remove_file(work.join("link")).unwrap();
            std::os::unix::fs::symlink("keep.txt", work.join("link")).unwrap();
        }
        let after = shadow.snapshot().unwrap();

        // As they were read before: the list, then a `git diff` per file.
        let cut = working_changes(&work).unwrap();
        let one_by_one = effects(&git(&["diff", "HEAD", "-M", "--raw"]), |paths| {
            let mut args = vec!["diff", "HEAD", "-M", "--no-color", "--"];
            args.extend(paths);
            Ok(git(&args))
        })
        .unwrap();
        assert_eq!(cut, one_by_one);
        let changes = shadow.changes(&before, &after).unwrap();
        let listing = shadow.git(&["diff-tree", "-r", "-M", "--raw", &before, &after]);
        let one_by_one = effects(&listing.unwrap(), |paths| {
            let mut args = vec!["diff", "-M", "--no-color", &before, &after, "--"];
            args.extend(paths);
            shadow.git(&args)
        })
        .unwrap();
        assert_eq!(changes, one_by_one);

        let file = |path: &str| cut.iter().find(|f| f.path == path).unwrap();
        let listed: Vec<&str> = cut.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(listed.len(), if cfg!(unix) { 25 } else { 23 }, "{listed:?}");
        assert_eq!((file("f00.txt").added, file("f00.txt").removed), (2, 1));
        assert_eq!((file("ñandú.txt").added, file("ñandú.txt").removed), (2, 1));
        assert_eq!(file("new.txt").kind, ChangeKind::Added);
        assert_eq!(file("gone.txt").kind, ChangeKind::Deleted);
        assert_eq!(
            file("moved.txt").kind,
            ChangeKind::Renamed {
                from: "old.txt".into()
            }
        );
        assert!(file("bin.dat").patch.contains("Binary files"));
        #[cfg(unix)]
        assert!(file("mode.sh").patch.contains("new mode 100755"));
        // A session in a folder of the repository reads all of it, as from its top.
        assert_eq!(working_changes(&work.join("sub")).unwrap(), cut);
    }

    /// `git` with a name to commit as, in `work`; what it printed.
    fn run(work: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(["-c", "core.quotePath=false"])
            .args(["-c", "user.name=q", "-c", "user.email=q@q"])
            .args(args)
            .current_dir(work)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    #[test]
    fn new_files_read_as_git_writes_them_once_added() {
        let (_, work) = folder("untracked");
        let git = |args: &[&str]| run(&work, args);
        git(&["init", "-q"]);
        fs::write(work.join("kept.txt"), "kept\n").unwrap();
        fs::write(work.join(".gitignore"), "*.log\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        // What an agent leaves beside its edits: text (a line that reads as a header among
        // them), a line with no end, a script, a picture, an empty file, one over the size
        // read and one past the lines shown; a log the folder ignores and a repository of
        // its own; and a file taken out of the index, which the tracked diff has already.
        fs::write(work.join("notes.txt"), "one\ntwo\n++i;\n").unwrap();
        fs::write(work.join("one.txt"), "only").unwrap();
        fs::write(work.join("run.sh"), "#!/bin/sh\necho hi\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(work.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        fs::write(work.join("pic.bin"), [1u8, 0, 2]).unwrap();
        fs::write(work.join("empty.txt"), "").unwrap();
        fs::write(work.join("huge.txt"), "x\n".repeat(600_000)).unwrap();
        fs::write(work.join("long.txt"), "line\n".repeat(2_500)).unwrap();
        fs::write(work.join("debug.log"), "ignored\n").unwrap();
        fs::create_dir_all(work.join("vendor")).unwrap();
        run(&work.join("vendor"), &["init", "-q"]);
        fs::write(work.join("vendor/lib.txt"), "theirs\n").unwrap();
        git(&["rm", "-q", "--cached", "kept.txt"]);

        let files = working_changes(&work).unwrap();
        let summary: Vec<_> = files
            .iter()
            .map(|f| (f.path.as_str(), f.kind.clone(), f.added, f.removed))
            .collect();
        let added = ChangeKind::Added;
        assert_eq!(
            summary,
            [
                ("kept.txt", ChangeKind::Deleted, 0, 1),
                ("empty.txt", added.clone(), 0, 0),
                ("huge.txt", added.clone(), 0, 0),
                ("long.txt", added.clone(), 2_500, 0),
                ("notes.txt", added.clone(), 3, 0),
                ("one.txt", added.clone(), 1, 0),
                ("pic.bin", added.clone(), 0, 0),
                ("run.sh", added, 2, 0),
            ]
        );
        let file = |path: &str| files.iter().find(|f| f.path == path).unwrap().clone();
        // Past the lines shown, and over the size read, a file has no hunk.
        assert!(!file("long.txt").patch.contains("@@"));
        assert_eq!(
            file("huge.txt").patch,
            "diff --git a/huge.txt b/huge.txt\nnew file mode 100644\n"
        );
        // Once added, git writes the others as they were read, its blob ids aside.
        let read = ["notes.txt", "one.txt", "run.sh", "pic.bin", "empty.txt"];
        git(&[&["add", "-N"][..], &read[..]].concat());
        let added = working_changes(&work).unwrap();
        for path in read {
            let mut by_git = added.iter().find(|f| f.path == path).unwrap().clone();
            by_git.patch = (by_git.patch.lines())
                .filter(|line| !line.starts_with("index "))
                .map(|line| format!("{line}\n"))
                .collect();
            assert_eq!(file(path), by_git);
        }
    }

    #[test]
    fn new_files_past_two_hundred_are_listed_unread_and_past_four_hundred_left_out() {
        let (_, work) = folder("many");
        let git = |args: &[&str]| run(&work, args);
        git(&["init", "-q"]);
        fs::write(work.join("kept.txt"), "kept\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-qm", "base"]);
        for n in 0..450 {
            fs::write(work.join(format!("n{n:03}.txt")), "line\n").unwrap();
        }
        let files = working_changes(&work).unwrap();
        assert_eq!(files.len(), 400);
        assert!(files[..200].iter().all(|f| (f.added, f.removed) == (1, 0)));
        assert!(files[200..]
            .iter()
            .all(|f| (f.added, f.removed) == (0, 0) && !f.patch.contains("@@")));
        assert_eq!(files[399].path, "n399.txt");
    }
}
