//! Newen's adapter for xAI's Grok Build (docs/grok.md): `grok agent stdio`, the Agent Client
//! Protocol over stdio, which `newen-agent-acp` speaks. What is Grok's own lives here: its
//! program, its modes (which it does not list over ACP), its questions and plans (requests of
//! its own), how its tools read as Newen's, and its connectors.

use newen_agent::{
    AgentAdapter, AgentCommand, AgentId, Choices, ConfiguredServer, Decision, Decoded, ExtensionOp,
    LaunchPlan, McpSource, McpStatus, ModeTone, ModeWords, ProgramRun, SessionExtras, SessionSpec,
};
use newen_agent_acp::{by_kind, mcp_name, Acp, Call, Flavor};
use serde_json::{json, Map, Value};

#[cfg(test)]
mod tests;

const DEFAULT: &str = "default";
const AUTO: &str = "auto";
const PLAN: &str = "plan";
const ALWAYS: &str = "always-approve";

/// Its requests that wait on the person (docs/grok.md "Preguntas y planes").
const ASK_QUESTION: &str = "_x.ai/ask_user_question";
const EXIT_PLAN: &str = "_x.ai/exit_plan_mode";
/// The notification that turns always-approve and auto on and off for the process's sessions.
const PERMISSION_MODE: &str = "_x.ai/yolo_mode_changed";

/// How Grok's tools read as Newen's (docs/grok.md "Herramientas"): its `rawInput` names its
/// tool in `variant`.
struct Tools;

impl Flavor for Tools {
    fn tool(&self, call: &Call, servers: &[String]) -> Option<(String, Value)> {
        let input = &call.input;
        let said = |key: &str| input[key].as_str().map(str::to_owned);
        // Its first report names the tool in its title; the next ones in `variant`.
        let tool = input["variant"].as_str().unwrap_or(&call.title);
        let (name, given) = match tool {
            // A new file is an edit of nothing.
            "SearchReplace" | "search_replace"
                if said("old_string").unwrap_or_default().is_empty() =>
            {
                (
                    "Write",
                    json!({ "file_path": said("file_path"), "content": said("new_string") }),
                )
            }
            "Bash" | "run_terminal_command" => (
                "Bash",
                json!({ "command": said("command"), "description": said("description") }),
            ),
            "ReadFile" | "read_file" => (
                "Read",
                json!({ "file_path": said("target_file").unwrap_or_else(|| call.file()) }),
            ),
            "SearchReplace" | "search_replace" => (
                "Edit",
                json!({ "file_path": said("file_path"), "old_string": said("old_string"),
                    "new_string": said("new_string") }),
            ),
            "Write" | "write" => (
                "Write",
                json!({ "file_path": said("file_path"), "content": said("content") }),
            ),
            "Grep" | "grep" => (
                "Grep",
                json!({ "pattern": said("pattern"), "path": said("path"), "glob": said("glob") }),
            ),
            "ListDir" | "list_dir" => ("LS", json!({ "path": said("target_directory") })),
            "WebSearch" | "web_search" => ("WebSearch", json!({ "query": said("query") })),
            "WebFetch" | "web_fetch" => ("WebFetch", json!({ "url": said("url") })),
            "Task" | "spawn_subagent" => (
                "Agent",
                json!({ "description": said("description").unwrap_or_else(|| call.title.clone()),
                    "prompt": said("prompt") }),
            ),
            "SearchTool" | "search_tool" => ("ToolSearch", json!({ "query": said("query") })),
            // An MCP server's tool, called through its `use_tool` as `<server>__<tool>`.
            "UseTool" | "use_tool" => {
                let named = said("tool_name").unwrap_or_else(|| call.title.clone());
                let (server, tool) = mcp(&named, servers);
                return Some((mcp_name(&server, &tool), input["tool_input"].clone()));
            }
            // Its task list comes whole as ACP's plan, which draws it; its questions and plans
            // are cards of their own ([`Flavor::ask`]).
            "TodoWrite" | "todo_write" | "AskUserQuestion" | "ask_user_question"
            | "ExitPlanMode" | "exit_plan_mode" => return None,
            _ => return Some(by_kind(call)),
        };
        Some((name.into(), given))
    }

    /// It names no `agentInfo`: its version is in `_meta`.
    fn version(&self, initialized: &Value) -> String {
        initialized["_meta"]["agentVersion"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    }

    /// Its notifications of its own (`_x.ai/…`: setup phases, servers, progress) carry nothing
    /// Newen shows.
    fn quiet(&self, method: &str) -> bool {
        method.starts_with("_x.ai/")
    }

    fn unlisted_modes(&self) -> &'static [&'static str] {
        &[DEFAULT, AUTO, PLAN, ALWAYS]
    }

