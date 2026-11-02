//! Every test replays a capture of the real CLI (`fixtures/0.163.0/`, docs/codex.md "Fase 1").
//!
//! A capture is driven the way the engine would: the lines quill sent become the commands
//! that make the adapter write them, and the lines Codex sent are decoded. What the adapter
//! writes must be the capture's `out` lines, in number and order, because the fake agent
//! waits for one line per `out`.

use super::*;
use newen_agent::AgentEvent::*;
use std::path::{Path, PathBuf};

fn entries(name: &str) -> Vec<(String, Value)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/0.163.0/{name}.jsonl"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    (text.lines())
        .map(|line| {
            let entry: Value = serde_json::from_str(line).unwrap();
            (text_of(&entry["dir"]), entry["msg"].clone())
        })
        .collect()
}

fn text_of(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// One process of the adapter: what it wrote and what it told.
struct Run {
    codex: Codex,
    written: Vec<Value>,
    events: Vec<AgentEvent>,
    prompts: u32,
}

impl Run {
    fn new(spec: SessionSpec) -> Self {
        let codex = Codex::new();
        codex.launch(&SessionSpec {
            cwd: PathBuf::from("/work"),
            model: Some("gpt-6-luna".into()),
            effort: Some("low".into()),
            ..spec
        });
        Run {
            codex,
            written: vec![],
            events: vec![],
            prompts: 0,
        }
    }

    fn started(spec: SessionSpec) -> Self {
        let mut run = Self::new(spec);
        run.send(AgentCommand::Start);
        run
    }

    fn write(&mut self, lines: Vec<String>) {
        self.written.extend(
            lines
                .iter()
                .map(|l| serde_json::from_str::<Value>(l).unwrap()),
        );
    }

    fn send(&mut self, command: AgentCommand) {
        let lines = self.codex.encode(command);
        self.write(lines);
    }

    fn feed(&mut self, msg: &Value) {
        let decoded = self.codex.decode(&msg.to_string());
        self.events.extend(decoded.events);
        self.write(decoded.replies);
    }

    fn prompt(&mut self, text: &str, steer: bool) {
        self.prompts += 1;
        self.send(AgentCommand::Prompt {
            request: format!("r{}", self.prompts),
            text: text.into(),
            steer,
            images: vec![],
        });
    }

    /// The decision the capture answered a server request with.
    fn answer(&mut self, id: &Value, result: &Value) {
        let asked = self.events.iter().rev().find_map(|e| match e {
            PermissionRequest { id: p, input, .. } if *p == key(id) => Some(input.clone()),
            _ => None,
        });
        let decision = if let Some(answers) = result["answers"].as_object() {
            // One question, as the capture asked (ask.jsonl:30).
            let question = text_of(&asked.unwrap()["questions"][0]["question"]);
            let picked = answers.values().next().unwrap()["answers"][0].clone();
            Decision::Answer(json!({ question: picked }))
        } else {
            match result["decision"].as_str() {
                Some("acceptForSession") => Decision::AllowAlways(vec![]),
                Some("decline") => Decision::Deny {
                    message: String::new(),
                },
                _ => Decision::Allow,
            }
        };
        self.send(AgentCommand::Decide {
            id: key(id),
            decision,
        });
    }
}

/// Replay a capture of one process (up to an `exit`, from `from`); returns where it ended.
fn replay(run: &mut Run, entries: &[(String, Value)], from: usize) -> usize {
    let mut at = from;
    while at < entries.len() {
        let (dir, msg) = &entries[at];
        at += 1;
        match (dir.as_str(), msg["method"].as_str()) {
            ("exit", _) => return at,
            ("in", _) => run.feed(msg),
            ("out", Some("turn/start")) => {
                run.prompt(msg["params"]["input"][0]["text"].as_str().unwrap(), false)
            }
            ("out", Some("turn/steer")) => {
                run.prompt(msg["params"]["input"][0]["text"].as_str().unwrap(), true)
            }
            ("out", Some("turn/interrupt")) => run.send(AgentCommand::Interrupt),
            // A fork inside one process: quill forks by launching a new one (below).
            // Asked by the capture's driver, not by quill: its answer is for no one.
            ("out", Some("thread/compact/start" | "review/start")) => {
                run.codex.pending.insert(key(&msg["id"]), Pending::Ignore);
            }
            ("out", Some("thread/fork")) => {
                run.codex.next_id = msg["id"].as_u64().unwrap();
                run.codex.pending.insert(key(&msg["id"]), Pending::Thread);
            }
            // Start, `initialized`, the thread, the turn list: the adapter writes them itself.
            ("out", Some(_)) => {}
            ("out", None) => run.answer(&msg["id"], &msg["result"]),
            _ => {}
        }
    }
    at
}

fn methods(lines: &[Value]) -> Vec<String> {
    (lines.iter())
        .map(|l| l["method"].as_str().unwrap_or("response").to_owned())
        .collect()
}

/// The `out` lines of a capture, as the adapter must write them: the same methods, ids and
/// answers.
fn out_lines(entries: &[(String, Value)]) -> Vec<Value> {
    (entries.iter())
        .filter(|(dir, _)| dir == "out")
        .map(|(_, msg)| msg.clone())
        .collect()
}

/// Replay `name` as one process and check the adapter wrote the capture's `out` lines.
fn run_of(name: &str) -> Run {
    let entries = entries(name);
    let mut run = Run::started(SessionSpec::default());
    replay(&mut run, &entries, 0);
    // A fork asks the thread of the process it is made in: not a line of this run.
    let expected: Vec<Value> = out_lines(&entries)
        .into_iter()
        // The adapter has no command for these (compact, review): the capture's driver asked.
        .filter(|l| {
            !matches!(
                l["method"].as_str(),
                Some("thread/fork" | "thread/compact/start" | "review/start")
            )
        })
        .collect();
    assert_eq!(
        methods(&run.written),
        methods(&expected),
        "{name}: lines written"
    );
    for (written, expected) in run.written.iter().zip(&expected) {
        assert_eq!(written["id"], expected["id"], "{name}: ids");
        // Only the request's identity and the answers are the capture's; the adapter sends more
        // parameters (policy, config) than the capture's driver did.
        if expected.get("result").is_some() {
            assert_eq!(written["result"], expected["result"], "{name}: answers");
        }
    }
    run
}

fn unknown(events: &[AgentEvent]) -> Vec<&Value> {
    (events.iter())
        .filter_map(|e| if let Unknown(v) = e { Some(v) } else { None })
        .collect()
}

fn calls(events: &[AgentEvent]) -> Vec<(&str, &Value)> {
    (events.iter())
        .filter_map(|e| match e {
            ToolUse { name, input, .. } => Some((name.as_str(), input)),
            _ => None,
        })
        .collect()
}

fn results(events: &[AgentEvent]) -> Vec<&AgentEvent> {
    events
        .iter()
        .filter(|e| matches!(e, ToolResult { .. }))
        .collect()
}

fn finished(events: &[AgentEvent]) -> Vec<(bool, bool)> {
    (events.iter())
        .filter_map(|e| match e {
            TurnFinished {
                ok, interrupted, ..
            } => Some((*ok, *interrupted)),
            _ => None,
        })
        .collect()
}

fn texts(events: &[AgentEvent]) -> Vec<&str> {
    (events.iter())
        .filter_map(|e| match e {
            Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

const SCENARIOS: [&str; 20] = [
    "simple",
    "edit-bash",
    "deny-patch",
    "deny",
    "always",
    "plan",
    "plan-mode",
    "steer",
    "interrupt",
    "fork",
    "think",
    "websearch",
    "ask",
    "resume",
    "rewind",
    "subagent",
    "image",
    "view-sleep",
    "compact",
    "review",
];

#[test]
fn every_fixture_writes_its_out_lines_and_decodes_without_unknown_messages() {
    for name in SCENARIOS {
        if matches!(name, "resume" | "fork" | "rewind") {
            continue; // two processes: their own tests
        }
        let run = run_of(name);
        assert_eq!(unknown(&run.events), Vec::<&Value>::new(), "{name}");
        // A compaction runs a turn of its own (compact.jsonl); a plan is carried out in another.
        let turns = if matches!(name, "compact" | "plan-mode") {
            2
        } else {
            1
        };
        assert_eq!(finished(&run.events).len(), turns, "{name}");
    }
}

#[test]
fn a_session_starts_with_its_id_and_what_codex_can_do() {
    let run = run_of("simple");
    assert!(
        matches!(&run.events[0], Session { id } if id == "01a1817c-3a5b-79d0-ae92-0f25db9b7c42")
    );
    let Ready {
        version,
        capabilities,
        mode,
    } = &run.events[1]
    else {
        panic!("{:?}", run.events[1]);
    };
    assert_eq!(version, "0.163.0");
    assert_eq!(mode.as_deref(), Some("auto"));
    assert_eq!(capabilities.modes, ["read-only", "auto", "plan"]);
    assert_eq!(capabilities.unsafe_modes, ["full-access"]);
    assert!(
        capabilities.steer && capabilities.resume && capabilities.fork && !capabilities.tool_hooks
    );
    // model/list's own order (account.jsonl), the thread's model among them.
    assert_eq!(capabilities.models.len(), 8);
    assert!(capabilities.models.iter().any(|m| m.id == "gpt-6-luna"));
    assert_eq!(texts(&run.events), ["ok"]);
    let streamed: Vec<_> = (run.events.iter())
        .filter_map(|e| {
            if let TextDelta { text, .. } = e {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(streamed, ["ok"]);
    assert!(run
        .events
        .iter()
        .any(|e| matches!(e, Running { requests } if requests == &["r1"])));
    assert!(run.events.iter().any(|e| matches!(e, Tokens { tokens: 5 })));
    assert!(run
        .events
        .iter()
        .any(|e| matches!(e, ContextUsage(c) if c.max == 258400 && c.used > 21000)));
    assert!(run
        .events
        .iter()
        .any(|e| matches!(e, Limits(l) if l.session.is_some() && l.weekly.is_some())));
    let Some(TurnFinished {
        ok: true,
        usage,
        error: None,
        ..
    }) = run.events.last()
    else {
        panic!("{:?}", run.events.last());
    };
    assert_eq!(
        (
            usage.input_tokens,
            usage.output_tokens,
            usage.context_window
        ),
        (21918, 5, 258400)
    );
}

#[test]
fn every_thread_is_opened_with_its_approval_policy_sandbox_and_config() {
    // The person's own Codex config may be danger-full-access: never inherit it.
    for (mode, approval, sandbox) in [
        (None, "on-request", "workspace-write"),
        (Some("read-only"), "untrusted", "read-only"),
        (Some("plan"), "on-request", "read-only"),
        (Some("full-access"), "never", "danger-full-access"),
    ] {
        let allow = mode == Some("full-access");
        for (resume, fork) in [(None, false), (Some("t1"), false), (Some("t1"), true)] {
            let mut run = Run::started(SessionSpec {
                mode: mode.map(str::to_owned),
                resume: resume.map(str::to_owned),
                fork,
                allow_unsafe: allow,
                ..Default::default()
            });
            let init = entries("simple")[1].1.clone();
            run.feed(&init);
            let thread = run.written.last().unwrap();
            let want = match (resume, fork) {
                (None, _) => "thread/start",
                (_, false) => "thread/resume",
                (_, true) => "thread/fork",
            };
            assert_eq!(thread["method"], want);
            let p = &thread["params"];
            assert_eq!(
                (&p["approvalPolicy"], &p["sandbox"]),
                (&json!(approval), &json!(sandbox))
            );
            assert_eq!(p["cwd"], "/work");
            assert_eq!(p["config"]["tools"]["update_plan"]["enabled"], true);
            assert_eq!(
                p["config"]["features"]["default_mode_request_user_input"],
                true
            );
            assert_eq!(p.get("threadId").is_some(), resume.is_some());
        }
    }
    // Full access is offered only to someone who allowed it.
    let mut run = Run::started(SessionSpec {
        mode: Some("full-access".into()),
        ..Default::default()
    });
    run.feed(&entries("simple")[1].1);
    assert_eq!(
        run.written.last().unwrap()["params"]["sandbox"],
        "workspace-write"
    );
}

#[test]
fn a_prompt_before_the_thread_waits_for_it() {
    let mut run = Run::started(SessionSpec::default());
    run.prompt("hi", false);
    assert_eq!(methods(&run.written), ["initialize"]);
    run.feed(&entries("simple")[1].1);
    run.feed(&entries("simple")[7].1);
    assert_eq!(
        methods(&run.written),
        [
            "initialize",
            "initialized",
            "model/list",
            "thread/start",
            "turn/start"
        ]
    );
    // The thread is told how to work with the user.
    let thread = (run.written.iter()).find(|m| m["method"] == "thread/start");
    assert_eq!(
        thread.unwrap()["params"]["developerInstructions"],
        newen_agent::WORK_GUIDE
    );
    let turn = run.written.last().unwrap();
    assert_eq!(turn["params"]["effort"], "low");
    assert_eq!(turn["params"]["model"], "gpt-6-luna");
    assert_eq!(turn["params"]["approvalPolicy"], "on-request");
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "workspaceWrite");
}

/// quill's own tools reach Codex as tools the client declares, and their calls come back to
/// the engine as `HostTool` (the shapes are the schema's, inferred: no capture).
#[test]
fn quill_s_tools_are_declared_to_a_new_thread_and_answered_through_the_engine() {
    let mut run = Run::started(SessionSpec::default());
    run.feed(&entries("simple")[1].1);
    let thread = (run.written.iter()).find(|m| m["method"] == "thread/start");
    let tools = thread.unwrap()["params"]["dynamicTools"]
        .as_array()
        .unwrap();
    let wait = tools.iter().find(|t| t["name"] == "wait_for_session");
    assert_eq!(
        wait.unwrap()["inputSchema"]["required"],
        json!(["session_id", "then"])
    );
    assert_eq!(tools.len(), newen_agent::HOST_TOOLS.len());
    let start = tools.iter().find(|t| t["name"] == "start_session");
    assert_eq!(
        start.unwrap()["inputSchema"]["properties"]["agent"]["type"],
        "string"
    );
    run.feed(&json!({ "id": 7, "method": "item/tool/call", "params": {
        "threadId": "t1", "turnId": "u1", "callId": "c1", "namespace": null,
        "tool": "wait_for_session", "arguments": { "session_id": 2, "then": "Release" } } }));
    let Some(HostTool { id, name, input }) = run.events.last().cloned() else {
        panic!("{:?}", run.events.last());
    };
    assert_eq!(
        (name.as_str(), &input["session_id"]),
        ("wait_for_session", &json!(2))
    );
    run.send(AgentCommand::HostToolResult {
        id,
        content: ToolContent::text("Waiting."),
        is_error: false,
    });
    assert_eq!(
        run.written.last().unwrap(),
        &json!({ "id": 7, "result": {
            "contentItems": [{ "type": "inputText", "text": "Waiting." }], "success": true } })
    );
    // Its item is drawn as quill's own tool.
    run.feed(
        &json!({ "method": "item/started", "params": { "threadId": "t1", "item": {
        "type": "dynamicToolCall", "id": "c1", "tool": "wait_for_session",
        "arguments": { "session_id": 2, "then": "Release" } } } }),
    );
    assert!(matches!(run.events.last(),
        Some(ToolUse { name, .. }) if name == "sessions__wait_for_session"));
}

#[test]
fn a_mode_a_model_and_an_effort_apply_to_the_next_turn() {
    let mut run = run_of("simple");
    run.send(AgentCommand::SetMode("read-only".into()));
    run.send(AgentCommand::SetModel("gpt-6.1-sol".into()));
    run.send(AgentCommand::SetEffort("high".into()));
    // The mode is told back when Codex answers the request that carried it, by a name that
    // keeps the numbers of the rest; one on its way tells the latest of two.
    assert_eq!(run.written.last().unwrap()["method"], "account/read");
    let id = run.written.last().unwrap()["id"].clone();
    assert_eq!(id, "newen-mode");
    let asked = run.written.len();
    run.send(AgentCommand::SetMode("plan".into()));
    run.send(AgentCommand::SetMode("read-only".into()));
    assert_eq!(run.written.len(), asked);
    run.feed(&json!({ "id": id, "result": {} }));
    assert_eq!(
        run.events.last(),
        Some(&ModeChanged {
            mode: "read-only".into()
        })
    );
    run.prompt("again", false);
    let turn = run.written.last().unwrap();
    assert_eq!(turn["params"]["sandboxPolicy"]["type"], "readOnly");
    assert_eq!(
        (&turn["params"]["model"], &turn["params"]["effort"]),
        (&json!("gpt-6.1-sol"), &json!("high"))
    );
    // Full access is taken: whether the person allows it is the engine's check, at the change.
    run.send(AgentCommand::SetMode("full-access".into()));
    assert_eq!(run.codex.mode, "full-access");
}

#[test]
fn commands_are_bash_reads_or_listings_and_files_are_edits_and_writes() {
    let run = run_of("edit-bash");
    // The commands of the capture, unwrapped from `bash -lc '…'` (edit-bash.jsonl:54, :80).
    let calls = calls(&run.events);
    assert_eq!(
        calls.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        ["Bash", "Write", "Edit", "Bash"]
    );
    assert_eq!(calls[0].1["command"], "cat /home/user/.codex/RTK.md && pwd");
    assert_eq!(
        calls[1].1,
        &json!({ "file_path": "/work/notes.txt", "content": "hello\n" })
    );
    assert_eq!(
        calls[2].1,
        &json!({ "file_path": "/work/notes.txt", "old_string": "hello\n", "new_string": "hello world\n" })
    );
    assert_eq!(calls[3].1["command"], "rtk cat notes.txt");
    let ends = results(&run.events);
    let ToolResult {
        files,
        output,
        is_error,
        command,
        ..
    } = ends[3]
    else {
        unreachable!()
    };
    assert_eq!((output.as_str(), *is_error), ("hello world\n", false));
    assert_eq!(command.as_ref().unwrap().command, "rtk cat notes.txt");
    assert!(files.is_empty());
    let ToolResult { files, .. } = ends[1] else {
        unreachable!()
    };
    assert_eq!(files[0].path, Path::new("/work/notes.txt"));
    assert_eq!(
        (files[0].before.as_deref(), files[0].after.as_deref()),
        (None, Some("hello\n"))
    );
    let ToolResult { files, .. } = ends[2] else {
        unreachable!()
    };
    assert_eq!(
        (files[0].before.as_deref(), files[0].after.as_deref()),
        (Some("hello\n"), Some("hello world\n"))
    );
    assert_eq!(texts(&run.events).len(), 2); // a commentary and the final answer
}

#[test]
fn a_lone_read_or_listing_is_the_tool_the_ui_draws() {
    let run = run_of("deny-patch");
    assert!(calls(&run.events)
        .iter()
        .any(|(n, i)| *n == "Read" && i["file_path"].is_string()));
    let run = run_of("plan");
    assert!(calls(&run.events)
        .iter()
        .any(|(n, i)| *n == "LS" && i["path"] == "."));
    // A search was never captured (docs/codex.md): shaped from the schema.
    let (name, input) = command_tool(&json!({
        "command": "bash -lc 'rg x'", "commandActions": [{ "type": "search", "command": "rg x", "query": "x", "path": null }],
    }));
    assert_eq!((name, input["pattern"].as_str()), ("Grep", Some("x")));
    assert_eq!(shell_line("/bin/bash -lc 'echo '\\''hi'\\'''"), "echo 'hi'");
    assert_eq!(shell_line("ls -la"), "ls -la");
    assert_eq!(
        shell_line(r#"/bin/bash -lc "printf 'hello\\n' > a.txt && echo \"\$HOME\"""#),
        r#"printf 'hello\n' > a.txt && echo "$HOME""#
    );
}

#[test]
fn an_approval_asks_the_card_claude_asks_and_a_decline_closes_the_tool_as_an_error() {
    let run = run_of("edit-bash");
    let asked: Vec<_> = (run.events.iter())
        .filter_map(|e| match e {
            PermissionRequest {
                id,
                call,
                tool,
                input,
                interactive,
                suggestions,
                ..
            } => Some((
                id.as_str(),
                call.as_deref(),
                tool.as_str(),
                input,
                *interactive,
                suggestions.len(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(asked.len(), 2);
    assert_eq!(asked[0].0, "0");
    assert_eq!(
        asked[0].1,
        Some("exec-0a95eab0-d99f-4d36-bc69-f650244e32fd")
    );
    assert_eq!((asked[0].2, asked[1].2), ("Write", "Edit"));
    assert_eq!(asked[1].3["old_string"], "hello\n");
    assert_eq!((asked[0].4, asked[0].5), (false, 1));

    let run = run_of("deny");
    let PermissionRequest { tool, input, .. } = run
        .events
        .iter()
        .find(|e| matches!(e, PermissionRequest { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(
        (tool.as_str(), input["command"].as_str()),
        ("Bash", Some("touch denied.txt"))
    );
    let ToolResult {
        is_error, output, ..
    } = results(&run.events)[0]
    else {
        unreachable!()
    };
    assert_eq!((*is_error, output.as_str()), (true, "Declined"));
    assert_eq!(run.written.last().unwrap()["result"]["decision"], "decline");

    let run = run_of("deny-patch");
    let declined = results(&run.events)
        .into_iter()
        .filter(|e| matches!(e, ToolResult { is_error: true, files, .. } if files.is_empty()))
        .count();
    assert_eq!(declined, 1);
    assert_eq!(finished(&run.events), [(true, false)]);
}

#[test]
fn allowing_always_is_accepting_for_the_session() {
    let run = run_of("always");
    let decisions: Vec<_> = (run.written.iter())
        .filter_map(|l| l["result"]["decision"].as_str())
        .collect();
    assert_eq!(decisions, ["acceptForSession"]);
    // The second `touch` ran without a request of its own (always.jsonl:44 on).
    assert_eq!(results(&run.events).len(), 2);
}

#[test]
fn the_plan_is_a_task_list() {
    let run = run_of("plan");
    let lists: Vec<Vec<String>> = (calls(&run.events).into_iter())
        .filter(|(n, _)| *n == "TodoWrite")
        .map(|(_, i)| {
            i["todos"]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| text_of(&t["status"]))
                .collect()
        })
        .collect();
    assert_eq!(lists.len(), 4);
    assert_eq!(lists[0], ["pending", "pending", "pending"]);
    assert_eq!(lists[1], ["completed", "in_progress", "pending"]);
    assert_eq!(lists[3], ["completed", "completed", "completed"]);
    let first = calls(&run.events)
        .into_iter()
        .find(|(n, _)| *n == "TodoWrite")
        .unwrap()
        .1;
    assert_eq!(first["todos"][0]["content"], "Create a.txt");
}

/// plan-mode.jsonl: a turn in Plan mode asks first (blocking there) and proposes a plan; the
/// person takes it, and the next turn, in Auto, carries it out.
#[test]
fn a_plan_turn_runs_in_codex_plan_mode_and_its_plan_is_the_proposed_plan() {
    let entries = entries("plan-mode");
    let mut run = Run::started(SessionSpec {
        mode: Some("plan".into()),
        ..Default::default()
    });
    // The person chose no effort: the collaboration mode keeps the thread's (the config's).
    run.codex.effort = None;
    let first_end = (entries.iter())
        .position(|(_, m)| m["method"] == "turn/completed")
        .unwrap();
    replay(&mut run, &entries[..=first_end], 0);
    run.codex.effort = Some("low".into());
    run.send(AgentCommand::SetMode("auto".into()));
    run.feed(&json!({ "id": MODE_ID, "result": {} }));
    replay(&mut run, &entries, first_end + 1);
    let turns: Vec<&Value> = (run.written.iter())
        .filter(|l| l["method"] == "turn/start")
        .collect();
    let mode = |turn: &Value| turn["params"]["collaborationMode"].clone();
    assert_eq!(
        mode(turns[0]),
        json!({ "mode": "plan", "settings": { "model": "gpt-6-luna", "reasoning_effort": "max",
            "developer_instructions": null } })
    );
    assert_eq!(turns[0]["params"]["sandboxPolicy"]["type"], "readOnly");
    assert_eq!(mode(turns[1])["mode"], "default");
    assert_eq!(mode(turns[1])["settings"]["reasoning_effort"], "low");
    assert_eq!(
        turns[1]["params"]["sandboxPolicy"]["type"],
        "workspaceWrite"
    );
    // The capture's own numbers: telling the mode back took none.
    assert_eq!((&turns[0]["id"], &turns[1]["id"]), (&json!(3), &json!(4)));
    assert!(run
        .events
        .iter()
        .any(|e| matches!(e, ModeChanged { mode } if mode == "auto")));
    let plan = calls(&run.events)
        .into_iter()
        .find(|(n, _)| *n == "ExitPlanMode")
        .unwrap()
        .1;
    assert!(plan["plan"]
        .as_str()
        .unwrap()
        .starts_with("## Add `hello.txt`\n\n- Create `hello.txt`"));
    assert!(run.events.iter().any(|e| matches!(e,
        ToolResult { call, is_error: false, .. } if call.ends_with("-plan"))));
    // Plan mode's question waits for the person (plan-mode.jsonl:67).
    assert!(run.events.iter().any(|e| matches!(e,
        PermissionRequest { input, .. } if input["blocking"] == true)));
    assert_eq!(finished(&run.events), [(true, false), (true, false)]);
    assert_eq!(unknown(&run.events), Vec::<&Value>::new());
}

#[test]
fn a_prompt_during_a_turn_steers_it() {
    let run = run_of("steer");
    let steer = run
        .written
        .iter()
        .find(|l| l["method"] == "turn/steer")
        .unwrap();
    assert_eq!(
        steer["params"]["expectedTurnId"],
        "01a1817d-03bc-73c3-bbb9-80cec5061abb"
    );
    assert!(run
        .events
        .iter()
        .any(|e| matches!(e, Running { requests } if requests == &["r1", "r2"])));
    assert_eq!(texts(&run.events).last(), Some(&"1\n2\n3\n4\n5"));
}

#[test]
fn an_interrupt_ends_the_turn_interrupted() {
    let run = run_of("interrupt");
    assert_eq!(run.written.last().unwrap()["method"], "turn/interrupt");
    assert_eq!(finished(&run.events), [(false, true)]);
}

#[test]
fn a_question_is_the_ask_user_question_card_and_its_answer_goes_by_id() {
    let run = run_of("ask");
    let PermissionRequest {
        tool,
        input,
        interactive,
        call,
        ..
    } = run
        .events
        .iter()
        .find(|e| matches!(e, PermissionRequest { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!((tool.as_str(), *interactive), ("AskUserQuestion", true));
    assert_eq!(
        call.as_deref(),
        Some("call_ac2837bc8308424d94a79e98800ec25f")
    );
    let q = &input["questions"][0];
    assert_eq!(q["question"], "Which do you pick: red or blue?");
    assert_eq!(q["header"], "Color");
    assert_eq!(q["options"][1]["label"], "Blue");
    // Codex goes on without an answer in Default mode (ask.jsonl:30); "Other" is allowed.
    assert_eq!(
        (&input["blocking"], &q["other"], &q["secret"]),
        (&json!(false), &json!(true), &json!(false))
    );
    // The card answers by the question's words; Codex wants its id (ask.jsonl:31).
    let answer = run.written.last().unwrap();
    assert_eq!(
        answer["result"],
        json!({ "answers": { "color_choice": { "answers": ["Red"] } } })
    );
    assert_eq!(texts(&run.events), ["Red"]);
}

/// The schema's other questions (no capture has them: the model's own tool always gives options
/// and allows "Other"): one answered in words only, a secret one, and a request that goes
/// unanswered, which leaves every question out.
#[test]
fn a_question_may_be_words_only_secret_or_go_unanswered() {
    let mut run = run_of("simple");
    let ask = |id: u64, blocking: bool| {
        json!({ "id": id, "method": "item/tool/requestUserInput", "params": {
            "threadId": "t", "turnId": "u", "itemId": "c1", "isBlocking": blocking,
            "autoResolutionMs": null, "questions": [
                { "id": "token", "header": "Token", "question": "Paste the API token",
                  "isOther": false, "isSecret": true, "options": null },
                { "id": "color", "header": "Color", "question": "Red or blue?",
                  "isOther": false, "isSecret": false,
                  "options": [{ "label": "Red", "description": "" }, { "label": "Blue", "description": "" }] },
            ] } })
    };
    let asked = |run: &Run| {
        (run.events.iter().rev())
            .find_map(|e| match e {
                PermissionRequest { input, .. } => Some(input.clone()),
                _ => None,
            })
            .unwrap()
    };
    run.feed(&ask(5, true));
    let input = asked(&run);
    assert_eq!(
        input["questions"][0],
        json!({ "question": "Paste the API token", "header": "Token", "options": [],
            "multiSelect": false, "other": false, "secret": true })
    );
    assert_eq!(
        (&input["blocking"], &input["questions"][1]["other"]),
        (&json!(true), &json!(false))
    );
    run.send(AgentCommand::Decide {
        id: "5".into(),
        decision: Decision::Answer(
            json!({ "Paste the API token": "s3cret", "Red or blue?": "Blue" }),
        ),
    });
    assert_eq!(
        run.written.last().unwrap()["result"],
        json!({ "answers": { "token": { "answers": ["s3cret"] }, "color": { "answers": ["Blue"] } } })
    );
    run.feed(&ask(6, false));
    assert_eq!(asked(&run)["blocking"], false);
    run.send(AgentCommand::Decide {
        id: "6".into(),
        decision: Decision::Answer(json!({})),
    });
    assert_eq!(
        run.written.last().unwrap(),
        &json!({ "id": 6, "result": { "answers": {} } })
    );
}

#[test]
fn a_web_search_is_drawn_with_its_sources() {
    let run = run_of("websearch");
    let ToolResult {
        web: Some(web),
        call,
        ..
    } = run
        .events
        .iter()
        .find(|e| matches!(e, ToolResult { web: Some(_), .. }))
        .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(
        web.query,
        "site:blog.rust-lang.org latest stable Rust release announcement"
    );
    assert_eq!(web.sources[0].url, "https://blog.rust-lang.org/releases/");
    assert!(call.starts_with("exec-"));
    let search = calls(&run.events)
        .into_iter()
        .find(|(n, _)| *n == "WebSearch")
        .unwrap();
    assert_eq!(search.1["query"], web.query);
}

#[test]
fn thinking_is_told_from_the_schema_shapes() {
    // `think.jsonl` holds none (docs/codex.md): the shapes come from the schema.
    let mut run = run_of("think");
    assert!(!run
        .events
        .iter()
        .any(|e| matches!(e, Thinking { .. } | ThinkingDelta { .. })));
    for (method, params) in [
        (
            "item/reasoning/summaryTextDelta",
            json!({ "delta": "Let me", "itemId": "r1", "summaryIndex": 0 }),
        ),
        (
            "item/started",
            json!({ "item": { "type": "reasoning", "id": "r1", "summary": [], "content": [] } }),
        ),
        (
            "item/completed",
            json!({ "item": { "type": "reasoning", "id": "r1", "summary": ["Let me think"], "content": [] } }),
        ),
    ] {
        run.feed(&json!({ "method": method, "params": params }));
    }
    let tail = &run.events[run.events.len() - 3..];
    assert_eq!(
        tail[0],
        ThinkingDelta {
            text: "Let me".into(),
            parent: None
        }
    );
    assert_eq!(tail[1], Thinking { parent: None });
    assert_eq!(
        tail[2],
        ThinkingText {
            text: "Let me think".into(),
            parent: None
        }
    );
}

#[test]
fn resume_is_a_second_process_that_starts_over_and_lists_its_turns() {
    let all = entries("resume");
    let mut first = Run::started(SessionSpec::default());
    let at = replay(&mut first, &all, 0);
    let Session { id } = first.events[0].clone() else {
        panic!()
    };
    // The engine keeps the session's id as `spec.resume` and starts the program again.
    let mut second = Run::started(SessionSpec {
        resume: Some(id),
        ..Default::default()
    });
    replay(&mut second, &all, at);
    let written: Vec<Value> = first
        .written
        .iter()
        .chain(&second.written)
        .cloned()
        .collect();
    assert_eq!(methods(&written), methods(&out_lines(&all)));
    for (w, e) in written.iter().zip(out_lines(&all)) {
        assert_eq!(w["id"], e["id"]);
    }
    assert_eq!(second.written[0]["params"]["clientInfo"]["name"], "newen");
    assert_eq!(
        second
            .written
            .iter()
            .find(|l| l["method"] == "thread/resume")
            .unwrap()["params"]["excludeTurns"],
        true
    );
    assert_eq!(texts(&second.events).len(), 1);
    assert!(unknown(&second.events).is_empty() && unknown(&first.events).is_empty());
    assert_eq!(finished(&second.events), [(true, false)]);
}

#[test]
fn a_fork_continues_a_copy_of_the_thread() {
    let all = entries("fork");
    // The capture forks inside the process; quill launches a new one with `fork` set, so the
    // second half is checked on its own, after the first.
    let mut first = Run::started(SessionSpec::default());
    replay(&mut first, &all[..37], 0);
    let Session { id } = first.events[0].clone() else {
        panic!()
    };
    assert_eq!(
        methods(&first.written),
        [
            "initialize",
            "initialized",
            "model/list",
            "thread/start",
            "turn/start"
        ]
    );
    let mut second = Run::started(SessionSpec {
        resume: Some(id.clone()),
        fork: true,
        ..Default::default()
    });
    // Ids as the capture has them from the fork on, where its process had counted four.
    second.feed(&all[1].1);
    assert_eq!(
        methods(&second.written),
        ["initialize", "initialized", "model/list", "thread/fork"]
    );
    assert_eq!(second.written.last().unwrap()["params"]["threadId"], id);
    second.feed(&json!({ "id": 2, "result": all[43].1["result"] }));
    assert!(
        matches!(&second.events[0], Session { id: forked } if forked == "01a18181-020f-72e0-bd68-fab8fef650f4")
    );
    second.prompt("What word did I ask you to remember? One word.", false);
    assert_eq!(
        second.written.last().unwrap()["params"]["threadId"],
        "01a18181-020f-72e0-bd68-fab8fef650f4"
    );
    // The capture's ids are those of a process that counted on; only its notifications apply.
    for (dir, msg) in &all[46..] {
        if dir == "in" && msg["method"].is_string() {
            second.feed(msg);
        }
    }
    assert_eq!(texts(&second.events).len(), 1);
}

const TURN_1: &str = "01a18237-e9ea-7123-b871-11ef7ee57cfa";
const TURN_2: &str = "01a18237-f867-75c3-aca5-457e1896a1f4";

#[test]
fn a_rewind_forks_the_thread_through_the_turn_before_the_message() {
    let all = entries("rewind");
    // The capture's process: two turns, then the fork. Each reply carries its turn's id.
    let mut first = Run::started(SessionSpec::default());
    replay(&mut first, &all[..47], 0);
    let Session { id } = first.events[0].clone() else {
        panic!()
    };
    let ids: Vec<_> = (first.events.iter())
        .filter_map(|e| match e {
            Text { id, .. } => id.as_deref(),
            _ => None,
        })
        .collect();
    assert_eq!(ids, [TURN_1, TURN_2]);
    // What the engine's `rewind_to` sets for "Rewind to here" on the second message.
    let spec = SessionSpec {
        resume: Some(id.clone()),
        fork: true,
        resume_at: Some(TURN_1.into()),
        ..Default::default()
    };
    let mut second = Run::started(spec);
    second.feed(&all[1].1);
    let fork = second.written.last().unwrap();
    assert_eq!(fork["method"], "thread/fork");
    assert_eq!(
        (&fork["params"]["threadId"], &fork["params"]["lastTurnId"]),
        (&json!(id), &json!(TURN_1))
    );
    second.feed(&json!({ "id": 2, "result": all[55].1["result"] }));
    second.prompt(
        "Which words did I ask you to remember? List them, nothing else.",
        false,
    );
    assert_eq!(
        second.written.last().unwrap()["params"]["threadId"],
        "01a18237-fdac-76c3-8316-d223162277c8"
    );
    for (dir, msg) in &all[58..76] {
        if dir == "in" && msg["method"].is_string() {
            second.feed(msg);
        }
    }
    assert_eq!(texts(&second.events), ["lantern"]);
    assert!(unknown(&second.events).is_empty());
    // A resume at a turn that is not a fork keeps the whole thread.
    let mut plain = Run::started(SessionSpec {
        resume: Some(id),
        resume_at: Some(TURN_1.into()),
        ..Default::default()
    });
    plain.feed(&all[1].1);
    let resume = plain.written.last().unwrap();
    assert_eq!(resume["method"], "thread/resume");
    assert!(resume["params"].get("lastTurnId").is_none());
}

#[test]
fn codex_does_not_restore_files_so_quill_does() {
    let run = run_of("simple");
    let Ready { capabilities, .. } = &run.events[1] else {
        panic!()
    };
    assert!(!capabilities.rewind);
}

#[test]
fn account_models_become_the_pickers_and_the_unauthorized_turn_asks_to_sign_in() {
    let account = entries("account");
    let mut run = Run::started(SessionSpec::default());
    run.feed(&account[1].1);
    // model/list answers under its own id, as the adapter asked it.
    let mut models = account[6].1.clone();
    models["id"] = json!(MODELS_ID);
    run.feed(&models);
    run.feed(&entries("simple")[7].1);
    let Ready { capabilities, .. } = run
        .events
        .iter()
        .rev()
        .find(|e| matches!(e, Ready { .. }))
        .unwrap()
    else {
        unreachable!()
    };
    let first = &capabilities.models[0];
    assert_eq!(
        (first.id.as_str(), first.name.as_str()),
        ("gpt-6.1-sol", "GPT-6.1-Sol")
    );
    assert_eq!(
        first.efforts,
        ["low", "medium", "high", "xhigh", "max", "ultra"]
    );
    assert_eq!(capabilities.plan, None);
    // Models after the thread tell the session again.
    let mut late = Run::started(SessionSpec::default());
    late.feed(&account[1].1);
    late.feed(&entries("simple")[7].1);
    late.feed(&models);
    assert_eq!(
        late.events
            .iter()
            .filter(|e| matches!(e, Ready { .. }))
            .count(),
        2
    );

    // Inferred from the schema (docs/codex.md Fase 1, 5): no capture has a turn without account.
    let mut run = run_of("simple");
    run.feed(&json!({ "method": "error", "params": { "error": { "message": "x", "codexErrorInfo": "unauthorized" }, "willRetry": false } }));
    run.feed(&json!({ "method": "turn/completed", "params": { "turn": { "status": "failed", "error": { "message": "x", "codexErrorInfo": "unauthorized" } } } }));
    let tail = &run.events[run.events.len() - 2..];
    assert_eq!(tail[0], AuthRequired);
    assert!(
        matches!(&tail[1], TurnFinished { ok: false, interrupted: false, error: Some(e), .. } if e == "x")
    );
}

#[test]
fn what_codex_asks_that_quill_cannot_answer_is_refused_not_awaited() {
    let mut codex = Codex::new();
    let decoded = codex.decode(
        &json!({ "id": 7, "method": "item/permissions/requestApproval", "params": {} }).to_string(),
    );
    assert!(matches!(&decoded.events[..], [Unknown(_)]));
    let reply: Value = serde_json::from_str(&decoded.replies[0]).unwrap();
    assert_eq!(
        (reply["id"].clone(), reply["error"]["code"].clone()),
        (json!(7), json!(-32601))
    );
    for line in [
        "not json",
        r#"{"method":"a/new","params":{}}"#,
        r#"{"id":99,"result":{}}"#,
        "{}",
        "[]",
    ] {
        assert!(
            matches!(&codex.decode(line).events[..], [Unknown(_)]),
            "{line}"
        );
    }
    // Items quill does not draw yet stay whole.
    let item = json!({ "method": "item/started", "params": { "item": { "type": "hookPrompt", "id": "c", "fragments": [] } } });
    assert!(matches!(
        &codex.decode(&item.to_string()).events[..],
        [Unknown(_)]
    ));
}

#[test]
fn a_diff_gives_the_hunk_old_and_new_lines_with_their_context() {
    assert_eq!(
        hunk_sides("@@ -1 +1 @@\n-hello\n+hello world\n"),
        ("hello\n".into(), "hello world\n".into())
    );
    let (old, new) = hunk_sides("diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n-b\n+c\n d\n\\ No newline at end of file\n");
    assert_eq!((old.as_str(), new.as_str()), ("a\nb\nd\n", "a\nc\nd\n"));
    let edits = file_edits(&json!({ "id": "i", "changes": [
        { "path": "/w/a", "kind": { "type": "add" }, "diff": "x\n" },
        { "path": "/w/b", "kind": { "type": "delete" }, "diff": "gone\n" },
    ] }));
    assert_eq!((edits[0].0.as_str(), edits[1].0.as_str()), ("i", "i#1"));
    assert_eq!(edits[1].3.before.as_deref(), Some("gone\n"));
    assert_eq!(edits[1].3.after, None);
}

#[test]
fn the_account_is_the_clis_own() {
    let codex = Codex::new();
    assert_eq!(codex.id(), "codex");
    assert_eq!(codex.name(), "Codex");
    assert_eq!(codex.tested(), ["0.163.0"]);
    let plan = codex.launch(&SessionSpec {
        cwd: "/p".into(),
        ..Default::default()
    });
    assert_eq!(
        (
            plan.program.as_str(),
            plan.args.as_slice(),
            plan.cwd.as_path()
        ),
        (
            "codex",
            ["app-server".to_owned()].as_slice(),
            Path::new("/p")
        )
    );
    assert_eq!(codex.auth_query().unwrap(), ["login", "status"]);
    // docs/codex.md "Fase 1", 6: the line is on stderr; the engine hands it over.
    assert!(
        matches!(codex.auth_state("Logged in using ChatGPT\n", 0), AuthState::SignedIn { method, .. } if method == "chatgpt")
    );
    assert!(matches!(
        codex.auth_state("Not logged in\n", 1),
        AuthState::SignedOut
    ));
    assert!(matches!(codex.auth_state("", 1), AuthState::Unknown(_)));
    assert_eq!(
        codex.sign_in_methods(),
        [("chatgpt".to_owned(), "ChatGPT account".to_owned())]
    );
    assert_eq!(codex.login_run("chatgpt").unwrap().args, ["login"]);
    assert!(codex.login_run("device").is_none());
    assert_eq!(codex.logout_run().unwrap().args, ["logout"]);
    // The page is the line on its own, not the local server's address (login.rs, upstream).
    assert_eq!(
        codex.login_url("Starting local login server on http://localhost:1455."),
        None
    );
    assert_eq!(
        codex
            .login_url("https://auth.openai.com/oauth/authorize?x=1\n")
            .as_deref(),
        Some("https://auth.openai.com/oauth/authorize?x=1")
    );
    assert!(codex.mode_waives_prompts("full-access") && !codex.mode_waives_prompts("auto"));
}

#[test]
fn the_mode_words_cover_the_modes_it_offers_and_the_default_is_one_of_them() {
    let codex = Codex::new();
    let words = codex.modes();
    let ids: Vec<&str> = words.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["read-only", "auto", "plan", "full-access"]);
    assert_eq!(codex.default_mode(), "auto");
    assert!(ids.contains(&codex.default_mode()));
    // Only the unsafe one waits for the person's say.
    let unlisted: Vec<&str> = words
        .iter()
        .filter(|w| !w.listed)
        .map(|w| w.id.as_str())
        .collect();
    assert_eq!(unlisted, ["full-access"]);
    assert!(codex.icon().is_some_and(|svg| svg.contains("currentColor")));
    assert!(!codex.login_takes_code());
}

#[test]
fn the_models_are_asked_without_a_thread() {
    let codex = Codex::new();
    let asked: Vec<Value> = (codex.models_query().unwrap().iter())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(methods(&asked), ["initialize", "initialized", "model/list"]);
    // account.jsonl:7, under the id the query asks it by.
    let mut answer = entries("account")[6].1.clone();
    answer["id"] = json!(MODELS_ID);
    let models = codex.models_answer(&answer.to_string()).unwrap();
    assert_eq!(models.len(), 8);
    assert!(codex
        .models_answer(&entries("account")[1].1.to_string())
        .is_none());
}

#[test]
fn the_context_is_split_into_what_fills_it() {
    // edit-bash.jsonl:58: 21450 in (7168 cached), 78 out, a 258400 window.
    let last = json!({ "inputTokens": 21450, "cachedInputTokens": 7168, "outputTokens": 78,
        "reasoningOutputTokens": 0, "totalTokens": 21528 });
    let split: Vec<(String, u64)> = context_categories(&last, 258_400)
        .into_iter()
        .map(|c| (c.name, c.tokens))
        .collect();
    let want = [
        ("Cached input", 7168),
        ("New input", 14282),
        ("Output", 78),
        ("Free space", 236_872),
    ];
    assert_eq!(split, want.map(|(n, t)| (n.to_owned(), t)));
}

fn tool_calls(events: &[AgentEvent]) -> Vec<(&str, Option<&str>)> {
    (events.iter())
        .filter_map(|e| match e {
            ToolUse { name, parent, .. } => Some((name.as_str(), parent.as_deref())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_subagent_is_the_agent_tool_with_its_task_its_words_and_its_account() {
    let run = run_of("subagent");
    let call = "call_c0445ff7950b4b418d88ab69a4138d73";
    let thread = "01a18239-60c9-7b52-87aa-ae736b0cf3a6";
    // subagent.jsonl:29 (started): the agent tool a Claude helper is.
    let Some(ToolUse { input, parent, .. }) = run
        .events
        .iter()
        .find(|e| matches!(e, ToolUse { name, .. } if name == "Agent"))
    else {
        panic!("no Agent tool");
    };
    assert_eq!(*parent, None);
    assert_eq!(
        (
            input["description"].as_str(),
            input["model"].as_str(),
            input["effort"].as_str()
        ),
        (Some("Ping"), Some("gpt-6-luna"), Some("low"))
    );
    assert!(run.events.contains(&TaskStarted {
        id: thread.into(),
        call: Some(call.into()),
        description: "Ping".into(),
        kind: "agent".into(),
    }));
    // The helper's own message goes with the call (:40), and ends the task as its summary (:43).
    assert!(run
        .events
        .iter()
        .any(|e| matches!(e, Text { text, parent: Some(p), .. } if text == "pong" && p == call)));
    let ended: Vec<_> = (run.events.iter())
        .filter(|e| matches!(e, TaskEnded { .. }))
        .collect();
    assert!(
        matches!(ended[..], [TaskEnded { id, summary: Some(s), ok: true, stopped: false, usage: Some(u), .. }]
            if id == thread && s == "pong" && u.tokens > 0)
    );
    assert!(run.events.iter().any(
        |e| matches!(e, ToolResult { call: c, output, is_error: false, .. } if c == call && output == "pong")
    ));
    assert!(run
        .events
        .iter()
        .any(|e| matches!(e, TaskProgress { id, usage } if id == thread && usage.tokens > 0)));
    // `wait` (:37) is a step of its own; the helper's turn is not the session's.
    assert_eq!(
        tool_calls(&run.events)
            .iter()
            .filter(|(n, _)| *n == "WaitAgent")
            .count(),
        1
    );
    assert_eq!(finished(&run.events).len(), 1);
    // Inferred from the schema: a spawn as a call, with its prompt and the thread it opened.
    let mut run = run_of("simple");
    let item = |done: bool, status: &str, states: Value| {
        json!({ "method": if done { "item/completed" } else { "item/started" }, "params": { "item": {
            "type": "collabAgentToolCall", "id": "c1", "tool": "spawnAgent", "status": status,
            "senderThreadId": "01a1817c-3a5b-79d0-ae92-0f25db9b7c42", "receiverThreadIds": ["t2"],
            "prompt": "Count the files.\nThen stop.", "model": "gpt-6-luna", "reasoningEffort": "low",
            "agentsStates": states } } })
    };
    run.feed(&item(false, "inProgress", json!({})));
    run.feed(&item(
        true,
        "completed",
        json!({ "t2": { "status": "completed", "message": "3 files" } }),
    ));
    let tail: Vec<_> = run.events.iter().rev().take(4).collect();
    assert!(tail
        .iter()
        .any(|e| matches!(e, TaskEnded { summary: Some(s), .. } if s == "3 files")));
    assert!(run.events.iter().any(|e| matches!(e, ToolUse { name, input, .. }
        if name == "Agent" && input["prompt"] == "Count the files.\nThen stop." && input["description"] == "Count the files")));
}

#[test]
fn compaction_is_a_step() {
    let run = run_of("compact");
    assert_eq!(unknown(&run.events), Vec::<&Value>::new());
    assert_eq!(tool_calls(&run.events), [("Compact", None)]);
    assert_eq!(results(&run.events).len(), 1);
    // The deprecated notice says the same again and draws nothing.
    let mut run = run_of("compact");
    let before = run.events.len();
    run.feed(
        &json!({ "method": "thread/compacted", "params": { "threadId": "x", "turnId": "y" } }),
    );
    assert_eq!(run.events.len(), before);
}

#[test]
fn a_review_is_two_steps_with_the_findings_in_the_last() {
    let run = run_of("review");
    assert_eq!(unknown(&run.events), Vec::<&Value>::new());
    let tools = tool_calls(&run.events);
    assert_eq!(tools.first(), Some(&("EnterReviewMode", None)));
    assert_eq!(tools.last(), Some(&("ExitReviewMode", None)));
    let entered = calls(&run.events)[0].1;
    assert_eq!(entered["review"], "current changes");
    let Some(ToolResult { output, .. }) = results(&run.events).last().map(|e| (*e).clone()) else {
        panic!("no result");
    };
    assert!(output.contains("[P1] Return the sum from `add`"));
}

#[test]
fn an_image_read_is_a_read_and_a_made_one_has_its_prompt_and_file() {
    let run = run_of("view-sleep");
    assert_eq!(unknown(&run.events), Vec::<&Value>::new());
    assert_eq!(
        calls(&run.events)
            .iter()
            .map(|(n, i)| (*n, (*i).clone()))
            .collect::<Vec<_>>(),
        [
            ("Read", json!({ "file_path": "/work/logo.png" })),
            ("Sleep", json!({ "duration_ms": 1000 })),
        ]
    );
    let run = run_of("image");
    assert_eq!(unknown(&run.events), Vec::<&Value>::new());
    assert_eq!(tool_calls(&run.events), [("ImageGeneration", None)]);
    let Some(ToolResult {
        output,
        files,
        is_error,
        ..
    }) = results(&run.events)[0..].first().map(|e| (*e).clone())
    else {
        panic!("no result");
    };
    assert!(!is_error);
    assert!(output.starts_with("A small, simple icon of a classic quill pen"));
    assert_eq!(files.len(), 1);
    assert!(files[0].path.to_string_lossy().ends_with(".png"));
    // Inferred from the schema: the limit ends it as a failure, with no file.
    let mut run = run_of("simple");
    let item = |done: bool| {
        json!({ "method": if done { "item/completed" } else { "item/started" },
        "params": { "item": { "type": "imageGeneration", "id": "g1", "status": "failed", "result": "",
        "savedPath": null, "revisedPrompt": null,
        "failure": { "type": "usageLimitExceeded", "limitId": "images", "resetsAt": null } } } })
    };
    run.feed(&item(false));
    run.feed(&item(true));
    assert!(
        matches!(run.events.last(), Some(ToolResult { is_error: true, files, output, .. })
        if files.is_empty() && output == "Image generation limit reached")
    );
}

#[test]
fn a_tool_the_client_declared_is_a_step_by_its_name() {
    // Inferred from the schema: quill declares no dynamic tools, so no capture has one.
    let mut run = run_of("simple");
    let item = |done: bool| {
        json!({ "method": if done { "item/completed" } else { "item/started" },
        "params": { "item": { "type": "dynamicToolCall", "id": "d1", "tool": "lookup", "namespace": null,
        "arguments": { "q": "x" }, "status": if done { "completed" } else { "inProgress" }, "success": done,
        "contentItems": [{ "type": "inputText", "text": "found" }], "durationMs": 3 } } })
    };
    run.feed(&item(false));
    run.feed(&item(true));
    let n = run.events.len();
    assert!(
        matches!(&run.events[n - 2], ToolUse { name, input, .. } if name == "lookup" && input["q"] == "x")
    );
    assert!(
        matches!(&run.events[n - 1], ToolResult { output, is_error: false, .. } if output == "found")
    );
}

#[test]
fn an_mcp_tool_s_pictures_are_read_for_the_engine_to_keep() {
    // Inferred from MCP's image blocks: no capture has an MCP tool that answers with one.
    let mut run = run_of("simple");
    run.feed(&json!({ "method": "item/completed", "params": { "item": {
        "type": "mcpToolCall", "id": "m1", "server": "shots", "tool": "take", "arguments": {},
        "status": "completed", "error": null, "result": { "content": [
            { "type": "text", "text": "taken" },
            { "type": "image", "data": "iVBORw0K", "mimeType": "image/png" }] } } } }));
    let picture = ToolContent::Image {
        mime: "image/png".into(),
        data: "iVBORw0K".into(),
    };
    assert!(
        matches!(run.events.last(), Some(ToolResult { output, inline, .. })
        if output == "taken" && *inline == [picture])
    );
}

#[test]
fn an_unauthorized_turn_asks_to_sign_in() {
    // SYNTHETIC (docs/codex.md): no capture has a turn without an account. The fixture is
    // simple.jsonl up to the person's message, then the `error` and the failed `turn/completed`
    // in the shape of the schema (ErrorNotification, TurnError, CodexErrorInfo "unauthorized").
    let run = run_of("synthetic-unauthorized");
    assert_eq!(unknown(&run.events), Vec::<&Value>::new());
    let tail = &run.events[run.events.len() - 2..];
    assert_eq!(tail[0], AuthRequired);
    assert!(
        matches!(&tail[1], TurnFinished { ok: false, interrupted: false, error: Some(e), .. } if e.contains("sign in"))
    );
    // Once, not once for the `error` and again for the turn.
    assert_eq!(run.events.iter().filter(|e| **e == AuthRequired).count(), 1);
}

/// Remote Control on the shared app-server (docs/remote-control.md §3). Inferred from the
/// protocol's schema and source (0.163.0) until a capture with a phone has it.
#[test]
fn a_shared_session_is_reached_from_the_phone() {
    let shared = SessionSpec {
        options: [("remote".to_string(), "true".to_string())].into(),
        env: vec![("BROWSER".into(), "/tmp/shim".into())],
        ..Default::default()
    };
    assert!(Codex::new().mux(&shared).is_some());
    assert!(Codex::new().mux(&SessionSpec::default()).is_none());
    let mut run = Run::started(shared);
    // The experimental API, which Remote Control needs.
    assert_eq!(
        run.written[0]["params"]["capabilities"]["experimentalApi"],
        true
    );
    run.feed(&json!({ "id": 1, "result": { "userAgent": "codex/0.163.0" } }));
    let thread = (run.written.iter())
        .find(|m| m["method"] == "thread/start")
        .unwrap()
        .clone();
    // The session's own variables reach its commands, not the shared process's environment.
    assert_eq!(
        thread["params"]["config"]["shell_environment_policy"]["set"]["BROWSER"],
        "/tmp/shim"
    );
    run.feed(&json!({ "id": thread["id"], "result": { "thread": { "id": "t1" } } }));
    let Some(Ready { capabilities, .. }) = run.events.iter().find(|e| matches!(e, Ready { .. }))
    else {
        panic!("{:?}", run.events)
    };
    assert!(capabilities.remote && capabilities.remote_pairing);
    // On, runtime-only: quill keeps the choice.
    run.send(AgentCommand::Remote {
        on: true,
        name: None,
    });
    let enable = run.written.last().unwrap().clone();
    assert_eq!(
        (enable["method"].as_str(), &enable["params"]),
        (Some("remoteControl/enable"), &json!({ "ephemeral": true }))
    );
    run.feed(
        &json!({ "id": enable["id"], "result": { "status": "connecting", "serverName": "host",
        "installationId": "i", "environmentId": null } }),
    );
    run.feed(
        &json!({ "method": "remoteControl/status/changed", "params": { "status": "connected",
        "serverName": "host", "installationId": "i", "environmentId": "e" } }),
    );
    let remote: Vec<bool> = (run.events.iter())
        .filter_map(|e| match e {
            AgentEvent::Remote { on, .. } => Some(*on),
            _ => None,
        })
        .collect();
    assert_eq!(remote, [true, true]);
    // A code to pair the phone with.
    run.send(AgentCommand::RemotePair);
    let pair = run.written.last().unwrap().clone();
    assert_eq!(pair["params"]["manualCode"], true);
    run.feed(
        &json!({ "id": pair["id"], "result": { "pairingCode": "p", "manualPairingCode": "ABCD-EFGH",
        "environmentId": "e", "expiresAt": 1_793_500_000u64 } }),
    );
    assert!(run.events.iter().any(|e| matches!(e,
        AgentEvent::RemotePairing { code, expires_at: 1_793_500_000 } if code == "ABCD-EFGH")));
    // A turn the phone started: its words are the person's.
    run.feed(&json!({ "method": "turn/started", "params": { "threadId": "t1", "turn": { "id": "u1" } } }));
    run.feed(
        &json!({ "method": "item/completed", "params": { "threadId": "t1", "item": {
        "type": "userMessage", "id": "m1", "clientId": null,
        "content": [{ "type": "text", "text": "hola desde el celular" }] } } }),
    );
    assert!(run.events.iter().any(|e| matches!(e,
        AgentEvent::Prompt { request, text } if request == "m1" && text == "hola desde el celular")));
    // An approval the phone answered leaves quill's card.
    run.feed(
        &json!({ "id": 7, "method": "item/commandExecution/requestApproval",
        "params": { "threadId": "t1", "turnId": "u1", "itemId": "c1", "command": "touch x" } }),
    );
    run.feed(&json!({ "method": "serverRequest/resolved", "params": { "threadId": "t1", "requestId": 7 } }));
    assert!(run
        .events
        .iter()
        .any(|e| matches!(e, AgentEvent::Withdrawn { id } if *id == key(&json!(7)))));
    // Turned off, and why it could not be turned on.
    run.send(AgentCommand::Remote {
        on: false,
        name: None,
    });
    let disable = run.written.last().unwrap().clone();
    assert_eq!(disable["method"], "remoteControl/disable");
    run.feed(&json!({ "id": disable["id"], "error": { "code": -32600,
        "message": "remote control requires ChatGPT authentication" } }));
    assert!(
        matches!(run.events.last(), Some(AgentEvent::Remote { on: false, error: Some(e), .. })
        if e.contains("ChatGPT"))
    );
}
