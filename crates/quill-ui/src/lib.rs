//! quill's interface: the state the engine's events build ([`Store::apply`]), what the person
//! is doing ([`Ui`]), what their actions ask ([`update`]) and the window ([`view`]).
//! Layout and colors follow Claude Desktop's Code surface (docs/ui.md, features/code-*.md).

mod account;
mod accounts;
pub mod annotate;
pub mod appearance;
mod files;
mod github;
mod greeting;
mod mention;
mod pixels;
pub mod remote;
mod slash;
mod terminals;
pub mod tour;
mod updates;
pub use quill_components::{icons, look};
pub mod view;
pub mod welcome;

pub use files::key as files_key;
pub use mention::{key as mention_key, mention_query};
pub use slash::{key as slash_key, slash_query};
pub use view::{menu_digit, view};

use quill_model::{
    AgentCapabilities, AgentEvent, BrowserOp, Decision, Event, FileEffect, Intent, Lane,
    LogoWanted, NewSession, NotifyLevel, ProposalAnswer, QueuedMessage, RequestId, SessionId,
    SessionStatus, Settings, Suggestion, TranscriptView,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// One entry of a session's transcript.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    User {
        request: RequestId,
        text: String,
        /// When it was sent, in seconds since 1970.
        at: Option<u64>,
        /// The image files it attached, drawn above the words.
        images: Vec<PathBuf>,
    },
    Assistant {
        text: String,
        streaming: bool,
        /// The agent's own id of the message, which "Fork from here" starts from.
        id: Option<String>,
        /// When it came, in seconds since 1970.
        at: Option<u64>,
    },
    /// What the model thought, shown only by the Thinking and Verbose views (not yet).
    Thinking { text: String, streaming: bool },
    Tool {
        call: String,
        name: String,
        input: Value,
        result: Option<ToolResult>,
    },
    Permission {
        id: String,
        /// The tool call it asks about.
        call: Option<String>,
        tool: String,
        input: Value,
        /// Why the agent asks, when it says.
        title: Option<String>,
        interactive: bool,
        suggestions: Vec<Suggestion>,
        decided: Option<&'static str>,
    },
    TurnEnd {
        ok: bool,
        interrupted: bool,
        cost: Option<f64>,
        error: Option<String>,
        /// The turn failed because the person is not signed in: its row offers Sign in.
        sign_in: bool,
    },
    /// Something quill left out of a message, shown where the message is.
    Notice { text: String },
    /// What one request changed in the folder, and whether it was rewound since.
    Changes {
        request: RequestId,
        files: Vec<FileEffect>,
        rewound: bool,
    },
    /// Tasks the agent proposed for sessions of their own, on one card per turn, each with
    /// what became of it.
    Proposals {
        tasks: Vec<(quill_model::Proposal, Option<quill_model::ProposalOutcome>)>,
    },
    /// The message after it was passed on by the session `from`.
    Forwarded { from: SessionId },
    /// What sessions it started said, sent to the agent as `request`.
    Reports {
        request: RequestId,
        reports: Vec<quill_model::Report>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    pub output: String,
    pub is_error: bool,
    /// Lines added and removed, from the files the agent reported.
    pub added: usize,
    pub removed: usize,
    /// The files it changed, before and after, for the diff its row opens.
    pub files: Vec<quill_model::FileChange>,
    /// It never finished, or the person denied it: Desktop reads "Stopped".
    pub stopped: bool,
    /// The query and sources of a web search (`None` for other tools).
    pub web: Option<quill_model::WebSearchRun>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    /// The agent said it needs the person signed in, for the turn that ends next.
    sign_in: bool,
    pub id: SessionId,
    pub agent: String,
    pub cwd: PathBuf,
    pub status: SessionStatus,
    pub items: Vec<Item>,
    pub capabilities: Option<AgentCapabilities>,
    pub mode: String,
    /// The model and the effort it runs at, as last chosen for it.
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The first message, as the sidebar's title until the agent names the session.
    pub title: String,
    pub branch: Option<String>,
    /// Tokens the conversation fills after its last turn, and what the session has cost.
    pub context_tokens: u64,
    /// The model's context size, once a turn reported it.
    pub context_window: u64,
    /// What fills the context, as the agent last told it while the Context card was open.
    pub context_usage: Option<quill_model::ContextUsage>,
    /// Its MCP servers, as the agent last told them while their dialog was open.
    pub mcp: Option<Vec<quill_model::McpServer>>,
    /// Its built-in browser, as the engine last told it.
    pub browser: SessionBrowser,
    pub cost: f64,
    /// What is uncommitted in its folder, as the engine last said.
    pub working: Option<Vec<quill_model::FileEffect>>,
    /// When the turn under way began, for the running line's clock.
    pub running_since: Option<std::time::Instant>,
    /// The model is thinking: the running line reads "Thinking" until it writes or acts.
    pub thinking: bool,
    /// The running turn's tokens so far, its helpers' included.
    pub tokens: u64,
    /// When the turn's latest step began, for the running line's clock on it.
    pub step_since: Option<std::time::Instant>,
    /// Work running beside the turns (helper agents, background commands), oldest first.
    pub tasks: Vec<Task>,
    /// Messages sent after the turn under way ends, as the engine last kept them.
    pub queue: Vec<QueuedMessage>,
    /// What each helper agent wrote and did, by the tool call that started it.
    pub helpers: std::collections::HashMap<String, Vec<Item>>,
    /// Kept at the top of the sidebar, under "Pinned".
    pub pinned: bool,
    /// The transcript view the person chose for it; `None` follows the default.
    pub view: Option<TranscriptView>,
    /// The repository its folder is in, as the engine last read it.
    pub repo: Option<quill_model::Repo>,
    /// Out of the sidebar; the search still finds it.
    pub archived: bool,
    /// The pull request the agent last gave the address of, if any.
    pub pull_request: Option<String>,
    /// Has news the person has not seen: a gray dot in its row.
    pub unread: bool,
    /// When it last changed in this run, for "Last updated".
    pub touched: u64,
    /// The session that started it, which it reports to and is nested under.
    pub parent: Option<SessionId>,
    /// A side chat (`/btw`): an aside of its parent's, not work started for it.
    pub aside: bool,
    /// The version of the agent's program, as it said when it started.
    pub agent_version: Option<String>,
    /// The messages the last rewind took out, while it can still be undone.
    pub rewound: Vec<Item>,
    /// Whether it can be reached from another device (docs/remote-control.md).
    pub remote: Remote,
    /// The code that pairs another device with its agent, and until when it works.
    pub remote_code: Option<(String, u64)>,
}

/// Whether a session can be reached from another device through its agent's own service.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Remote {
    #[default]
    Off,
    /// On, and where the session opens there when the agent said.
    On(Option<String>),
    /// Off: why it could not be turned on.
    Failed(String),
}

/// Work the agent runs beside its turns, as Desktop counts "1 running task" and
/// lists them in its Background tasks.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub id: String,
    pub call: Option<String>,
    pub description: String,
    pub summary: Option<String>,
    pub running: bool,
    /// `agent`, `command`, `workflow`, `monitor` or `task`.
    pub kind: String,
    /// How it ended when not done: it failed, or it was stopped.
    pub failed: bool,
    pub stopped: bool,
    pub usage: Option<quill_model::TaskUsage>,
    /// When it began here, for its clock while it runs.
    pub since: std::time::Instant,
}

impl Session {
    /// Whether its agent is at work on a turn, or waiting on the person within one.
    pub fn working(&self) -> bool {
        matches!(
            self.status,
            SessionStatus::Running | SessionStatus::Starting | SessionStatus::Awaiting
        )
    }

    /// Whether the tool call `call` started work that still runs.
    pub fn task_running(&self, call: &str) -> bool {
        self.tasks
            .iter()
            .any(|t| t.running && t.call.as_deref() == Some(call))
    }
}

/// What the engine said, as the transcript and sidebar show it.
#[derive(Default)]
pub struct Store {
    pub sessions: Vec<Session>,
    /// The plan's usage windows, as each agent last reported them: the person's, not one
    /// session's, and never one agent's shown for another (each has its own plan).
    pub limits_by_agent: std::collections::HashMap<String, quill_model::PlanLimits>,
    /// Counts the events applied, so a session knows when it last changed.
    pub clock: u64,
}

impl Store {
    pub fn session(&self, id: SessionId) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    fn session_mut(&mut self, id: SessionId) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    pub fn apply(&mut self, event: Event) {
        self.clock += 1;
        if matches!(
            event,
            Event::Queued { .. } | Event::Agent { .. } | Event::Forked { .. }
        ) {
            let clock = self.clock;
            if let Some(s) = self.session_mut(event.session()) {
                s.touched = clock;
            }
        }
        match event {
            Event::SessionCreated {
                session,
                agent,
                cwd,
            } => self.sessions.push(Session {
                sign_in: false,
                id: session,
                agent,
                cwd,
                status: SessionStatus::Starting,
                items: vec![],
                queue: vec![],
                capabilities: None,
                mode: "default".into(),
                model: None,
                effort: None,
                title: String::new(),
                branch: None,
                context_tokens: 0,
                context_window: 0,
                context_usage: None,
                mcp: None,
                browser: SessionBrowser::default(),
                working: None,
                cost: 0.0,
                running_since: None,
                thinking: false,
                tokens: 0,
                step_since: None,
                tasks: vec![],
                helpers: Default::default(),
                pinned: false,
                view: None,
                repo: None,
                archived: false,
                pull_request: None,
                unread: false,
                touched: 0,
                parent: None,
                aside: false,
                agent_version: None,
                rewound: vec![],
                remote: Remote::Off,
                remote_code: None,
            }),
            Event::Status { session, status } => {
                if let Some(s) = self.session_mut(session) {
                    // A process that ended (or a session read back) answers nothing it asked.
                    if matches!(status, SessionStatus::Exited | SessionStatus::Failed(_)) {
                        close_open_permissions(&mut s.items);
                        for task in &mut s.tasks {
                            task.running = false;
                        }
                    }
                    let busy = matches!(
                        status,
                        SessionStatus::Starting | SessionStatus::Running | SessionStatus::Awaiting
                    );
                    if !busy {
                        s.running_since = None;
                        s.thinking = false;
                        s.tokens = 0;
                    } else if s.running_since.is_none() {
                        s.running_since = Some(std::time::Instant::now());
                    }
                    s.status = status;
                }
            }
            Event::Queued {
                session,
                request,
                text,
                at,
                images,
                ..
            } => {
                if let Some(s) = self.session_mut(session) {
                    if s.title.is_empty() {
                        s.title = text
                            .lines()
                            .next()
                            .unwrap_or_default()
                            .chars()
                            .take(60)
                            .collect();
                    }
                    s.rewound.clear();
                    s.items.push(Item::User {
                        request,
                        text,
                        at,
                        images,
                    });
                }
            }
            Event::Notice { session, text } => {
                if let Some(s) = self.session_mut(session) {
                    s.items.push(Item::Notice { text });
                }
            }
            Event::QueueChanged { session, entries } => {
                if let Some(s) = self.session_mut(session) {
                    s.queue = entries;
                }
            }
            Event::QueueTaken { .. } => {}
            Event::Agent {
                session,
                event: AgentEvent::Limits(limits),
                ..
            } => {
                if let Some(agent) = self.session(session).map(|s| s.agent.clone()) {
                    self.limits_by_agent.insert(agent, limits);
                }
            }
            Event::Agent { session, event, .. } => {
                if let Some(s) = self.session_mut(session) {
                    agent_event(s, event);
                }
            }
            // A request that changed nothing gets no card.
            Event::Changes {
                session,
                request,
                files,
                ..
            } if !files.is_empty() => {
                if let Some(s) = self.session_mut(session) {
                    s.items.push(Item::Changes {
                        request,
                        files,
                        rewound: false,
                    });
                }
            }
            Event::Changes { .. } => {}
            Event::WorkingTree {
                session,
                files,
                repo,
            } => {
                if let Some(s) = self.session_mut(session) {
                    s.working = Some(files);
                    if let Some(repo) = &repo {
                        s.branch = Some(repo.branch.clone());
                    }
                    s.repo = repo;
                }
            }
            Event::Environment { session, branch } => {
                if let Some(s) = self.session_mut(session) {
                    s.branch = branch;
                }
            }
            Event::Decided {
                session,
                permission,
                decision,
            } => {
                if let Some(s) = self.session_mut(session) {
                    mark_decided(&mut s.items, &permission, &decision);
                }
            }
            Event::Rewound {
                session, request, ..
            } => {
                if let Some(s) = self.session_mut(session) {
                    for item in &mut s.items {
                        if let Item::Changes {
                            request: r,
                            rewound,
                            ..
                        } = item
                        {
                            if *r == request {
                                *rewound = true;
                            }
                        }
                    }
                }
            }
            Event::Renamed { session, title } => {
                if let Some(s) = self.session_mut(session) {
                    s.title = title;
                }
            }
            Event::Pinned { session, pinned } => {
                if let Some(s) = self.session_mut(session) {
                    s.pinned = pinned;
                }
            }
            Event::ViewChosen { session, view } => {
                if let Some(s) = self.session_mut(session) {
                    s.view = view;
                }
            }
            Event::PullRequest { session, url } => {
                if let Some(s) = self.session_mut(session) {
                    s.pull_request = Some(url);
                }
            }
            Event::TasksCleared { session, tasks } => {
                if let Some(s) = self.session_mut(session) {
                    s.tasks.retain(|t| !tasks.contains(&t.id));
                }
            }
            Event::Deleted { session } => self.sessions.retain(|s| s.id != session),
            Event::Archived { session, archived } => {
                if let Some(s) = self.session_mut(session) {
                    s.archived = archived;
                }
            }
            Event::ModelChosen { session, model } => {
                if let Some(s) = self.session_mut(session) {
                    s.model = Some(model);
                }
            }
            Event::EffortChosen { session, effort } => {
                if let Some(s) = self.session_mut(session) {
                    s.effort = Some(effort);
                }
            }
            // "Rewind to here": the message and all after it go, kept until a message is sent.
            Event::RewoundTo {
                session, request, ..
            } => {
                if let Some(s) = self.session_mut(session) {
                    let from = (s.items.iter())
                        .position(|i| matches!(i, Item::User { request: r, .. } if *r == request));
                    s.rewound = match from {
                        Some(from) => s.items.split_off(from),
                        None => vec![],
                    };
                }
            }
            // "Undo rewind": what the rewind took out is there again.
            Event::RewindUndone { session } => {
                if let Some(s) = self.session_mut(session) {
                    s.items.extend(std::mem::take(&mut s.rewound));
                }
            }
            Event::Unread { session, unread } => {
                if let Some(s) = self.session_mut(session) {
                    s.unread = unread;
                }
            }
            // A fork starts as its parent was: its conversation, under the parent's name
            // marked as a fork (inferred: Desktop's name for a fork was not captured).
            Event::Proposed { session, proposal } => {
                if let Some(s) = self.session_mut(session) {
                    // A turn's proposals share one card.
                    let turn = s
                        .items
                        .iter()
                        .rposition(|i| matches!(i, Item::User { .. } | Item::Reports { .. }))
                        .map_or(0, |at| at + 1);
                    let card = s.items[turn..]
                        .iter_mut()
                        .rev()
                        .find_map(|item| match item {
                            Item::Proposals { tasks } => Some(tasks),
                            _ => None,
                        });
                    match card {
                        Some(tasks) => tasks.push((proposal, None)),
                        None => s.items.push(Item::Proposals {
                            tasks: vec![(proposal, None)],
                        }),
                    }
                }
            }
            Event::ProposalAnswered {
                session,
                task,
                outcome,
            } => {
                if let Some(s) = self.session_mut(session) {
                    for item in &mut s.items {
                        if let Item::Proposals { tasks } = item {
                            for (proposal, answered) in tasks.iter_mut() {
                                if proposal.id == task {
                                    *answered = Some(outcome.clone());
                                }
                            }
                        }
                    }
                }
            }
            Event::Reported {
                session,
                request,
                reports,
            } => {
                if let Some(s) = self.session_mut(session) {
                    s.rewound.clear();
                    s.items.push(Item::Reports { request, reports });
                }
            }
            Event::Linked {
                session,
                parent,
                side,
            } => {
                if let Some(s) = self.session_mut(session) {
                    s.parent = Some(parent);
                    s.aside = side;
                }
            }
            Event::Forked { session, from, at } => {
                let parent = self.session(from).map(|p| {
                    // From a message, the fork's transcript ends with it.
                    let end = at
                        .as_ref()
                        .and_then(|at| {
                            p.items.iter().position(
                                |i| matches!(i, Item::Assistant { id: Some(id), .. } if id == at),
                            )
                        })
                        .map_or(p.items.len(), |i| i + 1);
                    (p.items[..end].to_vec(), p.title.clone())
                });
                if let (Some(s), Some((items, title))) = (self.session_mut(session), parent) {
                    s.items = items;
                    s.title = format!("{title} (fork)");
                }
            }
            Event::Forwarded { session, from } => {
                if let Some(s) = self.session_mut(session) {
                    s.items.push(Item::Forwarded { from });
                }
            }
            Event::Browser { session, update } => {
                if let Some(s) = self.session_mut(session) {
                    s.browser.heard(update);
                }
            }
            Event::Reading { .. }
            | Event::FileRead { .. }
            | Event::FileWriteFailed { .. }
            | Event::Listed { .. }
            | Event::FileOpDone { .. }
            | Event::FilesFound { .. }
            | Event::Settings { .. }
            | Event::Logo { .. }
            | Event::Github(_)
            | Event::GithubCode { .. }
            | Event::GithubFailed { .. }
            | Event::GithubContributions(_)
            | Event::Storage { .. }
            | Event::Restored
            | Event::SettingsNotSaved { .. }
            | Event::Update(_)
            | Event::Extensions { .. }
            | Event::Servers { .. }
            | Event::Global(_)
            | Event::ServersFound { .. }
            | Event::Auth { .. }
            | Event::AgentModels { .. }
            | Event::SignInUrl { .. }
            | Event::SignInFailed { .. } => {}
        }
    }
}

