//! The contract between quill's engine and one coding-agent CLI (docs/agents.md).
//!
//! An adapter is pure: it never opens a process or a socket. The engine starts what
//! [`AgentAdapter::launch`] describes, writes what [`AgentAdapter::encode`] returns, and
//! hands every line it reads to [`AgentAdapter::decode`]. Nothing here names a vendor.

use serde_json::Value;
use std::path::PathBuf;

/// Which agent: `"claude"`, later `"codex"`, `"opencode"`…
pub type AgentId = &'static str;

/// How the engine starts the agent's process.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LaunchPlan {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub cwd: PathBuf,
}

/// What a new or resumed session asks of the agent.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SessionSpec {
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
    /// The agent's own id of a session to continue.
    pub resume: Option<String>,
    /// Continue `resume` as a new session, leaving the old one as it was.
    pub fork: bool,
    /// Keep `resume`'s conversation only up to this message of it, by the agent's own id.
    pub resume_at: Option<String>,
    /// The person allowed the agent's modes that skip every approval.
    pub allow_unsafe: bool,
    /// The person's values for the adapter's own options ([`AgentAdapter::options`]), by key.
    #[serde(default)]
    pub options: std::collections::BTreeMap<String, String>,
    /// The "All agents" servers the session adds, already chosen for this agent by the engine
    /// (docs/agents.md "All agents"). They carry no secret: only the name of the variable of
    /// the process that holds it.
    #[serde(default)]
    pub servers: Vec<InjectedServer>,
    /// Folders of skills the session adds (`<folder>/<name>/SKILL.md`).
    #[serde(default)]
    pub skill_roots: Vec<PathBuf>,
    /// Variables of the session's own when its process is shared with others ([`Mux`]): the
    /// process's environment cannot carry them for one session.
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// An "All agents" MCP server as one session gets it. Plain values are written as they are;
/// a secret is `(key, variable)`, the variable being one of the agent process's environment.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct InjectedServer {
    pub name: String,
    pub target: String,
    pub args: Vec<String>,
    /// Reached by an address (else a command)...
    pub remote: bool,
    /// ...by the older server-sent-events transport rather than streamable HTTP.
    pub sse: bool,
    pub env: Vec<(String, String)>,
    pub env_vars: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub header_vars: Vec<(String, String)>,
}

/// What an adapter can add to a session without touching the agent's own files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionExtras {
    pub servers: bool,
    /// Takes servers by the older SSE transport too.
    pub sse: bool,
    /// Gives a secret to a server's environment only under the variable's own name.
    pub env_by_name: bool,
    pub skills: bool,
}

/// One of an agent's own settings, as its adapter declares it, which quill's Settings shows on
/// the agent's page and hands back in [`SessionSpec::options`]: a switch (`"true"` or
/// `"false"`) without choices, else one of `choices`, each with how to say it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentOption {
    pub key: String,
    pub label: String,
    pub description: String,
    pub choices: Choices,
    pub default: String,
}

/// What a permission mode is for, which picks its mark and ink in the mode menu without the
/// UI knowing any agent's words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ModeTone {
    /// Asks before changing anything.
    Ask,
    /// Takes file edits by itself.
    Edits,
    Plan,
    /// Decides what to allow by itself.
    Auto,
    /// Only reads (asks before anything else).
    Read,
    /// Skips every approval; shown as a warning.
    Unsafe,
}

/// One permission mode in the agent's own words, as the mode menu lists it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModeWords {
    /// The mode as the agent names it (`AgentCapabilities::modes`).
    pub id: String,
    pub label: String,
    pub about: String,
    pub tone: ModeTone,
    /// Listed before the agent has said what it offers (some modes depend on the model, and
    /// the ones that skip approvals wait for the person's say).
    pub listed: bool,
}

/// What a task has used: its tokens, its tool calls and how long it has run.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TaskUsage {
    pub tokens: u64,
    pub tool_uses: u64,
    pub duration_ms: u64,
}

/// One model the agent offers, with the efforts it accepts.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub efforts: Vec<String>,
    /// Permission modes only this model offers, after the agent's own.
    #[serde(default)]
    pub modes: Vec<String>,
    /// The one the agent runs when none is chosen.
    #[serde(default)]
    pub default: bool,
}

/// A command the agent runs itself when a message starts with `/name`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentSlashCommand {
    pub name: String,
    pub description: String,
    pub argument_hint: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

/// What this agent can do; the UI offers only what is here.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentCapabilities {
    pub models: Vec<ModelInfo>,
    /// The agent's own slash commands, sorted by name.
    #[serde(default)]
    pub commands: Vec<AgentSlashCommand>,
    /// Permission modes, in the agent's own words (`default`, `acceptEdits`, `plan`…).
    pub modes: Vec<String>,
    /// Modes that skip every approval, offered only once the person allows them.
    #[serde(default)]
    pub unsafe_modes: Vec<String>,
    pub interrupt: bool,
    pub resume: bool,
    pub fork: bool,
    /// The agent restores files to before a request by itself.
    pub rewind: bool,
    /// The agent reports each tool as it ends, tied to its request: per-tool attribution.
    pub tool_hooks: bool,
    /// Optional features the agent announced, as it names them.
    pub features: Vec<String>,
    /// The person's plan, as the agent names it ("max", "pro"), when it says.
    #[serde(default)]
    pub plan: Option<String>,
    /// The agent tells its MCP servers' state on request, and reconnects and turns them on
    /// and off ([`AgentCommand::McpStatus`]).
    #[serde(default)]
    pub mcp: bool,
    /// ... and turns one on or off for the session; off, the UI shows no switch.
    #[serde(default)]
    pub mcp_toggle: bool,
    /// ... and forgets a server's credentials ([`AgentCommand::McpClearAuth`]).
    #[serde(default)]
    pub mcp_clear_auth: bool,
    /// The agent can show what its model thought.
    #[serde(default)]
    pub thinking_text: bool,
    /// The agent can take a message into a turn already running.
    #[serde(default)]
    pub steer: bool,
    /// The person can reach the session from another device (a phone, the web) through the
    /// agent's own service ([`AgentCommand::Remote`]).
    #[serde(default)]
    pub remote: bool,
    /// ... where another device is paired once with a code ([`AgentCommand::RemotePair`]) and
    /// then reaches all of the agent's sessions, not this one alone.
    #[serde(default)]
    pub remote_pairing: bool,
}