    fn unsafe_modes(&self) -> &'static [&'static str] {
        &[ALWAYS]
    }

    /// What the model was told the call gave: a command's `output_for_prompt`, an edit's
    /// `tool_output_for_prompt`, a task list's `summary_for_prompt`, a listing's `content`, at
    /// the top of `rawOutput` or under its one tag (`{"EditsApplied": {…}}`).
    fn output(&self, raw: &Value) -> Option<String> {
        const KEYS: [&str; 4] = [
            "output_for_prompt",
            "tool_output_for_prompt",
            "summary_for_prompt",
            "content",
        ];
        let tagged = raw.as_object().into_iter().flat_map(|o| o.values());
        (std::iter::once(raw).chain(tagged))
            .find_map(|level| KEYS.iter().find_map(|key| level[*key].as_str()))
            .map(str::to_owned)
    }

    /// `session/set_mode` takes `default` and `plan`; always-approve and auto are the
    /// permission mode of its own notification (docs/grok.md "Modos").
    fn mode_switch(&self, mode: &str) -> (String, Vec<(&'static str, Value)>) {
        let note = match mode {
            AUTO => json!({ "permission_mode": "auto" }),
            ALWAYS => json!({ "yolo_mode": true }),
            PLAN => return (PLAN.into(), vec![]),
            _ => json!({ "yolo_mode": false, "permission_mode": "default" }),
        };
        (DEFAULT.into(), vec![(PERMISSION_MODE, note)])
    }

    fn ask(&self, method: &str, params: &Value) -> Option<(String, Value)> {
        match method {
            ASK_QUESTION => {
                let questions: Vec<Value> = (params["questions"].as_array().into_iter())
                    .flatten()
                    .map(|q| {
                        let options: Vec<Value> = (q["options"].as_array().into_iter().flatten())
                            .map(
                                |o| json!({ "label": o["label"], "description": o["description"] }),
                            )
                            .collect();
                        json!({ "question": q["question"], "header": "", "options": options,
                            "multiSelect": q["multiSelect"] == true, "other": true })
                    })
                    .collect();
                Some((
                    "AskUserQuestion".into(),
                    json!({ "questions": questions, "blocking": true }),
                ))
            }
            EXIT_PLAN => Some((
                "ExitPlanMode".into(),
                json!({ "plan": params["planContent"].as_str().unwrap_or_default() }),
            )),
            _ => None,
        }
    }

    fn answer(&self, method: &str, params: &Value, decision: Option<&Decision>) -> Value {
        match (method, decision) {
            (ASK_QUESTION, Some(Decision::Answer(given))) => answers(params, given),
            (EXIT_PLAN, Some(Decision::Allow | Decision::AllowAlways(_) | Decision::Answer(_))) => {
                json!({ "outcome": "approved" })
            }
            (EXIT_PLAN, Some(Decision::Deny { message })) if !message.trim().is_empty() => {
                json!({ "outcome": "cancelled", "feedback": message })
            }
            _ => json!({ "outcome": "cancelled" }),
        }
    }
}

/// The card's answers (each question's words, several choices joined by ", ") as Grok takes
/// them: each answered question's chosen labels, and words of the person's own as "Other" with
/// those words for a note. A question left alone is left out.
fn answers(params: &Value, given: &Value) -> Value {
    let (mut chosen, mut notes) = (Map::new(), Map::new());
    for q in params["questions"].as_array().into_iter().flatten() {
        let question = q["question"].as_str().unwrap_or_default();
        let said = given[question].as_str().unwrap_or_default().trim();
        if said.is_empty() || said == "[No preference]" {
            continue;
        }
        let labels: Vec<&str> = (q["options"].as_array().into_iter().flatten())
            .filter_map(|o| o["label"].as_str())
            .collect();
        let picked: Vec<&str> = match q["multiSelect"] == true {
            true => said.split(", ").collect(),
            false => vec![said],
        };
        if picked.iter().all(|p| labels.contains(p)) {
            chosen.insert(question.into(), json!(picked));
        } else {
            chosen.insert(question.into(), json!(["Other"]));
            notes.insert(question.into(), json!({ "notes": said }));
        }
    }
    let mut answer = json!({ "outcome": "accepted", "answers": chosen });
    if !notes.is_empty() {
        answer["annotations"] = Value::Object(notes);
    }
    answer
}

/// The server and tool of `<server>__<tool>`: the longest of the session's servers it starts
/// with, else its first `__`.
fn mcp(named: &str, servers: &[String]) -> (String, String) {
    let given = (servers.iter())
        .filter(|s| named.starts_with(&format!("{s}__")))
        .max_by_key(|s| s.len());
    match given {
        Some(server) => (server.clone(), named[server.len() + 2..].to_owned()),
        None => match named.split_once("__") {
            Some((server, tool)) => (server.into(), tool.into()),
            None => (String::new(), named.into()),
        },
    }
}

