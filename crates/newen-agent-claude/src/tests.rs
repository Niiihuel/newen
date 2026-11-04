//! Every test replays a capture of the real CLI (`fixtures/2.1.310/`, and `2.1.311/` for MCP,
//! plugins and skills; docs/engine.md §11.4).

use super::*;
use newen_agent::{PromptImage, WebSearchRun};
use std::path::Path;

/// Feed a fixture through a fresh adapter: lines Newen sent become the same commands
/// (so the adapter's state matches), lines the CLI sent are decoded.
fn replay(name: &str) -> (Claude, Vec<AgentEvent>, Vec<String>) {
    replay_at("2.1.310", name)
}

/// [`replay`] of a capture of another version of the CLI.
fn replay_at(version: &str, name: &str) -> (Claude, Vec<AgentEvent>, Vec<String>) {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/{version}/{name}.jsonl"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut claude = Claude::new();
    let (mut events, mut replies) = (vec![], vec![]);
    for line in text.lines() {
        let entry: Value = serde_json::from_str(line).unwrap();
        let msg = &entry["msg"];
        match entry["dir"].as_str() {
            Some("out") if msg["type"] == "control_request" => {
                match msg["request"]["subtype"].as_str() {
                    Some("initialize") => drop(claude.encode(AgentCommand::Start)),
                    Some("interrupt") => drop(claude.encode(AgentCommand::Interrupt)),
                    Some("get_context_usage") => drop(claude.encode(AgentCommand::ContextUsage)),
                    Some("mcp_status") => drop(claude.encode(AgentCommand::McpStatus)),
                    Some("mcp_reconnect") => drop(claude.encode(AgentCommand::McpReconnect(
                        super::text(&msg["request"]["serverName"]),
                    ))),
                    // Requests Newen does not send yet keep the ids of the ones after them in step.
                    Some("get_skills_dialog") => drop(claude.encode(AgentCommand::Skills)),
                    Some("reload_plugins") => drop(claude.encode(AgentCommand::ReloadPlugins)),
                    Some("reload_skills") => drop(claude.request(json!({}))),
                    Some("remote_control") => drop(claude.encode(AgentCommand::Remote {
                        on: msg["request"]["enabled"] == true,
                        name: msg["request"]["name"].as_str().map(str::to_owned),
                    })),
                    _ => {}
                }
            }
            Some("in") => {
                let decoded = claude.decode(&msg.to_string());
                events.extend(decoded.events);
                replies.extend(decoded.replies);
            }
            _ => {}
        }
    }
    (claude, events, replies)
}

fn unknown(events: &[AgentEvent]) -> Vec<&Value> {
    events
        .iter()
        .filter_map(|e| {
            if let AgentEvent::Unknown(v) = e {
                Some(v)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn every_fixture_decodes_without_unknown_messages() {
    for name in [
        "simple",
        "edit-bash",
        "ask",
        "plan",
        "steer",
        "interrupt",
        "rewind",
        "resume",
        "resume-fork",
        "deny",
        "always",
    ] {
        let (_, events, _) = replay(name);
        assert!(
            unknown(&events).is_empty(),
            "{name}: {:?}",
            unknown(&events)
        );
    }
}

#[test]
fn initialize_reports_models_with_their_efforts() {
    let (_, events, _) = replay("simple");
    let Some(AgentEvent::Ready {
        version,
        capabilities,
        mode,
    }) = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Ready { .. }))
    else {
        panic!("no Ready")
    };
    assert_eq!(version, "2.1.310");
    assert_eq!(mode.as_deref(), Some("default"));
    let opus = capabilities
        .models
        .iter()
        .find(|m| m.name == "Opus 5.5")
        .expect("Opus 5.5");
    assert!(opus.efforts.iter().any(|e| e == "low") && opus.efforts.iter().any(|e| e == "max"));
    // `/effort` takes ultracode: every model with levels offers it last.
    assert_eq!(opus.efforts.last().map(String::as_str), Some("ultracode"));
    assert!(capabilities
        .models
        .iter()
        .filter(|m| !m.efforts.is_empty())
        .all(|m| m.efforts.last().map(String::as_str) == Some("ultracode")));
}

#[test]
fn initialize_lists_the_agents_slash_commands_sorted_by_name() {
    let (_, events, _) = replay_at("2.1.311", "extensions");
    let Some(AgentEvent::Ready { capabilities, .. }) = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Ready { .. }))
    else {
        panic!("no Ready")
    };
    let names: Vec<&str> = capabilities
        .commands
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "deep-research",
            "graphify",
            "reverse-engineer-anything",
            "vercel:bootstrap"
        ]
    );
    let graphify = &capabilities.commands[1];
    assert!(graphify
        .description
        .starts_with("Use only when the user types /graphify"));
    assert_eq!(graphify.argument_hint, "");
    assert!(graphify.aliases.is_empty());
}

#[test]
fn system_init_names_the_commands_when_initialize_gave_none() {
    let (mut claude, _, _) = replay("simple");
    let id = claude.initialize.clone().unwrap();
    let answer = json!({ "type": "control_response", "response": {
        "subtype": "success", "request_id": id, "response": { "commands": [] } } });
    claude.decode(&answer.to_string());
    let init = json!({ "type": "system", "subtype": "init", "session_id": "s",
        "slash_commands": ["plan", "clear", "plan"] });
    let events = claude.decode(&init.to_string()).events;
    let Some(AgentEvent::Ready {
        capabilities, mode, ..
    }) = events
        .iter()
        .find(|e| matches!(e, AgentEvent::Ready { .. }))
    else {
        panic!("no Ready")
    };
    let names: Vec<&str> = capabilities
        .commands
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, ["clear", "plan"]);
    assert_eq!(*mode, None);
}

#[test]
fn ultracode_is_extra_high_effort_with_workflows_on() {
    let (mut claude, _, _) = replay("simple");
    let settings = |claude: &mut Claude, effort: &str| -> Value {
        let line = claude
            .encode(AgentCommand::SetEffort(effort.into()))
            .remove(0);
        serde_json::from_str::<Value>(&line).unwrap()["request"]["settings"].clone()
    };
    assert_eq!(
        settings(&mut claude, "ultracode"),
        serde_json::json!({ "effortLevel": "xhigh", "ultracode": true })
    );
    assert_eq!(
        settings(&mut claude, "high"),
        serde_json::json!({ "effortLevel": "high", "ultracode": false })
    );
}

#[test]
fn context_usage_keeps_each_category_with_its_kind_and_the_lists_under_them() {
    let (_, events, _) = replay("context");
    let [AgentEvent::ContextUsage(usage)] = events.as_slice() else {
        panic!("{events:?}");
    };
    assert_eq!((usage.used, usage.max), (15812, 1_000_000));
    let kinds: Vec<(&str, u64, ContextKind)> = usage
        .categories
        .iter()
        .map(|c| (c.name.as_str(), c.tokens, c.kind))
        .collect();
    assert_eq!(kinds[0], ("System prompt", 3331, ContextKind::Used));
    assert_eq!(
        kinds[2],
        ("System tools (deferred)", 14137, ContextKind::Deferred)
    );
    assert_eq!(kinds[7], ("Autocompact buffer", 33000, ContextKind::Buffer));
    assert_eq!(kinds[8], ("Free space", 951188, ContextKind::Free));
    let groups: Vec<(&str, usize)> = usage
        .groups
        .iter()
        .map(|(name, entries)| (name.as_str(), entries.len()))
        .collect();
    assert_eq!(groups, [("Memory files", 2), ("Custom agents", 2)]);
    assert_eq!(
        usage.groups[0].1[0],
        ("/home/user/.claude/CLAUDE.md".into(), 103)
    );
}