/// One MCP server a session's agent loaded, as the agent reports it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpServer {
    /// The name the agent knows it by, which reconnecting and toggling it take.
    pub name: String,
    /// The name to show, without what the agent prefixes to tell servers apart.
    pub label: String,
    pub status: McpStatus,
    pub source: McpSource,
    /// The plugin that ships it, for [`McpSource::Plugin`].
    pub plugin: Option<String>,
    /// Why it failed, when it did.
    pub error: Option<String>,
    /// Its tools, once connected.
    pub tools: Vec<McpTool>,
    /// The person can sign in to it from here ([`AgentCommand::McpAuthenticate`]): it
    /// signs in with OAuth and its sign-in is not held elsewhere.
    pub sign_in: bool,
}

/// One tool of an MCP server, with what its server says it does.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    /// It only reads.
    pub read_only: bool,
    /// It may change or delete things.
    pub destructive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum McpStatus {
    Connected,
    Pending,
    NeedsAuth,
    Failed,
    Disabled,
}

/// A plugin installed for the agent, as its program lists it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Plugin {
    /// The agent's own id of it, which switching it on and off takes.
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    /// Where it is installed, in the agent's words (`user`, `project`…).
    pub scope: String,
    pub enabled: bool,
}

/// A plugin a marketplace offers.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginOffer {
    /// The id installing it takes.
    pub id: String,
    pub name: String,
    pub description: String,
    pub marketplace: String,
    pub version: Option<String>,
    /// How many times it was installed, when the marketplace counts.
    pub installs: Option<u64>,
}

/// A marketplace of plugins the agent knows.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Marketplace {
    pub name: String,
    /// Where it comes from: a repository, an address or a folder.
    pub source: String,
}

/// A skill the agent offers, as it lists them.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Where it comes from, in the agent's words (`user`, `plugin`…).
    pub source: String,
    /// What the agent does with it, one of [`Extensions::skill_states`] (the first is on).
    pub state: String,
    /// Why its state cannot be changed here, when it cannot.
    pub locked: Option<String>,
    /// What the adapter needs back to change its state.
    pub key: Value,
}

/// An agent's plugins, the marketplaces it knows and what they offer, and its skills, as
/// its program lists them for a folder.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Extensions {
    pub plugins: Vec<Plugin>,
    pub offers: Vec<PluginOffer>,
    pub marketplaces: Vec<Marketplace>,
    pub skills: Vec<Skill>,
    /// Where a plugin can be installed, in the agent's words, each with how to say it; the
    /// first is the person's own, for every folder.
    pub scopes: Choices,
    /// What a skill can be, in the agent's words, each with how to say it.
    pub skill_states: Choices,
    pub kinds: ExtensionKinds,
}

/// What an agent's program can do with its extensions, for the pages to offer only that.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExtensionKinds {
    /// A skill's state is the folder's; off, it is the person's for every folder.
    pub skills_per_folder: bool,
    /// A plugin can be turned off and on without uninstalling it.
    pub switch_plugins: bool,
    /// An installed plugin can be updated by itself.
    pub update_plugins: bool,
}

impl Default for ExtensionKinds {
    fn default() -> Self {
        Self {
            skills_per_folder: true,
            switch_plugins: true,
            update_plugins: true,
        }
    }
}

/// Choices in the agent's words, each with how to say it.
pub type Choices = Vec<(String, String)>;

/// A change to an agent's plugins, marketplaces or skills, made by its own program.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExtensionOp {
    SwitchPlugin {
        plugin: Plugin,
        enabled: bool,
    },
    /// Install an offered plugin, for the person (`None`) or where `scope` says.
    Install {
        id: String,
        scope: Option<String>,
    },
    Uninstall(Plugin),
    Update(String),
    AddMarketplace(String),
    UpdateMarketplace(String),
    RemoveMarketplace(String),
    /// A skill's state, for the folder.
    SkillState {
        skill: Skill,
        state: String,
    },
    /// An MCP server added to the agent, or one of its own taken away, by name.
    AddServer(NewServer),
    RemoveServer(String),
}

/// An MCP server the agent is set up with, as its own list says: its name, where it runs (a
/// command or an address), how it stands, and where it came from.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ConfiguredServer {
    pub name: String,
    /// The name without what the agent prefixes to tell servers apart, as the server's own
    /// product is called (its logo is looked up by it).
    #[serde(default)]
    pub label: String,
    pub target: String,
    pub status: McpStatus,
    /// Its list checked that it connects; off, `status` only says it is set up (on or off).
    #[serde(default)]
    pub checked: bool,
    pub source: McpSource,
}

/// An MCP server to add: by an address (`remote`) or a command and its arguments, where
/// `scope` says (the agent's default without one), with its environment's `KEY=value`s and,
/// for an address, its `Name: value` headers.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NewServer {
    pub name: String,
    pub target: String,
    pub args: Vec<String>,
    pub remote: bool,
    pub scope: Option<String>,
    pub env: Vec<String>,
    pub headers: Vec<String>,
}