fn agent_event(s: &mut Session, event: AgentEvent) {
    match &event {
        AgentEvent::Thinking { parent: None } => s.thinking = true,
        AgentEvent::TextDelta { parent: None, .. }
        | AgentEvent::Text { parent: None, .. }
        | AgentEvent::ToolUse { parent: None, .. } => s.thinking = false,
        _ => {}
    }
    match event {
        AgentEvent::Ready {
            version,
            capabilities,
            mode,
        } => {
            s.agent_version = Some(version);
            // An agent that lists its skills apart (`Skills`) says no commands here: the ones
            // already merged stay.
            let mut capabilities = capabilities;
            if capabilities.commands.is_empty() {
                capabilities.commands = (s.capabilities.take()).map_or(vec![], |c| c.commands);
            }
            s.capabilities = Some(capabilities);
            if let Some(mode) = mode {
                s.mode = mode;
            }
        }
        AgentEvent::ModeChanged { mode } => s.mode = mode,
        AgentEvent::Remote { on, url, error } => {
            s.remote = match (on, error) {
                // A later word without the link keeps the one already given.
                (true, _) => Remote::On(url.or(match &s.remote {
                    Remote::On(old) => old.clone(),
                    _ => None,
                })),
                (false, Some(error)) => Remote::Failed(error),
                (false, None) => Remote::Off,
            };
            if !matches!(s.remote, Remote::On(_)) {
                s.remote_code = None;
            }
        }
        AgentEvent::RemotePairing { code, expires_at } => s.remote_code = Some((code, expires_at)),
        // Answered on another device: the card goes, as an answered one does.
        AgentEvent::Withdrawn { id } => {
            for item in &mut s.items {
                if let Item::Permission {
                    id: asked,
                    decided: decided @ None,
                    ..
                } = item
                {
                    if *asked == id {
                        *decided = Some("Withdrawn");
                    }
                }
            }
        }
        // The turn's end follows, and carries it.
        AgentEvent::AuthRequired => s.sign_in = true,
        AgentEvent::Tokens { tokens } => s.tokens = tokens,
        AgentEvent::ContextUsage(usage) => s.context_usage = Some(usage),
        AgentEvent::McpServers(servers) => s.mcp = Some(servers),
        // The agent's skills join its commands in the `/` menu, but a skill that is off.
        AgentEvent::Skills(skills) => {
            if let Some(capabilities) = s.capabilities.as_mut() {
                for skill in skills.into_iter().filter(|k| k.state != "off") {
                    if capabilities.commands.iter().any(|c| c.name == skill.name) {
                        continue;
                    }
                    capabilities.commands.push(quill_model::AgentSlashCommand {
                        name: skill.name,
                        description: skill.description,
                        argument_hint: String::new(),
                        aliases: vec![],
                    });
                }
                capabilities.commands.sort_by(|a, b| a.name.cmp(&b.name));
            }
        }
        AgentEvent::TaskStarted {
            id,
            call,
            description,
            kind,
        } if !s.tasks.iter().any(|t| t.id == id) => s.tasks.push(Task {
            id,
            call,
            description,
            summary: None,
            running: true,
            kind,
            failed: false,
            stopped: false,
            usage: None,
            since: std::time::Instant::now(),
        }),
        AgentEvent::TaskProgress { id, usage } => {
            if let Some(task) = s.tasks.iter_mut().find(|t| t.id == id) {
                task.usage = Some(usage);
            }
        }
        AgentEvent::TaskEnded {
            id,
            summary,
            ok,
            stopped,
            usage,
            ..
        } => {
            if let Some(task) = s.tasks.iter_mut().find(|t| t.id == id) {
                task.running = false;
                task.summary = summary;
                task.failed = !ok && !stopped;
                task.stopped = stopped;
                task.usage = usage.or(task.usage);
            }
        }
        // A helper's own steps and words go with the call that started it, for its pane.
        AgentEvent::ToolUse {
            call,
            name,
            input,
            parent: Some(parent),
        } => s.helpers.entry(parent).or_default().push(Item::Tool {
            call,
            name,
            input,
            result: None,
        }),
        AgentEvent::Text {
            text,
            parent: Some(parent),
            id,
            at,
        } => s.helpers.entry(parent).or_default().push(Item::Assistant {
            text,
            streaming: false,
            id,
            at,
        }),
        // Subagents' own text stays out of the main transcript for now.
        AgentEvent::TextDelta { text, parent: None } => match s.items.last_mut() {
            Some(Item::Assistant {
                text: shown,
                streaming: true,
                ..
            }) => shown.push_str(&text),
            _ => s.items.push(Item::Assistant {
                text,
                streaming: true,
                id: None,
                at: None,
            }),
        },
        AgentEvent::Text {
            text,
            parent: None,
            id,
            at,
        } => match s.items.last_mut() {
            Some(Item::Assistant {
                text: shown,
                streaming,
                id: shown_id,
                at: shown_at,
            }) if *streaming => {
                *shown = text;
                *streaming = false;
                *shown_id = id;
                *shown_at = at;
            }
            _ => s.items.push(Item::Assistant {
                text,
                streaming: false,
                id,
                at,
            }),
        },
        // Subagents' thinking stays out of the main transcript for now.
        AgentEvent::ThinkingDelta { text, parent: None } => match s.items.last_mut() {
            Some(Item::Thinking {
                text: shown,
                streaming: true,
            }) => shown.push_str(&text),
            _ => s.items.push(Item::Thinking {
                text,
                streaming: true,
            }),
        },
        AgentEvent::ThinkingText { text, parent: None } => match s.items.last_mut() {
            Some(Item::Thinking {
                text: shown,
                streaming,
            }) if *streaming => {
                *shown = text;
                *streaming = false;
            }
            _ => s.items.push(Item::Thinking {
                text,
                streaming: false,
            }),
        },
        AgentEvent::ToolUse {
            call,
            name,
            input,
            parent: None,
        } => {
            s.step_since = Some(std::time::Instant::now());
            s.items.push(Item::Tool {
                call,
                name,
                input,
                result: None,
            });
        }
        AgentEvent::ToolResult {
            call,
            output,
            is_error,
            files,
            web,
            ..
        } => {
            let (added, removed) = files
                .iter()
                .map(|f| line_changes(f.before.as_deref(), f.after.as_deref()))
                .fold((0, 0), |(a, r), (x, y)| (a + x, r + y));
            let denied = s.items.iter().any(|item| {
                matches!(item, Item::Permission { call: Some(c), decided: Some("Denied"), .. } if *c == call)
            });
            let helpers = s.helpers.values_mut().flat_map(|items| items.iter_mut());
            for item in s.items.iter_mut().rev().chain(helpers) {
                if let Item::Tool {
                    call: c, result, ..
                } = item
                {
                    if *c == call {
                        *result = Some(ToolResult {
                            output,
                            is_error,
                            added,
                            removed,
                            files,
                            stopped: denied,
                            web,
                        });
                        break;
                    }
                }
            }
        }
        AgentEvent::PermissionRequest {
            id,
            call,
            tool,
            input,
            title,
            interactive,
            suggestions,
        } => s.items.push(Item::Permission {
            id,
            call,
            tool,
            input,
            title,
            interactive,
            suggestions,
            decided: None,
        }),
        AgentEvent::TurnFinished {
            ok,
            interrupted,
            usage,
            error,
        } => {
            if let Some(Item::Assistant { streaming, .. }) = s.items.last_mut() {
                *streaming = false;
            }
            // A finished turn waits on nothing: a permission it left open (interrupted, or
            // answered before answers were kept) is closed.
            close_open_permissions(&mut s.items);
            if usage.context_window > 0 {
                s.context_window = usage.context_window;
            }
            if usage.context_tokens > 0 {
                s.context_tokens = usage.context_tokens;
            }
            s.cost += usage.cost_usd.unwrap_or(0.0);
            s.items.push(Item::TurnEnd {
                ok,
                interrupted,
                cost: usage.cost_usd,
                error,
                sign_in: std::mem::take(&mut s.sign_in),
            });
        }
        _ => {}
    }
}

/// Lines added and removed between two versions of a file, counted as unmatched lines
/// (a rewritten line counts once each way, as Desktop shows +1 −1).
fn line_changes(before: Option<&str>, after: Option<&str>) -> (usize, usize) {
    let mut left: Vec<&str> = before.unwrap_or_default().lines().collect();
    let mut added = 0;
    for line in after.unwrap_or_default().lines() {
        match left.iter().position(|l| *l == line) {
            Some(at) => drop(left.remove(at)),
            None => added += 1,
        }
    }
    (added, left.len())
}

/// The texts of the notices, which Settings' descriptions of the same events show too.
pub const NOTICE_PERMISSION: &str = "An agent needs your approval to use a tool.";
pub const NOTICE_QUESTION: &str = "An agent is asking you to choose between options.";
pub const NOTICE_DONE: &str = "An agent finished and is waiting for your input.";

/// The pointer over a row of the open "/" or "@" list: that row becomes the chosen one, so the
/// list has one highlight, as Desktop's palettes (the pointer moves the selection, the arrow
/// keys move it from there).
pub fn palette_hover(store: &Store, ui: &mut Ui, id: &str) {
    let at = |rows: Option<(usize, Vec<(String, limn::SuggestionItem)>)>| {
        rows.and_then(|(_, rows)| rows.iter().position(|(row, _)| row == id))
    };
    if let Some(index) = at(slash::open(store, ui)) {
        ui.slash_selected = index;
    } else if let Some(index) = at(mention::open(store, ui)) {
        ui.mention_selected = index;
    }
}

/// The banner an engine event earns, when its session is not in view: the level from
/// Settings, the session's title and the notice's text. A permission request, a question
/// or a finished turn of a session the person is not looking at; a side chat's turn is not.
pub fn notice(
    store: &Store,
    ui: &Ui,
    event: &Event,
    focused: bool,
) -> Option<(NotifyLevel, String, String)> {
    let Event::Agent { session, event, .. } = event else {
        return None;
    };
    let s = store.session(*session);
    let (level, text) = match event {
        AgentEvent::PermissionRequest {
            interactive: false, ..
        } => (ui.settings.notify_permission, NOTICE_PERMISSION),
        AgentEvent::PermissionRequest {
            interactive: true, ..
        } => (ui.settings.notify_question, NOTICE_QUESTION),
        // Desktop's "Task complete" has no badge, so it reads as a banner.
        AgentEvent::TurnFinished { .. } if !s.is_some_and(|s| s.aside) => (
            match ui.settings.notify_done {
                NotifyLevel::Badge => NotifyLevel::Banner,
                level => level,
            },
            NOTICE_DONE,
        ),
        _ => return None,
    };
    if level == NotifyLevel::Off || (focused && ui.selected == Some(*session)) {
        return None;
    }
    let title = s
        .map(|s| s.title.clone())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "quill".into());
    Some((level, title, text.into()))
}

/// The folder the terminal and new sessions work in: the selected session's, else the chosen one.
pub fn folder(store: &Store, ui: &Ui) -> PathBuf {
    ui.selected
        .and_then(|id| store.session(id))
        .map_or_else(|| ui.cwd.clone(), |s| s.cwd.clone())
}

/// The session code from an editor goes to: the one on screen if it works in `root`, else the
/// latest of that folder's own (not a helper's, not an aside), else none, for a new one.
pub fn session_for(store: &Store, ui: &Ui, root: &Path) -> Option<SessionId> {
    let root_real = root.canonicalize().ok();
    let same = |cwd: &Path| {
        cwd == root
            || root_real
                .as_ref()
                .is_some_and(|real| cwd.canonicalize().is_ok_and(|c| &c == real))
    };
    if let Some(shown) = ui.selected.and_then(|id| store.session(id)) {
        if same(&shown.cwd) {
            return Some(shown.id);
        }
    }
    store
        .sessions
        .iter()
        .filter(|s| !s.archived && s.parent.is_none() && !s.aside && same(&s.cwd))
        .max_by_key(|s| s.touched)
        .map(|s| s.id)
}

/// What code from an editor adds to the message: the file as an @-mention from its folder,
/// and the lines picked, fenced in their language.
pub fn mention(root: &Path, path: &Path, lines: Option<(u32, u32)>, text: &str) -> String {
    let file = path
        .strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string();
    match lines.filter(|_| !text.trim().is_empty()) {
        None => format!("@{file} "),
        Some((first, last)) => {
            let at = if first == last {
                format!("line {first}")
            } else {
                format!("lines {first}–{last}")
            };
            let fence = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            // A fence longer than any run of backticks inside keeps the code whole.
            let ticks = "`".repeat(longest_run(text, '`').max(2) + 1);
            format!(
                "@{file} ({at}):\n{ticks}{fence}\n{}\n{ticks}\n",
                text.trim_end()
            )
        }
    }
}

fn longest_run(text: &str, c: char) -> usize {
    text.split(|x| x != c).map(str::len).max().unwrap_or(0)
}

/// An editor found on the computer, as "Open in" offers it: its key (its URL scheme), its
/// name, and its own icon when the desktop has one.
#[derive(Clone, Debug, PartialEq)]
pub struct Editor {
    pub key: String,
    pub name: String,
    pub picture: Option<limn::RasterImage>,
}

/// A message written before its session existed: its text, the sessions it mentions and its
/// image files.
pub type Waiting = (String, Vec<(SessionId, String)>, Vec<PathBuf>);

/// The image the viewer shows over the window.
pub struct Viewer {
    /// The image file.
    pub path: PathBuf,
    /// The rectangle of its tile in the window, which the picture grows from. The host fills it,
    /// since only the frame knows where the tile is.
    pub origin: Option<limn::Rect>,
    /// Whether the image is still in the composer, so it can be annotated; a sent one is not.
    pub annotatable: bool,
}

/// What the person is doing, apart from what the engine knows.
pub struct Ui {
    pub selected: Option<SessionId>,
    pub prompt: String,
    /// Where a new session starts.
    pub cwd: PathBuf,
    pub agent: String,
    pub mode: String,
    pub model: Option<String>,
    /// The effort new sessions start with and the selected one runs at.
    pub effort: Option<String>,
    pub open_menu: Option<&'static str>,
    /// A link right-clicked in the conversation: its address and where the menu opens.
    pub link_menu: Option<(String, limn::Point)>,
    /// The editors found on the computer, for Open in.
    pub editors: Vec<Editor>,
    pub expanded: Vec<String>,
    /// A message written before its session existed, and the sessions it mentions, sent once
    /// it does.
    pub waiting: Option<Waiting>,
    /// The image files the composer's message attaches, sent with it, and their thumbnails
    /// the host decodes for the tiles and the messages shown.
    pub images: Vec<PathBuf>,
    /// The composer's Enter that is being handled had Ctrl: its message queues rather than steers.
    pub queue_submit: bool,
    pub thumbs: std::collections::HashMap<PathBuf, limn::RasterImage>,
    /// The logos of connectors and plugins by key, as the engine found them (`None`: none), and
    /// the keys asked for, each once.
    pub logos: std::collections::HashMap<String, (Option<PathBuf>, Option<PathBuf>)>,
    pub logo_asked: std::collections::HashSet<String>,
    /// The image the viewer shows, if any.
    pub viewer: Option<Viewer>,
    /// The images at up to 2048 px for the viewer, decoded off the window's thread once it opens
    /// on them; until then it shows the thumbnail.
    pub full: std::collections::HashMap<PathBuf, limn::RasterImage>,
    /// The composer's image being annotated, while its editor is open.
    pub annotating: Option<PathBuf>,
    /// The marks drawn on it, and the ones Redo brings back. Clear all moves the marks here in
    /// reverse, so Redo restores them in order; an Undo after it has nothing to undo.
    pub marks: Vec<limn::SketchMark>,
    pub redo: Vec<limn::SketchMark>,
    /// The tool a press and drag makes, and the last shape the Shapes menu chose, which its
    /// button shows and arms.
    pub tool: limn::SketchTool,
    pub last_shape: limn::SketchTool,
    /// The ink and the width of the marks, which stay for the app run as Desktop's do.
    pub ink: limn::Color,
    pub stroke: f32,
    /// Whether the "Discard your annotations?" question is open, and whether a save runs.
    pub discarding: bool,
    pub saving: bool,
    /// Each image's size in pixels, read from its file by the host: the viewer fits the
    /// picture to it and the editor lays it out at its shape.
    pub sizes: std::collections::HashMap<PathBuf, (f32, f32)>,
    /// Why the last image was left out, until the composer's images change again.
    pub image_notice: Option<String>,
    /// The request whose changes the diff pane shows.
    pub diff: Option<(SessionId, RequestId)>,
    /// The panes open beside the conversation, in the order they opened: the first sets the
    /// column's width and the rest stack under it, as Desktop's do.
    pub panes: Vec<Pane>,
    /// The pane that fills the space of the conversation, as Desktop's expand does.
    pub expanded_pane: Option<Pane>,
    /// Whether the right panel (Environment, Context, Changes) shows while no pane is open.
    pub panel_open: bool,
    /// The right panel's width, once the person dragged its edge.
    pub panel_width: f32,
    /// Folders whose sessions the sidebar folds away.
    pub collapsed: Vec<PathBuf>,
    /// The person's first name for the new session's greeting, when the host knows it.
    pub name: Option<String>,
    /// The local time's distance from UTC in seconds, for the hours limits reset at.
    pub utc_offset: i64,
    /// How the person allowed last, which the next permission card offers first.
    pub last_allow: Allow,
    /// Whether the transcript shows its end, which the host sets every frame.
    pub at_end: bool,
    /// The window's size, which the host sets every frame; a dragged boundary needs it.
    pub window: limn::Size,
    /// The column of panes' share of the space right of the sidebar, once the person dragged
    /// its boundary; until then the first pane's own share.
    pub column_fraction: Option<f32>,
    /// The top pane's share of two stacked panes, once their boundary was dragged.
    pub stack_fraction: Option<f32>,
    /// The labels picked on question cards, by permission and question index.
    pub answers: std::collections::HashMap<(String, usize), Vec<String>>,
    /// The permission whose "Always allow" menu is open.
    pub perm_menu: Option<String>,
    /// The tool call of the helper agent its pane shows.
    pub helper: Option<String>,
    /// The permission or tool call whose plan its pane shows.
    pub plan: Option<String>,
    /// The question a question card shows, by permission.
    pub question: std::collections::HashMap<String, usize>,
    /// What the person wrote under "Other", by permission and question.
    pub other: std::collections::HashMap<(String, usize), String>,
    /// The plan whose "Revise…" field is open, and what it holds.
    pub revising: Option<String>,
    pub revision: String,
    pub terminal: TerminalPane,
    /// The sidebar: Ctrl+B hides it, as in Desktop, and hidden it peeks.
    pub sidebar: quill_components::sidebar::Sidebar,
    /// Where the pointer is in the window and the control under it, which the host sets
    /// every frame, for the hidden sidebar's peek.
    pub pointer: Option<limn::Point>,
    pub hovered: Option<String>,
    /// The right panel's width, on a spring to 0 or its own width as it closes and opens.
    /// When the welcome was first drawn: what comes in under its word is timed from it.
    pub welcome_since: std::cell::OnceCell<std::time::Instant>,
    /// The host runs quill with a stand-in agent and no settings kept (tests, captures): the
    /// welcome is not shown, unless `QUILL_WELCOME` asks for it.
    pub skip_welcome: bool,
    /// The tour's step, from zero, while it runs (`tour`).
    pub tour: Option<usize>,
    pub panel_shown: limn::ScalarSpring,
    /// How far the column of panes is open, 0 to 1, on a spring as it opens and shuts, and
    /// the last pane closed, which it still shows while it shuts.
    pub column_shown: limn::ScalarSpring,
    pub closing: Option<Pane>,
    /// The column's share of the room, on a spring as panes that need more open and close; a
    /// dragged boundary sets it at once.
    pub column_share: limn::ScalarSpring,
    /// How far the expanded pane has grown over the conversation, 0 to 1, on a spring both
    /// ways.
    pub expand_shown: limn::ScalarSpring,
    /// How far the lower of two stacked panes is open, 0 to 1, on a spring, and the one that
    /// closed with its place in the stack, which it still shows while it shuts.
    pub stack_shown: limn::ScalarSpring,
    pub stack_closing: Option<(usize, Pane)>,
    /// The session whose sidebar row has its "···" menu open.
    pub row_menu: Option<SessionId>,
    /// The selected session's title while the person renames it in the top bar.
    pub title_edit: Option<String>,
    /// The session the delete dialog asks about.
    pub deleting: Option<SessionId>,
    /// What the search dialog looks for, while it is open.
    pub search: Option<String>,
    /// The session's MCP servers' dialog (`/mcp`), while open.
    pub mcp: Option<McpDialog>,
    /// Sessions whose branch bar the person closed.
    pub bar_closed: Vec<SessionId>,
    /// Sessions whose queue bar is expanded to its list; a queue starts folded.
    pub queue_expanded: Vec<SessionId>,
    /// The queued message whose "⋮" menu is open.
    pub queue_menu: Option<String>,
    /// How the sidebar groups and orders the sessions.
    pub group_by: GroupBy,
    pub sort_by: SortBy,
    pub ascending: bool,
    /// Sessions seen running in this run: only their turns that end unseen leave them
    /// unread, not the turns read back at start.
    pub live: std::collections::HashSet<SessionId>,
    /// The group header whose filter menu is open.
    pub filter_menu: Option<String>,
    /// The changed file of the diff whose Open in menu is open.
    pub diff_menu: Option<String>,
    /// Whether the menu of the sidebar's foot, the person's, is open.
    pub account_menu: bool,
    /// The agent's account and the sign-in screen (`account`).
    pub account: account::Account,
    /// GitHub through `gh`: the person's identity (github.rs).
    pub github: github::Github,
    /// quill's own update, as the engine last reported it.
    pub update: quill_model::UpdateState,
    /// "Agents are still working" is up, and "Wait for agents" was chosen (the update installs
    /// the first time none is busy).
    pub update_asking: bool,
    pub restart_when_idle: bool,
    /// A fork was asked for: the session the engine makes next is it, and opens.
    pub waiting_fork: bool,
    /// Words the host puts back in the message box: a message rewound or forked from, or a
    /// slash command picked.
    pub restore_prompt: Option<String>,
    /// A menu the "/" palette opened (`/usage`): when it closes, by a choice or Escape, the
    /// prompt loses its `/` token and the composer takes the keys again.
    pub menu_from_palette: bool,
    /// The slash palette's highlighted row, and the `/` (its byte index) Escape shut it at.
    pub slash_selected: usize,
    pub slash_dismissed: Option<usize>,
    /// The "@" list's highlighted row, and the `@query` Escape shut it at.
    pub mention_selected: usize,
    pub mention_dismissed: Option<String>,
    /// The sessions the message mentions, by id and title, for the next task.
    pub mentioned_sessions: Vec<(SessionId, String)>,
    /// The files the engine found for the "@" query it was asked: its session, query and paths.
    pub mention_found: Option<(SessionId, String, Vec<PathBuf>)>,
    /// The Completed group shows every session, not its first few.
    pub show_more: bool,
    /// The side chat its pane shows, one asked for that the engine is making, and what its
    /// reply field holds.
    pub side_chat: Option<SessionId>,
    pub side_waiting: bool,
    pub side_reply: String,
    /// The copy button last pressed and when: it shows a check for a moment.
    pub copied: Option<(String, std::time::Instant)>,
    /// The reply being read aloud, by its session and place.
    pub reading: Option<(SessionId, usize)>,
    /// Each session's Files pane: its folder's tree and the files open in it.
    pub files: std::collections::HashMap<SessionId, files::FilesPane>,
    /// What the Files pane asks of the window: a control to focus, and words to copy.
    pub files_focus: Option<String>,
    pub files_copy: Option<String>,
    /// The session whose Remote Control dialog shows (docs/remote-control.md).
    pub remote: Option<SessionId>,
    /// A session the person asked to make reachable, until its agent answers.
    pub remote_asked: Option<SessionId>,
    /// The QR code of the link the dialog shows, drawn by the host: the link and its picture.
    pub qr: Option<(String, limn::RasterImage)>,
    /// An editor of the Files pane whose caret the window brings into view, once it is laid
    /// out: a line the filter found.
    pub files_reveal: Option<String>,
    /// How many tabs each session's browser had when last heard: a new one opens the pane.
    pub browser_tabs: std::collections::HashMap<SessionId, usize>,
    /// What each session's browser last had to say, until dismissed.
    pub browser_notice: std::collections::HashMap<SessionId, String>,
    /// The settings in force, as the engine last kept them, and the agents it can run.
    pub settings: quill_model::Settings,
    pub agents: Vec<quill_model::AgentInfo>,
    /// The models each agent's program offered before any session of it (`Event::AgentModels`).
    pub agent_models: std::collections::HashMap<String, Vec<quill_model::ModelInfo>>,
    /// Settings, while open: the page shown, what its search looks for, and the select whose
    /// list is open.
    pub settings_page: Option<String>,
    pub settings_query: String,
    pub settings_menu: Option<String>,
    /// Where quill keeps its data and how much it takes, as last measured.
    pub storage: Option<(PathBuf, u64)>,
    /// Why the last change in Settings was not kept, until the next one.
    pub settings_error: Option<String>,
    /// Each agent's plugins and skills, as the engine last read them, by agent id.
    pub extensions: std::collections::HashMap<String, Extensions>,
    /// Customize › Connectors.
    pub connectors: Connectors,
    /// quill's "All agents" servers and skills, and how each agent treats them.
    pub global: quill_model::GlobalState,
    /// "New skill" or "Import skill" for "All agents", as typed.
    pub skill_dialog: Option<SkillDialog>,
    /// What Customize asks before removing, while it asks.
    pub removing: Option<Removing>,
    /// The marketplace being typed in Customize › Plugins, and the agents it is added to.
    pub marketplace_field: String,
    pub market_targets: Vec<String>,
    /// What Customize's page searches for, and Discover's directory.
    pub customize_query: String,
    pub directory_query: String,
    /// Discover's directory of plugins, while open, by agent; the marketplace it shows.
    pub directory: Option<String>,
    pub directory_market: Option<String>,
    /// The marketplaces' dialog over Customize, while open, by agent.
    pub marketplaces: Option<MarketDialog>,
    /// Things quill moves itself jump to where they go, as limn's do under reduced motion.
    pub reduced_motion: bool,
    /// How many of each session's items the person has had in view.
    pub seen: std::collections::HashMap<SessionId, usize>,
    /// Where the items the person had not seen began when they opened each session: the
    /// "New" line.
    pub new_from: std::collections::HashMap<SessionId, usize>,
    /// The effort slider's thumb in pixels along its travel: where the pointer holds it,
    /// then the spring that carries it to its stop once let go.
    pub effort_thumb: Option<limn::ScalarSpring>,
    pub effort_dragging: bool,
    /// Where the thumb was in the drag's last moments, for the speed it is let go at.
    pub effort_samples: Vec<(std::time::Instant, f32)>,
    /// Since when the open effort slider has stood on Ultracode, for its pixel field.
    pub ultra_since: Option<std::time::Instant>,
}

