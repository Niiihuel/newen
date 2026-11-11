//! Newen's adapter for Nous Research's Hermes Agent (docs/hermes.md): `hermes acp`, the Agent
//! Client Protocol over stdio, which `newen-agent-acp` speaks. What is Hermes's own lives here:
//! its program, its modes, how its tools read as Newen's, and its extensions.

use newen_agent::{
    AgentAdapter, AgentCommand, AgentId, AgentOption, Choices, ConfiguredServer, Decoded,
    ExtensionOp, LaunchPlan, McpSource, McpStatus, ModeTone, ModeWords, ProgramRun, SessionExtras,
    SessionSpec,
};
use newen_agent_acp::{by_kind, mcp_name, Acp, Call, Flavor};
use serde_json::{json, Value};

#[cfg(test)]
mod tests;

/// The person's options (Settings): Hermes's own launch switches, which become each new
/// session's policy (docs/hermes.md "Opciones"), and when each is on.
const OPTIONS: [(&str, &str, &str, &str, &str); 2] = [
    (
        "rules",
        "Project rules",
        "Give each session the project's AGENTS.md, SOUL.md, memory and preloaded skills.",
        "true",
        "HERMES_IGNORE_RULES",
    ),
    (
        "hooks",
        "Run hooks without asking",
        "Run the shell hooks of your Hermes settings without asking first.",
        "false",
        "HERMES_ACCEPT_HOOKS",
    ),
];

const ASK: &str = "default";
const EDITS: &str = "accept_edits";
const DONT_ASK: &str = "dont_ask";

/// How Hermes's tools read as Newen's (docs/hermes.md "Herramientas"). Hermes names a tool
/// only in its title, "<tool>: <what it does>", and gives no input for its own tools.
struct Tools;

impl Flavor for Tools {
    fn tool(&self, call: &Call, servers: &[String]) -> Option<(String, Value)> {
        let (name, said) = call.title.split_once(": ").unwrap_or((&call.title, ""));
        let said = said.trim();
        let (name, input) = match name {
            "terminal" => {
                let command = call.said(&["command"]).unwrap_or_else(|| {
                    let shown = call.text.strip_prefix("$ ").unwrap_or(said);
                    shown.lines().next().unwrap_or(said).to_owned()
                });
                ("Bash", json!({ "command": command }))
            }
            "read_file" => ("Read", json!({ "file_path": call.file() })),
            "write_file" => ("Write", json!({ "file_path": call.file() })),
            "patch" => ("Edit", json!({ "file_path": call.file() })),
            "search_files" => ("Grep", json!({ "pattern": said, "path": call.path })),
            "web_search" => ("WebSearch", json!({ "query": said })),
            "web_extract" => ("WebFetch", json!({ "url": said })),
            "delegate_task" => ("Agent", json!({ "description": said })),
            "skill_view" => ("Skill", json!({ "skill": said })),
            // Its task list comes whole as ACP's plan, which draws it.
            "todo_list" | "todo" => return None,
            name if name.starts_with("mcp__") => {
                return Some(match mcp(name, servers) {
                    Some((server, tool)) => (mcp_name(&server, &tool), call.input.clone()),
                    None => (name.to_owned(), call.input.clone()),
                })
            }
            // Its permission cards (`perm-check-…`, `edit-approval-…`) say what they ask in their
            // title and kind.
            _ => return Some(by_kind(call)),
        };
        Some((name.into(), input))
    }

    /// A reply it adds a note to (its check of the turn's edits) comes again whole.
    fn resends_words(&self) -> bool {
        true
    }

    /// It names its models `Provider · model` (`Custom endpoint · fixture-model`).
    fn provider(&self, name: &str) -> Option<(String, String)> {
        let (provider, own) = name.split_once(" · ")?;
        Some((provider.into(), own.into()))
    }
}

/// The server and tool of an MCP tool's name: `mcp__<server>__<tool>`, a server the session
/// was given being `editor_<server>_<8 hex>` (Hermes tells the session's own apart).
fn mcp(name: &str, servers: &[String]) -> Option<(String, String)> {
    let (server, tool) = name.strip_prefix("mcp__")?.split_once("__")?;
    let given = server.strip_prefix("editor_").and_then(|s| {
        let (base, tag) = s.rsplit_once('_')?;
        (tag.len() == 8 && tag.bytes().all(|b| b.is_ascii_hexdigit())).then_some(base)
    });
    let server = match given {
        // Hermes keeps letters, digits and `_` of a name: the session's server it came from.
        Some(base) => servers
            .iter()
            .find(|s| sanitized(s) == base)
            .cloned()
            .unwrap_or_else(|| base.to_owned()),
        None => server.to_owned(),
    };
    Some((server, tool.to_owned()))
}

