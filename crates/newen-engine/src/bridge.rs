//! quill's `$BROWSER`, as Claude Desktop's shim: what a session's tools open
//! (`gh`, a dev server, Python's `webbrowser`) shows in that session's browser when it is a page
//! of this computer, and in the desktop's own browser otherwise. The agent runs with
//! `BROWSER` set to a script that hands the address back to quill over a socket only quill
//! and the person can open, with a key that names the session.
//!
//! shortcut: any local port is taken, where Desktop takes only ports a process of the session
//! listens on (`/proc/net/tcp*` and the process tree); worth it if other local apps' pages
//! start landing in sessions.

use newen_browser::Browser;
use newen_model::{BrowserOp, SessionId};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// What the agent's process is told: the script, the socket and the session's key.
pub const SHIM_ENV: [&str; 3] = ["BROWSER", "QUILL_BROWSER_BRIDGE", "QUILL_BROWSER_KEY"];

pub struct Bridge {
    socket: PathBuf,
    shim: PathBuf,
    keys: Arc<Mutex<HashMap<String, SessionId>>>,
}

impl Bridge {
    /// Listen for the script. `None` when there is no private folder for the socket, or
    /// the person set `BROWSER` themselves (theirs is kept).
    pub fn start(browser: Browser) -> Option<Self> {
        if std::env::var_os("BROWSER").is_some_and(|b| !b.is_empty()) {
            return None;
        }
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("quill-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
        let socket = dir.join("browser.sock");
        let _ = std::fs::remove_file(&socket);
        let listener = tokio::net::UnixListener::bind(&socket).ok()?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).ok()?;
        let exe = std::env::current_exe().ok()?;
        let shim = dir.join("open-url");
        let quoted = exe.display().to_string().replace('\'', r"'\''");
        std::fs::write(
            &shim,
            format!("#!/bin/sh\n# quill's $BROWSER: a local page opens in the session's browser.\nexec '{quoted}' --open-url \"$@\"\n"),
        )
        .ok()?;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700)).ok()?;
        let keys: Arc<Mutex<HashMap<String, SessionId>>> = Arc::default();
        let known = keys.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (browser, keys) = (browser.clone(), known.clone());
                tokio::spawn(async move {
                    let _ =
                        tokio::time::timeout(Duration::from_secs(3), answer(stream, browser, keys))
                            .await;
                });
            }
        });
        Some(Self { socket, shim, keys })
    }

    /// The environment `session`'s agent runs with.
    pub fn env(&self, session: SessionId) -> Vec<(String, String)> {
        let key = uuid::Uuid::new_v4().simple().to_string();
        self.keys.lock().unwrap().insert(key.clone(), session);
        vec![
            (SHIM_ENV[0].into(), self.shim.display().to_string()),
            (SHIM_ENV[1].into(), self.socket.display().to_string()),
            (SHIM_ENV[2].into(), key),
        ]
    }
}

/// One request from the script: `{"key", "url"}` on a line, answered "pane" when the page
/// opens in the session's browser and "declined" when the script should hand it on.
async fn answer(
    stream: tokio::net::UnixStream,
    browser: Browser,
    keys: Arc<Mutex<HashMap<String, SessionId>>>,
) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut line = String::new();
    BufReader::new(read.take(8 * 1024))
        .read_line(&mut line)
        .await?;
    let request: serde_json::Value = serde_json::from_str(line.trim()).unwrap_or_default();
    let key = request["key"].as_str().unwrap_or_default();
    let url = request["url"].as_str().unwrap_or_default();
    let session = keys
        .lock()
        .unwrap()
        .iter()
        .find(|(known, _)| same(known.as_bytes(), key.as_bytes()))
        .map(|(_, session)| *session);
    let reply = match session {
        Some(session) if newen_browser::is_local(url) => {
            browser.send(
                session,
                BrowserOp::Open {
                    url: Some(url.to_owned()),
                },
            );
            "pane"
        }
        _ => "declined",
    };
    write.write_all(format!("{reply}\n").as_bytes()).await
}

/// The keys compared in a time that does not tell how much of one matched.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
