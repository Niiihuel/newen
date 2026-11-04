//! The protocol as ACP's spec writes it, without any agent's quirks: those are tested by each
//! agent's crate against captures of its real CLI.

use super::*;
use newen_agent::AgentEvent::*;

struct Plain;
impl Flavor for Plain {}
static PLAIN: Plain = Plain;

fn started(spec: SessionSpec) -> (Acp, Vec<Value>) {
    let mut acp = Acp::new(&PLAIN);
    acp.remember(&spec);
    let lines = acp.encode(AgentCommand::Start);
    (
        acp,
        lines
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
    )
}

fn feed(acp: &mut Acp, msg: Value) -> (Vec<AgentEvent>, Vec<Value>) {
    let decoded = acp.decode(&msg.to_string());
    let replies = decoded
        .replies
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    (decoded.events, replies)
}

/// A session open, with the agent's options: a model with two efforts and two modes.
fn open() -> Acp {
    let (mut acp, _) = started(SessionSpec {
        cwd: "/work".into(),
        ..Default::default()
    });
    let init = json!({ "jsonrpc": "2.0", "id": 1, "result": { "protocolVersion": 1,
        "agentCapabilities": { "loadSession": true, "sessionCapabilities": { "resume": {} } },
        "agentInfo": { "name": "x", "version": "1.2.3" } } });
    feed(&mut acp, init);
    let options = json!([
        { "id": "model", "category": "model", "type": "select", "currentValue": "p/a",
          "options": [{ "value": "p/a", "name": "A" }, { "value": "p/b", "name": "B" }] },
        { "id": "effort", "category": "thought_level", "type": "select", "currentValue": "default",
          "options": [{ "value": "low", "name": "Low" }, { "value": "default", "name": "Default" }] },
        { "id": "mode", "category": "mode", "type": "select", "currentValue": "build",
          "options": [{ "value": "build", "name": "Build" }, { "value": "plan", "name": "Plan" }] },
    ]);
    let (events, _) = feed(
        &mut acp,
        json!({ "jsonrpc": "2.0", "id": 2, "result": { "sessionId": "s1", "configOptions": options } }),
    );
    let Some(Ready {
        capabilities,
        mode,
        version,
    }) = events.get(1)
    else {
        panic!("{events:?}")
    };
    assert_eq!(
        (version.as_str(), mode.as_deref()),
        ("1.2.3", Some("build"))
    );
    assert_eq!(capabilities.modes, ["build", "plan"]);
    assert_eq!(capabilities.models[0].efforts, ["low"]);
    assert!(capabilities.models[1].efforts.is_empty() && capabilities.resume);
    acp
}

#[test]
fn it_opens_a_session_with_newens_servers_and_says_who_it_is() {
    let spec = SessionSpec {
        cwd: "/work".into(),
        servers: vec![
            InjectedServer {
                name: "sessions".into(),
                target: "/bin/newen".into(),
                args: vec!["--mcp-host".into(), "sessions".into()],
                env: vec![("K".into(), "v".into())],
                ..Default::default()
            },
            InjectedServer {
                name: "docs".into(),
                target: "https://x.io/mcp".into(),
                remote: true,
                headers: vec![("A".into(), "b".into())],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let (mut acp, lines) = started(spec);
    assert_eq!(lines[0]["params"]["clientInfo"]["name"], "newen");
    assert!(lines[0]["params"]["clientCapabilities"]["elicitation"]["form"].is_object());
    let init = json!({ "jsonrpc": "2.0", "id": 1, "result": { "agentCapabilities": {} } });
    let (_, replies) = feed(&mut acp, init);
    assert_eq!(replies[0]["method"], "session/new");
    let servers = &replies[0]["params"]["mcpServers"];
    assert_eq!(
        servers[0],
        json!({ "name": "sessions", "command": "/bin/newen", "args": ["--mcp-host", "sessions"],
            "env": [{ "name": "K", "value": "v" }] })
    );
    assert_eq!(servers[1]["type"], "http");
    assert_eq!(
        servers[1]["headers"][0],
        json!({ "name": "A", "value": "b" })
    );
}

#[test]
fn a_resumed_session_keeps_its_id_and_its_replay_is_not_shown_again() {
    let (mut acp, _) = started(SessionSpec {
        cwd: "/work".into(),
        resume: Some("s0".into()),
        ..Default::default()
    });
    let init = json!({ "jsonrpc": "2.0", "id": 1, "result": { "agentCapabilities": { "loadSession": true } } });
    let (_, replies) = feed(&mut acp, init);
    assert_eq!(replies[0]["method"], "session/load");
    assert_eq!(replies[0]["params"]["sessionId"], "s0");
    let replay = json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s0",
        "update": { "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "old" } } } });
    assert!(feed(&mut acp, replay).0.is_empty());
    let (events, _) = feed(&mut acp, json!({ "jsonrpc": "2.0", "id": 2, "result": {} }));
    assert_eq!(events[0], Session { id: "s0".into() });
}

#[test]
fn a_prompt_waits_for_the_session_and_its_turn_streams_and_ends() {
    let mut acp = open();
    let lines = acp.encode(AgentCommand::Prompt {
        request: "r1".into(),
        text: "hi".into(),
        steer: false,
        images: vec![PromptImage {
            media_type: "image/png".into(),
            data: vec![1, 2],
        }],
    });
    let prompt: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(prompt["method"], "session/prompt");
    assert_eq!(
        prompt["params"]["prompt"][0],
        json!({ "type": "image", "mimeType": "image/png", "data": "AQI=" })
    );
    let chunk = |kind: &str, words: &str| {
        json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s1",
        "update": { "sessionUpdate": kind, "messageId": "m1", "content": { "type": "text", "text": words } } } })
    };
    let (events, _) = feed(&mut acp, chunk("agent_thought_chunk", "hmm"));
    assert_eq!(
        events[0],
        Running {
            requests: vec!["r1".into()]
        }
    );
    assert_eq!(events[1], Thinking { parent: None });
    let (events, _) = feed(&mut acp, chunk("agent_message_chunk", "Hel"));
    assert!(matches!(&events[0], ThinkingText { text, .. } if text == "hmm"));
    feed(&mut acp, chunk("agent_message_chunk", "lo"));
    let usage = json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s1",
        "update": { "sessionUpdate": "usage_update", "used": 900, "size": 1000, "cost": { "amount": 0.5, "currency": "USD" } } } });
    feed(&mut acp, usage);
    let id = prompt["id"].clone();
    let end = json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn", "usage": { "inputTokens": 7, "outputTokens": 3 } } });
    let (events, _) = feed(&mut acp, end);
    assert!(matches!(&events[0], Text { text, id: Some(m), .. } if text == "Hello" && m == "m1"));
    let TurnFinished {
        ok: true,
        interrupted: false,
        usage,
        error: None,
    } = &events[1]
    else {
        panic!("{events:?}")
    };
    assert_eq!(
        (
            usage.input_tokens,
            usage.context_tokens,
            usage.context_window,
            usage.cost_usd
        ),
        (7, 900, 1000, Some(0.5))
    );
}

