//! Signing in to an agent through its own program (`AgentAdapter::auth_query`, `login_run`,
//! `logout_run`). The program keeps the account, so nothing here is stored: the state is asked
//! again at each start and after each login or logout. Everything runs off the UI's thread.

use crate::Sink;
use newen_agent::{AgentAdapter, AuthState, ProgramRun};
use newen_model::Event;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

/// What the person does to a sign-in under way.
pub(crate) enum Login {
    /// The code the sign-in page showed.
    Code(String),
    Cancel,
}

/// What a finished run printed: its exit code and both streams.
struct Ran {
    exit: i32,
    stdout: String,
    stderr: String,
}

impl Ran {
    /// What the program said: its stdout, or its stderr when stdout is empty (Codex's
    /// `login status` writes to stderr; Claude's JSON stays on stdout whatever stderr holds).
    fn said(&self) -> &str {
        if self.stdout.trim().is_empty() {
            &self.stderr
        } else {
            &self.stdout
        }
    }
}

async fn run(mut command: Command, seconds: u64) -> Result<Ran, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command
        .spawn()
        .map_err(|e| format!("could not start: {e}"))?;
    let waited = tokio::time::timeout(Duration::from_secs(seconds), child.wait_with_output());
    let out = waited
        .await
        .map_err(|_| format!("took longer than {seconds}s"))?
        .map_err(|e| e.to_string())?;
    Ok(Ran {
        exit: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Ask the program who is signed in (`command` is the program with the arguments of
/// `auth_query`, in the environment a session gets) and tell the UI.
pub(crate) async fn check(
    adapter: Box<dyn AgentAdapter>,
    agent: String,
    command: Command,
    ui: &Sink,
) {
    let ran = run(command, 30).await;
    announce(adapter.as_ref(), agent, ran, ui);
}

fn announce(adapter: &dyn AgentAdapter, agent: String, ran: Result<Ran, String>, ui: &Sink) {
    let state = match ran {
        Ok(ran) => adapter.auth_state(ran.said(), ran.exit),
        Err(e) => AuthState::Unknown(e),
    };
    ui(Event::Auth { agent, state });
}

/// Sign out, then ask who is signed in. A failed logout shows as the state that stayed.
pub(crate) async fn logout(
    adapter: Box<dyn AgentAdapter>,
    agent: String,
    logout: Command,
    status: Option<Command>,
    ui: &Sink,
) {
    let _ = run(logout, 60).await;
    if let Some(status) = status {
        let ran = run(status, 30).await;
        announce(adapter.as_ref(), agent, ran, ui);
    }
}

/// Run the program's login: report the page it prints (opening it in the browser), pass on
/// the code the person pastes, and when it exits ask who is signed in, or say why not. The
/// returned sender takes the person's code or cancel; dropping it cancels too.
pub(crate) fn login(
    adapter: Box<dyn AgentAdapter>,
    agent: String,
    run_spec: ProgramRun,
    mut command: Command,
    status: Option<Command>,
    ui: Sink,
    open_page: bool,
) -> UnboundedSender<Login> {
    let (tx, mut rx) = unbounded_channel();
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    tokio::spawn(async move {
        let failed = |message: String| {
            ui(Event::SignInFailed {
                agent: agent.clone(),
                message,
            })
        };
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => return failed(format!("could not start: {e}")),
        };
        let (mut stdin, stdout, stderr) = (
            child.stdin.take().expect("piped stdin"),
            child.stdout.take().expect("piped stdout"),
            child.stderr.take().expect("piped stderr"),
        );
        let mut lines = BufReader::new(stdout).lines();
        // The page address may come on either stream (Codex prints it on stderr).
        let mut errors = BufReader::new(stderr).lines();
        let mut errors_open = true;
        let mut last_error = String::new();
        let mut told = false;
        // The whole sign-in has the adapter's time; dropping the child kills it.
        // The adapter is `Send`, not `Sync`: shared across awaits only behind a lock.
        let adapter = std::sync::Mutex::new(adapter);
        let flow = async {
            loop {
                tokio::select! {
                    line = lines.next_line() => match line {
                        Ok(Some(line)) if !told => {
                            let url = adapter.lock().unwrap().login_url(&line);
                            if let Some(url) = url {
                                told = true;
                                if open_page {
                                    open(&url);
                                }
                                ui(Event::SignInUrl { agent: agent.clone(), url });
                            }
                        }
                        Ok(Some(_)) => {}
                        _ => break true,
                    },
                    line = errors.next_line(), if errors_open => match line {
                        Ok(Some(line)) => {
                            if !told {
                                let url = adapter.lock().unwrap().login_url(&line);
                                if let Some(url) = url {
                                    told = true;
                                    if open_page {
                                        open(&url);
                                    }
                                    ui(Event::SignInUrl { agent: agent.clone(), url });
                                }
                            }
                            if !line.trim().is_empty() {
                                last_error = line;
                            }
                        }
                        _ => errors_open = false,
                    },
                    input = rx.recv() => match input {
                        Some(Login::Code(code)) => {
                            let line = adapter.lock().unwrap().login_code(&code);
                            let _ = stdin.write_all(line.as_bytes()).await;
                        }
                        // The sign-in is the person's to abandon: no event follows.
                        Some(Login::Cancel) | None => break false,
                    },
                }
            }
        };
        let finished = tokio::time::timeout(Duration::from_secs(run_spec.timeout), flow).await;
        match finished {
            Ok(true) => {}
            Ok(false) => return drop(child.kill().await),
            Err(_) => {
                let _ = child.kill().await;
                return failed("Sign-in took too long".into());
            }
        }
        let exited = child.wait().await;
        if exited.is_ok_and(|status| status.success()) {
            if let Some(status) = status {
                let ran = run(status, 30).await;
                announce(adapter.lock().unwrap().as_ref(), agent, ran, &ui);
            }
            return;
        }
        let mut said = String::new();
        while let Ok(Some(line)) = errors.next_line().await {
            if !line.trim().is_empty() {
                said = line;
            }
        }
        let last = Some(said)
            .filter(|s| !s.is_empty())
            .or(Some(last_error).filter(|s| !s.is_empty()));
        failed(
            last.as_deref()
                .unwrap_or("Sign-in failed")
                .trim()
                .to_owned(),
        );
    });
    tx
}

/// The page in the person's browser, the way an MCP server's sign-in page opens, on any of
/// the three systems.
fn open(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    let _ = newen_files::open_in_system(url);
}

#[cfg(test)]
mod tests {
    use super::Ran;

    #[test]
    fn stdout_is_what_a_program_said_unless_it_is_empty() {
        let ran = |stdout: &str, stderr: &str| Ran {
            exit: 0,
            stdout: stdout.into(),
            stderr: stderr.into(),
        };
        assert_eq!(ran("{}", "warning").said(), "{}");
        assert_eq!(
            ran("", "Logged in using ChatGPT\n").said(),
            "Logged in using ChatGPT\n"
        );
    }
}
