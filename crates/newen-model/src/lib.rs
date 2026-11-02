//! The contract between quill's UI and its engine (spec.md D7): the UI sends [`Intent`]s,
//! the engine answers with [`Event`]s. Neither side knows the other's crate.

use std::path::PathBuf;

pub use newen_agent::{
    apply_task_tool, AgentCapabilities, AgentEvent, AgentOption, AgentSlashCommand, AuthState,
    ConfiguredServer, ContextCategory, ContextKind, ContextUsage, Decision, ExtensionKinds,
    ExtensionOp, Extensions, FileChange, Marketplace, McpServer, McpSource, McpStatus, McpTool,
    ModeTone, ModeWords, ModelInfo, NewServer, PlanLimits, Plugin, PluginOffer, Skill, Suggestion,
    TaskUsage, Todo, ToolContent, ToolPolicy, ToolRule, Usage, UsageWindow, WebSearchRun,
    WebSource,
};

/// One environment variable or header of a global server. A secret lives in the system's
/// keychain: the file and the UI never hold its value (`value` is `None`); an intent that adds
/// the server carries it once, in `value`.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GlobalVar {
    pub key: String,
    pub value: Option<String>,
    #[serde(default)]
    pub secret: bool,
}

/// An MCP server of the "All agents" layer (`extensions.json`, docs/agents.md): quill adds it
/// to the sessions of every agent that can use it, but the agents' own files stay as they are.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GlobalServer {
    pub name: String,
    /// An address (else a command with `args`), by SSE rather than streamable HTTP.
    pub remote: bool,
    pub sse: bool,
    pub target: String,
    pub args: Vec<String>,
    pub env: Vec<GlobalVar>,
    pub headers: Vec<GlobalVar>,
    /// The agents the person turned it off for; a new agent gets it.
    #[serde(default)]
    pub except: Vec<String>,
}

/// A skill in quill's own folder (`<config>/quill/skills/<name>/SKILL.md`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GlobalSkill {
    pub name: String,
    pub description: String,
    pub except: Vec<String>,
}

/// What an agent does with one global server or skill.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Applies {
    Active,
    /// The person turned it off for this agent.
    Excluded,
    /// The agent has one of its own with the same name, which wins; the global one is not added.
    Replaced,
    /// The agent cannot use it, and why.
    Refused(String),
}

/// How one agent treats one global item.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AppliedTo {
    pub agent: String,
    pub name: String,
    pub skill: bool,
    pub applies: Applies,
}

/// When one tool of one connector may be used, as the person chose on its page: for one
/// agent's own server (`agent` its id) or for an "All agents" server (`agent` empty).
/// `policy` `None` leaves it to the agent's own settings again.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolChoice {
    pub agent: String,
    pub server: String,
    pub tool: String,
    pub policy: Option<ToolPolicy>,
}

/// Everything the "All agents" layer holds and how each agent treats it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GlobalState {
    pub servers: Vec<GlobalServer>,
    pub skills: Vec<GlobalSkill>,
    pub applied: Vec<AppliedTo>,
    /// The tool permissions the person chose, for every agent's connectors.
    #[serde(default)]
    pub tools: Vec<ToolChoice>,
    /// Why the last change failed, or why the layer could not be read.
    pub error: Option<String>,
    /// The system keychain answers; without it a secret cannot be kept.
    pub keychain: bool,
}

/// A change to the "All agents" layer; answered by `Event::Global`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GlobalOp {
    Load,
    /// Add or replace a server by its name.
    SetServer(GlobalServer),
    RemoveServer(String),
    /// Turn a server or skill on or off for one agent.
    Use {
        skill: bool,
        name: String,
        agent: String,
        on: bool,
    },
    /// Copy a folder with a `SKILL.md` into quill's skills.
    ImportSkill(PathBuf),
    NewSkill {
        name: String,
        description: String,
        /// What the agent reads when the skill applies; a placeholder when empty.
        #[serde(default)]
        instructions: String,
    },
    RemoveSkill(String),
    /// A skill from a file: a `SKILL.md`-like `.md`, or a `.zip` / `.skill` of its folder.
    /// With `replace`, it takes the place of that skill.
    UploadSkill {
        path: PathBuf,
        replace: Option<String>,
    },
    /// A skill's folder written as a `.zip` at `to`.
    ExportSkill {
        name: String,
        to: PathBuf,
    },
    /// Tool permissions chosen, each replacing the one before for the same tool.
    Tools(Vec<ToolChoice>),
}

/// quill's own id of a session.
pub type SessionId = u64;
/// quill's own id of one message the person sent; the agent receives it as the prompt's id.
pub type RequestId = String;

/// What a new session starts with.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct NewSession {
    pub agent: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
}

/// An MCP server the public registry offers: its names and words, and how to run it, by an
/// address or a command (`npx`, `uvx`, `docker`), with the settings it needs filled in.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServerOffer {
    /// The registry's name, `namespace/name`.
    pub name: String,
    pub title: Option<String>,
    pub description: String,
    pub version: String,
    pub repository: Option<String>,
    /// How it runs: the address it is reached at, or the command with its arguments.
    pub remote: bool,
    pub target: String,
    pub args: Vec<String>,
    /// What it needs set: environment variables, or headers for an address.
    pub env: Vec<String>,
    pub headers: Vec<String>,
}

/// How one file changed during a request.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed { from: String },
}

/// One file a request changed, whoever changed it (an edit tool or a shell command).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FileEffect {
    /// Relative to the session's folder.
    pub path: String,
    pub kind: ChangeKind,
    pub added: u32,
    pub removed: u32,
    /// The unified diff of this file for the request, cut when very long.
    pub patch: String,
}

/// The repository a folder is in: its branch, where it pushes, and the branch pull requests
/// go into.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Repo {
    pub branch: String,
    /// `origin`'s URL, when there is one.
    pub remote: Option<String>,
    /// The branch `origin` names as its own, such as `main`.
    pub base: Option<String>,
}

/// How quill shows an event that needs the person while its session is not in view. Desktop's
/// choices are Off, Badge only and Banners; its badge is the dock count,
/// quill's is its own marks in the sidebar and on the window (inferred).
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyLevel {
    Off,
    Badge,
    #[default]
    Banner,
}

