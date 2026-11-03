//! Newen's `$BROWSER`, as Claude Desktop's shim: what a session's tools open
//! (`gh`, a dev server, Python's `webbrowser`) shows in that session's browser when it is a page
//! of this computer, and in the desktop's own browser otherwise. The agent runs with
//! `BROWSER` set to a script that hands the address back to Newen over a socket only Newen
//! and the person can open, with a key that names the session.
//!
//! The same socket serves Newen's own tools to an agent that takes tools only from MCP servers
//! it starts itself (`SessionExtras::host`): it runs `newen --mcp-host <server>`, which passes
//! its stdio here with the session's key (docs/agents.md "Las herramientas de Newen").
//!
//! shortcut: any local port is taken, where Desktop takes only ports a process of the session
//! listens on (`/proc/net/tcp*` and the process tree); worth it if other local apps' pages
//! start landing in sessions.

use newen_agent::{InjectedServer, ToolContent, BROWSER_TOOLS, HOST_SERVER, HOST_TOOLS};
use newen_browser::Browser;
use newen_model::{BrowserOp, SessionId};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};

/// What the agent's process is told: the script, the socket and the session's key.
pub const SHIM_ENV: [&str; 3] = ["BROWSER", "NEWEN_BROWSER_BRIDGE", "NEWEN_BROWSER_KEY"];

/// The server of the built-in browser's tools, beside [`HOST_SERVER`]'s.
const BROWSER_SERVER: &str = "browser";

/// Runs one of Newen's own tools that a session's agent called over the socket: its answer
/// and whether it failed.
pub type Host = Arc<
    dyn Fn(SessionId, String, Value) -> tokio::sync::oneshot::Receiver<(Vec<ToolContent>, bool)>
        + Send
        + Sync,
>;

pub struct Bridge {
    socket: PathBuf,
    shim: PathBuf,
    exe: PathBuf,
    /// The person set `BROWSER` themselves: theirs is kept, and the socket serves Newen's
    /// tools only.
    theirs: bool,
    keys: Arc<Mutex<HashMap<String, SessionId>>>,
}

impl Bridge {
    /// Listen for the script and for Newen's tools. `None` when there is no private folder
    /// for the socket.
    pub fn start(browser: Browser, host: Host) -> Option<Self> {
        let theirs = std::env::var_os("BROWSER").is_some_and(|b| !b.is_empty());
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("newen-{}", std::process::id()));
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
            format!("#!/bin/sh\n# Newen's $BROWSER: a local page opens in the session's browser.\nexec '{quoted}' --open-url \"$@\"\n"),
        )
        .ok()?;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o700)).ok()?;
        let keys: Arc<Mutex<HashMap<String, SessionId>>> = Arc::default();
        let known = keys.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let (browser, keys, host) = (browser.clone(), known.clone(), host.clone());
                tokio::spawn(answer(stream, browser, keys, host, theirs));
            }
        });
        Some(Self {
            socket,
            shim,
            exe,
            theirs,
            keys,
        })
    }

    /// The environment `session`'s agent runs with.
    pub fn env(&self, session: SessionId) -> Vec<(String, String)> {
        let key = uuid::Uuid::new_v4().simple().to_string();
        self.keys.lock().unwrap().insert(key.clone(), session);
        let mut env = vec![
            (SHIM_ENV[1].into(), self.socket.display().to_string()),
            (SHIM_ENV[2].into(), key),
        ];
        if !self.theirs {
            env.insert(0, (SHIM_ENV[0].into(), self.shim.display().to_string()));
        }
        env
    }

    /// Newen's own tools and the browser's as the MCP servers an agent starts, for the
    /// session whose environment ([`env`](Self::env)) is `env`. The key rides in the server's
    /// environment, which the agent passes to it.
    pub fn servers(&self, env: &[(String, String)]) -> Vec<InjectedServer> {
        let ours: Vec<(String, String)> = (env.iter())
            .filter(|(key, _)| SHIM_ENV[1..].contains(&key.as_str()))
            .cloned()
            .collect();
        if ours.len() < 2 {
            return vec![];
        }
        [HOST_SERVER, BROWSER_SERVER]
            .map(|name| InjectedServer {
                name: name.into(),
                target: self.exe.display().to_string(),
                args: vec!["--mcp-host".into(), name.into()],
                env: ours.clone(),
                ..Default::default()
            })
            .into()
    }
}

