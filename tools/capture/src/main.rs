//! Records what the installed `claude` CLI says over stream-json, scenario by scenario
//! (docs/plan.md Q1.2, docs/engine.md §11.3). Each run gets a fresh git repo and writes
//! `captures/<scenario>-<epoch>/session.jsonl`: one `{t_ms, dir, msg}` per line, `dir` being
//! `out` (Newen → CLI), `in` (CLI → Newen) or `stderr`.
//!
//! Usage: newen-capture [--agent codex|opencode|hermes|grok|antigravity] <scenario> [--model <m>] [--settings <sources>] [--resume <session-id>] [--fork]
//! Scenarios: simple, edit-bash, deny, always, ask, plan, steer, interrupt, resume, rewind,
//! todos, agent, think, workflow, suggest, remote (by hand, with a phone: `touch <capture>/stop` ends
//! it). With `--agent codex`, see codex.rs; with `--agent opencode`, `hermes`, `grok` or `antigravity`, acp.rs.

mod acp;
mod codex;
mod model;

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How a scenario answers `can_use_tool`.
#[derive(Clone, Copy, PartialEq)]
enum Policy {
    Allow,
    /// Allow and accept the CLI's own `permission_suggestions` as rules.
    AllowAlways,
    Deny,
    /// Leave it unanswered: another client (a phone over Remote Control) answers it.
    Hold,
}

