//! Replays of captures of the real CLI (`fixtures/1.0.77/`, recorded with `newen-capture --agent
//! grok <scenario>`, docs/grok.md "Capturas"): Grok 4.7 for most; `host` and `plan` with the
//! capture's scripted model. The adapter must write what it wrote then, line for line.

use super::*;
use newen_agent::AgentEvent::{self, *};
use newen_agent_acp::replay::{replay, Replayed};

#[test]
fn its_models_are_listed_apart_from_any_session_by_their_ids() {
    // What `grok models` printed (1.0.77), and a second model as it would list one (inferred).
    let printed = "You are logged in with grok.com.\n\nDefault model: grok-4.7\n\n\
        Available models:\n  * grok-4.7 (default)\n    grok-code-fast-1\n";
    let grok = Grok::new();
    assert_eq!(grok.models_runs()[0].args, ["models"]);
    let listed: Vec<(String, bool)> = (grok.models_listed(&[printed.to_owned()]).into_iter())
        .map(|m| (m.id, m.default))
        .collect();
    assert_eq!(
        listed,
        [
            ("grok-4.7".to_owned(), true),
            ("grok-code-fast-1".to_owned(), false)
        ]
    );
    assert!(grok
        .models_listed(&["Not logged in.".to_owned()])
        .is_empty());
}

fn run(name: &str) -> Replayed {
    let path = format!(
        "{}/fixtures/1.0.77/{name}.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let capture = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let replayed = replay(&mut Grok::new(), &capture);
    assert_eq!(
        replayed.written, replayed.expected,
        "{name}: what the adapter writes"
    );
    replayed
}

fn tool_uses(events: &[AgentEvent]) -> Vec<(&str, &Value)> {
    (events.iter())
        .filter_map(|e| match e {
            ToolUse { name, input, .. } => Some((name.as_str(), input)),
            _ => None,
        })
        .collect()
}

fn asks(events: &[AgentEvent]) -> Vec<(&str, bool)> {
    (events.iter())
        .filter_map(|e| match e {
            PermissionRequest {
                tool, interactive, ..
            } => Some((tool.as_str(), *interactive)),
            _ => None,
        })
        .collect()
}

#[test]
fn every_capture_is_written_again_line_for_line() {
    for name in [
        "simple",
        "edit-bash",
        "deny",
        "always",
        "todos",
        "ask",
        "interrupt",
        "resume",
        "plan",
        "unsafe",
        "host",
    ] {
        run(name);
    }
}

/// Its version is in `initialize`'s `_meta`; the mode that never asks is offered only as
/// unsafe; its models and efforts come from `session/new`; its own notifications are quiet;
/// the turn's tokens are in the prompt's `_meta`.
#[test]
fn a_session_opens_with_its_modes_and_a_turn_answers() {
    let replayed = run("simple");
    let ready = replayed.events.iter().rev().find_map(|e| match e {
        Ready {
            version,
            capabilities,
            mode,
        } => Some((version, capabilities, mode)),
        _ => None,
    });
    let (version, capabilities, mode) = ready.expect("ready");
    assert!(Grok::new().tested().contains(&version.as_str()));
    assert_eq!(mode.as_deref(), Some(DEFAULT));
    assert_eq!(capabilities.modes, [DEFAULT, AUTO, PLAN]);
    assert_eq!(capabilities.unsafe_modes, [ALWAYS]);
    let words: Vec<String> = Grok::new().modes().into_iter().map(|m| m.id).collect();
    assert_eq!(
        words,
        [DEFAULT, AUTO, PLAN, ALWAYS],
        "its words cover every mode"
    );
    assert_eq!(capabilities.models[0].id, "grok-4.7");
    assert_eq!(
        capabilities.models[0].efforts,
        ["xhigh", "high", "medium", "low"]
    );
    assert!(!(replayed.events.iter()).any(
        |e| matches!(e, Unknown(v) if v["method"].as_str().is_some_and(|m| m.starts_with("_x.ai/")))
    ));
    assert!(replayed
        .events
        .iter()
        .any(|e| matches!(e, Text { text, .. } if text == "ok")));
    let usage = replayed.events.iter().find_map(|e| match e {
        TurnFinished {
            ok: true, usage, ..
        } => Some(usage),
        _ => None,
    });
    let usage = usage.expect("a finished turn");
    assert_eq!((usage.input_tokens, usage.output_tokens), (13018, 528));
}

/// A new file is a Write (an edit of nothing), a change an Edit and a command Bash, drawn from
/// its first report, which names the tool in its title; each asked first.
#[test]
fn its_tools_read_as_newens() {
    let replayed = run("edit-bash");
    let tools = tool_uses(&replayed.events);
    let names: Vec<&str> = tools.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["Write", "Edit", "Bash"]);
    assert_eq!(tools[0].1["file_path"], "/work/notes.txt");
    assert_eq!(tools[0].1["content"], "hello\n");
    assert_eq!(tools[1].1["new_string"], "hello world");
    assert_eq!(
        tools[2].1["command"],
        "mv notes.txt moved.txt && rm moved.txt"
    );
    assert_eq!(
        asks(&replayed.events),
        [("Write", false), ("Edit", false), ("Bash", false)]
    );
    let edited = replayed.events.iter().any(|e| {
        matches!(e, ToolResult { files, .. } if files.iter().any(|f| f.after.as_deref() == Some("hello world")))
    });
    assert!(edited, "the edit's change");
}

