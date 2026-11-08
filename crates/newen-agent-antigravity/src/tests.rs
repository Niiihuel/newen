//! Replays of captures of the real CLI (`fixtures/1.5.3/`, recorded with `newen-capture --agent
//! antigravity <scenario>` and Gemini 3.8 Flash, docs/antigravity.md "Capturas"). The adapter
//! must write what it wrote then, line for line.

use super::*;
use newen_agent::AgentEvent::{self, *};
use newen_agent_acp::replay::{replay, Replayed};

fn run(name: &str) -> Replayed {
    let path = format!("{}/fixtures/1.5.3/{name}.jsonl", env!("CARGO_MANIFEST_DIR"));
    let capture = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let replayed = replay(&mut Antigravity::new(), &capture);
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

/// Its modes, models and efforts come from `session/new`; skipping permissions is offered only
/// as unsafe; the turn's tokens are ACP's `usage`.
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
    assert!(Antigravity::new().tested().contains(&version.as_str()));
    assert_eq!(mode.as_deref(), Some(DEFAULT));
    assert_eq!(capabilities.modes, [DEFAULT, EDITS, PLAN]);
    assert_eq!(capabilities.unsafe_modes, [SKIP]);
    let words: Vec<String> = (Antigravity::new().modes().into_iter())
        .map(|m| m.id)
        .collect();
    assert_eq!(
        words,
        [DEFAULT, EDITS, PLAN, SKIP],
        "its words cover every mode"
    );
    assert_eq!(capabilities.models[0].id, "gemini-3.8-flash");
    assert_eq!(capabilities.models[0].efforts, ["low", "medium", "high"]);
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
    assert_eq!((usage.input_tokens, usage.output_tokens), (12813, 85));
}

/// A new file is a Write, a change an Edit and a command Bash, from its capitalized input;
/// only the command asks.
#[test]
fn its_tools_read_as_newens() {
    let replayed = run("edit-bash");
    let tools = tool_uses(&replayed.events);
    let names: Vec<&str> = tools.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["Write", "Edit", "Bash"]);
    assert_eq!(tools[0].1["file_path"], "/work/notes.txt");
    assert_eq!(tools[0].1["content"], "hello\n");
    assert_eq!(tools[1].1["old_string"], "hello");
    assert_eq!(tools[1].1["new_string"], "hello world");
    assert_eq!(
        tools[2].1["command"],
        "mv notes.txt moved.txt && rm moved.txt"
    );
    assert_eq!(asks(&replayed.events), [("Bash", false)]);
}

/// A denied command picks its deny option; "always" has none, so it is allowed once.
#[test]
fn a_permission_is_answered_with_its_options() {
    let denied = run("deny");
    assert!(denied
        .written
        .iter()
        .any(|l| l["result"]["outcome"]["optionId"] == "deny"));
    let always = run("always");
    assert!(always
        .written
        .iter()
        .any(|l| l["result"]["outcome"]["optionId"] == "allow"));
}

/// Its question is a form: one question (its `_other` field is that question's own words), and
/// the label chosen goes back as its value.
#[test]
fn its_question_is_a_card_and_its_answer_goes_back() {
    let replayed = run("ask");
    assert_eq!(asks(&replayed.events), [("AskUserQuestion", true)]);
    let questions = replayed.events.iter().find_map(|e| match e {
        PermissionRequest { input, .. } => input["questions"].as_array(),
        _ => None,
    });
    assert_eq!(
        questions.map(Vec::len),
        Some(1),
        "no question of its own for the other words"
    );
    assert!(
        tool_uses(&replayed.events).is_empty(),
        "the card, not a step"
    );
    assert!(replayed
        .written
        .iter()
        .any(|l| l["result"] == json!({ "action": "accept", "content": { "q1": "1" } })));
}

/// In Plan it writes its plan for review as a file of its own and ends its turn: a plan the
/// person carries out from its card.
#[test]
fn a_plan_for_review_is_a_plan_card() {
    let replayed = run("plan");
    let plan = tool_uses(&replayed.events)
        .into_iter()
        .find(|(name, _)| *name == "ExitPlanMode");
    let plan = plan.expect("a plan");
    assert!(plan.1["plan"]
        .as_str()
        .is_some_and(|p| p.contains("planned.txt")));
    assert!(replayed
        .events
        .iter()
        .any(|e| matches!(e, ModeChanged { mode } if mode == PLAN)));
}

/// Skip permissions is a launch switch: the session starts again in it, says so though ACP
/// says `default`, and the command the default mode asks about runs without a card.
#[test]
fn skip_permissions_starts_it_again_and_never_asks() {
    let replayed = run("unsafe");
    let antigravity = Antigravity::new();
    let skipping = SessionSpec {
        mode: Some(SKIP.into()),
        ..Default::default()
    };
    let launched = antigravity.launch(&skipping).args;
    assert_eq!(launched, ["acp", "--dangerously-skip-permissions"]);
    assert!(antigravity.launch_mode(SKIP) && !antigravity.launch_mode(PLAN));
    let ready = replayed.events.iter().rev().find_map(|e| match e {
        Ready { mode, .. } => mode.as_deref(),
        _ => None,
    });
    assert_eq!(ready, Some(SKIP));
    assert!(asks(&replayed.events).is_empty());
    assert_eq!(tool_uses(&replayed.events)[0].0, "Bash");
}

/// Newen's own tools reach it as MCP servers: "Run mcp_<server>_<tool>", drawn as Newen's and
/// never asked about.
#[test]
fn newens_tools_are_called_and_never_asked() {
    let replayed = run("host");
    let names: Vec<&str> = tool_uses(&replayed.events)
        .iter()
        .map(|(n, _)| *n)
        .collect();
    assert_eq!(names, ["mcp__sessions__list_sessions"]);
    assert!(asks(&replayed.events).is_empty());
    let servers = ["sessions".to_owned(), "my_server".to_owned()];
    assert_eq!(
        mcp("my_server_run_it", &servers),
        Some(("my_server".into(), "run_it".into()))
    );
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

/// `agy mcp list`'s table, and what adds or takes a server away.
#[test]
fn its_connectors_are_listed_added_and_removed() {
    let listed = "NAME    TYPE   STATUS    COMMAND/URL\n\
                  demo    stdio  enabled   npx -y some-server --flag\n\
                  remote  http   disabled  https://example.com/mcp\n";
    let found = servers(listed);
    assert_eq!(found[0].target, "npx -y some-server --flag");
    assert_eq!(found[1].status, McpStatus::Disabled);
    assert!(servers("No MCP servers configured.\n").is_empty());
    let antigravity = Antigravity::new();
    let add = |server: newen_agent::NewServer| {
        antigravity
            .extension_op(&ExtensionOp::AddServer(server))
            .unwrap()
            .args
    };
    let local = add(newen_agent::NewServer {
        name: "demo".into(),
        target: "npx".into(),
        args: vec!["-y".into(), "x".into()],
        env: vec!["TOKEN=abc".into()],
        ..Default::default()
    });
    assert_eq!(
        local,
        [
            "mcp",
            "add",
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
        remote,
        [
            "mcp",
            "add",
            "-H",
            "Authorization: Bearer x",
            "-t",
            "http",
            "remote",
            "--",
            "https://example.com/mcp"
        ]
    );
}