#[test]
fn a_mode_asked_during_a_turn_is_set_when_it_ends() {
    let mut acp = open();
    let prompt = acp.encode(AgentCommand::Prompt {
        request: "r1".into(),
        text: "x".into(),
        steer: false,
        images: vec![],
    });
    let id = serde_json::from_str::<Value>(&prompt[0]).unwrap()["id"].clone();
    assert!(acp.encode(AgentCommand::SetMode("plan".into())).is_empty());
    let (_, replies) = feed(
        &mut acp,
        json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "end_turn" } }),
    );
    assert_eq!(replies[0]["method"], "session/set_config_option");
    assert_eq!(
        replies[0]["params"],
        json!({ "sessionId": "s1", "configId": "mode", "value": "plan" })
    );
    let set = replies[0]["id"].clone();
    let options = json!([{ "id": "mode", "category": "mode", "currentValue": "plan",
        "options": [{ "value": "build" }, { "value": "plan" }] }]);
    let (events, _) = feed(
        &mut acp,
        json!({ "jsonrpc": "2.0", "id": set, "result": { "configOptions": options } }),
    );
    assert_eq!(
        events[0],
        ModeChanged {
            mode: "plan".into()
        }
    );
}

#[test]
fn a_permission_is_asked_once_and_newens_own_tools_never_are() {
    let mut acp = open();
    let ask = |id: u64, tool: Value| {
        json!({ "jsonrpc": "2.0", "id": id, "method": "session/request_permission",
        "params": { "sessionId": "s1", "toolCall": tool, "options": [
            { "optionId": "once", "kind": "allow_once", "name": "Allow once" },
            { "optionId": "always", "kind": "allow_always", "name": "Always allow" },
            { "optionId": "reject", "kind": "reject_once", "name": "Reject" }] } })
    };
    let shell = json!({ "toolCallId": "c1", "kind": "execute", "title": "ls", "rawInput": { "command": "ls" } });
    let (events, replies) = feed(&mut acp, ask(0, shell));
    assert!(replies.is_empty());
    let PermissionRequest {
        id,
        tool,
        input,
        suggestions,
        ..
    } = &events[0]
    else {
        panic!("{events:?}")
    };
    assert_eq!((tool.as_str(), &input["command"]), ("Bash", &json!("ls")));
    let decided = acp.encode(AgentCommand::Decide {
        id: id.clone(),
        decision: Decision::AllowAlways(suggestions.clone()),
    });
    let decided: Value = serde_json::from_str(&decided[0]).unwrap();
    assert_eq!(
        decided,
        json!({ "jsonrpc": "2.0", "id": 0, "result": { "outcome": { "outcome": "selected", "optionId": "always" } } })
    );
    struct Named;
    impl Flavor for Named {
        fn tool(&self, call: &Call, _: &[String]) -> Option<(String, Value)> {
            Some((mcp_name("sessions", &call.name), call.input.clone()))
        }
    }
    static NAMED: Named = Named;
    acp.flavor = &NAMED;
    let ours =
        json!({ "toolCallId": "c2", "kind": "other", "name": "list_sessions", "title": "x" });
    let (events, replies) = feed(&mut acp, ask(1, ours));
    assert!(events.is_empty());
    assert_eq!(replies[0]["result"]["outcome"]["optionId"], "once");
}