/// One caller: the script, `{"key", "url"}` on a line, answered "pane" when the page opens in
/// the session's browser and "declined" when the script should hand it on; or Newen's tools,
/// `{"key", "mcp": <server>}`, after which the connection is that MCP server's.
async fn answer(
    stream: tokio::net::UnixStream,
    browser: Browser,
    keys: Arc<Mutex<HashMap<String, SessionId>>>,
    host: Host,
    theirs: bool,
) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut read = BufReader::new(read);
    let mut line = String::new();
    let mut first = (&mut read).take(8 * 1024);
    let said = tokio::time::timeout(Duration::from_secs(3), first.read_line(&mut line)).await;
    if said.is_err() {
        return Ok(());
    }
    let request: Value = serde_json::from_str(line.trim()).unwrap_or_default();
    let key = request["key"].as_str().unwrap_or_default();
    let url = request["url"].as_str().unwrap_or_default();
    let session = keys
        .lock()
        .unwrap()
        .iter()
        .find(|(known, _)| same(known.as_bytes(), key.as_bytes()))
        .map(|(_, session)| *session);
    if let (Some(server), Some(session)) = (request["mcp"].as_str(), session) {
        serve(read, write, session, server, host).await;
        return Ok(());
    }
    let reply = match session {
        Some(session) if !theirs && newen_browser::is_local(url) => {
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

/// One of Newen's MCP servers for the session's agent, a JSON-RPC message a line: the handshake
/// and the list are answered here, and a call runs as the agent's own calls do, each beside the
/// others (a browser tool may wait on a page).
async fn serve(
    read: impl AsyncBufRead + Unpin,
    mut write: impl AsyncWrite + Unpin + Send + 'static,
    session: SessionId,
    server: &str,
    host: Host,
) {
    let tools: &[(&str, &str, &str)] = if server == BROWSER_SERVER {
        &BROWSER_TOOLS
    } else {
        &HOST_TOOLS
    };
    let (out, mut lines_out) = tokio::sync::mpsc::unbounded_channel::<Value>();
    let writer = tokio::spawn(async move {
        while let Some(message) = lines_out.recv().await {
            let line = format!("{message}\n");
            if write.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    let mut lines = read.lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // Notifications (`notifications/initialized`, a cancel) want no answer.
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let result = match message["method"].as_str() {
            Some("initialize") => json!({
                "protocolVersion": message["params"]["protocolVersion"],
                "capabilities": { "tools": {} },
                "serverInfo": { "name": server, "version": env!("CARGO_PKG_VERSION") },
            }),
            Some("tools/list") => {
                json!({ "tools": tools.iter().map(|(name, description, schema)| json!({
                "name": name, "description": description,
                "inputSchema": serde_json::from_str::<Value>(schema).unwrap_or_default(),
            })).collect::<Vec<_>>() })
            }
            Some("tools/call") => {
                let params = &message["params"];
                let name = params["name"].as_str().unwrap_or_default();
                let answer = host(session, name.to_owned(), params["arguments"].clone());
                let out = out.clone();
                tokio::spawn(async move {
                    let (content, is_error) = answer.await.unwrap_or_else(|_| {
                        (ToolContent::text("Newen closed this session."), true)
                    });
                    let content: Vec<Value> = (content.into_iter())
                        .map(|block| match block {
                            ToolContent::Text(text) => json!({ "type": "text", "text": text }),
                            ToolContent::Image { mime, data } => {
                                json!({ "type": "image", "data": data, "mimeType": mime })
                            }
                        })
                        .collect();
                    let result = json!({ "content": content, "isError": is_error });
                    let _ = out.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                });
                continue;
            }
            Some("ping") => json!({}),
            _ => {
                let error = json!({ "code": -32601, "message": "Method not found" });
                let _ = out.send(json!({ "jsonrpc": "2.0", "id": id, "error": error }));
                continue;
            }
        };
        let _ = out.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }
    drop(out);
    let _ = writer.await;
}

/// The keys compared in a time that does not tell how much of one matched.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the agent's MCP client hears from one of Newen's servers for these lines.
    async fn heard(server: &str, lines: &[Value]) -> Vec<Value> {
        let host: Host = Arc::new(|session, name, input| {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let said = format!("{session} {name} {input}");
            let _ = tx.send((ToolContent::text(said), false));
            rx
        });
        let (agent, ours) = tokio::io::duplex(64 * 1024);
        let (read, write) = tokio::io::split(ours);
        let server = server.to_owned();
        let served =
            tokio::spawn(async move { serve(BufReader::new(read), write, 7, &server, host).await });
        let (from, mut to) = tokio::io::split(agent);
        for line in lines {
            to.write_all(format!("{line}\n").as_bytes()).await.unwrap();
        }
        to.shutdown().await.unwrap();
        let mut heard = vec![];
        let mut from = BufReader::new(from).lines();
        while let Ok(Some(line)) = from.next_line().await {
            heard.push(serde_json::from_str(&line).unwrap());
        }
        served.await.unwrap();
        heard
    }

    #[tokio::test]
    async fn every_one_of_newens_tools_is_offered_and_runs_for_its_session() {
        let heard = heard(
            HOST_SERVER,
            &[
                json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
                json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
                json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
                json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "list_sessions", "arguments": {} } }),
                json!({ "jsonrpc": "2.0", "id": 3, "method": "resources/list" }),
            ],
        )
        .await;
        let by_id = |id: u64| heard.iter().find(|m| m["id"] == id).unwrap().clone();
        assert_eq!(by_id(0)["result"]["protocolVersion"], "2025-06-18");
        let list = by_id(1);
        let names: Vec<&str> = (list["result"]["tools"].as_array().unwrap().iter())
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, HOST_TOOLS.map(|(name, ..)| name));
        assert_eq!(
            by_id(2)["result"],
            json!({ "content": [{ "type": "text", "text": "7 list_sessions {}" }], "isError": false })
        );
        assert_eq!(by_id(3)["error"]["code"], -32601);
        assert_eq!(heard.len(), 4, "a notification gets no answer");
    }

    #[tokio::test]
    async fn the_browser_server_offers_the_browser_tools() {
        let heard = heard(
            BROWSER_SERVER,
            &[json!({ "jsonrpc": "2.0", "id": "a", "method": "tools/list" })],
        )
        .await;
        let tools = heard[0]["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), BROWSER_TOOLS.len());
    }
}