/// The sidebar's groups: by folder, Desktop's default, by status, or one list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum GroupBy {
    #[default]
    Folder,
    /// Needs input, Working and Completed.
    Status,
    None,
}

/// The order within a group. "Last updated" counts what happened in this run; sessions
/// read back at start keep the order they were made in until they change.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum SortBy {
    #[default]
    Updated,
    Created,
    Name,
}

/// The two ways to allow a permission; the card's main button is the one used last.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Allow {
    Once,
    Always,
}

/// Whether a link opens on the computer rather than in quill's browser: any web link while
/// "Open links in built-in browser" is off, and a pull request's while "Open pull requests in
/// browser" is on. The UI and the host both ask this, so they cannot disagree.
pub fn opens_outside(settings: &quill_model::Settings, url: &str) -> bool {
    (url.starts_with("http://") || url.starts_with("https://"))
        && (!settings.links_in_browser
            || (settings.prs_in_browser && quill_model::is_pull_request(url)))
}

/// What the branch bar asks of the agent to open a pull request: commit what is left, push,
/// open it with `gh`, as a draft when that kind was chosen, and give its address. Desktop
/// sends its own request of the same steps; these words are quill's.
fn pr_request(draft: bool, prefix: &str) -> String {
    let (kind, open) = if draft {
        ("as a draft", "`gh pr create --draft`")
    } else {
        ("ready for review", "`gh pr create`, without `--draft`")
    };
    format!(
        "Create a pull request for the changes in this session, {kind}.\n\n\
         1. If the current branch is the repository's default branch, first create and switch \
         to a new branch named `{prefix}/` followed by a few words naming the change, in \
         kebab-case.\n\
         2. If there are uncommitted changes, commit them with a message that says what \
         changed and why.\n\
         3. Push the branch.\n\
         4. Open the pull request with {open}. Give it a title that sums up the change and a \
         body that says what changed, why, and what a reviewer should know; follow the \
         repository's template if it has one.\n\
         5. Reply with the pull request's address.\n\n\
         If this branch has a pull request already, push any new commits and give its address \
         instead of opening another."
    )
}

