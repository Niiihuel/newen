//! The dev servers the agent starts with `preview_start`, as Claude Desktop's:
//! a configuration of the project's launch file, approved by the person once (by what it
//! runs), started with its port in `PORT`, waited for until it listens, and opened in the
//! session's browser. Each runs in a process group of its own, so stopping it stops what it
//! started (npm's node) too.

use quill_agent::LaunchConfig;
use quill_model::SessionId;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Desktop's: at most five servers a folder, and three minutes to start listening.
const PER_FOLDER: usize = 5;
const READY: Duration = Duration::from_secs(180);
/// The lines of output a server keeps.
const KEPT: usize = 2000;

#[derive(Default)]
pub struct Servers {
    list: Mutex<Vec<Server>>,
    /// The configurations the person allowed for good, by what they run.
    approved: Mutex<HashSet<u64>>,
}

struct Server {
    id: String,
    session: SessionId,
    name: String,
    cwd: PathBuf,
    url: String,
    /// The process group's leader.
    pid: u32,
    /// Its output, errors marked, newest last.
    logs: Arc<Mutex<VecDeque<(bool, String)>>>,
    /// How it ended, once it did.
    ended: Arc<Mutex<Option<String>>>,
}

/// What `start` did.
pub struct Started {
    pub id: String,
    pub url: String,
    pub reused: bool,
}

/// The configuration `name` of the launch file `file` in `cwd`.
pub fn config(cwd: &Path, file: &str, name: &str) -> Result<LaunchConfig, String> {
    let path = cwd.join(file);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!("There is no {file} in this folder: create it, as your instructions say.")
    })?;
    let parsed: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{file} is not JSON: {e}"))?;
    let configurations = parsed["configurations"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let names: Vec<String> = configurations
        .iter()
        .filter_map(|c| c["name"].as_str().map(str::to_owned))
        .collect();
    let found = configurations
        .into_iter()
        .find(|c| c["name"] == name)
        .ok_or_else(|| {
            format!(
                "{file} has no server \"{name}\"; it has: {}.",
                names.join(", ")
            )
        })?;
    serde_json::from_value(found).map_err(|e| format!("The server \"{name}\" in {file}: {e}"))
}

