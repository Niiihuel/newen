//! Newen's adapter for the OpenCode CLI (docs/opencode.md): `opencode acp`, the Agent Client
//! Protocol over stdio, which `newen-agent-acp` speaks. What is OpenCode's own lives here: its
//! program, its modes, how its tools read as Newen's, and its extensions.

use newen_agent::{
    AgentAdapter, AgentCommand, AgentId, AgentOption, Choices, ConfiguredServer, Decoded,
    ExtensionOp, LaunchPlan, McpSource, McpStatus, ModeTone, ModeWords, ModelInfo, ProgramRun,
    Provider, SessionExtras, SessionSpec,
};
use newen_agent_acp::{by_kind, mcp_name, Acp, Call, Flavor};
use serde_json::{json, Value};

#[cfg(test)]
mod tests;

/// A request to OpenCode's own server, a private one for this run (`--standalone`).
fn api(path: &str) -> ProgramRun {
    ProgramRun {
        args: ["--standalone", "api", "GET", path]
            .map(String::from)
            .into(),
        stdin: None,
        timeout: 60,
        until: None,
    }
}

/// The JSON a run printed first: `api` follows a failed request's body with its HTTP status.
fn first_json(stdout: &str) -> Option<Value> {
    serde_json::Deserializer::from_str(stdout)
        .into_iter::<Value>()
        .next()?
        .ok()
}

/// The modes as `opencode acp` offers them: its primary agents (`build` first), and Newen's
/// own `ask`, an agent the session's config adds (`launch`).
const BUILD: &str = "build";
const PLAN: &str = "plan";
const ASK: &str = "ask";

/// The agent `ask`: OpenCode's build, asking before every edit and shell command
/// (docs/opencode.md "Modos"; rules are matched last first).
const ASK_AGENT: &str = r#"{"mode":"primary","description":"Asks before every edit and shell command","permissions":[{"action":"edit","resource":"*","effect":"ask"},{"action":"shell","resource":"*","effect":"ask"}]}"#;

/// The person's options (Settings), each turned off by one of OpenCode's own variables.
const OPTIONS: [(&str, &str, &str, &str); 3] = [
    (
        "free_models",
        "Free models",
        "Offer OpenCode Zen's free models, which need no account.",
        "OPENCODE_DISABLE_FREE_MODELS",
    ),
    (
        "claude_skills",
        "Claude Code skills",
        "Load the skills in ~/.claude/skills and in a project's .claude/skills.",
        "OPENCODE_DISABLE_CLAUDE_CODE",
    ),
    (
        "project_config",
        "Project settings",
        "Load a project's opencode.json and .opencode folder, its MCP servers included.",
        "OPENCODE_DISABLE_PROJECT_CONFIG",
    ),
];

/// Where `opencode mcp add` writes: the person's settings, or the project's.
const GLOBAL: &str = "global";
const PROJECT: &str = "project";

/// How OpenCode's tools read as Newen's (docs/opencode.md "Herramientas"): `name` is OpenCode's
/// own tool name, and its input keys are camelCase.
struct Tools;

impl Flavor for Tools {
    fn tool(&self, call: &Call, servers: &[String]) -> Option<(String, Value)> {
        let input = &call.input;
        let (name, input) = match call.name.as_str() {
            "shell" | "bash" => {
                let command = call
                    .said(&["command"])
                    .unwrap_or_else(|| call.title.clone());
                (
                    "Bash",
                    json!({ "command": command, "description": input["description"] }),
                )
            }
            "read" => (
                "Read",
                json!({ "file_path": call.file(), "offset": input["offset"], "limit": input["limit"] }),
            ),
            "edit" => (
                "Edit",
                json!({ "file_path": call.file(), "old_string": input["oldString"],
                    "new_string": input["newString"], "replace_all": input["replaceAll"] == true }),
            ),
            "write" => (
                "Write",
                json!({ "file_path": call.file(), "content": input["content"] }),
            ),
            // A patch may touch several files: the row names the first.
            "patch" | "apply_patch" => ("Edit", json!({ "file_path": call.file() })),
            "glob" => (
                "Glob",
                json!({ "pattern": input["pattern"], "path": input["path"] }),
            ),
            "grep" => (
                "Grep",
                json!({ "pattern": input["pattern"], "path": input["path"], "glob": input["include"] }),
            ),
            "webfetch" => ("WebFetch", json!({ "url": input["url"] })),
            "websearch" => ("WebSearch", json!({ "query": input["query"] })),
            // A task given up is no longer open: the list holds nobody waiting on the session.
            "todowrite" => {
                let todos: Vec<Value> = (input["todos"].as_array().into_iter().flatten())
                    .map(|t| {
                        let status = match t["status"].as_str() {
                            Some("cancelled") => "completed",
                            Some(status) => status,
                            None => "pending",
                        };
                        json!({ "content": t["content"], "activeForm": t["content"], "status": status })
                    })
                    .collect();
                ("TodoWrite", json!({ "todos": todos }))
            }
            "subagent" | "task" => (
                "Agent",
                json!({ "description": input["description"], "prompt": input["prompt"],
                    "subagent_type": input["agent"] }),
            ),
            "skill" => ("Skill", json!({ "skill": input["id"] })),
            // Code Mode: MCP tools are called from a script (`await tools.<server>.<tool>(…)`);
            // a script that calls one tool reads as that tool.
            "execute" => {
                let code = call.said(&["code"]).unwrap_or_default();
                return Some(match scripted(&code).as_slice() {
                    [(server, tool, args)] => (mcp_name(server, tool), args.clone()),
                    _ => ("execute".into(), json!({ "code": code })),
                });
            }
            // An MCP tool is `<server>_<tool>`; Newen's own servers' among them.
            name => {
                let server = servers.iter().find(|s| {
                    name.strip_prefix(s.as_str())
                        .is_some_and(|rest| rest.starts_with('_'))
                });
                return Some(match server {
                    Some(server) => (mcp_name(server, &name[server.len() + 1..]), input.clone()),
                    None => by_kind(call),
                });
            }
        };
        Some((name.into(), input))
    }

