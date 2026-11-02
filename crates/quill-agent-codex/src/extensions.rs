//! What Codex says about its extensions: MCP servers, plugins, marketplaces and skills, and
//! the runs that change them. Shapes are those of `codex-cli 0.163.0` (docs/codex.md "Extensiones"):
//! `codex mcp list --json`, `codex plugin list --json --available`, `codex plugin marketplace
//! list --json`, and the app-server's `skills/list` and `mcpServerStatus/list`.

use super::{client, text};
use quill_agent::{
    ConfiguredServer, ExtensionOp, Marketplace, McpServer, McpSource, McpStatus, McpTool,
    NewServer, Plugin, PluginOffer, ProgramRun, Skill,
};
use serde_json::{json, Value};

/// The ids of the app-server requests that are quill's own: names, not numbers, so the
/// numbers of the requests every capture holds stay the ones the adapter writes.
pub(crate) const SKILLS_ID: &str = "quill-skills";
pub(crate) const MCP_STATUS_ID: &str = "quill-mcp-status";
pub(crate) const MCP_RELOAD_ID: &str = "quill-mcp-reload";
pub(crate) const MCP_LOGIN_ID: &str = "quill-mcp-login";
const SKILL_WRITE_ID: &str = "quill-skill-write";

fn run(args: &[&str], timeout: u64) -> ProgramRun {
    ProgramRun {
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        stdin: None,
        timeout,
        until: None,
    }
}

/// `codex mcp list --json`: a server a row, with how it runs but not where it was set up.
pub(crate) fn servers(stdout: &str) -> Vec<ConfiguredServer> {
    let list: Value = serde_json::from_str(stdout).unwrap_or_default();
    (list.as_array().into_iter().flatten())
        .map(|row| {
            let transport = &row["transport"];
            let target = match transport["type"].as_str() {
                Some("stdio") => {
                    let args = (transport["args"].as_array().into_iter().flatten())
                        .map(text)
                        .collect::<Vec<_>>();
                    let mut words = vec![text(&transport["command"])];
                    words.extend(args);
                    words.join(" ")
                }
                _ => text(&transport["url"]),
            };
            // A plugin's server is listed like the person's own; its program lives in the
            // plugin's folder (inferred from cua_repl, whose status names its plugin).
            let source = if target.contains("/.codex/plugins/") {
                McpSource::Plugin
            } else {
                McpSource::Other
            };
            let status = if row["enabled"] == false {
                McpStatus::Disabled
            } else if row["auth_status"] == "not_logged_in" {
                McpStatus::NeedsAuth
            } else {
                McpStatus::Connected
            };
            let name = text(&row["name"]);
            ConfiguredServer {
                label: name.clone(),
                name,
                target,
                status,
                // The list does not start the servers: it only says they are set up.
                checked: false,
                source,
            }
        })
        .collect()
}

/// `codex plugin list --json --available`: `{installed, available}`, a plugin's id being
/// `name@marketplace`.
pub(crate) fn plugins(stdout: &str) -> (Vec<Plugin>, Vec<PluginOffer>) {
    let list: Value = serde_json::from_str(stdout).unwrap_or_default();
    let installed = (list["installed"].as_array().into_iter().flatten())
        .map(|row| Plugin {
            id: text(&row["pluginId"]),
            name: text(&row["name"]),
            version: row["version"].as_str().map(str::to_owned),
            scope: "user".into(),
            enabled: row["enabled"] != false,
        })
        .collect();
    let offers = (list["available"].as_array().into_iter().flatten())
        .map(|row| PluginOffer {
            id: text(&row["pluginId"]),
            name: text(&row["name"]),
            description: String::new(),
            marketplace: text(&row["marketplaceName"]),
            version: row["version"].as_str().map(str::to_owned),
            installs: None,
        })
        .collect();
    (installed, offers)
}

/// `codex plugin marketplace list --json`: `{marketplaces: [{name, root, marketplaceSource?}]}`.
pub(crate) fn marketplaces(stdout: &str) -> Vec<Marketplace> {
    let list: Value = serde_json::from_str(stdout).unwrap_or_default();
    (list["marketplaces"].as_array().into_iter().flatten())
        .map(|row| Marketplace {
            name: text(&row["name"]),
            source: row["marketplaceSource"]["source"]
                .as_str()
                .or(row["root"].as_str())
                .unwrap_or_default()
                .to_owned(),
        })
        .collect()
}

/// The lines that list the skills of `cwd` without a thread: the handshake, then `skills/list`.
pub(crate) fn skills_query(cwd: &std::path::Path) -> Vec<String> {
    vec![
        json!({ "id": "quill-ext", "method": "initialize", "params": { "clientInfo": client() } })
            .to_string(),
        json!({ "method": "initialized" }).to_string(),
        json!({
            "id": SKILLS_ID, "method": "skills/list",
            "params": { "cwds": [cwd.to_string_lossy()], "forceReload": true },
        })
        .to_string(),
    ]
}