/// What a configuration runs, as the person approves it.
pub fn command(config: &LaunchConfig) -> String {
    std::iter::once(config.runtime_executable.as_str())
        .chain(config.runtime_args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn fingerprint(cwd: &Path, config: &LaunchConfig) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    cwd.hash(&mut hasher);
    serde_json::to_string(config)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

impl Servers {
    pub fn approved(&self, fingerprint: u64) -> bool {
        self.approved.lock().unwrap().contains(&fingerprint)
    }

    pub fn approve(&self, fingerprint: u64) {
        self.approved.lock().unwrap().insert(fingerprint);
    }

    /// Start `config` in `cwd` for `session`, or reuse it when it runs there already; ready
    /// once its port takes connections.
    pub async fn start(
        &self,
        session: SessionId,
        cwd: &Path,
        config: &LaunchConfig,
    ) -> Result<Started, String> {
        let url = config
            .url
            .clone()
            .unwrap_or_else(|| format!("http://localhost:{}/", config.port));
        {
            let list = self.list.lock().unwrap();
            let running = |s: &&Server| s.ended.lock().unwrap().is_none();
            if let Some(server) = list
                .iter()
                .filter(running)
                .find(|s| s.cwd == cwd && s.name == config.name)
            {
                return Ok(Started {
                    id: server.id.clone(),
                    url: server.url.clone(),
                    reused: true,
                });
            }
            if list.iter().filter(running).filter(|s| s.cwd == cwd).count() >= PER_FOLDER {
                return Err(format!(
                    "{PER_FOLDER} servers already run in this folder: stop one (preview_stop) first."
                ));
            }
        }
        let mut process =
            tokio::process::Command::from(quill_files::command(&config.runtime_executable));
        process
            .args(&config.runtime_args)
            .current_dir(cwd)
            .env("PORT", config.port.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        process.process_group(0);
        // It goes with quill even when quill is killed rather than closed: Linux signals it
        // when the forking thread, one of the engine's runtime threads, ends.
        // SAFETY: only an async-signal-safe call between fork and exec.
        #[cfg(target_os = "linux")]
        unsafe {
            process.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
        let mut child = process
            .spawn()
            .map_err(|e| format!("Couldn't start {}: {e}", command(config)))?;
        let pid = child.id().unwrap_or_default();
        let logs: Arc<Mutex<VecDeque<(bool, String)>>> = Arc::default();
        let ended: Arc<Mutex<Option<String>>> = Arc::default();
        for (error, stream) in [
            (
                false,
                child
                    .stdout
                    .take()
                    .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
            ),
            (
                true,
                child
                    .stderr
                    .take()
                    .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
            ),
        ] {
            let (Some(stream), logs) = (stream, logs.clone()) else {
                continue;
            };
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut logs = logs.lock().unwrap();
                    if logs.len() == KEPT {
                        logs.pop_front();
                    }
                    logs.push_back((error, line));
                }
            });
        }
        let gone = ended.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            *gone.lock().unwrap() = Some(match status {
                Ok(status) => format!("exited ({status})"),
                Err(e) => format!("lost ({e})"),
            });
        });
        let id = format!("server-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        self.list.lock().unwrap().push(Server {
            id: id.clone(),
            session,
            name: config.name.clone(),
            cwd: cwd.to_owned(),
            url: url.clone(),
            pid,
            logs: logs.clone(),
            ended: ended.clone(),
        });
        let start = Instant::now();
        loop {
            if let Some(how) = ended.lock().unwrap().clone() {
                return Err(format!(
                    "{} {how} before it listened on port {}. Its last output:\n{}",
                    config.name,
                    config.port,
                    tail(&logs, 20, false, "")
                ));
            }
            let listening = tokio::time::timeout(
                Duration::from_secs(1),
                tokio::net::TcpStream::connect(("127.0.0.1", config.port)),
            )
            .await;
            if matches!(listening, Ok(Ok(_))) {
                return Ok(Started {
                    id,
                    url,
                    reused: false,
                });
            }
            if start.elapsed() > READY {
                return Err(format!(
                    "{} did not listen on port {} within {} s. Its last output:\n{}",
                    config.name,
                    config.port,
                    READY.as_secs(),
                    tail(&logs, 20, false, "")
                ));
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
    }

    pub fn stop(&self, session: SessionId, id: &str) -> Result<String, String> {
        let list = self.list.lock().unwrap();
        let server = list
            .iter()
            .find(|s| s.id == id && s.session == session)
            .ok_or_else(|| format!("There is no server {id}."))?;
        if server.ended.lock().unwrap().is_some() {
            return Ok(format!("{} had already stopped.", server.name));
        }
        stop_group(server.pid);
        Ok(format!("Stopped {}.", server.name))
    }

    pub fn list(&self, session: SessionId) -> String {
        let list = self.list.lock().unwrap();
        let lines: Vec<String> = list
            .iter()
            .filter(|s| s.session == session)
            .map(|s| {
                let state = s
                    .ended
                    .lock()
                    .unwrap()
                    .clone()
                    .unwrap_or_else(|| "running".into());
                format!("{} \"{}\" {} ({state})", s.id, s.name, s.url)
            })
            .collect();
        if lines.is_empty() {
            "No servers were started with preview_start.".into()
        } else {
            lines.join("\n")
        }
    }

    pub fn logs(
        &self,
        session: SessionId,
        id: &str,
        errors: bool,
        search: &str,
    ) -> Result<String, String> {
        let list = self.list.lock().unwrap();
        let server = list
            .iter()
            .find(|s| s.id == id && s.session == session)
            .ok_or_else(|| format!("There is no server {id}."))?;
        let text = tail(&server.logs, 400, errors, search);
        Ok(if text.is_empty() {
            "No output.".into()
        } else {
            text
        })
    }

    /// Stop every server of `session`: it was deleted.
    pub fn forget(&self, session: SessionId) {
        for server in self
            .list
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.session == session)
        {
            stop_group(server.pid);
        }
    }
}

/// Servers stop with quill.
impl Drop for Servers {
    fn drop(&mut self) {
        if let Ok(list) = self.list.get_mut() {
            for server in list.iter() {
                stop_group(server.pid);
            }
        }
    }
}

fn stop_group(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(unix)]
    // SAFETY: a signal to the process group the server leads.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
    // /T takes the tree: Windows has no process groups to signal.
    #[cfg(windows)]
    let _ = quill_files::command("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
}

fn tail(logs: &Mutex<VecDeque<(bool, String)>>, most: usize, errors: bool, search: &str) -> String {
    let logs = logs.lock().unwrap();
    let lines: Vec<String> = logs
        .iter()
        .filter(|(error, line)| {
            (!errors || *error || line.to_lowercase().contains("error")) && line.contains(search)
        })
        .map(|(error, line)| {
            if *error {
                format!("[err] {line}")
            } else {
                line.clone()
            }
        })
        .collect();
    lines[lines.len().saturating_sub(most)..].join("\n")
}