/// What the person chose in Settings, as the engine keeps it in `settings.json`. A key it
/// does not know is dropped and a missing one takes its default, so an older or newer file
/// still reads.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The welcome was seen and its Get started pressed.
    #[serde(default)]
    pub welcomed: bool,
    /// The appearance step after the welcome was passed with Continue. A person who was
    /// welcomed before it existed is not asked.
    #[serde(default)]
    pub looked: bool,
    /// The agent the new-session page last chose; none takes the first one available.
    pub default_agent: Option<String>,
    /// The tour after the welcome was finished or skipped; it shows again only from the
    /// account menu.
    #[serde(default)]
    pub toured: bool,
    pub theme: Theme,
    /// The size of the conversation's words.
    pub text_size: TextSize,
    /// The view sessions open in, Desktop's "Default transcript view"; a session's own choice
    /// wins (`Session::view`).
    pub transcript_view: TranscriptView,
    /// How wide the conversation and the message box get, Desktop's "Transcript width".
    pub width: Width,
    /// The font of the whole interface; see `InterfaceFont`.
    pub interface_font: InterfaceFont,
    /// The monospace font of code and the terminal, by name, which `fc-match` resolves;
    /// empty takes the desktop's own monospace.
    pub code_font: String,
    /// The old "wide", read once into `width` (Medium, which it was).
    #[serde(skip_serializing)]
    pub wide: bool,
    /// A link of the conversation opens in the session's browser; off, in the computer's own.
    pub links_in_browser: bool,
    /// Sessions with nothing new for this many days are archived; 0 never.
    pub archive_after_days: u32,
    pub motion: Motion,
    /// Keep the computer from idle sleep while an agent works.
    pub keep_awake: bool,
    /// Keep the computer awake while it runs on battery too;
    /// off, quill lets it sleep on battery.
    pub keep_awake_on_battery: bool,
    /// Ask the desktop to draw attention to quill when a session finishes or waits on the
    /// person while quill is not in front.
    pub attention: bool,
    /// How a permission request is shown when its session is not in view (Desktop's
    /// "Permission requests").
    pub notify_permission: NotifyLevel,
    /// How a question is shown when its session is not in view (Desktop's "Questions").
    pub notify_question: NotifyLevel,
    /// How a finished turn is shown when its session is not in view (Desktop's "Task
    /// complete"); it has no badge, so `Badge` reads as `Banner`.
    pub notify_done: NotifyLevel,
    /// Whether a banner comes with quill's own sound (Desktop's "Notification sound", which
    /// plays the system's).
    pub notification_sound: bool,
    /// A pull request opens as a draft unless the person picks otherwise.
    pub pr_draft: bool,
    /// The prefix of the branch names agents are asked to create, without its `/` (Desktop's
    /// "Branch prefix", whose default is the app's own name); empty takes "quill".
    pub branch_prefix: String,
    /// Open a pull request's link in the computer's default browser even while links open in
    /// the built-in one (Desktop's "Open pull requests in browser"); off is inferred.
    pub prs_in_browser: bool,
    /// Archive a session once its pull request is merged (Desktop's "Auto-archive after PR
    /// merge"); off is inferred.
    pub archive_on_merge: bool,
    /// Start quill when the person logs in.
    pub open_at_login: bool,
    /// Sites the agents may use in the built-in browser without asking; local addresses
    /// never ask (docs/browser.md §5).
    pub browser_sites: Vec<String>,
    /// The Chromium the built-in browser runs; `None` takes `QUILL_CHROME` or the first on
    /// `PATH`.
    pub browser_program: Option<String>,
    /// Draw the browser's pages with Chromium in quill's own process, without the headless
    /// one's delay. From the next start.
    pub browser_native: bool,
    /// The editor files open in: the last chosen in Open in, "quill-ide" or an editor's URL
    /// scheme ("vscode", "cursor", "windsurf", "zed").
    pub editor: String,
    /// Each agent's own, by its id.
    pub agents: std::collections::BTreeMap<String, AgentSettings>,
    /// Look up logos of connectors and plugins (svgl.app, then the server's own site); off,
    /// quill asks for none and shows its icons.
    pub logos: bool,
    /// Download a new version of quill in the background once one is out; installing always
    /// waits for the person. A mandatory version downloads whatever this says.
    pub auto_update: bool,
    /// Make every new session reachable from another device as it starts, for agents that
    /// offer it (Desktop's "Connect new sessions to Remote Control").
    pub remote_new_sessions: bool,
    /// Show a release's announcement before restarting into it and once after; off, the
    /// release notes are still a click away.
    pub whats_new: bool,
    /// Whether the person let quill download Cisco's OpenH264 codec to play the clips of What's
    /// new: `None` while never asked, so nothing is downloaded until they say yes.
    pub video_codec: Option<bool>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            welcomed: false,
            looked: false,
            default_agent: None,
            toured: false,
            theme: Theme::System,
            text_size: TextSize::Medium,
            transcript_view: TranscriptView::Normal,
            width: Width::Narrow,
            interface_font: InterfaceFont::System,
            code_font: String::new(),
            wide: false,
            links_in_browser: true,
            archive_after_days: 0,
            motion: Motion::System,
            // Desktop keeps the computer awake by default and does not flash the app icon; attention starts on with the banners (inferred).
            keep_awake: true,
            keep_awake_on_battery: true,
            attention: true,
            notify_permission: NotifyLevel::Banner,
            notify_question: NotifyLevel::Banner,
            notify_done: NotifyLevel::Banner,
            notification_sound: true,
            pr_draft: false,
            branch_prefix: String::new(),
            prs_in_browser: false,
            archive_on_merge: false,
            open_at_login: false,
            browser_sites: vec![],
            browser_program: None,
            browser_native: false,
            editor: "quill-ide".into(),
            agents: Default::default(),
            logos: true,
            auto_update: true,
            remote_new_sessions: false,
            whats_new: true,
            video_codec: None,
        }
    }
}

impl Settings {
    pub fn agent(&self, agent: &str) -> AgentSettings {
        self.agents.get(agent).cloned().unwrap_or_default()
    }

    /// The branch prefix without its trailing `/`, or "quill" when none is set.
    pub fn branch_prefix(&self) -> &str {
        let prefix = self.branch_prefix.trim().trim_end_matches('/');
        if prefix.is_empty() {
            "quill"
        } else {
            prefix
        }
    }
}

/// What the person chose for one agent.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    /// The agent's program, when it is not the one on `PATH`.
    pub program: Option<String>,
    /// What a new session starts with; `None` leaves it to the agent.
    pub model: Option<String>,
    pub effort: Option<String>,
    pub mode: Option<String>,
    /// Offer the agent's modes that skip every approval.
    pub allow_unsafe: bool,
    /// The adapter's own options as chosen, by key; one not chosen takes its default.
    pub options: std::collections::BTreeMap<String, String>,
    /// `KEY=value`s every session of the agent starts with, as Desktop's "Environment
    /// variables".
    pub env: Vec<String>,
}