/// What a scenario does, in order.
enum Step {
    /// Send a prompt and wait for its turn's `result`.
    Prompt(&'static str),
    /// Send a prompt; at its first streamed delta send `then` with `priority`, then wait for every `result`.
    Steer {
        prompt: &'static str,
        then: &'static str,
        priority: &'static str,
    },
    /// Send a prompt; at its first streamed delta send `interrupt`, then wait for the `result`.
    Interrupt(&'static str),
    /// `rewind_files` dry run to before the first prompt of this run.
    RewindDryRun,
    /// Send this control request (its JSON) and wait for its answer.
    Control(&'static str),
    /// Only listen, answering what the CLI asks, for at most this many seconds or until a file
    /// `stop` appears in the capture's folder: someone acts from another device meanwhile.
    Listen(u64),
}

struct Scenario {
    policy: Policy,
    mode: &'static str,
    steps: Vec<Step>,
    /// More CLI flags and environment the scenario needs.
    flags: &'static [&'static str],
    env: &'static [(&'static str, &'static str)],
    /// Switches the scenario turns on in `initialize`.
    init: &'static [&'static str],
}

fn scenario(name: &str) -> Option<Scenario> {
    use Step::*;
    let (policy, mode, steps) = match name {
        "simple" => (Policy::Allow, "default", vec![Prompt("Reply with exactly the word: ok")]),
        "edit-bash" => (
            Policy::Allow,
            "default",
            vec![Prompt(
                "Create notes.txt containing the line hello. Then edit it so the line reads hello world. \
                 Then, with the Bash tool, run exactly: mv notes.txt moved.txt && rm moved.txt",
            )],
        ),
        "deny" => (Policy::Deny, "default", vec![Prompt("Use the Bash tool to run exactly: touch denied.txt")]),
        "always" => (
            Policy::AllowAlways,
            "default",
            vec![Prompt("Use the Bash tool to run exactly: touch one.txt. Then run exactly: touch two.txt")],
        ),
        "ask" => (
            Policy::Allow,
            "default",
            vec![Prompt(
                "Use the AskUserQuestion tool to ask me to pick red or blue, then reply with my answer only.",
            )],
        ),
        "plan" => (
            Policy::Allow,
            "plan",
            vec![Prompt("Make a two-step plan to add a line 'quill' to README.md, then exit plan mode and do it.")],
        ),
        "steer" => (
            Policy::Allow,
            "default",
            vec![Steer {
                prompt: "Count from 1 to 40, one number per line, nothing else.",
                then: "Change of plans: stop at 5.",
                priority: "next",
            }],
        ),
        "interrupt" => (Policy::Allow, "default", vec![Interrupt("Write a 500 word story about a quill.")]),
        "resume" => (Policy::Allow, "default", vec![Prompt("What was my previous request? One sentence.")]),
        "rewind" => (
            Policy::Allow,
            "acceptEdits",
            vec![Prompt("Create rewind.txt containing the line one."), RewindDryRun],
        ),
        "todos" => (
            Policy::Allow,
            "default",
            vec![Prompt(
                "Use the task tools to list three steps: create a.txt, create b.txt, list the files. \
                 Then do each step with the Bash tool, updating the todos as you go.",
            )],
        ),
        "agent" => (
            Policy::Allow,
            "default",
            vec![Prompt(
                "Use the Agent tool to launch one general-purpose subagent that reads README.md and \
                 runs `ls`, and reports what it found in one sentence. Then tell me its report.",
            )],
        ),
        "think" => (Policy::Allow, "default", vec![Prompt("Think it through, then answer in one line: is 391 prime?")]),
        // A background workflow reports its phases and agents after the turn ends; listen until
        // its notification and the turn that follows (`touch <capture>/stop` ends it sooner).
        "workflow" => (
            Policy::Allow,
            "default",
            vec![
                Prompt(
                    "Run a workflow with the Workflow tool, named file-words, in two phases: 'Read' with \
                     two agents, one reading README.md and one listing the files with ls, each \
                     answering in one word; then 'Sum up' with one agent joining their answers in one \
                     sentence. Then wait for it and tell me its sentence.",
                ),
                Listen(600),
            ],
        ),
        // The suggestion comes after a turn's `result`, so each turn is listened past.
        "suggest" => (
            Policy::Allow,
            "default",
            vec![
                Prompt("Reply with exactly the word: ok"),
                Listen(10),
                Prompt("Write a one-line Python function that reverses a string. Reply with only the code."),
                Listen(45),
            ],
        ),
        // Run by hand with a phone: the phone sends a message and answers a permission while
        // this listens (docs/remote-control.md).
        "remote" => (
            Policy::Hold,
            "default",
            vec![
                Prompt("Reply with exactly the word: ok"),
                Control(r#"{"subtype":"remote_control","enabled":true,"name":"quill capture"}"#),
                Listen(1200),
                Control(r#"{"subtype":"remote_control","enabled":false}"#),
                Listen(10),
            ],
        ),
        _ => return None,
    };
    let flags: &[&str] = match name {
        "think" => &["--effort", "high", "--thinking-display", "summarized"],
        // Whether the phone's own messages come back on stdout with it.
        "remote" => &["--replay-user-messages"],
        _ => &[],
    };
    // The CLI offers its task tools (TaskCreate, TaskUpdate…) only with these set.
    let env: &[(&str, &str)] = match name {
        "todos" => &[
            ("CLAUDE_CODE_ENABLE_TASKS", "true"),
            ("CLAUDE_CODE_ENABLE_TODO_TOOLS", "true"),
        ],
        _ => &[],
    };
    let init: &[&str] = match name {
        "suggest" => &["promptSuggestions"],
        _ => &[],
    };
    Some(Scenario {
        policy,
        mode,
        steps,
        flags,
        env,
        init,
    })
}

/// One line of the capture file.
struct Log {
    start: Instant,
    file: Mutex<std::fs::File>,
}

impl Log {
    fn write(&self, dir: &str, msg: Value) {
        let line =
            json!({ "t_ms": self.start.elapsed().as_millis() as u64, "dir": dir, "msg": msg });
        let mut file = self.file.lock().unwrap();
        let _ = writeln!(file, "{line}");
    }
}

struct Cli {
    stdin: ChildStdin,
    log: Arc<Log>,
    next_request: u64,
}

impl Cli {
    fn send(&mut self, msg: Value) {
        self.log.write("out", msg.clone());
        writeln!(self.stdin, "{msg}").expect("CLI stdin closed");
        self.stdin.flush().expect("CLI stdin closed");
    }

    fn request(&mut self, request: Value) -> String {
        self.next_request += 1;
        let id = format!("q_{}", self.next_request);
        self.send(json!({ "type": "control_request", "request_id": id, "request": request }));
        id
    }

    fn prompt(&mut self, text: &str, priority: Option<&str>) -> String {
        let uuid = uuid::Uuid::new_v4().to_string();
        let mut msg = json!({
            "type": "user", "uuid": uuid, "session_id": "", "parent_tool_use_id": null,
            "message": { "role": "user", "content": [{ "type": "text", "text": text }] },
        });
        if let Some(priority) = priority {
            msg["priority"] = json!(priority);
        }
        self.send(msg);
        uuid
    }
}

/// The answer to one `control_request` coming from the CLI.
fn answer(request: &Value, policy: Policy) -> Value {
    let subtype = request["subtype"].as_str().unwrap_or("");
    match subtype {
        "can_use_tool" => {
            let tool_use_id = request["tool_use_id"].clone();
            let mut input = request["input"].clone();
            if request["tool_name"] == "AskUserQuestion" {
                // V5: the shape of the answer is what this capture verifies.
                let answers: serde_json::Map<String, Value> = input["questions"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|q| q["question"].as_str())
                    .map(|q| (q.to_owned(), json!("blue")))
                    .collect();
                input["answers"] = Value::Object(answers);
            }
            match policy {
                Policy::Deny => {
                    json!({ "behavior": "deny", "message": "Denied by the capture scenario.", "toolUseID": tool_use_id })
                }
                Policy::Allow => {
                    json!({ "behavior": "allow", "updatedInput": input, "toolUseID": tool_use_id })
                }
                Policy::AllowAlways => json!({
                    "behavior": "allow", "updatedInput": input, "toolUseID": tool_use_id,
                    "updatedPermissions": request["permission_suggestions"].clone(),
                }),
                // Never sent: `reply` leaves a held request unanswered.
                Policy::Hold => Value::Null,
            }
        }
        // PostToolUse, registered in `initialize`: an empty output lets the tool's result stand.
        "hook_callback" => json!({}),
        _ => Value::Null,
    }
}

fn run(
    name: &str,
    model: &str,
    settings: &str,
    resume: Option<&str>,
    fork: bool,
) -> Result<(), String> {
    let scenario = scenario(name).ok_or_else(|| format!("unknown scenario {name}"))?;
    let epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let dir = std::path::PathBuf::from(format!("captures/{name}-{epoch}"));
    let repo = dir.join("repo");
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    std::fs::write(repo.join("README.md"), "# capture\n").map_err(|e| e.to_string())?;
    for args in [
        &["init", "-q"][..],
        &["add", "-A"],
        &[
            "-c",
            "user.name=newen",
            "-c",
            "user.email=newen@localhost",
            "commit",
            "-qm",
            "init",
        ],
    ] {
        Command::new("git")
            .args(args)
            .current_dir(&repo)
            .status()
            .map_err(|e| e.to_string())?;
    }

    // Without the user's own settings by default: their allow rules and hooks would change
    // what the CLI asks, and their hooks would put personal data in the capture.
    let sources = format!("--setting-sources={settings}");
    let mut args: Vec<String> = [
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "stream-json",
        "--permission-prompt-tool",
        "stdio",
        "--include-partial-messages",
        &sources,
        "--permission-mode",
        scenario.mode,
        "--model",
        model,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.extend(scenario.flags.iter().map(|s| s.to_string()));
    if let Some(id) = resume {
        args.push(format!("--resume={id}"));
        if fork {
            args.push("--fork-session".into());
        }
    }

    let log = Arc::new(Log {
        start: Instant::now(),
        file: Mutex::new(
            std::fs::File::create(dir.join("session.jsonl")).map_err(|e| e.to_string())?,
        ),
    });
    let version = Command::new("claude")
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;
    let meta = json!({
        "scenario": name, "cli_version": String::from_utf8_lossy(&version.stdout).trim(),
        "args": args, "model": model, "epoch": epoch,
    });
    std::fs::write(
        dir.join("meta.json"),
        serde_json::to_string_pretty(&meta).unwrap(),
    )
    .map_err(|e| e.to_string())?;

    let mut child: Child = Command::new("claude")
        .args(&args)
        .current_dir(&repo)
        .env_remove("NODE_OPTIONS")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .env("CLAUDE_CODE_ENABLE_ASK_USER_QUESTION_TOOL", "true")
        .env("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING", "true")
        .env("CLAUDE_CODE_EMIT_TOOL_USE_SUMMARIES", "false")
        .env("CLAUDE_CODE_DISABLE_TERMINAL_TITLE", "1")
        .envs(scenario.env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn claude: {e}"))?;

    let (tx, rx) = mpsc::channel::<Value>();
    let stdout = child.stdout.take().unwrap();
    let out_log = log.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let msg = serde_json::from_str(&line).unwrap_or(Value::String(line));
            out_log.write("in", msg.clone());
            if tx.send(msg).is_err() {
                break;
            }
        }
    });
    let stderr = child.stderr.take().unwrap();
    let err_log = log.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            err_log.write("stderr", Value::String(line));
        }
    });

    let mut cli = Cli {
        stdin: child.stdin.take().unwrap(),
        log: log.clone(),
        next_request: 0,
    };
    let mut init = json!({
        "subtype": "initialize",
        "hooks": { "PostToolUse": [{ "hookCallbackIds": ["hook_0"] }] },
    });
    for switch in scenario.init {
        init[*switch] = json!(true);
    }
    let init = cli.request(init);
    wait(&rx, &mut cli, scenario.policy, |msg| {
        control_response_for(msg, &init)
    })?;

    let mut first_prompt: Option<String> = None;
    for step in &scenario.steps {
        match step {
            Step::Prompt(text) => {
                let uuid = cli.prompt(text, None);
                first_prompt.get_or_insert(uuid);
                wait(&rx, &mut cli, scenario.policy, is_turn_result)?;
            }
            Step::Steer {
                prompt,
                then,
                priority,
            } => {
                first_prompt.get_or_insert(cli.prompt(prompt, None));
                wait(&rx, &mut cli, scenario.policy, is_text_delta)?;
                cli.prompt(then, Some(priority));
                wait(&rx, &mut cli, scenario.policy, is_turn_result)?;
                // A steered message may run as a turn of its own; wait for it too, briefly.
                let _ = wait_for(
                    &rx,
                    &mut cli,
                    scenario.policy,
                    is_turn_result,
                    Duration::from_secs(20),
                );
            }
            Step::Interrupt(text) => {
                first_prompt.get_or_insert(cli.prompt(text, None));
                wait(&rx, &mut cli, scenario.policy, is_text_delta)?;
                let id = cli.request(json!({ "subtype": "interrupt" }));
                wait(&rx, &mut cli, scenario.policy, |msg| {
                    control_response_for(msg, &id)
                })?;
                wait(&rx, &mut cli, scenario.policy, is_turn_result)?;
            }
            Step::RewindDryRun => {
                let target = first_prompt
                    .clone()
                    .ok_or("rewind needs an earlier prompt")?;
                let id = cli.request(json!({ "subtype": "rewind_files", "user_message_id": target, "dry_run": true }));
                wait(&rx, &mut cli, scenario.policy, |msg| {
                    control_response_for(msg, &id)
                })?;
            }
            Step::Control(request) => {
                let request: Value = serde_json::from_str(request).map_err(|e| e.to_string())?;
                let id = cli.request(request);
                wait(&rx, &mut cli, scenario.policy, |msg| {
                    control_response_for(msg, &id)
                })?;
            }
            Step::Listen(seconds) => {
                let (stop, deadline) = (
                    dir.join("stop"),
                    Instant::now() + Duration::from_secs(*seconds),
                );
                while Instant::now() < deadline && !stop.exists() {
                    match rx.recv_timeout(Duration::from_millis(500)) {
                        Ok(msg) => reply(&mut cli, &msg, scenario.policy),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                // `stop` ends this listen only; the next one runs its full time.
                let _ = std::fs::remove_file(&stop);
            }
        }
    }

    drop(cli);
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    log.write("exit", json!({ "code": status }));
    println!("{}", dir.display());
    Ok(())
}

fn is_turn_result(msg: &Value) -> bool {
    msg["type"] == "result" && msg["parent_tool_use_id"].is_null()
}

fn is_text_delta(msg: &Value) -> bool {
    msg["type"] == "stream_event" && msg["event"]["delta"]["type"] == "text_delta"
}

fn control_response_for(msg: &Value, id: &str) -> bool {
    msg["type"] == "control_response" && msg["response"]["request_id"] == id
}

/// Read until `done`, answering the CLI's own control requests on the way.
fn wait(
    rx: &Receiver<Value>,
    cli: &mut Cli,
    policy: Policy,
    done: impl Fn(&Value) -> bool,
) -> Result<(), String> {
    wait_for(rx, cli, policy, done, Duration::from_secs(240))
}

fn wait_for(
    rx: &Receiver<Value>,
    cli: &mut Cli,
    policy: Policy,
    done: impl Fn(&Value) -> bool,
    limit: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + limit;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let msg = rx.recv_timeout(left).map_err(|e| format!("waiting: {e}"))?;
        reply(cli, &msg, policy);
        if done(&msg) {
            return Ok(());
        }
    }
}