/// The sidebar's and the top bar's own clicks: hiding the sidebar, search, and what a
/// session's menus do. `None` when `id` is none of them.
fn session_clicked(store: &Store, ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    let session = |prefix: &str| -> Option<SessionId> { id.strip_prefix(prefix)?.parse().ok() };
    if id == view::SIDEBAR_TOGGLE {
        ui.sidebar.toggle();
    } else if id == view::SEARCH {
        ui.search = Some(String::new());
    } else if id == view::SEARCH_CLOSE {
        ui.search = None;
    } else if id == view::SEARCH_NEW {
        return Some(clicked(store, ui, view::NEW_SESSION));
    } else if id == view::TITLE_MENU {
        ui.open_menu = (ui.open_menu != Some(view::TITLE_MENU)).then_some(view::TITLE_MENU);
    } else if id == format!("{}:dismiss", view::TITLE_MENU) {
        ui.open_menu = None;
    } else if id == view::OPEN_IN_MENU {
        ui.open_menu = (ui.open_menu != Some(view::OPEN_IN_MENU)).then_some(view::OPEN_IN_MENU);
    } else if id == format!("{}:dismiss", view::OPEN_IN_MENU) {
        ui.open_menu = None;
    } else if let Some((place, _)) = id.strip_prefix("open-in:").and_then(|r| r.split_once(':')) {
        // The host opens it; an editor chosen is the one the files' buttons use next, as
        // Desktop keeps the last one.
        ui.open_menu = None;
        if !matches!(place, "default" | "files") && place != ui.settings.editor {
            return Some(vec![Intent::SaveSettings(quill_model::Settings {
                editor: place.to_owned(),
                ..ui.settings.clone()
            })]);
        }
    } else if id == view::PR_MENU {
        ui.open_menu = (ui.open_menu != Some(view::PR_MENU)).then_some(view::PR_MENU);
    } else if id == format!("{}:dismiss", view::PR_MENU) {
        ui.open_menu = None;
    } else if let Some(kind) = id.strip_prefix("pr-kind:") {
        ui.open_menu = None;
        // The kind chosen is the next one's too, kept in Settings.
        return Some(vec![Intent::SaveSettings(quill_model::Settings {
            pr_draft: kind == "draft",
            ..ui.settings.clone()
        })]);
    } else if id == view::PR_CREATE {
        let session = ui.selected.and_then(|s| store.session(s));
        return Some(match session {
            Some(s) if view::pr_blocked(s).is_none() => vec![Intent::Send {
                session: s.id,
                text: pr_request(ui.settings.pr_draft, ui.settings.branch_prefix()),
                mentions: vec![],
                images: vec![],
                lane: Lane::default(),
            }],
            _ => vec![],
        });
    } else if id == view::BAR_CLOSE {
        ui.bar_closed.extend(ui.selected);
    } else if id == view::QUEUE_TOGGLE {
        if let Some(session) = ui.selected {
            if ui.queue_expanded.contains(&session) {
                ui.queue_expanded.retain(|s| *s != session);
            } else {
                ui.queue_expanded.push(session);
            }
        }
    } else if id == view::QUEUE_CLEAR {
        return Some(
            ui.selected
                .map(|session| vec![Intent::QueueClear { session }])
                .unwrap_or_default(),
        );
    } else if let Some(rest) = id.strip_prefix(view::QUEUE_MENU) {
        // A row's "⋮" opens its menu, and the menu's own dismiss closes it.
        ui.queue_menu = match rest.strip_suffix(":dismiss") {
            Some(_) => None,
            None => (ui.queue_menu.as_deref() != Some(rest)).then(|| rest.to_owned()),
        };
    } else if let Some(intent) = queue_row_intent(ui, id) {
        return Some(vec![intent]);
    } else if id == view::DELETE_CANCEL {
        ui.deleting = None;
    } else if id == view::DELETE_CONFIRM {
        return Some(
            ui.deleting
                .take()
                .map(|session| vec![Intent::Delete { session }])
                .unwrap_or_default(),
        );
    } else if let Some(s) = id
        .strip_suffix(":dismiss")
        .and_then(|rest| rest.strip_prefix("row-menu:"))
    {
        if s.parse().ok() == ui.row_menu {
            ui.row_menu = None;
        }
    } else if let Some(s) = session("row-menu:") {
        ui.row_menu = (ui.row_menu != Some(s)).then_some(s);
    } else if let Some(s) = session("rename:") {
        ui.row_menu = None;
        ui.open_menu = None;
        ui.selected = Some(s);
        ui.title_edit = store.session(s).map(|s| s.title.clone());
    } else if id == view::TRANSCRIPT_VIEW_CYCLE {
        let s = ui.selected.and_then(|s| store.session(s))?;
        let view = next_view(s, &ui.settings);
        return Some(vec![Intent::ChooseView {
            session: s.id,
            view: Some(view),
        }]);
    } else if let Some(rest) = id.strip_prefix("view:") {
        let (s, view) = rest.split_once(':')?;
        let view = match view {
            "normal" => TranscriptView::Normal,
            "thinking" => TranscriptView::Thinking,
            "verbose" => TranscriptView::Verbose,
            _ => return None,
        };
        return Some(vec![Intent::ChooseView {
            session: s.parse().ok()?,
            view: Some(view),
        }]);
    } else if let Some(s) = session("view-default:") {
        // The session's view becomes the default, and the session follows it again.
        let view = current_view(store.session(s)?, &ui.settings);
        return Some(vec![
            Intent::SaveSettings(Settings {
                transcript_view: view,
                ..ui.settings.clone()
            }),
            Intent::ChooseView {
                session: s,
                view: None,
            },
        ]);
    } else if let Some(s) = session("pin:") {
        ui.row_menu = None;
        let pinned = !store.session(s).is_some_and(|s| s.pinned);
        return Some(vec![Intent::Pin { session: s, pinned }]);
    } else if let Some(s) = session("archive:") {
        ui.row_menu = None;
        ui.open_menu = None;
        let archived = !store.session(s).is_some_and(|s| s.archived);
        return Some(vec![Intent::Archive {
            session: s,
            archived,
        }]);
    } else if let Some(s) = session("unread:") {
        ui.row_menu = None;
        let unread = !store.session(s).is_some_and(|s| s.unread);
        return Some(vec![Intent::MarkUnread { session: s, unread }]);
    } else if let Some(s) = session("thread:") {
        // A session another started opens beside it, as Desktop's threads do; inferred: the same pane as a side chat.
        if ui.side_chat != Some(s) || !ui.pane_open(Pane::Side) {
            ui.mark_new(store, s);
        }
        ui.side_chat = Some(s);
        ui.open_pane(Pane::Side);
        if store.session(s).is_some_and(|s| s.unread) {
            return Some(vec![Intent::MarkUnread {
                session: s,
                unread: false,
            }]);
        }
    } else if id == view::SIDE_OPEN {
        return ui
            .side_chat
            .map(|s| clicked(store, ui, &format!("session:{s}")));
    } else if let Some(s) = session("fork:") {
        ui.row_menu = None;
        ui.open_menu = None;
        // The fork opens as soon as the engine makes it, as a new session does.
        ui.waiting_fork = true;
        return Some(vec![Intent::Fork {
            session: s,
            at: None,
        }]);
    } else if let Some(key) = id.strip_prefix("filter-menu:") {
        match key.strip_suffix(":dismiss") {
            Some(key) if ui.filter_menu.as_deref() == Some(key) => ui.filter_menu = None,
            Some(_) => {}
            None => {
                ui.filter_menu = (ui.filter_menu.as_deref() != Some(key)).then(|| key.to_owned())
            }
        }
    } else if let Some(by) = id.strip_prefix("group-by:") {
        ui.filter_menu = None;
        ui.group_by = match by {
            "status" => GroupBy::Status,
            "none" => GroupBy::None,
            _ => GroupBy::Folder,
        };
    } else if let Some(by) = id.strip_prefix("sort-by:") {
        ui.filter_menu = None;
        ui.sort_by = match by {
            "created" => SortBy::Created,
            "name" => SortBy::Name,
            _ => SortBy::Updated,
        };
    } else if let Some(way) = id.strip_prefix("sort-way:") {
        ui.filter_menu = None;
        ui.ascending = way == "ascending";
    } else if let Some((what, task)) = id
        .strip_prefix("proposal:")
        .and_then(|rest| rest.split_once(':'))
    {
        let answer = match what {
            "start" => ProposalAnswer::Start,
            "here" => ProposalAnswer::Here,
            _ => ProposalAnswer::Dismiss,
        };
        return Some(
            ui.selected
                .map(|session| {
                    vec![Intent::AnswerProposal {
                        session,
                        task: task.to_owned(),
                        answer,
                    }]
                })
                .unwrap_or_default(),
        );
    } else if let Some(tasks) = id.strip_prefix("proposal-all:") {
        return Some(
            ui.selected
                .map(|session| {
                    tasks
                        .split(',')
                        .map(|task| Intent::AnswerProposal {
                            session,
                            task: task.to_owned(),
                            answer: ProposalAnswer::Start,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        );
    } else if id.starts_with("report:") {
        match ui.expanded.iter().position(|k| k == id) {
            Some(at) => drop(ui.expanded.remove(at)),
            None => ui.expanded.push(id.to_owned()),
        }
    } else if id == view::SHOW_MORE {
        ui.show_more = !ui.show_more;
    } else if id == view::FILTER_RESET {
        ui.filter_menu = None;
        (ui.group_by, ui.sort_by, ui.ascending) = Default::default();
    } else if id.starts_with("open-in:") {
        // The host opens the folder; the menus close.
        ui.row_menu = None;
        ui.open_menu = None;
    } else if let Some(s) = session("delete:") {
        ui.row_menu = None;
        ui.open_menu = None;
        ui.deleting = Some(s);
    } else {
        return None;
    }
    Some(vec![])
}

/// The click a key press stands for, wherever the focus is, as in Desktop: on a
/// permission Ctrl+Enter allows once and Ctrl+Shift+Enter always; on a question Ctrl+Enter
/// goes on or submits; on a plan Ctrl+Enter accepts into the
/// most trusting mode offered and Ctrl+Shift+Enter into Manual.
pub fn shortcut(store: &Store, ui: &Ui, key: limn::Key, ctrl: bool, shift: bool) -> Option<String> {
    if key != limn::Key::Enter || !ctrl {
        return None;
    }
    let session = store.session(ui.selected?)?;
    let (id, tool, input) = session.items.iter().rev().find_map(|item| match item {
        Item::Permission {
            id,
            tool,
            input,
            decided: None,
            ..
        } => Some((id, tool, input)),
        _ => None,
    })?;
    if input["questions"].is_array() {
        return Some(format!("ask:{id}:next"));
    }
    if tool == "ExitPlanMode" {
        let auto = session
            .capabilities
            .as_ref()
            .is_some_and(|c| c.modes.iter().any(|m| m == "auto"));
        let mode = match (shift, auto) {
            (true, _) => "default",
            (false, true) => "auto",
            (false, false) => "acceptEdits",
        };
        return Some(format!("planok:{id}:{mode}"));
    }
    Some(format!(
        "perm:{id}:{}",
        if shift { "always" } else { "once" }
    ))
}

/// A session's browser as the engine last told it (docs/browser.md).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionBrowser {
    pub tabs: Vec<quill_model::BrowserTab>,
    pub active: Option<u32>,
    /// The page's cursor under the pointer, as CSS names it.
    pub cursor: String,
    /// Why it could not start or stopped, until a tab opens again.
    pub failed: Option<String>,
    /// The page waits for the person to pick an element.
    pub selecting: bool,
}

impl SessionBrowser {
    fn heard(&mut self, update: quill_model::BrowserUpdate) {
        use quill_model::BrowserUpdate;
        match update {
            BrowserUpdate::Tabs { tabs, active } => {
                if !tabs.is_empty() {
                    self.failed = None;
                }
                self.tabs = tabs;
                self.active = active;
            }
            BrowserUpdate::Cursor(cursor) => self.cursor = cursor,
            BrowserUpdate::Failed(error) => self.failed = Some(error),
            // The view keeps notices until dismissed; the host puts copies on the clipboard.
            BrowserUpdate::Selecting(on) => self.selecting = on,
            BrowserUpdate::Notice(_)
            | BrowserUpdate::Copied(_)
            | BrowserUpdate::Selected(_)
            | BrowserUpdate::Kept(_)
            | BrowserUpdate::ChooseFiles { .. } => {}
        }
    }

    pub fn tab(&self) -> Option<&quill_model::BrowserTab> {
        self.tabs.iter().find(|t| Some(t.id) == self.active)
    }
}

/// What can open in the column beside the conversation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pane {
    Diff,
    Terminal,
    /// A helper agent's steps and words, the one `Ui::helper` names.
    Agent,
    /// The plan `Ui::plan` names.
    Plan,
    /// The side chat `Ui::side_chat` names: a `/btw` question beside the session.
    Side,
    /// The session's built-in browser.
    Browser,
    /// The session's folder, as a tree of its files.
    Files,
    /// What the session's agent runs beside its turns, Desktop's Background tasks.
    Tasks,
}

impl Pane {
    pub const ALL: [Pane; 8] = [
        Pane::Diff,
        Pane::Terminal,
        Pane::Agent,
        Pane::Plan,
        Pane::Side,
        Pane::Browser,
        Pane::Files,
        Pane::Tasks,
    ];

    /// The share of the room right of the sidebar this pane opens the column at. The widest
    /// pane open sets it, so a document opened beside the tree gets a reader's width.
    pub fn share(self) -> f32 {
        match self {
            // The diff takes 3 of 5 parts, a terminal 1 of 3 (measured at 1×, see look.rs).
            Pane::Diff => 0.6,
            // A helper agent and a plan open at a third too.
            Pane::Terminal | Pane::Agent | Pane::Plan | Pane::Tasks => 1.0 / 3.0,
            // A session beside reads at about Desktop's thread pane.
            Pane::Side => 0.42,
            // Inferred: a file reads best at half the room, its tree beside it, and so does a
            // page.
            Pane::Files | Pane::Browser => 0.5,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Pane::Diff => "diff",
            Pane::Terminal => "terminal",
            Pane::Agent => "agent",
            Pane::Plan => "plan",
            Pane::Side => "side",
            Pane::Browser => "browser",
            Pane::Files => "files",
            Pane::Tasks => "tasks",
        }
    }
}

/// A file of a session's folder open in the document pane: its text in an editor the person
/// may change, and whether it shows rendered (Markdown) or as text.
pub struct Document {
    pub session: SessionId,
    /// Relative to the session's folder.
    pub path: String,
    /// `None` until the engine read it.
    pub editor: Option<limn::EditorController>,
    /// It could not be read as text.
    pub unreadable: bool,
    /// Changed since it was read or saved.
    pub dirty: bool,
    /// Markdown shows rendered until the person edits it.
    pub preview: bool,
    pub error: Option<String>,
}

impl Document {
    pub fn markdown(&self) -> bool {
        self.path.ends_with(".md") || self.path.ends_with(".markdown")
    }

    /// What the editor holds now.
    pub fn text(&self) -> Option<String> {
        let snapshot = self.editor.as_ref()?.snapshot().ok()?;
        let mut out = Vec::new();
        snapshot.write_to(&mut out).ok()?;
        String::from_utf8(out).ok()
    }
}

/// The terminal pane: what the host's shells last showed, and what the person asked of them.
#[derive(Default)]
pub struct TerminalPane {
    /// The folder's tabs, as the host's shells last were.
    pub tabs: Vec<TerminalTab>,
    pub active: usize,
    pub screen: Option<std::sync::Arc<limn::TerminalScreen>>,
    /// A close that waits for the person to confirm, as a command runs where it closes.
    pub closing: Option<Closing>,
    /// The tab whose pill is a name field, and what the field holds.
    pub renaming: Option<(usize, String)>,
    /// What the host does with the folder's shells on the next frame.
    pub requests: Vec<TerminalRequest>,
}

/// One terminal tab as the pane shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalTab {
    pub title: String,
    pub busy: bool,
    /// Named by the person: its pill drops the folder.
    pub named: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Closing {
    One(usize),
    /// Every tab but this one.
    Others(usize),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TerminalRequest {
    New,
    Select(usize),
    Close(usize),
    CloseOthers(usize),
    Rename(usize, Option<String>),
    Clear(usize),
    Move {
        from: usize,
        to: usize,
    },
    /// A tab named `name` that follows the file a background command writes its output to.
    Output {
        name: String,
        path: PathBuf,
    },
}

impl Ui {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            selected: None,
            prompt: String::new(),
            cwd,
            // The first agent available, once the engine says which (`Event::Settings`).
            agent: String::new(),
            mode: String::new(),
            model: None,
            effort: None,
            open_menu: None,
            link_menu: None,
            editors: vec![],
            expanded: vec![],
            waiting: None,
            images: vec![],
            queue_submit: false,
            thumbs: Default::default(),
            logos: Default::default(),
            logo_asked: Default::default(),
            viewer: None,
            full: Default::default(),
            annotating: None,
            marks: vec![],
            redo: vec![],
            tool: limn::SketchTool::Pen,
            last_shape: limn::SketchTool::Rectangle,
            ink: annotate::INKS[0].1,
            stroke: annotate::WIDTHS[1].1,
            discarding: false,
            saving: false,
            sizes: Default::default(),
            image_notice: None,
            diff: None,
            panes: vec![],
            expanded_pane: None,
            panel_open: true,
            panel_width: look::PANEL_WIDTH,
            collapsed: vec![],
            last_allow: Allow::Once,
            name: None,
            utc_offset: 0,
            window: limn::Size::new(1408.0, 844.0),
            at_end: true,
            column_fraction: None,
            stack_fraction: None,
            answers: Default::default(),
            perm_menu: None,
            helper: None,
            plan: None,
            question: Default::default(),
            other: Default::default(),
            revising: None,
            revision: String::new(),
            terminal: TerminalPane::default(),
            sidebar: quill_components::sidebar::Sidebar::new(look::SIDEBAR_WIDTH),
            pointer: None,
            hovered: None,
            welcome_since: Default::default(),
            skip_welcome: false,
            tour: None,
            panel_shown: limn::ScalarSpring::new(look::PANEL_WIDTH, std::time::Instant::now()),
            column_shown: limn::ScalarSpring::new(0.0, std::time::Instant::now()),
            closing: None,
            column_share: limn::ScalarSpring::new(0.0, std::time::Instant::now()),
            expand_shown: limn::ScalarSpring::new(0.0, std::time::Instant::now()),
            stack_shown: limn::ScalarSpring::new(0.0, std::time::Instant::now()),
            stack_closing: None,
            row_menu: None,
            title_edit: None,
            deleting: None,
            search: None,
            mcp: None,
            bar_closed: vec![],
            queue_expanded: vec![],
            queue_menu: None,
            group_by: GroupBy::default(),
            sort_by: SortBy::default(),
            ascending: false,
            live: Default::default(),
            filter_menu: None,
            diff_menu: None,
            waiting_fork: false,
            restore_prompt: None,
            menu_from_palette: false,
            slash_selected: 0,
            slash_dismissed: None,
            mention_selected: 0,
            mention_dismissed: None,
            mentioned_sessions: vec![],
            mention_found: None,
            show_more: false,
            side_chat: None,
            side_waiting: false,
            side_reply: String::new(),
            copied: None,
            reading: None,
            files: std::collections::HashMap::new(),
            files_focus: None,
            files_reveal: None,
            files_copy: None,
            remote: None,
            remote_asked: None,
            qr: None,
            browser_tabs: Default::default(),
            browser_notice: Default::default(),
            settings: Default::default(),
            agents: vec![],
            agent_models: Default::default(),
            settings_page: None,
            account_menu: false,
            github: Default::default(),
            account: Default::default(),
            update: quill_model::UpdateState::Idle,
            update_asking: false,
            restart_when_idle: false,
            settings_query: String::new(),
            settings_menu: None,
            storage: None,
            settings_error: None,
            extensions: std::collections::HashMap::new(),
            connectors: Connectors::default(),
            global: Default::default(),
            skill_dialog: None,
            removing: None,
            marketplace_field: String::new(),
            market_targets: vec![],
            customize_query: String::new(),
            directory_query: String::new(),
            directory: None,
            directory_market: None,
            marketplaces: None,
            reduced_motion: false,
            seen: Default::default(),
            new_from: Default::default(),
            effort_thumb: None,
            effort_dragging: false,
            effort_samples: vec![],
            ultra_since: None,
        }
    }

    /// Something the UI moves itself is still moving, so the host keeps painting.
    pub fn animating(&self) -> bool {
        let now = std::time::Instant::now();
        self.effort_thumb.is_some_and(|thumb| thumb.is_active(now))
            || self.sidebar.animating(now)
            || self.panel_shown.is_active(now)
            || self.column_shown.is_active(now)
            || self.column_share.is_active(now)
            || self.expand_shown.is_active(now)
            || self.stack_shown.is_active(now)
    }

    /// When the host should paint again for what moves on its own: at once for a moving
    /// thumb, every 33 ms for the Ultracode field, as the reference draws it, and when a copy
    /// button's check is due to go.
    pub fn repaint_after(&self) -> Option<std::time::Duration> {
        let check = self
            .copied
            .as_ref()
            .and_then(|(_, at)| view::COPIED_FOR.checked_sub(at.elapsed()));
        let ultra = self
            .ultra_since
            .map(|_| std::time::Duration::from_millis(33));
        let mcp = (self.mcp.as_ref()).map(|m| MCP_POLL.saturating_sub(m.asked.elapsed()));
        if self.animating() {
            Some(std::time::Duration::ZERO)
        } else {
            [check, ultra, mcp].into_iter().flatten().min()
        }
    }

    /// Ask for every agent's plugins and skills, for Customize's folder.
    /// Customize › Connectors opened: each agent's MCP servers listed again, and the registry
    /// asked once for its first page.
    fn ask_servers(&mut self) -> Vec<Intent> {
        let folder = customize_folder(self);
        let mut intents = vec![];
        for agent in &self.agents {
            self.connectors
                .listed
                .entry(agent.id.clone())
                .or_default()
                .working = true;
            intents.push(Intent::Servers {
                agent: agent.id.clone(),
                folder: folder.clone(),
            });
        }
        intents.push(Intent::Global(quill_model::GlobalOp::Load));
        if self.connectors.found.is_none() {
            intents.push(Intent::FindServers {
                query: self.connectors.query.clone(),
            });
        }
        intents
    }

    fn ask_extensions(&mut self) -> Vec<Intent> {
        let folder = customize_folder(self);
        for agent in &self.agents {
            let known = self.extensions.entry(agent.id.clone()).or_default();
            known.folder = folder.clone();
            known.working = Some(String::new());
        }
        let agents = self.agents.iter().map(|a| Intent::Extensions {
            agent: a.id.clone(),
            folder: folder.clone(),
        });
        agents
            .chain([Intent::Global(quill_model::GlobalOp::Load)])
            .collect()
    }

    /// The logos the open Connectors or Plugins page, or MCP dialog, shows and nothing asked for
    /// yet, asked for once each; none while logos are off.
    pub fn logos_due(&mut self, store: &Store) -> Vec<Intent> {
        if !self.settings.logos {
            return vec![];
        }
        let page = self.settings_page.as_deref();
        let mut wanted = vec![];
        if page == Some(view::SETTINGS_CONNECTORS) && !self.connectors.discover {
            for server in self
                .connectors
                .listed
                .values()
                .flat_map(|l| l.servers.iter().flatten())
            {
                let name = server_logo_name(self, &server.name, &server.label);
                wanted.push(LogoWanted {
                    key: format!("mcp:{name}"),
                    name,
                    url: https_address(&server.target),
                });
            }
        }
        if page == Some(view::SETTINGS_PLUGINS) {
            for plugin in self.extensions.values().flat_map(|k| &k.list.plugins) {
                wanted.push(LogoWanted {
                    key: format!("plugin:{}", plugin.name),
                    name: plugin.name.clone(),
                    url: None,
                });
            }
        }
        if let Some(mcp) = &self.mcp {
            let servers = store.session(mcp.session).and_then(|s| s.mcp.as_ref());
            for server in servers.into_iter().flatten() {
                // The address is the one the Connectors page lists under the same name.
                let url = (self.connectors.listed.values())
                    .flat_map(|l| l.servers.iter().flatten())
                    .find(|c| c.name == server.name)
                    .and_then(|c| https_address(&c.target));
                let name = server_logo_name(self, &server.name, &server.label);
                wanted.push(LogoWanted {
                    key: format!("mcp:{name}"),
                    name,
                    url,
                });
            }
        }
        wanted.retain(|w| self.logo_asked.insert(w.key.clone()));
        if wanted.is_empty() {
            vec![]
        } else {
            vec![Intent::Logos { wanted }]
        }
    }

    /// What is due to be asked on its own: the open MCP dialog's servers, every
    /// [`MCP_POLL`], so one that connects or fails shows it; Customize's lists, read again
    /// for a folder chosen while it shows.
    pub fn due(&mut self) -> Vec<Intent> {
        let customizing = (self.settings_page.as_deref())
            .is_some_and(|p| [view::SETTINGS_PLUGINS, view::SETTINGS_SKILLS].contains(&p));
        let folder = customize_folder(self);
        if customizing && self.extensions.values().any(|k| k.folder != folder) {
            return self.ask_extensions();
        }
        match &mut self.mcp {
            Some(mcp) if mcp.asked.elapsed() >= MCP_POLL => {
                mcp.asked = std::time::Instant::now();
                vec![Intent::McpStatus {
                    session: mcp.session,
                }]
            }
            _ => vec![],
        }
    }

    /// Before each frame: the sidebar and the panel head for where they were toggled to, and
    /// the Ultracode field starts over each time the slider reaches it.
    pub fn tick(&mut self, store: &Store) {
        for id in [self.selected, self.side_chat].into_iter().flatten() {
            if let Some(session) = store.session(id) {
                self.seen.insert(id, session.items.len());
            }
        }
        let now = std::time::Instant::now();
        // The pace of limn's own opening and closing slots, one and a half `move_ms`;
        // Desktop's own timing is not measured.
        let motion = limn::SpringMotion::settling_in(std::time::Duration::from_millis(
            u64::from(limn::tokens().motion.move_ms) * 3 / 2,
        ));
        let open = |open: bool, width: f32| if open { width } else { 0.0 };
        let pointer = quill_components::sidebar::Pointer {
            at: self.pointer,
            on_toggle: self.hovered.as_deref() == Some(view::SIDEBAR_TOGGLE),
            // Its edges count, as `x <= 48` did.
            toggle_zone: limn::Rect::new(0.0, 0.0, 48f32.next_up(), look::TOP_BAR.next_up()),
            held: self.open_menu.is_some() || self.account_menu,
        };
        self.sidebar
            .tick(&pointer, motion, self.reduced_motion, now);
        // The panel gives way to the panes as they open, and comes back as they shut.
        self.panel_shown.launch(
            open(self.panel_open && self.panes.is_empty(), self.panel_width),
            motion,
            now,
        );
        self.column_shown
            .launch(open(!self.panes.is_empty(), 1.0), motion, now);
        // The widest pane open sets the column's share, unless the person dragged it; it
        // grows and shrinks on the spring, but a column that opens takes it at once, as the
        // column slides in on its own.
        if !self.panes.is_empty() {
            let share = self
                .column_fraction
                .unwrap_or_else(|| self.panes.iter().map(|p| p.share()).fold(0.0, f32::max));
            if self.column_shown.value(now) < 0.01 {
                self.column_share.snap_to(share, now);
            } else {
                self.column_share.launch(share, motion, now);
            }
        }
        self.expand_shown
            .launch(open(self.expanded_pane.is_some(), 1.0), motion, now);
        self.stack_shown
            .launch(open(self.panes.len() >= 2, 1.0), motion, now);
        if self.reduced_motion {
            self.panel_shown.snap_to(self.panel_shown.target(), now);
            self.column_shown.snap_to(self.column_shown.target(), now);
            self.column_share.snap_to(self.column_share.target(), now);
            self.expand_shown.snap_to(self.expand_shown.target(), now);
            self.stack_shown.snap_to(self.stack_shown.target(), now);
        }
        // A stacked pane closed is forgotten once the stack has closed over it, or once
        // another opened in its place.
        if let Some((_, pane)) = self.stack_closing {
            if self.panes.len() != 1 || !self.stack_shown.is_active(now) {
                self.stack_closing = None;
                if !self.pane_open(pane) {
                    self.forget_pane(pane);
                }
            }
        }
        // The last pane closed is forgotten once the column has shut, or once another opened.
        if let Some(pane) = self.closing {
            if !self.panes.is_empty() || !self.column_shown.is_active(now) {
                self.closing = None;
                if !self.pane_open(pane) {
                    self.forget_pane(pane);
                }
            }
        }
        let ultra = view::effort_shown(store, self).is_some_and(|e| view::ultra_effort(&e));
        match (ultra, self.ultra_since) {
            (true, None) => self.ultra_since = Some(std::time::Instant::now()),
            (false, Some(_)) => self.ultra_since = None,
            _ => {}
        }
    }

    /// A new session runs `agent` from now on, starting from its own mode; model and effort
    /// are the agent's default until Settings says otherwise.
    fn take_agent(&mut self, agent: &str) {
        self.agent = agent.to_owned();
        self.take_defaults();
    }

    /// What a new session starts with: the agent's defaults from Settings, else its own. All
    /// three are set every time, so nothing chosen for another agent (or in another session)
    /// carries over: a Claude model never reaches a Codex session.
    fn take_defaults(&mut self) {
        let defaults = self.settings.agent(&self.agent);
        self.model = defaults.model;
        self.effort = defaults.effort;
        let own = (self.agents.iter().find(|a| a.id == self.agent)).map(|a| a.default_mode.clone());
        if let Some(mode) = defaults.mode.or(own) {
            self.mode = mode;
        }
    }

    /// Opening `session`: what came since the person last had it in view is new.
    fn mark_new(&mut self, store: &Store, session: SessionId) {
        let len = store.session(session).map_or(0, |s| s.items.len());
        match self.seen.get(&session) {
            Some(&seen) if seen < len => {
                self.new_from.insert(session, seen);
            }
            _ => {
                self.new_from.remove(&session);
            }
        }
    }

    /// Whether the right panel's Context card is open (it starts open; `expanded` lists it
    /// once folded).
    pub fn context_open(&self) -> bool {
        !self.expanded.iter().any(|k| k == view::CONTEXT_CARD)
    }

    /// The Files pane of the session on screen.
    pub fn files_pane(&self) -> Option<&files::FilesPane> {
        self.files.get(&self.selected?)
    }

    /// The file the Files pane shows, of the session on screen.
    pub fn document(&self) -> Option<&Document> {
        self.files_pane()?.document()
    }

    pub fn pane_open(&self, pane: Pane) -> bool {
        self.panes.contains(&pane)
    }

    pub fn open_pane(&mut self, pane: Pane) {
        if !self.pane_open(pane) {
            self.panes.push(pane);
        }
    }

    pub fn close_pane(&mut self, pane: Pane) {
        let Some(at) = self.panes.iter().position(|p| *p == pane) else {
            return;
        };
        self.panes.remove(at);
        if self.expanded_pane == Some(pane) {
            self.expanded_pane = None;
        }
        // The last one keeps what it shows while the column shuts, and one of two while the
        // stack closes over it; `tick` forgets them after.
        if self.panes.is_empty() {
            if let Some(before) = self.closing.replace(pane).filter(|p| *p != pane) {
                self.forget_pane(before);
            }
        } else if self.panes.len() == 1 {
            if let Some((_, before)) = self.stack_closing.replace((at, pane)) {
                if before != pane && !self.pane_open(before) {
                    self.forget_pane(before);
                }
            }
        } else {
            self.forget_pane(pane);
        }
    }

    fn forget_pane(&mut self, pane: Pane) {
        match pane {
            Pane::Diff => self.diff = None,
            Pane::Agent => self.helper = None,
            Pane::Plan => self.plan = None,
            Pane::Side => self.side_chat = None,
            // A session's tree stays as it was, to open again where it was.
            Pane::Terminal | Pane::Browser | Pane::Files | Pane::Tasks => {}
        }
    }

    /// After the engine reported `event`: select a session the person just created
    /// and send what they wrote while it was being made.
    pub fn applied(&mut self, event: &Event) -> Vec<Intent> {
        mention::heard(self, event);
        updates::heard(self, event);
        tour::heard(self, event);
        remote::heard(self, event);
        let mut intents = account::heard(self, event);
        intents.extend(github::heard(self, event));
        intents.extend(files::heard(self, event));
        intents.extend(self.mcp_heard(event));
        intents.extend(self.applied_to_view(event));
        intents
    }

    fn applied_to_view(&mut self, event: &Event) -> Vec<Intent> {
        // A queued message taken back goes into the box, after what the person had written.
        if let Event::QueueTaken { text, images, .. } = event {
            let mut prompt = self.prompt.trim_end().to_owned();
            if !prompt.is_empty() {
                prompt.push_str("\n\n");
            }
            prompt.push_str(text);
            self.restore_prompt = Some(prompt);
            self.images.extend(images.iter().cloned());
            return vec![];
        }
        // A tab that appears in the session on screen, as when the agent opens a page, shows
        // the browser, as Desktop's does.
        if let Event::Browser {
            session,
            update: quill_model::BrowserUpdate::Tabs { tabs, .. },
        } = event
        {
            let before = self.browser_tabs.insert(*session, tabs.len()).unwrap_or(0);
            if self.selected == Some(*session) && tabs.len() > before {
                self.open_pane(Pane::Browser);
            }
        }
        if let Event::Browser {
            session,
            update:
                quill_model::BrowserUpdate::Notice(words) | quill_model::BrowserUpdate::Failed(words),
        } = event
        {
            self.browser_notice.insert(*session, words.clone());
        }
        if let Event::AgentModels { agent, models } = event {
            self.agent_models.insert(agent.clone(), models.clone());
        }
        if let Event::Settings { settings, agents } = event {
            look::set_text_size(settings.text_size);
            look::set_width(settings.width);
            let first = self.agents.is_empty();
            self.settings = settings.clone();
            // Welcomed before the appearance step existed (the first settings of a run already
            // say so): not asked now. Later ones echo a Get started pressed in this run.
            self.settings.looked |= first && settings.welcomed;
            self.agents = agents.clone();
            // The agent last chosen, else the first available, until there is one in use.
            if !agents.iter().any(|a| a.id == self.agent) {
                let wanted = settings.default_agent.as_deref();
                if let Some(agent) = agents
                    .iter()
                    .find(|a| Some(a.id.as_str()) == wanted)
                    .or(agents.first())
                {
                    self.take_agent(&agent.id.clone());
                }
            }
            if self.selected.is_none() {
                self.take_defaults();
            }
            return vec![];
        }
        if let Event::Logo { key, light, dark } = event {
            self.logos
                .insert(key.clone(), (light.clone(), dark.clone()));
            return vec![];
        }
        if let Event::Storage { path, bytes } = event {
            self.storage = Some((path.clone(), *bytes));
            return vec![];
        }
        if let Event::SettingsNotSaved { error } = event {
            self.settings_error = Some(error.clone());
            return vec![];
        }
        if let Event::Global(state) = event {
            self.global = state.clone();
            return vec![];
        }
        if let Event::Servers {
            agent,
            servers,
            scopes,
            error,
            ..
        } = event
        {
            let listed = self.connectors.listed.entry(agent.clone()).or_default();
            listed.servers = Some(servers.clone());
            listed.scopes = scopes.clone();
            listed.error = error.clone();
            listed.working = false;
            return vec![];
        }
        // Only for the words in the search now: an answer to older ones is let go.
        if let Event::ServersFound {
            query,
            offers,
            error,
        } = event
        {
            if *query == self.connectors.query {
                self.connectors.found = Some((query.clone(), offers.clone(), error.clone()));
            }
            return vec![];
        }
        if let Event::Extensions {
            agent,
            folder,
            extensions,
            error,
        } = event
        {
            let done = self.extensions.get(agent).and_then(|k| k.done.clone());
            // Desktop's toast once an install went through.
            let notice = (done.filter(|_| error.is_none()))
                .map(|name| format!("{name} is installed and ready to use."));
            let known = Extensions {
                loaded: true,
                folder: folder.clone(),
                list: extensions.clone(),
                error: error.clone(),
                working: None,
                notice,
                done: None,
            };
            self.extensions.insert(agent.clone(), known);
            return vec![];
        }
        if let Event::Reading {
            session,
            reading: false,
        } = event
        {
            if self.reading.is_some_and(|(s, _)| s == *session) {
                self.reading = None;
            }
        }
        if let Event::Deleted { session } = event {
            if self.selected == Some(*session) {
                self.selected = None;
                self.title_edit = None;
                self.panes.clear();
                self.expanded_pane = None;
            }
        }
        if let Event::SessionCreated { session, .. } = event {
            // A side chat opens in its pane; the session in view stays.
            if std::mem::take(&mut self.side_waiting) {
                self.side_chat = Some(*session);
                self.open_pane(Pane::Side);
                return vec![];
            }
            if std::mem::take(&mut self.waiting_fork) {
                self.selected = Some(*session);
                self.title_edit = None;
                return vec![Intent::WorkingTree { session: *session }];
            }
            if let Some((text, mentions, images)) = self.waiting.take() {
                self.selected = Some(*session);
                // The branch bar shows from the start, not after the first step.
                return vec![
                    Intent::Send {
                        session: *session,
                        text,
                        mentions,
                        images,
                        lane: Lane::default(),
                    },
                    Intent::WorkingTree { session: *session },
                ];
            }
        }
        if let Event::Status {
            session,
            status: SessionStatus::Running,
        } = event
        {
            self.live.insert(*session);
        }
        // A turn that ends while the person looks elsewhere leaves news, as Desktop's
        // "ready" rows.
        if let Event::Agent {
            session,
            event: AgentEvent::TurnFinished { .. },
            ..
        } = event
        {
            // A side chat open in its pane is in view too.
            let seen = self.selected == Some(*session)
                || (self.side_chat == Some(*session) && self.pane_open(Pane::Side));
            if !seen && self.live.contains(session) {
                return vec![Intent::MarkUnread {
                    session: *session,
                    unread: true,
                }];
            }
        }
        // A finished turn or step may have changed files: what is uncommitted is asked for
        // again, so the branch bar's counts follow the work as it happens.
        if let Event::Agent {
            session,
            event: AgentEvent::TurnFinished { .. } | AgentEvent::ToolResult { .. },
            ..
        } = event
        {
            if self.selected == Some(*session) {
                let mut intents = vec![Intent::WorkingTree { session: *session }];
                // The breakdown follows each step and each turn while it is open, so the first
                // turn shows its share before it ends (Desktop's waits for the end).
                if self.context_open() {
                    intents.push(Intent::ContextUsage { session: *session });
                }
                return intents;
            }
        }
        vec![]
    }
}

/// What one action of the person asks of the engine; the rest changes `ui` only.
/// What `action` asks of the engine, and the Files pane's tree for the session on screen when
/// it has none yet (another session selected with the pane open).
/// The most images one message carries (Desktop's count for a local session).
pub const MAX_IMAGES: usize = 20;

/// Whether a file is an image the composer takes: PNG, JPEG, GIF or WebP by its extension.
pub fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        ["png", "jpg", "jpeg", "gif", "webp"]
            .iter()
            .any(|kind| e.eq_ignore_ascii_case(kind))
    })
}

