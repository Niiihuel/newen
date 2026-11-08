//! Newen's adapter for Google's Antigravity CLI (docs/antigravity.md): `agy acp`, the Agent
//! Client Protocol over stdio, which `newen-agent-acp` speaks. What is Antigravity's own lives
//! here: its program, its modes (the one that never asks is a launch switch), how its tools read
//! as Newen's, and its connectors.

use newen_agent::{
    AgentAdapter, AgentCommand, AgentEvent, AgentId, Choices, ConfiguredServer, Decoded,
    ExtensionOp, LaunchPlan, McpSource, McpStatus, ModeTone, ModeWords, ProgramRun, SessionExtras,
    SessionSpec,
};
use newen_agent_acp::{by_kind, mcp_name, Acp, Call, Flavor};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
mod tests;

const DEFAULT: &str = "default";
const EDITS: &str = "accept-edits";
const PLAN: &str = "plan";
/// Newen's id for `--dangerously-skip-permissions`, which ACP's modes do not have.
const SKIP: &str = "skip-permissions";

/// How Antigravity's tools read as Newen's (docs/antigravity.md "Herramientas"): its input's
/// keys are capitalized (`Path`, `CommandLine`…), and a tool it does not name by kind is
/// "Run <tool>".
struct Tools;

impl Flavor for Tools {
    fn tool(&self, call: &Call, servers: &[String]) -> Option<(String, Value)> {
        let input = &call.input;
        let said = |key: &str| input[key].as_str().map(str::to_owned);
        let file = said("Path").unwrap_or_else(|| call.file());
        let (name, given) = match call.kind.as_str() {
            "execute" => (
                "Bash",
                json!({ "command": said("CommandLine").unwrap_or_else(|| call.title.clone()),
                    "description": said("toolSummary") }),
            ),
            "read" => ("Read", json!({ "file_path": file })),
            // A plan it asks the person to review is an artifact it writes, then ends its turn.
            "edit" if input["ArtifactMetadata"]["RequestFeedback"] == true => {
                ("ExitPlanMode", json!({ "plan": said("Content") }))
            }
            "edit" if input.get("Content").is_some() => (
                "Write",
                json!({ "file_path": file, "content": said("Content") }),
            ),
            "edit" if input.get("Target").is_some() => (
                "Edit",
                json!({ "file_path": file, "old_string": said("Target"),
                    "new_string": said("Replacement") }),
            ),
            _ => match call.title.strip_prefix("Run ") {
                // Its question is the form it asks with (`elicitation/create`).
                Some("ask_question") => return None,
                Some(tool) => match tool.strip_prefix("mcp_").and_then(|t| mcp(t, servers)) {
                    Some((server, tool)) => return Some((mcp_name(&server, &tool), json!({}))),
                    None => (tool, input.clone()),
                },
                None => return Some(by_kind(call)),
            },
        };
        Some((name.into(), given))
    }

    fn unsafe_modes(&self) -> &'static [&'static str] {
        &[SKIP]
    }
}

/// The server and tool of `<server>_<tool>`, `mcp_` taken off: the longest of the session's
/// servers it starts with (a server's name may hold `_` too).
fn mcp(named: &str, servers: &[String]) -> Option<(String, String)> {
    let server = (servers.iter())
        .filter(|s| named.starts_with(&format!("{s}_")))
        .max_by_key(|s| s.len())?;
    Some((server.clone(), named[server.len() + 1..].to_owned()))
}

/// The servers of `agy mcp list`'s table: `NAME TYPE STATUS COMMAND/URL`, apart by spaces.
fn servers(stdout: &str) -> Vec<ConfiguredServer> {
    (stdout
        .lines()
        .skip_while(|l| !l.starts_with("NAME"))
        .skip(1))
    .filter_map(|row| {
        let mut cells = row.split_whitespace();
        let name = cells.next()?.to_owned();
        let _kind = cells.next()?;
        let status = cells.next()?;
        Some(ConfiguredServer {
            label: name.clone(),
            name,
            target: cells.collect::<Vec<_>>().join(" "),
            status: match status {
                "disabled" => McpStatus::Disabled,
                _ => McpStatus::Connected,
            },
            // Read from its config: set up, not connected to.
            checked: false,
            source: McpSource::User,
        })
    })
    .collect()
}

static TOOLS: Tools = Tools;

pub struct Antigravity {
    acp: Acp,
    /// The process was launched skipping every approval: ACP still says `default`.
    skipping: AtomicBool,
}

