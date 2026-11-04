//! Replays of captures of the real CLI (`fixtures/2026.9.24/`, recorded with `newen-capture
//! --agent hermes <scenario> --fake-model <port>` against Hermes's `main` after v2026.9.24,
//! docs/hermes.md "Capturas"). Hermes's protocol is real; the model is the capture's script.
//! The adapter must write what it wrote then, line for line.

use super::*;
use newen_agent::AgentEvent::{self, *};
use newen_agent_acp::replay::{replay, Replayed};

fn run(name: &str) -> Replayed {
    let path = format!(
        "{}/fixtures/2026.9.24/{name}.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let capture = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let replayed = replay(&mut Hermes::new(), &capture);
    assert_eq!(
        replayed.written, replayed.expected,
        "{name}: what the adapter writes"
    );
    replayed
}

fn tool_uses(events: &[AgentEvent]) -> Vec<(&str, &Value)> {
    events
        .iter()
        .filter_map(|e| match e {
            ToolUse { name, input, .. } => Some((name.as_str(), input)),
            _ => None,
        })
        .collect()
}

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
    assert!(Hermes::new().tested().contains(&version.as_str()));
    assert_eq!(mode.as_deref(), Some(ASK));
    assert_eq!(capabilities.modes, [ASK, EDITS, DONT_ASK]);
    let modes: Vec<String> = Hermes::new().modes().into_iter().map(|m| m.id).collect();
    assert_eq!(
        capabilities.modes, modes,
        "its words cover every mode it offers"
    );
    assert_eq!(capabilities.models[0].id, "custom:fixture-model");
    // `Custom endpoint · fixture-model`: its provider, and the model's own name.
    assert_eq!(
        capabilities.models[0].provider.as_deref(),
        Some("Custom endpoint")
    );
    assert_eq!(capabilities.models[0].name, "fixture-model");
    assert!(capabilities.models[0].efforts.contains(&"high".to_owned()));
    assert!(capabilities.commands.iter().any(|c| c.name == "compress"));
    assert!(replayed
        .events
        .iter()
        .any(|e| matches!(e, Text { text, .. } if text == "ok")));
    let Some(TurnFinished {
        ok: true, usage, ..
    }) = replayed.events.last()
    else {
        panic!("{:?}", replayed.events.last())
    };
    assert_eq!(usage.context_window, 256000);
}

#[test]
fn every_edit_asks_with_its_diff_and_tools_read_as_newens() {
    let replayed = run("edit-bash");
    let uses = tool_uses(&replayed.events);
    assert_eq!(uses[0], ("Write", &json!({ "file_path": "notes.txt" })));
    assert_eq!(uses[1], ("Edit", &json!({ "file_path": "notes.txt" })));
    // Its title shortens the command; its words carry it whole.
    assert_eq!(
        uses[2],
        (
            "Bash",
            &json!({ "command": "mv notes.txt moved.txt && rm moved.txt" })
        )
    );
    assert_eq!(uses.len(), 3, "a card's own updates are not tools");
    let cards: Vec<(&String, &Value)> = replayed
        .events
        .iter()
        .filter_map(|e| match e {
            PermissionRequest { tool, input, .. } => Some((tool, input)),
            _ => None,
        })
        .collect();
    assert_eq!(cards[0].0, "Write");
    assert_eq!(cards[0].1["content"], "hello\n");
    assert_eq!(cards[1].0, "Edit");
    assert_eq!(
        (&cards[1].1["old_string"], &cards[1].1["new_string"]),
        (&json!("hello\n"), &json!("hello world\n"))
    );
}

