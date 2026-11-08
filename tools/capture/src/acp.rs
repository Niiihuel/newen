//! The ACP driver of newen-capture: `newen-capture --agent opencode|hermes|grok|antigravity <scenario> [--model <m>]`
//! drives the agent's real CLI through Newen's own adapter, as the engine would, and records
//! every line (docs/acp.md "Capturas"). Same format as the other drivers, `{t_ms, dir, msg}` a
//! line in `captures/<agent>-<scenario>-<epoch>/session.jsonl`, plus `spec` (the session the
//! adapter is launched for) and `cmd` (what Newen asked of it), so a test replays it exactly
//! (`newen_agent_acp::replay`). Paths are written as `/work` (the repo), `/home/user` and
//! `/bin/newen`.
//!
//! Newen's own servers are stood in for by this program (`--mcp-stub <server>`), which answers
//! every call with a fixed line. Scenarios: simple, edit-bash, deny, always, todos, ask,
//! interrupt, resume, fork, plan, ask-mode, risky, unsafe, host. `--fake-model <port>` answers as the model from a
//! script per scenario (model.rs), for an agent pointed at that port.

use newen_agent::{
    AgentAdapter, AgentCommand, AgentEvent, Decision, InjectedServer, SessionSpec, BROWSER_TOOLS,
    HOST_TOOLS,
};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq)]
enum Policy {
    Allow,
    Always,
    Deny,
}

