//! Newen's first start on a computer quill used (docs/UPDATES.md, «Venir de quill»): quill's
//! folders and database take Newen's names, the paths its record keeps follow them, and what is
//! derived from the old place goes. Each step is done once and can be done again; one that
//! fails leaves quill's names in use for this start ([`newen_files::own_dir`]) and is tried
//! again at the next. The database is renamed last: until then it says quill's data is here
//! ([`newen_files::quill_was_here`]), on Windows too, where no link is left in quill's place.
//! Legacy names are read only here and in the resolvers that fall back to them.

use std::path::{Path, PathBuf};

/// Left in Newen's data folder once every step is done: later starts do nothing.
pub const MARKER: &str = ".migrated-from-quill";

/// Newen's icon, which the launcher put in quill's place names.
const ICON_PNG: &[u8] = include_bytes!("../../../apps/newen/assets/icons/newen-256.png");
const ICON_SVG: &[u8] = include_bytes!("../../../apps/newen/assets/icons/newen.svg");

/// Whether this build may move quill's data: a release always; a debug build only when
/// `NEWEN_MIGRATE` is `1` (`asked`), as a developer's build shares the person's own folders.
pub fn allowed(debug: bool, asked: Option<&std::ffi::OsStr>) -> bool {
    !debug || asked.is_some_and(|asked| asked == "1")
}

/// Move what quill kept under the data folder `data` and the settings folder `config` (the
/// bases, [`newen_files::data_dir`] and [`newen_files::config_dir`]) to Newen's names: what
/// happened, a line a step, for the update log. Nothing when there is nothing of quill's (a
/// folder only named quill is another app's), or when it was all done before.
pub fn from_quill(data: &Path, config: &Path) -> Vec<String> {
    let newen = data.join("newen");
    let quill = data.join("quill");
    if newen.join(MARKER).exists() || !newen_files::quill_was_here(data) {
        return vec![];
    }
    let moved = |base: &Path| newen_files::move_legacy(base, "newen", "quill");
    let mut steps = vec![
        ("data folder", moved(data).map_err(|e| e.to_string())),
        ("settings folder", moved(config).map_err(|e| e.to_string())),
    ];
    // The rest is in Newen's folders: while one is quill's, its steps wait for the next start.
    let data_moved = newen_files::own_dir(data, data) == newen;
    if data_moved {
        steps.push(("pictures in the record", rewrite_record(&newen, &quill)));
        steps.push((
            "what's new",
            rewrite_notes(&newen.join("whats-new"), &quill),
        ));
        steps.push((
            "launcher",
            replace_launcher(
                &data.join("applications"),
                &quill.join("app"),
                &newen_update::install::Install::at(newen.join("app")).program(),
            ),
        ));
    }
    let config_moved = newen_files::own_dir(config, data) == config.join("newen");
    if config_moved {
        // Derived, made again for each session: its links point into quill's folder.
        let removed = match std::fs::remove_dir_all(config.join("newen/injected")) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.to_string()),
        };
        steps.push(("injected skills", removed));
    }
    let mut done = data_moved && config_moved && steps.iter().all(|(_, result)| result.is_ok());
    // Last, once the rest is done: until it is renamed, the next start takes up what is left.
    if done {
        let renamed = rename_database(&newen);
        done = renamed.is_ok();
        steps.push(("database", renamed));
    }
    let mut said: Vec<String> = (steps.into_iter())
        .filter_map(|(what, result)| match result {
            Ok(true) => Some(format!("{what}: done")),
            Ok(false) => None,
            Err(error) => Some(format!("{what}: not done, quill's stays in use: {error}")),
        })
        .collect();
    if done {
        match std::fs::write(newen.join(MARKER), newen_update::VERSION) {
            Ok(()) => said.push("done".into()),
            Err(error) => said.push(format!("not marked done: {error}")),
        }
    }
    said
}

/// `quill.db` in `dir` renamed `newen.db`, its write-ahead log folded in first so the file alone
/// holds the record; a log a reader holds back leaves it as it is.
fn rename_database(dir: &Path) -> Result<bool, String> {
    let (quill, newen) = (dir.join("quill.db"), dir.join("newen.db"));
    if !quill.is_file() || newen.exists() {
        return Ok(false);
    }
    if !newen_store::checkpoint(&quill).map_err(|e| e.to_string())? {
        return Err("the database is in use".into());
    }
    let with = |path: &Path, suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    };
    for suffix in ["-wal", "-shm"] {
        // A stale one of an earlier try is no newer than the file, folded in.
        let _ = std::fs::remove_file(with(&newen, suffix));
        if with(&quill, suffix).exists() {
            std::fs::rename(with(&quill, suffix), with(&newen, suffix))
                .map_err(|e| e.to_string())?;
        }
    }
    // The file last: until it is there, `quill.db` is the one opened.
    std::fs::rename(&quill, &newen).map_err(|e| e.to_string())?;
    Ok(true)
}