/// Whether the person is signed in to an agent, as its own program reports it: the account
/// belongs to the program, quill keeps none of it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AuthState {
    SignedIn {
        /// How, in the program's words ("claude.ai", "api_key"); not shown as it is.
        method: String,
        email: Option<String>,
        org: Option<String>,
        /// The subscription, as the program names it ("max").
        plan: Option<String>,
    },
    SignedOut,
    /// An answer that was not understood, kept whole (the program may be a newer one).
    Unknown(String),
}

/// What to run for an [`ExtensionOp`]: the program's arguments and what goes to its stdin.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgramRun {
    pub args: Vec<String>,
    pub stdin: Option<String>,
    /// How long it may take, in seconds.
    pub timeout: u64,
    /// For a program that takes its requests on stdin and stops when it closes before
    /// answering the last: keep stdin open until a line of stdout holds this text.
    pub until: Option<String>,
}

/// Where a server's definition came from: the person's own settings, the project's, the
/// person's for this project only, the organization's, a plugin, the person's account, the
/// app running the agent (quill's own tools), or somewhere else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum McpSource {
    User,
    Project,
    Local,
    Managed,
    Plugin,
    Account,
    App,
    /// quill's own "All agents" layer, which the engine marks (adapters never do).
    Global,
    Other,
}

/// One file a tool changed, as the agent reported it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileChange {
    pub path: PathBuf,
    /// Content before the change; `None` when the tool created the file.
    pub before: Option<String>,
    pub after: Option<String>,
}

/// A shell command a tool ran.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandRun {
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub interrupted: bool,
}

/// One source a web search returned.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebSource {
    pub title: String,
    pub url: String,
}

/// A web search a tool ran: the query and the sources it returned, in the agent's order.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebSearchRun {
    pub query: String,
    pub sources: Vec<WebSource>,
}

/// One permission the agent offers to remember, kept as the agent wrote it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Suggestion {
    /// Allow rules for later calls.
    Rules(Value),
    /// Switch the session's permission mode.
    Mode(String),
    /// Anything else (e.g. extra directories), passed back untouched if chosen.
    Other(Value),
}

/// The agent's own token use for a turn.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
    /// How much of the model's context the conversation fills after the turn.
    #[serde(default)]
    pub context_tokens: u64,
    /// The size of the model's context, when the agent says.
    #[serde(default)]
    pub context_window: u64,
}

/// One usage window of the person's plan.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UsageWindow {
    /// The share spent, 0 to 1.
    pub utilization: f32,
    /// When it starts again, in Unix seconds.
    pub resets_at: u64,
}

/// How the agent's context is spent, as it reports it on request.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContextUsage {
    /// Tokens in use and the context's size.
    pub used: u64,
    pub max: u64,
    /// What fills it, in the agent's own words, each with its share.
    pub categories: Vec<ContextCategory>,
    /// Lists that make up some categories (tools of each server, memory files…), each by
    /// name with its entries' tokens.
    pub groups: Vec<(String, Vec<(String, u64)>)>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContextCategory {
    pub name: String,
    pub tokens: u64,
    pub kind: ContextKind,
}

/// What a category's tokens are: in the context now, loaded only when needed, kept free
/// for the agent's own compaction, or not used yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ContextKind {
    Used,
    Deferred,
    Buffer,
    Free,
}

/// The plan's usage windows the agent reported: the session's five hours and the week.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlanLimits {
    pub session: Option<UsageWindow>,
    pub weekly: Option<UsageWindow>,
}