enum Step {
    Prompt(&'static str),
    /// A prompt, stopped at the agent's first word or tool.
    Interrupt(&'static str),
    /// A mode for the turns that follow.
    Mode(&'static str),
    /// The process closes and another continues the session: resumed, or forked.
    Restart {
        fork: bool,
    },
}

struct Scenario {
    policy: Policy,
    steps: Vec<Step>,
    /// Files the repo starts with.
    files: &'static [(&'static str, &'static str)],
}

fn scenario(agent: &str, name: &str) -> Option<Scenario> {
    use Step::*;
    // What makes each agent ask: OpenCode asks only to read a `.env` file by default; Hermes
    // asks before every edit in its default mode.
    // Antigravity asks before commands only.
    let (ask, files): (&str, &[(&str, &str)]) = match agent {
        "opencode" => (
            "Read the file .env with your read tool and tell me the value of NAME.",
            &[(".env", "NAME=capture\n")],
        ),
        "antigravity" => ("Run exactly this shell command: echo hi > denied.txt", &[]),
        _ => ("Create the file denied.txt containing the word hi.", &[]),
    };
    let (policy, steps) = match name {
        "simple" => (Policy::Allow, vec![Prompt("Reply with exactly the word: ok")]),
        "edit-bash" => (
            Policy::Allow,
            vec![Prompt(
                "Create notes.txt containing the line hello. Then edit it so the line reads hello world. \
                 Then run exactly this shell command: mv notes.txt moved.txt && rm moved.txt",
            )],
        ),
        "deny" => (Policy::Deny, vec![Prompt(ask)]),
        "always" => (Policy::Always, vec![Prompt(ask)]),
        "todos" => (
            Policy::Allow,
            vec![Prompt(
                "First write a todo list of three steps with your todo tool: create a.txt, create b.txt, list the folder. \
                 Then do each step, marking it done in the list as you go.",
            )],
        ),
        "ask" => (
            Policy::Allow,
            vec![Prompt(
                "Before doing anything, ask me with your question tool whether I prefer red or blue (two options). \
                 Then reply with only the color I chose.",
            )],
        ),
        "interrupt" => (
            Policy::Allow,
            vec![Interrupt("Count slowly from 1 to 200, one number per line, with no tools.")],
        ),
        "resume" => (
            Policy::Allow,
            vec![
                Prompt("Remember the word: lantern. Reply with exactly: ok"),
                Restart { fork: false },
                Prompt("What word did I ask you to remember? Reply with only that word."),
            ],
        ),
        "fork" => (
            Policy::Allow,
            vec![
                Prompt("Remember the word: harbor. Reply with exactly: ok"),
                Restart { fork: true },
                Prompt("What word did I ask you to remember? Reply with only that word."),
            ],
        ),
        "plan" => (
            Policy::Allow,
            vec![
                Mode(match agent {
                    "opencode" | "grok" | "antigravity" => "plan",
                    _ => "accept_edits",
                }),
                Prompt("Create the file planned.txt containing the word hi, then reply with what you did."),
            ],
        ),
        // Newen's own Ask mode for OpenCode (its `ask` agent): an edit asks, and is denied.
        "ask-mode" => (
            Policy::Deny,
            vec![Mode("ask"), Prompt("Create the file asked.txt containing the word hi.")],
        ),
        // The mode that never asks: an edit that asks in the default mode does not (and would
        // be denied if it did).
        "unsafe" => (
            Policy::Deny,
            vec![Mode(UNSAFE.iter().find(|(a, _)| *a == agent).map_or("", |(_, m)| m)),
                Prompt(match agent {
                    "antigravity" => "Run exactly this shell command: echo hi > free.txt",
                    _ => "Create the file free.txt containing the word hi.",
                })],
        ),
        // A command Hermes's approvals call risky.
        "risky" => (Policy::Deny, vec![Prompt("Run exactly this shell command: rm -rf build")]),
        "host" => (
            Policy::Allow,
            vec![Prompt(
                "Call the list_sessions tool (from the sessions server) once and reply with exactly what it returned.",
            )],
        ),
        _ => return None,
    };
    Some(Scenario {
        policy,
        steps,
        files,
    })
}

/// Each agent's mode that never asks.
const UNSAFE: [(&str, &str); 2] = [
    ("grok", "always-approve"),
    ("antigravity", "skip-permissions"),
];

fn adapter(agent: &str) -> Option<Box<dyn AgentAdapter>> {
    match agent {
        "grok" => Some(Box::new(newen_agent_grok::Grok::new())),
        "antigravity" => Some(Box::new(newen_agent_antigravity::Antigravity::new())),
        "opencode" => Some(Box::new(newen_agent_opencode::OpenCode::new())),
        "hermes" => Some(Box::new(newen_agent_hermes::Hermes::new())),
        _ => None,
    }
}

pub fn is_acp(agent: &str) -> bool {
    adapter(agent).is_some()
}

struct Run {
    adapter: Box<dyn AgentAdapter>,
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<(&'static str, String)>,
    out: std::fs::File,
    start: Instant,
    policy: Policy,
    session: Option<String>,
}

impl Run {
    fn record(&mut self, dir: &str, msg: Value) {
        let t_ms = self.start.elapsed().as_millis() as u64;
        let line = json!({ "t_ms": t_ms, "dir": dir, "msg": msg });
        let _ = writeln!(self.out, "{line}");
    }

    fn write(&mut self, line: &str) {
        self.record("out", serde_json::from_str(line).unwrap_or(json!(line)));
        if let Some(stdin) = self.stdin.as_mut() {
            let _ = writeln!(stdin, "{line}");
            let _ = stdin.flush();
        }
    }

    fn send(&mut self, command: AgentCommand) {
        self.record("cmd", serde_json::to_value(&command).unwrap());
        for line in self.adapter.encode(command) {
            self.write(&line);
        }
    }

    /// The events of the agent's next line, `None` when it said nothing in `wait`.
    fn next(&mut self, wait: Duration) -> Option<Vec<AgentEvent>> {
        let (dir, line) = self.lines.recv_timeout(wait).ok()?;
        if dir == "stderr" {
            self.record("stderr", json!(line));
            return Some(vec![]);
        }
        let msg: Value = serde_json::from_str(&line).unwrap_or(json!(line));
        self.record("in", msg);
        let decoded = self.adapter.decode(&line);
        for reply in decoded.replies {
            self.write(&reply);
        }
        for event in &decoded.events {
            if let AgentEvent::Session { id } = event {
                self.session = Some(id.clone());
            }
        }
        Some(decoded.events)
    }

    /// Listen until `done` says so, answering what the agent asks; at most three minutes.
    fn until(
        &mut self,
        mut done: impl FnMut(&mut Self, &AgentEvent) -> bool,
    ) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(180);
        while Instant::now() < deadline {
            let Some(events) = self.next(Duration::from_secs(1)) else {
                continue;
            };
            for event in events {
                if let AgentEvent::PermissionRequest {
                    id,
                    input,
                    interactive,
                    suggestions,
                    ..
                } = &event
                {
                    let decision = match (interactive, self.policy) {
                        (true, _) => Decision::Answer(answers(input)),
                        (false, Policy::Allow) => Decision::Allow,
                        (false, Policy::Always) => Decision::AllowAlways(suggestions.clone()),
                        (false, Policy::Deny) => Decision::Deny {
                            message: "Denied by the capture.".into(),
                        },
                    };
                    self.send(AgentCommand::Decide {
                        id: id.clone(),
                        decision,
                    });
                }
                if done(self, &event) {
                    return Ok(());
                }
            }
        }
        Err("the agent took more than three minutes".into())
    }

    fn close(mut self) {
        drop(self.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                self.record("exit", json!(status.code()));
                while let Ok((dir, line)) = self.lines.try_recv() {
                    self.record(if dir == "stderr" { "stderr" } else { "in" }, json!(line));
                }
                return;
            }
            let _ = self.next(Duration::from_millis(200));
        }
        let _ = self.child.kill();
        self.record("exit", json!("killed"));
    }
}

/// The card's answers: each question's first choice.
fn answers(input: &Value) -> Value {
    let mut answers = serde_json::Map::new();
    for q in input["questions"].as_array().into_iter().flatten() {
        let first = q["options"][0]["label"].as_str().unwrap_or("blue");
        answers.insert(
            q["question"].as_str().unwrap_or_default().into(),
            json!(first),
        );
    }
    Value::Object(answers)
}

fn launch(
    agent: &str,
    spec: &SessionSpec,
    out: std::fs::File,
    start: Instant,
    policy: Policy,
) -> Result<Run, String> {
    let adapter = adapter(agent).ok_or("no such agent")?;
    let plan = adapter.launch(spec);
    let mut command = Command::new(&plan.program);
    command
        .args(&plan.args)
        .current_dir(&plan.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &plan.env {
        command.env(key, value);
    }
    for key in &plan.env_remove {
        command.env_remove(key);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("{}: {e}", plan.program))?;
    let (tx, lines) = mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let tx2 = tx.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(("in", line));
        }
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = tx2.send(("stderr", line));
        }
    });
    let stdin = child.stdin.take();
    let mut run = Run {
        adapter,
        child,
        stdin,
        lines,
        out,
        start,
        policy,
        session: None,
    };
    run.record("spec", serde_json::to_value(spec).unwrap());
    run.send(AgentCommand::Start);
    run.until(|_, e| matches!(e, AgentEvent::Ready { .. }))?;
    Ok(run)
}

