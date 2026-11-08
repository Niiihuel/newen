//! The adapter core for agent CLIs that speak the Agent Client Protocol (ACP,
//! agentclientprotocol.com): JSON-RPC 2.0 over stdio, one message a line (docs/acp.md).
//!
//! It names no agent. Each agent's crate (`newen-agent-<id>`) wraps [`Acp`] in its own
//! `AgentAdapter`: its identity, its program, its account and extensions, and, through
//! [`Flavor`], how its tools read as the ones Newen draws. Pure, like every adapter: the
//! engine runs the process and passes each line through [`Acp::decode`].

use base64::Engine as _;
use newen_agent::{
    AgentCapabilities, AgentCommand, AgentEvent, AgentSlashCommand, CommandRun, Decision, Decoded,
    FileChange, InjectedServer, ModelInfo, PromptImage, SessionSpec, Suggestion, ToolContent,
    Usage, BROWSER_TOOLS, HOST_SERVER, HOST_TOOLS,
};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub mod replay;
#[cfg(test)]
mod tests;

/// What one agent says in its own way over ACP.
pub trait Flavor: Sync {
    /// One of the agent's tool calls as Newen draws it: a tool Newen knows (`Bash`, `Read`,
    /// `Edit`…) with its input in that tool's shape, or `None` to show nothing of it (another
    /// update draws it already). `servers` are the session's MCP servers, by name.
    fn tool(&self, call: &Call, servers: &[String]) -> Option<(String, Value)> {
        let _ = servers;
        Some(by_kind(call))
    }
    /// More of Newen's client capabilities, as `initialize`'s `clientCapabilities._meta`.
    fn client_meta(&self) -> Value {
        json!({})
    }
    /// Notifications of the agent's own that carry nothing Newen shows.
    fn quiet(&self, _method: &str) -> bool {
        false
    }
    /// The agent may send a message's words again whole, with what it adds, under the same id:
    /// such a chunk adds only what is new.
    fn resends_words(&self) -> bool {
        false
    }
    /// The agent's version, from what `initialize` answered: ACP's `agentInfo.version`.
    fn version(&self, initialized: &Value) -> String {
        text(&initialized["agentInfo"]["version"])
    }

    /// The modes the agent takes though it lists none, by Newen's ids for them; a session starts
    /// in the first.
    fn unlisted_modes(&self) -> &'static [&'static str] {
        &[]
    }

    /// The words a finished call gave, from its `rawOutput`, when its content has none.
    fn output(&self, _raw: &Value) -> Option<String> {
        None
    }

    /// Its modes that skip every approval, which the mode menu offers only once Settings allow
    /// them (`AgentCapabilities::unsafe_modes`).
    fn unsafe_modes(&self) -> &'static [&'static str] {
        &[]
    }

    /// What moves the session to one of its unlisted modes: the `session/set_mode` id that mode
    /// is, and notifications of the agent's own sent before it (method, and params that get the
    /// `sessionId`).
    fn mode_switch(&self, mode: &str) -> (String, Vec<(&'static str, Value)>) {
        (mode.to_owned(), vec![])
    }

    /// A request of the agent's own that waits on the person, a question or a plan to accept:
    /// the tool whose card it is (`AskUserQuestion`, `ExitPlanMode`) and that tool's input.
    /// `None` for one Newen does not answer.
    fn ask(&self, _method: &str, _params: &Value) -> Option<(String, Value)> {
        None
    }

    /// What answers such a request: the person's `decision`, or `None` when the turn stopped.
    fn answer(&self, _method: &str, _params: &Value, _decision: Option<&Decision>) -> Value {
        json!({})
    }

    /// The provider a model of the session's list comes through and the model's own name, when
    /// its name says both; `None` keeps the name whole.
    fn provider(&self, name: &str) -> Option<(String, String)> {
        let _ = name;
        None
    }
}

/// A tool call as the agent has reported it so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Call {
    pub id: String,
    /// The agent's own name for the tool when it says (ACP's `name`), else empty.
    pub name: String,
    /// ACP's coarse kind: `read`, `edit`, `delete`, `move`, `search`, `execute`, `think`,
    /// `fetch`, `switch_mode`, `other`.
    pub kind: String,
    pub title: String,
    /// What the tool was given (`rawInput`), `Null` when the agent did not say.
    pub input: Value,
    /// The first file it works on (`locations`).
    pub path: Option<String>,
    /// Its text content so far.
    pub text: String,
    /// The first file change it shows: the file, its words before (none for a new file) and after.
    pub diff: Option<(String, Option<String>, String)>,
}

impl Call {
    /// Its input's string at the first of `keys` that has one.
    pub fn said(&self, keys: &[&str]) -> Option<String> {
        keys.iter()
            .find_map(|k| self.input[*k].as_str())
            .map(str::to_owned)
    }

    /// The file it works on: its input's, else the first location, else the diff's.
    pub fn file(&self) -> String {
        self.said(&["file_path", "filePath", "path"])
            .or(self.path.clone())
            .or(self.diff.as_ref().map(|d| d.0.clone()))
            .unwrap_or_default()
    }

    /// The text after the tool's name in a title like "terminal: ls -la".
    pub fn preview(&self) -> &str {
        self.title.split_once(": ").map_or("", |(_, rest)| rest)
    }
}