/// Something the agent said, in quill's terms.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AgentEvent {
    /// The agent answered `Start`: who it is and what it can do.
    Ready {
        version: String,
        capabilities: AgentCapabilities,
        mode: Option<String>,
    },
    /// The agent's id for this session, once it is known.
    Session {
        id: String,
    },
    /// A request began running (`request` is the id quill sent with the prompt).
    Running {
        requests: Vec<String>,
    },
    TextDelta {
        text: String,
        parent: Option<String>,
    },
    Text {
        text: String,
        parent: Option<String>,
        /// The agent's own id of the message: a fork can start from it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        /// When it came, in seconds since 1970; the engine sets it as it keeps it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<u64>,
    },
    Thinking {
        parent: Option<String>,
    },
    /// A piece of what the model thinks, as it streams.
    ThinkingDelta {
        text: String,
        parent: Option<String>,
    },
    /// What the model thought, whole, as its message ends.
    ThinkingText {
        text: String,
        parent: Option<String>,
    },
    /// The tokens the running turn has used so far, its helpers' included.
    Tokens {
        tokens: u64,
    },
    /// Work the agent runs beside its turn began: a helper agent, a command in the
    /// background. `call` is the tool call that started it; `kind` says what it is in
    /// quill's words: `agent`, `command`, `workflow`, `monitor` or `task`.
    TaskStarted {
        id: String,
        call: Option<String>,
        description: String,
        #[serde(default)]
        kind: String,
    },
    /// What that work has used so far.
    TaskProgress {
        id: String,
        usage: TaskUsage,
    },
    /// That work ended; `summary` is its own account of what it did, `stopped` when it was
    /// stopped before it finished, and `usage` what it used.
    TaskEnded {
        id: String,
        call: Option<String>,
        summary: Option<String>,
        ok: bool,
        #[serde(default)]
        stopped: bool,
        #[serde(default)]
        usage: Option<TaskUsage>,
    },
    ToolUse {
        call: String,
        name: String,
        input: Value,
        parent: Option<String>,
    },
    ToolResult {
        call: String,
        output: String,
        is_error: bool,
        files: Vec<FileChange>,
        command: Option<CommandRun>,
        /// The sources of a web search (`None` for other tools).
        #[serde(default)]
        web: Option<WebSearchRun>,
        /// The pictures it gave back, as the engine kept them: files under quill's data folder.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<PathBuf>,
        /// The same pictures as the adapter read them, for the engine to keep; never stored.
        #[serde(skip)]
        inline: Vec<ToolContent>,
    },
    /// A tool ended. `prompt` is the agent's own id of the request that caused it, the
    /// same for every tool of that request; the engine ties it to quill's request id.
    ToolDone {
        call: String,
        prompt: Option<String>,
    },
    PermissionRequest {
        id: String,
        call: Option<String>,
        tool: String,
        input: Value,
        title: Option<String>,
        /// The person must answer something, not only allow or deny (a question).
        interactive: bool,
        suggestions: Vec<Suggestion>,
    },
    ModeChanged {
        mode: String,
    },
    /// The agent would not switch to `mode` ([`AgentCommand::SetMode`]); `error` is its reason.
    /// The engine starts it again in that mode when its process was launched without the
    /// Settings that allow it.
    ModeRefused {
        mode: String,
        error: String,
    },
    TurnFinished {
        ok: bool,
        interrupted: bool,
        usage: Usage,
        error: Option<String>,
    },
    /// The answer to a command quill sent, by the id `encode` gave it.
    Reply {
        id: String,
        ok: bool,
        body: Value,
    },
    /// How much of the plan's usage windows is spent.
    Limits(PlanLimits),
    /// The answer to [`AgentCommand::ContextUsage`].
    ContextUsage(ContextUsage),
    /// The answer to [`AgentCommand::McpStatus`]: every server the session loaded.
    McpServers(Vec<McpServer>),
    /// The answer to [`AgentCommand::Skills`].
    Skills(Vec<Skill>),
    /// The answer to [`AgentCommand::McpAuthenticate`]: the page where the person signs in,
    /// or none when the server needed no one (it has valid credentials); or why it could
    /// not start.
    McpSignIn {
        server: String,
        url: Option<String>,
        error: Option<String>,
    },
    /// Whether the session can be reached from another device, and where it opens there
    /// (unknown when the agent connected by itself); `error` says why it is off.
    Remote {
        on: bool,
        url: Option<String>,
        error: Option<String>,
    },
    /// The agent no longer waits for this permission: another device answered it.
    Withdrawn {
        id: String,
    },
    /// A message the person sent from another device, which the agent took as `request`.
    Prompt {
        request: String,
        text: String,
    },
    /// The code to type on another device to pair it, until `expires_at` (seconds since 1970).
    RemotePairing {
        code: String,
        expires_at: u64,
    },
    /// What the person might ask next, as the agent guesses it after a turn ends.
    PromptSuggestion {
        text: String,
    },
    /// The agent calls one of quill's own tools, the ones that start and propose other
    /// sessions ([`HOST_TOOLS`]). The engine answers with [`AgentCommand::HostToolResult`].
    HostTool {
        id: String,
        name: String,
        input: Value,
    },
    /// The turn could not run because the person is not signed in to the agent.
    AuthRequired,
    /// A line the adapter does not understand, kept whole.
    Unknown(Value),
}

/// The answer to a [`AgentEvent::PermissionRequest`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Decision {
    Allow,
    /// Allow and remember the chosen suggestions.
    AllowAlways(Vec<Suggestion>),
    Deny {
        message: String,
    },
    /// Allow with the person's answers to an interactive request.
    Answer(Value),
}

/// An image a prompt carries, already sized for the agent (vendor-neutral: each adapter
/// writes it in its own protocol).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PromptImage {
    pub media_type: String,
    pub data: Vec<u8>,
}

/// Something quill asks of the agent.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AgentCommand {
    Start,
    Prompt {
        request: String,
        text: String,
        /// Into the turn already running, read at the agent's next step ([`AgentCapabilities::steer`]).
        #[serde(default)]
        steer: bool,
        #[serde(default)]
        images: Vec<PromptImage>,
    },
    Interrupt,
    Decide {
        id: String,
        decision: Decision,
    },
    SetMode(String),
    SetModel(String),
    SetEffort(String),
    /// Restore files to before `request`; `dry_run` only reports what would change.
    Rewind {
        request: String,
        dry_run: bool,
    },
    /// Ask how the context is spent, answered by [`AgentEvent::ContextUsage`]; an agent
    /// that cannot tell sends nothing.
    ContextUsage,
    /// Ask for the MCP servers' state, answered by [`AgentEvent::McpServers`].
    McpStatus,
    /// Connect a server that failed or has not connected again, by its name.
    McpReconnect(String),
    /// Stop a task that runs beside the turn, by its id.
    StopTask(String),
    /// Turn a server on or off, by its name.
    McpToggle {
        name: String,
        enabled: bool,
    },
    /// Start signing in to a server with OAuth, answered by [`AgentEvent::McpSignIn`]; the
    /// agent takes the page's answer itself and connects the server.
    McpAuthenticate(String),
    /// Forget a server's credentials.
    McpClearAuth(String),
    /// Ask for the skills the agent offers, answered by [`AgentEvent::Skills`].
    Skills,
    /// Load the plugins again, after one was switched on or off.
    ReloadPlugins,
    /// Load the skills again, after one's state changed.
    ReloadSkills,
    /// Let the person reach the session from another device, listed there as `name`, or stop
    /// ([`AgentCapabilities::remote`]); answered by [`AgentEvent::Remote`].
    Remote {
        on: bool,
        name: Option<String>,
    },
    /// Start pairing another device, answered by [`AgentEvent::RemotePairing`]
    /// ([`AgentCapabilities::remote_pairing`]).
    RemotePair,
    /// The answer to an [`AgentEvent::HostTool`] call.
    HostToolResult {
        id: String,
        content: Vec<ToolContent>,
        is_error: bool,
    },
}

/// One block of a tool's answer: words, or a picture (a page's screenshot).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ToolContent {
    Text(String),
    Image {
        /// "image/jpeg", "image/png".
        mime: String,
        /// The picture, base64.
        data: String,
    },
}

impl ToolContent {
    pub fn text(words: impl Into<String>) -> Vec<Self> {
        vec![Self::Text(words.into())]
    }
}