/// The answer to `skills/list`: `{data: [{cwd, skills: [{name, description, path, scope,
/// enabled, pluginId}], errors}]}`, `scope` being `user`, `repo`, `system` or `admin`.
pub(crate) fn skills(result: &Value) -> Vec<Skill> {
    let rows = (result["data"].as_array().into_iter().flatten())
        .flat_map(|folder| folder["skills"].as_array().into_iter().flatten());
    rows.map(|row| {
        let scope = row["scope"].as_str().unwrap_or("user");
        let source = match scope {
            _ if !row["pluginId"].is_null() => "plugin",
            "repo" => "project",
            other => other,
        };
        let built_in = scope == "system" || scope == "admin";
        Skill {
            name: text(&row["name"]),
            description: row["interface"]["shortDescription"]
                .as_str()
                .or(row["description"].as_str())
                .unwrap_or_default()
                .to_owned(),
            source: source.to_owned(),
            state: if row["enabled"] == false { "off" } else { "on" }.into(),
            locked: (built_in || source == "plugin").then(|| {
                if source == "plugin" {
                    "A plugin skill is managed with the plugin, not here.".to_owned()
                } else {
                    "This skill comes with Codex; its state cannot be changed here.".to_owned()
                }
            }),
            key: json!({ "path": row["path"] }),
        }
    })
    .collect()
}

/// The answer to `mcpServerStatus/list`: each server with how it stands in the thread, what it
/// is, and its tools.
pub(crate) fn mcp_servers(data: &Value) -> Vec<McpServer> {
    (data.as_array().into_iter().flatten())
        .map(|row| {
            let name = text(&row["name"]);
            let tools: Vec<McpTool> = (row["tools"].as_object().into_iter().flatten())
                .map(|(key, tool)| McpTool {
                    name: tool["name"].as_str().unwrap_or(key).to_owned(),
                    description: tool["description"].as_str().map(str::to_owned),
                    read_only: tool["annotations"]["readOnlyHint"] == true,
                    destructive: tool["annotations"]["destructiveHint"] == true,
                })
                .collect();
            let auth = row["authStatus"].as_str();
            // `runtimeStatus` is the thread's own; without one the server's tools say it.
            let status = match row["runtimeStatus"].as_str() {
                Some("connected") => McpStatus::Connected,
                Some("starting" | "notStarted") => McpStatus::Pending,
                Some("authenticationRequired") => McpStatus::NeedsAuth,
                Some("disabled") => McpStatus::Disabled,
                Some(_) => McpStatus::Failed,
                None if auth == Some("notLoggedIn") => McpStatus::NeedsAuth,
                None if !tools.is_empty() => McpStatus::Connected,
                None if !row["toolsError"].is_null() => McpStatus::Failed,
                None => McpStatus::Pending,
            };
            // `name@marketplace` is the plugin's id; people know it by its name.
            let plugin = (row["pluginId"].as_str())
                .map(|id| id.split_once('@').map_or(id, |(name, _)| name).to_owned());
            // The status does not say which layer of config.toml set a server up (inferred:
            // the person's own, or the project's).
            let source = if plugin.is_some() {
                McpSource::Plugin
            } else if name == "codex_apps" {
                McpSource::Account
            } else {
                McpSource::User
            };
            McpServer {
                label: if name == "codex_apps" {
                    "ChatGPT apps".into()
                } else {
                    name.clone()
                },
                name,
                status,
                source,
                plugin,
                error: row["toolsError"].as_str().map(str::to_owned),
                tools,
                // Signing in is OAuth's: a server with a bearer token or none has no page.
                sign_in: matches!(auth, Some("notLoggedIn" | "oAuth")),
            }
        })
        .collect()
}

/// `config.mcp_servers` of `thread/start` (V2): `command`, `args`, `env` and `env_vars` for a
/// command, `url` and `http_headers` / `env_http_headers` for an address. A secret is only
/// the name of a variable of the app-server's process.
/// The person's tool permissions in a thread's config, which Codex lays over the person's own
/// for that thread: a blocked tool among the server's `disabled_tools`, the others as the
/// tool's `approval_mode`, `approve` to allow and `prompt` to ask.
pub(crate) fn tool_rules_config(config: &mut Value, rules: &[quill_agent::ToolRule]) {
    use quill_agent::ToolPolicy;
    for rule in rules {
        let server = &mut config["mcp_servers"][&rule.server];
        match rule.policy {
            ToolPolicy::Block => {
                let hidden = &mut server["disabled_tools"];
                if !hidden.is_array() {
                    *hidden = json!([]);
                }
                hidden
                    .as_array_mut()
                    .expect("a list")
                    .push(json!(rule.tool));
            }
            ToolPolicy::Allow | ToolPolicy::Ask => {
                let mode = if rule.policy == ToolPolicy::Allow {
                    "approve"
                } else {
                    "prompt"
                };
                server["tools"][&rule.tool] = json!({ "approval_mode": mode });
            }
        }
    }
}