/// A call drawn as the Newen tool its ACP kind is closest to.
pub fn by_kind(call: &Call) -> (String, Value) {
    let file = call.file();
    let (name, input) = match call.kind.as_str() {
        "execute" => {
            let command = call.said(&["command", "cmd"]).unwrap_or_else(|| {
                let preview = call.preview();
                if preview.is_empty() {
                    &call.title
                } else {
                    preview
                }
                .to_owned()
            });
            let mut input = json!({ "command": command });
            if let Some(about) = call.said(&["description"]) {
                input["description"] = json!(about);
            }
            ("Bash", input)
        }
        "read" => ("Read", json!({ "file_path": file })),
        "edit" => match (&call.diff, call.said(&["content"])) {
            (Some((_, None, after)), _) => {
                ("Write", json!({ "file_path": file, "content": after }))
            }
            (Some((_, Some(before), after)), _) => (
                "Edit",
                json!({ "file_path": file, "old_string": before, "new_string": after }),
            ),
            (None, Some(content)) => ("Write", json!({ "file_path": file, "content": content })),
            (None, None) => {
                let old = call.said(&["old_string", "oldString"]).unwrap_or_default();
                let new = call.said(&["new_string", "newString"]).unwrap_or_default();
                (
                    "Edit",
                    json!({ "file_path": file, "old_string": old, "new_string": new }),
                )
            }
        },
        "search" => {
            let pattern = call
                .said(&["pattern", "query"])
                .unwrap_or_else(|| call.preview().to_owned());
            (
                "Grep",
                json!({ "pattern": pattern, "path": call.said(&["path"]) }),
            )
        }
        "fetch" => match call.said(&["url"]) {
            Some(url) => ("WebFetch", json!({ "url": url })),
            None => {
                let query = call
                    .said(&["query"])
                    .unwrap_or_else(|| call.preview().to_owned());
                ("WebSearch", json!({ "query": query }))
            }
        },
        "think" => {
            let about = call
                .said(&["description"])
                .unwrap_or_else(|| call.title.clone());
            (
                "Agent",
                json!({ "description": about, "prompt": call.said(&["prompt"]) }),
            )
        }
        _ => {
            let name = if call.name.is_empty() {
                &call.title
            } else {
                &call.name
            };
            return (name.clone(), call.input.clone());
        }
    };
    (name.into(), input)
}

/// A tool of an MCP server as Newen names it (`mcp__<server>__<tool>`), which draws Newen's own
/// tools and the browser's as its own.
pub fn mcp_name(server: &str, tool: &str) -> String {
    format!("mcp__{server}__{tool}")
}

/// Whether a tool, by Newen's name, is one of Newen's own: those never ask the person.
fn ours(name: &str) -> bool {
    let tool = |server: &str, tools: &[(&str, &str, &str)]| {
        name.strip_prefix(&format!("mcp__{server}__"))
            .is_some_and(|t| tools.iter().any(|(n, ..)| *n == t))
    };
    tool(HOST_SERVER, &HOST_TOOLS)
        || tool("browser", &BROWSER_TOOLS)
        || tool(newen_agent::TERMINAL_SERVER, &newen_agent::TERMINAL_TOOLS)
}

/// What a request Newen sent waits to hear.
#[derive(Clone, Debug, PartialEq)]
enum Pending {
    Init,
    /// The session opening: `session/new`, `load`, `resume` or `fork`.
    Open,
    /// A mode, model or effort set: the mode when it was one.
    Set(Option<String>),
    /// The running turn, for Newen's request.
    Prompt(String),
}

/// Something the agent waits for the person to answer.
enum Ask {
    /// `session/request_permission`: its JSON-RPC id and its options, by kind.
    Permission(Value, Vec<(String, String)>),
    /// `elicitation/create` (a form of questions): its id and its fields.
    Form(Value, Vec<Field>),
    /// A request of the agent's own ([`Flavor::ask`]): its id, method and params.
    Own(Value, String, Value),
}

/// One question of a form: its key, how the card asks it, and its choices as (label, value).
struct Field {
    key: String,
    question: String,
    choices: Vec<(String, Value)>,
    multi: bool,
    /// The form's field for an answer in the person's own words, when it has one.
    custom: Option<String>,
}

/// A message being streamed: its id and its words so far.
#[derive(Default)]
struct Stream {
    id: Option<String>,
    text: String,
    open: bool,
}

/// One session of an ACP agent, over one process.
pub struct Acp {
    flavor: &'static dyn Flavor,
    /// What `launch` was given: the session is opened after `initialize`, not on the command line.
    spec: Mutex<SessionSpec>,
    next: u64,
    pending: HashMap<String, Pending>,
    version: String,
    resumes: bool,
    loads: bool,
    forks: bool,
    session: Option<String>,
    /// The session is opening: what a load replays is Newen's already.
    opening: bool,
    /// `configOptions`, as the agent last listed them.
    options: Vec<Value>,
    /// The modes by id and name, the current one, and the option that sets them when only a
    /// config option offers them.
    modes: Vec<String>,
    mode: String,
    mode_option: Option<String>,
    /// The models from `models`, when no config option offers them, and the current one.
    models: Vec<(String, String)>,
    model: String,
    commands: Vec<AgentSlashCommand>,
    /// Prompts sent before the session could take them: (request, blocks).
    queued: Vec<(String, Vec<Value>)>,
    /// Mode, model and effort changes asked while a turn ran, for when it ends.
    held: Vec<AgentCommand>,
    /// The turn running: its request id and Newen's, and whether Newen was told it runs.
    turn: Option<(String, String)>,
    announced: bool,
    interrupted: bool,
    calls: HashMap<String, Call>,
    /// The calls the agent announced (`tool_call`): an update of any other id closes a card of its
    /// own, not a tool.
    announced_calls: HashSet<String>,
    /// The calls drawn so far, with what they were drawn as (`None`: not shown).
    shown: HashMap<String, Option<(String, Value)>>,
    text: Stream,
    thought: Stream,
    asks: HashMap<String, Ask>,
    /// Asks the person can no longer answer (the turn was stopped), to take off at its end.
    withdrawn: Vec<String>,
    plans: u64,
    used: u64,
    size: u64,
    cost: Option<f64>,
    cost_before: Option<f64>,
}

impl Acp {
    pub fn new(flavor: &'static dyn Flavor) -> Self {
        Self {
            flavor,
            spec: Mutex::default(),
            next: 0,
            pending: HashMap::new(),
            version: String::new(),
            resumes: false,
            loads: false,
            forks: false,
            session: None,
            opening: false,
            options: vec![],
            modes: vec![],
            mode: String::new(),
            mode_option: None,
            models: vec![],
            model: String::new(),
            commands: vec![],
            queued: vec![],
            held: vec![],
            turn: None,
            announced: false,
            interrupted: false,
            calls: HashMap::new(),
            announced_calls: HashSet::new(),
            shown: HashMap::new(),
            text: Stream::default(),
            thought: Stream::default(),
            asks: HashMap::new(),
            withdrawn: vec![],
            plans: 0,
            used: 0,
            size: 0,
            cost: None,
            cost_before: None,
        }
    }