/// Where a session's dev servers come from: its folder and its agent's launch file.
pub struct Place {
    pub cwd: Option<PathBuf>,
    pub file: Option<&'static str>,
}

/// The dev server tools (`preview_start`, `preview_stop`, `preview_list`, `preview_logs`).
pub async fn tool(
    servers: &Servers,
    browser: &quill_browser::Browser,
    ask: &crate::Ask,
    session: SessionId,
    place: Place,
    name: &str,
    input: &serde_json::Value,
) -> (Vec<quill_agent::ToolContent>, bool) {
    use quill_agent::ToolContent;
    let answer = |text: String| (ToolContent::text(text), false);
    let failed = |text: String| (ToolContent::text(text), true);
    let server_id = input["serverId"].as_str().unwrap_or_default();
    match name {
        "preview_list" => answer(servers.list(session)),
        "preview_stop" => match servers.stop(session, server_id) {
            Ok(text) => answer(text),
            Err(text) => failed(text),
        },
        "preview_logs" => {
            let errors = input["level"] == "error";
            let search = input["search"].as_str().unwrap_or_default();
            match servers.logs(session, server_id, errors, search) {
                Ok(text) => answer(text),
                Err(text) => failed(text),
            }
        }
        "preview_start" => {
            if let Some(url) = input["url"].as_str().filter(|_| input["name"].is_null()) {
                return browser
                    .tool(session, "tabs_create", serde_json::json!({ "url": url }))
                    .await;
            }
            let Some(server) = input["name"].as_str() else {
                return failed("preview_start needs a name or a url.".into());
            };
            let (Some(cwd), Some(file)) = (place.cwd, place.file) else {
                return failed("This agent keeps no launch file of dev servers.".into());
            };
            let config = match config(&cwd, file, server) {
                Ok(config) => config,
                Err(text) => return failed(text),
            };
            // The person approves what a configuration runs, once or for as long as quill runs.
            let print = fingerprint(&cwd, &config);
            if !servers.approved(print) {
                let input = serde_json::json!({
                    "kind": "server", "name": config.name, "command": command(&config),
                    "folder": cwd.display().to_string(),
                });
                match ask(session, input, None).await {
                    Ok(quill_browser::Grant::Always) => servers.approve(print),
                    Ok(quill_browser::Grant::Once) => {}
                    _ => {
                        return failed(format!(
                        "The user did not allow starting {}. Do not try it again unless they ask.",
                        config.name
                    ))
                    }
                }
            }
            let started = match servers.start(session, &cwd, &config).await {
                Ok(started) => started,
                Err(text) => return failed(text),
            };
            // The tab's answer leads, for the conversation's card.
            let (mut content, is_error) = browser
                .tool(
                    session,
                    "tabs_create",
                    serde_json::json!({ "url": started.url }),
                )
                .await;
            let said = if started.reused {
                format!(
                    "{} was already running ({}) at {}.",
                    config.name, started.id, started.url
                )
            } else {
                format!(
                    "Started {} ({}) at {}.",
                    config.name, started.id, started.url
                )
            };
            content.push(ToolContent::Text(format!(
                "{said} serverId {} is for preview_stop and preview_logs.",
                started.id
            )));
            (content, is_error)
        }
        _ => failed(format!("There is no tool {name}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_server_starts_listens_logs_and_stops() {
        let dir = std::env::temp_dir().join(format!("quill-servers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::fs::write(
            dir.join("launch.json"),
            format!(
                r#"{{"version":"0.0.1","configurations":[{{"name":"dev","runtimeExecutable":"python3","runtimeArgs":["-c","import os,http.server,sys; print('serving', os.environ['PORT'], flush=True); http.server.HTTPServer(('127.0.0.1', int(os.environ['PORT'])), http.server.SimpleHTTPRequestHandler).serve_forever()"],"port":{port}}}]}}"#
            ),
        )
        .unwrap();
        let config = config(&dir, "launch.json", "dev").unwrap();
        assert!(super::config(&dir, "launch.json", "nope")
            .unwrap_err()
            .contains("it has: dev"));
        let servers = Servers::default();
        let started = servers.start(1, &dir, &config).await.unwrap();
        assert_eq!(started.url, format!("http://localhost:{port}/"));
        // The same server again is the one running.
        assert!(servers.start(1, &dir, &config).await.unwrap().reused);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(servers
            .logs(1, &started.id, false, "")
            .unwrap()
            .contains(&format!("serving {port}")));
        assert!(servers.list(1).contains("running"));
        servers.stop(1, &started.id).unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(servers.list(1).contains("exited"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