#[test]
fn mcp_status_tells_each_server_with_where_it_came_from_and_its_tools() {
    let (_, events, _) = replay_at("2.1.311", "extensions");
    let answers: Vec<&Vec<McpServer>> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::McpServers(servers) => Some(servers),
            _ => None,
        })
        .collect();
    // Asked at once, the servers are still connecting; 15 s later they are connected.
    let [first, later] = answers.as_slice() else {
        panic!("{events:?}");
    };
    let row = |servers: &[McpServer], name: &str| {
        servers.iter().find(|s| s.name == name).cloned().unwrap()
    };
    assert_eq!(row(first, "rea").status, McpStatus::Pending);
    let rea = row(later, "rea");
    assert_eq!(
        (rea.status, rea.source),
        (McpStatus::Connected, McpSource::User)
    );
    assert_eq!(rea.tools.len(), 2);
    let vercel = row(later, "plugin:vercel:vercel");
    assert_eq!(
        (vercel.status, vercel.source, vercel.plugin.as_deref()),
        (McpStatus::NeedsAuth, McpSource::Plugin, Some("vercel"))
    );
    assert_eq!(
        (vercel.label.as_str(), rea.label.as_str()),
        ("vercel", "rea")
    );
    assert_eq!(row(later, "claude.ai Gmail").source, McpSource::Account);
    // Its reconnect's empty answer is a plain reply, not a server list.
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Reply { ok: true, body, .. } if body.is_null())));
    let ready = events.iter().find_map(|e| match e {
        AgentEvent::Ready { capabilities, .. } => Some(capabilities.mcp),
        _ => None,
    });
    assert_eq!(ready, Some(true));
}

/// A mode the CLI refuses is told as refused, not dropped; one it takes is the session's mode
/// from its answer (both as CLI 2.1.316 answers them).
#[test]
fn a_mode_change_is_told_taken_or_refused() {
    let mut claude = Claude::new();
    let id =
        |sent: Vec<String>| serde_json::from_str::<Value>(&sent[0]).unwrap()["request_id"].clone();
    let asked = id(claude.encode(AgentCommand::SetMode("bypassPermissions".into())));
    let refused = json!({"type": "control_response", "response": {"subtype": "error", "request_id": asked,
        "error": "Cannot set permission mode to bypassPermissions because the session was not launched with --dangerously-skip-permissions",
        "error_code": "bypass_not_launched"}});
    let events = claude.decode(&refused.to_string()).events;
    assert!(
        matches!(&events[..], [AgentEvent::ModeRefused { mode, error }]
            if mode == "bypassPermissions" && error.contains("not launched")),
        "{events:?}"
    );
    let asked = id(claude.encode(AgentCommand::SetMode("acceptEdits".into())));
    let taken = json!({"type": "control_response", "response": {"subtype": "success", "request_id": asked,
        "response": {"mode": "acceptEdits"}}});
    assert_eq!(
        claude.decode(&taken.to_string()).events,
        [AgentEvent::ModeChanged {
            mode: "acceptEdits".into()
        }]
    );
}

/// A server added while the session runs: `mcp_set_servers` replaces every server not of
/// `--mcp-config`, so Newen's own SDK servers and the ones added before go in each time (2.1.317,
/// verified with the CLI); one the CLI could not add comes back as a sign-in that failed.
#[test]
fn adding_a_server_keeps_newens_own_and_the_ones_added_before() {
    let mut claude = Claude::new();
    let docs = newen_agent::NewServer {
        name: "context7".into(),
        target: "https://mcp.context7.com/mcp".into(),
        remote: true,
        ..Default::default()
    };
    claude.encode(AgentCommand::McpAdd(docs));
    let probe = newen_agent::NewServer {
        name: "probe".into(),
        target: "python3".into(),
        args: vec!["probe.py".into()],
        env: vec!["TOKEN=x=y".into()],
        ..Default::default()
    };
    let sent = claude.encode(AgentCommand::McpAdd(probe));
    let request: Value = serde_json::from_str(&sent[0]).unwrap();
    assert_eq!(
        request["request"],
        json!({"subtype": "mcp_set_servers", "servers": {
            "sessions": {"type": "sdk", "name": "sessions"},
            "browser": {"type": "sdk", "name": "browser"},
            "context7": {"type": "http", "url": "https://mcp.context7.com/mcp"},
            "probe": {"type": "stdio", "command": "python3", "args": ["probe.py"], "env": {"TOKEN": "x=y"}},
        }})
    );
    // The CLI's answer (2.1.317): what it added and removed, and each server it could not add.
    let id = request["request_id"].clone();
    let added = json!({"type": "control_response", "response": {"subtype": "success", "request_id": id,
        "response": {"added": ["probe"], "removed": [], "errors": {}}}});
    assert!(claude.decode(&added.to_string()).events.is_empty());
    let bad = newen_agent::NewServer {
        name: "bad".into(),
        target: "nope".into(),
        ..Default::default()
    };
    let sent = claude.encode(AgentCommand::McpAdd(bad));
    let id = serde_json::from_str::<Value>(&sent[0]).unwrap()["request_id"].clone();
    let failed = json!({"type": "control_response", "response": {"subtype": "success", "request_id": id,
        "response": {"added": [], "removed": [], "errors": {"bad": "spawn nope ENOENT"}}}});
    assert_eq!(
        claude.decode(&failed.to_string()).events,
        [AgentEvent::McpSignIn {
            server: "bad".into(),
            url: None,
            error: Some("spawn nope ENOENT".into())
        }]
    );
    // Not sent again with the next one.
    let sent = claude.encode(AgentCommand::McpAdd(newen_agent::NewServer {
        name: "other".into(),
        target: "x".into(),
        ..Default::default()
    }));
    let request: Value = serde_json::from_str(&sent[0]).unwrap();
    assert!(request["request"]["servers"].get("bad").is_none());
    // A new process has them from the config: not added again.
    claude.encode(AgentCommand::Start);
    let sent = claude.encode(AgentCommand::McpAdd(newen_agent::NewServer {
        name: "last".into(),
        target: "x".into(),
        ..Default::default()
    }));
    let request: Value = serde_json::from_str(&sent[0]).unwrap();
    let servers = request["request"]["servers"].as_object().unwrap();
    assert_eq!(
        servers.keys().map(String::as_str).collect::<Vec<_>>(),
        ["browser", "last", "sessions"]
    );
}