/// Desktop's "Transcript width": its 48rem, 960 px and 1280 px columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Width {
    #[default]
    Narrow,
    Medium,
    Wide,
}

/// Desktop's "Interface font", which offers Anthropic Sans, System and
/// OpenDyslexic. quill does not ship Anthropic's face: System is the desktop's sans-serif and
/// Ubuntu is the face limn bundles. OpenDyslexic is left out, as it would mean bundling it.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InterfaceFont {
    #[default]
    System,
    Ubuntu,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextSize {
    Small,
    #[default]
    Medium,
    Large,
}

/// What the transcript shows: Normal the answers and the work, Thinking the model's words
/// too, Verbose also every collapsible open [Desktop's transcript views, docs/ui.md].
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptView {
    #[default]
    Normal,
    Thinking,
    Verbose,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    #[default]
    System,
    Reduced,
    Full,
}

/// What of Customize an agent takes part in, as its adapter declares it: a tab it has no part
/// in shows off, with why, and a tool permission it cannot say is not offered.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CustomizeSupport {
    pub plugins: bool,
    pub connectors: bool,
    pub tool_policies: Vec<ToolPolicy>,
    /// A server can be added with an OAuth client of the person's own.
    pub oauth_client: bool,
}

impl Default for CustomizeSupport {
    fn default() -> Self {
        Self {
            plugins: true,
            connectors: true,
            tool_policies: ToolPolicy::ALL.to_vec(),
            oauth_client: false,
        }
    }
}

/// An agent quill can run: its id, its name for people, and the versions of its program
/// quill was tested with.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub tested: Vec<String>,
    /// Its own settings, as its adapter declares them.
    #[serde(default)]
    pub options: Vec<AgentOption>,
    /// Its logo, an SVG in `currentColor`, as its adapter ships it.
    #[serde(default)]
    pub icon: Option<String>,
    /// The ways to sign in to its program, as (key, label); none when it has no sign-in of its own.
    #[serde(default)]
    pub sign_in_methods: Vec<(String, String)>,
    /// Its sign-in page shows a code to paste back; off, the browser returns by itself.
    #[serde(default)]
    pub sign_in_code: bool,
    /// Its permission modes in its own words, unsafe ones included; the menu picks marks from
    /// each one's tone.
    #[serde(default)]
    pub modes: Vec<ModeWords>,
    /// The mode a new session starts in.
    #[serde(default)]
    pub default_mode: String,
    #[serde(default)]
    pub customize: CustomizeSupport,
}

/// Where a message sent while a turn runs goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lane {
    /// Into the running turn: the agent reads it at its next step.
    #[default]
    Steer,
    /// Waits in the queue until the turn ends.
    Queue,
}

/// A message held in a session's queue, not yet sent.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct QueuedMessage {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub images: Vec<PathBuf>,
    /// The other sessions the message names, as `Intent::Send` has them.
    #[serde(default)]
    pub mentions: Vec<(SessionId, String)>,
    /// Restored from an earlier run: it is not sent until the person sends it.
    #[serde(default)]
    pub held: bool,
}

/// A connector or plugin whose logo is wanted: its key (`mcp:<name>` or `plugin:<name>`), the
/// name to look up, and the server's address when it has one.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LogoWanted {
    pub key: String,
    pub name: String,
    pub url: Option<String>,
}

/// Whether the person is signed in to GitHub through the `gh` program, which keeps the
/// credentials (in the system's keychain): GitHub is the person's identity, not an agent.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GithubState {
    /// `gh` is not installed; the text says how to install it on this system.
    Missing(String),
    SignedOut,
    SignedIn {
        login: String,
        name: Option<String>,
        /// The picture, kept as a PNG under quill's data folder once downloaded.
        avatar: Option<PathBuf>,
    },
    /// `gh` answered something that was not understood, kept as its words.
    Unknown(String),
}

/// One day of the contribution calendar.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContributionDay {
    /// `YYYY-MM-DD`.
    pub date: String,
    pub count: u32,
    /// GitHub's intensity, 0 (none) to 4.
    pub level: u8,
}

/// The last year of contributions: weeks from Sunday to Saturday, a missing day (outside the
/// period) as `None`. Ported from openADE's `GithubContributionCalendar`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ContributionCalendar {
    pub login: String,
    pub total: u32,
    pub weeks: Vec<[Option<ContributionDay>; 7]>,
}