    /// A helper agent's work comes apart from the session's, as OpenCode's own extension, so
    /// its words never mix into the conversation (docs/opencode.md "Subagentes").
    fn client_meta(&self) -> Value {
        json!({ "opencode/child-session-updates": true })
    }

    fn quiet(&self, method: &str) -> bool {
        method.trim_start_matches('_') == "opencode/session/child_update"
    }
}

/// The MCP tools a Code Mode script calls, as (server, tool, input): `tools.<server>.<tool>(<input>)`,
/// the input when it is written as JSON (else none).
fn scripted(code: &str) -> Vec<(String, String, Value)> {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    let mut calls: Vec<(String, String, Value)> = vec![];
    for (at, _) in code.match_indices("tools.") {
        let rest = &code[at + "tools.".len()..];
        let server: String = rest.chars().take_while(|c| word(*c)).collect();
        let Some(rest) = rest[server.len()..].strip_prefix('.') else {
            continue;
        };
        let tool: String = rest.chars().take_while(|c| word(*c)).collect();
        let Some(args) = rest[tool.len()..].strip_prefix('(') else {
            continue;
        };
        let input = args
            .rfind(')')
            .and_then(|end| serde_json::from_str::<Value>(args[..end].trim()).ok())
            .unwrap_or_else(|| json!({}));
        if !server.is_empty()
            && !tool.is_empty()
            && !calls.iter().any(|c| c.0 == server && c.1 == tool)
        {
            calls.push((server, tool, input));
        }
    }
    calls
}