/// quill's own tools, which an adapter offers its agent where the agent can take tools from
/// its host: a tool's name, what it is for, and the JSON schema of its input.
pub const HOST_TOOLS: [(&str, &str, &str); 6] = [
    (
        "start_session",
        "Start a separate session for a task of its own while you stay with the user, or propose one. `initiation` says who wants it: \"user_asked\" (the user asked in words for separate, parallel or background work) starts it now; \"own_initiative\" (follow-up work you noticed or that your own work turned up, which the user has not asked for) shows the user a card, and nothing runs until they start it there. Both need `prompt`: the brief, standing alone (the task, the files or area, what done looks like). context \"fresh\" (default) gives the new session your `prompt` and `background` only, so `background` is required: the goal, decisions and constraints so far, for someone who has not seen this conversation; \"fork\" gives it this whole conversation and your `prompt`. A session you start reports back to you when its turn ends. A session is for a task of its own that ends in something of its own; searching, independent parts of one change, and checking what you built stay here or go to a subagent. Start nothing the user did not ask for.",
        r#"{"type":"object","properties":{"initiation":{"type":"string","enum":["user_asked","own_initiative"]},"title":{"type":"string","description":"Under 60 characters; names the task on its row or card."},"prompt":{"type":"string","description":"The brief: the task, the files or area, what done looks like."},"context":{"type":"string","enum":["fresh","fork"]},"background":{"type":"string","description":"With fresh: the goal, decisions and constraints so far."},"proposal_id":{"type":"string","description":"Start a proposal the user told you to start, exactly as proposed: its task id and nothing else."}}}"#,
    ),
    (
        "dismiss_task",
        "Withdraw a proposal you made with start_session (initiation \"own_initiative\") that the user has not started, when it went stale: the work was done here after all, it turned out unnecessary, or you proposed a better one.",
        r#"{"type":"object","properties":{"task_id":{"type":"string"}},"required":["task_id"]}"#,
    ),
    (
        "send_to_session",
        "Pass a message from the user to a session you started, when it is about that session's task: a change of plan, a detail, an answer it waits for. The session gets `message` as from the user and carries on there; tell the user in a line that you passed it on. `session` is its title as you started it.",
        r#"{"type":"object","properties":{"session":{"type":"string","description":"The session's title, as you started it."},"message":{"type":"string","description":"What it should know or do, standing alone."}},"required":["session","message"]}"#,
    ),
    (
        "read_session",
        "Read another quill session the user mentioned: its title, folder and conversation so far (the people's messages and the agent's replies, the latest last). Read-only.",
        r#"{"type":"object","properties":{"session_id":{"type":"integer"}},"required":["session_id"]}"#,
    ),
    (
        "list_sessions",
        "List the other quill sessions of this project (this folder, or another checkout of the same repository): each one's id, title, agent, whether it is working, idle or waiting for the user, and its folder. Read-only.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "wait_for_session",
        "Wait for another quill session to finish its task, then go on here by yourself. Use it when the user ties work to another conversation finishing (\"when the other conversation is done, cut the release\"); never promise to wait without it, since nothing of that conversation reaches you otherwise. Find the session with list_sessions, or by the id a mention of it gives; if more than one running session could be meant, ask the user with your question tool instead of guessing. The call returns at once: then end your turn, telling the user in a line what you will do when it finishes. quill sends you a message when that session's turn ends with its task done (not while it asks the user something), with its last report and your `then`; if it is deleted or archived first, you are told that instead.",
        r#"{"type":"object","properties":{"session_id":{"type":"integer","description":"The session to wait for."},"then":{"type":"string","description":"What you will do when it finishes, standing alone: it is given back to you then."}},"required":["session_id","then"]}"#,
    ),
];

/// Many sessions over one process of the agent's ([`AgentAdapter::mux`]). Pure, as an adapter
/// is: the engine runs the process and passes every line through here, so each session's
/// adapter reads and writes as if the process were its own.
pub trait Mux: Send {
    /// `session`'s line for the process: what to write to it, and what to answer the session
    /// at once.
    fn outbound(&mut self, session: u64, line: &str) -> (Vec<String>, Vec<String>);
    /// A line of the process, for each session it is for.
    fn inbound(&mut self, line: &str) -> Vec<(u64, String)>;
    /// `session` left the process.
    fn closed(&mut self, session: u64);
}

/// What one line from the agent became.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Decoded {
    pub events: Vec<AgentEvent>,
    /// Lines to write back at once, without the engine deciding (e.g. hook acknowledgements).
    pub replies: Vec<String>,
}