/// What the UI asks of the engine.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Intent {
    NewSession(NewSession),
    Send {
        session: SessionId,
        text: String,
        /// The other sessions the message names (`@[title]`), by first appearance: the agent
        /// is told which session each is, while the stored text stays as typed.
        #[serde(default)]
        mentions: Vec<(SessionId, String)>,
        /// Image files the message attaches (pasted ones saved under quill's data folder,
        /// or picked and dropped ones where they are); the engine reads and sizes them.
        #[serde(default)]
        images: Vec<PathBuf>,
        /// Where the message goes when a turn is running (Enter steers, Ctrl+Enter queues).
        #[serde(default)]
        lane: Lane,
    },
    /// Take a queued message out of the queue, without sending it.
    QueueRemove {
        session: SessionId,
        id: String,
    },
    /// Remove every queued message.
    QueueClear {
        session: SessionId,
    },
    /// Send a queued message now: as a steer when a turn runs, as a message when idle.
    QueueSendNow {
        session: SessionId,
        id: String,
    },
    /// Take a queued message back into the composer: answered by `Event::QueueTaken`.
    QueueTake {
        session: SessionId,
        id: String,
    },
    Decide {
        session: SessionId,
        permission: String,
        decision: Decision,
    },
    Interrupt {
        session: SessionId,
    },
    SetMode {
        session: SessionId,
        mode: String,
    },
    SetModel {
        session: SessionId,
        model: String,
    },
    /// How hard the agent thinks, one of the levels its model accepts.
    SetEffort {
        session: SessionId,
        effort: String,
    },
    /// How the session's context is spent, answered by the agent's `ContextUsage` while it
    /// runs; a session whose agent is not running gets no answer.
    ContextUsage {
        session: SessionId,
    },
    /// The session's MCP servers, answered by the agent's `McpServers` while it runs.
    McpStatus {
        session: SessionId,
    },
    /// Connect one of the session's MCP servers again.
    McpReconnect {
        session: SessionId,
        name: String,
    },
    /// Stop a task that runs beside the session's turn, by its id.
    StopTask {
        session: SessionId,
        task: String,
    },
    /// Sign in to one of the session's MCP servers: the agent gives the page, which the
    /// engine opens in the browser.
    McpAuthenticate {
        session: SessionId,
        name: String,
    },
    /// Forget the credentials of one of the session's MCP servers.
    McpClearAuth {
        session: SessionId,
        name: String,
    },
    /// Turn one of the session's MCP servers on or off.
    McpToggle {
        session: SessionId,
        name: String,
        enabled: bool,
    },
    /// Let the person reach the session from another device, listed there as `name`, or stop.
    Remote {
        session: SessionId,
        on: bool,
        name: Option<String>,
    },
    /// A code to pair another device with the session's agent, for agents that pair.
    RemotePair {
        session: SessionId,
    },
    /// What is uncommitted in the session's folder, answered by `Event::WorkingTree`.
    WorkingTree {
        session: SessionId,
    },
    /// Put the session's files back as they were before `request` ran.
    Rewind {
        session: SessionId,
        request: RequestId,
    },
    /// Desktop's "Rewind to here": take the conversation back to before the person's
    /// message `request`, the agent going on from its message `at`. Answered by
    /// `Event::RewoundTo`.
    RewindTo {
        session: SessionId,
        request: RequestId,
        at: Option<String>,
    },
    /// Undo the last rewind: its messages come back and the agent goes on in the
    /// conversation it had. Answered by `Event::RewindUndone`; not once a message was sent.
    UndoRewind {
        session: SessionId,
    },
    /// The person's own name for the session, in place of the one its first message gave.
    Rename {
        session: SessionId,
        title: String,
    },
    /// Show an OS banner about `session`, from the computer's own notifier, with quill's own
    /// sound if any (the banner itself stays silent). A click on it is
    /// `Event::NotificationClicked`, where the system tells it.
    Notify {
        session: SessionId,
        title: String,
        body: String,
        sound: Option<Chime>,
    },
    /// Keep the session at the top of the list, or stop keeping it there.
    Pin {
        session: SessionId,
        pinned: bool,
    },
    /// The transcript view the session is drawn in, or `None` to follow the default again.
    ChooseView {
        session: SessionId,
        view: Option<TranscriptView>,
    },
    /// Remove these finished tasks from the session's list.
    ClearTasks {
        session: SessionId,
        tasks: Vec<String>,
    },
    /// Stop the session's agent and remove everything quill kept of it.
    Delete {
        session: SessionId,
    },
    /// Take the session out of the list, or put it back.
    Archive {
        session: SessionId,
        archived: bool,
    },
    /// Mark the session as having news the person has not seen, or as seen.
    MarkUnread {
        session: SessionId,
        unread: bool,
    },
    /// A new session that continues this one's conversation, leaving it as it was: all of
    /// it, or up to the message `at` (the agent's own id of it), as Desktop's "Fork from here".
    Fork {
        session: SessionId,
        #[serde(default)]
        at: Option<String>,
    },
    /// A file of the session's folder, by its path in it, for the document pane: answered by
    /// `Event::FileRead`.
    ReadFile {
        session: SessionId,
        path: String,
    },
    /// The entries of a folder of the session's, for the Files pane: answered by
    /// `Event::Listed`. Only a folder inside the session's is listed.
    ListDir {
        session: SessionId,
        dir: std::path::PathBuf,
    },
    /// The Files pane's filter: the session's files whose paths match `query`, or with
    /// `contents` the lines that hold it. Answered by `Event::FilesFound`.
    FindFiles {
        session: SessionId,
        query: String,
        contents: bool,
    },
    /// A file operation in the session's folder, from the Files pane: answered by
    /// `Event::FileOpDone`.
    FileOp {
        session: SessionId,
        op: newen_files::Op,
    },
    /// Write what the person edited over a file of the session's folder.
    WriteFile {
        session: SessionId,
        path: String,
        text: String,
    },
    /// Keep these settings and act on them (start at login, keep awake…); answered by
    /// `Event::Settings`.
    SaveSettings(Settings),
    /// How much room quill's own data takes; answered by `Event::Storage`.
    MeasureStorage,
    /// Logos for connectors and plugins not yet kept, each by its key; answered by one
    /// `Event::Logo` per key, when it is found or not.
    Logos {
        wanted: Vec<LogoWanted>,
    },
    /// An agent's plugins, marketplaces and skills for a folder, outside any session;
    /// answered by `Event::Extensions`.
    Extensions {
        agent: String,
        folder: PathBuf,
    },
    /// Change quill's "All agents" layer of servers and skills, or read it; answered by
    /// `Event::Global`.
    Global(GlobalOp),
    /// Look for a newer quill now; answered by `Event::Update`.
    CheckUpdates,
    /// Download the newer quill found, if it is not here yet; answered by `Event::Update`.
    DownloadUpdate,
    /// Install the downloaded quill and start it in place of this one; answered by
    /// `Event::Update` with `Installed`, then the host restarts.
    InstallUpdate,
    /// Never offer the optional version found again (a newer one is offered); answered by
    /// `Event::Update`. A mandatory one cannot be skipped.
    SkipUpdate,
    /// The window gained or lost the focus: a check long due is made, and a ready update
    /// is announced by the system only while the window is away.
    WindowFocused {
        focused: bool,
    },
    /// The person opened the release notes: this version's "What's new" button goes; answered
    /// by `Event::WhatsNew` once that is kept.
    WhatsNewOpened,
    /// The tools of an agent's MCP servers, asked of a short start of its program with no
    /// session; answered by `Event::ServerTools`.
    ServerTools {
        agent: String,
        folder: PathBuf,
    },
    /// The MCP servers an agent is set up with for a folder, outside any session; answered by
    /// `Event::Servers`. Listing them can start them, to see how they stand.
    Servers {
        agent: String,
        folder: PathBuf,
    },
    /// Ask an agent's program whether the person is signed in; answered by `Event::Auth`.
    CheckSignIn {
        agent: String,
    },
    /// Sign in to an agent by one of its `sign_in_methods`; its program opens the page in the
    /// browser. Answered by `Event::SignInUrl`, then `Event::Auth` or `Event::SignInFailed`.
    SignIn {
        agent: String,
        method: String,
    },
    /// The code the sign-in page showed, for the sign-in under way.
    SubmitSignInCode {
        agent: String,
        code: String,
    },
    CancelSignIn {
        agent: String,
    },
    /// Sign out of an agent's program (it is signed out wherever that program runs).
    SignOut {
        agent: String,
    },
    /// Ask `gh` who is signed in to GitHub; answered by `Event::Github`.
    CheckGithub,
    /// Sign in to GitHub through `gh`'s browser flow. Answered by `Event::GithubCode`, then
    /// `Event::Github` or `Event::GithubFailed`.
    ConnectGithub,
    /// Abandon the GitHub sign-in under way (its process is killed).
    CancelGithub,
    /// Sign `login` out of GitHub in `gh` (the terminal's `gh` is signed out too).
    DisconnectGithub {
        login: String,
    },
    /// The last year of contributions of `login`, the account the person sees; answered by
    /// `Event::GithubContributions`.
    GithubContributions {
        login: String,
    },
    /// Servers the public MCP registry lists for `query`; answered by `Event::ServersFound`.
    FindServers {
        query: String,
    },
    /// Change an agent's plugins, marketplaces or skills; its running sessions load them
    /// again, and `Event::Extensions` follows.
    ExtensionOp {
        agent: String,
        folder: PathBuf,
        op: ExtensionOp,
    },
    /// Read a reply aloud with the system's voice, stopping any other; `None` stops.
    ReadAloud {
        session: SessionId,
        text: Option<String>,
    },
    /// A side question (`/btw`): a fork of the session, opened beside it, that answers without
    /// adding to its conversation.
    SideChat {
        session: SessionId,
        text: String,
    },
    /// Stop `session` waiting for `on` (`wait_for_session`): nothing wakes it then.
    CancelWait {
        session: SessionId,
        on: SessionId,
    },
    /// The person's answer to a task the session proposed: start it, do it here, or not.
    AnswerProposal {
        session: SessionId,
        task: String,
        answer: ProposalAnswer,
    },
    /// Something the person does in the session's browser (docs/browser.md); answered by
    /// `Event::Browser`.
    Browser {
        session: SessionId,
        op: BrowserOp,
    },
}