#[test]
fn a_form_of_questions_is_the_question_card_and_its_answers_go_back_as_values() {
    let mut acp = open();
    let schema = json!({ "type": "object", "properties": {
        "q0": { "type": "string", "title": "Runtime", "description": "Which runtime?",
                "oneOf": [{ "const": "bun", "title": "Bun" }, { "const": "node", "title": "Node" }] },
        "q0_custom": { "type": "string" },
        "q1": { "type": "array", "description": "What matters?", "items": { "enum": ["Fast", "Small"] } },
        "answer": { "type": "string" } } });
    let form = json!({ "jsonrpc": "2.0", "id": 4, "method": "elicitation/create",
        "params": { "mode": "form", "sessionId": "s1", "message": "Name it?", "requestedSchema": schema } });
    let (events, _) = feed(&mut acp, form);
    let PermissionRequest {
        id,
        tool,
        input,
        interactive: true,
        ..
    } = &events[0]
    else {
        panic!("{events:?}")
    };
    assert_eq!(tool, "AskUserQuestion");
    let questions = input["questions"].as_array().unwrap();
    assert_eq!(questions.len(), 3, "the custom field is its question's");
    let answers =
        json!({ "Which runtime?": "Deno", "What matters?": "Fast, Small", "Name it?": "newen" });
    let lines = acp.encode(AgentCommand::Decide {
        id: id.clone(),
        decision: Decision::Answer(answers),
    });
    let reply: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(
        reply["result"],
        json!({ "action": "accept", "content": { "answer": "newen", "q0_custom": "Deno", "q1": ["Fast", "Small"] } })
    );
}

#[test]
fn an_interrupted_turn_answers_its_asks_cancelled_and_ends_stopped() {
    let mut acp = open();
    let prompt = acp.encode(AgentCommand::Prompt {
        request: "r1".into(),
        text: "x".into(),
        steer: false,
        images: vec![],
    });
    let id = serde_json::from_str::<Value>(&prompt[0]).unwrap()["id"].clone();
    let ask = json!({ "jsonrpc": "2.0", "id": 9, "method": "session/request_permission", "params": {
        "sessionId": "s1", "toolCall": { "toolCallId": "c1", "kind": "edit", "title": "a.txt" },
        "options": [{ "optionId": "once", "kind": "allow_once" }] } });
    let (events, _) = feed(&mut acp, ask);
    assert_eq!(
        events[0],
        Running {
            requests: vec!["r1".into()]
        }
    );
    let lines = acp.encode(AgentCommand::Interrupt);
    assert!(lines[0].contains("session/cancel") && lines[1].contains("\"cancelled\""));
    let (events, _) = feed(
        &mut acp,
        json!({ "jsonrpc": "2.0", "id": id, "result": { "stopReason": "cancelled" } }),
    );
    assert_eq!(events[0], Withdrawn { id: "9".into() });
    assert!(matches!(
        events[1],
        TurnFinished {
            ok: false,
            interrupted: true,
            ..
        }
    ));
}

#[test]
fn a_tool_is_drawn_once_its_input_is_whole_and_its_diff_is_the_change() {
    let mut acp = open();
    acp.encode(AgentCommand::Prompt {
        request: "r1".into(),
        text: "x".into(),
        steer: false,
        images: vec![],
    });
    let update = |u: Value| json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "sessionId": "s1", "update": u } });
    let (events, _) = feed(
        &mut acp,
        update(json!({ "sessionUpdate": "tool_call", "toolCallId": "c1",
        "kind": "edit", "title": "edit", "status": "pending", "rawInput": {} })),
    );
    assert_eq!(
        events,
        [Running {
            requests: vec!["r1".into()]
        }]
    );
    let (events, _) = feed(
        &mut acp,
        update(
            json!({ "sessionUpdate": "tool_call_update", "toolCallId": "c1",
        "status": "in_progress", "rawInput": { "filePath": "/work/a.txt", "oldString": "a", "newString": "b" } }),
        ),
    );
    assert_eq!(
        events,
        [ToolUse {
            call: "c1".into(),
            name: "Edit".into(),
            parent: None,
            input: json!({ "file_path": "/work/a.txt", "old_string": "a", "new_string": "b" })
        }]
    );
    let (events, _) = feed(
        &mut acp,
        update(
            json!({ "sessionUpdate": "tool_call_update", "toolCallId": "c1",
        "status": "completed", "content": [{ "type": "diff", "path": "/work/a.txt", "oldText": "a\n", "newText": "b\n" }] }),
        ),
    );
    let ToolResult {
        files,
        is_error: false,
        ..
    } = &events[0]
    else {
        panic!("{events:?}")
    };
    assert_eq!(files[0].before.as_deref(), Some("a\n"));
}
