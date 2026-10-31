//! quill's adapter for the Claude Code CLI: stream-json in and out, plus its control
//! protocol (docs/engine.md §2–3). Shapes are the ones captured from CLI 2.1.310
//! (`fixtures/2.1.310/`, docs/engine.md §11.4).

use quill_agent::{
    AgentAdapter, AgentCapabilities, AgentCommand, AgentEvent, AgentId, AgentSlashCommand,
    AuthState, Choices, CommandRun, ContextCategory, ContextKind, ContextUsage, Decision, Decoded,
    ExtensionOp, FileChange, LaunchPlan, Marketplace, McpServer, McpSource, McpStatus, McpTool,
    ModeWords, ModelInfo, PlanLimits, Plugin, PluginOffer, ProgramRun, SessionSpec, Skill,
    Suggestion, TaskUsage, ToolContent, Usage, UsageWindow, WebSearchRun, WebSource, BROWSER_GUIDE,
    BROWSER_TOOLS, HOST_TOOLS,
};
use serde_json::{json, Value};
use std::collections::HashMap;

/// The PostToolUse hook quill registers: each call reports a tool's end with its prompt.
const POST_TOOL_HOOK: &str = "quill_post_tool";

/// Permission modes the CLI accepts at launch (`auto` stays out of the MVP, spec.md §6).
const MODES: [&str; 3] = ["default", "acceptEdits", "plan"];

/// Message types that carry nothing quill shows yet; not `Unknown`, not events.
const QUIET: [&str; 2] = ["command_lifecycle", "auth_status"];

#[derive(Default)]
pub struct Claude {
    next_request: u64,
    initialize: Option<String>,
    /// The answer to `initialize`, kept to tell the commands again when `system/init` lists them.
    ready: Option<AgentEvent>,
    /// The `get_context_usage` request waiting for its answer.
    context_request: Option<String>,
    /// `mcp_status` requests waiting for their answers.
    mcp_requests: Vec<String>,
    /// The `get_skills_dialog` request waiting for its answer.
    skills_request: Option<String>,
    /// `mcp_authenticate` requests waiting for their answers, with their server.
    sign_ins: HashMap<String, String>,
    interrupted: bool,
    /// The turn's answer was the CLI's `authentication_failed` (cli-auth.md §4): its result
    /// ends the turn with `AuthRequired`.
    auth_failed: bool,
    /// `can_use_tool` waiting for a decision: its tool call and input, echoed back on allow.
    pending: HashMap<String, (Value, Value)>,
    /// Each tool call's name and input, to describe its result.
    calls: HashMap<String, (String, Value)>,
    /// The context the last answer was given, in tokens: what the conversation fills now.
    context: u64,
    /// The running turn's tokens: its finished answers, the answer being written, and each
    /// helper task's latest total.
    turn_tokens: u64,
    live_tokens: u64,
    task_tokens: HashMap<String, u64>,
    /// The CLI's `/effort` takes `ultracode`: it is offered as the slider's last stop.
    ultracode: bool,
    /// quill's tool calls waiting for the engine: the control request's id and the JSON-RPC id
    /// the answer goes back under.
    host_calls: HashMap<String, Value>,
    /// The `remote_control` request waiting for its answer, and whether it turns it on.
    remote_request: Option<(String, bool)>,
    /// Remote Control is on, as the CLI last said.
    remote: bool,
    /// quill's own prompts, until the CLI echoes them back (`--replay-user-messages`).
    prompts: std::collections::HashSet<String>,
}

impl Claude {
    pub fn new() -> Self {
        Self::default()
    }

    fn request(&mut self, request: Value) -> String {
        self.next_request += 1;
        let id = format!("q_{}", self.next_request);
        json!({ "type": "control_request", "request_id": id, "request": request }).to_string()
    }

    /// The running turn's tokens as Desktop counts them ("30.7k tokens"); which tokens it adds
    /// up is inferred from what a turn reports.
    fn tokens(&self) -> AgentEvent {
        AgentEvent::Tokens {
            tokens: self.turn_tokens + self.live_tokens + self.task_tokens.values().sum::<u64>(),
        }
    }

    fn last_request_id(&self) -> String {
        format!("q_{}", self.next_request)
    }

    fn decide(&mut self, id: String, decision: Decision) -> Vec<String> {
        let (call, input) = self.pending.remove(&id).unwrap_or_default();
        let response = match decision {
            Decision::Allow => {
                json!({ "behavior": "allow", "updatedInput": input, "toolUseID": call })
            }
            Decision::AllowAlways(suggestions) => json!({
                "behavior": "allow", "updatedInput": input, "toolUseID": call,
                "updatedPermissions": suggestions.into_iter().map(suggestion_value).collect::<Vec<_>>(),
            }),
            Decision::Deny { message } => {
                json!({ "behavior": "deny", "message": message, "toolUseID": call })
            }
            Decision::Answer(answers) => {
                let mut input = input;
                input["answers"] = answers;
                json!({ "behavior": "allow", "updatedInput": input, "toolUseID": call })
            }
        };
        vec![success(&Value::String(id), response)]
    }

    fn control_request(&mut self, id: Value, request: &Value, decoded: &mut Decoded) {
        let id_text = id.as_str().unwrap_or_default().to_owned();
        match request["subtype"].as_str() {
            // quill's own tools are quill's to run: they never ask the person (an
            // `own_initiative` call only proposes, and its card is where the person answers).
            Some("can_use_tool")
                if [SERVER, BROWSER_SERVER].iter().any(|server| {
                    text(&request["tool_name"]).starts_with(&format!("mcp__{server}__"))
                }) =>
            {
                decoded.replies.push(success(
                    &id,
                    json!({ "behavior": "allow", "updatedInput": request["input"],
                        "toolUseID": request["tool_use_id"] }),
                ));
            }
            Some("mcp_message")
                if request["server_name"] == SERVER || request["server_name"] == BROWSER_SERVER =>
            {
                let tools: &[(&str, &str, &str)] = if request["server_name"] == SERVER {
                    &HOST_TOOLS
                } else {
                    &BROWSER_TOOLS
                };
                self.host_message(
                    id,
                    text(&request["server_name"]),
                    tools,
                    &request["message"],
                    decoded,
                )
            }
            Some("can_use_tool") => {
                let tool = text(&request["tool_name"]);
                let input = request["input"].clone();
                let call = request["tool_use_id"].clone();
                self.pending
                    .insert(id_text.clone(), (call.clone(), input.clone()));
                decoded.events.push(AgentEvent::PermissionRequest {
                    id: id_text,
                    call: call.as_str().map(str::to_owned),
                    interactive: request["requires_user_interaction"] == true
                        || tool == "AskUserQuestion",
                    title: request["title"].as_str().map(str::to_owned),
                    tool,
                    input,
                    suggestions: suggestions(&request["permission_suggestions"]),
                });
            }
            Some("hook_callback") => {
                decoded.events.push(AgentEvent::ToolDone {
                    call: text(&request["input"]["tool_use_id"]),
                    prompt: request["input"]["prompt_id"].as_str().map(str::to_owned),
                });
                decoded.replies.push(success(&id, json!({})));
            }
            // Elicitations, dialogs and token refreshes: quill offers none (engine.md §3.3).
            _ => {
                decoded.events.push(AgentEvent::Unknown(request.clone()));
                decoded.replies.push(
                    json!({ "type": "control_response", "response": {
                        "subtype": "error", "request_id": id, "error": "not supported by quill" } })
                    .to_string(),
                );
            }
        }
    }