/// The servers of `debug config`'s layers, the person's own (`[{type, path, info}]`, the
/// global config's folder listed after its file): each by the last layer that sets it, which is
/// the one OpenCode uses. A layer without a file is a process's own (Newen's), not listed.
fn servers(stdout: &str) -> Vec<ConfiguredServer> {
    let layers: Value = serde_json::from_str(stdout).unwrap_or_default();
    let layers = layers.as_array().cloned().unwrap_or_default();
    let global = (layers.iter())
        .find(|l| l["type"] == "directory")
        .and_then(|l| l["path"].as_str())
        .map(str::to_owned);
    let mut found: Vec<ConfiguredServer> = vec![];
    for layer in layers.iter().filter(|l| l["type"] == "document") {
        let Some(path) = layer["path"].as_str() else {
            continue;
        };
        let source = match &global {
            Some(dir) if path.starts_with(dir.as_str()) => McpSource::User,
            _ => McpSource::Project,
        };
        let servers = layer["info"]["mcp"]["servers"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        for (name, server) in servers {
            let target = match server["type"].as_str() {
                Some("remote") => server["url"].as_str().unwrap_or_default().to_owned(),
                _ => (server["command"].as_array().into_iter().flatten())
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
            };
            found.retain(|s| s.name != name);
            found.push(ConfiguredServer {
                label: name.clone(),
                name,
                target,
                status: if server["disabled"] == true {
                    McpStatus::Disabled
                } else {
                    McpStatus::Connected
                },
                // Read from its config: set up, not connected to.
                checked: false,
                source,
            });
        }
    }
    found
}

static TOOLS: Tools = Tools;

pub struct OpenCode(Acp);

impl OpenCode {
    pub fn new() -> Self {
        Self(Acp::new(&TOOLS))
    }
}

impl Default for OpenCode {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAdapter for OpenCode {
    fn id(&self) -> AgentId {
        "opencode"
    }

    fn name(&self) -> &'static str {
        "OpenCode"
    }

    fn tested(&self) -> &'static [&'static str] {
        &["2.2.5"]
    }

    /// One process a session. Its config for this process alone (`OPENCODE_CONFIG_CONTENT`, a
    /// layer over the person's own): the `ask` agent and the "All agents" skills. Newen writes
    /// none of OpenCode's files.
    fn launch(&self, spec: &SessionSpec) -> LaunchPlan {
        self.0.remember(spec);
        let mut config =
            json!({ "agents": { ASK: serde_json::from_str::<Value>(ASK_AGENT).unwrap() } });
        if !spec.skill_roots.is_empty() {
            config["skills"] = json!(spec.skill_roots);
        }
        let mut env = vec![("OPENCODE_CONFIG_CONTENT".to_owned(), config.to_string())];
        for (key, _, _, variable) in OPTIONS {
            if spec.options.get(key).is_some_and(|v| v == "false") {
                env.push((variable.into(), "1".into()));
            }
        }
        LaunchPlan {
            program: "opencode".into(),
            args: vec!["acp".into()],
            env,
            env_remove: vec![],
            cwd: spec.cwd.clone(),
        }
    }

    fn options(&self) -> Vec<AgentOption> {
        (OPTIONS.iter())
            .map(|(key, label, description, _)| AgentOption {
                key: (*key).into(),
                label: (*label).into(),
                description: (*description).into(),
                choices: vec![],
                default: "true".into(),
            })
            .collect()
    }

    /// Its integrations, the model providers it can use, as its server lists them
    /// (docs/opencode.md "Proveedores"), on a private server of its own as `debug config` is.
    fn providers_run(&self) -> Option<ProgramRun> {
        Some(api("/api/integration"))
    }

    fn providers(&self, stdout: &str) -> Vec<Provider> {
        let Some(body) = first_json(stdout) else {
            return vec![];
        };
        let mut providers: Vec<Provider> = (body["data"].as_array().into_iter().flatten())
            .filter_map(|integration| {
                let id = integration["id"].as_str()?.to_owned();
                let methods: Vec<&Value> =
                    (integration["methods"].as_array().into_iter().flatten()).collect();
                let has = |kind: &str| methods.iter().any(|m| m["type"] == kind);
                let mut connections = integration["connections"].as_array().into_iter().flatten();
                Some(Provider {
                    name: integration["name"].as_str().unwrap_or(&id).to_owned(),
                    connected: (connections.clone())
                        .filter(|c| c["type"] == "credential")
                        .filter_map(|c| c["id"].as_str().map(str::to_owned))
                        .collect(),
                    key: has("key"),
                    env: (methods.iter())
                        .filter(|m| m["type"] == "env")
                        .flat_map(|m| m["names"].as_array().into_iter().flatten())
                        .filter_map(|n| n.as_str().map(str::to_owned))
                        .collect(),
                    env_set: (connections)
                        .find(|c| c["type"] == "env")
                        .and_then(|c| c["name"].as_str().map(str::to_owned)),
                    // A sign-in in the browser or a profile: OpenCode's own terminal asks for it.
                    terminal: (has("oauth") || has("external"))
                        .then(|| format!("opencode auth login {id}")),
                    id,
                })
            })
            .collect();
        providers.sort_by_key(|p| p.name.to_lowercase());
        providers
    }

    /// The key goes in as a credential `auth import` reads on stdin: its `auth login` asks for
    /// a key only in a terminal, and the server's `connect/key` takes it in an argument.
    fn provider_connect_run(&self, provider: &str, key: &str) -> Option<ProgramRun> {
        let credential = json!([{
            "id": format!("newen-{provider}"), "integrationID": provider, "label": "Newen",
            "active": true, "value": { "type": "key", "key": key },
        }]);
        Some(ProgramRun {
            args: ["--standalone", "auth", "import"].map(String::from).into(),
            stdin: Some(credential.to_string()),
            timeout: 60,
            until: None,
        })
    }

    fn provider_disconnect_runs(&self, provider: &str, ids: &[String]) -> Vec<ProgramRun> {
        (ids.iter())
            .map(|id| ProgramRun {
                args: ["--standalone", "auth", "logout", provider, id]
                    .map(String::from)
                    .into(),
                stdin: None,
                timeout: 60,
                until: None,
            })
            .collect()
    }

    /// Its catalogue of the models its providers give, and the one it runs by default: a
    /// session's own list has the efforts of its current model alone.
    fn models_runs(&self) -> Vec<ProgramRun> {
        vec![api("/api/model"), api("/api/model/default")]
    }

    fn models_listed(&self, outputs: &[String]) -> Vec<ModelInfo> {
        let id = |m: &Value| -> Option<String> {
            Some(format!(
                "{}/{}",
                m["providerID"].as_str()?,
                m["id"].as_str()?
            ))
        };
        let default = (outputs.get(1).and_then(|o| first_json(o))).and_then(|d| id(&d["data"]));
        let Some(list) = outputs.first().and_then(|o| first_json(o)) else {
            return vec![];
        };
        (list["data"].as_array().into_iter().flatten())
            .filter(|m| m["enabled"] != false)
            .filter_map(|m| {
                let model = id(m)?;
                Some(ModelInfo {
                    name: m["name"].as_str().unwrap_or(&model).to_owned(),
                    // Its variants are its efforts, the session's `thought_level` choices.
                    efforts: (m["variants"].as_array().into_iter().flatten())
                        .filter_map(|v| v.as_str().or(v["id"].as_str()).map(str::to_owned))
                        .collect(),
                    modes: vec![],
                    default: default.as_deref() == Some(model.as_str()),
                    id: model,
                })
            })
            .collect()
    }

    /// The layers of its config, as read for the folder (`debug config`, which waits for them;
    /// its server list does not, docs/opencode.md "Customize").
    fn servers_query(&self) -> Option<Vec<String>> {
        Some(["debug", "config", "--standalone"].map(String::from).into())
    }

    fn servers(&self, stdout: &str) -> Vec<ConfiguredServer> {
        servers(stdout)
    }

    fn server_scopes(&self) -> Choices {
        vec![
            (GLOBAL.into(), "All my projects".into()),
            (PROJECT.into(), "This project".into()),
        ]
    }

    /// `opencode mcp add`; it has no command that removes a server (its file is edited by hand).
    fn extension_op(&self, op: &ExtensionOp) -> Option<ProgramRun> {
        let ExtensionOp::AddServer(server) = op else {
            return None;
        };
        if server.client_id.is_some() {
            return None;
        }
        let mut args: Vec<String> = vec!["mcp".into(), "add".into(), server.name.clone()];
        if server.scope.as_deref() != Some(PROJECT) {
            args.push("--global".into());
        }
        if server.remote {
            args.extend(["--url".into(), server.target.clone()]);
            for header in &server.headers {
                // `Name: value` as Newen keeps it, `Name=value` as OpenCode takes it.
                let (name, value) = header.split_once(':').unwrap_or((header, ""));
                args.extend([
                    "--header".into(),
                    format!("{}={}", name.trim(), value.trim()),
                ]);
            }
        } else {
            for pair in &server.env {
                args.extend(["--env".into(), pair.clone()]);
            }
            args.push("--".into());
            args.push(server.target.clone());
            args.extend(server.args.iter().cloned());
        }
        Some(ProgramRun {
            args,
            stdin: None,
            timeout: 30,
            until: None,
        })
    }

    // stdio and streamable HTTP only: `session/new` refuses SSE (docs/opencode.md). A secret
    // reaches a server through OpenCode's own environment, which its servers inherit.
    fn session_extras(&self) -> SessionExtras {
        SessionExtras {
            servers: true,
            sse: false,
            env_by_name: false,
            skills: true,
            host: true,
            inline_secrets: true,
        }
    }

    /// Build asks only as the person's OpenCode permissions say (by default, only to read a
    /// `.env` file), so a dev server starts without a card.
    fn mode_waives_prompts(&self, mode: &str) -> bool {
        mode == BUILD
    }

    fn modes(&self) -> Vec<ModeWords> {
        let mode = |id: &str, label: &str, about: &str, tone| ModeWords {
            id: id.into(),
            label: label.into(),
            about: about.into(),
            tone,
            listed: true,
        };
        vec![
            mode(
                BUILD,
                "Build",
                "Works with every tool, asking only where your OpenCode permissions say",
                ModeTone::Auto,
            ),
            mode(
                PLAN,
                "Plan",
                "Reads and plans; changes no file of the project",
                ModeTone::Plan,
            ),
            mode(
                ASK,
                "Ask",
                "Asks before every edit and shell command",
                ModeTone::Ask,
            ),
        ]
    }

    fn default_mode(&self) -> &'static str {
        BUILD
    }

    /// OpenCode's mark from its brand assets, in currentColor (MIT, anomalyco/opencode).
    fn icon(&self) -> Option<&'static str> {
        Some(include_str!("../assets/opencode.svg"))
    }

    fn encode(&mut self, command: AgentCommand) -> Vec<String> {
        self.0.encode(command)
    }

    fn decode(&mut self, line: &str) -> Decoded {
        self.0.decode(line)
    }
}