impl Antigravity {
    pub fn new() -> Self {
        Self {
            acp: Acp::new(&TOOLS),
            skipping: AtomicBool::new(false),
        }
    }
}

impl Default for Antigravity {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAdapter for Antigravity {
    fn id(&self) -> AgentId {
        "antigravity"
    }

    fn name(&self) -> &'static str {
        "Antigravity"
    }

    fn tested(&self) -> &'static [&'static str] {
        &["1.5.3"]
    }

    fn launch(&self, spec: &SessionSpec) -> LaunchPlan {
        self.acp.remember(spec);
        let skip = spec.mode.as_deref() == Some(SKIP);
        self.skipping.store(skip, Ordering::Relaxed);
        let mut args = vec!["acp".to_owned()];
        if skip {
            args.push("--dangerously-skip-permissions".into());
        }
        LaunchPlan {
            program: "agy".into(),
            args,
            env: vec![],
            env_remove: vec![],
            cwd: spec.cwd.clone(),
        }
    }

    fn launch_mode(&self, mode: &str) -> bool {
        mode == SKIP
    }

    fn mode_waives_prompts(&self, mode: &str) -> bool {
        mode == SKIP
    }

    fn servers_query(&self) -> Option<Vec<String>> {
        Some(["mcp", "list"].map(String::from).into())
    }

    fn servers(&self, stdout: &str) -> Vec<ConfiguredServer> {
        servers(stdout)
    }

    /// Its servers are the person's (`~/.gemini/config/mcp_config.json`): its CLI writes no
    /// project's.
    fn server_scopes(&self) -> Choices {
        vec![("user".into(), "All my projects".into())]
    }

    /// `agy mcp add` (its switches before the name) and `agy mcp remove`.
    fn extension_op(&self, op: &ExtensionOp) -> Option<ProgramRun> {
        let args: Vec<String> = match op {
            ExtensionOp::AddServer(server) if server.client_id.is_none() => {
                let mut args: Vec<String> = vec!["mcp".into(), "add".into()];
                for header in server.headers.iter().filter(|_| server.remote) {
                    args.extend(["-H".into(), header.clone()]);
                }
                for pair in server.env.iter().filter(|_| !server.remote) {
                    args.extend(["-e".into(), pair.clone()]);
                }
                if server.remote {
                    args.extend(["-t".into(), "http".into()]);
                }
                args.extend([server.name.clone(), "--".into(), server.target.clone()]);
                args.extend(server.args.iter().cloned());
                args
            }
            ExtensionOp::RemoveServer(name) => vec!["mcp".into(), "remove".into(), name.clone()],
            _ => return None,
        };
        Some(ProgramRun {
            args,
            stdin: None,
            timeout: 30,
            until: None,
        })
    }

    // `session/new` takes stdio servers (ACP's baseline; it offers neither HTTP nor SSE).
    fn session_extras(&self) -> SessionExtras {
        SessionExtras {
            servers: true,
            sse: false,
            env_by_name: false,
            skills: false,
            host: true,
            inline_secrets: true,
        }
    }

    fn modes(&self) -> Vec<ModeWords> {
        let mode = |id: &str, label: &str, about: &str, tone, listed| ModeWords {
            id: id.into(),
            label: label.into(),
            about: about.into(),
            tone,
            listed,
        };
        vec![
            mode(
                DEFAULT,
                "Default",
                "Asks before edits and commands, as your permission settings say",
                ModeTone::Ask,
                true,
            ),
            mode(
                EDITS,
                "Accept edits",
                "Edits files in the workspace without asking",
                ModeTone::Edits,
                true,
            ),
            mode(
                PLAN,
                "Plan",
                "Plans carefully before it changes anything",
                ModeTone::Plan,
                true,
            ),
            mode(
                SKIP,
                "Skip permissions",
                "Never asks",
                ModeTone::Unsafe,
                false,
            ),
        ]
    }

    fn default_mode(&self) -> &'static str {
        DEFAULT
    }

    fn encode(&mut self, command: AgentCommand) -> Vec<String> {
        self.acp.encode(command)
    }

    /// Launched skipping every approval, ACP's `default` is that mode.
    fn decode(&mut self, line: &str) -> Decoded {
        let mut decoded = self.acp.decode(line);
        if self.skipping.load(Ordering::Relaxed) {
            for event in &mut decoded.events {
                if let AgentEvent::ModeChanged { mode }
                | AgentEvent::Ready {
                    mode: Some(mode), ..
                } = event
                {
                    if mode == DEFAULT {
                        *mode = SKIP.into();
                    }
                }
            }
        }
        decoded
    }
}