/// The tool a site permission's card names: the built-in browser asks, not the agent, before
/// the agent opens or uses a site that is not local nor allowed (docs/browser.md §5).
pub const SITE_PERMISSION: &str = "quill.browser.site";

/// What the person does in a session's browser.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BrowserOp {
    /// Show the browser: its tabs, or a first one at `url` (a blank one without).
    Open {
        url: Option<String>,
    },
    Navigate {
        tab: u32,
        url: String,
    },
    NewTab {
        url: Option<String>,
    },
    CloseTab {
        tab: u32,
    },
    SelectTab {
        tab: u32,
    },
    Back {
        tab: u32,
    },
    Forward {
        tab: u32,
    },
    Reload {
        tab: u32,
    },
    Stop {
        tab: u32,
    },
    /// The tab on screen and the size of its view in logical pixels, or none on screen:
    /// only what is shown is drawn.
    Show {
        view: Option<BrowserView>,
    },
    Input {
        tab: u32,
        input: BrowserInput,
    },
    /// Copy what is selected in the page to the clipboard (`BrowserUpdate::Copied`).
    Copy {
        tab: u32,
    },
    /// Draw the page at `size` as a phone or a tablet would, or at the pane's size again
    /// (Desktop's Viewport menu: Responsive, Mobile, Tablet).
    Viewport {
        tab: u32,
        size: Option<(u32, u32)>,
    },
    /// Pick an element of the page to tell the agent about, or stop picking (Desktop's
    /// "Select element"): the pick comes back as `BrowserUpdate::Selected`.
    Select {
        tab: u32,
        on: bool,
    },
    /// The files the person chose for the page's file field `node`
    /// (`BrowserUpdate::ChooseFiles`); none leaves it as it was.
    Files {
        tab: u32,
        node: i64,
        paths: Vec<PathBuf>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrowserView {
    pub tab: u32,
    pub width: u32,
    pub height: u32,
}

/// The pointer or the keyboard in a page, in logical pixels from the view's corner.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BrowserInput {
    Move {
        x: f32,
        y: f32,
    },
    Leave,
    Down {
        x: f32,
        y: f32,
        button: PointerButton,
        clicks: u8,
        keys: Modifiers,
    },
    Up {
        x: f32,
        y: f32,
        button: PointerButton,
        keys: Modifiers,
    },
    Wheel {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    },
    /// A key pressed and released: the DOM's `key` and `code`, the Windows key code Chrome
    /// edits by, and the text it types, if any.
    Key {
        key: String,
        code: String,
        key_code: u32,
        text: String,
        keys: Modifiers,
    },
    /// Text typed or pasted, or committed by an input method.
    Text(String),
    /// What an input method is composing, shown in place until it commits.
    Composition(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// One tab of a session's browser.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrowserTab {
    pub id: u32,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub back: bool,
    pub forward: bool,
    /// The viewport set for it, by the person or the agent, instead of the pane's size.
    #[serde(default)]
    pub emulated: Option<(u32, u32)>,
    /// Whether the agent set `emulated`, rather than the person from the Viewport menu.
    #[serde(default)]
    pub emulated_by_agent: bool,
}

/// What changed in a session's browser.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BrowserUpdate {
    Tabs {
        tabs: Vec<BrowserTab>,
        active: Option<u32>,
    },
    /// The pointer's shape over the page, as CSS names it ("pointer", "text"…).
    Cursor(String),
    /// What was selected in the page, for the clipboard.
    Copied(String),
    /// Something the person should know: a dialog the page opened, a download saved.
    Notice(String),
    /// The browser could not start or stopped.
    Failed(String),
    /// Whether the page waits for the person to pick an element.
    Selecting(bool),
    /// The element the person picked, as words for the message being written.
    Selected(String),
    /// The addresses of the session's tabs changed: kept, so they open again after a restart
    /// (the only browser update that is stored).
    Kept(Vec<String>),
    /// The page asks for files: the host shows the desktop's file dialog and answers with
    /// `BrowserOp::Files`.
    ChooseFiles { tab: u32, node: i64, multiple: bool },
}

/// What the person does with a proposed task.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ProposalAnswer {
    /// In a session of its own, which reports back to the one that proposed it.
    Start,
    /// In the session that proposed it: its brief is sent there.
    Here,
    Dismiss,
}

/// A task a session proposed to run in a session of its own; nothing runs until the person
/// starts it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Proposal {
    pub id: String,
    pub title: String,
    /// The brief the new session starts from.
    pub prompt: String,
    /// The goal, decisions and constraints so far, for a session that starts fresh.
    pub background: Option<String>,
    /// The new session continues this whole conversation instead of starting fresh.
    pub fork: bool,
    /// The agent that runs it, by id; `None` is the proposing session's own.
    #[serde(default)]
    pub agent: Option<String>,
}

