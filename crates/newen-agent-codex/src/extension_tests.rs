//! What Codex says about its extensions, parsed from the shapes of `fixtures/0.163.0/*.json`
//! (`codex mcp list --json`, `plugin list --json --available`, `plugin marketplace list --json`
//! and the app-server's `skills/list` and `mcpServerStatus/list`, anonymized), and the runs that
//! change them.

use super::*;
use newen_agent::{ExtensionOp, McpSource, McpStatus, NewServer, Plugin, Skill};

fn fixture(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/0.163.0")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

fn args(run: &ProgramRun) -> Vec<&str> {
    run.args.iter().map(String::as_str).collect()
}

#[test]
fn the_servers_come_from_mcp_list_json_with_how_they_run() {
    let codex = Codex::new();
    assert_eq!(codex.servers_query().unwrap(), ["mcp", "list", "--json"]);
    let servers = codex.servers(&fixture("mcp-list.json"));
    let by = |name: &str| servers.iter().find(|s| s.name == name).unwrap();
    assert_eq!(servers.len(), 4);
    assert_eq!(by("docs").target, "https://docs.example.com/mcp");
    assert_eq!(by("docs").status, McpStatus::NeedsAuth);
    assert_eq!(by("node_repl").target, "/usr/bin/node_repl");
    assert_eq!(by("scratch").target, "npx -y scratch-mcp");
    assert_eq!(by("scratch").status, McpStatus::Disabled);
    // The list does not start them: it only says they are set up.
    assert!(servers.iter().all(|s| !s.checked));
    assert_eq!(by("node_repl").source, McpSource::Other);
    assert_eq!(by("computer").source, McpSource::Plugin);
    assert_eq!(
        codex.server_scopes(),
        [("user".to_owned(), "All my projects".to_owned())]
    );
    // Anything but a list is no servers, never a crash.
    assert!(codex.servers("not json").is_empty());
}

#[test]
fn plugins_and_marketplaces_come_from_codex_plugin() {
    let codex = Codex::new();
    assert_eq!(
        codex.plugins_query().unwrap(),
        ["plugin", "list", "--json", "--available"]
    );
    let (plugins, offers) = codex.plugins(&fixture("plugin-list.json"));
    assert_eq!(plugins.len(), 1);
    assert_eq!(
        (plugins[0].id.as_str(), plugins[0].name.as_str()),
        ("pdf@primary-runtime", "pdf")
    );
    assert!(plugins[0].enabled && plugins[0].scope == "user");
    assert_eq!(offers[0].id, "latex@bundled");
    assert_eq!(offers[0].marketplace, "bundled");
    let markets = codex.marketplaces(&fixture("marketplace-list.json"));
    assert_eq!(markets.len(), 2);
    assert_eq!(markets[0].source, "/opt/codex/plugins/primary-runtime");
    // Without a source of its own, its root.
    assert_eq!(markets[1].source, "/home/user/.codex/.tmp/plugins");
}

#[test]
fn skills_are_asked_without_a_thread_and_read_with_their_state() {
    let codex = Codex::new();
    let lines = codex.skills_query(std::path::Path::new("/work")).unwrap();
    let methods: Vec<String> = lines
        .iter()
        .map(|l| {
            serde_json::from_str::<Value>(l).unwrap()["method"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(methods, ["initialize", "initialized", "skills/list"]);
    let ask: Value = serde_json::from_str(&lines[2]).unwrap();
    assert_eq!(ask["params"]["cwds"][0], "/work");
    let skills = codex
        .skills_answer(fixture("skills-list.json").trim())
        .unwrap();
    let by = |name: &str| skills.iter().find(|s| s.name == name).unwrap();
    assert_eq!(
        (by("review").source.as_str(), by("review").state.as_str()),
        ("user", "on")
    );
    assert!(by("review").locked.is_none());
    assert_eq!(
        (
            by("conventions").source.as_str(),
            by("conventions").state.as_str()
        ),
        ("project", "off")
    );
    assert_eq!(by("conventions").description, "Repo conventions");
    assert!(by("imagegen").locked.is_some() && by("imagegen").source == "system");
    assert_eq!(by("pdf:fill").source, "plugin");
    assert!(by("pdf:fill").locked.is_some());
    assert_eq!(
        by("review").key["path"],
        "/home/user/.agents/skills/review/SKILL.md"
    );
    // Another answer is not the skills'.
    assert!(codex
        .skills_answer(r#"{"id":"newen-ext","result":{}}"#)
        .is_none());
    assert!(codex.skills_answer(r#"{"id":3,"result":{}}"#).is_none());
}

#[test]
fn a_skills_state_is_written_through_the_app_server_that_stays_open_until_it_answers() {
    let codex = Codex::new();
    let skill = Skill {
        name: "review".into(),
        description: String::new(),
        source: "user".into(),
        state: "on".into(),
        locked: None,
        key: json!({ "path": "/home/user/.agents/skills/review/SKILL.md" }),
    };
    let run = codex
        .extension_op(&ExtensionOp::SkillState {
            skill,
            state: "off".into(),
        })
        .unwrap();
    assert_eq!(args(&run), ["app-server"]);
    let lines: Vec<Value> = run
        .stdin
        .as_deref()
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[2]["method"], "skills/config/write");
    assert_eq!(lines[2]["params"]["enabled"], false);
    assert_eq!(
        lines[2]["params"]["path"],
        "/home/user/.agents/skills/review/SKILL.md"
    );
    assert!(run
        .until
        .as_deref()
        .is_some_and(|u| u.contains("newen-skill-write")));
    // Its error is the JSON-RPC one.
    let said = r#"{"id":"newen-skill-write","error":{"code":-32600,"message":"no such skill"}}"#;
    assert_eq!(
        codex.extension_failed(said).as_deref(),
        Some("no such skill")
    );
    assert!(codex
        .extension_failed(r#"{"id":"newen-skill-write","result":{"effectiveEnabled":false}}"#)
        .is_none());
    assert_eq!(codex.extension_words().1[0].0, "on");
}

#[test]
fn servers_are_added_and_removed_with_codex_mcp_and_what_it_cannot_is_declined() {
    let codex = Codex::new();
    let add = |server: NewServer| codex.extension_op(&ExtensionOp::AddServer(server));
    let stdio = add(NewServer {
        name: "pg".into(),
        target: "npx".into(),
        args: vec!["-y".into(), "@x/pg".into()],
        env: vec!["URL=postgres://db".into()],
        scope: Some("user".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        args(&stdio),
        [
            "mcp",
            "add",
            "pg",
            "--env",
            "URL=postgres://db",
            "--",
            "npx",
            "-y",
            "@x/pg"
        ]
    );
    let remote = add(NewServer {
        name: "docs".into(),
        target: "https://docs.example.com/mcp".into(),
        remote: true,
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        args(&remote),
        [
            "mcp",
            "add",
            "docs",
            "--url",
            "https://docs.example.com/mcp"
        ]
    );
    // `mcp add --help` has no header option: the engine says Codex cannot.
    assert!(add(NewServer {
        name: "docs".into(),
        target: "https://docs.example.com/mcp".into(),
        remote: true,
        headers: vec!["X-Key: 1".into()],
        ..Default::default()
    })
    .is_none());
    let remove = codex
        .extension_op(&ExtensionOp::RemoveServer("pg".into()))
        .unwrap();
    assert_eq!(args(&remove), ["mcp", "remove", "pg"]);
    // It exits well when the name is not there.
    assert_eq!(
        codex
            .extension_failed("No MCP server named 'pg' found.\n")
            .as_deref(),
        Some("No MCP server named 'pg' found.")
    );
    assert!(codex
        .extension_failed("Removed global MCP server 'pg'.\n")
        .is_none());
}

#[test]
fn plugins_and_marketplaces_change_with_codex_plugin_and_no_switch_is_offered() {
    let codex = Codex::new();
    let op = |op: ExtensionOp| codex.extension_op(&op);
    let plugin = Plugin {
        id: "pdf@primary-runtime".into(),
        name: "pdf".into(),
        version: None,
        scope: "user".into(),
        enabled: true,
    };
    let run = |op| args_of(codex.extension_op(&op).unwrap());
    assert_eq!(
        run(ExtensionOp::Install {
            id: "latex@bundled".into(),
            scope: Some("project".into())
        }),
        ["plugin", "add", "latex@bundled"]
    );
    assert_eq!(
        run(ExtensionOp::Uninstall(plugin.clone())),
        ["plugin", "remove", "pdf@primary-runtime"]
    );
    assert_eq!(
        run(ExtensionOp::AddMarketplace(" owner/repo ".into())),
        ["plugin", "marketplace", "add", "owner/repo"]
    );
    assert_eq!(
        run(ExtensionOp::UpdateMarketplace("curated".into())),
        ["plugin", "marketplace", "upgrade", "curated"]
    );
    assert_eq!(
        run(ExtensionOp::RemoveMarketplace("curated".into())),
        ["plugin", "marketplace", "remove", "curated"]
    );
    // Codex has no command to turn a plugin off or to update one by itself.
    assert!(op(ExtensionOp::SwitchPlugin {
        plugin,
        enabled: false
    })
    .is_none());
    assert!(op(ExtensionOp::Update("pdf@primary-runtime".into())).is_none());
    let kinds = codex.extension_kinds();
    assert!(!kinds.switch_plugins && !kinds.update_plugins && !kinds.skills_per_folder);
}

fn args_of(run: ProgramRun) -> Vec<String> {
    run.args
}

/// A started adapter with its thread.
fn started() -> Codex {
    let mut codex = Codex::new();
    codex.launch(&SessionSpec {
        cwd: "/work".into(),
        ..Default::default()
    });
    codex.encode(AgentCommand::Start);
    codex.thread = Some("th1".into());
    codex
}

#[test]
fn the_sessions_mcp_state_is_asked_of_the_thread_and_told_with_each_servers_tools() {
    let mut codex = started();
    let ask: Value = serde_json::from_str(&codex.encode(AgentCommand::McpStatus)[0]).unwrap();
    assert_eq!(ask["method"], "mcpServerStatus/list");
    assert_eq!(ask["id"], "newen-mcp-status");
    assert_eq!(ask["params"]["threadId"], "th1");
    let decoded = codex.decode(fixture("mcp-status.json").trim());
    let [AgentEvent::McpServers(servers)] = decoded.events.as_slice() else {
        panic!("{:?}", decoded.events)
    };
    let by = |name: &str| servers.iter().find(|s| s.name == name).unwrap();
    assert_eq!(by("node_repl").status, McpStatus::Connected);
    assert_eq!(by("node_repl").tools.len(), 2);
    assert!(by("node_repl")
        .tools
        .iter()
        .any(|t| t.name == "js_reset" && t.read_only));
    assert_eq!(by("docs").status, McpStatus::NeedsAuth);
    assert!(by("docs").sign_in);
    assert_eq!(by("computer").status, McpStatus::Failed);
    assert_eq!(by("computer").source, McpSource::Plugin);
    assert_eq!(by("computer").plugin.as_deref(), Some("computer"));
    assert_eq!(by("node_repl").source, McpSource::User);
    assert_eq!(by("computer").error.as_deref(), Some("MCP startup failed"));
    // Without a runtime state, its tools say it; the account's connectors are the account's.
    assert_eq!(by("codex_apps").status, McpStatus::Connected);
    assert_eq!(by("codex_apps").source, McpSource::Account);
    assert!(!by("codex_apps").sign_in);
}

#[test]
fn reconnecting_loads_the_config_again_and_asks_the_state_once_it_has() {
    let mut codex = started();
    let sent: Value =
        serde_json::from_str(&codex.encode(AgentCommand::McpReconnect("docs".into()))[0]).unwrap();
    assert_eq!(sent["method"], "config/mcpServer/reload");
    let decoded = codex.decode(&json!({ "id": "newen-mcp-reload", "result": {} }).to_string());
    assert_eq!(decoded.replies.len(), 1);
    let asked: Value = serde_json::from_str(&decoded.replies[0]).unwrap();
    assert_eq!(asked["method"], "mcpServerStatus/list");
    // A sign-in that finished asks it too.
    let done = codex
        .decode(&json!({ "method": "mcpServer/oauthLogin/completed", "params": {} }).to_string());
    assert_eq!(done.replies.len(), 1);
}

#[test]
fn signing_in_to_a_server_gives_its_page_or_why_it_could_not() {
    let mut codex = started();
    let sent: Value =
        serde_json::from_str(&codex.encode(AgentCommand::McpAuthenticate("docs".into()))[0])
            .unwrap();
    assert_eq!(sent["method"], "mcpServer/oauth/login");
    assert_eq!(
        (
            sent["params"]["name"].as_str(),
            sent["params"]["threadId"].as_str()
        ),
        (Some("docs"), Some("th1"))
    );
    let page = codex.decode(&json!({ "id": "newen-mcp-login", "result": { "authorizationUrl": "https://example.com/auth" } }).to_string());
    assert_eq!(
        page.events,
        [AgentEvent::McpSignIn {
            server: "docs".into(),
            url: Some("https://example.com/auth".into()),
            error: None
        }]
    );
    codex.encode(AgentCommand::McpAuthenticate("docs".into()));
    let failed = codex.decode(
        &json!({ "id": "newen-mcp-login", "error": { "code": -1, "message": "no oauth" } })
            .to_string(),
    );
    assert!(matches!(
        failed.events.first(),
        Some(AgentEvent::McpSignIn { error: Some(e), url: None, .. }) if e == "no oauth"
    ));
}

#[test]
fn the_session_reports_its_servers_but_cannot_turn_one_off() {
    let codex = started();
    let AgentEvent::Ready { capabilities, .. } = codex.ready() else {
        unreachable!()
    };
    assert!(capabilities.mcp && !capabilities.mcp_toggle && !capabilities.mcp_clear_auth);
}

#[test]
fn a_skill_picked_from_the_slash_menu_is_a_mention_and_a_path_is_not() {
    assert_eq!(
        extensions::skill_mention("/review the diff"),
        "$review the diff"
    );
    assert_eq!(extensions::skill_mention("/review"), "$review");
    assert_eq!(
        extensions::skill_mention("/pdf:fill form.pdf"),
        "$pdf:fill form.pdf"
    );
    assert_eq!(
        extensions::skill_mention("/etc/hosts is odd"),
        "/etc/hosts is odd"
    );
    assert_eq!(extensions::skill_mention("fix the bug"), "fix the bug");
    assert_eq!(extensions::skill_mention("/"), "/");
}

#[test]
fn global_servers_and_skills_are_set_in_the_handshake_and_the_thread_with_no_secret() {
    use newen_agent::InjectedServer;
    let mut codex = Codex::new();
    codex.launch(&SessionSpec {
        cwd: "/work".into(),
        servers: vec![
            InjectedServer {
                name: "github".into(),
                target: "npx".into(),
                args: vec!["-y".into(), "server-github".into()],
                env: vec![("LOG".into(), "warn".into())],
                env_vars: vec![("GITHUB_TOKEN".into(), "GITHUB_TOKEN".into())],
                ..Default::default()
            },
            InjectedServer {
                name: "linear".into(),
                target: "https://mcp.linear.app/mcp".into(),
                remote: true,
                headers: vec![("X-Team".into(), "core".into())],
                header_vars: vec![(
                    "Authorization".into(),
                    "NEWEN_MCP_LINEAR_H_AUTHORIZATION".into(),
                )],
                ..Default::default()
            },
        ],
        skill_roots: vec!["/cfg/newen/injected/codex/skills".into()],
        ..Default::default()
    });
    codex.encode(AgentCommand::Start);
    let init = codex.decode(r#"{"id":1,"result":{"userAgent":"newen/0.163.0 (Linux)"}}"#);
    let sent: Vec<Value> = init
        .replies
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let methods: Vec<&str> = sent.iter().map(|m| m["method"].as_str().unwrap()).collect();
    assert_eq!(
        methods,
        [
            "initialized",
            "model/list",
            "skills/extraRoots/set",
            "thread/start"
        ]
    );
    assert_eq!(
        sent[2]["params"]["extraRoots"][0],
        "/cfg/newen/injected/codex/skills"
    );
    let servers = &sent[3]["params"]["config"]["mcp_servers"];
    assert_eq!(servers["github"]["command"], "npx");
    assert_eq!(servers["github"]["env_vars"], json!(["GITHUB_TOKEN"]));
    assert_eq!(servers["github"]["env"]["LOG"], "warn");
    assert_eq!(servers["linear"]["url"], "https://mcp.linear.app/mcp");
    assert_eq!(servers["linear"]["http_headers"]["X-Team"], "core");
    assert_eq!(
        servers["linear"]["env_http_headers"]["Authorization"],
        "NEWEN_MCP_LINEAR_H_AUTHORIZATION"
    );
    // The answer to the roots is not an event.
    assert!(codex
        .decode(r#"{"id":"newen-skill-roots","result":{}}"#)
        .events
        .is_empty());
    let extras = codex.session_extras();
    assert!(!extras.sse && extras.env_by_name);
}

/// The person's tool permissions ride in the thread's config: an approval mode per tool, and
/// the blocked ones among the server's disabled tools, beside a global server's own entry.
#[test]
fn tool_permissions_are_the_threads_approval_modes_and_disabled_tools() {
    use newen_agent::{InjectedServer, ToolPolicy, ToolRule};
    let mut codex = Codex::new();
    assert_eq!(codex.tool_policies(), ToolPolicy::ALL.to_vec());
    let rule = |server: &str, tool: &str, policy| ToolRule {
        server: server.into(),
        tool: tool.into(),
        policy,
    };
    codex.launch(&SessionSpec {
        cwd: "/work".into(),
        servers: vec![InjectedServer {
            name: "linear".into(),
            target: "https://mcp.linear.app/mcp".into(),
            remote: true,
            ..Default::default()
        }],
        tool_rules: vec![
            rule("github", "list_issues", ToolPolicy::Allow),
            rule("github", "create_issue", ToolPolicy::Ask),
            rule("github", "delete_repo", ToolPolicy::Block),
            rule("linear", "delete_issue", ToolPolicy::Block),
        ],
        ..Default::default()
    });
    codex.encode(AgentCommand::Start);
    let init = codex.decode(r#"{"id":1,"result":{"userAgent":"newen/0.163.0 (Linux)"}}"#);
    let thread: Value = (init.replies.iter())
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .find(|m| m["method"] == "thread/start")
        .unwrap();
    let servers = &thread["params"]["config"]["mcp_servers"];
    assert_eq!(
        servers["github"],
        json!({
            "tools": {
                "list_issues": { "approval_mode": "approve" },
                "create_issue": { "approval_mode": "prompt" },
            },
            "disabled_tools": ["delete_repo"],
        })
    );
    assert_eq!(servers["linear"]["url"], "https://mcp.linear.app/mcp");
    assert_eq!(servers["linear"]["disabled_tools"], json!(["delete_issue"]));
}

/// Codex has no OAuth client of the person's own: such a server is not added here.
#[test]
fn a_server_with_an_oauth_client_is_not_added_by_codex() {
    let codex = Codex::new();
    assert!(!codex.server_oauth_client());
    assert!(codex
        .extension_op(&ExtensionOp::AddServer(NewServer {
            name: "notion".into(),
            target: "https://mcp.notion.com/mcp".into(),
            remote: true,
            client_id: Some("abc".into()),
            ..Default::default()
        }))
        .is_none());
}
