//! Replays of captures of the real CLI (`fixtures/2.2.5/`, recorded with `newen-capture
//! --agent opencode <scenario> --model opencode/big-pickle`, docs/opencode.md "Capturas"). The
//! adapter must write what it wrote then, line for line.

use super::*;
use newen_agent::AgentEvent::{self, *};
use newen_agent::{Decision, InjectedServer};
use newen_agent_acp::replay::{replay, Replayed};

fn run(name: &str) -> Replayed {
    let path = format!("{}/fixtures/2.2.5/{name}.jsonl", env!("CARGO_MANIFEST_DIR"));
    let capture = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let replayed = replay(&mut OpenCode::new(), &capture);
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
fn a_session_opens_with_its_models_and_modes_and_a_turn_answers() {
    let replayed = run("simple");
    // Its commands come after the session opens, and Newen hears it ready again with them.
    let ready = replayed.events.iter().rev().find_map(|e| match e {
        Ready {
            version,
            capabilities,
            mode,
        } => Some((version, capabilities, mode)),
        _ => None,
    });
    let (version, capabilities, mode) = ready.expect("ready");
    assert!(OpenCode::new().tested().contains(&version.as_str()));
    assert_eq!(mode.as_deref(), Some(BUILD));
    assert_eq!(capabilities.modes, [BUILD, PLAN, ASK]);
    let words: Vec<String> = OpenCode::new().modes().into_iter().map(|m| m.id).collect();
    assert_eq!(
        capabilities.modes, words,
        "its words cover every mode it offers"
    );
    let default = capabilities.models.iter().find(|m| m.default).unwrap();
    assert_eq!(default.id, "opencode/big-pickle");
    assert!(capabilities.commands.iter().any(|c| c.name == "init"));
    assert!(capabilities.resume && capabilities.fork && !capabilities.steer);
    let turn = replayed.turns()[0];
    assert!(turn.contains(&Text {
        text: "ok".into(),
        parent: None,
        id: Some("msg_1a26fb496001tlR42zdk6WbHro".into()),
        at: None
    }));
    let Some(TurnFinished {
        ok: true, usage, ..
    }) = turn.last()
    else {
        panic!("{turn:?}")
    };
    assert_eq!((usage.context_tokens, usage.context_window), (5368, 200000));
}

#[test]
fn writing_editing_and_running_read_as_newens_tools() {
    let replayed = run("edit-bash");
    let uses = tool_uses(&replayed.events);
    assert_eq!(
        uses[0],
        (
            "Write",
            &json!({ "file_path": "/work/notes.txt", "content": "hello\n" })
        )
    );
    assert_eq!(uses[1].0, "Edit");
    assert_eq!(
        (&uses[1].1["old_string"], &uses[1].1["new_string"]),
        (&json!("hello"), &json!("hello world"))
    );
    assert_eq!(uses[2].0, "Bash");
    assert_eq!(
        uses[2].1["command"],
        "mv notes.txt moved.txt && rm moved.txt"
    );
    let edited = replayed.events.iter().find_map(|e| match e {
        ToolResult { files, .. } if !files.is_empty() => Some(files),
        _ => None,
    });
    let file = &edited.expect("the edit's diff")[0];
    assert_eq!(
        (file.before.as_deref(), file.after.as_deref()),
        (Some("hello\n"), Some("hello world\n"))
    );
    let ran = replayed.events.iter().find_map(|e| match e {
        ToolResult {
            command: Some(run), ..
        } => Some(run),
        _ => None,
    });
    assert_eq!(
        ran.unwrap().command,
        "mv notes.txt moved.txt && rm moved.txt"
    );
}

#[test]
fn a_permission_is_a_card_and_denying_it_stops_the_turn() {
    let replayed = run("deny");
    let card = replayed.events.iter().find_map(|e| match e {
        PermissionRequest {
            tool,
            input,
            call,
            suggestions,
            ..
        } => Some((tool, input, call, suggestions)),
        _ => None,
    });
    let (tool, input, call, suggestions) = card.expect("a card");
    assert_eq!(
        (tool.as_str(), &input["file_path"]),
        ("Read", &json!("/work/.env"))
    );
    assert_eq!(call.as_deref(), Some("call_00_y262smygd6xna50qgjn6rwx7"));
    assert_eq!(suggestions.len(), 1, "Always allow");
    assert!(matches!(
        replayed.events.last(),
        Some(TurnFinished {
            ok: false,
            interrupted: true,
            ..
        })
    ));
}

#[test]
fn always_allowing_picks_its_always_option() {
    let replayed = run("always");
    assert!(replayed
        .expected
        .iter()
        .any(|m| m["result"]["outcome"]["optionId"] == "always"));
    assert!(matches!(
        replayed.events.last(),
        Some(TurnFinished { ok: true, .. })
    ));
}

#[test]
fn its_question_tool_is_the_question_card() {
    let replayed = run("ask");
    let card = replayed.events.iter().find_map(|e| match e {
        PermissionRequest {
            tool,
            input,
            interactive: true,
            ..
        } => Some((tool, input)),
        _ => None,
    });
    let (tool, input) = card.expect("a question");
    assert_eq!(tool, "AskUserQuestion");
    let question = &input["questions"][0];
    assert_eq!(question["question"], "Which color do you prefer?");
    assert_eq!(question["options"][1]["label"], "Blue");
    assert_eq!(question["other"], true);
    assert!(replayed
        .events
        .iter()
        .any(|e| matches!(e, Text { text, .. } if text == "Red")));
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
fn a_session_resumes_and_forks_with_its_memory() {
    for (name, method, word) in [
        ("resume", "session/resume", "lantern"),
        ("fork", "session/fork", "harbor"),
    ] {
        let replayed = run(name);
        assert!(
            replayed.written.iter().any(|m| m["method"] == method),
            "{name}"
        );
        let sessions: Vec<&String> = replayed
            .events
            .iter()
            .filter_map(|e| match e {
                Session { id } => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(
            sessions[0] == sessions[1],
            name == "resume",
            "{name}: {sessions:?}"
        );
        assert!(
            replayed
                .events
                .iter()
                .any(|e| matches!(e, Text { text, .. } if text == word)),
            "{name}"
        );
    }
}

#[test]
fn plan_mode_is_set_before_the_turn() {
    let replayed = run("plan");
    assert!(replayed.events.contains(&ModeChanged { mode: PLAN.into() }));
}

#[test]
fn newens_own_tools_reach_it_through_code_mode_and_read_as_newens() {
    let replayed = run("host");
    let uses = tool_uses(&replayed.events);
    assert!(
        uses.contains(&("mcp__sessions__list_sessions", &json!({}))),
        "{uses:?}"
    );
    let session_new = replayed
        .written
        .iter()
        .find(|m| m["method"] == "session/new")
        .unwrap();
    let names: Vec<&str> = (session_new["params"]["mcpServers"]
        .as_array()
        .unwrap()
        .iter())
    .map(|s| s["name"].as_str().unwrap())
    .collect();
    assert_eq!(names, ["sessions", "browser"]);
}

#[test]
fn newens_tools_are_offered_as_its_own_servers_and_read_as_newens() {
    let adapter = OpenCode::new();
    assert!(adapter.session_extras().host);
    let servers = vec!["sessions".to_owned(), "browser".to_owned()];
    for (tool, ..) in newen_agent::HOST_TOOLS {
        let direct = Call {
            name: format!("sessions_{tool}"),
            ..Default::default()
        };
        assert_eq!(
            TOOLS.tool(&direct, &servers).unwrap().0,
            format!("mcp__sessions__{tool}")
        );
        let code =
            format!("const r = await tools.sessions.{tool}({{\"session_id\": 2}});\nreturn r;");
        let scripted = Call {
            name: "execute".into(),
            input: json!({ "code": code }),
            ..Default::default()
        };
        let (name, input) = TOOLS.tool(&scripted, &servers).unwrap();
        assert_eq!(
            (name, input),
            (format!("mcp__sessions__{tool}"), json!({ "session_id": 2 }))
        );
    }
    let browse = Call {
        name: "browser_navigate".into(),
        ..Default::default()
    };
    assert_eq!(
        TOOLS.tool(&browse, &servers).unwrap().0,
        "mcp__browser__navigate"
    );
    let _ = (Decision::Allow, InjectedServer::default());
}

#[test]
fn a_cancelled_task_is_done_for_the_list() {
    let todos = json!({ "todos": [{ "content": "x", "status": "cancelled" }] });
    let call = Call {
        name: "todowrite".into(),
        input: todos,
        ..Default::default()
    };
    assert_eq!(
        TOOLS.tool(&call, &[]).unwrap().1["todos"][0]["status"],
        "completed"
    );
}

#[test]
fn its_session_gets_newens_skills_and_ask_mode_and_the_persons_options_in_its_own_config() {
    let adapter = OpenCode::new();
    let mut spec = SessionSpec {
        cwd: "/work".into(),
        skill_roots: vec!["/data/newen/injected/opencode/skills".into()],
        ..Default::default()
    };
    spec.options.insert("free_models".into(), "false".into());
    let plan = adapter.launch(&spec);
    assert_eq!(
        (plan.program.as_str(), plan.args.as_slice()),
        ("opencode", &["acp".to_owned()][..])
    );
    let config = plan
        .env
        .iter()
        .find(|(k, _)| k == "OPENCODE_CONFIG_CONTENT")
        .unwrap();
    let config: Value = serde_json::from_str(&config.1).unwrap();
    assert_eq!(
        config["skills"],
        json!(["/data/newen/injected/opencode/skills"])
    );
    assert_eq!(config["agents"]["ask"]["permissions"][1]["action"], "shell");
    assert!(plan
        .env
        .contains(&("OPENCODE_DISABLE_FREE_MODELS".into(), "1".into())));
    assert!(!plan
        .env
        .iter()
        .any(|(k, _)| k == "OPENCODE_DISABLE_CLAUDE_CODE"));
    let ids: Vec<String> = adapter.options().into_iter().map(|o| o.key).collect();
    assert_eq!(ids, ["free_models", "claude_skills", "project_config"]);
    assert!(adapter.session_extras().skills);
}

#[test]
fn its_servers_are_read_from_its_config_layers() {
    let path = format!(
        "{}/fixtures/2.2.5/debug-config.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let servers = OpenCode::new().servers(&std::fs::read_to_string(path).unwrap());
    let rows: Vec<(&str, &str, McpSource)> = servers
        .iter()
        .map(|s| (s.name.as_str(), s.target.as_str(), s.source))
        .collect();
    assert_eq!(
        rows,
        [
            ("web", "https://example.invalid/mcp", McpSource::User),
            ("demo", "echo hi", McpSource::Project)
        ]
    );
    assert!(servers
        .iter()
        .all(|s| !s.checked && s.status == McpStatus::Connected));
}

#[test]
fn a_server_is_added_with_its_own_command_where_the_person_chose() {
    let adapter = OpenCode::new();
    let remote = newen_agent::NewServer {
        name: "docs".into(),
        target: "https://x.io/mcp".into(),
        remote: true,
        headers: vec!["Authorization: Bearer k".into()],
        ..Default::default()
    };
    let run = adapter
        .extension_op(&ExtensionOp::AddServer(remote))
        .unwrap();
    assert_eq!(
        run.args,
        [
            "mcp",
            "add",
            "docs",
            "--global",
            "--url",
            "https://x.io/mcp",
            "--header",
            "Authorization=Bearer k"
        ]
    );
    let local = newen_agent::NewServer {
        name: "pg".into(),
        target: "npx".into(),
        args: vec!["-y".into(), "@x/pg".into()],
        env: vec!["PG=1".into()],
        scope: Some(PROJECT.into()),
        ..Default::default()
    };
    let run = adapter
        .extension_op(&ExtensionOp::AddServer(local))
        .unwrap();
    assert_eq!(
        run.args,
        ["mcp", "add", "pg", "--env", "PG=1", "--", "npx", "-y", "@x/pg"]
    );
    assert!(adapter
        .extension_op(&ExtensionOp::RemoveServer("pg".into()))
        .is_none());
}

#[test]
fn newens_ask_mode_asks_before_an_edit() {
    let replayed = run("ask-mode");
    assert!(replayed.events.contains(&ModeChanged { mode: ASK.into() }));
    let card = replayed.events.iter().find_map(|e| match e {
        PermissionRequest { tool, input, .. } => Some((tool, input)),
        _ => None,
    });
    let (tool, input) = card.expect("a card");
    assert_eq!(
        (tool.as_str(), &input["file_path"]),
        ("Write", &json!("/work/asked.txt"))
    );
}