    /// One JSON-RPC message for quill's in-process MCP server: the handshake and the tool list
    /// are answered here, a tool call goes to the engine (verified on CLI 2.1.310,
    /// [F:sessions]; the protocol is the CLI's `mcp_message`, as `initialize.sdkMcpServers`
    /// names the server).
    fn host_message(
        &mut self,
        id: Value,
        server: String,
        tools: &[(&str, &str, &str)],
        message: &Value,
        decoded: &mut Decoded,
    ) {
        let rpc = message["id"].clone();
        let result = match message["method"].as_str() {
            Some("initialize") => json!({
                "protocolVersion": message["params"]["protocolVersion"],
                "capabilities": { "tools": {} },
                "serverInfo": { "name": server, "version": env!("CARGO_PKG_VERSION") },
            }),
            Some("tools/list") => {
                let tools: Vec<Value> = tools
                    .iter()
                    .map(|(name, description, schema)| {
                        // Loaded with the session rather than found through tool search, as
                        // Desktop marks its own the same way.
                        json!({
                            "name": name, "description": description,
                            "inputSchema": serde_json::from_str::<Value>(schema).unwrap_or_default(),
                            "_meta": { "anthropic/alwaysLoad": true },
                        })
                    })
                    .collect();
                json!({ "tools": tools })
            }
            Some("tools/call") => {
                let id_text = text(&id);
                self.host_calls.insert(id_text.clone(), rpc);
                decoded.events.push(AgentEvent::HostTool {
                    id: id_text,
                    name: text(&message["params"]["name"]),
                    input: message["params"]["arguments"].clone(),
                });
                return;
            }
            // Notifications want only an acknowledgement.
            _ => json!({}),
        };
        decoded.replies.push(success(
            &id,
            json!({ "mcp_response": { "jsonrpc": "2.0", "id": rpc, "result": result } }),
        ));
    }

    fn tool_result(&mut self, block: &Value, structured: &Value) -> AgentEvent {
        let call = text(&block["tool_use_id"]);
        let output = match &block["content"] {
            Value::String(s) => s.clone(),
            Value::Array(parts) => parts
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            other => other.to_string(),
        };
        let (name, input) = self.calls.get(&call).cloned().unwrap_or_default();
        let files = file_changes(structured);
        let command = (name == "Bash").then(|| CommandRun {
            command: text(&input["command"]),
            stdout: text(&structured["stdout"]),
            stderr: text(&structured["stderr"]),
            interrupted: structured["interrupted"] == true,
        });
        let web = (name == "WebSearch").then(|| web_run(&input, structured, &output));
        AgentEvent::ToolResult {
            call,
            output,
            is_error: block["is_error"] == true,
            files,
            command,
            web,
        }
    }
}

impl AgentAdapter for Claude {
    fn id(&self) -> AgentId {
        "claude"
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    /// Desktop waives it in bypass and auto mode.
    fn mode_waives_prompts(&self, mode: &str) -> bool {
        matches!(mode, "bypassPermissions" | "auto")
    }

    /// Claude Desktop's own.
    fn launch_file(&self) -> Option<&'static str> {
        Some(LAUNCH_FILE)
    }

