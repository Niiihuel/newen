//! The shared terminal's tools (docs/terminal.md §3): the agent runs a command in its session's
//! tab of Newen's terminal, which the person sees and types into, and reads how it stands. A
//! call returns once the command ends, waits for someone, goes quiet, or its time is up; the
//! agent then answers it or waits again.

use crate::Ask;
use newen_agent::ToolContent;
use newen_model::SessionId;
use newen_pty::{Progress, Run, State, Terminals};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A new tab's size until the window shows it at the pane's.
const COLS: u16 = 120;
const ROWS: u16 = 30;
/// A command quiet this long while it runs returns the call: it may be asking in a way the
/// terminal does not show (a prompt read by polling), and the agent looks.
const QUIET: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(50);
/// The most of a command's output a call returns by default, its end kept.
const MOST: usize = 20_000;

/// What the tools keep between calls: each session's command under way, and the sessions the
/// person allowed every command of.
#[derive(Clone, Default)]
pub(crate) struct Shared {
    runs: Arc<Mutex<HashMap<SessionId, Run>>>,
    allowed: Arc<Mutex<HashSet<SessionId>>>,
}

/// Where a session's commands run: its folder, the tab's name and who to ask.
pub(crate) struct Place {
    pub cwd: PathBuf,
    pub title: String,
}

type Answer = (Vec<ToolContent>, bool);

fn failed(text: impl Into<String>) -> Answer {
    (ToolContent::text(text.into()), true)
}

/// Run the terminal tool `name` for `session`.
pub(crate) async fn tool(
    terminals: &Terminals,
    shared: &Shared,
    ask: &Ask,
    session: SessionId,
    place: Place,
    name: &str,
    input: &Value,
) -> Answer {
    let owner = session.to_string();
    let most = (input["max_chars"].as_u64()).map_or(MOST, |n| n as usize);
    let yielded = |default: u64| {
        Duration::from_millis(
            input["yield_ms"]
                .as_u64()
                .unwrap_or(default)
                .clamp(250, 30_000),
        )
    };
    match name {
        "terminal_run" => {
            let Some(command) = input["command"].as_str().filter(|c| !c.trim().is_empty()) else {
                return failed("terminal_run needs a command.");
            };
            if let Err(text) = permitted(shared, ask, session, &place, command).await {
                return failed(text);
            }
            // The tab, opened in the session's folder the first time.
            {
                let mut folders = terminals.folders();
                let tabs = folders.entry(place.cwd.clone()).or_default();
                if tabs.agent_mut(&owner).is_none() {
                    let name = tab_name(&place.title);
                    let opened = tabs.open_agent(
                        &place.cwd,
                        &owner,
                        name,
                        terminals.shell(),
                        COLS,
                        ROWS,
                        terminals.waker(),
                    );
                    if let Err(error) = opened {
                        return failed(format!("No terminal could be opened: {error}"));
                    }
                }
                let (_, terminal) = tabs.agent_mut(&owner).expect("opened above");
                if terminal.state() != State::Prompt {
                    return failed("A command is still running in your terminal tab: answer it with terminal_write, wait with empty text, or stop it with \\u0003.");
                }
            }
            // A shell that is still starting is not typed into.
            let started = Instant::now();
            while !with_tab(terminals, &place, &owner, |t| t.ready()).unwrap_or(true) {
                if started.elapsed() > Duration::from_secs(10) {
                    break;
                }
                tokio::time::sleep(POLL).await;
            }
            let Some(run) = with_tab(terminals, &place, &owner, |t| t.run_command(command)) else {
                return failed("Your terminal tab was closed.");
            };
            shared.runs.lock().unwrap().insert(session, run);
            follow(terminals, shared, &place, session, yielded(10_000), most).await
        }
        "terminal_write" => {
            let text = input["text"].as_str().unwrap_or_default();
            let state = with_tab(terminals, &place, &owner, |t| t.state());
            let Some(state) = state else {
                return failed("You have no terminal tab yet: run a command with terminal_run.");
            };
            if !shared.runs.lock().unwrap().contains_key(&session) || state == State::Prompt {
                if text.is_empty() {
                    return read(terminals, &place, &owner, 0);
                }
                return failed(
                    "Nothing is running in your terminal tab: run a command with terminal_run.",
                );
            }
            if !text.is_empty() {
                // The person types a secret, never the agent.
                if state == (State::Waiting { secret: true }) {
                    return failed("The command is reading a secret, which the user types in the Terminal. Ask them to, then wait with terminal_write and empty text.");
                }
                let enter = input["enter"].as_bool().unwrap_or(true);
                with_tab(terminals, &place, &owner, |t| {
                    t.write(text.as_bytes());
                    if enter {
                        t.write(b"\r");
                    }
                });
            }
            let default = if text.is_empty() { 10_000 } else { 2_000 };
            follow(terminals, shared, &place, session, yielded(default), most).await
        }
        "terminal_read" => {
            let above = input["lines"].as_u64().unwrap_or(0) as usize;
            read(terminals, &place, &owner, above)
        }
        _ => failed(format!("{name} is not a terminal tool.")),
    }
}