    /// Keep what the session asks for: the engine calls `launch` before `Start`.
    pub fn remember(&self, spec: &SessionSpec) {
        *self.spec.lock().unwrap() = spec.clone();
    }

    /// The agent's version, as its `initialize` said.
    pub fn version(&self) -> &str {
        &self.version
    }

    fn request(&mut self, method: &str, params: Value, pending: Pending) -> String {
        self.next += 1;
        self.pending.insert(self.next.to_string(), pending);
        json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params }).to_string()
    }

    fn servers(&self) -> Vec<String> {
        let spec = self.spec.lock().unwrap();
        spec.servers.iter().map(|s| s.name.clone()).collect()
    }

    /// `session/new`, or the spec's session continued, forked or loaded.
    fn open(&mut self) -> String {
        let spec = self.spec.lock().unwrap().clone();
        let mut params = json!({
            "cwd": spec.cwd.to_string_lossy(),
            "mcpServers": spec.servers.iter().map(mcp_server).collect::<Vec<_>>(),
        });
        self.opening = true;
        let Some(id) = spec.resume else {
            return self.request("session/new", params, Pending::Open);
        };
        params["sessionId"] = json!(id);
        // shortcut: a fork copies the whole conversation (`resume_at` has no ACP counterpart);
        // a rewind keeps what came after it in the agent's memory until ACP can cut a session.
        let method = match (spec.fork && self.forks, self.resumes, self.loads) {
            (true, ..) => "session/fork",
            (_, true, _) => "session/resume",
            (_, _, true) => "session/load",
            _ => {
                params.as_object_mut().map(|p| p.remove("sessionId"));
                "session/new"
            }
        };
        self.request(method, params, Pending::Open)
    }

    fn option(&self, category: &str) -> Option<&Value> {
        self.options.iter().find(|o| o["category"] == category)
    }

    /// The values of a select option, each with its name.
    fn choices(option: &Value) -> Vec<(String, String)> {
        // A select's options may come in groups.
        let mut out = vec![];
        for entry in option["options"].as_array().into_iter().flatten() {
            match entry["options"].as_array() {
                Some(group) => {
                    out.extend(group.iter().map(|o| (text(&o["value"]), text(&o["name"]))))
                }
                None => out.push((text(&entry["value"]), text(&entry["name"]))),
            }
        }
        out
    }

    /// The session's options, models and modes as the agent says them now; whether the mode
    /// changed.
    fn take_options(&mut self, result: &Value) -> bool {
        if let Some(options) = result["configOptions"].as_array() {
            self.options = options.clone();
        }
        if let Some(models) = result["models"].as_object() {
            self.model = text(&models["currentModelId"]);
            self.models = (models["availableModels"].as_array().into_iter().flatten())
                .map(|m| (text(&m["modelId"]), text(&m["name"])))
                .collect();
        }
        let before = self.mode.clone();
        if let Some(modes) = result["modes"].as_object() {
            self.mode_option = None;
            self.mode = text(&modes["currentModeId"]);
            self.modes = (modes["availableModes"].as_array().into_iter().flatten())
                .map(|m| text(&m["id"]))
                .collect();
        } else if self.mode_option.is_some() || self.modes.is_empty() {
            if let Some(option) = self.option("mode").cloned() {
                self.mode_option = Some(text(&option["id"]));
                self.mode = text(&option["currentValue"]);
                self.modes = Self::choices(&option).into_iter().map(|(v, _)| v).collect();
            } else if let Some(first) = self.flavor.unlisted_modes().first() {
                self.modes = (self.flavor.unlisted_modes().iter())
                    .map(|m| (*m).to_owned())
                    .collect();
                self.mode = (*first).to_owned();
            }
        }
        before != self.mode && !before.is_empty()
    }

    fn ready(&self) -> AgentEvent {
        let efforts: Vec<String> = (self.option("thought_level").map(Self::choices))
            .unwrap_or_default()
            .into_iter()
            .map(|(value, _)| value)
            .filter(|value| value != "default")
            .collect();
        let (models, current) = match self.option("model") {
            Some(option) => (Self::choices(option), text(&option["currentValue"])),
            None => (self.models.clone(), self.model.clone()),
        };
        // The efforts are the current model's: another's are told once it is chosen.
        let models = models
            .into_iter()
            .map(|(id, name)| {
                let name = if name.is_empty() { id.clone() } else { name };
                let (provider, name) = match self.flavor.provider(&name) {
                    Some((provider, own)) => (Some(provider), own),
                    None => (None, name),
                };
                ModelInfo {
                    efforts: if id == current {
                        efforts.clone()
                    } else {
                        vec![]
                    },
                    default: id == current,
                    name,
                    id,
                    modes: vec![],
                    provider,
                }
            })
            .collect();
        AgentEvent::Ready {
            version: self.version.clone(),
            capabilities: AgentCapabilities {
                models,
                commands: self.commands.clone(),
                modes: (self.modes.iter())
                    .filter(|m| !self.flavor.unsafe_modes().contains(&m.as_str()))
                    .cloned()
                    .collect(),
                unsafe_modes: (self.flavor.unsafe_modes().iter())
                    .map(|m| (*m).to_owned())
                    .collect(),
                interrupt: true,
                resume: self.resumes || self.loads,
                fork: self.forks,
                thinking_text: true,
                ..Default::default()
            },
            mode: Some(self.mode.clone()).filter(|m| !m.is_empty()),
        }
    }

    /// The lines that set a mode, model or effort, none when the agent offers no such thing or
    /// is there already.
    fn set(&mut self, command: &AgentCommand) -> Vec<String> {
        self.set_lines(command).unwrap_or_default()
    }

    fn set_lines(&mut self, command: &AgentCommand) -> Option<Vec<String>> {
        let session = self.session.clone()?;
        let (category, value) = match command {
            AgentCommand::SetMode(mode) => ("mode", mode),
            AgentCommand::SetModel(model) => ("model", model),
            AgentCommand::SetEffort(effort) => ("thought_level", effort),
            _ => return None,
        };
        let mode = (category == "mode").then(|| value.clone());
        if category == "mode" && (self.mode == *value || !self.modes.contains(value)) {
            return None;
        }
        if let Some(option) = self.option(category) {
            if option["currentValue"] == *value {
                return None;
            }
            // The modes `modes` lists are set by their own method (their ids are not the option's values).
            if category != "mode" || self.mode_option.is_some() {
                let params =
                    json!({ "sessionId": session, "configId": option["id"], "value": value });
                let line = self.request("session/set_config_option", params, Pending::Set(mode));
                return Some(vec![line]);
            }
        }
        match category {
            "mode" => {
                // A mode the agent does not list: its own notifications first, then its set_mode.
                let (id, notes) = match self.flavor.unlisted_modes().contains(&value.as_str()) {
                    true => self.flavor.mode_switch(value),
                    false => (value.clone(), vec![]),
                };
                let mut lines: Vec<String> = (notes.into_iter())
                    .map(|(method, mut params)| {
                        params["sessionId"] = json!(session);
                        json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string()
                    })
                    .collect();
                let params = json!({ "sessionId": session, "modeId": id });
                lines.push(self.request("session/set_mode", params, Pending::Set(mode)));
                Some(lines)
            }
            "model" if !self.models.is_empty() && self.model != *value => Some(vec![self.request(
                "session/set_model",
                json!({ "sessionId": session, "modelId": value }),
                Pending::Set(None),
            )]),
            _ => None,
        }
    }

    fn settling(&self) -> bool {
        self.pending.values().any(|p| matches!(p, Pending::Set(_)))
    }

    /// The next prompt waiting, once the session takes one.
    fn next_prompt(&mut self) -> Option<String> {
        if self.session.is_none()
            || self.turn.is_some()
            || self.settling()
            || self.queued.is_empty()
        {
            return None;
        }
        let (request, blocks) = self.queued.remove(0);
        let params = json!({ "sessionId": self.session, "prompt": blocks });
        let line = self.request("session/prompt", params, Pending::Prompt(request.clone()));
        self.turn = Some((self.next.to_string(), request));
        self.announced = false;
        self.interrupted = false;
        Some(line)
    }

    fn response(&mut self, id: &Value, msg: &Value, out: &mut Decoded) {
        let Some(pending) = self.pending.remove(&key(id)) else {
            return out.events.push(AgentEvent::Unknown(msg.clone()));
        };
        let result = &msg["result"];
        let error = msg.get("error").map(|e| text(&e["message"]));
        match pending {
            Pending::Init => {
                if let Some(error) = error {
                    return self.failed(error, msg, out);
                }
                self.version = self.flavor.version(result);
                let caps = &result["agentCapabilities"];
                self.loads = caps["loadSession"] == true;
                self.resumes = !caps["sessionCapabilities"]["resume"].is_null();
                self.forks = !caps["sessionCapabilities"]["fork"].is_null();
                let open = self.open();
                out.replies.push(open);
            }
            Pending::Open => {
                self.opening = false;
                if let Some(error) = error {
                    return self.failed(error, msg, out);
                }
                let id = result["sessionId"]
                    .as_str()
                    .map(str::to_owned)
                    .or(self.spec.lock().unwrap().resume.clone())
                    .unwrap_or_default();
                self.session = Some(id.clone());
                self.take_options(result);
                out.events.push(AgentEvent::Session { id });
                out.events.push(self.ready());
                // What the person chose for the session, before its first prompt.
                let spec = self.spec.lock().unwrap().clone();
                let chosen = [
                    spec.mode.map(AgentCommand::SetMode),
                    spec.model.map(AgentCommand::SetModel),
                    spec.effort.map(AgentCommand::SetEffort),
                ];
                for command in chosen.into_iter().flatten() {
                    out.replies.extend(self.set(&command));
                }
                out.replies.extend(self.next_prompt());
            }
            Pending::Set(mode) => {
                match error {
                    None => {
                        let moved = self.take_options(result);
                        let asked = mode.is_some();
                        if let Some(mode) = mode.filter(|_| self.mode_option.is_none()) {
                            self.mode = mode;
                        }
                        if moved || asked {
                            out.events.push(AgentEvent::ModeChanged {
                                mode: self.mode.clone(),
                            });
                        }
                        out.events.push(self.ready());
                    }
                    Some(_) => out.events.push(AgentEvent::Unknown(msg.clone())),
                }
                out.replies.extend(self.next_prompt());
            }
            Pending::Prompt(_) => self.turn_ended(result, msg, out),
        }
    }

    /// A request the session needed failed: a turn waiting on it ends with the agent's words.
    fn failed(&mut self, error: String, msg: &Value, out: &mut Decoded) {
        out.events.push(AgentEvent::Unknown(msg.clone()));
        if is_auth(msg) {
            out.events.push(AgentEvent::AuthRequired);
        }
        if !self.queued.is_empty() {
            self.queued.clear();
            out.events
                .push(finished(false, false, Usage::default(), Some(error)));
        }
    }

    fn turn_ended(&mut self, result: &Value, msg: &Value, out: &mut Decoded) {
        self.announce(out);
        self.flush(out);
        for id in std::mem::take(&mut self.withdrawn) {
            out.events.push(AgentEvent::Withdrawn { id });
        }
        let reason = text(&result["stopReason"]);
        let error = msg.get("error").map(|e| text(&e["message"]));
        if is_auth(msg) {
            out.events.push(AgentEvent::AuthRequired);
        }
        let cost = self.cost.map(|now| now - self.cost_before.unwrap_or(0.0));
        self.cost_before = self.cost;
        // ACP's `usage`, or the same counts some agents give in the response's `_meta`.
        let counts = match result["usage"].is_object() {
            true => &result["usage"],
            false => &result["_meta"],
        };
        let usage = Usage {
            input_tokens: number(&counts["inputTokens"]),
            output_tokens: number(&counts["outputTokens"]),
            cost_usd: cost,
            context_tokens: self.used,
            context_window: self.size,
        };
        let stopped = reason == "cancelled" || (self.interrupted && error.is_some());
        let ok = error.is_none() && matches!(reason.as_str(), "end_turn" | "max_tokens");
        out.events.push(finished(ok, stopped, usage, error));
        self.turn = None;
        self.calls.clear();
        self.announced_calls.clear();
        self.asks.clear();
        for command in std::mem::take(&mut self.held) {
            out.replies.extend(self.set(&command));
        }
        out.replies.extend(self.next_prompt());
    }

    /// The message and the thought being streamed end.
    fn flush(&mut self, out: &mut Decoded) {
        let thought = std::mem::take(&mut self.thought);
        if thought.open && !thought.text.is_empty() {
            out.events.push(AgentEvent::ThinkingText {
                text: thought.text,
                parent: None,
            });
        }
        let said = std::mem::take(&mut self.text);
        if said.open && !said.text.is_empty() {
            out.events.push(AgentEvent::Text {
                text: said.text,
                parent: None,
                id: said.id,
                at: None,
            });
        }
    }

    fn announce(&mut self, out: &mut Decoded) {
        if let (Some((_, request)), false) = (&self.turn, self.announced) {
            out.events.push(AgentEvent::Running {
                requests: vec![request.clone()],
            });
            self.announced = true;
        }
    }

    fn update(&mut self, update: &Value, msg: &Value, out: &mut Decoded) {
        let kind = update["sessionUpdate"].as_str().unwrap_or_default();
        let replayed = matches!(
            kind,
            "user_message_chunk"
                | "agent_message_chunk"
                | "agent_thought_chunk"
                | "tool_call"
                | "tool_call_update"
                | "plan"
        );
        // A loaded session's history is Newen's already; another client's turn too.
        if replayed && (self.opening || self.turn.is_none()) {
            return;
        }
        if replayed {
            self.announce(out);
        }
        match kind {
            "agent_message_chunk" | "agent_thought_chunk" => {
                let mut words = text(&update["content"]["text"]);
                let id = update["messageId"].as_str().map(str::to_owned);
                let thinking = kind == "agent_thought_chunk";
                let stream = if thinking { &self.thought } else { &self.text };
                if self.flavor.resends_words()
                    && stream.open
                    && stream.id == id
                    && !stream.text.is_empty()
                    && words.starts_with(&stream.text)
                {
                    words = words[stream.text.len()..].to_owned();
                }
                // A new message (by its id), or a thought after words: the last one ends.
                if stream.open && id.is_some() && stream.id != id || (thinking && self.text.open) {
                    self.flush(out);
                }
                if !thinking && self.thought.open {
                    let thought = std::mem::take(&mut self.thought);
                    if !thought.text.is_empty() {
                        out.events.push(AgentEvent::ThinkingText {
                            text: thought.text,
                            parent: None,
                        });
                    }
                }
                let stream = if thinking {
                    &mut self.thought
                } else {
                    &mut self.text
                };
                if thinking && !stream.open {
                    out.events.push(AgentEvent::Thinking { parent: None });
                }
                stream.open = true;
                stream.id = id;
                stream.text.push_str(&words);
                out.events.push(if thinking {
                    AgentEvent::ThinkingDelta {
                        text: words,
                        parent: None,
                    }
                } else {
                    AgentEvent::TextDelta {
                        text: words,
                        parent: None,
                    }
                });
            }
            "tool_call" | "tool_call_update" => {
                self.flush(out);
                self.tool(update, out);
            }
            // The whole list each time; the task list is drawn from TodoWrite.
            "plan" => {
                self.plans += 1;
                let call = format!("plan-{}", self.plans);
                let todos: Vec<Value> = (update["entries"].as_array().into_iter().flatten())
                    .map(|e| json!({ "content": e["content"], "activeForm": e["content"], "status": e["status"] }))
                    .collect();
                out.events.push(AgentEvent::ToolUse {
                    call: call.clone(),
                    name: "TodoWrite".into(),
                    input: json!({ "todos": todos }),
                    parent: None,
                });
                out.events
                    .push(result(call, String::new(), false, vec![], None));
            }
            "available_commands_update" => {
                let mut commands: Vec<AgentSlashCommand> =
                    (update["availableCommands"].as_array().into_iter().flatten())
                        .map(|c| AgentSlashCommand {
                            name: text(&c["name"]),
                            description: text(&c["description"]),
                            argument_hint: text(&c["input"]["hint"]),
                            aliases: vec![],
                        })
                        .collect();
                commands.sort_by(|a, b| a.name.cmp(&b.name));
                self.commands = commands;
                if self.session.is_some() && !self.opening {
                    out.events.push(self.ready());
                }
            }
            "current_mode_update" => {
                let said = text(&update["currentModeId"]);
                // Unlisted modes: the agent says the `session/set_mode` id, which several of
                // Newen's modes may share; the one the session is in stays.
                let unlisted = self.flavor.unlisted_modes();
                let same = |mode: &str| self.flavor.mode_switch(mode).0 == said;
                if unlisted.is_empty() || !same(&self.mode) {
                    self.mode = (unlisted.iter())
                        .find(|mode| same(mode))
                        .map_or(said, |mode| (*mode).to_owned());
                    out.events.push(AgentEvent::ModeChanged {
                        mode: self.mode.clone(),
                    });
                }
            }
            "config_option_update" => {
                let moved = self.take_options(update);
                if moved {
                    out.events.push(AgentEvent::ModeChanged {
                        mode: self.mode.clone(),
                    });
                }
                if self.session.is_some() && !self.opening {
                    out.events.push(self.ready());
                }
            }
            "usage_update" => {
                self.used = number(&update["used"]);
                self.size = number(&update["size"]);
                if let Some(amount) = update["cost"]["amount"].as_f64() {
                    self.cost = Some(amount);
                }
            }
            "user_message_chunk" | "session_info_update" => {}
            _ => out.events.push(AgentEvent::Unknown(msg.clone())),
        }
    }

    /// A tool call opens, changes or ends.
    fn tool(&mut self, update: &Value, out: &mut Decoded) {
        let id = text(&update["toolCallId"]);
        let call = self.merge(&id, update);
        if update["sessionUpdate"] == "tool_call" {
            self.announced_calls.insert(id.clone());
        } else if !self.announced_calls.contains(&id) {
            return;
        }
        let status = update["status"].as_str();
        // A call whose input is still coming (its status "pending") is drawn once it runs.
        let runs = status != Some("pending");
        if runs && !self.shown.contains_key(&id) {
            let servers = self.servers();
            let drawn = self.flavor.tool(&call, &servers);
            if let Some((name, input)) = &drawn {
                out.events.push(AgentEvent::ToolUse {
                    call: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                    parent: None,
                });
            }
            self.shown.insert(id.clone(), drawn);
        }
        if !matches!(status, Some("completed" | "failed")) {
            return;
        }
        self.calls.remove(&id);
        let Some(Some((name, input))) = self.shown.get(&id).cloned() else {
            return;
        };
        let failed = status == Some("failed");
        let mut output: Vec<String> = vec![];
        let mut files = vec![];
        let mut inline = vec![];
        for block in update["content"].as_array().into_iter().flatten() {
            match block["type"].as_str() {
                Some("diff") => files.push(FileChange {
                    path: text(&block["path"]).into(),
                    before: block["oldText"].as_str().map(str::to_owned),
                    after: block["newText"].as_str().map(str::to_owned),
                }),
                Some("content") if block["content"]["type"] == "image" => {
                    inline.push(ToolContent::Image {
                        mime: text(&block["content"]["mimeType"]),
                        data: text(&block["content"]["data"]),
                    })
                }
                Some("content") => {
                    output.extend(block["content"]["text"].as_str().map(str::to_owned))
                }
                _ => {}
            }
        }
        let raw = &update["rawOutput"];
        if output.iter().all(|words| words.trim().is_empty()) {
            output = (raw.as_str().or(raw["error"].as_str()).map(str::to_owned))
                .or_else(|| self.flavor.output(raw))
                .into_iter()
                .collect();
        }
        let output = output.join("\n");
        let command = (name == "Bash").then(|| CommandRun {
            command: text(&input["command"]),
            stdout: output.clone(),
            stderr: String::new(),
            interrupted: false,
        });
        out.events.push(result(id, output, failed, files, command));
        if let Some(AgentEvent::ToolResult { inline: kept, .. }) = out.events.last_mut() {
            *kept = inline;
        }
    }

    /// What a tool call update adds to the call as known so far.
    fn merge(&mut self, id: &str, update: &Value) -> Call {
        let call = self.calls.entry(id.to_owned()).or_insert_with(|| Call {
            id: id.to_owned(),
            input: Value::Null,
            ..Default::default()
        });
        for (key, field) in [
            ("name", &mut call.name),
            ("kind", &mut call.kind),
            ("title", &mut call.title),
        ] {
            if let Some(said) = update[key].as_str() {
                *field = said.to_owned();
            }
        }
        if !update["rawInput"].is_null() {
            call.input = update["rawInput"].clone();
        }
        if let Some(path) = update["locations"][0]["path"].as_str() {
            call.path = Some(path.to_owned());
        }
        for block in update["content"].as_array().into_iter().flatten() {
            match block["type"].as_str() {
                Some("diff") if call.diff.is_none() => {
                    call.diff = Some((
                        text(&block["path"]),
                        block["oldText"].as_str().map(str::to_owned),
                        text(&block["newText"]),
                    ))
                }
                Some("content") => {
                    if let Some(words) = block["content"]["text"].as_str() {
                        call.text = words.to_owned();
                    }
                }
                _ => {}
            }
        }
        call.clone()
    }

    /// A request of the agent's: a permission, a form of questions, or something Newen does
    /// not offer.
    fn agent_request(
        &mut self,
        method: &str,
        id: &Value,
        p: &Value,
        msg: &Value,
        out: &mut Decoded,
    ) {
        let key = key(id);
        self.announce(out);
        match method {
            "session/request_permission" => {
                let tool = &p["toolCall"];
                let call = self.merge(&text(&tool["toolCallId"]), tool);
                let servers = self.servers();
                let (name, input) = self
                    .flavor
                    .tool(&call, &servers)
                    .unwrap_or_else(|| by_kind(&call));
                let options: Vec<(String, String)> =
                    (p["options"].as_array().into_iter().flatten())
                        .map(|o| (text(&o["optionId"]), text(&o["kind"])))
                        .collect();
                // Newen's own tools are Newen's to run: they never ask the person.
                if ours(&name) {
                    let allow = pick(&options, &["allow_once", "allow_always"]);
                    return out.replies.push(selected(id, allow));
                }
                let always = pick(&options, &["allow_always"]);
                self.asks
                    .insert(key.clone(), Ask::Permission(id.clone(), options));
                out.events.push(AgentEvent::PermissionRequest {
                    id: key,
                    call: Some(call.id).filter(|c| !c.is_empty()),
                    tool: name,
                    input,
                    title: Some(call.title).filter(|t| !t.is_empty()),
                    interactive: false,
                    suggestions: always
                        .map(|o| vec![Suggestion::Other(json!(o))])
                        .unwrap_or_default(),
                });
            }
            "elicitation/create" if p["mode"] == "form" => {
                let fields = form_fields(p);
                let questions: Vec<Value> = fields
                    .iter()
                    .map(|f| {
                        let options: Vec<Value> = (f.choices.iter())
                            .map(|(label, _)| json!({ "label": label, "description": "" }))
                            .collect();
                        json!({ "question": f.question, "header": "", "options": options,
                            "multiSelect": f.multi, "other": f.custom.is_some() || f.choices.is_empty() })
                    })
                    .collect();
                self.asks.insert(key.clone(), Ask::Form(id.clone(), fields));
                out.events.push(AgentEvent::PermissionRequest {
                    id: key,
                    call: p["toolCallId"].as_str().map(str::to_owned),
                    tool: "AskUserQuestion".into(),
                    input: json!({ "questions": questions, "blocking": true }),
                    title: p["message"].as_str().map(str::to_owned),
                    interactive: true,
                    suggestions: vec![],
                });
            }
            // A form Newen cannot show (a page to open, say): the agent goes on without it.
            "elicitation/create" => out.replies.push(reply(id, json!({ "action": "decline" }))),
            // A question or a plan of the agent's own, as its flavor reads it.
            _ if self.flavor.ask(method, p).is_some() => {
                let (tool, input) = self.flavor.ask(method, p).unwrap_or_default();
                self.asks.insert(
                    key.clone(),
                    Ask::Own(id.clone(), method.to_owned(), p.clone()),
                );
                out.events.push(AgentEvent::PermissionRequest {
                    id: key,
                    call: p["toolCallId"].as_str().map(str::to_owned),
                    tool,
                    input,
                    title: None,
                    interactive: true,
                    suggestions: vec![],
                });
            }
            // Files and terminals stay the agent's own: Newen offers none of the client's.
            _ => {
                out.events.push(AgentEvent::Unknown(msg.clone()));
                out.replies.push(
                    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "not supported by Newen" } })
                        .to_string(),
                );
            }
        }
    }

    fn decide(&mut self, id: String, decision: Decision) -> Vec<String> {
        match self.asks.remove(&id) {
            Some(Ask::Permission(rpc, options)) => {
                let wanted: &[&str] = match &decision {
                    Decision::Allow | Decision::Answer(_) => &["allow_once", "allow_always"],
                    Decision::AllowAlways(_) => &["allow_always", "allow_once"],
                    Decision::Deny { .. } => &["reject_once", "reject_always"],
                };
                match pick(&options, wanted) {
                    Some(option) => vec![selected(&rpc, Some(option))],
                    None => vec![reply(
                        &rpc,
                        json!({ "outcome": { "outcome": "cancelled" } }),
                    )],
                }
            }
            Some(Ask::Form(rpc, fields)) => {
                let Decision::Answer(answers) = decision else {
                    return vec![reply(&rpc, json!({ "action": "decline" }))];
                };
                vec![reply(
                    &rpc,
                    json!({ "action": "accept", "content": form_answer(&fields, &answers) }),
                )]
            }
            Some(Ask::Own(rpc, method, params)) => {
                vec![reply(
                    &rpc,
                    self.flavor.answer(&method, &params, Some(&decision)),
                )]
            }
            None => vec![],
        }
    }

    pub fn encode(&mut self, command: AgentCommand) -> Vec<String> {
        match command {
            AgentCommand::Start => {
                // A new process: its requests count from 1 again.
                let spec = self.spec.lock().unwrap().clone();
                *self = Self::new(self.flavor);
                self.remember(&spec);
                let meta = self.flavor.client_meta();
                vec![self.request(
                    "initialize",
                    json!({
                        "protocolVersion": 1,
                        "clientCapabilities": {
                            "fs": { "readTextFile": false, "writeTextFile": false },
                            "terminal": false,
                            "elicitation": { "form": {} },
                            "_meta": meta,
                        },
                        // Newen says who it is, never another client.
                        "clientInfo": { "name": "newen", "title": "Newen", "version": env!("CARGO_PKG_VERSION") },
                    }),
                    Pending::Init,
                )]
            }
            AgentCommand::Prompt {
                request,
                text,
                images,
                ..
            } => {
                let mut blocks: Vec<Value> = images.iter().map(image_block).collect();
                blocks.push(json!({ "type": "text", "text": text }));
                self.queued.push((request, blocks));
                self.next_prompt().into_iter().collect()
            }
            AgentCommand::Interrupt => {
                let Some(session) = self.session.clone().filter(|_| self.turn.is_some()) else {
                    return vec![];
                };
                self.interrupted = true;
                let mut lines = vec![json!({ "jsonrpc": "2.0", "method": "session/cancel",
                    "params": { "sessionId": session } })
                .to_string()];
                // A stopped turn's questions are answered "cancelled", as ACP asks.
                for (id, ask) in std::mem::take(&mut self.asks) {
                    lines.push(match ask {
                        Ask::Permission(rpc, _) => {
                            reply(&rpc, json!({ "outcome": { "outcome": "cancelled" } }))
                        }
                        Ask::Form(rpc, _) => reply(&rpc, json!({ "action": "cancel" })),
                        Ask::Own(rpc, method, params) => {
                            reply(&rpc, self.flavor.answer(&method, &params, None))
                        }
                    });
                    self.withdrawn.push(id);
                }
                lines
            }
            AgentCommand::Decide { id, decision } => self.decide(id, decision),
            // Agents change these only between turns (some refuse while one runs).
            command @ (AgentCommand::SetMode(_)
            | AgentCommand::SetModel(_)
            | AgentCommand::SetEffort(_)) => {
                if self.turn.is_some() {
                    self.held.push(command);
                    return vec![];
                }
                self.set(&command)
            }
            // Newen's own tools are answered over their own server (`SessionExtras::host`); no
            // context breakdown, MCP state, rewind or remote in ACP.
            _ => vec![],
        }
    }

    pub fn decode(&mut self, line: &str) -> Decoded {
        let mut decoded = Decoded::default();
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            decoded
                .events
                .push(AgentEvent::Unknown(Value::String(line.into())));
            return decoded;
        };
        match (msg["method"].as_str(), msg.get("id")) {
            (Some(method), Some(id)) => {
                self.agent_request(method, id, &msg["params"], &msg, &mut decoded)
            }
            (Some("session/update"), None) => {
                self.update(&msg["params"]["update"], &msg, &mut decoded)
            }
            // The agent no longer waits for an answer (another ask's "always" settled it).
            (Some("$/cancel_request"), None) => {
                let gone = key(&msg["params"]["requestId"]);
                if self.asks.remove(&gone).is_some() {
                    decoded.events.push(AgentEvent::Withdrawn { id: gone });
                }
            }
            (Some(method), None) if self.flavor.quiet(method) => {}
            (None, Some(id)) => self.response(id, &msg, &mut decoded),
            _ => decoded.events.push(AgentEvent::Unknown(msg)),
        }
        decoded
    }
}