/// Its task list is ACP's plan, drawn as TodoWrite; a listing is LS and says what it found.
#[test]
fn its_task_list_and_a_listing_show() {
    let replayed = run("todos");
    let names: Vec<&str> = tool_uses(&replayed.events)
        .iter()
        .map(|(n, _)| *n)
        .collect();
    assert!(
        names.contains(&"TodoWrite") && names.contains(&"LS"),
        "{names:?}"
    );
    assert!(!names.iter().any(|n| n.contains("todo_write")), "{names:?}");
    let listed = replayed.events.iter().any(|e| {
        matches!(e, ToolResult { output, .. } if output.contains("a.txt") && output.contains("b.txt"))
    });
    assert!(listed, "the listing's words");
}

/// A denied edit is refused with its reject option; "always" picks its allow-always one.
#[test]
fn a_permission_is_answered_with_its_options() {
    let denied = run("deny");
    assert!(denied
        .written
        .iter()
        .any(|l| l["result"]["outcome"]["optionId"] == "reject-once"));
    let always = run("always");
    assert!(always
        .written
        .iter()
        .any(|l| l["result"]["outcome"]["optionId"] == "allow-edits-session"));
}

/// Its question is a request of its own: a card of questions, answered with each one's labels.
#[test]
fn its_question_is_a_card_and_its_answer_goes_back() {
    let replayed = run("ask");
    assert_eq!(asks(&replayed.events), [("AskUserQuestion", true)]);
    let answer = (replayed.written.iter())
        .find(|l| l["result"]["outcome"] == "accepted")
        .expect("an answer");
    assert_eq!(
        answer["result"]["answers"]["Do you prefer red or blue?"],
        json!(["Red"])
    );
}