/// The servers of `grok mcp list --json`: `{name, scope, enabled, command, args}` or
/// `{name, scope, enabled, url}`.
fn servers(stdout: &str) -> Vec<ConfiguredServer> {
    let listed: Value = serde_json::from_str(stdout).unwrap_or_default();
    (listed.as_array().into_iter().flatten())
        .filter_map(|server| {
            let name = server["name"].as_str()?.to_owned();
            let target = match server["url"].as_str() {
                Some(url) => url.to_owned(),
                None => (std::iter::once(&server["command"]))
                    .chain(server["args"].as_array().into_iter().flatten())
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
            };
            Some(ConfiguredServer {
                label: name.clone(),
                name,
                target,
                status: match server["enabled"] == false {
                    true => McpStatus::Disabled,
                    false => McpStatus::Connected,
                },
                // Read from its config: set up, not connected to.
                checked: false,
                source: match server["scope"].as_str() {
                    Some("project") => McpSource::Project,
                    _ => McpSource::User,
                },
            })
        })
        .collect()
}

/// `grok` from `PATH`, else where its installer puts it, `~/.grok/bin`, which it adds to the
/// `PATH` of bash and zsh only.
fn program() -> String {
    let on_path = std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("grok").is_file()));
    let installed = std::env::var_os("HOME")
        .map(|home| std::path::Path::new(&home).join(".grok/bin/grok"))
        .filter(|path| path.is_file());
    match (on_path, installed) {
        (false, Some(path)) => path.display().to_string(),
        _ => "grok".into(),
    }
}

static TOOLS: Tools = Tools;

pub struct Grok(Acp);

impl Grok {
    pub fn new() -> Self {
        Self(Acp::new(&TOOLS))
    }
}

impl Default for Grok {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAdapter for Grok {
    fn id(&self) -> AgentId {
        "grok"
    }

    fn name(&self) -> &'static str {
        "Grok"
    }

    fn tested(&self) -> &'static [&'static str] {
        &["1.0.77"]
    }

    fn launch(&self, spec: &SessionSpec) -> LaunchPlan {
        self.0.remember(spec);
        LaunchPlan {
            program: program(),
            args: vec!["agent".into(), "stdio".into()],
            env: vec![],
            env_remove: vec![],
            cwd: spec.cwd.clone(),
        }
    }

    /// Always-approve and auto start a dev server without a card, as bypass and auto do.
    fn mode_waives_prompts(&self, mode: &str) -> bool {
        matches!(mode, ALWAYS | AUTO)
    }

    /// Its list reads its config and starts no server.
    fn servers_query(&self) -> Option<Vec<String>> {
        Some(["mcp", "list", "--json"].map(String::from).into())
    }

    fn servers(&self, stdout: &str) -> Vec<ConfiguredServer> {
        servers(stdout)
    }

    fn server_scopes(&self) -> Choices {
        vec![
            ("user".into(), "All my projects".into()),
            ("project".into(), "This project".into()),
        ]
    }

    /// `grok mcp add` and `grok mcp remove`, in the scope chosen.
    fn extension_op(&self, op: &ExtensionOp) -> Option<ProgramRun> {
        let args: Vec<String> = match op {
            ExtensionOp::AddServer(server) if server.client_id.is_none() => {
                let mut args: Vec<String> = vec!["mcp".into(), "add".into()];
                if let Some(scope) = &server.scope {
                    args.extend(["-s".into(), scope.clone()]);
                }
                if server.remote {
                    args.extend(["-t".into(), "http".into()]);
                    for header in &server.headers {
                        args.extend(["-H".into(), header.clone()]);
                    }
                    args.extend([server.name.clone(), server.target.clone()]);
                } else {
                    for pair in &server.env {
                        args.extend(["-e".into(), pair.clone()]);
                    }
                    args.extend([server.name.clone(), "--".into(), server.target.clone()]);
                    args.extend(server.args.iter().cloned());
                }
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

    // `session/new` takes stdio, HTTP and SSE servers; they run in Grok's own process, with its
    // environment.
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
                "Ask",
                "Asks before edits and commands",
                ModeTone::Ask,
                true,
            ),
            mode(
                AUTO,
                "Auto",
                "A safety check decides what runs; it asks or blocks the rest",
                ModeTone::Auto,
                true,
            ),
            mode(
                PLAN,
                "Plan",
                "Explores read-only and proposes a plan",
                ModeTone::Plan,
                true,
            ),
            mode(
                ALWAYS,
                "Always approve",
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
        self.0.encode(command)
    }

    fn decode(&mut self, line: &str) -> Decoded {
        self.0.decode(line)
    }
}