#[test]
fn signing_in_to_a_server_gives_its_page_or_why_not() {
    let mut claude = Claude::new();
    let sent = claude.encode(AgentCommand::McpAuthenticate("vercel".into()));
    let request: Value = serde_json::from_str(&sent[0]).unwrap();
    // No redirect address: the CLI takes the page's answer on its own localhost listener.
    assert_eq!(
        request["request"],
        json!({"subtype": "mcp_authenticate", "serverName": "vercel"})
    );
    let id = request["request_id"].clone();
    // The reply's shape is the CLI's schema (2.1.311, at 0x10f65f60).
    let reply = json!({"type": "control_response", "response": {"subtype": "success", "request_id": id,
        "response": {"authUrl": "https://mcp.vercel.com/authorize?x=1", "requiresUserAction": true,
        "callbackExpected": true, "redirectScheme": "localhost", "callbackPort": 41000}}});
    let events = claude.decode(&reply.to_string()).events;
    assert_eq!(
        events,
        [AgentEvent::McpSignIn {
            server: "vercel".into(),
            url: Some("https://mcp.vercel.com/authorize?x=1".into()),
            error: None
        }]
    );
    let sent = claude.encode(AgentCommand::McpAuthenticate("rea".into()));
    let id = serde_json::from_str::<Value>(&sent[0]).unwrap()["request_id"].clone();
    let refused = json!({"type": "control_response", "response": {"subtype": "error", "request_id": id,
        "error": "Server type \"stdio\" does not support OAuth authentication"}});
    let events = claude.decode(&refused.to_string()).events;
    assert!(
        matches!(&events[..], [AgentEvent::McpSignIn { url: None, error: Some(e), .. }] if e.contains("stdio"))
    );
    // Only an http or sse server not signed in by the account can be signed in from Newen.
    let (_, events, _) = replay_at("2.1.311", "extensions");
    let Some(AgentEvent::McpServers(servers)) = events
        .iter()
        .rfind(|e| matches!(e, AgentEvent::McpServers(_)))
    else {
        panic!("{events:?}");
    };
    let can: Vec<(&str, bool)> = servers
        .iter()
        .map(|s| (s.label.as_str(), s.sign_in))
        .take(3)
        .collect();
    assert_eq!(
        can,
        [
            ("vercel", true),
            ("rea", false),
            ("claude.ai Claude Docs", false)
        ]
    );
}

#[test]
fn skills_and_plugins_are_read_as_the_cli_lists_them() {
    let (_, events, _) = replay_at("2.1.311", "extensions");
    let skills = events.iter().find_map(|e| match e {
        AgentEvent::Skills(skills) => Some(skills),
        _ => None,
    });
    let skills = skills.expect("get_skills_dialog's rows");
    assert_eq!(skills.len(), 6);
    let sources: Vec<&str> = skills.iter().map(|s| s.source.as_str()).collect();
    assert!(sources.contains(&"user") && sources.contains(&"plugin"));
    assert!(skills.iter().all(|s| s.state == "on" && !s.name.is_empty()));
    // A plugin's skill is the plugin's to switch, as the CLI says.
    let of_plugin = skills.iter().find(|s| s.source == "plugin").unwrap();
    assert!(of_plugin
        .locked
        .as_deref()
        .unwrap()
        .contains("managed with the plugin"));
    let claude = Claude::new();
    let read = |name: &str| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/2.1.311/{name}"));
        std::fs::read_to_string(path).unwrap()
    };
    // A plain list (without --available) and the full one both read.
    let (plugins, offers) = claude.plugins(&read("plugin-list.json"));
    assert_eq!((plugins.len(), offers.len()), (7, 0));
    let first = &plugins[0];
    assert_eq!(
        (
            first.id.as_str(),
            first.name.as_str(),
            first.version.as_deref(),
            first.scope.as_str(),
            first.enabled
        ),
        (
            "agent-skills@addy-agent-skills",
            "agent-skills",
            Some("0.6.12"),
            "user",
            true
        )
    );
    let (plugins, offers) = claude.plugins(&read("plugin-list-available.json"));
    assert_eq!((plugins.len(), offers.len()), (2, 6));
    assert_eq!(offers[0].marketplace, "claude-plugins-official");
    assert!(offers[0].installs.is_some_and(|n| n > 1_000_000));
    let markets = claude.marketplaces(&read("marketplace-list.json"));
    let named: Vec<(&str, &str)> = markets
        .iter()
        .map(|m| (m.name.as_str(), m.source.as_str()))
        .collect();
    assert!(named.contains(&("better-claude", "midudev/better-claude")));
    assert!(named.contains(&("ponytail", "https://github.com/DietrichGebert/ponytail.git")));
    // What it cannot read is nothing, not a crash.
    assert!(claude.plugins("not json").0.is_empty() && claude.marketplaces("").is_empty());
}

#[test]
fn the_pages_options_reach_the_cli_as_its_own_settings() {
    let claude = Claude::new();
    let keys: Vec<_> = claude.options().into_iter().map(|o| o.key).collect();
    assert_eq!(
        keys,
        [
            "output_style",
            "sandbox",
            "strict_sandbox",
            "prompt_suggestions"
        ]
    );
    let settings = |pairs: &[(&str, &str)]| {
        let spec = SessionSpec {
            options: pairs
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
            ..SessionSpec::default()
        };
        let args = claude.launch(&spec).args;
        args.iter()
            .position(|a| a == "--settings")
            .map(|at| args[at + 1].clone())
    };
    assert_eq!(settings(&[]), None);
    assert_eq!(settings(&[("output_style", "")]), None);
    assert_eq!(
        settings(&[("output_style", "Learning"), ("sandbox", "true")]).as_deref(),
        Some(r#"{"outputStyle":"Learning","sandbox":{"enabled":true}}"#)
    );
    assert_eq!(
        settings(&[("strict_sandbox", "true")]).as_deref(),
        Some(r#"{"sandbox":{"allowUnsandboxedCommands":false,"enabled":true}}"#)
    );
}

#[test]
fn mcp_servers_are_listed_added_and_removed_with_the_clis_own_commands() {
    let claude = Claude::new();
    let listed = claude.servers(
        "Checking MCP server health\u{2026}\n\n\
         claude.ai Gmail: https://gmailmcp.googleapis.com/mcp/v1 - \u{2714} Connected\n\
         plugin:vercel:vercel: https://mcp.vercel.com (HTTP) - ! Needs authentication\n\
         rea: /home/me/bin/rea mcp - \u{2718} Failed to connect\n",
    );
    let names: Vec<_> = listed
        .iter()
        .map(|s| (s.name.as_str(), s.target.as_str(), s.status, s.source))
        .collect();
    use newen_agent::{McpSource, McpStatus};
    assert_eq!(
        names,
        [
            (
                "claude.ai Gmail",
                "https://gmailmcp.googleapis.com/mcp/v1",
                McpStatus::Connected,
                McpSource::Account
            ),
            (
                "plugin:vercel:vercel",
                "https://mcp.vercel.com",
                McpStatus::NeedsAuth,
                McpSource::Plugin
            ),
            (
                "rea",
                "/home/me/bin/rea mcp",
                McpStatus::Failed,
                McpSource::Other
            ),
        ]
    );
    // Each is labelled as its own product is called, for its logo.
    let labels: Vec<_> = listed.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["Gmail", "vercel", "rea"]);
    let args = |server| {
        claude
            .extension_op(&ExtensionOp::AddServer(server))
            .unwrap()
            .args
            .join(" ")
    };
    let remote = newen_agent::NewServer {
        name: "ctx".into(),
        target: "https://mcp.context7.com/mcp".into(),
        remote: true,
        scope: Some("user".into()),
        headers: vec!["X-Key: 1".into()],
        ..Default::default()
    };
    assert_eq!(
        args(remote),
        "mcp add --transport http --scope user -H X-Key: 1 ctx https://mcp.context7.com/mcp"
    );
    let local = newen_agent::NewServer {
        name: "pg".into(),
        target: "npx".into(),
        args: vec!["-y".into(), "@x/pg".into()],
        env: vec!["TOKEN=a".into()],
        ..Default::default()
    };
    assert_eq!(args(local), "mcp add -e TOKEN=a pg npx -- -y @x/pg");
    assert_eq!(
        claude
            .extension_op(&ExtensionOp::RemoveServer("pg".into()))
            .unwrap()
            .args
            .join(" "),
        "mcp remove pg"
    );
}

#[test]
fn changes_run_the_clis_own_commands() {
    let claude = Claude::new();
    let args = |op: ExtensionOp| claude.extension_op(&op).unwrap().args.join(" ");
    let plugin = Plugin {
        id: "commit@better-claude".into(),
        name: "commit".into(),
        version: None,
        scope: "project".into(),
        enabled: true,
    };
    assert_eq!(
        args(ExtensionOp::SwitchPlugin {
            plugin: plugin.clone(),
            enabled: false
        }),
        "plugin disable commit@better-claude --scope project --json"
    );
    assert_eq!(
        args(ExtensionOp::Uninstall(plugin)),
        "plugin uninstall commit@better-claude --scope project --json"
    );
    // The person's own install takes no scope, as Desktop's "Install for me".
    assert_eq!(
        args(ExtensionOp::Install {
            id: "a@m".into(),
            scope: Some("user".into())
        }),
        "plugin install a@m --json"
    );
    assert_eq!(
        args(ExtensionOp::Install {
            id: "a@m".into(),
            scope: Some("local".into())
        }),
        "plugin install a@m --scope local --json"
    );
    // A pasted command or ssh address is cleaned as Desktop's dialog does.
    assert_eq!(
        args(ExtensionOp::AddMarketplace(
            "claude plugin marketplace add git@github.com:o/r.git".into()
        )),
        "plugin marketplace add https://github.com/o/r --json"
    );
    assert_eq!(
        args(ExtensionOp::AddMarketplace("  gh repo clone o/r ".into())),
        "plugin marketplace add o/r --json"
    );
    // A skill's state goes on stdin, with the names the CLI resolves it by.
    let skill = Skill {
        name: "graphify".into(),
        description: String::new(),
        source: "user".into(),
        state: "on".into(),
        locked: None,
        key: json!({"name": "graphify", "handles": {"unqualified_name": "graphify"}}),
    };
    let run = claude
        .extension_op(&ExtensionOp::SkillState {
            skill,
            state: "off".into(),
        })
        .unwrap();
    assert_eq!(run.args, ["edit-skill-overrides", "--json"]);
    let edit: Value = serde_json::from_str(run.stdin.as_deref().unwrap()).unwrap();
    assert_eq!(
        edit,
        json!({"name": "graphify", "state": "off", "handles": {"unqualified_name": "graphify"}})
    );
    // A --json result that did not go through says why.
    assert_eq!(
        claude.extension_failed(
            "noise\n{\"outcome\":\"failed\",\"message\":\"not found in any marketplace\"}\n"
        ),
        Some("not found in any marketplace".into())
    );
    assert_eq!(claude.extension_failed("{\"outcome\":\"ok\"}"), None);
    assert_eq!(
        claude.extension_failed("{\"ok\":false,\"error\":\"locked\"}"),
        Some("locked".into())
    );
}

#[test]
fn a_simple_turn_streams_text_and_finishes() {
    let (_, events, _) = replay("simple");
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Session { id } if !id.is_empty())));
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::TextDelta { .. })));
    let finished: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, AgentEvent::TurnFinished { ok: true, .. }))
        .collect();
    assert_eq!(finished.len(), 1);
}