/// An "All agents" server, or one of Newen's, as ACP's `mcpServers` takes it. A secret of a
/// command's environment is in the agent's own environment under its own name
/// (`SessionExtras::env_by_name`), which the server inherits; ACP has no way to name it.
pub fn mcp_server(server: &InjectedServer) -> Value {
    let pairs = |list: &[(String, String)]| -> Vec<Value> {
        list.iter()
            .map(|(name, value)| json!({ "name": name, "value": value }))
            .collect()
    };
    if server.remote {
        let kind = if server.sse { "sse" } else { "http" };
        return json!({ "type": kind, "name": server.name, "url": server.target,
            "headers": pairs(&server.headers) });
    }
    json!({ "name": server.name, "command": server.target, "args": server.args,
        "env": pairs(&server.env) })
}

/// The questions of a form, `<key>_custom` folded into its question as the answer in the
/// person's words.
fn form_fields(p: &Value) -> Vec<Field> {
    let schema = &p["requestedSchema"]["properties"];
    let Some(properties) = schema.as_object() else {
        return vec![];
    };
    let message = text(&p["message"]);
    properties
        .iter()
        // `<key>_custom` or `<key>_other`: the field for words of the person's own.
        .filter(|(key, _)| {
            !(key.strip_suffix("_custom").or(key.strip_suffix("_other")))
                .is_some_and(|base| properties.contains_key(base))
        })
        .map(|(key, field)| {
            let multi = field["type"] == "array";
            let source = if multi { &field["items"] } else { field };
            let mut choices: Vec<(String, Value)> = vec![];
            for list in ["oneOf", "anyOf"] {
                for choice in source[list].as_array().into_iter().flatten() {
                    let label = choice["title"].as_str().map(str::to_owned);
                    choices.push((
                        label.unwrap_or_else(|| text(&choice["const"])),
                        choice["const"].clone(),
                    ));
                }
            }
            for value in source["enum"].as_array().into_iter().flatten() {
                choices.push((text(value), value.clone()));
            }
            let question = [&field["description"], &field["title"]]
                .into_iter()
                .find_map(|v| v.as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| message.clone());
            let custom = ["_custom", "_other"]
                .map(|end| format!("{key}{end}"))
                .into_iter()
                .find(|field| properties.contains_key(field));
            Field {
                key: key.clone(),
                question,
                choices,
                multi,
                custom,
            }
        })
        .collect()
}