    fn tested(&self) -> &'static [&'static str] {
        // The versions the fixtures were captured from.
        &["2.1.310"]
    }

    fn session_extras(&self) -> quill_agent::SessionExtras {
        quill_agent::SessionExtras {
            servers: true,
            sse: true,
            env_by_name: false,
            skills: true,
        }
    }

    fn launch(&self, spec: &SessionSpec) -> LaunchPlan {
        let mut args: Vec<String> = [
            "--output-format",
            "stream-json",
            "--verbose",
            "--input-format",
            "stream-json",
            "--permission-prompt-tool",
            "stdio",
            "--include-partial-messages",
            "--thinking-display",
            "summarized",
            "--setting-sources=user,project,local",
            // Without it a message sent from the phone over Remote Control reaches quill as
            // its id alone (2.1.316 [F:remote]).
            "--replay-user-messages",
        ]
        .map(String::from)
        .into();
        args.extend([
            "--permission-mode".into(),
            spec.mode.clone().unwrap_or_else(|| "default".into()),
        ]);
        if let Some(effort) = &spec.effort {
            // shortcut: Ultracode at launch starts at its effort, extra high, without the
            // workflows it adds; choosing it again in the session turns them on.
            let effort = if effort == ULTRACODE { "xhigh" } else { effort };
            args.extend(["--effort".into(), effort.to_owned()]);
        }
        // Desktop's "Allow bypass permissions mode": the session may be
        // switched to it later, which the CLI allows only when launched with this flag.
        if spec.allow_unsafe {
            args.push("--allow-dangerously-skip-permissions".into());
        }
        if let Some(model) = &spec.model {
            args.extend(["--model".into(), model.clone()]);
        }
        // The page's own options, as Claude Code settings for this session (`--settings`).
        if let Some(settings) = option_settings(spec) {
            args.extend(["--settings".into(), settings.to_string()]);
        }
        if let Some(id) = &spec.resume {
            args.push(format!("--resume={id}"));
            if spec.fork {
                args.push("--fork-session".into());
            }
            // Desktop's "Fork from here" resumes the same way; the CLI takes it in stream-json
            // mode too (tried on 2.1.310).
            if let Some(at) = &spec.resume_at {
                args.push(format!("--resume-session-at={at}"));
            }
        }
        args.extend(inject::args(spec));
        // Same switches Desktop sets; quill does not claim to be Desktop (spec.md D2).
        let env = [
            ("CLAUDE_CODE_ENABLE_ASK_USER_QUESTION_TOOL", "true"),
            ("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING", "true"),
            ("CLAUDE_CODE_EMIT_TOOL_USE_SUMMARIES", "false"),
            ("CLAUDE_CODE_DISABLE_TERMINAL_TITLE", "1"),
            ("MCP_CONNECTION_NONBLOCKING", "true"),
            // The task list (TaskCreate, TaskUpdate…) Desktop's sessions show;
            // 2.1.310 offers those tools only with both set (tools/capture `todos`).
            ("CLAUDE_CODE_ENABLE_TASKS", "true"),
            ("CLAUDE_CODE_ENABLE_TODO_TOOLS", "true"),
        ];
        LaunchPlan {
            program: "claude".into(),
            args,
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            env_remove: vec!["NODE_OPTIONS".into(), "CLAUDE_CODE_ENTRYPOINT".into()],
            cwd: spec.cwd.clone(),
        }
    }

    // Desktop's words for each mode, as the mode menu shows them;
    // Auto's says who decides without naming the agent.
    fn modes(&self) -> Vec<ModeWords> {
        use quill_agent::ModeTone::*;
        let w = |id: &str, label: &str, about: &str, tone, listed| ModeWords {
            id: id.into(),
            label: label.into(),
            about: about.into(),
            tone,
            listed,
        };
        vec![
            w(
                "default",
                "Manual",
                "Always ask before making changes",
                Ask,
                true,
            ),
            w(
                "acceptEdits",
                "Accept edits",
                "Automatically accept all file edits",
                Edits,
                true,
            ),
            w(
                "plan",
                "Plan",
                "Create a plan before making changes",
                Plan,
                true,
            ),
            w(
                "auto",
                "Auto",
                "The agent handles permission decisions",
                Auto,
                false,
            ),
            w(
                "bypassPermissions",
                "Bypass permissions",
                "Accepts all permissions",
                Unsafe,
                false,
            ),
        ]
    }

    fn default_mode(&self) -> &'static str {
        "default"
    }

    // As Desktop does, through the CLI's own commands (`plugin list --json --available`,
    // `plugin install … --scope`), never
    // editing its files.
    // Sign-in is the CLI's own `claude auth …` (Claude Code 2.1.312, observed): quill drives
    // it and never holds a token. Its login prints the page address and then waits on stdin
    // for `CODE#STATE`, the code the manual page shows, while a local callback also listens.
    fn sign_in_methods(&self) -> Choices {
        [
            ("claudeai", "Claude subscription"),
            ("console", "Anthropic Console"),
        ]
        .map(|(key, label)| (key.to_owned(), label.to_owned()))
        .into()
    }

    fn auth_query(&self) -> Option<Vec<String>> {
        Some(["auth", "status", "--json"].map(String::from).into())
    }

    fn auth_state(&self, stdout: &str, exit: i32) -> AuthState {
        // `status` exits 1 when signed out, and prints the same JSON either way.
        let Ok(status) = serde_json::from_str::<Value>(stdout) else {
            return AuthState::Unknown(format!("claude auth status exited {exit}"));
        };
        let word = |key: &str| status[key].as_str().map(str::to_owned);
        match status["loggedIn"].as_bool() {
            Some(true) => AuthState::SignedIn {
                method: word("authMethod").unwrap_or_default(),
                email: word("email"),
                org: word("orgName"),
                plan: word("subscriptionType"),
            },
            Some(false) => AuthState::SignedOut,
            None => AuthState::Unknown("claude auth status gave no loggedIn".into()),
        }
    }

    fn login_run(&self, method: &str) -> Option<ProgramRun> {
        let flag = match method {
            "claudeai" => "--claudeai",
            "console" => "--console",
            _ => return None,
        };
        Some(ProgramRun {
            args: ["auth", "login", flag].map(String::from).into(),
            stdin: None,
            timeout: 600,
            until: None,
        })
    }

    /// The address is wrapped in an OSC 8 hyperlink even without a terminal
    /// (`ESC]8;;URL BEL URL ESC]8;; BEL`); only the address is taken, and only a secure one.
    fn login_url(&self, line: &str) -> Option<String> {
        let from = line.find("https://")?;
        let rest = &line[from..];
        let end = rest.find(['\x07', '\x1b', ' ']).unwrap_or(rest.len());
        Some(rest[..end].trim().to_owned())
    }

    fn logout_run(&self) -> Option<ProgramRun> {
        Some(ProgramRun {
            args: ["auth", "logout"].map(String::from).into(),
            stdin: None,
            timeout: 60,
            until: None,
        })
    }

    fn plugins_query(&self) -> Option<Vec<String>> {
        Some(
            ["plugin", "list", "--json", "--available"]
                .map(String::from)
                .into(),
        )
    }

    fn plugins(&self, stdout: &str) -> (Vec<Plugin>, Vec<PluginOffer>) {
        let list: Value = serde_json::from_str(stdout).unwrap_or_default();
        // `{installed, available}` with --available, a plain list without.
        let installed = list["installed"].as_array().or(list.as_array());
        let plugins = (installed.into_iter().flatten())
            .map(|row| {
                let id = text(&row["id"]);
                Plugin {
                    name: id
                        .split_once('@')
                        .map_or(id.as_str(), |(name, _)| name)
                        .to_owned(),
                    version: row["version"].as_str().map(str::to_owned),
                    scope: text(&row["scope"]),
                    enabled: row["enabled"] == true,
                    id,
                }
            })
            .collect();
        let offers = (list["available"].as_array().into_iter().flatten())
            .map(|row| PluginOffer {
                id: text(&row["pluginId"]),
                name: text(&row["name"]),
                description: text(&row["description"]),
                marketplace: text(&row["marketplaceName"]),
                version: row["version"].as_str().map(str::to_owned),
                installs: row["installCount"].as_u64(),
            })
            .collect();
        (plugins, offers)
    }

    fn marketplaces_query(&self) -> Option<Vec<String>> {
        Some(
            ["plugin", "marketplace", "list", "--json"]
                .map(String::from)
                .into(),
        )
    }

    fn marketplaces(&self, stdout: &str) -> Vec<Marketplace> {
        let list: Value = serde_json::from_str(stdout).unwrap_or_default();
        (list.as_array().into_iter().flatten())
            .map(|row| Marketplace {
                name: text(&row["name"]),
                source: ["repo", "url", "path"]
                    .iter()
                    .find_map(|key| row[*key].as_str())
                    .unwrap_or_default()
                    .to_owned(),
            })
            .collect()
    }

    // Desktop's install menu, and the CLI's own /skills states.
    // `claude mcp list` prints no JSON (2.1.311): a line a server, "name: target - ✔ Connected",
    // after checking each one's health.
    fn servers_query(&self) -> Option<Vec<String>> {
        Some(vec!["mcp".into(), "list".into()])
    }

    // svgl.app's "Claude AI" icon, recolored to currentColor.
    fn icon(&self) -> Option<&'static str> {
        Some(include_str!("../assets/claude.svg"))
    }

    fn servers(&self, stdout: &str) -> Vec<quill_agent::ConfiguredServer> {
        stdout.lines().filter_map(configured_server).collect()
    }

    fn server_scopes(&self) -> quill_agent::Choices {
        [
            ("local", "This project, for me"),
            ("project", "This project, shared"),
            ("user", "All my projects"),
        ]
        .map(|(a, b)| (a.to_owned(), b.to_owned()))
        .into()
    }

    // Desktop's local Claude Code options
    // that the CLI itself takes: its output style and its sandbox.
    fn options(&self) -> Vec<quill_agent::AgentOption> {
        let option =
            |key: &str, label: &str, description: &str, choices: &[(&str, &str)], default: &str| {
                quill_agent::AgentOption {
                    key: key.into(),
                    label: label.into(),
                    description: description.into(),
                    choices: choices
                        .iter()
                        .map(|(a, b)| ((*a).into(), (*b).into()))
                        .collect(),
                    default: default.into(),
                }
            };
        vec![
            option(
                OUTPUT_STYLE,
                "Output style",
                "How Claude structures its responses. Applies to new sessions.",
                &[("", "Default"), ("Explanatory", "Explanatory"), ("Learning", "Learning")],
                "",
            ),
            option(
                SANDBOX,
                "Local sandbox",
                "Runs commands from Claude Code in an isolated sandbox. Applies to new sessions.",
                &[],
                "false",
            ),
            option(
                STRICT_SANDBOX,
                "Strict sandbox mode",
                "Blocks commands that can't run inside the sandbox instead of asking to run them outside it. Applies to new sessions.",
                &[],
                "false",
            ),
        ]
    }

    fn extension_words(&self) -> (quill_agent::Choices, quill_agent::Choices) {
        let pairs = |list: &[(&str, &str)]| {
            (list.iter())
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect()
        };
        (
            pairs(&[
                ("user", "Install for me"),
                ("project", "Install for project (shared)"),
                ("local", "Install for project (personal)"),
            ]),
            pairs(&[
                ("on", "On"),
                ("name-only", "Name only"),
                ("user-invocable-only", "User only"),
                ("off", "Off"),
            ]),
        )
    }

    fn extension_op(&self, op: &ExtensionOp) -> Option<ProgramRun> {
        let run = |args: &[&str], timeout| ProgramRun {
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            stdin: None,
            timeout,
            until: None,
        };
        Some(match op {
            ExtensionOp::SwitchPlugin { plugin, enabled } => {
                let verb = if *enabled { "enable" } else { "disable" };
                run(
                    &[
                        "plugin",
                        verb,
                        &plugin.id,
                        "--scope",
                        &plugin.scope,
                        "--json",
                    ],
                    30,
                )
            }
            // The person's own takes no scope, as Desktop's "Install for me".
            ExtensionOp::Install { id, scope } => match scope.as_deref() {
                None | Some("user") => run(&["plugin", "install", id, "--json"], 120),
                Some(scope) => run(&["plugin", "install", id, "--scope", scope, "--json"], 120),
            },
            ExtensionOp::Uninstall(plugin) => run(
                &[
                    "plugin",
                    "uninstall",
                    &plugin.id,
                    "--scope",
                    &plugin.scope,
                    "--json",
                ],
                30,
            ),
            ExtensionOp::Update(id) => run(&["plugin", "update", id, "--json"], 120),
            ExtensionOp::AddMarketplace(source) => run(
                &[
                    "plugin",
                    "marketplace",
                    "add",
                    &marketplace_source(source),
                    "--json",
                ],
                180,
            ),
            ExtensionOp::UpdateMarketplace(name) => {
                run(&["plugin", "marketplace", "update", name, "--json"], 180)
            }
            ExtensionOp::RemoveMarketplace(name) => {
                run(&["plugin", "marketplace", "remove", name, "--json"], 60)
            }
            // `claude mcp add [--transport http] [-s scope] [-e K=V]… [-H "N: v"]… name
            // target [-- args…]` (2.1.311 `mcp add --help`): options before the name.
            ExtensionOp::AddServer(server) => {
                let mut args: Vec<String> = vec!["mcp".into(), "add".into()];
                if server.remote {
                    args.extend(["--transport".into(), "http".into()]);
                }
                if let Some(scope) = &server.scope {
                    args.extend(["--scope".into(), scope.clone()]);
                }
                for pair in &server.env {
                    args.extend(["-e".into(), pair.clone()]);
                }
                for header in server.headers.iter().filter(|_| server.remote) {
                    args.extend(["-H".into(), header.clone()]);
                }
                args.extend([server.name.clone(), server.target.clone()]);
                if !server.args.is_empty() {
                    args.push("--".into());
                    args.extend(server.args.iter().cloned());
                }
                ProgramRun {
                    args,
                    stdin: None,
                    timeout: 30,
                    until: None,
                }
            }
            ExtensionOp::RemoveServer(name) => run(&["mcp", "remove", name], 30),
            // The CLI writes it into the folder's local settings (`skillOverrides`).
            ExtensionOp::SkillState { skill, state } => {
                let mut edit = skill.key.clone();
                edit["state"] = json!(state);
                ProgramRun {
                    stdin: Some(edit.to_string()),
                    ..run(&["edit-skill-overrides", "--json"], 30)
                }
            }
        })
    }

    // Their --json result is the last line: `{outcome: ok|failed|…, message}` for plugins,
    // `{ok, error}` for a skill's state.
    fn extension_failed(&self, stdout: &str) -> Option<String> {
        let last = stdout.lines().rev().find(|l| !l.trim().is_empty())?;
        let result: Value = serde_json::from_str(last).ok()?;
        let failed =
            (result["outcome"].as_str()).is_some_and(|o| o != "ok") || result["ok"] == false;
        failed.then(|| {
            (result["message"].as_str())
                .or(result["error"].as_str())
                .unwrap_or("it failed")
                .to_owned()
        })
    }

    fn encode(&mut self, command: AgentCommand) -> Vec<String> {
        match command {
            AgentCommand::Start => {
                let line = self.request(json!({
                    "subtype": "initialize",
                    "hooks": { "PostToolUse": [{ "hookCallbackIds": [POST_TOOL_HOOK] }] },
                    "sdkMcpServers": [SERVER, BROWSER_SERVER],
                    "appendSystemPrompt": format!("{SESSIONS_GUIDE}\n\n{BROWSER_GUIDE}\n\n{LAUNCH_GUIDE}"),
                }));
                self.initialize = Some(self.last_request_id());
                vec![line]
            }
            AgentCommand::Prompt {
                request,
                text,
                steer,
                images,
            } => {
                // The images come first, then the text, as Desktop sends them.
                let mut content: Vec<Value> = images
                    .iter()
                    .map(|image| {
                        use base64::Engine;
                        json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": image.media_type,
                                "data": base64::engine::general_purpose::STANDARD.encode(&image.data),
                            },
                        })
                    })
                    .collect();
                content.push(json!({ "type": "text", "text": text }));
                self.prompts.insert(request.clone());
                let mut msg = json!({
                    "type": "user", "uuid": request, "session_id": "", "parent_tool_use_id": null,
                    "message": { "role": "user", "content": content },
                });
                // The CLI's word for a message taken into the running turn.
                if steer {
                    msg["priority"] = json!("next");
                }
                vec![msg.to_string()]
            }
            AgentCommand::Interrupt => {
                self.interrupted = true;
                vec![self.request(json!({ "subtype": "interrupt" }))]
            }
            AgentCommand::Decide { id, decision } => self.decide(id, decision),
            AgentCommand::SetMode(mode) => {
                vec![self.request(json!({ "subtype": "set_permission_mode", "mode": mode }))]
            }
            AgentCommand::SetModel(model) => {
                vec![self.request(json!({ "subtype": "set_model", "model": model }))]
            }
            // As Desktop changes it; verified on CLI 2.1.310, which answers
            // success even to a level it does not know, so only the model's levels are sent.
            // Ultracode is extra high effort with dynamic workflows on, as Desktop applies it
            // (`{effortLevel: "xhigh", ultracode: true}`);
            // any other level turns it off again.
            AgentCommand::SetEffort(effort) => {
                let settings = match effort.as_str() {
                    ULTRACODE => json!({ "effortLevel": "xhigh", "ultracode": true }),
                    _ if self.ultracode => json!({ "effortLevel": effort, "ultracode": false }),
                    _ => json!({ "effortLevel": effort }),
                };
                vec![self.request(json!({
                    "subtype": "apply_flag_settings", "settings": settings,
                }))]
            }
            AgentCommand::Rewind { request, dry_run } => vec![self.request(json!({
                "subtype": "rewind_files", "user_message_id": request, "dry_run": dry_run,
            }))],
            // What Desktop's context breakdown asks for.
            AgentCommand::ContextUsage => {
                let line =
                    self.request(json!({ "subtype": "get_context_usage", "detail": "full" }));
                self.context_request = Some(self.last_request_id());
                vec![line]
            }
            // Desktop's session MCP panel polls these;
            // the shapes are the CLI's own (fixtures/2.1.311/README.md).
            AgentCommand::McpStatus => {
                let line = self.request(json!({ "subtype": "mcp_status" }));
                self.mcp_requests.push(self.last_request_id());
                vec![line]
            }
            AgentCommand::McpReconnect(name) => vec![self.request(json!({
                "subtype": "mcp_reconnect", "serverName": name,
            }))],
            // Desktop's Stop on a background task.
            AgentCommand::StopTask(id) => vec![self.request(json!({
                "subtype": "stop_task", "task_id": id,
            }))],
            AgentCommand::McpToggle { name, enabled } => vec![self.request(json!({
                "subtype": "mcp_toggle", "serverName": name, "enabled": enabled,
            }))],
            // Without a redirect address the CLI listens on localhost for the page's answer
            // and connects the server itself; it never opens the browser (2.1.311).
            AgentCommand::McpAuthenticate(name) => {
                let line = self.request(json!({
                    "subtype": "mcp_authenticate", "serverName": name,
                }));
                self.sign_ins.insert(self.last_request_id(), name);
                vec![line]
            }
            AgentCommand::McpClearAuth(name) => vec![self.request(json!({
                "subtype": "mcp_clear_auth", "serverName": name,
            }))],
            // The rows of the CLI's /skills menu, which it offers to hosts that draw it.
            AgentCommand::Skills => {
                let line = self.request(json!({ "subtype": "get_skills_dialog" }));
                self.skills_request = Some(self.last_request_id());
                vec![line]
            }
            // What Desktop sends its idle sessions when plugins change.
            AgentCommand::ReloadPlugins => {
                vec![self.request(json!({ "subtype": "reload_plugins" }))]
            }
            AgentCommand::ReloadSkills => vec![self.request(json!({ "subtype": "reload_skills" }))],
            // Verified on 2.1.316 [F:remote]: on answers `session_url`, off an empty success.
            AgentCommand::RemotePair => vec![],
            AgentCommand::Remote { on, name } => {
                let mut request = json!({ "subtype": "remote_control", "enabled": on });
                if let Some(name) = name.filter(|_| on) {
                    request["name"] = json!(name);
                }
                let line = self.request(request);
                self.remote_request = Some((self.last_request_id(), on));
                vec![line]
            }
            AgentCommand::HostToolResult {
                id,
                content,
                is_error,
            } => {
                let rpc = self.host_calls.remove(&id).unwrap_or_default();
                // MCP's content blocks; the CLI hands a picture to the model as one
                // (verified on 2.1.310 with a screenshot's text read back).
                let content: Vec<Value> = content
                    .into_iter()
                    .map(|block| match block {
                        ToolContent::Text(text) => json!({ "type": "text", "text": text }),
                        ToolContent::Image { mime, data } => {
                            json!({ "type": "image", "data": data, "mimeType": mime })
                        }
                    })
                    .collect();
                vec![success(
                    &Value::String(id),
                    json!({ "mcp_response": { "jsonrpc": "2.0", "id": rpc, "result": {
                        "content": content, "isError": is_error,
                    } } }),
                )]
            }
        }
    }

    fn decode(&mut self, line: &str) -> Decoded {
        let mut decoded = Decoded::default();
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            decoded
                .events
                .push(AgentEvent::Unknown(Value::String(line.to_owned())));
            return decoded;
        };
        let parent = msg["parent_tool_use_id"].as_str().map(str::to_owned);
        match (msg["type"].as_str().unwrap_or(""), msg["subtype"].as_str()) {
            ("control_response", _) => {
                let response = &msg["response"];
                let id = text(&response["request_id"]);
                let ok = response["subtype"] == "success";
                let body = response["response"].clone();
                if ok && self.initialize.as_deref() == Some(id.as_str()) {
                    self.ultracode = offers_ultracode(&body);
                    self.ready = Some(ready(&body, self.ultracode));
                    decoded.events.extend(self.ready.clone());
                }
                if let Some((_, on)) = self.remote_request.take_if(|(request, _)| *request == id) {
                    if ok {
                        self.remote = on;
                    }
                    decoded.events.push(AgentEvent::Remote {
                        on: self.remote,
                        url: body["session_url"].as_str().map(str::to_owned),
                        error: (!ok).then(|| text(&response["error"])),
                    });
                    return decoded;
                }
                if let Some(server) = self.sign_ins.remove(&id) {
                    let url = (body["authUrl"].as_str())
                        .filter(|_| body["requiresUserAction"] != false)
                        .map(str::to_owned);
                    let error = (!ok).then(|| {
                        let said = response["error"].as_str().unwrap_or("could not sign in");
                        said.to_owned()
                    });
                    decoded
                        .events
                        .push(AgentEvent::McpSignIn { server, url, error });
                    return decoded;
                }
                if self.skills_request.as_deref() == Some(id.as_str()) {
                    self.skills_request = None;
                    if ok {
                        decoded.events.push(AgentEvent::Skills(skills(&body)));
                    }
                    return decoded;
                }
                if let Some(at) = self.mcp_requests.iter().position(|r| *r == id) {
                    self.mcp_requests.remove(at);
                    if ok {
                        decoded
                            .events
                            .push(AgentEvent::McpServers(mcp_servers(&body)));
                    }
                    return decoded;
                }
                if self.context_request.as_deref() == Some(id.as_str()) {
                    self.context_request = None;
                    if ok {
                        decoded
                            .events
                            .push(AgentEvent::ContextUsage(context_usage(&body)));
                    }
                    return decoded;
                }
                decoded.events.push(AgentEvent::Reply { id, ok, body });
            }
            ("control_request", _) => {
                self.control_request(msg["request_id"].clone(), &msg["request"], &mut decoded)
            }
            ("control_cancel_request", _) => {
                // The CLI no longer waits for this decision: another client answered it (a
                // permission approved on the phone, 2.1.316 [F:remote]) or the turn ended.
                let id = text(&msg["request_id"]);
                if self.pending.remove(&id).is_some() {
                    decoded.events.push(AgentEvent::Withdrawn { id });
                } else {
                    decoded.events.push(AgentEvent::Unknown(msg));
                }
            }
            // Verified on 2.1.316 [F:remote]: `ready`, then `connected`, once it is turned on.
            ("system", Some("bridge_state"))
                if msg["state"] == "ready" || msg["state"] == "connected" =>
            {
                // Connected without being asked: a resumed session reconnects by itself, as
                // the CLI's docs say (inferred for quill's launch).
                if self.remote_request.is_none() && !std::mem::replace(&mut self.remote, true) {
                    decoded.events.push(AgentEvent::Remote {
                        on: true,
                        url: None,
                        error: None,
                    });
                }
            }
            ("system", Some("init")) => {
                decoded.events.push(AgentEvent::Session {
                    id: text(&msg["session_id"]),
                });
                // Without the initialize answer's commands, `system/init` names them (no details).
                if let Some(AgentEvent::Ready {
                    version,
                    mut capabilities,
                    ..
                }) = self.ready.clone()
                {
                    if capabilities.commands.is_empty() {
                        capabilities.commands = slash_commands(&msg["slash_commands"]);
                        decoded.events.push(AgentEvent::Ready {
                            version,
                            capabilities,
                            mode: None,
                        });
                    }
                }
            }
            ("system", Some("status")) => {
                if let Some(requests) = msg["user_message_uuids"].as_array() {
                    let requests = requests
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect();
                    decoded.events.push(AgentEvent::Running { requests });
                }
                if let Some(mode) = msg["permissionMode"].as_str() {
                    decoded.events.push(AgentEvent::ModeChanged {
                        mode: mode.to_owned(),
                    });
                }
            }
            // Verified on 2.1.310: a thinking block reports its estimated size as it grows.
            ("system", Some("thinking_tokens")) if parent.is_none() => {
                self.live_tokens = msg["estimated_tokens"].as_u64().unwrap_or(0);
                decoded.events.push(self.tokens());
            }
            // Verified on 2.1.310 ([F:agent]): a helper agent runs as a task tied to the
            // tool call that started it, reports its tokens as it goes, and ends with a summary.
            ("system", Some("task_started")) => decoded.events.push(AgentEvent::TaskStarted {
                id: text(&msg["task_id"]),
                call: msg["tool_use_id"].as_str().map(str::to_owned),
                description: text(&msg["description"]),
                // Desktop's names for the CLI's task types.
                kind: match msg["task_type"].as_str().unwrap_or_default() {
                    "local_agent" | "in_process_teammate" | "remote_agent" => "agent",
                    "local_bash" => "command",
                    "local_workflow" => "workflow",
                    "monitor_mcp" | "monitor_ws" => "monitor",
                    _ => "task",
                }
                .into(),
            }),
            ("system", Some(kind @ ("task_progress" | "task_notification"))) => {
                let usage = &msg["usage"];
                let usage = usage.is_object().then(|| TaskUsage {
                    tokens: usage["total_tokens"].as_u64().unwrap_or(0),
                    tool_uses: usage["tool_uses"].as_u64().unwrap_or(0),
                    duration_ms: usage["duration_ms"].as_u64().unwrap_or(0),
                });
                if let Some(usage) = usage {
                    self.task_tokens.insert(text(&msg["task_id"]), usage.tokens);
                    decoded.events.push(self.tokens());
                }
                if kind == "task_notification" {
                    decoded.events.push(AgentEvent::TaskEnded {
                        id: text(&msg["task_id"]),
                        call: msg["tool_use_id"].as_str().map(str::to_owned),
                        summary: msg["summary"].as_str().map(str::to_owned),
                        ok: msg["status"] == "completed",
                        stopped: msg["status"] == "stopped" || msg["status"] == "killed",
                        usage,
                    });
                } else if let Some(usage) = usage {
                    decoded.events.push(AgentEvent::TaskProgress {
                        id: text(&msg["task_id"]),
                        usage,
                    });
                }
            }
            ("system", _) => {}
            ("stream_event", _) => {
                // Thinking shows as it starts; the finished block carries nothing quill shows.
                if msg["event"]["content_block"]["type"] == "thinking" {
                    decoded.events.push(AgentEvent::Thinking { parent });
                } else if msg["event"]["delta"]["type"] == "thinking_delta" {
                    let text = text(&msg["event"]["delta"]["thinking"]);
                    if !text.is_empty() {
                        decoded
                            .events
                            .push(AgentEvent::ThinkingDelta { text, parent });
                    }
                } else if msg["event"]["delta"]["type"] == "text_delta" {
                    let text = text(&msg["event"]["delta"]["text"]);
                    decoded.events.push(AgentEvent::TextDelta { text, parent });
                } else if msg["event"]["type"] == "message_delta" && parent.is_none() {
                    // An answer's final size replaces the estimate of its thinking.
                    self.turn_tokens +=
                        msg["event"]["usage"]["output_tokens"].as_u64().unwrap_or(0);
                    self.live_tokens = 0;
                    decoded.events.push(self.tokens());
                }
            }
            ("assistant", _) => {
                if msg["error"] == "authentication_failed" {
                    self.auth_failed = true;
                }
                let usage = &msg["message"]["usage"];
                if parent.is_none() && usage.is_object() {
                    self.context = [
                        "input_tokens",
                        "cache_read_input_tokens",
                        "cache_creation_input_tokens",
                    ]
                    .iter()
                    .filter_map(|k| usage[*k].as_u64())
                    .sum();
                }
                for block in msg["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str() {
                        Some("text") => decoded.events.push(AgentEvent::Text {
                            text: text(&block["text"]),
                            parent: parent.clone(),
                            id: msg["uuid"].as_str().map(String::from),
                            at: None,
                        }),
                        Some("thinking") => {
                            let thinking = text(&block["thinking"]).trim().to_owned();
                            if !thinking.is_empty() {
                                decoded.events.push(AgentEvent::ThinkingText {
                                    text: thinking,
                                    parent: parent.clone(),
                                });
                            }
                        }
                        Some("tool_use") => {
                            let call = text(&block["id"]);
                            let name = text(&block["name"]);
                            self.calls
                                .insert(call.clone(), (name.clone(), block["input"].clone()));
                            decoded.events.push(AgentEvent::ToolUse {
                                call,
                                name,
                                input: block["input"].clone(),
                                parent: parent.clone(),
                            });
                        }
                        _ => {}
                    }
                }
            }
            // A message from another device comes back as quill's own do, but as a person's
            // and under an id quill never sent (2.1.316 [F:remote]).
            ("user", _) if msg["isReplay"] == true => {
                let request = text(&msg["uuid"]);
                if !self.prompts.remove(&request) && msg["origin"]["kind"] == "human" {
                    let text = match &msg["message"]["content"] {
                        Value::String(text) => text.clone(),
                        blocks => (blocks.as_array().into_iter().flatten())
                            .filter_map(|block| block["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    };
                    decoded.events.push(AgentEvent::Prompt { request, text });
                }
            }
            ("user", _) => {
                // Echoes of quill's own prompts carry no tool result and are skipped.
                for block in msg["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_result" {
                        let event = self.tool_result(block, &msg["tool_use_result"]);
                        decoded.events.push(event);
                    }
                }
            }
            ("result", _) if parent.is_none() => {
                self.turn_tokens = 0;
                self.live_tokens = 0;
                self.task_tokens.clear();
                let interrupted = std::mem::take(&mut self.interrupted)
                    && msg["subtype"] == "error_during_execution";
                let errors = msg["errors"]
                    .as_array()
                    .map(|e| e.iter().map(text).collect::<Vec<_>>().join("\n"));
                if std::mem::take(&mut self.auth_failed) {
                    decoded.events.push(AgentEvent::AuthRequired);
                }
                decoded.events.push(AgentEvent::TurnFinished {
                    ok: msg["is_error"] != true,
                    interrupted,
                    usage: Usage {
                        input_tokens: msg["usage"]["input_tokens"].as_u64().unwrap_or(0),
                        output_tokens: msg["usage"]["output_tokens"].as_u64().unwrap_or(0),
                        cost_usd: msg["total_cost_usd"].as_f64(),
                        context_tokens: std::mem::take(&mut self.context),
                        // Verified on 2.1.310: each model in `modelUsage` gives its window.
                        context_window: msg["modelUsage"]
                            .as_object()
                            .into_iter()
                            .flat_map(|models| models.values())
                            .filter_map(|m| m["contextWindow"].as_u64())
                            .max()
                            .unwrap_or(0),
                    },
                    error: errors.filter(|e| !e.is_empty()),
                });
            }
            ("result", _) => {}
            // Verified on 2.1.310: `rate_limit_info.unifiedWindows` gives each window's
            // `utilization` (0–1) and `resetsAt`, as Desktop's usage popover reads them.
            ("rate_limit_event", _) => {
                let windows = &msg["rate_limit_info"]["unifiedWindows"];
                let window = |w: &Value| {
                    Some(UsageWindow {
                        utilization: w["utilization"].as_f64()? as f32,
                        resets_at: w["resetsAt"].as_u64()?,
                    })
                };
                decoded.events.push(AgentEvent::Limits(PlanLimits {
                    session: window(&windows["five_hour"]),
                    weekly: window(&windows["seven_day"]),
                }));
            }
            (kind, _) if QUIET.contains(&kind) => {}
            _ => decoded.events.push(AgentEvent::Unknown(msg)),
        }
        decoded
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn success(id: &Value, response: Value) -> String {
    json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": response } })
        .to_string()
}

/// The in-process MCP server that carries quill's own tools ([`HOST_TOOLS`]); the agent sees
/// them as `mcp__sessions__…`.
const SERVER: &str = "sessions";
/// Where a project keeps its dev servers, as Claude Desktop does.
const LAUNCH_FILE: &str = ".claude/launch.json";
/// The launch file's place and format, for preview_start, in quill's words.
const LAUNCH_GUIDE: &str = "# Dev servers\n\npreview_start {name} runs a dev server from .claude/launch.json and opens it in the built-in browser. If the file does not exist, create it in this format:\n{\n  \"version\": \"0.0.1\",\n  \"configurations\": [\n    { \"name\": \"dev\", \"runtimeExecutable\": \"npm\", \"runtimeArgs\": [\"run\", \"dev\"], \"port\": 5173 }\n  ]\n}\nThe server gets its port in PORT. An optional \"url\" opens the preview there instead of http://localhost:<port>.";
/// The in-process MCP server of the built-in browser's tools ([`BROWSER_TOOLS`]):
/// `mcp__browser__navigate` and the rest.
const BROWSER_SERVER: &str = "browser";

/// When to start or propose another session, appended to the agent's system prompt. quill's
/// words for what Desktop says in its own section on starting other sessions.
const SESSIONS_GUIDE: &str = "# Other sessions\n\nWith start_session you can start a separate session for a task of its own, or propose one. A session is for work that outlives this turn and ends in something of its own (a branch, a PR, a change elsewhere); searching or reading widely, independent parts of one change, and testing or checking what you built stay here or go to a subagent. When the user asks in words for separate, parallel or background work, start it (initiation \"user_asked\"). When your work turns up follow-up that belongs in its own session and the user has not asked for it, propose it (initiation \"own_initiative\") rather than only mentioning it: the user sees a card and starts it with a click. Give every session a brief that stands alone. When the user says something meant for a session you started (a change to its task, a detail it needs, an answer it waits for), pass it on with send_to_session instead of doing that part here. Sessions you start report back to you in a message when their turn ends; then carry on with what their results unblock, or tell the user the outcome in a line or two. Start nothing the user did not ask for.";

/// The effort level Desktop adds over the model's own, its slider's last stop
/// ("Added Ultracode to the effort slider").
const ULTRACODE: &str = "ultracode";

/// Whether `/effort` takes `ultracode`, as its argument hint says ([F:simple]:
/// `<low|medium|high|xhigh|max|auto|ultracode [on|off]>`).
fn offers_ultracode(body: &Value) -> bool {
    body["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|command| {
            command["name"] == "effort" && text(&command["argumentHint"]).contains(ULTRACODE)
        })
}

/// The agent's slash commands, sorted by name and without repeats: objects from `initialize`
/// (`name`, `description`, `argumentHint`, `aliases`), or bare names from `system/init`.
fn slash_commands(list: &Value) -> Vec<AgentSlashCommand> {
    let mut commands: Vec<AgentSlashCommand> = list
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| match c.as_str() {
            Some(name) => AgentSlashCommand {
                name: name.into(),
                ..Default::default()
            },
            None => AgentSlashCommand {
                name: text(&c["name"]),
                description: text(&c["description"]),
                argument_hint: text(&c["argumentHint"]),
                aliases: c["aliases"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            },
        })
        .filter(|c| !c.name.is_empty())
        .collect();
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    commands.dedup_by(|a, b| a.name == b.name);
    commands
}

/// The answer to `get_context_usage` (CLI 2.1.310): its categories, each with a `kind`, and
/// the lists Desktop opens under them, MCP tools, memory files and custom agents.
fn context_usage(body: &Value) -> ContextUsage {
    let number = |v: &Value| v.as_u64().unwrap_or(0);
    let categories = body["categories"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| ContextCategory {
            name: text(&c["name"]),
            tokens: number(&c["tokens"]),
            kind: match c["kind"].as_str() {
                Some("deferred") => ContextKind::Deferred,
                Some("buffer") => ContextKind::Buffer,
                Some("free") => ContextKind::Free,
                _ if c["isDeferred"] == true => ContextKind::Deferred,
                _ => ContextKind::Used,
            },
        })
        .collect();
    let list = |key: &str, name: &dyn Fn(&Value) -> String| -> Vec<(String, u64)> {
        body[key]
            .as_array()
            .into_iter()
            .flatten()
            .map(|e| (name(e), number(&e["tokens"])))
            .collect()
    };
    // A tool's name carries its server ("mcp__github__create_issue"); the row shows both once,
    // a claude.ai connector by its own name ("claude_ai_Gmail" is "Gmail").
    let tool = |e: &Value| {
        let (server, name) = (text(&e["serverName"]), text(&e["name"]));
        let prefix = format!("mcp__{server}__");
        let name = name.strip_prefix(&prefix).unwrap_or(&name);
        let server = server
            .strip_prefix("claude_ai_")
            .unwrap_or(&server)
            .replace('_', " ");
        format!("{server} · {name}")
    };
    let groups = [
        ("MCP tools", list("mcpTools", &tool)),
        ("Memory files", list("memoryFiles", &|e| text(&e["path"]))),
        ("Custom agents", list("agents", &|e| text(&e["agentType"]))),
    ]
    .into_iter()
    .filter(|(_, entries)| !entries.is_empty())
    .map(|(name, entries)| (name.to_owned(), entries))
    .collect();
    ContextUsage {
        used: number(&body["totalTokens"]),
        max: body["rawMaxTokens"]
            .as_u64()
            .unwrap_or_else(|| number(&body["maxTokens"])),
        categories,
        groups,
    }
}

/// The answer to `initialize` describes the agent ([F:simple]). With `ultracode`, every model
/// with effort levels also offers Ultracode above them.
fn ready(body: &Value, ultracode: bool) -> AgentEvent {
    let models = body["models"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| ModelInfo {
            // `value` is what `--model` and `set_model` take; older entries may lack it.
            id: m["value"]
                .as_str()
                .map_or_else(|| text(&m["resolvedModel"]), str::to_owned),
            name: text(&m["displayName"]),
            efforts: {
                let mut efforts: Vec<String> = m["supportedEffortLevels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(text)
                    .collect();
                // Every model with effort levels offers it (inferred: Desktop showed it for
                // Opus 5.5; the person expects it on Haiku 5.5 too). It is applied as
                // Desktop applies it, whatever the model's own top level.
                if ultracode && !efforts.is_empty() {
                    efforts.push(ULTRACODE.into());
                }
                efforts
            },
            // Auto mode is the model's to offer (CLI 2.1.311 `supportsAutoMode`), as Desktop
            // lists it only for a model that has it.
            modes: if m["supportsAutoMode"].as_bool() == Some(true) {
                vec!["auto".into()]
            } else {
                vec![]
            },
        });
    AgentEvent::Ready {
        version: text(&body["claude_code_version"]),
        capabilities: AgentCapabilities {
            models: models.collect(),
            modes: MODES.map(String::from).into(),
            unsafe_modes: vec!["bypassPermissions".into()],
            interrupt: true,
            resume: true,
            fork: true,
            rewind: true,
            tool_hooks: true,
            mcp: true,
            mcp_toggle: true,
            mcp_clear_auth: true,
            thinking_text: true,
            steer: true,
            commands: slash_commands(&body["commands"]),
            remote: body["remote_control_available"] == true,
            // Each session has its own link; nothing to pair.
            remote_pairing: false,
            plan: body["account"]["subscriptionType"]
                .as_str()
                .map(str::to_owned),
            features: body["capabilities"]
                .as_array()
                .into_iter()
                .flatten()
                .map(text)
                .collect(),
        },
        mode: body["current_permission_mode"].as_str().map(str::to_owned),
    }
}

/// The keys of [`Claude`]'s own options.
const OUTPUT_STYLE: &str = "output_style";
const SANDBOX: &str = "sandbox";
const STRICT_SANDBOX: &str = "strict_sandbox";

/// The session's options as Claude Code settings: `outputStyle`, and `sandbox.enabled` with
/// `allowUnsandboxedCommands` off when strict. `None` when none is set.
fn option_settings(spec: &SessionSpec) -> Option<Value> {
    let on = |key: &str| spec.options.get(key).is_some_and(|v| v == "true");
    let mut settings = serde_json::Map::new();
    if let Some(style) = spec.options.get(OUTPUT_STYLE).filter(|s| !s.is_empty()) {
        settings.insert("outputStyle".into(), json!(style));
    }
    if on(SANDBOX) || on(STRICT_SANDBOX) {
        let mut sandbox = json!({ "enabled": true });
        if on(STRICT_SANDBOX) {
            sandbox["allowUnsandboxedCommands"] = json!(false);
        }
        settings.insert("sandbox".into(), sandbox);
    }
    (!settings.is_empty()).then_some(Value::Object(settings))
}

/// One line of `claude mcp list`: its name before the first ": " (a name may hold colons,
/// "plugin:vercel:vercel"), its target before the last " - ", and how it stands after it;
/// where it comes from by its name, as the CLI names plugins' and claude.ai's.
fn configured_server(line: &str) -> Option<quill_agent::ConfiguredServer> {
    use quill_agent::{McpSource, McpStatus};
    let (name, rest) = line.split_once(": ")?;
    let (target, state) = rest.rsplit_once(" - ")?;
    let status = if state.contains("Connected") {
        McpStatus::Connected
    } else if state.contains("authentication") {
        McpStatus::NeedsAuth
    } else if state.contains("Pending") {
        McpStatus::Pending
    } else if state.contains("isabled") {
        McpStatus::Disabled
    } else {
        McpStatus::Failed
    };
    let source = if name.starts_with("plugin:") {
        McpSource::Plugin
    } else if name.starts_with("claude.ai ") {
        McpSource::Account
    } else {
        McpSource::Other
    };
    let target = target
        .trim_end_matches(" (HTTP)")
        .trim_end_matches(" (SSE)");
    let name = name.trim();
    // "plugin:vercel:vercel" is its plugin's server "vercel"; "claude.ai Figma" the account's.
    let label = (name.strip_prefix("plugin:"))
        .and_then(|rest| rest.split_once(':').map(|(_, server)| server))
        .or_else(|| name.strip_prefix("claude.ai "))
        .unwrap_or(name);
    Some(quill_agent::ConfiguredServer {
        label: label.to_owned(),
        name: name.to_owned(),
        target: target.trim().to_owned(),
        status,
        checked: true,
        source,
    })
}

/// The rows of `get_skills_dialog` (CLI 2.1.311): a skill whose state is `off` is listed and
/// never used.
fn skills(body: &Value) -> Vec<Skill> {
    let rows = body["skills"].as_array().into_iter().flatten();
    rows.map(|row| Skill {
        name: text(&row["display_name"]),
        description: text(&row["description"]),
        source: text(&row["source"]),
        state: row["state"].as_str().unwrap_or("on").to_owned(),
        // The CLI's own words for each lock (2.1.311 `runSkillOverrideEdit`).
        locked: row["locked_by"].as_str().map(|lock| {
            match lock {
                "plugin" => "A plugin skill is managed with the plugin, not here.",
                "author" => "This skill's own file turns model invocation off; its state cannot be changed here.",
                "reserved-name" => "Settings can't store an entry with this skill's name; rename its folder or file to configure it.",
                _ => "A higher-priority configuration sets this skill's state; it cannot be changed here.",
            }
            .to_owned()
        }),
        key: json!({ "name": row["name"], "handles": row["handles"] }),
    })
    .collect()
}

/// A marketplace's source as a person may paste it, cleaned as Desktop's add dialog does: the command around it dropped, and an ssh address of GitHub
/// made https.
fn marketplace_source(pasted: &str) -> String {
    let mut source = pasted.trim();
    for lead in [
        "claude plugin marketplace add",
        "git clone",
        "gh repo clone",
    ] {
        source = source.strip_prefix(lead).map_or(source, str::trim);
    }
    match source.strip_prefix("git@github.com:") {
        Some(repo) => format!("https://github.com/{}", repo.trim_end_matches(".git")),
        None => source.to_owned(),
    }
}

/// The answer to `mcp_status` (CLI 2.1.311): each server with its status, where it came
/// from (`source`, else `scope`), why it failed and its tools. A plugin's server is named
/// `plugin:{plugin}:{server}`; quill's own in-process ones are `sdk`.
fn mcp_servers(body: &Value) -> Vec<McpServer> {
    let servers = body["mcpServers"].as_array().into_iter().flatten();
    servers
        .map(|server| {
            let name = text(&server["name"]);
            let status = match server["status"].as_str() {
                Some("connected") => McpStatus::Connected,
                Some("needs-auth") => McpStatus::NeedsAuth,
                Some("failed") => McpStatus::Failed,
                Some("disabled") => McpStatus::Disabled,
                _ => McpStatus::Pending,
            };
            let from = server["source"].as_str().or(server["scope"].as_str());
            let source = match from {
                Some("user") => McpSource::User,
                Some("project") => McpSource::Project,
                Some("local") => McpSource::Local,
                Some("managed" | "enterprise") => McpSource::Managed,
                Some("plugin") => McpSource::Plugin,
                Some("claudeai") => McpSource::Account,
                Some("sdk") => McpSource::App,
                _ => McpSource::Other,
            };
            let parts = (name.strip_prefix("plugin:")).and_then(|rest| rest.split_once(':'));
            let plugin = parts.map(|(plugin, _)| plugin.to_owned());
            // OAuth is for http and sse servers whose sign-in is not held by the account.
            let web = matches!(server["transport"].as_str(), Some("http" | "sse"));
            let held = server["sessionProxied"] == true || server["scope"] == "claudeai";
            McpServer {
                sign_in: web && !held,
                label: parts.map_or(name.as_str(), |(_, server)| server).to_owned(),
                status,
                source,
                plugin,
                error: server["error"].as_str().map(str::to_owned),
                tools: (server["tools"].as_array().into_iter().flatten())
                    .map(|tool| McpTool {
                        name: text(&tool["name"]),
                        description: tool["description"].as_str().map(str::to_owned),
                        read_only: tool["annotations"]["readOnly"] == true,
                        destructive: tool["annotations"]["destructive"] == true,
                    })
                    .collect(),
                name,
            }
        })
        .collect()
}

/// Rules and mode switches arrive mixed ([F:always]); quill keeps them apart so
/// "Always allow" never changes the mode by accident (engine.md §11.4).
fn suggestions(value: &Value) -> Vec<Suggestion> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| match s["type"].as_str() {
            Some("addRules") => Suggestion::Rules(s.clone()),
            Some("setMode") => Suggestion::Mode(text(&s["mode"])),
            _ => Suggestion::Other(s.clone()),
        })
        .collect()
}