#[test]
fn edits_and_bash_carry_their_files_and_command() {
    let (_, events, replies) = replay("edit-bash");
    let results: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let AgentEvent::ToolResult { files, command, .. } = e {
                Some((files, command))
            } else {
                None
            }
        })
        .collect();
    // In this capture the model created the file with Bash, not Write: only a snapshot sees it.
    assert_eq!(results.len(), 3, "Bash, Edit, Bash");
    assert!(results[0].0.is_empty());
    assert!(results[0]
        .1
        .as_ref()
        .is_some_and(|c| c.command.starts_with("printf")));
    let edit = &results[1].0[0];
    assert_eq!(
        (edit.before.as_deref(), edit.after.as_deref()),
        (Some("hello\n"), Some("hello world\n"))
    );
    assert_eq!(
        results[2].1.as_ref().map(|c| c.command.as_str()),
        Some("mv notes.txt moved.txt && rm moved.txt")
    );
    // Each tool's end is reported by the hook and acknowledged at once, all under one prompt.
    let prompts: std::collections::HashSet<_> = events
        .iter()
        .filter_map(|e| {
            if let AgentEvent::ToolDone { prompt, .. } = e {
                prompt.clone()
            } else {
                None
            }
        })
        .collect();
    assert_eq!(prompts.len(), 1);
    assert_eq!(replies.len(), 3);
}

#[test]
fn a_permission_request_is_answered_with_its_call_and_input() {
    let (mut claude, events, _) = replay("deny");
    let Some(AgentEvent::PermissionRequest { id, tool, call, .. }) = events
        .iter()
        .find(|e| matches!(e, AgentEvent::PermissionRequest { .. }))
    else {
        panic!("no permission request")
    };
    assert_eq!(tool, "Bash");
    // The fixture already denied it; ask again as if it were still pending.
    claude.pending.insert(
        id.clone(),
        (json!(call), json!({ "command": "touch denied.txt" })),
    );
    let line = &claude.encode(AgentCommand::Decide {
        id: id.clone(),
        decision: Decision::Allow,
    })[0];
    let reply: Value = serde_json::from_str(line).unwrap();
    assert_eq!(reply["response"]["response"]["behavior"], "allow");
    assert_eq!(
        reply["response"]["response"]["updatedInput"]["command"],
        "touch denied.txt"
    );
    assert_eq!(reply["response"]["response"]["toolUseID"], json!(call));
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolResult { is_error: true, .. })));
}

#[test]
fn always_allow_returns_only_the_rules() {
    let (mut claude, events, _) = replay("always");
    let Some(AgentEvent::PermissionRequest {
        id, suggestions, ..
    }) = events
        .iter()
        .find(|e| matches!(e, AgentEvent::PermissionRequest { .. }))
    else {
        panic!("no permission request")
    };
    assert!(suggestions
        .iter()
        .any(|s| matches!(s, Suggestion::Mode(m) if m == "acceptEdits")));
    let rules: Vec<_> = suggestions
        .iter()
        .filter(|s| matches!(s, Suggestion::Rules(_)))
        .cloned()
        .collect();
    claude.pending.insert(id.clone(), (json!("t"), json!({})));
    let line = &claude.encode(AgentCommand::Decide {
        id: id.clone(),
        decision: Decision::AllowAlways(rules),
    })[0];
    let reply: Value = serde_json::from_str(line).unwrap();
    let kept = reply["response"]["response"]["updatedPermissions"]
        .as_array()
        .unwrap();
    assert!(kept.iter().all(|p| p["type"] == "addRules"), "{kept:?}");
}

#[test]
fn a_question_is_interactive_and_answered_with_answers() {
    let (mut claude, events, _) = replay("ask");
    let Some(AgentEvent::PermissionRequest {
        id,
        interactive,
        input,
        ..
    }) = events
        .iter()
        .find(|e| matches!(e, AgentEvent::PermissionRequest { .. }))
    else {
        panic!("no question")
    };
    assert!(interactive);
    claude
        .pending
        .insert(id.clone(), (json!("t"), input.clone()));
    let answers = json!({ "Pick red or blue.": "blue" });
    let line = &claude.encode(AgentCommand::Decide {
        id: id.clone(),
        decision: Decision::Answer(answers.clone()),
    })[0];
    let reply: Value = serde_json::from_str(line).unwrap();
    assert_eq!(
        reply["response"]["response"]["updatedInput"]["answers"],
        answers
    );
    assert!(reply["response"]["response"]["updatedInput"]["questions"].is_array());
}