/// `path` as it is written inside a JSON string: Windows' backslashes doubled.
fn in_json(path: &Path) -> String {
    let quoted = serde_json::to_string(&path.to_string_lossy()).unwrap_or_default();
    quoted.trim_matches('"').to_owned()
}

/// The pairs that take a path under `quill`'s folders `folders` to the same under `newen`'s,
/// as JSON writes them; the separator after each, so `attachments-old/` is not one.
fn moved_paths(quill: &Path, newen: &Path, folders: &[&str]) -> Vec<(String, String)> {
    let separator = in_json(Path::new(std::path::MAIN_SEPARATOR_STR));
    (folders.iter())
        .map(|folder| {
            let under = |root: &Path| in_json(&root.join(folder)) + &separator;
            (under(quill), under(newen))
        })
        .collect()
}

/// The record's pictures (pasted and annotated ones in `attachments/`, the agents' in
/// `pictures/`), kept as paths under quill's folder, written under Newen's. Only those: the
/// record also holds commands and outputs that name the folder, and they are what was said.
fn rewrite_record(newen: &Path, quill: &Path) -> Result<bool, String> {
    let database = [newen.join("newen.db"), newen.join("quill.db")]
        .into_iter()
        .find(|path| path.is_file());
    let Some(database) = database else {
        return Ok(false);
    };
    let pairs = moved_paths(quill, newen, &["attachments", "pictures"]);
    newen_store::rewrite(&database, &pairs)
        .map(|changed| changed > 0)
        .map_err(|e| e.to_string())
}

/// Each release's `notes.json` in `folder` (Newen's `whats-new/`), its media's paths under
/// quill's folder written under Newen's.
fn rewrite_notes(folder: &Path, quill: &Path) -> Result<bool, String> {
    let newen = folder.parent().unwrap_or(folder);
    let pairs = moved_paths(quill, newen, &["whats-new"]);
    let mut changed = false;
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let notes = entry.path().join("notes.json");
        let Ok(text) = std::fs::read_to_string(&notes) else {
            continue;
        };
        let rewritten =
            (pairs.iter()).fold(text.clone(), |text, (from, to)| text.replace(from, to));
        if rewritten != text {
            std::fs::write(&notes, rewritten).map_err(|e| e.to_string())?;
            changed = true;
        }
    }
    Ok(changed)
}