/// Attaches image files to the composer's message, up to `MAX_IMAGES`; the rest are left out
/// and the composer says so.
pub fn add_images(ui: &mut Ui, paths: Vec<PathBuf>) {
    let room = MAX_IMAGES.saturating_sub(ui.images.len());
    ui.image_notice =
        (paths.len() > room).then(|| format!("Maximum of {MAX_IMAGES} images allowed."));
    ui.images.extend(paths.into_iter().take(room));
}

/// The images the host decodes thumbnails for: the composer's and those of the shown session's
/// messages.
/// The name a server's logo is looked up and kept by: the Connectors page's label for the
/// server called `name`, else `label`, else `name` itself.
/// Whether the welcome shows: once the settings are known (their agents with them), until
/// its Get started is pressed.
pub fn welcoming(ui: &Ui) -> bool {
    !ui.skip_welcome && !ui.agents.is_empty() && !ui.settings.welcomed
}

pub(crate) fn server_logo_name(ui: &Ui, name: &str, label: &str) -> String {
    let listed = (ui.connectors.listed.values())
        .flat_map(|l| l.servers.iter().flatten())
        .find(|c| c.name == name)
        .map(|c| c.label.as_str());
    [listed.unwrap_or_default(), label, name]
        .into_iter()
        .find(|n| !n.is_empty())
        .unwrap_or_default()
        .to_owned()
}

pub fn image_paths(store: &Store, ui: &Ui) -> Vec<PathBuf> {
    let items = ui
        .selected
        .and_then(|id| store.session(id))
        .map_or(&[][..], |s| &s.items[..]);
    let shown = items.iter().flat_map(|item| match item {
        Item::User { images, .. } => images.clone(),
        // The pictures of a step: a generated image, an image the agent read.
        Item::Tool {
            name,
            input,
            result,
            ..
        } => view::step_image(name, input, result.as_ref())
            .into_iter()
            .collect(),
        _ => vec![],
    });
    // The logos of this theme, while they are on.
    let logos = (ui.logos.values())
        .filter(|_| ui.settings.logos)
        .filter_map(|(light, dark)| {
            if look::is_dark() {
                dark.clone()
            } else {
                light.clone()
            }
        });
    let avatar = github::signed_in(ui).and_then(|(.., avatar)| avatar.cloned());
    ui.images
        .iter()
        .cloned()
        .chain(shown)
        .chain(logos)
        .chain(avatar)
        .collect()
}

/// A server's address, when it is an https one: the only kind a logo is looked up at.
fn https_address(target: &str) -> Option<String> {
    let target = target.trim();
    target.starts_with("https://").then(|| target.to_owned())
}

/// A queue row's button or menu item, by its id (`queue:send:<id>` and so on), for the
/// selected session. It closes the row's menu.
fn queue_row_intent(ui: &mut Ui, id: &str) -> Option<Intent> {
    let (action, entry) = id.strip_prefix("queue:")?.split_once(':')?;
    let session = ui.selected?;
    let id = entry.to_owned();
    let intent = match action {
        "send" => Intent::QueueSendNow { session, id },
        "take" => Intent::QueueTake { session, id },
        "remove" => Intent::QueueRemove { session, id },
        _ => return None,
    };
    ui.queue_menu = None;
    Some(intent)
}

/// ArrowUp in an empty composer takes the session's last queued message back for editing, as
/// Desktop does.
pub fn queue_key(
    store: &Store,
    ui: &Ui,
    focused: Option<&str>,
    event: &limn::UiEvent,
) -> Option<Vec<Intent>> {
    let limn::UiEvent::Key {
        key: limn::Key::ArrowUp,
        ..
    } = event
    else {
        return None;
    };
    if focused != Some(view::COMPOSER) || !ui.prompt.is_empty() || !ui.images.is_empty() {
        return None;
    }
    let session = store.session(ui.selected?)?;
    let last = session.queue.last()?;
    Some(vec![Intent::QueueTake {
        session: session.id,
        id: last.id.clone(),
    }])
}

/// Ctrl+Enter in the composer queues its message, a plain Enter steers it into the running
/// turn. Limn reports both as one `Submitted`, so the host says which key reached the composer
/// before the window handles it.
pub fn composer_key(ui: &mut Ui, focused: Option<&str>, event: &limn::UiEvent) {
    if let limn::UiEvent::Key { key, ctrl, .. } = event {
        ui.queue_submit = *key == limn::Key::Enter && *ctrl && focused == Some(view::COMPOSER);
    }
}

/// After the store took an event: what "Wait for agents" was waiting for.
pub fn settled(store: &Store, ui: &mut Ui) -> Vec<Intent> {
    updates::settled(store, ui)
}

pub fn update(store: &Store, ui: &mut Ui, action: &limn::UiAction) -> Vec<Intent> {
    let mut intents = match files::action(store, ui, action) {
        Some(intents) => intents,
        None => update_action(store, ui, action),
    };
    intents.extend(files::shown(store, ui));
    if ui.menu_from_palette && ui.open_menu.is_none() {
        ui.menu_from_palette = false;
        ui.slash_dismissed = None;
        ui.restore_prompt
            .get_or_insert_with(|| slash::without_token(&ui.prompt));
    }
    intents
}

fn update_action(store: &Store, ui: &mut Ui, action: &limn::UiAction) -> Vec<Intent> {
    use limn::UiAction;
    if annotate::action(ui, action) {
        return vec![];
    }
    match action {
        UiAction::InputChanged { id, value } if id == view::TERMINAL_RENAME => {
            if let Some((_, name)) = &mut ui.terminal.renaming {
                *name = value.clone();
            }
            vec![]
        }
        UiAction::Submitted { id, value } if id == view::TERMINAL_RENAME => {
            if let Some((tab, _)) = ui.terminal.renaming.take() {
                ui.terminal
                    .requests
                    .push(TerminalRequest::Rename(tab, Some(value.clone())));
            }
            vec![]
        }
        UiAction::InputChanged { id, value } if id.starts_with("ask-other:") => {
            let rest = &id["ask-other:".len()..];
            if let Some((permission, q)) = rest.rsplit_once(':') {
                if let Ok(q) = q.parse() {
                    ui.other.insert((permission.to_owned(), q), value.clone());
                }
            }
            vec![]
        }
        UiAction::InputChanged { id, value } if id.starts_with(account::CODE) => {
            ui.account.code = value.clone();
            vec![]
        }
        UiAction::Submitted { id, .. } if id.starts_with(account::CODE) => account::submit(ui),
        UiAction::InputChanged { id, value } if id.starts_with("revise-text:") => {
            ui.revision = value.clone();
            vec![]
        }
        // "Revise…" sends what should change as the reason the plan is turned down.
        UiAction::Submitted { id, value } if id.starts_with("revise-text:") => {
            let permission = &id["revise-text:".len()..];
            ui.revising = None;
            ui.revision.clear();
            let Some(session) = ui.selected else {
                return vec![];
            };
            let message = if value.trim().is_empty() {
                "The person wants the plan revised.".to_owned()
            } else {
                value.trim().to_owned()
            };
            vec![Intent::Decide {
                session,
                permission: permission.to_owned(),
                decision: Decision::Deny { message },
            }]
        }
        // The address bar: Enter opens what was typed in the tab on screen.
        UiAction::Submitted { id, value } if id == view::BROWSER_URL => {
            let Some(session) = ui.selected else {
                return vec![];
            };
            let tab = store.session(session).and_then(|s| s.browser.active);
            let op = match tab {
                Some(tab) => BrowserOp::Navigate {
                    tab,
                    url: value.clone(),
                },
                None => BrowserOp::Open {
                    url: Some(value.clone()),
                },
            };
            vec![Intent::Browser { session, op }]
        }
        UiAction::InputChanged { id, value } if id == view::TITLE_FIELD => {
            ui.title_edit = Some(value.clone());
            vec![]
        }
        UiAction::Submitted { id, value } if id == view::TITLE_FIELD => {
            ui.title_edit = None;
            let title = value.trim();
            match ui.selected.and_then(|id| store.session(id)) {
                Some(s) if !title.is_empty() && title != s.title => vec![Intent::Rename {
                    session: s.id,
                    title: title.to_owned(),
                }],
                _ => vec![],
            }
        }
        UiAction::InputChanged { id, value } if id == view::SEARCH_FIELD => {
            ui.search = Some(value.clone());
            vec![]
        }
        // Enter opens what the search lists first, as its "↵" marks.
        UiAction::Submitted { id, .. } if id == view::SEARCH_FIELD => {
            let first = ui
                .search
                .as_deref()
                .and_then(|query| view::found(store, query).into_iter().next());
            match first {
                Some(session) => clicked(store, ui, &format!("search:{session}")),
                None => clicked(store, ui, view::NEW_SESSION),
            }
        }
        UiAction::InputChanged { id, value } if id == view::COMPOSER => {
            let before = mention_query(&ui.prompt).map(|(_, query)| query.to_owned());
            ui.prompt = value.clone();
            // The highlight starts at the top with each keystroke; a `/` that went away can open again.
            ui.slash_selected = 0;
            ui.mention_selected = 0;
            if slash_query(&ui.prompt).is_none() {
                ui.slash_dismissed = None;
            }
            if mention_query(&ui.prompt).is_none() {
                ui.mention_dismissed = None;
            }
            mention::asked(ui, before.as_deref())
        }
        UiAction::InputChanged { id, value } if id == view::SETTINGS_SEARCH => {
            ui.settings_query = value.clone();
            vec![]
        }
        // Connectors' search asks the MCP registry for each word typed.
        UiAction::InputChanged { id, value } if id == view::SRV_SEARCH => {
            ui.connectors.query = value.trim().to_owned();
            vec![Intent::FindServers {
                query: ui.connectors.query.clone(),
            }]
        }
        UiAction::InputChanged { id, value } if id.starts_with(view::SRV_FIELD) => {
            if let Some(adding) = ui.connectors.adding.as_mut() {
                let field = match &id[view::SRV_FIELD.len()..] {
                    "name" => &mut adding.name,
                    "target" => &mut adding.target,
                    "env" => &mut adding.env,
                    "secret-env" => &mut adding.secret_env,
                    "secret-headers" => &mut adding.secret_headers,
                    _ => &mut adding.headers,
                };
                *field = value.clone();
            }
            vec![]
        }
        UiAction::InputChanged { id, value } if id.starts_with(view::GSK_FIELD) => {
            match (ui.skill_dialog.as_mut(), &id[view::GSK_FIELD.len()..]) {
                (Some(SkillDialog::New { name, .. }), "name") => *name = value.clone(),
                (Some(SkillDialog::New { description, .. }), "description") => {
                    *description = value.clone()
                }
                (Some(SkillDialog::Import { path }), "path") => *path = value.clone(),
                _ => {}
            }
            vec![]
        }
        UiAction::InputChanged { id, value } if id == view::CZ_SEARCH => {
            ui.customize_query = value.clone();
            vec![]
        }
        UiAction::InputChanged { id, value } if id == view::CZ_DIR_SEARCH => {
            ui.directory_query = value.clone();
            vec![]
        }
        UiAction::InputChanged { id, value } if id.starts_with(view::EXT_MKT_FIELD) => {
            ui.marketplace_field = value.clone();
            vec![]
        }
        // Enter in the marketplace's field adds it, as its button does.
        UiAction::Submitted { id, value } if id.starts_with(view::EXT_MKT_FIELD) => {
            ui.marketplace_field = value.clone();
            clicked(store, ui, "ext:mkt-add")
        }
        UiAction::InputChanged { id, value }
            if id.starts_with("set:agent:") && id.ends_with(":env") =>
        {
            let Some(agent) = id
                .strip_prefix("set:agent:")
                .and_then(|r| r.strip_suffix(":env"))
            else {
                return vec![];
            };
            let mut settings = ui.settings.clone();
            settings.agents.entry(agent.to_owned()).or_default().env =
                value.lines().map(str::to_owned).collect();
            vec![Intent::SaveSettings(settings)]
        }
        // Enter saves the code font, which blocks on fontconfig when the faces are built.
        UiAction::Submitted { id, value } if id == "set:code_font" => {
            let mut settings = ui.settings.clone();
            settings.code_font = value.trim().to_owned();
            vec![Intent::SaveSettings(settings)]
        }
        UiAction::Submitted { id, value } if id == "set:branch_prefix" => {
            let mut settings = ui.settings.clone();
            settings.branch_prefix = value.trim().trim_end_matches('/').to_owned();
            vec![Intent::SaveSettings(settings)]
        }
        UiAction::InputChanged { id, value } if id.starts_with("set:agent:") => {
            let Some(agent) = id
                .strip_prefix("set:agent:")
                .and_then(|r| r.strip_suffix(":program"))
            else {
                return vec![];
            };
            let mut settings = ui.settings.clone();
            let program = value.trim();
            settings.agents.entry(agent.to_owned()).or_default().program =
                (!program.is_empty()).then(|| program.to_owned());
            vec![Intent::SaveSettings(settings)]
        }
        UiAction::InputChanged { id, value } if id == view::SIDE_FIELD => {
            ui.side_reply = value.clone();
            vec![]
        }
        UiAction::Submitted { id, value } if id == view::SIDE_FIELD => {
            let text = value.trim().to_owned();
            ui.side_reply.clear();
            match ui.side_chat {
                Some(session) if !text.is_empty() => vec![Intent::Send {
                    session,
                    text,
                    mentions: vec![],
                    images: vec![],
                    lane: Lane::default(),
                }],
                _ => vec![],
            }
        }
        UiAction::Submitted { id, value } if id == view::COMPOSER => {
            let lane = if std::mem::take(&mut ui.queue_submit) {
                Lane::Queue
            } else {
                Lane::Steer
            };
            let text = value.trim().to_owned();
            if text.is_empty() {
                return vec![];
            }
            ui.prompt.clear();
            ui.image_notice = None;
            ui.slash_dismissed = None;
            let mentions = mention::mentions_in(&text, &ui.mentioned_sessions);
            ui.mentioned_sessions.clear();
            // "/mcp" shows the session's MCP servers, as Desktop's does.
            if text == "/mcp" {
                if let Some(session) = ui.selected {
                    ui.mcp = Some(McpDialog::new(session));
                    return vec![Intent::McpStatus { session }];
                }
            }
            // "/btw …" asks a side question beside the session.
            if let (Some(question), Some(session)) = (
                text.strip_prefix("/btw ")
                    .map(str::trim)
                    .filter(|q| !q.is_empty()),
                ui.selected,
            ) {
                ui.side_waiting = true;
                return vec![Intent::SideChat {
                    session,
                    text: question.to_owned(),
                }];
            }
            match ui.selected {
                Some(session) => vec![Intent::Send {
                    session,
                    text,
                    mentions,
                    images: std::mem::take(&mut ui.images),
                    lane,
                }],
                None => {
                    ui.waiting = Some((text, mentions, std::mem::take(&mut ui.images)));
                    vec![Intent::NewSession(NewSession {
                        agent: ui.agent.clone(),
                        cwd: ui.cwd.clone(),
                        model: ui.model.clone(),
                        effort: ui.effort.clone(),
                        mode: Some(ui.mode.clone()),
                    })]
                }
            }
        }
        UiAction::SliderChanged {
            id,
            fraction,
            committed,
        } if id == view::EFFORT_SLIDER => effort_slid(store, ui, *fraction, *committed),
        UiAction::Clicked { id } => clicked(store, ui, id),
        UiAction::DragStarted { id, position } | UiAction::DragMoved { id, position, .. } => {
            dragged(ui, id, *position);
            vec![]
        }
        _ => vec![],
    }
}

/// The effort slider moved: while dragged the thumb follows the pointer, pulled
/// towards the nearest stop by limn's magnetic steps; let go, it springs on to the stop it
/// was let go towards at the pointer's speed, and that level is chosen, as the person's
/// reference selector does (github.com/zanwei/claude-model-selector: stiffness 920, damping
/// 40, the speed of the last 90 ms, at most 8 stops a second).
fn effort_slid(store: &Store, ui: &mut Ui, fraction: f32, committed: bool) -> Vec<Intent> {
    use std::time::{Duration, Instant};
    let now = Instant::now();
    let x = fraction * view::EFFORT_TRAVEL;
    let efforts = view::efforts(store, ui);
    let last = efforts.len().saturating_sub(1).max(1) as f32;
    let thumb = ui.effort_thumb.get_or_insert_with(|| {
        limn::ScalarSpring::new(x, now).motion(limn::SpringMotion {
            damping_ratio: 0.66,
            stiffness: 920.0,
        })
    });
    if !committed {
        ui.effort_dragging = true;
        thumb.snap_to(x, now);
        ui.effort_samples.push((now, x));
        ui.effort_samples
            .retain(|(at, _)| now.duration_since(*at) < Duration::from_millis(90));
        return vec![];
    }
    let velocity = match (ui.effort_samples.first(), ui.effort_samples.last()) {
        (Some(first), Some(end)) if ui.effort_dragging => {
            let seconds = end.0.duration_since(first.0).as_secs_f32().max(0.016);
            let most = 8.0 * view::EFFORT_TRAVEL / last;
            ((end.1 - first.1) / seconds).clamp(-most, most)
        }
        _ => 0.0,
    };
    thumb.release(thumb.value(now), velocity, x, now);
    ui.effort_dragging = false;
    ui.effort_samples.clear();
    let Some(effort) = efforts.get((fraction * last).round() as usize) else {
        return vec![];
    };
    ui.effort = Some(effort.clone());
    ui.selected
        .map(|session| {
            vec![Intent::SetEffort {
                session,
                effort: effort.clone(),
            }]
        })
        .unwrap_or_default()
}

/// The browser's own controls: its toggle, tabs and toolbar. `None` when `id` is none of
/// them.
fn browser_clicked(store: &Store, ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    let session = ui.selected?;
    let browser = &store.session(session)?.browser;
    let tab = browser.active;
    if id == view::BROWSER_NOTICE_CLOSE {
        ui.browser_notice.remove(&session);
        return Some(vec![]);
    }
    let op = if id == view::BROWSER_TOGGLE {
        if ui.pane_open(Pane::Browser) {
            ui.close_pane(Pane::Browser);
            return Some(vec![]);
        }
        ui.open_pane(Pane::Browser);
        BrowserOp::Open { url: None }
    } else if let Some(tab) = id
        .strip_prefix("browser:card:")
        .and_then(|t| t.parse().ok())
    {
        // "Open" on a card: the pane, on that tab.
        ui.open_pane(Pane::Browser);
        BrowserOp::SelectTab { tab }
    } else if id == view::BROWSER_NEW_TAB {
        BrowserOp::NewTab { url: None }
    } else if let Some(tab) = id.strip_prefix("browser:tab:").and_then(|t| t.parse().ok()) {
        BrowserOp::SelectTab { tab }
    } else if let Some(tab) = id
        .strip_prefix("browser:close:")
        .and_then(|t| t.parse().ok())
    {
        BrowserOp::CloseTab { tab }
    } else if id == view::BROWSER_VIEWPORT {
        ui.open_menu =
            (ui.open_menu != Some(view::BROWSER_VIEWPORT)).then_some(view::BROWSER_VIEWPORT);
        return Some(vec![]);
    } else if let Some(choice) = id.strip_prefix("browser:viewport:") {
        ui.open_menu = None;
        if choice == "dismiss" {
            return Some(vec![]);
        }
        let size = choice
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
        BrowserOp::Viewport { tab: tab?, size }
    } else if id == view::BROWSER_SELECT {
        BrowserOp::Select {
            tab: tab?,
            on: !browser.selecting,
        }
    } else if id == view::BROWSER_BACK {
        BrowserOp::Back { tab: tab? }
    } else if id == view::BROWSER_FORWARD {
        BrowserOp::Forward { tab: tab? }
    } else if id == view::BROWSER_RELOAD {
        BrowserOp::Reload { tab: tab? }
    } else if id == view::BROWSER_STOP {
        BrowserOp::Stop { tab: tab? }
    } else {
        return None;
    };
    Some(vec![Intent::Browser { session, op }])
}

/// A boundary followed the pointer to `position`: the column's left edge, or the top of the
/// lower stacked pane.
fn dragged(ui: &mut Ui, id: &str, position: limn::Point) {
    let rest = ui.window.width - view::sidebar_width(ui);
    if id == view::COLUMN_HANDLE && rest > 0.0 {
        let width = view::column_width(rest, (ui.window.width - position.x) / rest);
        ui.column_fraction = Some(width / rest);
        // Followed at once, not on the spring that widens the column for a pane.
        ui.column_share
            .snap_to(width / rest, std::time::Instant::now());
    } else if id == view::PANEL_HANDLE {
        // Followed at once, not on the spring that slides the panel in and out.
        ui.panel_width = view::panel_width(rest, ui.window.width - position.x);
        ui.panel_shown
            .snap_to(ui.panel_width, std::time::Instant::now());
    } else if id == view::STACK_HANDLE {
        let room = ui.window.height - look::TOP_BAR - 1.0 - 2.0;
        if room > 0.0 {
            let top = (position.y - look::TOP_BAR - 1.0)
                .clamp(view::MIN_PANE, (room - view::MIN_PANE).max(view::MIN_PANE));
            ui.stack_fraction = Some(top / room);
        }
    }
}