fn sanitized(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The servers of `hermes mcp list`'s table, under its rule of `─`: name, transport (the
/// command or address, cut short), tools, and `✓ enabled` or `✗ disabled`, apart by runs of
/// spaces.
fn servers(stdout: &str) -> Vec<ConfiguredServer> {
    let rows = stdout
        .lines()
        .skip_while(|l| !l.trim_start().starts_with('─'))
        .skip(1)
        .take_while(|l| !l.trim().is_empty());
    rows.filter_map(|row| {
        let cells: Vec<&str> = row
            .trim()
            .split("  ")
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        let name = row.split_whitespace().next()?.to_owned();
        Some(ConfiguredServer {
            label: name.clone(),
            target: cells.get(1).copied().unwrap_or_default().to_owned(),
            status: if row.contains("disabled") {
                McpStatus::Disabled
            } else {
                McpStatus::Connected
            },
            checked: false,
            source: McpSource::User,
            name,
        })
    })
    .collect()
}

static TOOLS: Tools = Tools;

pub struct Hermes(Acp);

impl Hermes {
    pub fn new() -> Self {
        Self(Acp::new(&TOOLS))
    }
}

impl Default for Hermes {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAdapter for Hermes {
    fn id(&self) -> AgentId {
        "hermes"
    }

    fn name(&self) -> &'static str {
        "Hermes"
    }

    fn tested(&self) -> &'static [&'static str] {
        &["2026.9.24", "0.0.0"]
    }

    fn launch(&self, spec: &SessionSpec) -> LaunchPlan {
        self.0.remember(spec);
        // `rules` is on unless turned off; `hooks` is off unless turned on.
        let env = (OPTIONS.iter())
            .filter(|(key, _, _, default, _)| {
                let on = spec.options.get(*key).map_or(*default, String::as_str) == "true";
                on != (*key == "rules")
            })
            .map(|(.., variable)| ((*variable).to_owned(), "1".to_owned()))
            .collect();
        LaunchPlan {
            program: "hermes".into(),
            args: vec!["acp".into()],
            env,
            env_remove: vec![],
            cwd: spec.cwd.clone(),
        }
    }

    fn options(&self) -> Vec<AgentOption> {
        (OPTIONS.iter())
            .map(|(key, label, description, default, _)| AgentOption {
                key: (*key).into(),
                label: (*label).into(),
                description: (*description).into(),
                choices: vec![],
                default: (*default).into(),
            })
            .collect()
    }

    /// Its list reads its config and starts no server.
    fn servers_query(&self) -> Option<Vec<String>> {
        Some(["mcp", "list"].map(String::from).into())
    }

    fn servers(&self, stdout: &str) -> Vec<ConfiguredServer> {
        servers(stdout)
    }

    /// Its servers are the profile's (config.yaml): no project has its own.
    fn server_scopes(&self) -> Choices {
        vec![("user".into(), "All my projects".into())]
    }

    /// Written with `config set`, which takes the whole entry (its own `mcp add` asks in the
    /// terminal and takes no headers), and taken away with `config unset`.
    fn extension_op(&self, op: &ExtensionOp) -> Option<ProgramRun> {
        let pairs = |list: &[String], split: char| -> serde_json::Map<String, Value> {
            list.iter()
                .filter_map(|p| p.split_once(split))
                .map(|(k, v)| (k.trim().to_owned(), json!(v.trim())))
                .collect()
        };
        let args: Vec<String> = match op {
            ExtensionOp::AddServer(server) if server.client_id.is_none() => {
                let entry = if server.remote {
                    json!({ "url": server.target, "headers": pairs(&server.headers, ':'), "enabled": true })
                } else {
                    json!({ "command": server.target, "args": server.args,
                        "env": pairs(&server.env, '='), "enabled": true })
                };
                let key = format!("mcp_servers.{}", server.name);
                vec!["config".into(), "set".into(), key, entry.to_string()]
            }
            ExtensionOp::RemoveServer(name) => {
                vec![
                    "config".into(),
                    "unset".into(),
                    format!("mcp_servers.{name}"),
                ]
            }
            _ => return None,
        };
        Some(ProgramRun {
            args,
            stdin: None,
            timeout: 60,
            until: None,
        })
    }

    // `session/new` takes stdio, HTTP and SSE servers (docs/hermes.md). A secret reaches a
    // server through Hermes's own environment, which its servers inherit.
    fn session_extras(&self) -> SessionExtras {
        SessionExtras {
            servers: true,
            sse: true,
            env_by_name: false,
            skills: false,
            host: true,
            inline_secrets: true,
        }
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
                ASK,
                "Ask",
                "Asks before every edit and risky command",
                ModeTone::Ask,
            ),
            mode(
                EDITS,
                "Accept edits",
                "Edits this folder by itself; asks for sensitive files and risky commands",
                ModeTone::Edits,
            ),
            mode(
                DONT_ASK,
                "Don't ask",
                "Makes every edit by itself but sensitive files; asks before risky commands",
                ModeTone::Auto,
            ),
        ]
    }

    fn default_mode(&self) -> &'static str {
        ASK
    }

    /// The Nous Research mascot from Hermes's assets, in currentColor (MIT, NousResearch/hermes-agent).
    fn icon(&self) -> Option<&'static str> {
        Some(include_str!("../assets/hermes.svg"))
    }

    // `hermes update`, as its documentation says; not tried here (inferred).
    fn update_args(&self) -> Option<Vec<String>> {
        Some(vec!["update".into()])
    }

    fn encode(&mut self, command: AgentCommand) -> Vec<String> {
        self.0.encode(command)
    }

    fn decode(&mut self, line: &str) -> Decoded {
        self.0.decode(line)
    }
}