/// quill's launcher in the menu (`applications/quill.desktop`, Linux), when it starts quill's
/// installation `old`: `newen.desktop` in its place, starting `program` as Newen's window (its
/// app id, `newen`), and Newen's icon in the person's theme in place of quill's, as
/// scripts/install-desktop.sh puts them. One that starts something else (a wrapper made by
/// hand) is the person's and stays, as does any while `program` is not there to start.
fn replace_launcher(applications: &Path, old: &Path, program: &Path) -> Result<bool, String> {
    let quill = applications.join("quill.desktop");
    let Ok(text) = std::fs::read_to_string(&quill) else {
        return Ok(false);
    };
    let installed = format!("{}/", old.display());
    if !program.is_file()
        || !(text.lines()).any(|line| line.starts_with("Exec=") && line.contains(&installed))
    {
        return Ok(false);
    }
    let icons = applications.with_file_name("icons").join("hicolor");
    for (size, name, bytes) in [
        ("256x256", "newen.png", ICON_PNG),
        ("scalable", "newen.svg", ICON_SVG),
    ] {
        let folder = icons.join(size).join("apps");
        std::fs::create_dir_all(&folder)
            .and_then(|()| std::fs::write(folder.join(name), bytes))
            .map_err(|e| e.to_string())?;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Newen\nComment=Agents at work on your code\nIcon=newen\nExec=\"{}\" %U\nStartupWMClass=newen\nCategories=Development;\n",
        program.display()
    );
    std::fs::write(applications.join("newen.desktop"), entry).map_err(|e| e.to_string())?;
    std::fs::remove_file(&quill).map_err(|e| e.to_string())?;
    // quill's icons, which only its launcher named.
    for size in std::fs::read_dir(&icons).into_iter().flatten().flatten() {
        for name in ["quill.png", "quill.svg"] {
            let _ = std::fs::remove_file(size.path().join("apps").join(name));
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use newen_model::Event;

    fn queued(images: Vec<PathBuf>) -> Event {
        Event::Queued {
            session: 1,
            request: "r".into(),
            text: "look at ~/.local/share/quill/attachments/ too".into(),
            at: None,
            images,
        }
    }

    /// What quill 0.1.9 leaves: its data folder (the database with a pasted picture's path in
    /// its record, the picture, a release's notes naming its medium, the installation), its
    /// settings folder (the settings, a skill and its injected link), and a launcher with its
    /// icons.
    fn quill_s(data: &Path, config: &Path) {
        let quill = data.join("quill");
        std::fs::create_dir_all(quill.join("attachments")).unwrap();
        std::fs::write(quill.join("attachments/pasted.png"), "png").unwrap();
        let store = newen_store::Store::open(&quill.join("quill.db")).unwrap();
        store
            .append(1, &queued(vec![quill.join("attachments/pasted.png")]))
            .unwrap();
        drop(store);
        let notes = quill.join("whats-new/0.2.0");
        std::fs::create_dir_all(&notes).unwrap();
        let medium = in_json(&notes.join("ab.png"));
        std::fs::write(
            notes.join("notes.json"),
            format!(r#"{{"media":{{"Image":{{"path":"{medium}"}}}}}}"#),
        )
        .unwrap();
        let settings = config.join("quill");
        std::fs::create_dir_all(settings.join("skills/notes")).unwrap();
        std::fs::create_dir_all(settings.join("injected/claude/skills")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            settings.join("skills/notes"),
            settings.join("injected/claude/skills/notes"),
        )
        .unwrap();
        std::fs::write(settings.join("settings.json"), r#"{"open_at_login": true}"#).unwrap();
        // Its installation, `current` naming 0.1.9, which has only `quill`.
        let version = quill.join("app/versions/0.1.9");
        std::fs::create_dir_all(&version).unwrap();
        std::fs::write(version.join("quill"), "quill 0.1.9").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("versions/0.1.9", quill.join("app/current")).unwrap();
        // A launcher of it in the menu, and its icons in the person's theme.
        for icon in ["256x256/apps/quill.png", "scalable/apps/quill.svg"] {
            let icon = data.join("icons/hicolor").join(icon);
            std::fs::create_dir_all(icon.parent().unwrap()).unwrap();
            std::fs::write(icon, "quill's").unwrap();
        }
        std::fs::create_dir_all(data.join("applications")).unwrap();
        std::fs::write(
            data.join("applications/quill.desktop"),
            format!(
                "[Desktop Entry]\nName=quill\nExec=\"{}\" %U\n",
                quill.join("app/current/quill").display()
            ),
        )
        .unwrap();
    }

    /// Everything quill kept moves to Newen's names: the folders (links left in their place on
    /// Unix), the database renamed with its record whole and its pictures' paths following, the
    /// notes' media, the injected links gone; marked done, and nothing more at later starts.
    #[test]
    fn quill_s_data_moves_to_newen_s_names_once() {
        let base = tempfile::tempdir().unwrap();
        let (data, config) = (base.path().join("data"), base.path().join("config"));
        assert!(from_quill(&data, &config).is_empty(), "nothing of quill's");
        quill_s(&data, &config);
        let said = from_quill(&data, &config);
        assert_eq!(said.last().map(String::as_str), Some("done"), "{said:?}");
        let newen = data.join("newen");
        assert!(newen.join(MARKER).is_file());
        assert!(newen.join("newen.db").is_file() && !newen.join("quill.db").exists());
        assert!(!newen.join("quill.db-wal").exists());
        let store = newen_store::Store::open(&newen.join("newen.db")).unwrap();
        // The picture's path follows; what the person wrote stays as written.
        assert_eq!(
            store.load_session(1).unwrap(),
            [queued(vec![newen.join("attachments/pasted.png")])]
        );
        drop(store);
        let notes = std::fs::read_to_string(newen.join("whats-new/0.2.0/notes.json")).unwrap();
        assert!(notes.contains(&in_json(&newen.join("whats-new/0.2.0/ab.png"))));
        assert!(!notes.contains(&in_json(&data.join("quill/whats-new"))));
        assert!(config.join("newen/settings.json").is_file());
        assert!(config.join("newen/skills/notes").is_dir());
        // The launcher starts the installation's program as it is: quill's 0.1.9 until an
        // update brings Newen, whose versions all have a `quill` link to `newen`.
        #[cfg(target_os = "linux")]
        {
            let launcher =
                std::fs::read_to_string(data.join("applications/newen.desktop")).unwrap();
            let program = newen.join("app/current/quill");
            assert!(program.is_file());
            assert!(launcher.contains(&format!("Exec=\"{}\" %U", program.display())));
            assert!(launcher.contains("Icon=newen") && launcher.contains("StartupWMClass=newen"));
            assert!(!data.join("applications/quill.desktop").exists());
            let icons = data.join("icons/hicolor");
            assert_eq!(
                std::fs::read(icons.join("256x256/apps/newen.png")).unwrap(),
                ICON_PNG
            );
            assert!(icons.join("scalable/apps/newen.svg").is_file());
            assert!(!icons.join("256x256/apps/quill.png").exists());
            assert!(!icons.join("scalable/apps/quill.svg").exists());
        }
        assert!(!config.join("newen/injected").exists());
        #[cfg(unix)]
        for base in [&data, &config] {
            assert_eq!(
                std::fs::read_link(base.join("quill")).unwrap(),
                Path::new("newen")
            );
        }
        // What starts reads Newen's.
        let paths = crate::Config::under(&data, &config);
        assert_eq!(paths.database, Some(newen.join("newen.db")));
        assert_eq!(paths.settings, Some(config.join("newen/settings.json")));
        // Done: a later start does nothing, even with a folder of quill's back.
        #[cfg(unix)]
        std::fs::remove_file(data.join("quill")).unwrap();
        std::fs::create_dir_all(data.join("quill")).unwrap();
        assert!(from_quill(&data, &config).is_empty());
        assert!(data.join("quill").is_dir());
    }

    /// A folder that cannot be moved leaves quill's in use for this start, nothing marked done
    /// and nothing empty made in Newen's name; the next start that can, moves it.
    #[cfg(unix)]
    #[test]
    fn a_move_that_fails_leaves_quill_s_in_use_and_is_tried_again() {
        use std::os::unix::fs::PermissionsExt;
        let base = tempfile::tempdir().unwrap();
        let (data, config) = (base.path().join("data"), base.path().join("config"));
        quill_s(&data, &config);
        let locked =
            |mode| std::fs::set_permissions(&data, std::fs::Permissions::from_mode(mode)).unwrap();
        locked(0o555);
        let said = from_quill(&data, &config);
        locked(0o755);
        if std::fs::metadata(data.join("newen")).is_ok() {
            // Root ignores the folder's permissions: nothing to check here.
            return;
        }
        assert!(said[0].starts_with("data folder: not done"), "{said:?}");
        assert!(!said.contains(&"done".to_owned()));
        assert!(data.join("quill/quill.db").is_file());
        let paths = crate::Config::under(&data, &config);
        assert_eq!(paths.database, Some(data.join("quill/quill.db")));
        // The settings folder could move; quill's data stays read where it is.
        assert_eq!(paths.settings, Some(config.join("newen/settings.json")));
        let said = from_quill(&data, &config);
        assert_eq!(said.last().map(String::as_str), Some("done"), "{said:?}");
        assert!(data.join("newen/newen.db").is_file());
    }

    /// A database that cannot be read (here, not one at all) is not renamed: it stays
    /// `quill.db` in Newen's folder, read there; nothing is marked done, so the next start
    /// tries again.
    #[test]
    fn a_database_not_renamed_is_read_where_it_is() {
        let base = tempfile::tempdir().unwrap();
        let (data, config) = (base.path().join("data"), base.path().join("config"));
        quill_s(&data, &config);
        std::fs::write(data.join("quill/quill.db"), "not a database").unwrap();
        let undone = |said: &[String]| {
            said.iter()
                .any(|line| line.starts_with("pictures in the record: not done"))
        };
        let said = from_quill(&data, &config);
        assert!(undone(&said), "{said:?}");
        assert!(!data.join("newen").join(MARKER).exists());
        let paths = crate::Config::under(&data, &config);
        assert_eq!(paths.database, Some(data.join("newen/quill.db")));
        assert!(undone(&from_quill(&data, &config)));
    }

    /// A step left undone after the folders moved is taken up at the next start even where no
    /// link is left in quill's place (Windows): the database, renamed last, still says quill's
    /// data is here.
    #[test]
    fn a_step_left_undone_is_taken_up_without_quill_s_links() {
        let base = tempfile::tempdir().unwrap();
        let (data, config) = (base.path().join("data"), base.path().join("config"));
        quill_s(&data, &config);
        // A file where `injected/` was cannot be removed as a folder.
        std::fs::remove_dir_all(config.join("quill/injected")).unwrap();
        std::fs::write(config.join("quill/injected"), "").unwrap();
        let said = from_quill(&data, &config);
        assert!(
            said.iter()
                .any(|line| line.starts_with("injected skills: not done")),
            "{said:?}"
        );
        assert!(data.join("newen/quill.db").is_file(), "not renamed yet");
        #[cfg(unix)]
        for base in [&data, &config] {
            std::fs::remove_file(base.join("quill")).unwrap();
        }
        std::fs::remove_file(config.join("newen/injected")).unwrap();
        let said = from_quill(&data, &config);
        assert_eq!(said.last().map(String::as_str), Some("done"), "{said:?}");
        let newen = data.join("newen");
        let store = newen_store::Store::open(&newen.join("newen.db")).unwrap();
        assert_eq!(
            store.load_session(1).unwrap(),
            [queued(vec![newen.join("attachments/pasted.png")])]
        );
    }

    /// Another app's folders named quill (Windows and macOS find a `Quill` by that name), with
    /// no database of quill's, are not quill's: nothing moves, and Newen's own folders are its
    /// own.
    #[test]
    fn another_app_s_quill_folder_is_left_alone() {
        let base = tempfile::tempdir().unwrap();
        let (data, config) = (base.path().join("data"), base.path().join("config"));
        for base in [&data, &config] {
            std::fs::create_dir_all(base.join("quill")).unwrap();
            std::fs::write(base.join("quill/settings.json"), "{}").unwrap();
        }
        assert!(from_quill(&data, &config).is_empty());
        assert!(!data.join("newen").exists() && !config.join("newen").exists());
        assert!(config.join("quill/settings.json").is_file());
        let paths = crate::Config::under(&data, &config);
        assert_eq!(paths.database, Some(data.join("newen/newen.db")));
        assert_eq!(paths.settings, Some(config.join("newen/settings.json")));
    }

    /// A launcher that starts something else than quill's installation (a wrapper) stays, and
    /// so does one while the installation has no program to start; one that starts the
    /// installation starts Newen's program from then on.
    #[test]
    fn a_launcher_made_by_hand_or_with_nothing_to_start_stays() {
        let folder = tempfile::tempdir().unwrap();
        let applications = folder.path().join("applications");
        std::fs::create_dir_all(&applications).unwrap();
        let quill = applications.join("quill.desktop");
        std::fs::write(
            &quill,
            "[Desktop Entry]\nExec=\"/home/ana/.local/bin/quill\"\n",
        )
        .unwrap();
        let program = folder.path().join("newen");
        std::fs::write(&program, "newen").unwrap();
        assert_eq!(
            replace_launcher(&applications, Path::new("/d/quill/app"), &program),
            Ok(false)
        );
        assert!(quill.is_file() && !applications.join("newen.desktop").exists());
        let installed = Path::new("/home/ana/.local");
        assert_eq!(
            replace_launcher(&applications, installed, &folder.path().join("missing")),
            Ok(false),
            "nothing to start"
        );
        assert!(quill.is_file() && !applications.join("newen.desktop").exists());
        assert_eq!(
            replace_launcher(&applications, installed, &program),
            Ok(true),
            "one into the installation given"
        );
        assert!(!quill.exists() && applications.join("newen.desktop").is_file());
    }

    /// A developer's build moves nothing unless asked; a release always may.
    #[test]
    fn only_a_release_or_a_build_asked_to_moves_quill_s_data() {
        assert!(allowed(false, None));
        assert!(!allowed(true, None));
        assert!(!allowed(true, Some("0".as_ref())));
        assert!(allowed(true, Some("1".as_ref())));
    }

    /// Windows writes a path's backslashes doubled inside JSON; the separator after a folder
    /// keeps a longer name out.
    #[test]
    fn paths_are_rewritten_as_json_holds_them() {
        assert_eq!(
            in_json(Path::new(r"C:\Users\ana\quill")),
            r"C:\\Users\\ana\\quill"
        );
        let pairs = moved_paths(Path::new("/d/quill"), Path::new("/d/newen"), &["pictures"]);
        let separator = if cfg!(windows) { r"\\" } else { "/" };
        assert_eq!(
            pairs,
            [(
                format!("{}{separator}", in_json(Path::new("/d/quill/pictures"))),
                format!("{}{separator}", in_json(Path::new("/d/newen/pictures")))
            )]
        );
    }
}