fn terminal_clicked(pane: &mut TerminalPane, id: &str) {
    let index = |prefix: &str| id.strip_prefix(prefix)?.parse::<usize>().ok();
    if id == view::TERMINAL_NEW {
        pane.requests.push(TerminalRequest::New);
    } else if id == view::TERMINAL_CLOSE_CONFIRM {
        pane.requests
            .extend(pane.closing.take().map(|closing| match closing {
                Closing::One(tab) => TerminalRequest::Close(tab),
                Closing::Others(tab) => TerminalRequest::CloseOthers(tab),
            }));
    } else if id == view::TERMINAL_CLOSE_CANCEL {
        pane.closing = None;
    } else if let Some(tab) = index("terminal:menu:rename:") {
        // Desktop's field starts empty.
        pane.renaming = Some((tab, String::new()));
    } else if let Some(tab) = index("terminal:menu:clear:") {
        pane.requests.push(TerminalRequest::Clear(tab));
    } else if let Some(tab) = index("terminal:menu:others:") {
        let busy = pane
            .tabs
            .iter()
            .enumerate()
            .any(|(i, t)| i != tab && t.busy);
        if busy {
            pane.closing = Some(Closing::Others(tab));
        } else {
            pane.requests.push(TerminalRequest::CloseOthers(tab));
        }
    } else if let Some(tab) = index("terminal:tab:") {
        pane.requests.push(TerminalRequest::Select(tab));
    } else if let Some(tab) = index("terminal:close:").or_else(|| index("terminal:menu:close:")) {
        // A running command is stopped only once the person confirms.
        if pane.tabs.get(tab).is_some_and(|t| t.busy) {
            pane.closing = Some(Closing::One(tab));
        } else {
            pane.requests.push(TerminalRequest::Close(tab));
        }
    }
}

/// Keys for the terminal's tabs, which the host passes before the window takes them: on a
/// focused pill Delete or Backspace closes it and Ctrl+Shift+arrows move it; in the name
/// field Escape gives up. Whether the key was taken.
pub fn terminal_key(
    ui: &mut Ui,
    focused: Option<&str>,
    key: limn::Key,
    ctrl: bool,
    shift: bool,
) -> bool {
    use limn::Key;
    // The title's field gives up the rename too.
    if focused == Some(view::TITLE_FIELD) && key == Key::Escape {
        ui.title_edit = None;
        return true;
    }
    let pane = &mut ui.terminal;
    if focused == Some(view::TERMINAL_RENAME) && key == Key::Escape {
        pane.renaming = None;
        return true;
    }
    let Some(tab) = focused
        .and_then(|f| f.strip_prefix("terminal:tab:"))
        .and_then(|i| i.parse::<usize>().ok())
    else {
        return false;
    };
    match key {
        Key::Delete | Key::Backspace => terminal_clicked(pane, &format!("terminal:close:{tab}")),
        Key::ArrowLeft | Key::ArrowRight if ctrl && shift => {
            let to = if key == Key::ArrowLeft {
                tab.checked_sub(1)
            } else {
                Some(tab + 1).filter(|to| *to < pane.tabs.len())
            };
            if let Some(to) = to {
                pane.requests.push(TerminalRequest::Move { from: tab, to });
            }
        }
        _ => return false,
    }
    true
}