/// The form's content for the card's answers (`{question: "label, label"}`): a label is its
/// choice's value; other words go to the field for them, or stand as the answer.
fn form_answer(fields: &[Field], answers: &Value) -> Value {
    let mut content = serde_json::Map::new();
    for field in fields {
        let said = text(&answers[&field.question]);
        if said.is_empty() || said == "[No preference]" {
            continue;
        }
        let value_of = |words: &str| {
            (field.choices.iter())
                .find(|(label, _)| label == words)
                .map(|(_, value)| value.clone())
        };
        let parts: Vec<&str> = if field.multi {
            said.split(", ").collect()
        } else {
            vec![said.as_str()]
        };
        let (picked, own): (Vec<&str>, Vec<&str>) =
            parts.into_iter().partition(|p| value_of(p).is_some());
        let picked: Vec<Value> = picked.into_iter().filter_map(value_of).collect();
        let own = own.join(", ");
        match (&field.custom, field.multi) {
            (Some(custom), _) if !own.is_empty() => {
                content.insert(custom.clone(), json!(own));
            }
            (None, true) if !own.is_empty() => {
                let mut all = picked.clone();
                all.push(json!(own));
                content.insert(field.key.clone(), json!(all));
                continue;
            }
            (None, false) if !own.is_empty() => {
                content.insert(field.key.clone(), json!(own));
                continue;
            }
            _ => {}
        }
        if field.multi {
            content.insert(field.key.clone(), json!(picked));
        } else if let Some(value) = picked.into_iter().next() {
            content.insert(field.key.clone(), value);
        }
    }
    Value::Object(content)
}