/// What became of a proposal.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ProposalOutcome {
    Started { session: SessionId },
    Kept,
    Dismissed,
}

/// What a session another one started said when its turn ended.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Report {
    pub from: SessionId,
    pub title: String,
    pub text: String,
    /// Sent because a wait on `from` ended (`wait_for_session`): how, and what the waiting
    /// session said it would do then. `None` for a session it started.
    #[serde(default)]
    pub waited: Option<(WaitOutcome, String)>,
}

/// A session waiting for another's turn to end with its task done (`wait_for_session`), so it
/// goes on by itself then.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Wait {
    /// The session waited for.
    pub on: SessionId,
    /// Its name when the wait began, for the message that wakes the waiting one.
    pub title: String,
    /// What the waiting session's agent said it would do then, in its own words.
    pub then: String,
    /// When the wait began, in seconds since 1970.
    pub at: u64,
}

/// How a wait ended.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum WaitOutcome {
    /// The session waited for finished its task.
    Finished,
    /// It was deleted or archived before it finished.
    Gone,
    /// The person stopped the wait.
    Canceled,
}

/// Where a session's agent process is.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SessionStatus {
    Starting,
    Idle,
    Running,
    /// Waiting for the person to decide a permission or answer a question.
    Awaiting,
    /// The process ended; the next message starts it again, resuming the conversation.
    Exited,
    Failed(String),
}

/// Where quill's own update stands, as Settings and the account menu show it. Never kept in
/// the session log: it is the moment's.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum UpdateState {
    /// Nothing checked yet.
    Idle,
    Checking,
    /// The last check found nothing newer; `checked_at` in seconds since 1970.
    UpToDate {
        checked_at: u64,
    },
    /// A newer version is published; `notes` is the annotated tag's message (Markdown).
    Available {
        version: String,
        notes: String,
        mandatory: bool,
        size: u64,
    },
    Downloading {
        version: String,
        done: u64,
        total: u64,
        mandatory: bool,
    },
    /// Downloaded and verified: installing it only waits for the person.
    Ready {
        version: String,
        mandatory: bool,
    },
    /// Installed; the host starts `program` and closes this window.
    Installed {
        version: String,
        program: PathBuf,
    },
    /// A check, a download or an installation did not go through; `reason` is for the person.
    Failed {
        reason: String,
    },
    /// Built from source (under `target/`, or `QUILL_NO_UPDATE` set): updated with `git pull`.
    FromSource,
    /// A binary that `quill --install` never placed: nothing to update in place.
    NotInstalled,
}

/// The release notes quill keeps (docs/UPDATES.md): each downloaded with its update, the
/// media matched against their digests, kept in the data folder.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WhatsNew {
    /// This version's and earlier ones', newest first.
    pub releases: Vec<ReleaseNotes>,
    /// The downloaded version's, which a restart installs.
    pub ready: Option<ReleaseNotes>,
    /// The first of `releases` is this version, just updated to: its announcement shows now,
    /// once (it is already kept as seen).
    pub announce: bool,
    /// This version came by an update and its notes were never opened: the "What's new"
    /// button shows.
    pub unopened: bool,
}

/// Cisco's OpenH264 library, which plays the clips of What's new once the person allows its
/// download (`Settings::video_codec`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum CodecState {
    /// Not allowed, or not there.
    #[default]
    Off,
    Downloading,
    /// Downloaded from Cisco and matched against its digest: the library to load.
    Ready(PathBuf),
    /// The download or the check failed; `reason` is for the person.
    Failed(String),
}

/// What one release says is new in it.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ReleaseNotes {
    pub version: String,
    /// As its author wrote it (`2026-11-10`).
    pub date: String,
    pub new: Vec<String>,
    pub improved: Vec<String>,
    pub fixed: Vec<String>,
    pub announcement: Option<Announcement>,
}

/// The one thing a release puts forward.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Announcement {
    pub title: String,
    pub body: String,
    pub bullets: Vec<String>,
    /// Its picture or clip, on this computer and matched; none when none matched.
    pub media: Option<Media>,
    /// Where "Try it" takes the person: one of the interface's own targets, or no button.
    pub try_it: Option<String>,
}

/// An announcement's picture or clip.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Media {
    Image {
        path: PathBuf,
    },
    /// A clip and the picture shown while it cannot play.
    Video {
        poster: PathBuf,
        clip: PathBuf,
    },
}

/// quill's own sounds, made by the engine: one when a task completes, one when a session
/// needs the person.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Chime {
    Done,
    Ask,
}