/// Whether the person lets the agent run `command`: once, for the rest of the session, or not.
/// A mode that skips every approval skips this one too (the `ask` itself knows).
async fn permitted(
    shared: &Shared,
    ask: &Ask,
    session: SessionId,
    place: &Place,
    command: &str,
) -> Result<(), String> {
    if shared.allowed.lock().unwrap().contains(&session) {
        return Ok(());
    }
    let input = json!({
        "kind": "command",
        "command": command,
        "folder": place.cwd.display().to_string(),
    });
    match ask(session, input, None).await {
        Ok(newen_browser::Grant::Always) => {
            shared.allowed.lock().unwrap().insert(session);
            Ok(())
        }
        Ok(newen_browser::Grant::Once) => Ok(()),
        _ => Err(
            "The user did not allow running this command. Do not run it again unless they ask."
                .into(),
        ),
    }
}

/// `act` on `owner`'s tab, `None` once it is gone.
fn with_tab<T>(
    terminals: &Terminals,
    place: &Place,
    owner: &str,
    act: impl FnOnce(&mut newen_pty::Terminal) -> T,
) -> Option<T> {
    let mut folders = terminals.folders();
    let (_, terminal) = folders.get_mut(&place.cwd)?.agent_mut(owner)?;
    Some(act(terminal))
}

/// Watch the session's command until it ends, waits for someone (still so a moment later),
/// goes quiet, or `limit` passes; then say how it stands and what it printed.
async fn follow(
    terminals: &Terminals,
    shared: &Shared,
    place: &Place,
    session: SessionId,
    limit: Duration,
    most: usize,
) -> Answer {
    let owner = session.to_string();
    let started = Instant::now();
    let mut waited = 0;
    loop {
        let progress = {
            let mut runs = shared.runs.lock().unwrap();
            let Some(run) = runs.get_mut(&session) else {
                return failed("Nothing is running in your terminal tab.");
            };
            with_tab(terminals, place, &owner, |t| t.progress(run))
        };
        let Some(progress) = progress else {
            shared.runs.lock().unwrap().remove(&session);
            return failed("Your terminal tab was closed.");
        };
        waited = match progress.state {
            State::Waiting { .. } => waited + 1,
            _ => 0,
        };
        let quiet = progress.state == State::Running
            && progress.idle >= QUIET
            && started.elapsed() >= Duration::from_millis(500);
        if progress.done || waited >= 2 || quiet || started.elapsed() >= limit {
            if progress.done {
                shared.runs.lock().unwrap().remove(&session);
            }
            return answer(&progress, most);
        }
        tokio::time::sleep(POLL).await;
    }
}

/// The tab as the person sees it, with how its command stands.
fn read(terminals: &Terminals, place: &Place, owner: &str, above: usize) -> Answer {
    let read = with_tab(terminals, place, owner, |t| {
        (t.state(), t.idle(), t.lines(above))
    });
    let Some((state, idle, text)) = read else {
        return failed("You have no terminal tab yet: run a command with terminal_run.");
    };
    let mut head = head(state, None, idle);
    if let State::Waiting { .. } = state {
        let last = text.lines().rev().find(|l| !l.trim().is_empty());
        head["prompt"] = json!(last.unwrap_or_default().trim_end());
    }
    (ToolContent::text(format!("{head}\n{text}")), false)
}

/// The call's answer: its JSON line, then the output, cut to its last `most` characters.
fn answer(progress: &Progress, most: usize) -> Answer {
    let mut head = head(progress.state, progress.exit, progress.idle);
    if progress.done {
        head["state"] = json!("done");
    }
    if let State::Waiting { .. } = progress.state {
        head["prompt"] = json!(progress.last_line);
    }
    let output = &progress.output;
    let kept = output.chars().count().saturating_sub(most);
    let text = match kept {
        0 => output.clone(),
        cut => format!(
            "[{cut} characters cut]\n{}",
            output.chars().skip(cut).collect::<String>()
        ),
    };
    (ToolContent::text(format!("{head}\n{text}")), false)
}

fn head(state: State, exit: Option<i32>, idle: Duration) -> Value {
    let (state, secret) = match state {
        State::Prompt => ("idle", false),
        State::Running => ("running", false),
        State::Waiting { secret } => ("waiting", secret),
    };
    json!({
        "type": "terminal",
        "state": state,
        "exit": exit,
        "secret": secret,
        "idle_ms": idle.as_millis() as u64,
    })
}

/// The tab's name: the session's title, short, else "Agent".
fn tab_name(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return "Agent".into();
    }
    let short: String = title.chars().take(24).collect();
    if short.chars().count() < title.chars().count() {
        format!("{}\u{2026}", short.trim_end())
    } else {
        short
    }
}