/// One coding-agent CLI.
pub trait AgentAdapter: Send {
    fn id(&self) -> AgentId;
    /// The agent's name for people, as Settings lists it.
    fn name(&self) -> &'static str;
    /// The versions of the agent's program quill was tested with.
    fn tested(&self) -> &'static [&'static str];
    fn launch(&self, spec: &SessionSpec) -> LaunchPlan;
    /// What the session can be given from quill's "All agents" layer.
    fn session_extras(&self) -> SessionExtras {
        SessionExtras::default()
    }
    /// Whether, in this permission `mode`, the agent starts a dev server without a card: the
    /// modes that skip every approval, or in which it decides alone.
    fn mode_waives_prompts(&self, _mode: &str) -> bool {
        false
    }
    /// The words and tone of every permission mode the agent has, unsafe ones too.
    fn modes(&self) -> Vec<ModeWords> {
        vec![]
    }
    /// The mode a new session starts in, one of [`modes`](Self::modes).
    fn default_mode(&self) -> &'static str {
        ""
    }
    /// Where, in a session's folder, the agent keeps the dev servers `preview_start` runs
    /// ([`LaunchConfig`]); `None` when it keeps none.
    fn launch_file(&self) -> Option<&'static str> {
        None
    }
    /// The arguments that make the agent's program list its installed plugins and the
    /// ones its marketplaces offer, read by [`plugins`](Self::plugins); `None` when it has
    /// none.
    fn plugins_query(&self) -> Option<Vec<String>> {
        None
    }
    /// The installed plugins and the offered ones in what
    /// [`plugins_query`](Self::plugins_query) printed.
    fn plugins(&self, _stdout: &str) -> (Vec<Plugin>, Vec<PluginOffer>) {
        (vec![], vec![])
    }
    /// The arguments that list the marketplaces, read by
    /// [`marketplaces`](Self::marketplaces).
    fn marketplaces_query(&self) -> Option<Vec<String>> {
        None
    }
    fn marketplaces(&self, _stdout: &str) -> Vec<Marketplace> {
        vec![]
    }
    /// The arguments that list the MCP servers the agent is set up with, read by
    /// [`servers`](Self::servers), and where one can be added, with how to say each.
    fn servers_query(&self) -> Option<Vec<String>> {
        None
    }
    fn servers(&self, _stdout: &str) -> Vec<ConfiguredServer> {
        vec![]
    }
    fn server_scopes(&self) -> Choices {
        vec![]
    }
    /// Lines that ask the program, started as for a session but with none, for the skills it
    /// offers in `cwd`; its stdin stays open until [`skills_answer`](Self::skills_answer) reads
    /// them. `None` when only a session tells ([`AgentCommand::Skills`]).
    fn skills_query(&self, _cwd: &std::path::Path) -> Option<Vec<String>> {
        None
    }
    /// The skills in a line the program wrote after [`skills_query`](Self::skills_query).
    fn skills_answer(&self, _line: &str) -> Option<Vec<Skill>> {
        None
    }
    /// The agent's own settings, shown on its page in Settings.
    fn options(&self) -> Vec<AgentOption> {
        vec![]
    }
    /// One process for all of the agent's sessions, when the person's options ask for it: how
    /// to share it among them. The process is the one [`AgentAdapter::launch`] gives for `spec`.
    fn mux(&self, _spec: &SessionSpec) -> Option<Box<dyn Mux>> {
        None
    }
    /// The agent's own logo, an SVG drawn in `currentColor`, to name it where quill lists agents (AGENTS.md).
    fn icon(&self) -> Option<&'static str> {
        None
    }
    /// Where a plugin can be installed and what a skill can be, with how to say each: see
    /// [`Extensions::scopes`] and [`Extensions::skill_states`].
    fn extension_words(&self) -> (Choices, Choices) {
        (vec![], vec![])
    }
    fn extension_kinds(&self) -> ExtensionKinds {
        ExtensionKinds::default()
    }
    /// What to run for `op`; `None` when the agent cannot.
    fn extension_op(&self, _op: &ExtensionOp) -> Option<ProgramRun> {
        None
    }
    /// Why `op` failed, in what its run printed although it exited well.
    fn extension_failed(&self, _stdout: &str) -> Option<String> {
        None
    }
    /// The ways to sign in to the agent's program, as (key, label) to [`login_run`](Self::login_run);
    /// empty when it has none of its own.
    fn sign_in_methods(&self) -> Choices {
        vec![]
    }
    /// The arguments that make the program say whether the person is signed in, read by
    /// [`auth_state`](Self::auth_state) with the exit code.
    fn auth_query(&self) -> Option<Vec<String>> {
        None
    }
    fn auth_state(&self, _stdout: &str, _exit: i32) -> AuthState {
        AuthState::Unknown(String::new())
    }
    /// What to run to sign in by `method`; the program prints a page address, waits for the
    /// code that page shows (or for the browser to come back to it) and exits.
    fn login_run(&self, _method: &str) -> Option<ProgramRun> {
        None
    }
    /// The page address in a line [`login_run`](Self::login_run) printed.
    fn login_url(&self, _line: &str) -> Option<String> {
        None
    }
    /// Whether the sign-in page shows a code the person pastes back; off, the browser returns
    /// to the program by itself and quill only waits.
    fn login_takes_code(&self) -> bool {
        true
    }
    /// Lines that ask the program, started as for a session but with none, which models it
    /// offers; its stdin stays open until [`models_answer`](Self::models_answer) reads them.
    /// `None` when only a session tells.
    fn models_query(&self) -> Option<Vec<String>> {
        None
    }
    /// The models in a line the program wrote after [`models_query`](Self::models_query).
    fn models_answer(&self, _line: &str) -> Option<Vec<ModelInfo>> {
        None
    }
    /// What goes to the program's stdin for the code the person pasted.
    fn login_code(&self, code: &str) -> String {
        format!("{code}\n")
    }
    fn logout_run(&self) -> Option<ProgramRun> {
        None
    }
    /// Lines for `command`. Commands the agent answers get an id, reported back as [`AgentEvent::Reply`].
    fn encode(&mut self, command: AgentCommand) -> Vec<String>;
    fn decode(&mut self, line: &str) -> Decoded;
}

/// The built-in browser's tools (docs/browser.md §4), served like [`HOST_TOOLS`] by a server of
/// their own, `browser`. Their meaning follows Claude Desktop's browser tools;
/// the words are quill's.
/// One dev server of a project's launch file (Claude Desktop's format):
/// `{"version": "0.0.1", "configurations": [{"name", "runtimeExecutable", "runtimeArgs",
/// "port", "url"?}]}`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchConfig {
    pub name: String,
    pub runtime_executable: String,
    #[serde(default)]
    pub runtime_args: Vec<String>,
    pub port: u16,
    /// Where the preview opens instead of http://localhost:<port>.
    #[serde(default)]
    pub url: Option<String>,
}