#[test]
fn an_interrupted_turn_finishes_interrupted() {
    let (_, events, _) = replay("interrupt");
    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::TurnFinished {
            ok: false,
            interrupted: true,
            ..
        }
    )));
    assert!(events.iter().any(
        |e| matches!(e, AgentEvent::Reply { ok: true, body, .. } if body["still_queued"].is_array())
    ));
}

#[test]
fn rewind_dry_run_reports_the_files() {
    let (_, events, _) = replay("rewind");
    // A Write reports the new file whole, with nothing before it.
    assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolResult { files, .. }
        if files.len() == 1 && files[0].before.is_none() && files[0].after.as_deref() == Some("one\n"))));
    let reply = events
        .iter()
        .find_map(|e| {
            if let AgentEvent::Reply { body, .. } = e {
                body.get("canRewind").map(|_| body)
            } else {
                None
            }
        })
        .expect("rewind reply");
    assert_eq!(reply["filesChanged"].as_array().map(Vec::len), Some(1));
}

#[test]
fn the_running_request_and_mode_changes_are_reported() {
    let (_, events, _) = replay("always");
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Running { requests } if requests.len() == 1)));
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ModeChanged { mode } if mode == "acceptEdits")));
}

#[test]
fn launch_never_claims_to_be_desktop() {
    let plan = Claude::new().launch(&SessionSpec {
        cwd: "/work".into(),
        resume: Some("s".into()),
        fork: true,
        ..Default::default()
    });
    assert!(
        plan.args.contains(&"--resume=s".to_string())
            && plan.args.contains(&"--fork-session".to_string())
    );
    assert!(plan
        .env_remove
        .contains(&"CLAUDE_CODE_ENTRYPOINT".to_string()));
    assert!(!plan.env.iter().any(|(_, v)| v.contains("desktop")));
}

#[test]
fn a_fork_from_a_message_resumes_up_to_it_and_replies_carry_their_ids() {
    let plan = Claude::new().launch(&SessionSpec {
        cwd: "/work".into(),
        resume: Some("s".into()),
        fork: true,
        resume_at: Some("u1".into()),
        ..Default::default()
    });
    assert!(plan.args.contains(&"--resume-session-at=u1".to_string()));
    let (_, events, _) = replay("always");
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Text { id: Some(id), .. } if !id.is_empty())));
}

#[test]
fn plan_usage_and_plan_come_from_rate_limits_and_initialize() {
    let (_, events, _) = replay("always");
    let limits = events.iter().find_map(|e| match e {
        AgentEvent::Limits(limits) => Some(*limits),
        _ => None,
    });
    let limits = limits.expect("a rate_limit_event");
    let session = limits.session.unwrap();
    assert!((session.utilization - 0.14).abs() < 1e-6);
    assert_eq!(session.resets_at, 1_792_926_000);
    assert!((limits.weekly.unwrap().utilization - 0.04).abs() < 1e-6);
    let plan = events.iter().find_map(|e| match e {
        AgentEvent::Ready { capabilities, .. } => Some(capabilities.plan.clone()),
        _ => None,
    });
    assert!(plan.is_some());
}

#[test]
fn a_helper_agent_is_a_task_tied_to_its_call() {
    let (_, events, _) = replay("agent");
    let call = events.iter().find_map(|e| match e {
        AgentEvent::ToolUse { call, name, .. } if name == "Agent" => Some(call.clone()),
        _ => None,
    });
    assert!(call.is_some());
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::TaskStarted { call: c, description, kind, .. }
            if *c == call && description == "Read README and list repo" && kind == "agent")));
    // What it used as it went, and in all once it ended.
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::TaskProgress { usage, .. } if usage.tool_uses == 1 && usage.tokens == 11312)));
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::TaskEnded { call: c, summary: Some(_), ok: true, stopped: false, usage: Some(u), .. }
            if *c == call && u.tokens == 14773 && u.duration_ms == 3390)));
    // Its own steps come tied to the call that started it.
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::ToolUse { name, parent: Some(p), .. } if name == "Bash" && Some(p) == call.as_ref())));
}

#[test]
fn a_workflow_reports_its_name_phases_and_agents() {
    let (_, events, _) = replay_at("2.1.316", "workflow");
    assert!(events.iter().any(|e| matches!(e,
        AgentEvent::TaskStarted { kind, .. } if kind == "workflow")));
    let snapshots: Vec<_> = (events.iter())
        .filter_map(|e| match e {
            AgentEvent::TaskPhases { name, phases, .. } => Some((name.clone(), phases.clone())),
            _ => None,
        })
        .collect();
    let (name, first) = &snapshots[0];
    assert_eq!(name.as_deref(), Some("file-words"));
    let titles: Vec<_> = first.iter().map(|p| p.title.as_str()).collect();
    assert_eq!(titles, ["Read", "Sum up"]);
    // The first agent has started, the second waits in the same phase.
    assert_eq!(first[0].agents.len(), 2);
    assert_eq!(first[0].agents[0].state, newen_agent::TaskState::Running);
    // At the last snapshot every agent is done, each with what it gave back.
    let (_, last) = snapshots.last().unwrap();
    let agents: Vec<_> = last.iter().flat_map(|p| &p.agents).collect();
    assert_eq!(agents.len(), 3);
    assert!(agents
        .iter()
        .all(|a| a.state == newen_agent::TaskState::Done));
    assert_eq!(last[1].agents[0].label, "sum up");
    assert!(last[1].agents[0]
        .doing
        .as_deref()
        .is_some_and(|d| d.starts_with("The README.md")));
    // It ends as any task does, and the CLI then answers in a turn of its own.
    let ended = events
        .iter()
        .position(|e| matches!(e, AgentEvent::TaskEnded { ok: true, .. }));
    let finished = events
        .iter()
        .rposition(|e| matches!(e, AgentEvent::TurnFinished { .. }));
    assert!(ended.is_some() && finished > ended);
}

#[test]
fn a_turn_counts_its_tokens_as_it_goes() {
    let (_, events, _) = replay("think");
    let counts: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Tokens { tokens } => Some(*tokens),
            _ => None,
        })
        .collect();
    // It counts while the model thinks, and the thinking's estimate gives way to the answer's size.
    let output = events.iter().find_map(|e| match e {
        AgentEvent::TurnFinished { usage, .. } => Some(usage.output_tokens),
        _ => None,
    });
    assert!(counts.len() >= 2 && counts[0] > 0, "{counts:?}");
    assert_eq!(counts.last().copied(), output);
}

#[test]
fn thinking_streams_and_ends_whole_before_the_answer() {
    let (_, events, _) = replay_at("2.1.311", "think");
    let streamed: String = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ThinkingDelta { text, parent: None } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let whole = events
        .iter()
        .find_map(|e| match e {
            AgentEvent::ThinkingText { text, parent: None } => Some(text.as_str()),
            _ => None,
        })
        .expect("no ThinkingText");
    assert!(
        whole.contains("391") && whole.contains("prime"),
        "{whole:?}"
    );
    // The streamed pieces keep the whitespace the final text trims.
    assert!(streamed.starts_with(whole), "{streamed:?} vs {whole:?}");
}