fn clicked(store: &Store, ui: &mut Ui, id: &str) -> Vec<Intent> {
    if id == welcome::START {
        ui.settings.welcomed = true;
        return vec![Intent::SaveSettings(ui.settings.clone())];
    }
    if let Some(index) = id
        .strip_prefix("image:remove:")
        .and_then(|i| i.parse::<usize>().ok())
    {
        if index < ui.images.len() {
            ui.images.remove(index);
            ui.image_notice = None;
        }
        return vec![];
    }
    // The viewer's close, Escape and backdrop all report the close control.
    if id == view::IMAGE_CLOSE {
        ui.viewer = None;
        return vec![];
    }
    if id == view::IMAGE_ANNOTATE {
        // Desktop closes the viewer as the editor opens.
        ui.annotating = ui.viewer.take().map(|viewer| viewer.path);
        return vec![];
    }
    if let Some(index) = id
        .strip_prefix("image:open:")
        .and_then(|i| i.parse::<usize>().ok())
    {
        if let Some(path) = ui.images.get(index).cloned() {
            ui.viewer = Some(Viewer {
                path,
                origin: None,
                annotatable: true,
            });
        }
        return vec![];
    }
    if let Some(call) = id.strip_prefix("image:tool:") {
        let shown = ui.selected.and_then(|id| store.session(id)).and_then(|s| {
            s.items.iter().find_map(|item| match item {
                Item::Tool {
                    call: c,
                    name,
                    input,
                    result,
                } if c == call => view::step_image(name, input, result.as_ref()),
                _ => None,
            })
        });
        ui.viewer = shown.map(|path| Viewer {
            path,
            origin: None,
            annotatable: false,
        });
        return vec![];
    }
    if let Some(rest) = id.strip_prefix("image:msg:") {
        let shown = rest.rsplit_once(':').and_then(|(request, index)| {
            let index: usize = index.parse().ok()?;
            let session = store.session(ui.selected?)?;
            session.items.iter().find_map(|item| match item {
                Item::User {
                    request: r, images, ..
                } if r.as_str() == request => images.get(index).cloned(),
                _ => None,
            })
        });
        ui.viewer = shown.map(|path| Viewer {
            path,
            origin: None,
            annotatable: false,
        });
        return vec![];
    }
    if id == view::NEW_SESSION {
        ui.selected = None;
        ui.search = None;
        ui.title_edit = None;
        ui.take_defaults();
        return vec![];
    }
    if let Some(intents) = updates::clicked(store, ui, id) {
        return intents;
    }
    if let Some(intents) = account::clicked(ui, id) {
        return intents;
    }
    if let Some(intents) = appearance::clicked(ui, id) {
        return intents;
    }
    if let Some(intents) = github::clicked(ui, id) {
        return intents;
    }
    if let Some(intents) = remote::clicked(store, ui, id) {
        return intents;
    }
    if let Some(intents) = terminals::clicked(store, ui, id) {
        return intents;
    }
    if let Some(intents) = tour::clicked(ui, id) {
        return intents;
    }
    if let Some(intents) = settings_clicked(ui, id) {
        return intents;
    }
    if let Some(intents) = mcp_clicked(ui, id) {
        return intents;
    }
    if let Some(intents) = session_clicked(store, ui, id) {
        return intents;
    }
    if let Some(intents) = message_clicked(store, ui, id) {
        return intents;
    }
    if let Some(rest) = id.strip_prefix("ask:") {
        return answer(store, ui, rest);
    }
    if id == view::TERMINAL_TOGGLE {
        if ui.pane_open(Pane::Terminal) {
            ui.close_pane(Pane::Terminal);
        } else {
            ui.open_pane(Pane::Terminal);
        }
        return vec![];
    }
    if id.starts_with("terminal:") {
        terminal_clicked(&mut ui.terminal, id);
        return vec![];
    }
    if let Some(folder) = id.strip_prefix("group:") {
        let folder = PathBuf::from(folder);
        if let Some(at) = ui.collapsed.iter().position(|f| *f == folder) {
            ui.collapsed.remove(at);
        } else {
            ui.collapsed.push(folder);
        }
        return vec![];
    }
    // Desktop's edge collapses the panel on a click ("Click to collapse").
    if id == view::PANEL_TOGGLE || id == view::PANEL_HANDLE {
        ui.panel_open = !ui.panel_open;
        return vec![];
    }
    if id == view::DIFF_TOGGLE {
        // Desktop's diff is what is uncommitted, "master → working tree".
        if ui.pane_open(Pane::Diff) {
            ui.close_pane(Pane::Diff);
        } else if let Some(session) = ui.selected {
            ui.diff = None;
            ui.open_pane(Pane::Diff);
            return vec![Intent::WorkingTree { session }];
        }
        return vec![];
    }
    // Closing a side chat puts it away once it is between turns, as Desktop archives a
    // settled one.
    if id == "pane:side:close" {
        let settled = ui.side_chat.and_then(|s| store.session(s)).filter(|s| {
            s.aside
                && !matches!(
                    s.status,
                    SessionStatus::Running | SessionStatus::Starting | SessionStatus::Awaiting
                )
        });
        ui.close_pane(Pane::Side);
        return settled
            .map(|s| {
                vec![Intent::Archive {
                    session: s.id,
                    archived: true,
                }]
            })
            .unwrap_or_default();
    }
    if let Some(intents) = browser_clicked(store, ui, id) {
        return intents;
    }
    if let Some(rest) = id.strip_prefix("pane:") {
        for pane in Pane::ALL {
            if rest == format!("{}:close", pane.key()) {
                ui.close_pane(pane);
            } else if rest == format!("{}:expand", pane.key()) {
                ui.expanded_pane = (ui.expanded_pane != Some(pane)).then_some(pane);
            }
        }
        return vec![];
    }
    if let Some(rest) = id.strip_prefix("changes:") {
        let Some(session) = ui.selected else {
            return vec![];
        };
        if rest.ends_with(":more") {
            match ui.expanded.iter().position(|k| k == id) {
                Some(at) => drop(ui.expanded.remove(at)),
                None => ui.expanded.push(id.to_owned()),
            }
        } else if let Some((request, _file)) = rest.split_once(":view") {
            ui.diff = Some((session, request.to_owned()));
            ui.open_pane(Pane::Diff);
        } else if let Some(request) = rest.strip_suffix(":undo") {
            return vec![Intent::Rewind {
                session,
                request: request.to_owned(),
            }];
        }
        return vec![];
    }
    if id == view::REWIND_UNDO {
        let Some(s) = ui.selected.and_then(|id| store.session(id)) else {
            return vec![];
        };
        if s.working() || s.rewound.is_empty() {
            return vec![];
        }
        // The words the rewind put back in the box go with it, unless the person wrote on.
        if let Some(Item::User { text, .. }) = s.rewound.first() {
            if ui.prompt == *text {
                ui.prompt.clear();
            }
        }
        return vec![Intent::UndoRewind { session: s.id }];
    }
    if id == view::STOP {
        return ui
            .selected
            .map(|session| vec![Intent::Interrupt { session }])
            .unwrap_or_default();
    }
    if id == view::SEND {
        let value = ui.prompt.clone();
        return update(
            store,
            ui,
            &limn::UiAction::Submitted {
                id: view::COMPOSER.into(),
                value,
            },
        );
    }
    // The "+" actions are the host's (pickers, the editor's text); the menu closes.
    if [
        view::ATTACH_FILES,
        view::ATTACH_FOLDER,
        view::SLASH,
        view::PICK_FOLDER,
        view::USAGE_ALL,
    ]
    .contains(&id)
    {
        ui.open_menu = None;
        return vec![];
    }
    if let Some(folder) = id.strip_prefix("new-in:") {
        ui.selected = None;
        ui.cwd = PathBuf::from(folder);
        return vec![];
    }
    if let Some(agent) = id.strip_prefix("pick-agent:") {
        ui.open_menu = None;
        ui.take_agent(agent);
        // Persist before the next frame shows it (AGENTS.md): the choice survives a restart.
        ui.settings.default_agent = Some(agent.to_owned());
        return vec![Intent::SaveSettings(ui.settings.clone())];
    }
    if let Some(folder) = id.strip_prefix("folder:") {
        ui.open_menu = None;
        ui.cwd = PathBuf::from(folder);
        return vec![];
    }
    if let Some(effort) = id.strip_prefix("effort:") {
        ui.open_menu = None;
        ui.effort = Some(effort.to_owned());
        return ui
            .selected
            .map(|session| {
                vec![Intent::SetEffort {
                    session,
                    effort: effort.to_owned(),
                }]
            })
            .unwrap_or_default();
    }
    if id == format!("{}:effort", view::MODEL_MENU) {
        return vec![];
    }
    if let Some(intents) = plus_clicked(ui, id) {
        return intents;
    }
    if let Some(intents) = slash::clicked(store, ui, id) {
        return intents;
    }
    if mention::clicked(store, ui, id) {
        return vec![];
    }
    for menu in [
        view::MODE_MENU,
        view::MODEL_MENU,
        view::PLUS_MENU,
        view::FOLDER_MENU,
        view::AGENT_MENU,
        view::USAGE_MENU,
        view::EFFORT_MENU,
    ] {
        if id == menu {
            ui.open_menu = if ui.open_menu == Some(menu) {
                None
            } else {
                Some(menu)
            };
            return vec![];
        }
        if id == format!("{menu}:dismiss") {
            ui.open_menu = None;
            return vec![];
        }
    }
    if let Some(mode) = id.strip_prefix("mode:") {
        ui.open_menu = None;
        ui.mode = mode.to_owned();
        return ui
            .selected
            .map(|session| {
                vec![Intent::SetMode {
                    session,
                    mode: mode.to_owned(),
                }]
            })
            .unwrap_or_default();
    }
    if let Some(model) = id.strip_prefix("model:") {
        ui.open_menu = None;
        ui.model = Some(model.to_owned());
        return ui
            .selected
            .map(|session| {
                vec![Intent::SetModel {
                    session,
                    model: model.to_owned(),
                }]
            })
            .unwrap_or_default();
    }
    if let Some(session) = id
        .strip_prefix("session:")
        .or_else(|| id.strip_prefix("search:"))
        .and_then(|s| s.parse().ok())
    {
        ui.search = None;
        if ui.selected != Some(session) {
            ui.title_edit = None;
            ui.mark_new(store, session);
        }
        ui.selected = Some(session);
        // The composer shows the model and effort this session runs at, as Desktop's does;
        // a session from before they were kept leaves them as they are.
        if let Some(s) = store.session(session) {
            if s.model.is_some() {
                ui.model = s.model.clone();
            }
            if s.effort.is_some() {
                ui.effort = s.effort.clone();
            }
        }
        // Going somewhere from the peek puts it away, as Desktop's does ("navigated").
        ui.sidebar.peek = false;
        // A session beside another goes with it: shown on its own, or from elsewhere, it closes.
        let beside = ui.side_chat.and_then(|s| store.session(s));
        if beside.is_none_or(|s| s.id == session || s.parent != Some(session)) {
            ui.close_pane(Pane::Side);
        }
        let mut intents = vec![Intent::WorkingTree { session }];
        if ui.context_open() {
            intents.push(Intent::ContextUsage { session });
        }
        if store.session(session).is_some_and(|s| s.unread) {
            intents.push(Intent::MarkUnread {
                session,
                unread: false,
            });
        }
        return intents;
    }
    if id.starts_with(view::DIFF_FOLD) || id.starts_with("web:") {
        match ui.expanded.iter().position(|k| k == id) {
            Some(at) => drop(ui.expanded.remove(at)),
            None => ui.expanded.push(id.to_owned()),
        }
        return vec![];
    }
    if let Some(rest) = id.strip_prefix(view::OPEN_FILE_MENU) {
        match rest.strip_suffix(":dismiss") {
            Some(path) if ui.diff_menu.as_deref() == Some(path) => ui.diff_menu = None,
            Some(_) => {}
            None => ui.diff_menu = (ui.diff_menu.as_deref() != Some(rest)).then(|| rest.to_owned()),
        }
        return vec![];
    }
    // The host opens the file there; an editor chosen is the one Open in offers first next.
    if let Some((place, _)) = id
        .strip_prefix(view::OPEN_FILE_IN)
        .and_then(|rest| rest.split_once(':'))
    {
        ui.diff_menu = None;
        if !matches!(place, "default" | "files") && place != ui.settings.editor {
            return vec![Intent::SaveSettings(quill_model::Settings {
                editor: place.to_owned(),
                ..ui.settings.clone()
            })];
        }
        return vec![];
    }
    // A link in the conversation: a page opens in the session's browser, a file of its folder
    // in Files; the host opens what else it names (mail, another app's) on the computer.
    if let Some(url) = view::link_url(id).or_else(|| id.strip_prefix(view::LINK_BROWSER)) {
        ui.link_menu = None;
        let Some(session) = ui.selected.and_then(|s| store.session(s)) else {
            return vec![];
        };
        let web = url.starts_with("http://") || url.starts_with("https://");
        // Settings' "Open links in built-in browser" off, or a pull request with "Open pull
        // requests in browser" on: the host opens a clicked page on the computer; the menu's
        // "Open in Browser" still opens it here.
        if view::link_url(id).is_some() && opens_outside(&ui.settings, url) {
            return vec![];
        }
        if web {
            ui.open_pane(Pane::Browser);
            let op = if session.browser.tabs.is_empty() {
                BrowserOp::Open {
                    url: Some(url.to_owned()),
                }
            } else {
                BrowserOp::NewTab {
                    url: Some(url.to_owned()),
                }
            };
            return vec![Intent::Browser {
                session: session.id,
                op,
            }];
        }
        let path = url.strip_prefix("file://").unwrap_or(url);
        if !path.contains("://") && !path.starts_with("mailto:") {
            let id = format!("{}1:{path}", view::OPEN_HERE);
            return files::action(store, ui, &limn::UiAction::Clicked { id }).unwrap_or_default();
        }
        return vec![];
    }
    if id.starts_with(view::LINK_EXTERNAL)
        || id.starts_with(view::LINK_COPY)
        || id == format!("{}:dismiss", view::LINK_MENU)
    {
        // The host opens or copies it; the menu closes.
        ui.link_menu = None;
        return vec![];
    }
    if id == view::TASKS_OPEN {
        ui.open_pane(Pane::Tasks);
        return vec![];
    }
    if id == view::TASKS_TOGGLE {
        if ui.pane_open(Pane::Tasks) {
            ui.close_pane(Pane::Tasks);
        } else {
            ui.open_pane(Pane::Tasks);
        }
        return vec![];
    }
    if id == view::TASKS_CLEAR {
        let finished = (ui.selected.and_then(|s| store.session(s)))
            .map(|s| {
                s.tasks
                    .iter()
                    .filter(|t| !t.running)
                    .map(|t| t.id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return match ui.selected {
            Some(session) if !finished.is_empty() => {
                vec![Intent::ClearTasks {
                    session,
                    tasks: finished,
                }]
            }
            _ => vec![],
        };
    }
    if let Some(key) = ["tasks:running", "tasks:finished"]
        .into_iter()
        .find(|k| *k == id)
    {
        match ui.expanded.iter().position(|k| k == key) {
            Some(at) => drop(ui.expanded.remove(at)),
            None => ui.expanded.push(key.to_owned()),
        }
        return vec![];
    }
    if let Some(task) = id.strip_prefix("task:stop:") {
        return ui
            .selected
            .map(|session| {
                vec![Intent::StopTask {
                    session,
                    task: task.to_owned(),
                }]
            })
            .unwrap_or_default();
    }
    if let Some(call) = id.strip_prefix("agent:") {
        ui.helper = Some(call.to_owned());
        ui.open_pane(Pane::Agent);
        return vec![];
    }
    if let Some(permission) = id.strip_prefix("revise:") {
        ui.revising = Some(permission.to_owned());
        ui.revision.clear();
        return vec![];
    }
    // Accepting a plan allows it and puts the session in the mode chosen.
    if let Some((permission, mode)) = id.strip_prefix("planok:").and_then(|r| r.rsplit_once(':')) {
        ui.perm_menu = None;
        let Some(session) = asking(store, ui, permission).map(|s| s.id) else {
            return vec![];
        };
        ui.mode = mode.to_owned();
        return vec![
            Intent::Decide {
                session,
                permission: permission.to_owned(),
                decision: Decision::Allow,
            },
            Intent::SetMode {
                session,
                mode: mode.to_owned(),
            },
        ];
    }
    if let Some(plan) = id.strip_prefix("plan:") {
        ui.plan = Some(plan.to_owned());
        ui.open_pane(Pane::Plan);
        return vec![];
    }
    if id.starts_with("steps:") || id.starts_with("panel:") {
        if let Some(at) = ui.expanded.iter().position(|c| c == id) {
            ui.expanded.remove(at);
        } else {
            ui.expanded.push(id.to_owned());
        }
        return match ui.selected {
            Some(session) if id == view::CONTEXT_CARD && ui.context_open() => {
                vec![Intent::ContextUsage { session }]
            }
            _ => vec![],
        };
    }
    if let Some(call) = id.strip_prefix("tool:") {
        if let Some(at) = ui.expanded.iter().position(|c| c == call) {
            ui.expanded.remove(at);
        } else {
            ui.expanded.push(call.to_owned());
        }
        return vec![];
    }
    if let Some(rest) = id.strip_prefix("perm:") {
        if let Some(permission) = rest.strip_suffix(":menu") {
            ui.perm_menu =
                (ui.perm_menu.as_deref() != Some(permission)).then(|| permission.to_owned());
            return vec![];
        }
        if rest.ends_with(":menu-dismiss") {
            ui.perm_menu = None;
            return vec![];
        }
        ui.perm_menu = None;
        let Some((permission, choice)) = rest.rsplit_once(':') else {
            return vec![];
        };
        let Some(session) = asking(store, ui, permission) else {
            return vec![];
        };
        let Some(Item::Permission { suggestions, .. }) = session
            .items
            .iter()
            .find(|i| matches!(i, Item::Permission { id, .. } if id == permission))
        else {
            return vec![];
        };
        let decision = match choice {
            "deny" => Decision::Deny {
                message: "The person denied this.".into(),
            },
            // Everything the agent suggests, a mode switch included: Desktop's "Always allow" on
            // a file-creating command left the session in Accept edits.
            "always" => Decision::AllowAlways(suggestions.clone()),
            _ => Decision::Allow,
        };
        // Desktop offers the last choice first on the next card.
        match choice {
            "once" => ui.last_allow = Allow::Once,
            "always" => ui.last_allow = Allow::Always,
            _ => {}
        }
        return vec![Intent::Decide {
            session: session.id,
            permission: permission.to_owned(),
            decision,
        }];
    }
    vec![]
}

/// The session in view, or the one beside it, that asks `permission`: its card is answered
/// in either.
fn asking<'a>(store: &'a Store, ui: &Ui, permission: &str) -> Option<&'a Session> {
    [ui.selected, ui.side_chat]
        .into_iter()
        .flatten()
        .filter_map(|s| store.session(s))
        .find(|s| {
            s.items
                .iter()
                .any(|i| matches!(i, Item::Permission { id, .. } if id == permission))
        })
}

/// Marks a permission as decided once the person chose, so its card closes at once.
pub fn decided(store: &mut Store, intent: &Intent) {
    if let Intent::Decide {
        session,
        permission,
        decision,
    } = intent
    {
        if let Some(s) = store.session_mut(*session) {
            mark_decided(&mut s.items, permission, decision);
        }
    }
}

/// What a turn left open once nothing can answer it: its permissions close, and a tool that
/// never reported stops spinning.
fn close_open_permissions(items: &mut [Item]) {
    for item in items {
        match item {
            Item::Permission {
                decided: decided @ None,
                ..
            } => *decided = Some("Closed"),
            Item::Tool {
                result: result @ None,
                ..
            } => {
                *result = Some(ToolResult {
                    output: String::new(),
                    is_error: false,
                    added: 0,
                    removed: 0,
                    files: vec![],
                    stopped: true,
                    web: None,
                })
            }
            _ => {}
        }
    }
}

fn mark_decided(items: &mut [Item], permission: &str, decision: &Decision) {
    for item in items {
        if let Item::Permission { id, decided, .. } = item {
            if id == permission {
                *decided = Some(match decision {
                    Decision::Deny { .. } => "Denied",
                    Decision::AllowAlways(_) => "Always allowed",
                    _ => "Allowed",
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;

/// A question card's action: `{permission}:{question}:{option}`
/// picks an option (several when the question allows it, the last being "Other"),
/// `prev` and `next` turn the page, `skip` answers "[No preference]" and moves on, and `submit` (or `next` on the last page) sends every answer.
fn answer(store: &Store, ui: &mut Ui, rest: &str) -> Vec<Intent> {
    let Some(session) = rest
        .split_once(':')
        .and_then(|(permission, _)| asking(store, ui, permission))
    else {
        return vec![];
    };
    let find = |permission: &str| {
        session.items.iter().find_map(|i| match i {
            Item::Permission { id, input, .. } if id == permission => Some(input.clone()),
            _ => None,
        })
    };
    let Some((permission, action)) = rest.split_once(':') else {
        return vec![];
    };
    let Some(input) = find(permission) else {
        return vec![];
    };
    let questions = input["questions"].as_array().cloned().unwrap_or_default();
    let page = ui.question.get(permission).copied().unwrap_or(0);
    let last = page + 1 >= questions.len();
    let submit = |ui: &mut Ui| {
        let answers: serde_json::Map<String, Value> = questions
            .iter()
            .enumerate()
            .map(|(q, question)| {
                let other = ui.other.get(&(permission.to_owned(), q));
                let labels: Vec<String> = view::picked(ui, permission, q, question)
                    .into_iter()
                    .map(|l| match other {
                        Some(typed) if l == "Other" && !typed.trim().is_empty() => typed.clone(),
                        _ => l,
                    })
                    .collect();
                let said = if labels.is_empty() {
                    "[No preference]".to_owned()
                } else {
                    labels.join(", ")
                };
                let asked = question["question"].as_str().unwrap_or_default().to_owned();
                (asked, Value::String(said))
            })
            .collect();
        vec![Intent::Decide {
            session: session.id,
            permission: permission.to_owned(),
            decision: Decision::Answer(Value::Object(answers)),
        }]
    };
    match action {
        "submit" => return submit(ui),
        "next" if last => return submit(ui),
        "skip" => {
            ui.answers.insert((permission.to_owned(), page), vec![]);
            if last {
                return submit(ui);
            }
            ui.question.insert(permission.to_owned(), page + 1);
        }
        "next" => {
            ui.question.insert(permission.to_owned(), page + 1);
        }
        "prev" => {
            ui.question
                .insert(permission.to_owned(), page.saturating_sub(1));
        }
        _ => {
            let Some((q, o)) = action.split_once(':') else {
                return vec![];
            };
            let (Ok(q), Ok(o)) = (q.parse::<usize>(), o.parse::<usize>()) else {
                return vec![];
            };
            let question = &questions.get(q).cloned().unwrap_or_default();
            let options = question["options"].as_array().map_or(0, Vec::len);
            let label = if o == options {
                "Other".to_owned()
            } else {
                match question["options"][o]["label"].as_str() {
                    Some(label) => label.to_owned(),
                    None => return vec![],
                }
            };
            let mut labels = view::picked(ui, permission, q, question);
            if question["multiSelect"] == true {
                if let Some(at) = labels.iter().position(|l| *l == label) {
                    labels.remove(at);
                } else {
                    labels.push(label);
                }
            } else {
                labels = vec![label];
            }
            ui.answers.insert((permission.to_owned(), q), labels);
        }
    }
    vec![]
}

/// An agent's plugins, marketplaces and skills, for Settings' Customize pages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extensions {
    /// They were read at least once; until then the page says it looks.
    pub loaded: bool,
    /// The folder they were read for.
    pub folder: PathBuf,
    pub list: quill_model::Extensions,
    /// Why the last change or list failed.
    pub error: Option<String>,
    /// What is being changed, by its id or name, until the engine reads them again.
    pub working: Option<String>,
    /// What the last change did, in Desktop's words ("… is installed and ready to use.").
    pub notice: Option<String>,
    /// What the last change was, to say what it did once the lists come back.
    pub done: Option<String>,
}

/// Customize › Connectors: Yours or Discover, each agent's MCP servers as last listed, what
/// the MCP registry found for the words searched, and the server being added.
#[derive(Default)]
pub struct Connectors {
    pub discover: bool,
    pub listed: std::collections::HashMap<String, ServersListed>,
    pub query: String,
    pub found: Option<(String, Vec<quill_model::ServerOffer>, Option<String>)>,
    pub adding: Option<AddServer>,
}

/// An agent's MCP servers: `None` until listed, where one can be added, why the last list or
/// change failed, and whether a change is under way.
#[derive(Default)]
pub struct ServersListed {
    pub servers: Option<Vec<quill_model::ConfiguredServer>>,
    pub scopes: Vec<(String, String)>,
    pub error: Option<String>,
    pub working: bool,
}

/// "Add custom connector" as typed: for which agents, its name, its address or command line,
/// its `KEY=value` and `Name: value` lines, and where it goes in each agent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddServer {
    /// The agents' own programs it is added with, one or several.
    pub agents: Vec<String>,
    pub name: String,
    pub target: String,
    pub env: String,
    pub headers: String,
    /// Where it goes, by agent; an agent's first scope without one.
    pub scopes: std::collections::BTreeMap<String, String>,
    /// Added to quill's "All agents" layer instead (`agents` is then not used).
    pub all: bool,
    /// For "All agents": an address that speaks the older SSE transport.
    pub sse: bool,
    /// For "All agents": `KEY=value` and `Name: value` lines kept in the keychain.
    pub secret_env: String,
    pub secret_headers: String,
}

/// "New skill" or "Import skill" for "All agents": what is typed.
#[derive(Clone, Debug, PartialEq)]
pub enum SkillDialog {
    New { name: String, description: String },
    Import { path: String },
}

impl AddServer {
    /// What an agent is asked to add: an address is reached over HTTP, anything else is a
    /// command and its arguments, split at spaces.
    pub fn server(&self) -> Option<quill_model::NewServer> {
        let name = self.name.trim();
        let mut words = self.target.split_whitespace();
        let first = words.next()?;
        let remote = first.starts_with("https://") || first.starts_with("http://");
        let lines = |text: &str, mark: char| -> Vec<String> {
            (text.lines().map(str::trim))
                .filter(|l| l.contains(mark))
                .map(str::to_owned)
                .collect()
        };
        (!name.is_empty() && (self.all || !self.agents.is_empty())).then(|| {
            quill_model::NewServer {
                name: name.to_owned(),
                target: first.to_owned(),
                args: if remote {
                    vec![]
                } else {
                    words.map(str::to_owned).collect()
                },
                remote,
                scope: None,
                env: lines(&self.env, '='),
                headers: if remote {
                    lines(&self.headers, ':')
                } else {
                    vec![]
                },
            }
        })
    }

    /// The server for quill's "All agents" layer: the plain lines as they are, the secret ones
    /// marked so the engine keeps them in the keychain.
    pub fn global(&self) -> Option<quill_model::GlobalServer> {
        let server = self.server().filter(|_| self.all)?;
        let vars = |plain: &[String], secret: &str, mark: char| -> Vec<quill_model::GlobalVar> {
            let pair = |line: &str, secret| {
                let (key, value) = line.split_once(mark)?;
                Some(quill_model::GlobalVar {
                    key: key.trim().to_owned(),
                    value: Some(value.trim().to_owned()),
                    secret,
                })
            };
            let secrets = secret.lines().filter_map(|l| pair(l, true));
            plain
                .iter()
                .filter_map(|l| pair(l, false))
                .chain(secrets)
                .collect()
        };
        let secret_headers = if server.remote {
            &self.secret_headers
        } else {
            ""
        };
        Some(quill_model::GlobalServer {
            sse: self.sse && server.remote,
            env: vars(&server.env, &self.secret_env, '='),
            headers: vars(&server.headers, secret_headers, ':'),
            name: server.name,
            remote: server.remote,
            target: server.target,
            args: server.args,
            except: vec![],
        })
    }

    /// [`server`](Self::server) for `agent`, with the scope chosen for it.
    pub fn server_for(&self, agent: &str) -> Option<quill_model::NewServer> {
        let mut server = self.server()?;
        server.scope = self.scopes.get(agent).cloned();
        Some(server)
    }
}

/// What Customize asks before it removes: a plugin, or a marketplace with its plugins.
#[derive(Clone, Debug, PartialEq)]
pub enum Removing {
    Plugin(String, quill_model::Plugin),
    Marketplace(String, String),
}

/// The folder Customize reads plugins and skills for: the one a new session starts in, or
/// the person's own when none is chosen.
pub fn customize_folder(ui: &Ui) -> PathBuf {
    if ui.cwd.as_os_str().is_empty() {
        quill_files::home_dir().unwrap_or_default()
    } else {
        ui.cwd.clone()
    }
}

/// Whether `folder` is a project, not the person's own folder: what installing for a
/// project and a skill's state need.
pub fn is_project(folder: &std::path::Path) -> bool {
    quill_files::home_dir().is_none_or(|home| folder != home)
}

/// The marketplaces' dialogs: adding one, and the list of them to update or remove.
#[derive(Clone, Debug, PartialEq)]
pub enum MarketDialog {
    Add,
    Manage,
}

/// Customize's own clicks, around the changes: its menus (Add, an item's "⋯", Discover's
/// filter) open and close; Discover's directory and the marketplaces' dialogs open and close.
/// `None` when `id` is none of them.
fn customize_clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    let rest = id.strip_prefix("cz:")?;
    let toggle = |ui: &mut Ui, menu: &str| {
        let open = ui.settings_menu.as_deref() == Some(menu);
        ui.settings_menu = (!open).then(|| menu.to_owned());
    };
    if let Some(menu) = rest.strip_suffix(":dismiss") {
        if ui.settings_menu.as_deref() == Some(&format!("cz:{menu}")) {
            ui.settings_menu = None;
        }
        return Some(vec![]);
    }
    let (verb, arg) = rest.split_once(':').unwrap_or((rest, ""));
    match verb {
        "add" | "more" | "filter-menu" => toggle(ui, id),
        "discover" => {
            ui.settings_menu = None;
            ui.directory = Some(arg.to_owned());
            ui.directory_query.clear();
            ui.directory_market = None;
        }
        // Another agent's directory, in the same dialog.
        "dir" => {
            ui.directory = Some(arg.to_owned());
            ui.directory_market = None;
        }
        // An agent the marketplace is added to, ticked or not.
        "to" => {
            let at = ui.market_targets.iter().position(|a| a == arg);
            match at {
                Some(at) if ui.market_targets.len() > 1 => drop(ui.market_targets.remove(at)),
                Some(_) => {}
                None => ui.market_targets.push(arg.to_owned()),
            }
        }
        "filter" => {
            ui.settings_menu = None;
            ui.directory_market = (!arg.is_empty()).then(|| arg.to_owned());
        }
        // From the directory, for the agent whose it is; from the page, for the agent a new
        // session would run.
        "add-marketplace" => {
            ui.settings_menu = None;
            ui.marketplace_field.clear();
            let first = (!arg.is_empty()).then(|| arg.to_owned());
            ui.market_targets = first.into_iter().collect();
            if ui.market_targets.is_empty() {
                ui.market_targets = vec![ui.agent.clone()];
            }
            ui.marketplaces = Some(MarketDialog::Add);
        }
        "manage" => {
            ui.settings_menu = None;
            ui.marketplaces = Some(MarketDialog::Manage);
        }
        "close" => {
            ui.settings_menu = None;
            // The dialog over the directory goes first.
            if ui.marketplaces.take().is_none() {
                ui.directory = None;
            }
        }
        _ => return None,
    }
    Some(vec![])
}

/// Customize's clicks: a plugin's switch, install (for a scope from its menu),
/// update and removal (after asking); a marketplace's update, removal (after asking) and
/// adding one; a skill's state from its menu. Each sends the change with the folder, and
/// the engine reads the lists again. `None` when `id` is none of them.
fn extensions_clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    use quill_model::ExtensionOp;
    let folder = customize_folder(ui);
    if id == view::EXT_REMOVE_NO {
        ui.removing = None;
        return Some(vec![]);
    }
    let send = |ui: &mut Ui, agent: &str, working: String, done: Option<String>, op| {
        ui.settings_menu = None;
        if let Some(known) = ui.extensions.get_mut(agent) {
            known.working = Some(working);
            known.error = None;
            known.notice = None;
            known.done = done;
        }
        Some(vec![Intent::ExtensionOp {
            agent: agent.to_owned(),
            folder: folder.clone(),
            op,
        }])
    };
    if id == view::EXT_REMOVE_YES {
        return match ui.removing.take()? {
            Removing::Plugin(agent, plugin) => send(
                ui,
                &agent,
                plugin.id.clone(),
                None,
                ExtensionOp::Uninstall(plugin),
            ),
            Removing::Marketplace(agent, name) => send(
                ui,
                &agent,
                name.clone(),
                None,
                ExtensionOp::RemoveMarketplace(name),
            ),
        };
    }
    // A menu of Customize's (a skill's states, where to install) closing on its own.
    if let Some(menu) = id.strip_prefix("ext:").and(id.strip_suffix(":dismiss")) {
        if ui.settings_menu.as_deref() == Some(menu) {
            ui.settings_menu = None;
        }
        return Some(vec![]);
    }
    // A marketplace goes to every agent ticked, each by its own program.
    if id == "ext:mkt-add" {
        let source = ui.marketplace_field.trim().to_owned();
        if source.is_empty() || ui.market_targets.is_empty() {
            return Some(vec![]);
        }
        ui.marketplace_field.clear();
        ui.marketplaces = None;
        let mut intents = vec![];
        for agent in std::mem::take(&mut ui.market_targets) {
            let op = ExtensionOp::AddMarketplace(source.clone());
            intents.extend(send(ui, &agent, source.clone(), None, op)?);
        }
        return Some(intents);
    }
    let (verb, rest) = id.strip_prefix("ext:")?.split_once(':')?;
    let (agent, name) = rest.split_once(':').unwrap_or((rest, ""));
    let known = ui.extensions.get(agent)?;
    let plugin = known.list.plugins.iter().find(|p| p.id == name).cloned();
    match verb {
        "switch" => {
            let plugin = plugin?;
            let enabled = !plugin.enabled;
            send(
                ui,
                agent,
                name.to_owned(),
                None,
                ExtensionOp::SwitchPlugin { plugin, enabled },
            )
        }
        "update" => send(
            ui,
            agent,
            name.to_owned(),
            None,
            ExtensionOp::Update(name.to_owned()),
        ),
        "remove" => {
            ui.removing = Some(Removing::Plugin(agent.to_owned(), plugin?));
            Some(vec![])
        }
        "mkt-update" => send(
            ui,
            agent,
            name.to_owned(),
            None,
            ExtensionOp::UpdateMarketplace(name.to_owned()),
        ),
        "mkt-remove" => {
            ui.removing = Some(Removing::Marketplace(agent.to_owned(), name.to_owned()));
            Some(vec![])
        }
        "install" => {
            // `{id}` for the person's own, `{id}={scope}` from the menu of scopes.
            let (id, scope) = match name.rsplit_once('=') {
                Some((id, scope)) => (id, Some(scope.to_owned())),
                None => (name, None),
            };
            if scope.is_none() && is_project(&folder) {
                let menu = format!("ext:install:{agent}:{id}");
                ui.settings_menu = (ui.settings_menu.as_deref() != Some(&menu)).then_some(menu);
                return Some(vec![]);
            }
            ui.settings_menu = None;
            let label = (known.list.offers.iter())
                .find(|o| o.id == id)
                .map_or(id.to_owned(), |o| o.name.clone());
            let op = ExtensionOp::Install {
                id: id.to_owned(),
                scope,
            };
            send(ui, agent, id.to_owned(), Some(label), op)
        }
        "skill" => {
            let (skill, state) = match name.rsplit_once('=') {
                Some((skill, state)) => (skill, state),
                None => {
                    let menu = format!("ext:skill:{agent}:{name}");
                    ui.settings_menu = (ui.settings_menu.as_deref() != Some(&menu)).then_some(menu);
                    return Some(vec![]);
                }
            };
            ui.settings_menu = None;
            let skill = known
                .list
                .skills
                .iter()
                .find(|s| s.name == skill)
                .cloned()?;
            let op = ExtensionOp::SkillState {
                state: state.to_owned(),
                skill: skill.clone(),
            };
            send(ui, agent, skill.name, None, op)
        }
        _ => None,
    }
}

/// The composer's "+": opened, it asks for the session's MCP servers and its agent's plugins,
/// which its Connectors and Plugins list; their switches, and where finding and managing
/// them go. `None` when `id` is none of them.
fn plus_clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    let session = ui.selected;
    if id == view::PLUS_MENU && ui.open_menu != Some(view::PLUS_MENU) {
        ui.open_menu = Some(view::PLUS_MENU);
        let mut intents: Vec<Intent> = session
            .map(|session| Intent::McpStatus { session })
            .into_iter()
            .collect();
        intents.extend(ui.ask_extensions());
        return Some(intents);
    }
    if let Some((on, name)) = id
        .strip_prefix(view::CONNECTOR_SWITCH)
        .and_then(|r| r.split_once(':'))
    {
        let session = session?;
        return Some(vec![
            Intent::McpToggle {
                session,
                name: name.to_owned(),
                enabled: on == "1",
            },
            Intent::McpStatus { session },
        ]);
    }
    // A plugin's skill goes into the message, as Desktop's chip does.
    if let Some(skill) = id.strip_prefix(view::PLUGIN_SKILL) {
        ui.open_menu = None;
        let mut prompt = ui.prompt.trim_end().to_owned();
        if !prompt.is_empty() {
            prompt.push(' ');
        }
        prompt.push_str(&format!("/{skill} "));
        ui.restore_prompt = Some(prompt);
        return Some(vec![]);
    }
    let page = |ui: &mut Ui, page: &str| {
        ui.open_menu = None;
        ui.settings_page = Some(page.to_owned());
        ui.settings_query.clear();
        ui.ask_extensions()
    };
    match id {
        view::CONNECTORS_MANAGE => {
            ui.open_menu = None;
            let session = session?;
            ui.mcp = Some(McpDialog::new(session));
            Some(vec![Intent::McpStatus { session }])
        }
        view::CONNECTORS_BROWSE => {
            page(ui, view::SETTINGS_CONNECTORS);
            ui.connectors.discover = true;
            Some(ui.ask_servers())
        }
        view::PLUGINS_MANAGE => Some(page(ui, view::SETTINGS_PLUGINS)),
        view::PLUGINS_BROWSE => {
            let intents = page(ui, view::SETTINGS_PLUGINS);
            ui.directory = Some(ui.agent.clone());
            ui.directory_query.clear();
            Some(intents)
        }
        _ => None,
    }
}

/// The "All agents" layer's clicks: an item's "Use in" and removal, and the skill dialogs.
/// `None` when `id` is none of them.
fn global_clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    use quill_model::GlobalOp;
    let rest = id.strip_prefix("glob:")?;
    let parts: Vec<&str> = rest.split(':').collect();
    let op = match parts[..] {
        ["use", kind, name, agent] => {
            let skill = kind == "skill";
            let off = if skill {
                (ui.global.skills.iter().find(|s| s.name == name)).map(|s| &s.except)
            } else {
                (ui.global.servers.iter().find(|s| s.name == name)).map(|s| &s.except)
            };
            GlobalOp::Use {
                skill,
                name: name.to_owned(),
                agent: agent.to_owned(),
                on: off?.iter().any(|a| a == agent),
            }
        }
        ["remove", "server", name] => GlobalOp::RemoveServer(name.to_owned()),
        ["remove", _, name] => GlobalOp::RemoveSkill(name.to_owned()),
        ["skill-new"] => {
            ui.settings_menu = None;
            ui.skill_dialog = Some(SkillDialog::New {
                name: String::new(),
                description: String::new(),
            });
            return Some(vec![]);
        }
        ["skill-import"] => {
            ui.settings_menu = None;
            ui.skill_dialog = Some(SkillDialog::Import {
                path: String::new(),
            });
            return Some(vec![]);
        }
        ["skill-cancel"] => {
            ui.skill_dialog = None;
            return Some(vec![]);
        }
        ["skill-save"] => match ui.skill_dialog.take()? {
            SkillDialog::New { name, description } => GlobalOp::NewSkill {
                name: name.trim().to_owned(),
                description: description.trim().to_owned(),
            },
            SkillDialog::Import { path } => GlobalOp::ImportSkill(path.trim().into()),
        },
        _ => return None,
    };
    ui.settings_menu = None;
    Some(vec![Intent::Global(op)])
}

/// Customize › Connectors' clicks: its tabs; "Add custom connector", empty or from an offer of
/// the registry; where the server goes; adding it; and a server's menu and removal. `None`
/// when `id` is none of them.
fn connectors_clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    let folder = customize_folder(ui);
    let rest = id.strip_prefix("srv:")?;
    let c = &mut ui.connectors;
    match rest.split_once(':').unwrap_or((rest, "")) {
        ("tab", tab) => c.discover = tab == "discover",
        // From the header, for the agent a new session would run; ticking more is the dialog's.
        ("add", agent) => {
            let agent = if agent.is_empty() {
                ui.agent.clone()
            } else {
                agent.to_owned()
            };
            c.adding = Some(AddServer {
                agents: vec![agent],
                ..AddServer::default()
            })
        }
        ("offer", at) => {
            let (_, offers, _) = c.found.as_ref()?;
            let offer = offers.get(at.parse::<usize>().ok()?)?.clone();
            // The registry's name's last part, as a server's own name.
            let name = offer.name.rsplit(['/', '.']).next().unwrap_or(&offer.name);
            let mut target = vec![offer.target.clone()];
            target.extend(offer.args.iter().cloned());
            ui.connectors.adding = Some(AddServer {
                agents: vec![ui.agent.clone()],
                name: name.to_owned(),
                target: target.join(" "),
                env: offer.env.join("\n"),
                headers: offer.headers.join("\n"),
                ..AddServer::default()
            });
        }
        // "All agents" is a choice of its own: quill adds it to each session.
        ("to", "*") => {
            let adding = c.adding.as_mut()?;
            adding.all = !adding.all;
        }
        ("sse", _) => {
            let adding = c.adding.as_mut()?;
            adding.sse = !adding.sse;
        }
        // An agent the server goes to, ticked or not; one stays.
        ("to", agent) => {
            let adding = c.adding.as_mut()?;
            adding.all = false;
            match adding.agents.iter().position(|a| a == agent) {
                Some(at) if adding.agents.len() > 1 => drop(adding.agents.remove(at)),
                Some(_) => {}
                None => adding.agents.push(agent.to_owned()),
            }
        }
        ("scope", rest) => {
            let (agent, scope) = rest.split_once(':')?;
            c.adding
                .as_mut()?
                .scopes
                .insert(agent.to_owned(), scope.to_owned());
        }
        ("cancel", _) => c.adding = None,
        // Each agent adds it with its own program, and keeps or fails on its own.
        ("save", _) => {
            let adding = c.adding.take()?;
            adding.server()?;
            let mut intents = vec![];
            if adding.all {
                intents.push(Intent::Global(quill_model::GlobalOp::SetServer(
                    adding.global()?,
                )));
            }
            for agent in adding.agents.iter().filter(|_| !adding.all) {
                let op = quill_model::ExtensionOp::AddServer(adding.server_for(agent)?);
                c.listed.entry(agent.clone()).or_default().working = true;
                intents.push(Intent::ExtensionOp {
                    agent: agent.clone(),
                    folder: folder.clone(),
                    op,
                });
            }
            return Some(intents);
        }
        ("remove", rest) => {
            let (agent, name) = rest.split_once(':')?;
            ui.settings_menu = None;
            c.listed.entry(agent.to_owned()).or_default().working = true;
            return Some(vec![Intent::ExtensionOp {
                agent: agent.to_owned(),
                folder,
                op: quill_model::ExtensionOp::RemoveServer(name.to_owned()),
            }]);
        }
        _ => return None,
    }
    Some(vec![])
}