pub const BROWSER_TOOLS: [(&str, &str, &str); 19] = [
    (
        "navigate",
        "Open a URL in a tab of the built-in browser, which the user sees beside the conversation, or go \"back\" or \"forward\" in its history. Waits for the page to load. Local addresses (localhost, 127.0.0.1) are always allowed; other sites may need the user's permission.",
        r#"{"type":"object","properties":{"url":{"type":"string","description":"The address to open, or \"back\" / \"forward\"."},"tabId":{"type":"integer","description":"The tab to use; the one on screen when left out."}},"required":["url"]}"#,
    ),
    (
        "read_page",
        "The page as a tree of its elements, one per line: role, name, and a ref (ref_N) that other tools take to act on it. filter \"interactive\" keeps only the controls visible on screen. Start from ref_id to read part of a page.",
        r#"{"type":"object","properties":{"filter":{"type":"string","enum":["all","interactive"]},"ref_id":{"type":"string"},"depth":{"type":"integer","description":"How deep to go, 15 when left out."},"max_chars":{"type":"integer","description":"50000 when left out."},"tabId":{"type":"integer"}}}"#,
    ),
    (
        "find",
        "Look for elements whose role or name contains the words given, with the refs to act on them; at most 20.",
        r#"{"type":"object","properties":{"query":{"type":"string"},"tabId":{"type":"integer"}},"required":["query"]}"#,
    ),
    (
        "get_page_text",
        "The page's readable text: its main content when it has one, else the whole body.",
        r#"{"type":"object","properties":{"tabId":{"type":"integer"}}}"#,
    ),
    (
        "form_input",
        "Set the value of a form field by its ref: the text of an input or text area, the option of a select (by value or label), whether a checkbox or radio is checked (true/false).",
        r#"{"type":"object","properties":{"ref":{"type":"string"},"value":{"type":["string","boolean","number"]},"tabId":{"type":"integer"}},"required":["ref","value"]}"#,
    ),
    (
        "computer",
        "Look at and use the page as the user would. Actions: screenshot (a picture of the tab, at most 800 px wide; coordinates in later actions are in its pixels), left_click, right_click, double_click, triple_click (at coordinate, or at an element's ref), type (text into what has the focus), key (keys such as \"Enter\", \"Tab\", \"ctrl+a\"), scroll (scroll_direction up/down/left/right and scroll_amount ticks at coordinate), scroll_to (ref), hover, left_click_drag (from start_coordinate to coordinate), wait (duration seconds, at most 10).",
        r#"{"type":"object","properties":{"action":{"type":"string","enum":["screenshot","left_click","right_click","double_click","triple_click","type","key","scroll","scroll_to","hover","left_click_drag","wait"]},"coordinate":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"start_coordinate":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},"ref":{"type":"string"},"text":{"type":"string"},"scroll_direction":{"type":"string","enum":["up","down","left","right"]},"scroll_amount":{"type":"integer"},"duration":{"type":"number"},"tabId":{"type":"integer"}},"required":["action"]}"#,
    ),
    (
        "javascript_tool",
        "Run JavaScript in the page and get its value (awaited when it is a promise). The last expression is the value.",
        r#"{"type":"object","properties":{"text":{"type":"string","description":"The script."},"tabId":{"type":"integer"}},"required":["text"]}"#,
    ),
    (
        "read_console_messages",
        "What the page logged to its console since it opened, with uncaught errors; pattern keeps the lines that match it, clear forgets what was read.",
        r#"{"type":"object","properties":{"pattern":{"type":"string"},"only_errors":{"type":"boolean"},"clear":{"type":"boolean"},"tabId":{"type":"integer"}}}"#,
    ),
    (
        "read_network_requests",
        "The requests the page made, with their method and status; url_pattern keeps those whose address contains it, clear forgets what was read.",
        r#"{"type":"object","properties":{"url_pattern":{"type":"string"},"clear":{"type":"boolean"},"tabId":{"type":"integer"}}}"#,
    ),
    (
        "resize_window",
        "Size the tab's viewport, to check a layout at another width (a phone is 375 x 812, a tablet 768 x 1024). Width 0 gives the pane's size back.",
        r#"{"type":"object","properties":{"width":{"type":"integer"},"height":{"type":"integer"},"tabId":{"type":"integer"}},"required":["width","height"]}"#,
    ),
    (
        "tabs_context",
        "The browser's tabs: their ids, titles and addresses, and which is on screen.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "tabs_create",
        "Open a new tab, at url or blank, and put it on screen.",
        r#"{"type":"object","properties":{"url":{"type":"string"}}}"#,
    ),
    (
        "tabs_select",
        "Put a tab on screen.",
        r#"{"type":"object","properties":{"tabId":{"type":"integer"}},"required":["tabId"]}"#,
    ),
    (
        "tabs_close",
        "Close a tab.",
        r#"{"type":"object","properties":{"tabId":{"type":"integer"}},"required":["tabId"]}"#,
    ),
    (
        "preview_start",
        "Start a dev server of this project by its name in the project's launch file and open it in a tab of the built-in browser, or open url in a tab. Reuses the server if it already runs; the user approves a server the first time. Use this instead of Bash to run dev servers.",
        r#"{"type":"object","properties":{"name":{"type":"string","description":"The server's name in the launch file."},"url":{"type":"string","description":"An address to open instead of starting a server."}}}"#,
    ),
    (
        "preview_stop",
        "Stop a dev server started with preview_start.",
        r#"{"type":"object","properties":{"serverId":{"type":"string"}},"required":["serverId"]}"#,
    ),
    (
        "preview_list",
        "The dev servers started with preview_start: their ids, names, addresses and whether they run.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "preview_logs",
        "A dev server's output, its standard output and errors: level \"error\" keeps the error lines, search the lines that contain it.",
        r#"{"type":"object","properties":{"serverId":{"type":"string"},"level":{"type":"string","enum":["all","error"]},"search":{"type":"string"}},"required":["serverId"]}"#,
    ),
    (
        "browser_batch",
        "Run up to 25 of these tools one after another, stopping at the first that fails: a list of {\"tool\": name, \"input\": {...}}. For steps whose outcome you do not need to see in between.",
        r#"{"type":"object","properties":{"actions":{"type":"array","maxItems":25,"items":{"type":"object","properties":{"tool":{"type":"string"},"input":{"type":"object"}},"required":["tool"]}}},"required":["actions"]}"#,
    ),
];

