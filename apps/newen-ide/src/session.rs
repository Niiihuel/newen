//! What was open in each folder, to open it again: kept in
//! `$XDG_DATA_HOME/quill-ide/sessions.json`, by folder.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// The open files, in their pills' order, and the one on screen.
    pub open: Vec<PathBuf>,
    pub active: Option<usize>,
    /// The folders opened in the tree.
    pub expanded: Vec<PathBuf>,
}

fn path() -> PathBuf {
    let data = newen_files::data_dir().unwrap_or_else(|| ".".into());
    data.join("quill-ide/sessions.json")
}

fn all() -> BTreeMap<PathBuf, Session> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// `root`'s, or an empty one.
pub fn load(root: &Path) -> Session {
    all().remove(root).unwrap_or_default()
}

/// Keep `root`'s, whole or not at all, beside every other folder's.
pub fn save(root: &Path, session: &Session) -> std::io::Result<()> {
    let mut sessions = all();
    sessions.insert(root.to_path_buf(), session.clone());
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension(format!("json.{}", std::process::id()));
    let json = serde_json::to_vec_pretty(&sessions).map_err(std::io::Error::other)?;
    std::fs::write(&temporary, json)?;
    std::fs::rename(&temporary, &path)
}