/// How often the open MCP dialog asks for its servers again (inferred: Desktop polls them,
/// at a pace not found).
pub const MCP_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// The MCP dialog's clicks: done; a server's details and back; reconnecting it, turning it
/// on, signing in to it and forgetting its credentials (after asking), each followed by
/// asking for the servers again. `None` when `id` is none of them.
fn mcp_clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    let mcp = ui.mcp.as_mut()?;
    let session = mcp.session;
    if id == view::MCP_DONE {
        ui.mcp = None;
        return Some(vec![]);
    }
    if id == view::MCP_BACK {
        mcp.detail = None;
        return Some(vec![]);
    }
    if let Some(name) = id.strip_prefix(view::MCP_ROW) {
        mcp.detail = Some(name.to_owned());
        return Some(vec![]);
    }
    if id == view::MCP_CLEAR_NO {
        mcp.clearing = None;
        return Some(vec![]);
    }
    let asked = |intent| Some(vec![intent, Intent::McpStatus { session }]);
    if id == view::MCP_CLEAR_YES {
        let name = mcp.clearing.take()?;
        mcp.notice = Some(format!("Authentication cleared for {name}."));
        return asked(Intent::McpClearAuth { session, name });
    }
    if let Some(name) = id.strip_prefix(view::MCP_CLEAR) {
        mcp.clearing = Some(name.to_owned());
        return Some(vec![]);
    }
    if let Some(name) = id.strip_prefix(view::MCP_CONNECT) {
        mcp.notice = None;
        mcp.phases.insert(name.to_owned(), McpPhase::Opening);
        return Some(vec![Intent::McpAuthenticate {
            session,
            name: name.to_owned(),
        }]);
    }
    if let Some(name) = id.strip_prefix(view::MCP_RECONNECT) {
        mcp.phases.remove(name);
        return asked(Intent::McpReconnect {
            session,
            name: name.to_owned(),
        });
    }
    if let Some(name) = id.strip_prefix(view::MCP_TURN_ON) {
        return asked(Intent::McpToggle {
            session,
            name: name.to_owned(),
            enabled: true,
        });
    }
    None
}

impl Ui {
    /// What the open MCP dialog hears: a sign-in's page opened in the browser, a sign-in that
    /// needed no one (the server is reconnected), a sign-in that could not start; and the
    /// servers, whose sign-in is over once their status changes, as Desktop's.
    fn mcp_heard(&mut self, event: &Event) -> Vec<Intent> {
        let Some(mcp) = self.mcp.as_mut() else {
            return vec![];
        };
        let Event::Agent { session, event, .. } = event else {
            return vec![];
        };
        if *session != mcp.session {
            return vec![];
        }
        match event {
            AgentEvent::McpSignIn { server, url, error } => match (url, error) {
                (_, Some(error)) => {
                    mcp.phases.remove(server);
                    mcp.notice = Some(error.clone());
                }
                (Some(_), None) => {
                    mcp.phases.insert(server.clone(), McpPhase::InBrowser);
                }
                (None, None) => {
                    mcp.phases.remove(server);
                    return vec![
                        Intent::McpReconnect {
                            session: *session,
                            name: server.clone(),
                        },
                        Intent::McpStatus { session: *session },
                    ];
                }
            },
            AgentEvent::McpServers(servers) => {
                for server in servers {
                    let was = mcp.status.insert(server.name.clone(), server.status);
                    if was.is_some_and(|was| was != server.status) {
                        mcp.phases.remove(&server.name);
                    }
                }
            }
            _ => {}
        }
        vec![]
    }
}

/// The session's MCP servers' dialog, while open.
#[derive(Clone, Debug)]
pub struct McpDialog {
    pub session: SessionId,
    /// When its servers were last asked for.
    pub asked: std::time::Instant,
    /// The server whose details show, by name; the list without one.
    pub detail: Option<String>,
    /// Where each sign-in under way is.
    pub phases: std::collections::HashMap<String, McpPhase>,
    /// Each server's status as last heard, to tell when a sign-in is over.
    pub status: std::collections::HashMap<String, quill_model::McpStatus>,
    /// The server whose credentials the person is asked about forgetting.
    pub clearing: Option<String>,
    /// What the last action said: a sign-in that could not start, credentials cleared.
    pub notice: Option<String>,
}

impl McpDialog {
    pub fn new(session: SessionId) -> Self {
        Self {
            session,
            asked: std::time::Instant::now(),
            detail: None,
            phases: std::collections::HashMap::new(),
            status: std::collections::HashMap::new(),
            clearing: None,
            notice: None,
        }
    }
}

/// A sign-in to a server, as Desktop's phases name it: its page being asked for ("Opening
/// sign-in…"), then open in the browser ("Complete sign-in in your browser…").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpPhase {
    Opening,
    InBrowser,
}

/// Settings' own clicks: open and close, pick a page, and change a
/// setting, which the engine keeps before the UI shows it. `None` when `id` is none of
/// them. A select reports its trigger, `set:{name}`, then a choice, `set:{name}={value}`.
/// A notification level from its select's value; an unknown one is the default.
fn notify_level(value: &str) -> NotifyLevel {
    match value {
        "off" => NotifyLevel::Off,
        "badge" => NotifyLevel::Badge,
        _ => NotifyLevel::Banner,
    }
}

fn settings_clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    use quill_model::{InterfaceFont, Motion, TextSize, Theme, TranscriptView};
    if id == view::ACCOUNT_MENU {
        ui.account_menu = !ui.account_menu;
        return Some(vec![]);
    }
    if id.strip_suffix(":dismiss") == Some(view::ACCOUNT_MENU) {
        ui.account_menu = false;
        return Some(vec![]);
    }
    if id == view::SETTINGS_OPEN {
        ui.account_menu = false;
        ui.settings_page = Some(view::SETTINGS_FIRST.into());
        ui.settings_query.clear();
        return Some(vec![Intent::MeasureStorage]);
    }
    // The sidebar's Customize opens Settings on its first page of them, as Desktop's opens
    // its Customize.
    if id == view::CUSTOMIZE_OPEN {
        ui.settings_page = Some(view::SETTINGS_SKILLS.into());
        ui.settings_query.clear();
        return Some(ui.ask_extensions());
    }
    ui.settings_page.as_ref()?;
    if id == view::SETTINGS_CLOSE {
        ui.settings_page = None;
        ui.settings_menu = None;
        ui.settings_error = None;
        return Some(vec![]);
    }
    if let Some(page) = id.strip_prefix("settings:page:") {
        ui.settings_page = Some(page.to_owned());
        ui.settings_query.clear();
        // Customize's pages read each agent's plugins and skills, or servers, afresh.
        if [view::SETTINGS_PLUGINS, view::SETTINGS_SKILLS].contains(&page) {
            return Some(ui.ask_extensions());
        }
        if page == view::SETTINGS_CONNECTORS {
            return Some(ui.ask_servers());
        }
        // The Accounts page shows the contributions, read when it opens.
        if page == view::SETTINGS_ACCOUNTS {
            return Some(github::want_calendar(ui));
        }
        return Some(vec![]);
    }
    if let Some(intents) = customize_clicked(ui, id) {
        return Some(intents);
    }
    if let Some(intents) = extensions_clicked(ui, id) {
        return Some(intents);
    }
    if let Some(intents) = global_clicked(ui, id) {
        return Some(intents);
    }
    if let Some(intents) = connectors_clicked(ui, id) {
        return Some(intents);
    }
    let rest = id.strip_prefix("set:")?;
    // A new change is tried afresh: the last one's failure goes.
    ui.settings_error = None;
    if let Some(menu) = rest.strip_suffix(":dismiss") {
        if ui.settings_menu.as_deref() == Some(menu) {
            ui.settings_menu = None;
        }
        return Some(vec![]);
    }
    let mut settings = ui.settings.clone();
    if let Some((name, value)) = rest.split_once('=') {
        ui.settings_menu = None;
        match (name, value) {
            ("theme", "system") => settings.theme = Theme::System,
            ("theme", "light") => settings.theme = Theme::Light,
            ("theme", "dark") => settings.theme = Theme::Dark,
            ("interface_font", "system") => settings.interface_font = InterfaceFont::System,
            ("interface_font", "ubuntu") => settings.interface_font = InterfaceFont::Ubuntu,
            ("notify_permission", value) => settings.notify_permission = notify_level(value),
            ("notify_question", value) => settings.notify_question = notify_level(value),
            ("notify_done", value) => settings.notify_done = notify_level(value),
            ("notification_sound", value) => settings.notification_sound = value == "system",
            ("text_size", "small") => settings.text_size = TextSize::Small,
            ("text_size", "medium") => settings.text_size = TextSize::Medium,
            ("text_size", "large") => settings.text_size = TextSize::Large,
            ("transcript_view", "normal") => settings.transcript_view = TranscriptView::Normal,
            ("transcript_view", "thinking") => settings.transcript_view = TranscriptView::Thinking,
            ("transcript_view", "verbose") => settings.transcript_view = TranscriptView::Verbose,
            ("width", value) => {
                settings.width = match value {
                    "wide" => quill_model::Width::Wide,
                    "medium" => quill_model::Width::Medium,
                    _ => quill_model::Width::Narrow,
                }
            }
            ("motion", "system") => settings.motion = Motion::System,
            ("motion", "reduced") => settings.motion = Motion::Reduced,
            ("motion", "full") => settings.motion = Motion::Full,
            ("pr_kind", value) => settings.pr_draft = value == "draft",
            ("archive", days) => settings.archive_after_days = days.parse().unwrap_or(0),
            ("browser_site_remove", site) => settings.browser_sites.retain(|s| s != site),
            ("browser_program", program) => {
                settings.browser_program = (!program.is_empty()).then(|| program.to_owned())
            }
            (name, value) => {
                let (agent, field) = name.strip_prefix("agent:")?.rsplit_once(':')?;
                let entry = settings.agents.entry(agent.to_owned()).or_default();
                // An empty choice leaves it to the agent.
                let value = (!value.is_empty()).then(|| value.to_owned());
                match field {
                    "model" => {
                        entry.model = value;
                        // An effort the new model lacks would not start.
                        entry.effort = None;
                    }
                    "effort" => entry.effort = value,
                    "mode" => entry.mode = value,
                    _ => {
                        let key = field.strip_prefix("opt.")?;
                        entry
                            .options
                            .insert(key.to_owned(), value.unwrap_or_default());
                    }
                }
            }
        }
        return Some(vec![Intent::SaveSettings(settings)]);
    }
    match rest {
        "keep_awake" => settings.keep_awake = !settings.keep_awake,
        "keep_awake_battery" => settings.keep_awake_on_battery = !settings.keep_awake_on_battery,
        "remote_new" => settings.remote_new_sessions = !settings.remote_new_sessions,
        "attention" => settings.attention = !settings.attention,
        "login" => settings.open_at_login = !settings.open_at_login,
        "browser_native" => settings.browser_native = !settings.browser_native,
        "links_in_browser" => settings.links_in_browser = !settings.links_in_browser,
        "prs_in_browser" => settings.prs_in_browser = !settings.prs_in_browser,
        "archive_on_merge" => settings.archive_on_merge = !settings.archive_on_merge,
        "logos" => settings.logos = !settings.logos,
        "auto_update" => settings.auto_update = !settings.auto_update,
        other => match other
            .strip_prefix("agent:")
            .and_then(|r| r.strip_suffix(":unsafe"))
        {
            Some(agent) => {
                let entry = settings.agents.entry(agent.to_owned()).or_default();
                entry.allow_unsafe = !entry.allow_unsafe;
            }
            // An adapter's switch flips; its list opens, below.
            None if agent_switch(ui, other).is_some() => {
                let (agent, key, default) = agent_switch(ui, other)?;
                let entry = settings.agents.entry(agent).or_default();
                let on = entry
                    .options
                    .get(&key)
                    .map_or(default == "true", |v| v == "true");
                entry.options.insert(key, (!on).to_string());
            }
            // A select's trigger opens its list, or closes it.
            None => {
                ui.settings_menu =
                    (ui.settings_menu.as_deref() != Some(other)).then(|| other.to_owned());
                return Some(vec![]);
            }
        },
    }
    Some(vec![Intent::SaveSettings(settings)])
}

/// The sessions with nothing new for `days` days at `now` (seconds since 1970), which
/// Settings' "Archive inactive sessions" puts away: never a pinned one, one at work, or one
/// whose messages carry no time.
pub fn inactive(store: &Store, days: u32, now: u64) -> Vec<SessionId> {
    if days == 0 {
        return vec![];
    }
    let limit = now.saturating_sub(u64::from(days) * 86_400);
    (store.sessions.iter())
        .filter(|s| !s.archived && !s.pinned && !s.working())
        .filter(|s| {
            let last = (s.items.iter())
                .filter_map(|i| match i {
                    Item::User { at, .. } | Item::Assistant { at, .. } => *at,
                    _ => None,
                })
                .max();
            last.is_some_and(|last| last < limit)
        })
        .map(|s| s.id)
        .collect()
}

/// `agent:{agent}:opt.{key}` when it names an adapter's switch: the agent, the key and its
/// default.
fn agent_switch(ui: &Ui, name: &str) -> Option<(String, String, String)> {
    let (agent, key) = name.strip_prefix("agent:")?.split_once(":opt.")?;
    let option = (ui.agents.iter())
        .find(|a| a.id == agent)?
        .options
        .iter()
        .find(|o| o.key == key && o.choices.is_empty())?;
    Some((agent.to_owned(), key.to_owned(), option.default.clone()))
}

/// The actions under a reply: copy, fork from it, and read it
/// aloud, by "{action}:{session}:{item}"; and the branch bar's "View changes". `None` when
/// `id` is none of them.
fn message_clicked(store: &Store, ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    if id == view::VIEW_CHANGES {
        let session = ui.selected?;
        ui.diff = None;
        ui.open_pane(Pane::Diff);
        return Some(vec![Intent::WorkingTree { session }]);
    }
    let (action, rest) = id.split_once(':')?;
    let (session, at) = rest.split_once(':')?;
    let (session, at): (SessionId, usize) = (session.parse().ok()?, at.parse().ok()?);
    if let Some(Item::User { request, text, .. }) =
        store.session(session).and_then(|s| s.items.get(at))
    {
        let s = store.session(session)?;
        // Both go on from the agent's message before this one, its words back in the box.
        let before = (s.items[..at].iter().rev()).find_map(|i| match i {
            Item::Assistant { id: Some(id), .. } => Some(id.clone()),
            _ => None,
        });
        return Some(match action {
            "copy" => {
                ui.copied = Some((id.to_owned(), std::time::Instant::now()));
                vec![]
            }
            "rewind-to" if !s.working() => {
                ui.restore_prompt = Some(text.clone());
                vec![Intent::RewindTo {
                    session,
                    request: request.clone(),
                    at: before,
                }]
            }
            "fork-from" if before.is_some() => {
                ui.waiting_fork = true;
                ui.restore_prompt = Some(text.clone());
                vec![Intent::Fork {
                    session,
                    at: before,
                }]
            }
            _ => vec![],
        });
    }
    // A thinking block copies as a quote and has no other action.
    if let Some(Item::Thinking { .. }) = store.session(session).and_then(|s| s.items.get(at)) {
        if action == "copy" {
            ui.copied = Some((id.to_owned(), std::time::Instant::now()));
        }
        return Some(vec![]);
    }
    let Some(Item::Assistant {
        text, id: message, ..
    }) = store.session(session).and_then(|s| s.items.get(at))
    else {
        return None;
    };
    Some(match action {
        // The host puts the words on the clipboard; the button shows a check a moment.
        "copy" => {
            ui.copied = Some((id.to_owned(), std::time::Instant::now()));
            vec![]
        }
        "fork-at" => {
            ui.waiting_fork = true;
            vec![Intent::Fork {
                session,
                at: message.clone(),
            }]
        }
        "read" if ui.reading == Some((session, at)) => {
            ui.reading = None;
            vec![Intent::ReadAloud {
                session,
                text: None,
            }]
        }
        "read" => {
            ui.reading = Some((session, at));
            vec![Intent::ReadAloud {
                session,
                text: Some(text.clone()),
            }]
        }
        _ => return None,
    })
}

/// The words of the reply a "copy:{session}:{item}" button stands for; a thinking block's
/// as a quote.
pub fn copied_text(store: &Store, id: &str) -> Option<String> {
    let (session, at) = id.strip_prefix("copy:")?.split_once(':')?;
    let session = store.session(session.parse().ok()?)?;
    match session.items.get(at.parse::<usize>().ok()?)? {
        Item::Assistant { text, .. } | Item::User { text, .. } => Some(text.clone()),
        Item::Thinking { text, .. } => Some(
            text.lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => None,
    }
}

/// Whether a session offers Thinking: its agent can show what the model thought, and the
/// session has thought or has not answered yet (as in Desktop).
pub fn thinking_offered(session: &Session, thinking_text: bool) -> bool {
    let items = &session.items;
    thinking_text
        && (items.iter().any(|i| matches!(i, Item::Thinking { .. }))
            || !items.iter().any(|i| matches!(i, Item::Assistant { .. })))
}

/// The view a session's transcript is drawn in: its own choice or the default, and Thinking
/// where it is not offered is Normal.
pub fn view_of(session: &Session, settings: &Settings, thinking_text: bool) -> TranscriptView {
    match session.view.unwrap_or(settings.transcript_view) {
        TranscriptView::Thinking if !thinking_offered(session, thinking_text) => {
            TranscriptView::Normal
        }
        view => view,
    }
}

/// `view_of` for the session's own agent.
pub fn current_view(session: &Session, settings: &Settings) -> TranscriptView {
    let thinking = session
        .capabilities
        .as_ref()
        .is_some_and(|c| c.thinking_text);
    view_of(session, settings, thinking)
}

/// The view Ctrl+O goes to after the session's: Normal, Thinking, Verbose, and back, without
/// Thinking where it is not offered.
fn next_view(session: &Session, settings: &Settings) -> TranscriptView {
    let thinking = session
        .capabilities
        .as_ref()
        .is_some_and(|c| c.thinking_text);
    let views: Vec<TranscriptView> = [
        TranscriptView::Normal,
        TranscriptView::Thinking,
        TranscriptView::Verbose,
    ]
    .into_iter()
    .filter(|v| *v != TranscriptView::Thinking || thinking_offered(session, thinking))
    .collect();
    let now = current_view(session, settings);
    let at = views.iter().position(|v| *v == now).unwrap_or(0);
    views[(at + 1) % views.len()]
}