/// Answer `msg` if it is a control request of the CLI's.
fn reply(cli: &mut Cli, msg: &Value, policy: Policy) {
    if msg["type"] != "control_request"
        || (policy == Policy::Hold && msg["request"]["subtype"] == "can_use_tool")
    {
        return;
    }
    let id = msg["request_id"].clone();
    let response = answer(&msg["request"], policy);
    let reply = if response.is_null() {
        json!({ "subtype": "error", "request_id": id, "error": "not supported by newen-capture" })
    } else {
        json!({ "subtype": "success", "request_id": id, "response": response })
    };
    cli.send(json!({ "type": "control_response", "response": reply }));
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [flag, server] = &args[..] {
        if flag == "--mcp-stub" {
            return acp::stub(server);
        }
    }
    // The fake model alone, for trying an agent by hand (in the app, say): `--serve-model <port> <scenario>`.
    if let [flag, port, scenario] = &args[..] {
        if flag == "--serve-model" {
            model::serve(port.parse().expect("a port"), scenario.clone());
            loop {
                std::thread::park();
            }
        }
    }
    if let [flag, agent, name, ..] = &args[..] {
        if flag == "--agent" && acp::is_acp(agent) {
            let flag = |f: &str| {
                (args.iter().position(|a| a == f))
                    .and_then(|i| args.get(i + 1))
                    .map(String::as_str)
            };
            let model = flag("--model");
            if let Some(port) = flag("--fake-model").and_then(|p| p.parse().ok()) {
                model::serve(port, name.clone());
            }
            if let Err(error) = acp::run(agent, name, model) {
                eprintln!("newen-capture: {error}");
                std::process::exit(1);
            }
            return;
        }
        if flag == "--agent" && agent == "codex" {
            if let Err(error) = codex::run(name) {
                eprintln!("newen-capture: {error}");
                std::process::exit(1);
            }
            return;
        }
    }
    let Some(name) = args.first() else {
        eprintln!("usage: newen-capture <scenario> [--model <m>] [--settings <sources>] [--resume <session-id>] [--fork]");
        std::process::exit(2);
    };
    let flag = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
    };
    let model = flag("--model").unwrap_or("haiku");
    if let Err(error) = run(
        name,
        model,
        flag("--settings").unwrap_or("project,local"),
        flag("--resume"),
        args.iter().any(|a| a == "--fork"),
    ) {
        eprintln!("newen-capture: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_documented_scenario_exists() {
        for name in [
            "simple",
            "edit-bash",
            "deny",
            "always",
            "ask",
            "plan",
            "steer",
            "interrupt",
            "resume",
            "rewind",
            "suggest",
        ] {
            assert!(scenario(name).is_some(), "{name}");
        }
    }

    #[test]
    fn deny_and_allow_answers_carry_the_tool_use_id() {
        let request = json!({ "subtype": "can_use_tool", "tool_name": "Bash", "input": { "command": "echo hi" }, "tool_use_id": "t1" });
        assert_eq!(answer(&request, Policy::Deny)["behavior"], "deny");
        let allow = answer(&request, Policy::Allow);
        assert_eq!(
            (allow["behavior"].as_str(), allow["toolUseID"].as_str()),
            (Some("allow"), Some("t1"))
        );
    }

    #[test]
    fn ask_user_question_is_answered_per_question() {
        let request = json!({ "subtype": "can_use_tool", "tool_name": "AskUserQuestion", "tool_use_id": "t2",
            "input": { "questions": [{ "question": "Red or blue?" }] } });
        assert_eq!(
            answer(&request, Policy::Allow)["updatedInput"]["answers"]["Red or blue?"],
            "blue"
        );
    }
}