fn suggestion_value(suggestion: Suggestion) -> Value {
    match suggestion {
        Suggestion::Rules(v) | Suggestion::Other(v) => v,
        Suggestion::Mode(mode) => {
            json!({ "type": "setMode", "mode": mode, "destination": "session" })
        }
    }
}

/// A WebSearch: its sources from the structured `results` (link lists), or else from the
/// `Links: [...]` JSON that starts the text; deduped by URL, in order.
fn web_run(input: &Value, structured: &Value, output: &str) -> WebSearchRun {
    let mut items: Vec<Value> = structured["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["content"].as_array())
        .flatten()
        .cloned()
        .collect();
    if items.is_empty() {
        let links = output
            .split_once("Links: ")
            .and_then(|(_, rest)| rest.lines().next());
        items = links
            .and_then(|json| serde_json::from_str::<Vec<Value>>(json).ok())
            .unwrap_or_default();
    }
    let mut sources: Vec<WebSource> = vec![];
    for item in &items {
        let url = text(&item["url"]);
        if !url.is_empty() && !sources.iter().any(|s| s.url == url) {
            sources.push(WebSource {
                title: text(&item["title"]),
                url,
            });
        }
    }
    let query = Some(text(&structured["query"])).filter(|q| !q.is_empty());
    let query = query.unwrap_or_else(|| text(&input["query"]));
    WebSearchRun { query, sources }
}

/// Write and Edit report the file before and after ([F:edit-bash]); other tools report none.
fn file_changes(structured: &Value) -> Vec<FileChange> {
    let Some(path) = structured["filePath"].as_str() else {
        return vec![];
    };
    let before = structured["originalFile"].as_str().map(str::to_owned);
    let after = if let Some(content) = structured["content"].as_str() {
        Some(content.to_owned())
    } else if let (Some(old), Some(new), Some(original)) = (
        structured["oldString"].as_str(),
        structured["newString"].as_str(),
        before.as_deref(),
    ) {
        Some(if structured["replaceAll"] == true {
            original.replace(old, new)
        } else {
            original.replacen(old, new, 1)
        })
    } else {
        None
    };
    vec![FileChange {
        path: path.into(),
        before,
        after,
    }]
}

mod inject;

#[cfg(test)]
mod tests;