/// In Plan its plan waits on the person; accepted, it goes back to Ask and carries it out.
#[test]
fn a_plan_is_accepted_and_carried_out() {
    let replayed = run("plan");
    assert_eq!(asks(&replayed.events)[0], ("ExitPlanMode", true));
    assert!(replayed
        .written
        .iter()
        .any(|l| l["result"] == json!({ "outcome": "approved" })));
    let modes: Vec<&str> = (replayed.events.iter())
        .filter_map(|e| match e {
            ModeChanged { mode } => Some(mode.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(modes, [PLAN, DEFAULT]);
}

/// Always approve: its notification, then `default`; the edit the default mode asks about is
/// made without a card.
#[test]
fn always_approve_never_asks() {
    let replayed = run("unsafe");
    assert_eq!(replayed.written[2]["method"], PERMISSION_MODE);
    assert_eq!(replayed.written[2]["params"]["yolo_mode"], true);
    assert_eq!(replayed.written[3]["params"]["modeId"], DEFAULT);
    assert!(replayed
        .events
        .iter()
        .any(|e| matches!(e, ModeChanged { mode } if mode == ALWAYS)));
    assert!(asks(&replayed.events).is_empty());
    assert_eq!(tool_uses(&replayed.events)[0].0, "Write");
}

/// Newen's own tools reach it as MCP servers it calls through `use_tool`: drawn as Newen's,
/// never asked about.
#[test]
fn newens_tools_are_called_and_never_asked() {
    let replayed = run("host");
    let names: Vec<&str> = tool_uses(&replayed.events)
        .iter()
        .map(|(n, _)| *n)
        .collect();
    assert!(names.contains(&"mcp__sessions__list_sessions"), "{names:?}");
    assert!(asks(&replayed.events).is_empty());
}

#[test]
fn a_stopped_turn_finishes_interrupted() {
    let replayed = run("interrupt");
    assert!(replayed.events.iter().any(|e| matches!(
        e,
        TurnFinished {
            interrupted: true,
            ..
        }
    )));
}

/// The next process resumes the session (`session/resume`), and the conversation goes on.
#[test]
fn a_session_is_resumed() {
    let replayed = run("resume");
    assert!(replayed
        .written
        .iter()
        .any(|l| l["method"] == "session/resume"));
    assert!(replayed
        .events
        .iter()
        .any(|e| matches!(e, Text { text, .. } if text.to_lowercase().contains("lantern"))));
}

/// Its answer format: labels when the person chose them, "Other" with a note for words of
/// their own, several labels for a multiple choice; a question left alone is left out.
#[test]
fn answers_are_its_labels_or_other_with_a_note() {
    let params = json!({ "questions": [
        { "question": "Color?", "options": [{ "label": "Red" }, { "label": "Blue" }] },
        { "question": "Sizes?", "multiSelect": true, "options": [{ "label": "S" }, { "label": "M" }] },
        { "question": "Name?", "options": [{ "label": "Ana" }] },
        { "question": "Skipped?", "options": [{ "label": "Yes" }] },
    ] });
    let given = json!({ "Color?": "Blue", "Sizes?": "S, M", "Name?": "Juan", "Skipped?": "[No preference]" });
    let answer = answers(&params, &given);
    assert_eq!(answer["answers"]["Color?"], json!(["Blue"]));
    assert_eq!(answer["answers"]["Sizes?"], json!(["S", "M"]));
    assert_eq!(answer["answers"]["Name?"], json!(["Other"]));
    assert_eq!(answer["annotations"]["Name?"]["notes"], "Juan");
    assert!(answer["answers"].get("Skipped?").is_none());
    let plan = json!({});
    let stopped = TOOLS.answer(EXIT_PLAN, &plan, None);
    assert_eq!(stopped, json!({ "outcome": "cancelled" }));
    let revise = Decision::Deny {
        message: "Use b.txt".into(),
    };
    let feedback = TOOLS.answer(EXIT_PLAN, &plan, Some(&revise));
    assert_eq!(feedback["feedback"], "Use b.txt");
}

/// `grok mcp list --json`, and what adds or takes a server away.
#[test]
fn its_connectors_are_listed_added_and_removed() {
    let listed = r#"[{"command":"npx","args":["-y","some-server"],"env":{"TOKEN":"abc"},"enabled":true,"name":"demo","scope":"user"},
        {"url":"https://example.com/mcp","headers":{"A":"b"},"enabled":false,"name":"remote","scope":"project"}]"#;
    let found = servers(listed);
    assert_eq!(found[0].target, "npx -y some-server");
    assert_eq!(found[0].source, McpSource::User);
    assert_eq!(found[1].target, "https://example.com/mcp");
    assert_eq!(found[1].status, McpStatus::Disabled);
    assert_eq!(found[1].source, McpSource::Project);
    let grok = Grok::new();
    let add = |server: newen_agent::NewServer| grok.extension_op(&ExtensionOp::AddServer(server));
    let local = add(newen_agent::NewServer {
        name: "demo".into(),
        target: "npx".into(),
        args: vec!["-y".into(), "x".into()],
        env: vec!["TOKEN=abc".into()],
        scope: Some("project".into()),
        ..Default::default()
    });
    assert_eq!(
        local.unwrap().args,
        [
            "mcp",
            "add",
            "-s",
            "project",
            "-e",
            "TOKEN=abc",
            "demo",
            "--",
            "npx",
            "-y",
            "x"
        ]
    );
    let remote = add(newen_agent::NewServer {
        name: "remote".into(),
        target: "https://example.com/mcp".into(),
        remote: true,
        headers: vec!["Authorization: Bearer x".into()],
        ..Default::default()
    });
    assert_eq!(
        remote.unwrap().args,
        [
            "mcp",
            "add",
            "-t",
            "http",
            "-H",
            "Authorization: Bearer x",
            "remote",
            "https://example.com/mcp"
        ]
    );
    let gone = grok.extension_op(&ExtensionOp::RemoveServer("demo".into()));
    assert_eq!(gone.unwrap().args, ["mcp", "remove", "demo"]);
}

/// `<server>__<tool>`: the longest server of the session it starts with, else its first `__`.
#[test]
fn an_mcp_tool_is_read_by_its_server() {
    let servers = ["sessions".to_owned(), "my__server".to_owned()];
    assert_eq!(
        mcp("sessions__list_sessions", &servers),
        ("sessions".into(), "list_sessions".into())
    );
    assert_eq!(
        mcp("my__server__run", &servers),
        ("my__server".into(), "run".into())
    );
    assert_eq!(mcp("other__do", &servers), ("other".into(), "do".into()));
}

/// Settings' options reach `grok agent stdio`: a switch moved from its default sets its
/// variable, the sandbox its profile, web search off its flag before `agent`; left alone,
/// nothing, so Grok's own settings decide.
#[test]
fn its_options_reach_its_launch() {
    let grok = Grok::new();
    let plain = grok.launch(&SessionSpec::default());
    assert_eq!(plain.args, ["agent", "stdio"]);
    assert!(plain.env.is_empty());
    let mut spec = SessionSpec::default();
    for (key, value) in [
        ("sandbox", "workspace"),
        ("web", "false"),
        ("subagents", "false"),
        ("memory", "true"),
        ("gitignore", "true"),
        ("telemetry", "true"),
    ] {
        spec.options.insert(key.into(), value.into());
    }
    let plan = grok.launch(&spec);
    assert_eq!(plan.args, ["--disable-web-search", "agent", "stdio"]);
    let set = |name: &str| {
        (plan.env.iter())
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(set("GROK_SANDBOX"), Some("workspace"));
    assert_eq!(set("GROK_SUBAGENTS"), Some("0"));
    assert_eq!(set("GROK_MEMORY"), Some("1"));
    assert_eq!(set("GROK_RESPECT_GITIGNORE"), Some("1"));
    assert_eq!(set("GROK_TELEMETRY_ENABLED"), None, "at its default");
    let keys: Vec<String> = grok.options().into_iter().map(|o| o.key).collect();
    assert_eq!(
        keys,
        [
            "sandbox",
            "web",
            "subagents",
            "memory",
            "workflows",
            "gitignore",
            "telemetry"
        ]
    );
}

/// Its logo names it where Newen lists agents.
#[test]
fn it_has_its_logo() {
    let logo = Grok::new().icon().expect("a logo");
    assert!(logo.starts_with("<svg fill=\"currentColor\""));
}