#[test]
fn a_denied_edit_fails_its_tool_and_the_reply_is_said_once() {
    let replayed = run("deny");
    assert!(replayed
        .expected
        .iter()
        .any(|m| m["result"]["outcome"]["optionId"] == "deny"));
    assert!(replayed
        .events
        .iter()
        .any(|e| matches!(e, ToolResult { is_error: true, .. })));
    let said: Vec<&String> = replayed
        .events
        .iter()
        .filter_map(|e| match e {
            Text { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(said.len(), 1);
    assert_eq!(said[0].matches("Done.").count(), 1, "{said:?}");
    assert!(said[0].contains("File-mutation verifier"));
}

#[test]
fn a_risky_command_asks_with_its_choices_of_remembering() {
    let replayed = run("risky");
    let card = replayed.events.iter().find_map(|e| match e {
        PermissionRequest {
            tool,
            input,
            suggestions,
            ..
        } => Some((tool, input, suggestions)),
        _ => None,
    });
    let (tool, input, suggestions) = card.expect("a card");
    assert_eq!(
        (tool.as_str(), &input["command"]),
        ("Bash", &json!("rm -rf build"))
    );
    assert_eq!(suggestions.len(), 1, "Allow for session / always");
}

#[test]
fn its_task_list_is_drawn_from_its_plan() {
    let replayed = run("todos");
    let uses = tool_uses(&replayed.events);
    let lists: Vec<&&Value> = uses
        .iter()
        .filter(|(n, _)| *n == "TodoWrite")
        .map(|(_, i)| i)
        .collect();
    assert_eq!(lists.len(), 2);
    assert_eq!(
        lists[1]["todos"][1]["status"], "completed",
        "a task given up is done"
    );
    assert!(
        !uses.iter().any(|(n, _)| n.starts_with("todo")),
        "its own todo tool is not drawn again"
    );
}

#[test]
fn a_stopped_turn_ends_interrupted() {
    let replayed = run("interrupt");
    assert!(matches!(
        replayed.events.last(),
        Some(TurnFinished {
            interrupted: true,
            ..
        })
    ));
}

#[test]
fn a_session_resumes_without_its_history_again_and_forks() {
    for (name, method, word) in [
        ("resume", "session/resume", "lantern"),
        ("fork", "session/fork", "harbor"),
    ] {
        let replayed = run(name);
        assert!(
            replayed.written.iter().any(|m| m["method"] == method),
            "{name}"
        );
        let said: Vec<&String> = replayed
            .events
            .iter()
            .filter_map(|e| match e {
                Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert_eq!(
            said,
            [&"ok".to_owned(), &word.to_owned()],
            "{name}: the replay is not said again"
        );
    }
}

#[test]
fn a_mode_is_set_by_its_own_method() {
    let replayed = run("plan");
    assert!(replayed
        .written
        .iter()
        .any(|m| m["method"] == "session/set_mode"));
    assert!(replayed
        .events
        .contains(&ModeChanged { mode: EDITS.into() }));
    assert!(!replayed
        .events
        .iter()
        .any(|e| matches!(e, PermissionRequest { .. })));
}

#[test]
fn newens_own_tools_reach_it_and_read_as_newens() {
    let replayed = run("host");
    let uses = tool_uses(&replayed.events);
    assert_eq!(
        uses.first().map(|u| u.0),
        Some("mcp__sessions__list_sessions")
    );
    assert!(replayed
        .events
        .iter()
        .any(|e| matches!(e, ToolResult { output, .. } if output.contains("No other sessions"))));
}

#[test]
fn newens_tools_read_as_newens_from_the_names_hermes_gives_a_sessions_servers() {
    let servers = vec!["sessions".to_owned(), "browser".to_owned()];
    for (tool, ..) in newen_agent::HOST_TOOLS {
        let call = Call {
            title: format!("mcp__editor_sessions_0a1b2c3d__{tool}"),
            kind: "other".into(),
            ..Default::default()
        };
        assert_eq!(
            TOOLS.tool(&call, &servers).unwrap().0,
            format!("mcp__sessions__{tool}")
        );
    }
    let own = Call {
        title: "mcp__github__create_issue".into(),
        ..Default::default()
    };
    assert_eq!(
        TOOLS.tool(&own, &servers).unwrap().0,
        "mcp__github__create_issue"
    );
    assert!(Hermes::new().session_extras().host);
}

#[test]
fn its_switches_are_its_own_launch_variables() {
    let adapter = Hermes::new();
    let plain = adapter.launch(&SessionSpec {
        cwd: "/work".into(),
        ..Default::default()
    });
    assert!(plain.env.is_empty());
    let mut spec = SessionSpec {
        cwd: "/work".into(),
        ..Default::default()
    };
    spec.options.insert("rules".into(), "false".into());
    spec.options.insert("hooks".into(), "true".into());
    let plan = adapter.launch(&spec);
    assert_eq!(
        plan.env,
        [
            ("HERMES_IGNORE_RULES".into(), "1".into()),
            ("HERMES_ACCEPT_HOOKS".into(), "1".into())
        ]
    );
    assert_eq!(
        (plan.program.as_str(), plan.args.as_slice()),
        ("hermes", &["acp".to_owned()][..])
    );
}

#[test]
fn its_servers_are_read_from_its_list_and_changed_through_its_config() {
    // `hermes mcp list` as it printed it (docs/hermes.md "Customize").
    let listed = "\n  MCP Servers:\n\n  Name             Transport                      Tools        Status    \n  ──────────────── ────────────────────────────── ──────────── ──────────\n  fs               python3 /tmp/claude-1000/...   all          ✓ enabled\n  remote           http://127.0.0.1:9/mcp         all          ✗ disabled\n";
    let servers = Hermes::new().servers(listed);
    let rows: Vec<(&str, &str, McpStatus)> = servers
        .iter()
        .map(|s| (s.name.as_str(), s.target.as_str(), s.status))
        .collect();
    assert_eq!(
        rows,
        [
            ("fs", "python3 /tmp/claude-1000/...", McpStatus::Connected),
            ("remote", "http://127.0.0.1:9/mcp", McpStatus::Disabled)
        ]
    );
    assert!(Hermes::new()
        .servers("\n  No MCP servers configured.\n")
        .is_empty());
    let add = newen_agent::NewServer {
        name: "docs".into(),
        target: "https://x.io/mcp".into(),
        remote: true,
        headers: vec!["X-Key: k".into()],
        ..Default::default()
    };
    let run = Hermes::new()
        .extension_op(&ExtensionOp::AddServer(add))
        .unwrap();
    assert_eq!(run.args[..3], ["config", "set", "mcp_servers.docs"]);
    let entry: Value = serde_json::from_str(&run.args[3]).unwrap();
    assert_eq!(
        entry,
        json!({ "url": "https://x.io/mcp", "headers": { "X-Key": "k" }, "enabled": true })
    );
    let run = Hermes::new()
        .extension_op(&ExtensionOp::RemoveServer("docs".into()))
        .unwrap();
    assert_eq!(run.args, ["config", "unset", "mcp_servers.docs"]);
}