pub(crate) fn mcp_servers_config(servers: &[quill_agent::InjectedServer]) -> Value {
    let map = |pairs: &[(String, String)]| -> Value {
        pairs
            .iter()
            .map(|(k, v)| (k.clone(), json!(v)))
            .collect::<serde_json::Map<_, _>>()
            .into()
    };
    let mut all = serde_json::Map::new();
    for s in servers {
        let mut entry = if s.remote {
            json!({ "url": s.target })
        } else {
            json!({ "command": s.target, "args": s.args })
        };
        if s.remote {
            if !s.headers.is_empty() {
                entry["http_headers"] = map(&s.headers);
            }
            if !s.header_vars.is_empty() {
                entry["env_http_headers"] = map(&s.header_vars);
            }
        } else {
            if !s.env.is_empty() {
                entry["env"] = map(&s.env);
            }
            if !s.env_vars.is_empty() {
                // Same name: the variable the server reads is the one the process has.
                entry["env_vars"] = s.env_vars.iter().map(|(_, var)| json!(var)).collect();
            }
        }
        all.insert(s.name.clone(), entry);
    }
    Value::Object(all)
}

/// What to run for `op`, with Codex's own commands: `codex mcp`, `codex plugin`, and for a
/// skill's state the app-server, which only answers while its stdin is open.
pub(crate) fn op(op: &ExtensionOp) -> Option<ProgramRun> {
    Some(match op {
        // The plugin's switch is the config file's, which Codex has no command for.
        ExtensionOp::SwitchPlugin { .. } | ExtensionOp::Update(_) => return None,
        ExtensionOp::Install { id, .. } => run(&["plugin", "add", id], 120),
        ExtensionOp::Uninstall(plugin) => run(&["plugin", "remove", &plugin.id], 60),
        ExtensionOp::AddMarketplace(source) => {
            run(&["plugin", "marketplace", "add", source.trim()], 180)
        }
        ExtensionOp::UpdateMarketplace(name) => {
            run(&["plugin", "marketplace", "upgrade", name], 180)
        }
        ExtensionOp::RemoveMarketplace(name) => run(&["plugin", "marketplace", "remove", name], 60),
        ExtensionOp::AddServer(server) => add_server(server)?,
        ExtensionOp::RemoveServer(name) => run(&["mcp", "remove", name], 30),
        // `skills/config/write` by the skill's file; the write is in config.toml.
        ExtensionOp::SkillState { skill, state } => {
            let lines = [
                json!({ "id": "quill-ext", "method": "initialize", "params": { "clientInfo": client() } }),
                json!({ "method": "initialized" }),
                json!({
                    "id": SKILL_WRITE_ID, "method": "skills/config/write",
                    "params": { "path": skill.key["path"], "enabled": state == "on" },
                }),
            ];
            let lines: Vec<String> = lines.iter().map(Value::to_string).collect();
            ProgramRun {
                args: vec!["app-server".into()],
                stdin: Some(lines.join("\n") + "\n"),
                timeout: 30,
                until: Some(format!("\"id\":\"{SKILL_WRITE_ID}\"")),
            }
        }
    })
}

/// `codex mcp add NAME --url URL` or `codex mcp add NAME [--env K=V]… -- COMMAND [ARGS]…`
/// (`mcp add --help`, 0.163.0). Codex has no option for headers or an OAuth client of the
/// person's own: a server that needs them is not added here.
fn add_server(server: &NewServer) -> Option<ProgramRun> {
    let mut args: Vec<String> = vec!["mcp".into(), "add".into(), server.name.clone()];
    if server.remote {
        if !server.headers.is_empty() || server.client_id.is_some() {
            return None;
        }
        args.extend(["--url".into(), server.target.clone()]);
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

/// Why a run failed although it exited well: `codex mcp remove` of a name it does not have
/// says so and exits 0; the app-server answers an error as a JSON-RPC one.
pub(crate) fn failed(stdout: &str) -> Option<String> {
    let last = stdout.lines().rev().find(|l| !l.trim().is_empty())?;
    match serde_json::from_str::<Value>(last) {
        Ok(answer) => answer["error"]["message"].as_str().map(str::to_owned),
        Err(_) => last
            .starts_with("No MCP server named")
            .then(|| last.trim().to_owned()),
    }
}

/// A skill is named `$name` in a message to Codex (inferred from its TUI: the explicit
/// mention), where quill's `/` menu writes `/name`; a first word like a skill's name is turned,
/// so a path or a sentence is not.
pub(crate) fn skill_mention(text: &str) -> String {
    let Some(rest) = text.strip_prefix('/') else {
        return text.to_owned();
    };
    let (name, more) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':');
    if name.is_empty() || !name.chars().all(plain) {
        return text.to_owned();
    }
    format!("${name} {more}").trim_end().to_owned()
}