/// What the engine tells the UI.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Event {
    SessionCreated {
        session: SessionId,
        agent: String,
        cwd: PathBuf,
    },
    Status {
        session: SessionId,
        status: SessionStatus,
    },
    /// The engine took a message and gave it its id; it is sent to the agent at once.
    Queued {
        session: SessionId,
        request: RequestId,
        text: String,
        /// When it was sent, in seconds since 1970; older logs lack it.
        #[serde(default)]
        at: Option<u64>,
        /// The image files it attached, as sent (before any resizing).
        #[serde(default)]
        images: Vec<PathBuf>,
    },
    /// The session's whole queue of messages not yet sent, each time it changes.
    QueueChanged {
        session: SessionId,
        entries: Vec<QueuedMessage>,
    },
    /// A queued message taken back for editing: its text and images go to the composer.
    QueueTaken {
        session: SessionId,
        text: String,
        images: Vec<PathBuf>,
    },
    /// Something the person should know about a message, not stored: an image left out.
    Notice {
        session: SessionId,
        text: String,
    },
    /// Something the agent said, tied to the request it answers when known.
    Agent {
        session: SessionId,
        request: Option<RequestId>,
        event: AgentEvent,
    },
    /// What files a finished request created, changed, removed or moved.
    Changes {
        session: SessionId,
        request: RequestId,
        files: Vec<FileEffect>,
        /// The folder before the request, as the engine's snapshot id: what a rewind restores.
        snapshot: String,
    },
    /// Where the session works, as the right panel shows it.
    Environment {
        session: SessionId,
        branch: Option<String>,
    },
    /// What is uncommitted in the session's folder now, against its branch's last commit,
    /// and the repository it is in, if it is in one.
    WorkingTree {
        session: SessionId,
        files: Vec<FileEffect>,
        repo: Option<Repo>,
    },
    /// The person answered a permission; kept so a session read back shows it answered.
    Decided {
        session: SessionId,
        permission: String,
        decision: Decision,
    },
    /// The conversation taken back to before the person's message `request`: it and what
    /// followed are gone, and the agent goes on from its message `at` (from the start
    /// without one). The files stay as they are.
    RewoundTo {
        session: SessionId,
        request: RequestId,
        at: Option<String>,
    },
    /// The last rewind taken back: its messages are there again and the agent goes on in
    /// the conversation it had.
    RewindUndone {
        session: SessionId,
    },
    /// The model or the effort the session runs at, kept so it goes on with them: chosen
    /// when it was made and each time the person changed them.
    ModelChosen {
        session: SessionId,
        model: String,
    },
    EffortChosen {
        session: SessionId,
        effort: String,
    },
    /// The session's files were put back as they were before `request`.
    Rewound {
        session: SessionId,
        request: RequestId,
        files: Vec<FileEffect>,
    },
    Renamed {
        session: SessionId,
        title: String,
    },
    Pinned {
        session: SessionId,
        pinned: bool,
    },
    /// The session's transcript view, as chosen; `None` follows the default.
    ViewChosen {
        session: SessionId,
        view: Option<TranscriptView>,
    },
    /// The person cleared these finished tasks from the session's list.
    TasksCleared {
        session: SessionId,
        tasks: Vec<String>,
    },
    /// Nothing of the session is kept any more.
    Deleted {
        session: SessionId,
    },
    Archived {
        session: SessionId,
        archived: bool,
    },
    /// The session's pull request, as the agent gave its address.
    PullRequest {
        session: SessionId,
        url: String,
    },
    Unread {
        session: SessionId,
        unread: bool,
    },
    /// The session continues `from`'s conversation: what `from` showed until now, or up to
    /// its message `at`, is its start.
    Forked {
        session: SessionId,
        from: SessionId,
        #[serde(default)]
        at: Option<String>,
    },
    /// A file of the session's folder as it is now on disk; `None` when it cannot be read as
    /// text, or is outside the folder.
    FileRead {
        session: SessionId,
        path: String,
        text: Option<String>,
    },
    /// A folder of the session's as it is now on disk, and when it last changed.
    Listed {
        session: SessionId,
        dir: std::path::PathBuf,
        entries: Vec<newen_files::Entry>,
        stamp: newen_files::Stamp,
    },
    /// What the filter asked for `query` found: the best paths from the folder, or with
    /// `contents` the lines that hold it.
    FilesFound {
        session: SessionId,
        query: String,
        contents: bool,
        paths: Vec<std::path::PathBuf>,
        hits: Vec<newen_files::find::Hit>,
    },
    /// A file operation done, with what it made (paths in the session's folder), or why not.
    FileOpDone {
        session: SessionId,
        op: newen_files::Op,
        result: Result<Vec<std::path::PathBuf>, String>,
    },
    /// Writing a file the person edited failed, and why.
    FileWriteFailed {
        session: SessionId,
        path: String,
        error: String,
    },
    /// The settings in force, and the agents quill can run. Not a session's: it is sent at
    /// start and after each save.
    Settings {
        settings: Settings,
        agents: Vec<AgentInfo>,
    },
    /// Every kept session has been told again: what follows is new, not history.
    Restored,
    /// The logo kept for a connector or plugin `key`, as light and dark PNG files under quill's
    /// data folder; `None` where none was found. Not session state: never stored.
    Logo {
        key: String,
        light: Option<PathBuf>,
        dark: Option<PathBuf>,
    },
    /// Where quill keeps its data and how many bytes it takes.
    Storage {
        path: PathBuf,
        bytes: u64,
    },
    /// Settings could not be written: what is in force stays as it was.
    SettingsNotSaved {
        error: String,
    },
    /// Where quill's own update stands; not stored.
    Update(UpdateState),
    /// A check for quill's updates ended at `at` (seconds since 1970), with `error` when it
    /// failed; not stored.
    UpdateChecked {
        at: u64,
        error: Option<String>,
    },
    /// The release notes quill keeps, and whether to announce this version; not stored.
    WhatsNew(WhatsNew),
    /// Where the video codec stands on this computer; not stored.
    Codec(CodecState),
    /// An agent's plugins, marketplaces and skills for `folder`, as its program lists them;
    /// `error` says why a change or a list failed.
    Extensions {
        agent: String,
        folder: PathBuf,
        extensions: Extensions,
        error: Option<String>,
    },
    /// The "All agents" layer and how each agent treats it; written before it is announced.
    Global(GlobalState),
    /// The servers an agent's program loaded with their tools, outside any session, or why it
    /// did not say.
    ServerTools {
        agent: String,
        servers: Vec<McpServer>,
        error: Option<String>,
    },
    /// An agent's MCP servers for a folder, where one can be added, or why they could not be
    /// listed.
    Servers {
        agent: String,
        folder: PathBuf,
        servers: Vec<ConfiguredServer>,
        scopes: Vec<(String, String)>,
        error: Option<String>,
    },
    /// Who the agent's program says is signed in. The program keeps the account, so nothing
    /// of it is stored: it is asked again at each start.
    Auth {
        agent: String,
        state: AuthState,
    },
    /// The models an agent's program offers, asked before any session of it: what the model
    /// menu of a new session lists until a session tells. Asked again at each start.
    AgentModels {
        agent: String,
        models: Vec<ModelInfo>,
    },
    /// The page where the person signs in, which the engine also opened in their browser.
    SignInUrl {
        agent: String,
        url: String,
    },
    SignInFailed {
        agent: String,
        message: String,
    },
    /// Who is signed in to GitHub, as `gh` says.
    Github(GithubState),
    /// The one-time code `gh` printed and the page where it is typed (opened in the browser).
    GithubCode {
        code: String,
        url: String,
    },
    GithubFailed {
        message: String,
    },
    /// The contribution calendar, or why it could not be had.
    GithubContributions(Result<ContributionCalendar, String>),
    /// What the MCP registry lists for `query`, or why it could not be asked.
    ServersFound {
        query: String,
        offers: Vec<ServerOffer>,
        error: Option<String>,
    },
    /// What the session hears next was passed on by the session `from`, which started it.
    Forwarded {
        session: SessionId,
        from: SessionId,
    },
    /// The system's voice started or stopped reading one of the session's replies.
    Reading {
        session: SessionId,
        reading: bool,
    },
    /// The session was started by `parent`, which it reports back to: the sidebar nests it
    /// there. A side chat (`/btw`) reports nothing: it is the person's own aside.
    Linked {
        session: SessionId,
        parent: SessionId,
        #[serde(default)]
        side: bool,
    },
    /// The agent proposed a task for a session of its own.
    Proposed {
        session: SessionId,
        proposal: Proposal,
    },
    ProposalAnswered {
        session: SessionId,
        task: String,
        outcome: ProposalOutcome,
    },
    /// Sessions it started finished their turn; what they said is sent to the session as
    /// `request`, which begins a turn of its own.
    Reported {
        session: SessionId,
        request: RequestId,
        reports: Vec<Report>,
    },
    /// The session waits for another's task to end (`wait_for_session`); a wait on the same
    /// session replaces the one before.
    Waiting {
        session: SessionId,
        wait: Wait,
    },
    /// The session's wait on `on` is over.
    WaitEnded {
        session: SessionId,
        on: SessionId,
        outcome: WaitOutcome,
        /// What the session is told with it (`on`'s last reply, or why it will not finish),
        /// kept so a restart before it hears it tells it then; empty for a cancel.
        #[serde(default)]
        text: String,
    },
    /// The session's browser changed; never stored.
    Browser {
        session: SessionId,
        update: BrowserUpdate,
    },
    /// The person clicked the banner about the session; never stored.
    NotificationClicked {
        session: SessionId,
    },
}