#[test]
fn thinking_text_is_asked_for_at_launch() {
    let plan = Claude::new().launch(&SessionSpec {
        cwd: "/work".into(),
        ..Default::default()
    });
    assert!(plan
        .args
        .windows(2)
        .any(|w| w == ["--thinking-display", "summarized"]));
}

#[test]
fn newens_own_tools_are_served_in_process_and_never_ask() {
    let (mut claude, events, replies) = replay("sessions");
    // The handshake and the tool list are answered by the adapter itself.
    let answered = |method: &str| {
        replies.iter().any(|line| {
            let reply: Value = serde_json::from_str(line).unwrap();
            let result = &reply["response"]["response"]["mcp_response"]["result"];
            match method {
                "initialize" => result["serverInfo"]["name"] == "sessions",
                // Every one of Newen's tools, start_session first, with its `agent`.
                _ => {
                    result["tools"].as_array().map(Vec::len) == Some(newen_agent::HOST_TOOLS.len())
                        && result["tools"][0]["name"] == "start_session"
                        && result["tools"][0]["inputSchema"]["properties"]["agent"]["type"]
                            == "string"
                }
            }
        })
    };
    assert!(answered("initialize") && answered("tools/list"));
    // The call goes to the engine; the permission it would need is granted at once.
    let Some(AgentEvent::HostTool { id, name, input }) = events
        .iter()
        .find(|e| matches!(e, AgentEvent::HostTool { .. }))
    else {
        panic!("no HostTool")
    };
    assert_eq!(name, "start_session");
    assert_eq!(input["initiation"], "own_initiative");
    assert!(!events.iter().any(|e| matches!(
        e,
        AgentEvent::PermissionRequest { tool, .. } if tool.starts_with("mcp__sessions__")
    )));
    // The engine's answer goes back under the call's JSON-RPC id.
    let line = claude
        .encode(AgentCommand::HostToolResult {
            id: id.clone(),
            content: vec![
                ToolContent::Text("Proposed".into()),
                ToolContent::Image {
                    mime: "image/jpeg".into(),
                    data: "AAAA".into(),
                },
            ],
            is_error: false,
        })
        .remove(0);
    let reply: Value = serde_json::from_str(&line).unwrap();
    let response = &reply["response"]["response"]["mcp_response"];
    assert_eq!(response["id"], 2);
    assert_eq!(response["result"]["content"][0]["text"], "Proposed");
    // A picture goes as MCP's image block.
    assert_eq!(
        response["result"]["content"][1],
        serde_json::json!({ "type": "image", "data": "AAAA", "mimeType": "image/jpeg" })
    );
}

#[test]
fn a_tool_result_s_pictures_are_read_for_the_engine_to_keep() {
    // A screenshot as the Messages API's block, which a Read of an image and an MCP tool's
    // picture answer with (inferred: no capture holds one); a picture by address is not kept.
    let result = json!({ "type": "user", "message": { "role": "user", "content": [{
        "type": "tool_result", "tool_use_id": "t1", "content": [
            { "type": "text", "text": "Captured" },
            { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "iVBORw0K" } },
            { "type": "image", "source": { "type": "url", "url": "https://example.com/a.png" } },
        ] }] } });
    let events = Claude::new().decode(&result.to_string()).events;
    let [AgentEvent::ToolResult {
        output,
        images,
        inline,
        ..
    }] = &events[..]
    else {
        panic!("{events:?}");
    };
    assert_eq!(output, "Captured");
    assert!(images.is_empty(), "the engine keeps them, not the adapter");
    assert_eq!(
        inline,
        &[ToolContent::Image {
            mime: "image/png".into(),
            data: "iVBORw0K".into()
        }]
    );
}

#[test]
fn the_browser_server_lists_its_tools_and_its_calls_never_ask() {
    // The same protocol as the sessions server's capture, for the server named "browser".
    let mut claude = Claude::new();
    let ask = |claude: &mut Claude, id: &str, request: Value| {
        claude.decode(
            &serde_json::json!({ "type": "control_request", "request_id": id, "request": request })
                .to_string(),
        )
    };
    let listed = ask(
        &mut claude,
        "r1",
        serde_json::json!({ "subtype": "mcp_message", "server_name": "browser",
            "message": { "jsonrpc": "2.0", "id": 1, "method": "tools/list" } }),
    );
    let reply: Value = serde_json::from_str(&listed.replies[0]).unwrap();
    let tools = reply["response"]["response"]["mcp_response"]["result"]["tools"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(tools.len(), BROWSER_TOOLS.len());
    assert_eq!(tools[0]["name"], "navigate");
    assert_eq!(tools[0]["inputSchema"]["required"][0], "url");
    // Loaded with the session, not deferred behind tool search.
    assert_eq!(tools[0]["_meta"]["anthropic/alwaysLoad"], true);
    let allowed = ask(
        &mut claude,
        "r2",
        serde_json::json!({ "subtype": "can_use_tool", "tool_name": "mcp__browser__computer",
            "input": { "action": "screenshot" }, "tool_use_id": "t1" }),
    );
    assert!(allowed.events.is_empty());
    let reply: Value = serde_json::from_str(&allowed.replies[0]).unwrap();
    assert_eq!(reply["response"]["response"]["behavior"], "allow");
}

#[test]
fn a_prompt_with_an_image_sends_the_image_before_the_text() {
    let mut claude = Claude::new();
    let image = PromptImage {
        media_type: "image/png".into(),
        data: b"\x89PNG".to_vec(),
    };
    let line = &claude.encode(AgentCommand::Prompt {
        request: "r1".into(),
        text: "look".into(),
        steer: false,
        images: vec![image],
    })[0];
    let msg: Value = serde_json::from_str(line).unwrap();
    assert_eq!(
        msg["message"]["content"],
        json!([
            {
                "type": "image",
                "source": { "type": "base64", "media_type": "image/png", "data": "iVBORw==" },
            },
            { "type": "text", "text": "look" },
        ])
    );
}

#[test]
fn a_prompt_without_images_keeps_one_text_block() {
    let mut claude = Claude::new();
    let line = &claude.encode(AgentCommand::Prompt {
        request: "r1".into(),
        text: "look".into(),
        steer: false,
        images: vec![],
    })[0];
    let msg: Value = serde_json::from_str(line).unwrap();
    assert_eq!(
        msg["message"]["content"],
        json!([{ "type": "text", "text": "look" }])
    );
}

/// A steer goes out as the CLI's `priority:"next"`, the shape Desktop sent in `steer.jsonl`.
#[test]
fn a_steer_prompt_is_the_clis_next_priority() {
    let fixture = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/2.1.310/steer.jsonl"),
    )
    .unwrap();
    let desktop: Value = fixture
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|entry| entry["msg"]["priority"] == "next")
        .unwrap()["msg"]
        .clone();
    let mut claude = Claude::new();
    let steer: Value = serde_json::from_str(
        &claude.encode(AgentCommand::Prompt {
            request: "r2".into(),
            text: "Change of plans: stop at 5.".into(),
            steer: true,
            images: vec![],
        })[0],
    )
    .unwrap();
    assert_eq!(steer["priority"], desktop["priority"]);
    assert_eq!(steer["type"], desktop["type"]);
    let mut claude = Claude::new();
    let plain: Value = serde_json::from_str(
        &claude.encode(AgentCommand::Prompt {
            request: "r3".into(),
            text: "later".into(),
            steer: false,
            images: vec![],
        })[0],
    )
    .unwrap();
    assert!(plain.get("priority").is_none());
}