/// Close `session`'s tab, as its session goes.
pub(crate) fn forget(terminals: &Terminals, shared: &Shared, session: SessionId) {
    shared.runs.lock().unwrap().remove(&session);
    shared.allowed.lock().unwrap().remove(&session);
    let owner = session.to_string();
    for tabs in terminals.folders().values_mut() {
        if let Some((at, _)) = tabs.agent_mut(&owner) {
            tabs.close(at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow() -> Ask {
        Arc::new(|_, _, _| {
            let (answer, heard) = tokio::sync::oneshot::channel();
            let _ = answer.send(newen_browser::Grant::Once);
            heard
        })
    }

    fn deny() -> Ask {
        Arc::new(|_, _, _| {
            let (answer, heard) = tokio::sync::oneshot::channel();
            let _ = answer.send(newen_browser::Grant::No);
            heard
        })
    }

    fn head_of(answer: &Answer) -> Value {
        let ToolContent::Text(text) = &answer.0[0] else {
            panic!("not text");
        };
        serde_json::from_str(text.lines().next().unwrap()).unwrap()
    }

    fn body_of(answer: &Answer) -> String {
        let ToolContent::Text(text) = &answer.0[0] else {
            panic!("not text");
        };
        text.lines().skip(1).collect::<Vec<_>>().join("\n")
    }

    fn place() -> Place {
        Place {
            cwd: std::env::temp_dir(),
            title: "Install the tools".into(),
        }
    }

    /// The agent runs a command that asks, answers it, and reads its end; a secret is the
    /// person's to type, and nothing runs that they did not allow.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_command_is_run_answered_and_followed_to_its_end() {
        // A plain sh: no marks and none of the person's configuration, so the command's end
        // is told by the shell having the terminal back.
        let terminals = Terminals::with_shell("/bin/sh");
        let shared = Shared::default();
        let run = |input: Value, ask: Ask| {
            let (terminals, shared) = (terminals.clone(), shared.clone());
            async move {
                tool(
                    &terminals,
                    &shared,
                    &ask,
                    7,
                    place(),
                    "terminal_run",
                    &input,
                )
                .await
            }
        };
        let write = |text: &str| {
            let (terminals, shared) = (terminals.clone(), shared.clone());
            let input = json!({ "text": text, "yield_ms": 5000 });
            async move {
                tool(
                    &terminals,
                    &shared,
                    &allow(),
                    7,
                    place(),
                    "terminal_write",
                    &input,
                )
                .await
            }
        };
        let refused = run(json!({ "command": "echo no" }), deny()).await;
        assert!(refused.1);
        assert!(terminals.folders().is_empty(), "nothing opened");
        let script = r#"read -p "Go on? " a; read -s -p "Password: " b; echo; echo got-$a"#;
        let asked = run(
            json!({ "command": format!("bash -c '{script}'"), "yield_ms": 8000 }),
            allow(),
        )
        .await;
        assert!(!asked.1, "{:?}", asked.0);
        assert_eq!(head_of(&asked)["state"], "waiting");
        assert_eq!(head_of(&asked)["prompt"], "Go on?");
        // One command at a time.
        let again = run(json!({ "command": "echo two" }), allow()).await;
        assert!(again.1);
        let secret = write("yes").await;
        assert_eq!(
            (
                head_of(&secret)["state"].as_str(),
                head_of(&secret)["secret"].as_bool()
            ),
            (Some("waiting"), Some(true))
        );
        assert!(write("hunter2").await.1, "the agent never types a secret");
        // The person types it in the tab.
        with_tab(&terminals, &place(), "7", |t| t.write(b"hunter2\r"));
        let done = write("").await;
        assert_eq!(head_of(&done)["state"], "done");
        assert!(body_of(&done).contains("got-yes"), "{}", body_of(&done));
        assert!(!body_of(&done).contains("hunter2"));
        assert!(write("late").await.1, "nothing runs any more");
        let read = tool(
            &terminals,
            &shared,
            &allow(),
            7,
            place(),
            "terminal_read",
            &json!({}),
        )
        .await;
        assert_eq!(head_of(&read)["state"], "idle");
        assert!(body_of(&read).contains("got-yes"));
        // The tab is named for the session and goes with it.
        let titles: Vec<String> = (terminals.folders().get(&std::env::temp_dir()))
            .map(|tabs| tabs.list().into_iter().map(|t| t.title).collect())
            .unwrap_or_default();
        assert_eq!(titles, ["Install the tools"]);
        forget(&terminals, &shared, 7);
        assert!(terminals.folders()[&std::env::temp_dir()].is_empty());
    }

    #[test]
    fn a_long_output_keeps_its_end() {
        let progress = Progress {
            state: State::Prompt,
            done: true,
            exit: Some(0),
            idle: Duration::ZERO,
            output: "abcdef".into(),
            last_line: String::new(),
        };
        let answered = answer(&progress, 2);
        assert_eq!(head_of(&answered)["exit"], 0);
        assert_eq!(body_of(&answered), "[4 characters cut]\nef");
        assert_eq!(tab_name("  "), "Agent");
        assert_eq!(
            tab_name("A title that is longer than the tab"),
            "A title that is longer t\u{2026}"
        );
    }
}