impl Event {
    pub fn session(&self) -> SessionId {
        match self {
            Event::SessionCreated { session, .. }
            | Event::Status { session, .. }
            | Event::Queued { session, .. }
            | Event::QueueChanged { session, .. }
            | Event::QueueTaken { session, .. }
            | Event::Notice { session, .. }
            | Event::Agent { session, .. }
            | Event::Changes { session, .. }
            | Event::Rewound { session, .. }
            | Event::RewoundTo { session, .. }
            | Event::RewindUndone { session }
            | Event::ModelChosen { session, .. }
            | Event::EffortChosen { session, .. }
            | Event::Decided { session, .. }
            | Event::WorkingTree { session, .. }
            | Event::Environment { session, .. }
            | Event::Renamed { session, .. }
            | Event::Pinned { session, .. }
            | Event::ViewChosen { session, .. }
            | Event::TasksCleared { session, .. }
            | Event::Deleted { session }
            | Event::Archived { session, .. }
            | Event::PullRequest { session, .. }
            | Event::Unread { session, .. }
            | Event::Forked { session, .. }
            | Event::Reading { session, .. }
            | Event::Forwarded { session, .. }
            | Event::FileRead { session, .. }
            | Event::FileWriteFailed { session, .. }
            | Event::Listed { session, .. }
            | Event::FileOpDone { session, .. }
            | Event::FilesFound { session, .. }
            | Event::Linked { session, .. }
            | Event::Proposed { session, .. }
            | Event::ProposalAnswered { session, .. }
            | Event::Reported { session, .. }
            | Event::Waiting { session, .. }
            | Event::WaitEnded { session, .. }
            | Event::Browser { session, .. }
            | Event::NotificationClicked { session } => *session,
            // quill's own, not a session's: no session has id 0.
            Event::Settings { .. }
            | Event::Restored
            | Event::Logo { .. }
            | Event::Storage { .. }
            | Event::SettingsNotSaved { .. }
            | Event::Update(_)
            | Event::UpdateChecked { .. }
            | Event::WhatsNew(_)
            | Event::Codec(_)
            | Event::Extensions { .. }
            | Event::Global(_)
            | Event::ServerTools { .. }
            | Event::Servers { .. }
            | Event::ServersFound { .. }
            | Event::Auth { .. }
            | Event::AgentModels { .. }
            | Event::SignInUrl { .. }
            | Event::SignInFailed { .. }
            | Event::Github(_)
            | Event::GithubCode { .. }
            | Event::GithubFailed { .. }
            | Event::GithubContributions(_) => 0,
        }
    }
}

/// A link to a pull request (`/pull/12`, `/merge_requests/7`), with or without what follows it.
pub fn is_pull_request(url: &str) -> bool {
    let url = url.split(['?', '#']).next().unwrap_or(url);
    let Some(path) = url.splitn(4, '/').nth(3) else {
        return false;
    };
    path.split('/').collect::<Vec<_>>().windows(2).any(|pair| {
        matches!(pair[0], "pull" | "merge_requests")
            && !pair[1].is_empty()
            && pair[1].bytes().all(|b| b.is_ascii_digit())
    })
}

/// The last pull request link in `text`, from what an agent said; a markdown link or a link
/// with a period after it counts.
pub fn find_pull_request(text: &str) -> Option<&str> {
    text.match_indices("http")
        .filter_map(|(at, _)| {
            let rest = &text[at..];
            let end = rest
                .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '`'))
                .unwrap_or(rest.len());
            let url = rest[..end].trim_end_matches([')', ']', ',', '.', ';', ':', '!', '*', '\'']);
            ((url.starts_with("http://") || url.starts_with("https://")) && is_pull_request(url))
                .then_some(url)
        })
        .last()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pull_request_link_is_recognised_with_or_without_what_follows() {
        for url in [
            "https://github.com/o/r/pull/12",
            "https://github.com/o/r/pull/12/files",
            "https://github.com/o/r/pull/12?tab=files",
            "https://gitlab.com/g/p/-/merge_requests/7",
        ] {
            assert!(is_pull_request(url), "{url}");
        }
        for url in [
            "https://github.com/o/r/pulls",
            "https://example.com/pull/abc",
            "https://example.com/docs",
            "mailto:someone@example.com",
        ] {
            assert!(!is_pull_request(url), "{url}");
        }
    }

    #[test]
    fn the_last_pull_request_in_a_text_is_found() {
        let pr = "https://github.com/o/r/pull/3";
        assert_eq!(find_pull_request(&format!("Opened {pr}")), Some(pr));
        assert_eq!(
            find_pull_request(&format!("See [the PR]({pr}) now")),
            Some(pr)
        );
        assert_eq!(find_pull_request(&format!("Done: {pr}.")), Some(pr));
        assert_eq!(
            find_pull_request(&format!("{pr} and {pr}/files.")),
            Some("https://github.com/o/r/pull/3/files")
        );
        assert_eq!(find_pull_request("first https://github.com/o/r/pull/1 then https://gitlab.com/g/p/-/merge_requests/2!"), Some("https://gitlab.com/g/p/-/merge_requests/2"));
        assert_eq!(
            find_pull_request("https://example.com/docs and nothing else"),
            None
        );
        assert_eq!(find_pull_request("no link at all"), None);
    }
}