/// The CLI in the steer capture can take a message into its running turn.
#[test]
fn the_claude_adapter_offers_steering() {
    let (_, events, _) = replay("steer");
    let steer = events
        .iter()
        .any(|event| matches!(event, AgentEvent::Ready { capabilities, .. } if capabilities.steer));
    assert!(steer);
}

#[test]
fn web_search_results_keep_their_query_and_sources() {
    let (_, events, _) = replay_at("2.1.311", "websearch");
    assert!(unknown(&events).is_empty());
    let runs: Vec<&WebSearchRun> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolResult { web: Some(run), .. } => Some(run),
            _ => None,
        })
        .collect();
    // Two searches in one turn, each with its own query and the ten links the Links list gave.
    let [first, second] = runs.as_slice() else {
        panic!("{events:?}")
    };
    assert_eq!(
        first.query,
        "latest stable Rust version release announcement"
    );
    assert_eq!(first.sources.len(), 10);
    assert_eq!(first.sources[0].url, "https://www.linux.org/threads/phoronix-rust-1-94-released-with-stable-support-for-avx-512-fp16-intrinsics-array-windows.63426/latest");
    assert_eq!(
        second.query,
        "Rust stable release 2026 latest version announced blog.rust-lang.org"
    );
    assert_eq!(second.sources.len(), 10);
    assert_eq!(second.sources[0].title, "Announcing Rust 1.97.0");
    assert_eq!(
        second.sources[0].url,
        "https://blog.rust-lang.org/2026/07/09/Rust-1.97.0/"
    );
}

// `claude auth status --json` as 2.1.312 printed it.
#[test]
fn auth_state_reads_the_cli_status() {
    let claude = Claude::new();
    let signed_in = r#"{"loggedIn":true,"authMethod":"claude.ai","email":"a@b.c","orgName":"Org","subscriptionType":"max"}"#;
    assert_eq!(
        claude.auth_state(signed_in, 0),
        AuthState::SignedIn {
            method: "claude.ai".into(),
            email: Some("a@b.c".into()),
            org: Some("Org".into()),
            plan: Some("max".into()),
        }
    );
    // Signed out exits 1 and still prints the JSON.
    let out = r#"{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}"#;
    assert_eq!(claude.auth_state(out, 1), AuthState::SignedOut);
    let key = r#"{"loggedIn":true,"authMethod":"api_key","apiKeySource":"ANTHROPIC_API_KEY"}"#;
    assert!(matches!(
        claude.auth_state(key, 0),
        AuthState::SignedIn { method, email: None, .. } if method == "api_key"
    ));
    assert!(matches!(
        claude.auth_state("oops", 2),
        AuthState::Unknown(_)
    ));
}

#[test]
fn login_url_strips_the_osc_8_wrapper() {
    let line = "If the browser didn't open, visit: \x1b]8;;https://claude.com/cai/oauth/authorize?code=true&state=x\x07https://claude.com/cai/oauth/authorize?code=true&state=x\x1b]8;;\x07";
    assert_eq!(
        Claude::new().login_url(line).as_deref(),
        Some("https://claude.com/cai/oauth/authorize?code=true&state=x")
    );
    assert_eq!(Claude::new().login_url("Opening browser to sign in…"), None);
}