/// How an agent is asked to work with the user, in words of quill's own: ask before work that
/// leaves room for doubt, plan what is large, keep a task list, check, and close with what is
/// left. Every adapter hands it to its agent.
pub const WORK_GUIDE: &str = "# Working with the user\n\n- Ask before real work when a request leaves room for doubt (what to build, which approach, how far to go): use your question tool with two to four concrete choices, the one you recommend first. Ask one decision at a time, and never what reading the code or the folder answers.\n- For a change that is large, risky or spread over many files, propose planning it first: enter plan mode when you have it, or lay out the plan and ask for approval before you edit.\n- For work of more than a few steps, keep a task list from your first tool call. Mark each step as you finish it, send the whole list again when the plan changes, and make the last step a check of the result.\n- Before you say something works, check it: run its tests or the program, and look at what is visible.\n- When the user wants something done after another conversation finishes, call wait_for_session instead of promising to wait: nothing of that conversation reaches you otherwise. Then end your turn.\n- When you finish, say in a few lines what changed, how to check it, and what you could not verify. If a decision is left to the user, end with one question with choices.";

/// What an agent is told of the built-in browser, in words of quill's own.
pub const BROWSER_GUIDE: &str = "# Built-in browser\n\nYou have a browser built into this app, with the browser tools (navigate, read_page, find, get_page_text, form_input, computer, javascript_tool, read_console_messages, read_network_requests, tabs_*). The user sees its tab beside the conversation and can use it too. Use it to check what you build for the web: start the dev server, open it, read the console and the page, act on it, and take a screenshot to show the result instead of asking the user to look. Prefer read_page and find to pick elements by ref; use screenshots to see layout. Local addresses are always allowed; other sites may ask the user first. Do not use it for what a plain fetch answers.";

/// One entry of an agent's task list.
#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    pub id: String,
    pub subject: String,
    /// "pending", "in_progress" or "completed".
    pub status: String,
}

/// The task list after one call of the task tools, as adapters name them: `TaskCreate` adds
/// (its id read from `output`, the CLI's "Task #1 created successfully: …"), `TaskUpdate`
/// changes or deletes by id, `TodoWrite` writes the whole list.
pub fn apply_task_tool(list: &mut Vec<Todo>, name: &str, input: &Value, output: Option<&str>) {
    match name {
        "TaskCreate" => {
            let id = output
                .and_then(|o| o.strip_prefix("Task #"))
                .and_then(|rest| rest.split(' ').next())
                .map(str::to_owned)
                .unwrap_or_else(|| (list.len() + 1).to_string());
            list.push(Todo {
                id,
                subject: input["subject"].as_str().unwrap_or_default().to_owned(),
                status: "pending".into(),
            });
        }
        "TaskUpdate" => {
            let id = input["taskId"].as_str().unwrap_or_default();
            if input["status"] == "deleted" {
                list.retain(|t| t.id != id);
            } else if let Some(todo) = list.iter_mut().find(|t| t.id == id) {
                if let Some(status) = input["status"].as_str() {
                    todo.status = status.to_owned();
                }
                if let Some(subject) = input["subject"].as_str() {
                    todo.subject = subject.to_owned();
                }
            }
        }
        "TodoWrite" => {
            *list = input["todos"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, t)| Todo {
                    id: (i + 1).to_string(),
                    subject: t["content"].as_str().unwrap_or_default().to_owned(),
                    status: t["status"].as_str().unwrap_or("pending").to_owned(),
                })
                .collect();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_tools_build_one_list() {
        let mut list = vec![];
        let create = serde_json::json!({ "subject": "Write it" });
        apply_task_tool(
            &mut list,
            "TaskCreate",
            &create,
            Some("Task #7 created successfully: Write it"),
        );
        assert_eq!(list[0].id, "7");
        let done = serde_json::json!({ "taskId": "7", "status": "completed" });
        apply_task_tool(&mut list, "TaskUpdate", &done, None);
        assert_eq!(list[0].status, "completed");
        let todos = serde_json::json!({ "todos": [{ "content": "a", "status": "completed" }, { "content": "b" }] });
        apply_task_tool(&mut list, "TodoWrite", &todos, None);
        assert_eq!((list.len(), list[1].status.as_str()), (2, "pending"));
    }

    #[test]
    fn wait_for_session_takes_a_session_and_what_to_do_then() {
        let schema = HOST_TOOLS
            .iter()
            .find(|(name, ..)| *name == "wait_for_session")
            .map(|(_, _, schema)| *schema)
            .expect("wait_for_session is a host tool");
        let schema: serde_json::Value = serde_json::from_str(schema).unwrap();
        assert_eq!(schema["properties"]["session_id"]["type"], "integer");
        assert_eq!(
            schema["required"],
            serde_json::json!(["session_id", "then"])
        );
        assert!(WORK_GUIDE.contains("wait_for_session"));
    }

    #[test]
    fn read_session_is_a_host_tool_with_a_session_id() {
        let schema = HOST_TOOLS
            .iter()
            .find(|(name, ..)| *name == "read_session")
            .map(|(_, _, schema)| *schema)
            .expect("read_session is a host tool");
        let schema: serde_json::Value = serde_json::from_str(schema).unwrap();
        assert_eq!(schema["properties"]["session_id"]["type"], "integer");
        assert_eq!(schema["required"], serde_json::json!(["session_id"]));
    }
}