pub fn run(agent: &str, name: &str, model: Option<&str>) -> Result<(), String> {
    let scenario = scenario(agent, name).ok_or_else(|| format!("no scenario {name}"))?;
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let dir = std::path::PathBuf::from(format!("captures/{agent}-{name}-{epoch}"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let repo = std::env::temp_dir().join(format!("newen-capture-{agent}-{name}-{epoch}"));
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    let _ = Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(&repo)
        .status();
    for (file, words) in scenario.files {
        std::fs::write(repo.join(file), words).map_err(|e| e.to_string())?;
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut spec = SessionSpec {
        cwd: repo.clone(),
        model: model.map(str::to_owned),
        servers: ["sessions", "browser"]
            .map(|server| InjectedServer {
                name: server.into(),
                target: exe.display().to_string(),
                args: vec!["--mcp-stub".into(), server.into()],
                ..Default::default()
            })
            .into(),
        ..Default::default()
    };
    let path = dir.join("session.jsonl");
    let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let start = Instant::now();
    let mut run = launch(
        agent,
        &spec,
        file.try_clone().unwrap(),
        start,
        scenario.policy,
    )?;
    let mut prompts = 0;
    for step in scenario.steps {
        match step {
            Step::Prompt(text) | Step::Interrupt(text) => {
                prompts += 1;
                let stop = matches!(step, Step::Interrupt(_));
                run.send(AgentCommand::Prompt {
                    request: format!("r{prompts}"),
                    text: text.into(),
                    steer: false,
                    images: vec![],
                });
                let mut stopped = false;
                run.until(|run, e| {
                    let started = matches!(
                        e,
                        AgentEvent::TextDelta { .. }
                            | AgentEvent::ToolUse { .. }
                            | AgentEvent::ThinkingDelta { .. }
                    );
                    if stop && started && !stopped {
                        stopped = true;
                        run.send(AgentCommand::Interrupt);
                    }
                    matches!(e, AgentEvent::TurnFinished { .. })
                })?;
            }
            // A mode the agent takes only at launch starts it again in it, as the engine does.
            Step::Mode(mode) if run.adapter.launch_mode(mode) => {
                let session = run.session.clone();
                run.close();
                spec = SessionSpec {
                    resume: session,
                    mode: Some(mode.into()),
                    ..spec.clone()
                };
                run = launch(
                    agent,
                    &spec,
                    file.try_clone().unwrap(),
                    start,
                    scenario.policy,
                )?;
            }
            Step::Mode(mode) => {
                run.send(AgentCommand::SetMode(mode.into()));
                run.until(|_, e| matches!(e, AgentEvent::ModeChanged { .. }))?;
            }
            Step::Restart { fork } => {
                let session = run.session.clone();
                run.close();
                let spec = SessionSpec {
                    resume: session,
                    fork,
                    ..spec.clone()
                };
                run = launch(
                    agent,
                    &spec,
                    file.try_clone().unwrap(),
                    start,
                    scenario.policy,
                )?;
            }
        }
    }
    run.close();
    drop(file);
    // The same paths on every machine.
    let home = std::env::var("HOME").unwrap_or_default();
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut text = text
        .replace(&repo.display().to_string(), "/work")
        .replace(&exe.display().to_string(), "/bin/newen");
    if !home.is_empty() {
        text = text.replace(&home, "/home/user");
    }
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    println!("{}", path.display());
    Ok(())
}

/// `--mcp-stub <server>`: Newen's MCP server for a capture, each call answered with a line.
pub fn stub(server: &str) {
    let tools: &[(&str, &str, &str)] = if server == "browser" {
        &BROWSER_TOOLS
    } else {
        &HOST_TOOLS
    };
    let stdout = std::io::stdout();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let result = match msg["method"].as_str() {
            Some("initialize") => json!({ "protocolVersion": msg["params"]["protocolVersion"],
                "capabilities": { "tools": {} }, "serverInfo": { "name": server, "version": "capture" } }),
            Some("tools/list") => json!({ "tools": tools.iter().map(|(name, about, schema)| json!({
                "name": name, "description": about,
                "inputSchema": serde_json::from_str::<Value>(schema).unwrap_or_default() })).collect::<Vec<_>>() }),
            Some("tools/call") => {
                let said = match msg["params"]["name"].as_str() {
                    Some("list_sessions") => "No other sessions in this project.\n\nAgents a session can be started with: opencode (you), hermes, claude, codex".to_owned(),
                    Some(name) => format!("{name} done"),
                    None => String::new(),
                };
                json!({ "content": [{ "type": "text", "text": said }], "isError": false })
            }
            _ => json!({}),
        };
        let mut out = stdout.lock();
        let _ = writeln!(
            out,
            "{}",
            json!({ "jsonrpc": "2.0", "id": id, "result": result })
        );
        let _ = out.flush();
    }
}