#[test]
fn an_unauthenticated_turn_asks_to_sign_in() {
    let mut claude = Claude::new();
    let mut events = claude.decode(r#"{"type":"assistant","error":"authentication_failed","message":{"content":[{"type":"text","text":"Not logged in"}]}}"#).events;
    events.extend(
        claude
            .decode(
                r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in"}"#,
            )
            .events,
    );
    assert!(matches!(
        &events[..],
        [
            AgentEvent::Text { .. },
            AgentEvent::AuthRequired,
            AgentEvent::TurnFinished { ok: false, .. }
        ]
    ));
}

#[test]
fn the_mode_words_are_desktops_and_the_default_is_manual() {
    let claude = Claude::new();
    let words: Vec<(String, String)> = claude
        .modes()
        .into_iter()
        .map(|w| (w.id, w.label))
        .collect();
    assert_eq!(
        words,
        [
            ("default", "Manual"),
            ("acceptEdits", "Accept edits"),
            ("plan", "Plan"),
            ("auto", "Auto"),
            ("bypassPermissions", "Bypass permissions"),
        ]
        .map(|(id, label)| (id.to_owned(), label.to_owned()))
    );
    assert_eq!(claude.default_mode(), "default");
    assert!(claude.login_takes_code());
}

#[test]
fn global_servers_ride_in_mcp_config_with_only_the_names_of_their_secrets() {
    use newen_agent::InjectedServer;
    let dir = std::env::temp_dir().join(format!("newen-claude-plugin-{}", std::process::id()));
    // quill's plugin, named `quill`, left in the folder: Newen's takes its place.
    std::fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
    std::fs::write(
        dir.join(".claude-plugin/plugin.json"),
        r#"{"name":"quill","description":"Skills for all agents, kept by quill"}"#,
    )
    .unwrap();
    let plan = Claude::new().launch(&SessionSpec {
        cwd: "/work".into(),
        servers: vec![
            InjectedServer {
                name: "github".into(),
                target: "npx".into(),
                args: vec!["-y".into(), "server-github".into()],
                env: vec![("LOG".into(), "warn".into())],
                env_vars: vec![(
                    "GITHUB_TOKEN".into(),
                    "NEWEN_MCP_GITHUB_GITHUB_TOKEN".into(),
                )],
                ..Default::default()
            },
            InjectedServer {
                name: "linear".into(),
                target: "https://mcp.linear.app/mcp".into(),
                remote: true,
                sse: true,
                header_vars: vec![(
                    "Authorization".into(),
                    "NEWEN_MCP_LINEAR_H_AUTHORIZATION".into(),
                )],
                ..Default::default()
            },
        ],
        skill_roots: vec![dir.join("skills")],
        ..Default::default()
    });
    let at = plan.args.iter().position(|a| a == "--mcp-config").unwrap();
    let config: serde_json::Value = serde_json::from_str(&plan.args[at + 1]).unwrap();
    let github = &config["mcpServers"]["github"];
    assert_eq!(github["type"], "stdio");
    assert_eq!(
        github["env"]["GITHUB_TOKEN"],
        "${NEWEN_MCP_GITHUB_GITHUB_TOKEN}"
    );
    assert_eq!(github["env"]["LOG"], "warn");
    let linear = &config["mcpServers"]["linear"];
    assert_eq!(linear["type"], "sse");
    assert_eq!(
        linear["headers"]["Authorization"],
        "${NEWEN_MCP_LINEAR_H_AUTHORIZATION}"
    );
    // The generated plugin the skills come in.
    let plugin = plan.args.iter().position(|a| a == "--plugin-dir").unwrap();
    assert_eq!(plan.args[plugin + 1], dir.to_string_lossy());
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join(".claude-plugin/plugin.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["name"], "newen");
    let _ = std::fs::remove_dir_all(dir);
    // Without any, the launch is as it was.
    let plain = Claude::new().launch(&SessionSpec::default());
    assert!(!plain
        .args
        .iter()
        .any(|a| a == "--mcp-config" || a == "--plugin-dir"));
    assert!(Claude::new().session_extras().sse);
}

#[test]
fn remote_control_turns_on_and_off_and_brings_the_phones_messages() {
    let (mut claude, events, _) = replay_at("2.1.316", "remote");
    assert!(matches!(
        events.first(),
        Some(AgentEvent::Ready { capabilities, .. }) if capabilities.remote
    ));
    // On, with the session's page on claude.ai, then off.
    let remote: Vec<_> = (events.iter())
        .filter_map(|e| match e {
            AgentEvent::Remote { on, url, error } => Some((*on, url.clone(), error.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        remote,
        [
            (
                true,
                Some("https://claude.ai/code/session_01REMOTE".into()),
                None
            ),
            (false, None, None)
        ]
    );
    // The phone's two messages, and not the echo of the one the capture sent itself.
    let prompts: Vec<&str> = (events.iter())
        .filter_map(|e| match e {
            AgentEvent::Prompt { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        prompts,
        [
            "Usá la herramienta Bash para correr exactamente: touch remoto.txt",
            "Escribí una historia de 500 palabras sobre una pluma"
        ]
    );
    // The permission approved on the phone is no longer Newen's to answer.
    let asked = (events.iter())
        .find_map(|e| match e {
            AgentEvent::PermissionRequest { id, .. } => Some(id.clone()),
            _ => None,
        })
        .unwrap();
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::Withdrawn { id } if *id == asked)));
    // Newen's own messages are not taken for the phone's.
    let mine = claude.encode(AgentCommand::Prompt {
        request: "r1".into(),
        text: "hi".into(),
        steer: false,
        images: vec![],
    });
    assert_eq!(mine.len(), 1);
    let echo = json!({ "type": "user", "isReplay": true, "uuid": "r1", "origin": { "kind": "human" },
        "message": { "role": "user", "content": "hi" } });
    assert!(claude.decode(&echo.to_string()).events.is_empty());
    assert!(Claude::new()
        .launch(&SessionSpec::default())
        .args
        .contains(&"--replay-user-messages".to_string()));
}

#[test]
fn the_models_are_asked_outside_a_session_with_the_default_named() {
    let claude = Claude::new();
    let asked: Value = serde_json::from_str(&claude.models_query().unwrap()[0]).unwrap();
    assert_eq!(asked["request"]["subtype"], "initialize");
    // The CLI's answer to `initialize` (2.1.316 [F:remote]), under the query's id.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/2.1.316/remote.jsonl");
    let text = std::fs::read_to_string(path).unwrap();
    let answer = (text.lines())
        .map(|l| serde_json::from_str::<Value>(l).unwrap()["msg"].clone())
        .find(|m| m["type"] == "control_response" && m["response"]["request_id"] == "q_1")
        .unwrap();
    let mut answer = answer;
    answer["response"]["request_id"] = asked["request_id"].clone();
    let models = claude.models_answer(&answer.to_string()).unwrap();
    // "Default (recommended)" is Opus 5.5 there: that one is marked, not listed twice.
    assert!(models.iter().all(|m| m.id != "default"));
    let default: Vec<&str> = (models.iter())
        .filter(|m| m.default)
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(default, ["Opus 5.5"]);
    assert!(claude.models_answer("{\"type\":\"system\"}").is_none());
}

#[test]
fn a_suggested_prompt_comes_after_a_turn_but_the_first() {
    let (_, events, _) = replay_at("2.1.316", "suggest");
    assert!(unknown(&events).is_empty(), "{:?}", unknown(&events));
    let turns_and_suggestions: Vec<Option<&str>> = (events.iter())
        .filter_map(|e| match e {
            AgentEvent::TurnFinished { .. } => Some(None),
            AgentEvent::PromptSuggestion { text } => Some(Some(text.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(turns_and_suggestions, [None, None, Some("add a docstring")]);
    // One with nothing to say gives nothing.
    let blank = Claude::new().decode(r#"{"type":"prompt_suggestion","suggestion":"  "}"#);
    assert!(blank.events.is_empty());
}

#[test]
fn a_session_asks_for_suggested_prompts_unless_the_person_turned_them_off() {
    let asks = |options: &[(&str, &str)]| {
        let mut claude = Claude::new();
        claude.launch(&SessionSpec {
            options: (options.iter())
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
            ..SessionSpec::default()
        });
        let line = claude.encode(AgentCommand::Start).remove(0);
        serde_json::from_str::<Value>(&line).unwrap()["request"]["promptSuggestions"] == true
    };
    assert!(asks(&[]));
    assert!(asks(&[("prompt_suggestions", "true")]));
    assert!(!asks(&[("prompt_suggestions", "false")]));
    let switch = (Claude::new().options().into_iter())
        .find(|o| o.key == "prompt_suggestions")
        .unwrap();
    assert_eq!(
        (switch.label.as_str(), switch.default.as_str()),
        ("Prompt suggestions", "true")
    );
    assert!(switch.choices.is_empty());
}

#[test]
fn a_session_is_told_how_to_work_with_the_user_first() {
    let mut claude = Claude::new();
    let line = claude.encode(AgentCommand::Start).remove(0);
    let asked: Value = serde_json::from_str(&line).unwrap();
    let prompt = asked["request"]["appendSystemPrompt"].as_str().unwrap();
    assert!(prompt.starts_with(newen_agent::WORK_GUIDE));
}

/// The person's tool permissions become Claude Code's own rules for the session, in
/// `--settings`: allow, ask and deny by the tool's `mcp__<server>__<tool>` name.
#[test]
fn tool_permissions_are_the_sessions_allow_ask_and_deny_rules() {
    use newen_agent::{ToolPolicy, ToolRule};
    let claude = Claude::new();
    assert_eq!(claude.tool_policies(), ToolPolicy::ALL.to_vec());
    let rule = |server: &str, tool: &str, policy| ToolRule {
        server: server.into(),
        tool: tool.into(),
        policy,
    };
    let spec = SessionSpec {
        tool_rules: vec![
            rule("github", "list_issues", ToolPolicy::Allow),
            rule("github", "create_issue", ToolPolicy::Ask),
            rule("my.server", "delete_repo", ToolPolicy::Block),
        ],
        ..SessionSpec::default()
    };
    let args = claude.launch(&spec).args;
    let at = args.iter().position(|a| a == "--settings").unwrap();
    let settings: Value = serde_json::from_str(&args[at + 1]).unwrap();
    assert_eq!(
        settings["permissions"],
        json!({
            "allow": ["mcp__github__list_issues"],
            "ask": ["mcp__github__create_issue"],
            "deny": ["mcp__my_server__delete_repo"],
        })
    );
    // None chosen, no permissions of Newen's.
    let plain = claude.launch(&SessionSpec::default()).args;
    assert!(!plain.iter().any(|a| a.contains("permissions")));
}

/// An OAuth client of the person's own: its id an argument, its secret typed where the CLI
/// asks for it, never an argument.
#[test]
fn a_servers_oauth_client_secret_goes_to_stdin_not_the_arguments() {
    let claude = Claude::new();
    assert!(claude.server_oauth_client());
    let run = claude
        .extension_op(&ExtensionOp::AddServer(newen_agent::NewServer {
            name: "notion".into(),
            target: "https://mcp.notion.com/mcp".into(),
            remote: true,
            client_id: Some("abc".into()),
            client_secret: Some("s3cret".into()),
            ..Default::default()
        }))
        .unwrap();
    assert_eq!(
        run.args.join(" "),
        "mcp add --transport http --client-id abc --client-secret notion https://mcp.notion.com/mcp"
    );
    assert_eq!(run.stdin.as_deref(), Some("s3cret\n"));
    assert!(!run.args.iter().any(|a| a.contains("s3cret")));
}