/// The first option of these kinds, in order.
fn pick(options: &[(String, String)], kinds: &[&str]) -> Option<String> {
    kinds.iter().find_map(|kind| {
        options
            .iter()
            .find(|(_, k)| k == kind)
            .map(|(id, _)| id.clone())
    })
}

fn selected(id: &Value, option: Option<String>) -> String {
    match option {
        Some(option) => reply(
            id,
            json!({ "outcome": { "outcome": "selected", "optionId": option } }),
        ),
        None => reply(id, json!({ "outcome": { "outcome": "cancelled" } })),
    }
}

fn reply(id: &Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

/// ACP's `auth_required` error.
fn is_auth(msg: &Value) -> bool {
    msg["error"]["code"] == -32000
}

fn image_block(image: &PromptImage) -> Value {
    let data = base64::engine::general_purpose::STANDARD.encode(&image.data);
    json!({ "type": "image", "mimeType": image.media_type, "data": data })
}

fn finished(ok: bool, interrupted: bool, usage: Usage, error: Option<String>) -> AgentEvent {
    AgentEvent::TurnFinished {
        ok,
        interrupted,
        usage,
        error,
    }
}

fn result(
    call: String,
    output: String,
    is_error: bool,
    files: Vec<FileChange>,
    command: Option<CommandRun>,
) -> AgentEvent {
    AgentEvent::ToolResult {
        call,
        output,
        is_error,
        files,
        command,
        web: None,
        images: vec![],
        inline: vec![],
    }
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

fn number(value: &Value) -> u64 {
    value.as_u64().unwrap_or_default()
}

/// A JSON-RPC id as a key: numbers and strings alike.
fn key(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
