//! Customize as a page of its own, in the conversation's place (docs/extensiones.md §12): a
//! header with the layer it shows (every agent with quill's "All agents" first, or one agent),
//! the tabs Skills, Connectors and Plugins, breadcrumbs inside an item, and each tab's search,
//! Discover and "Add ▾". A tab none of the agents shown takes part in is off, with why.
//!
//! A connector's page lists its tools by what they do, each with when the agent may use it
//! (Always allow, Needs approval, Blocked); quill keeps the choice a moment after the last
//! change and each agent's adapter says it in that agent's own permission settings.

use crate::view::{self, SETTINGS_CONNECTORS, SETTINGS_PLUGINS, SETTINGS_SKILLS};
use crate::{icons, Intent, Store, Ui};
use limn::{Component, Element, FontFamily};
use quill_components::customize;
use quill_components::look::{self, BODY, SMALL};
use quill_components::pieces::{hgap, icon_tool, muted, text};
use quill_model::{AgentInfo, McpSource, McpStatus, McpTool, ToolChoice, ToolPolicy};

/// How long after the last change of a tool's permission it is kept, so a run of clicks is
/// kept once.
pub const KEEP_AFTER: std::time::Duration = std::time::Duration::from_millis(500);

/// The page's own state while it shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    /// Its tab: [`SETTINGS_SKILLS`], [`SETTINGS_CONNECTORS`] or [`SETTINGS_PLUGINS`].
    pub tab: String,
    /// The layer shown: every agent, quill's "All agents" first (`None`), or one agent.
    pub layer: Option<String>,
    /// The item open under the breadcrumbs.
    pub detail: Option<Detail>,
    /// Connectors: all, the connected ones or the others.
    pub filter: Filter,
    /// Skills: in one list by name, else by where they come from, and only those from here.
    pub by_name: bool,
    pub source: Option<String>,
}

/// An item open on its own page, by its agent (empty for quill's "All agents").
#[derive(Clone, Debug, PartialEq)]
pub enum Detail {
    Connector { agent: String, name: String },
    Skill { agent: String, name: String },
    Plugin { agent: String, id: String },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Connected,
    NotConnected,
}

/// Open the page on `tab`, keeping its layer, and read what the tab lists afresh.
pub fn open(ui: &mut Ui, tab: &str) -> Vec<Intent> {
    let layer = ui.customize.as_ref().and_then(|p| p.layer.clone());
    ui.customize = Some(Page {
        tab: tab.to_owned(),
        layer,
        ..Page::default()
    });
    ui.customize_query.clear();
    ui.settings_menu = None;
    ui.settings_page = None;
    if tab == SETTINGS_CONNECTORS {
        ui.ask_servers()
    } else {
        ui.ask_extensions()
    }
}

/// The agents the page shows: every one, or the one its layer chose.
pub fn agents(ui: &Ui) -> Vec<&AgentInfo> {
    let layer = ui.customize.as_ref().and_then(|p| p.layer.as_deref());
    (ui.agents.iter())
        .filter(|a| layer.is_none_or(|id| a.id == id))
        .collect()
}

/// Why `tab` cannot be used with the agents shown, from what their adapters declare; `None`
/// when one of them takes part in it.
pub fn tab_off(ui: &Ui, tab: &str) -> Option<String> {
    let (has, what): (fn(&AgentInfo) -> bool, &str) = match tab {
        SETTINGS_PLUGINS => (|a| a.customize.plugins, "plugins"),
        SETTINGS_CONNECTORS => (|a| a.customize.connectors, "connectors"),
        _ => return None,
    };
    let shown = agents(ui);
    if shown.iter().any(|a| has(a)) {
        return None;
    }
    Some(
        match (shown.as_slice(), ui.customize.as_ref()?.layer.is_some()) {
            ([one], true) => format!("{} has no {what} quill can manage.", one.name),
            _ => format!("None of your agents has {what} quill can manage."),
        },
    )
}

/// The arrow at an item's end that opens it on its own page.
pub fn opener(kind: &str, agent: &str, name: &str) -> Element {
    icon_tool(
        &format!("czp:open:{kind}:{agent}:{name}"),
        icons::of(icons::CARET_RIGHT),
        "Open",
        28.0,
        look::palette(),
    )
}

/// The page's own clicks; `None` when `id` is none of them.
pub fn clicked(store: &Store, ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    if id == view::CUSTOMIZE_OPEN {
        let tab = (ui.customize.as_ref()).map_or(SETTINGS_SKILLS.to_owned(), |p| p.tab.clone());
        return Some(open(ui, &tab));
    }
    let rest = id.strip_prefix("czp:")?;
    let (verb, arg) = rest.split_once(':').unwrap_or((rest, ""));
    // The menus of the page close on their own as their entries do.
    if verb == "file" {
        // The host asks the platform for the file and answers with `file_chosen`.
        ui.settings_menu = None;
        return Some(vec![]);
    }
    match verb {
        "tab" => {
            if tab_off(ui, arg).is_some() {
                return Some(vec![]);
            }
            return Some(open(ui, arg));
        }
        "settings" => {
            ui.settings_page = Some(view::SETTINGS_FIRST.into());
            ui.settings_query.clear();
            return Some(vec![Intent::MeasureStorage]);
        }
        "crumb" => {
            let page = ui.customize.as_mut()?;
            page.detail = None;
            if arg == "root" {
                return Some(open(ui, SETTINGS_SKILLS));
            }
        }
        "layer" => {
            ui.settings_menu = None;
            let page = ui.customize.as_mut()?;
            page.layer = arg
                .strip_prefix('=')
                .filter(|a| !a.is_empty())
                .map(str::to_owned);
            page.detail = None;
            let tab = page.tab.clone();
            if tab_off(ui, &tab).is_some() {
                return Some(open(ui, SETTINGS_SKILLS));
            }
        }
        "filter" => {
            ui.customize.as_mut()?.filter = match arg {
                "connected" => Filter::Connected,
                "not" => Filter::NotConnected,
                _ => Filter::All,
            };
        }
        "sort" => {
            ui.settings_menu = None;
            ui.customize.as_mut()?.by_name = arg == "name";
        }
        "source" => {
            ui.settings_menu = None;
            ui.customize.as_mut()?.source = (!arg.is_empty()).then(|| arg.to_owned());
        }
        "open" => return Some(open_item(store, ui, arg)),
        "tools-again" => {
            let Some(Detail::Connector { agent, .. }) = ui.customize.as_ref()?.detail.clone()
            else {
                return Some(vec![]);
            };
            return Some(ask_tools(ui, &agent, true));
        }
        "reconnect-all" => return Some(reconnect_all(store, ui)),
        "tool" | "group" => return Some(choose(store, ui, verb, arg)),
        "skill-switch" => return Some(switch_skill(ui, arg)),
        "skill-delete" => {
            ui.settings_menu = None;
            ui.customize.as_mut()?.detail = None;
            return Some(vec![Intent::Global(quill_model::GlobalOp::RemoveSkill(
                arg.to_owned(),
            ))]);
        }
        // "Create with an agent": a new session's message asks the agent to write one.
        "skill-create" => {
            ui.settings_menu = None;
            ui.customize = None;
            ui.selected = None;
            ui.restore_prompt = Some(
                "Help me write a new skill: ask me what it should do and when to use it, then write its SKILL.md."
                    .into(),
            );
        }
        _ => return None,
    }
    Some(vec![])
}

/// An item opened from its arrow (`{kind}:{agent}:{name}`); a connector's tools asked of its
/// agent when no session has said them.
fn open_item(store: &Store, ui: &mut Ui, arg: &str) -> Vec<Intent> {
    let Some((kind, rest)) = arg.split_once(':') else {
        return vec![];
    };
    let Some((agent, name)) = rest.split_once(':') else {
        return vec![];
    };
    let (agent, name) = (agent.to_owned(), name.to_owned());
    ui.settings_menu = None;
    // From Discover's directory, its page opens in its place.
    ui.directory = None;
    ui.marketplaces = None;
    let Some(page) = ui.customize.as_mut() else {
        return vec![];
    };
    page.detail = Some(match kind {
        "srv" => Detail::Connector {
            agent: agent.clone(),
            name: name.clone(),
        },
        "skill" => Detail::Skill {
            agent: agent.clone(),
            name: name.clone(),
        },
        _ => Detail::Plugin {
            agent: agent.clone(),
            id: name.clone(),
        },
    });
    if kind != "srv" || tools_of(store, ui, &agent, &name).is_some() {
        return vec![];
    }
    ask_tools(ui, &agent, false)
}

/// Ask `agent`'s program for its servers' tools (the agent a new session runs, for an "All
/// agents" server), once unless `again`.
fn ask_tools(ui: &mut Ui, agent: &str, again: bool) -> Vec<Intent> {
    let agent = if agent.is_empty() {
        ui.agent.clone()
    } else {
        agent.to_owned()
    };
    if !again && ui.connectors.tools.contains_key(&agent) {
        return vec![];
    }
    ui.connectors
        .tools
        .insert(agent.clone(), crate::ServerTools::default());
    vec![Intent::ServerTools {
        agent,
        folder: crate::customize_folder(ui),
    }]
}

/// A connector's tools: as a session of its agent loaded them, else as its program said them
/// outside a session. `None` while nobody has said.
pub fn tools_of(store: &Store, ui: &Ui, agent: &str, name: &str) -> Option<Vec<McpTool>> {
    let loaded = (store.sessions.iter())
        .filter(|s| agent.is_empty() || s.agent == agent)
        .rev()
        .filter_map(|s| s.mcp.as_ref())
        .flatten()
        .find(|m| (m.name == name || m.label == name) && !m.tools.is_empty());
    if let Some(server) = loaded {
        return Some(server.tools.clone());
    }
    let asked = if agent.is_empty() { &ui.agent } else { agent };
    let servers = ui.connectors.tools.get(asked)?.servers.as_ref()?;
    Some(
        (servers.iter())
            .find(|m| m.name == name || m.label == name)
            .map(|m| m.tools.clone())
            .unwrap_or_default(),
    )
}

/// The tool groups of a connector's page: what only reads, what changes or deletes, the rest.
pub const GROUPS: [(&str, &str); 3] = [
    ("read", "Read-only"),
    ("write", "Write/delete"),
    ("other", "Other"),
];

fn group_of(tool: &McpTool) -> &'static str {
    if tool.read_only {
        "read"
    } else if tool.destructive {
        "write"
    } else {
        "other"
    }
}

fn policy_word(policy: ToolPolicy) -> &'static str {
    match policy {
        ToolPolicy::Allow => "allow",
        ToolPolicy::Ask => "ask",
        ToolPolicy::Block => "block",
    }
}

/// The policies the agents of a connector can be given: its agent's, or for an "All agents"
/// server those every agent can.
pub fn policies(ui: &Ui, agent: &str) -> Vec<ToolPolicy> {
    let mut all = ToolPolicy::ALL.to_vec();
    for a in ui
        .agents
        .iter()
        .filter(|a| agent.is_empty() || a.id == agent)
    {
        all.retain(|p| a.customize.tool_policies.contains(p));
    }
    all
}

/// The policy shown for a tool: the one changed and not kept yet, else the one kept.
pub fn policy(ui: &Ui, agent: &str, server: &str, tool: &str) -> Option<ToolPolicy> {
    let same = |c: &&ToolChoice| c.agent == agent && c.server == server && c.tool == tool;
    match ui.tool_changes.iter().rev().find(same) {
        Some(changed) => changed.policy,
        None => ui.global.tools.iter().find(same).and_then(|c| c.policy),
    }
}

/// A tool's policy chosen (`tool:{policy}:{tool}`), or a whole group's from its menu
/// (`group:{policy}:{group}`); `default` gives it back to the agent's own settings. Kept
/// [`KEEP_AFTER`] the last change.
fn choose(store: &Store, ui: &mut Ui, verb: &str, arg: &str) -> Vec<Intent> {
    ui.settings_menu = None;
    let Some((word, what)) = arg.split_once(':') else {
        return vec![];
    };
    let Some(Some(Detail::Connector { agent, name })) =
        ui.customize.as_ref().map(|p| p.detail.clone())
    else {
        return vec![];
    };
    let chosen = match word {
        "allow" => Some(ToolPolicy::Allow),
        "ask" => Some(ToolPolicy::Ask),
        "block" => Some(ToolPolicy::Block),
        _ => None,
    };
    if chosen.is_some_and(|p| !policies(ui, &agent).contains(&p)) {
        return vec![];
    }
    let tools = tools_of(store, ui, &agent, &name).unwrap_or_default();
    let names: Vec<String> = if verb == "group" {
        (tools.iter())
            .filter(|t| group_of(t) == what)
            .map(|t| t.name.clone())
            .collect()
    } else {
        vec![what.to_owned()]
    };
    for tool in names {
        ui.tool_changes.push(ToolChoice {
            agent: agent.clone(),
            server: name.clone(),
            tool,
            policy: chosen,
        });
    }
    ui.tool_changed = Some(std::time::Instant::now());
    vec![]
}

/// "Reconnect all": each connector that needs the person is connected again in every session
/// that loaded it, and each agent's list is read again.
fn reconnect_all(store: &Store, ui: &mut Ui) -> Vec<Intent> {
    let mut intents = vec![];
    for (agent, names) in attention(ui) {
        for session in store.sessions.iter().filter(|s| s.agent == agent) {
            let loaded = session.mcp.iter().flatten();
            for server in loaded.filter(|m| names.contains(&m.name)) {
                intents.push(Intent::McpReconnect {
                    session: session.id,
                    name: server.name.clone(),
                });
            }
        }
        ui.connectors
            .listed
            .entry(agent.clone())
            .or_default()
            .working = true;
        intents.push(Intent::Servers {
            agent,
            folder: crate::customize_folder(ui),
        });
    }
    intents
}

/// The connectors that need the person (a sign-in, or one that fails), by agent, of the agents
/// shown.
pub fn attention(ui: &Ui) -> Vec<(String, Vec<String>)> {
    agents(ui)
        .into_iter()
        .filter_map(|a| {
            let servers = ui.connectors.listed.get(&a.id)?.servers.as_ref()?;
            let names: Vec<String> = (servers.iter())
                .filter(|s| matches!(s.status, McpStatus::NeedsAuth | McpStatus::Failed))
                .map(|s| s.name.clone())
                .collect();
            (!names.is_empty()).then(|| (a.id.clone(), names))
        })
        .collect()
}

/// A skill on or off from its row: the agent's first state or its last, or, for an "All
/// agents" skill with one agent shown, in that agent.
fn switch_skill(ui: &mut Ui, arg: &str) -> Vec<Intent> {
    let Some((agent, name)) = arg.split_once(':') else {
        return vec![];
    };
    let folder = crate::customize_folder(ui);
    if agent.is_empty() {
        let Some(layer) = ui.customize.as_ref().and_then(|p| p.layer.clone()) else {
            return vec![];
        };
        let Some(skill) = ui.global.skills.iter().find(|s| s.name == name) else {
            return vec![];
        };
        return vec![Intent::Global(quill_model::GlobalOp::Use {
            skill: true,
            name: name.to_owned(),
            on: skill.except.contains(&layer),
            agent: layer,
        })];
    }
    let Some(known) = ui.extensions.get_mut(agent) else {
        return vec![];
    };
    let Some(skill) = known.list.skills.iter().find(|s| s.name == name).cloned() else {
        return vec![];
    };
    let states = &known.list.skill_states;
    let (Some(on), Some(off)) = (states.first(), states.last()) else {
        return vec![];
    };
    if skill.locked.is_some() || states.len() < 2 {
        return vec![];
    }
    let state = if skill.state == on.0 {
        off.0.clone()
    } else {
        on.0.clone()
    };
    known.working = Some(skill.name.clone());
    known.error = None;
    vec![Intent::ExtensionOp {
        agent: agent.to_owned(),
        folder,
        op: quill_model::ExtensionOp::SkillState { skill, state },
    }]
}

/// A file the host's dialog gave for `purpose` (`czp:file:upload`, `…:replace:{name}`,
/// `…:download:{name}`): a skill to add, one to put in another's place, or where to save one.
pub fn file_chosen(ui: &mut Ui, purpose: &str, path: std::path::PathBuf) -> Vec<Intent> {
    use quill_model::GlobalOp;
    let Some(rest) = purpose.strip_prefix("czp:file:") else {
        return vec![];
    };
    let (verb, name) = rest.split_once(':').unwrap_or((rest, ""));
    let op = match verb {
        "upload" => GlobalOp::UploadSkill {
            path,
            replace: None,
        },
        "replace" => GlobalOp::UploadSkill {
            path,
            replace: Some(name.to_owned()),
        },
        "download" => GlobalOp::ExportSkill {
            name: name.to_owned(),
            to: path,
        },
        _ => return vec![],
    };
    ui.settings_menu = None;
    vec![Intent::Global(op)]
}

/// The id of the dialog's purpose for an action of a skill's page, which the host answers with
/// a file: `upload`, `replace` or `download`.
pub fn file_action(verb: &str, name: &str) -> String {
    if name.is_empty() {
        format!("czp:file:{verb}")
    } else {
        format!("czp:file:{verb}:{name}")
    }
}

// The page.

const SCROLL: &str = "czp:scroll";

/// The page in `width` × `height`: its header, tabs and crumbs, then the tab or the item, in a
/// column of at most 1040 centered in it.
pub fn page(store: &Store, ui: &Ui, width: f32, height: f32) -> Element {
    let Some(page) = &ui.customize else {
        return Element::spacer();
    };
    let inner = (width - 48.0).clamp(240.0, 1040.0);
    let side = ((width - inner) / 2.0).max(0.0);
    let mut top = vec![header(ui, inner), tab_strip(ui, page)];
    let body = match &page.detail {
        Some(detail) => {
            top.push(crumbs(ui, page, detail, inner));
            match detail {
                Detail::Connector { agent, name } => connector(store, ui, agent, name, inner),
                Detail::Skill { agent, name } => skill(ui, agent, name, inner),
                Detail::Plugin { agent, id } => plugin(ui, agent, id, inner),
            }
        }
        None => {
            top.push(toolbar(ui, page, inner));
            match page.tab.as_str() {
                SETTINGS_CONNECTORS => connectors(ui, page, inner),
                SETTINGS_PLUGINS => view::plugins_page(ui, inner),
                _ => view::skills_page(ui, inner),
            }
        }
    };
    let column = Element::column(vec![
        Element::column(top).gap(14.0).size(Some(inner), None),
        Element::spacer().size(None, Some(8.0)),
        Element::scroll(
            SCROLL,
            Element::column(vec![body, Element::spacer().size(None, Some(32.0))])
                .size(Some(inner), None),
        )
        .flex(1.0),
    ])
    .size(Some(inner), Some(height - 20.0));
    Element::column(vec![
        Element::spacer().size(None, Some(20.0)),
        Element::row(vec![hgap(side), column]).size(Some(width), None),
    ])
    .size(Some(width), Some(height))
}

/// "Customize" in 22 bold, the layer it shows as a chip with its menu, and Settings' gear.
fn header(ui: &Ui, width: f32) -> Element {
    let layer = ui.customize.as_ref().and_then(|p| p.layer.clone());
    let chosen = layer
        .as_ref()
        .and_then(|id| ui.agents.iter().find(|a| &a.id == id));
    let (icon, label) = match chosen {
        Some(agent) => (icons::agent(agent.icon.as_deref()), agent.name.clone()),
        None => (icons::GLOBE, "All agents".to_owned()),
    };
    let menu = "cz:layer".to_owned();
    let mut entries: Vec<limn::DropdownMenuEntry> =
        vec![limn::DropdownMenuItem::new("czp:layer:=", "All agents")
            .icon(icons::of(icons::GLOBE))
            .description("quill's own layer first, then each agent's")
            .radio(layer.is_none())
            .into()];
    entries.extend(ui.agents.iter().map(|a| {
        limn::DropdownMenuItem::new(format!("czp:layer:={}", a.id), a.name.clone())
            .icon(icons::of(icons::agent(a.icon.as_deref())))
            .radio(layer.as_deref() == Some(a.id.as_str()))
            .into()
    }));
    let chip = customize::menu_button(
        customize::button(&menu, &label, Some(icon), true, false),
        &menu,
        entries,
        ui.settings_menu.as_deref() == Some(menu.as_str()),
    );
    let gear = icon_tool(
        "czp:settings",
        icons::of(icons::SETTINGS),
        "Settings",
        28.0,
        look::palette(),
    );
    Element::row(vec![
        text("Customize")
            .font_family(FontFamily::SansBold)
            .font_size(22.0)
            .centered_y(),
        chip.centered_y(),
        Element::spacer().flex(1.0),
        gear,
    ])
    .gap(12.0)
    .size(Some(width), Some(32.0))
}

fn tab_strip(ui: &Ui, page: &Page) -> Element {
    let tabs: Vec<(String, &str, bool, Option<String>)> = [
        (SETTINGS_SKILLS, "Skills"),
        (SETTINGS_CONNECTORS, "Connectors"),
        (SETTINGS_PLUGINS, "Plugins"),
    ]
    .into_iter()
    .map(|(key, label)| {
        (
            format!("czp:tab:{key}"),
            label,
            page.tab == key,
            tab_off(ui, key),
        )
    })
    .collect();
    let tabs: Vec<(&str, &str, bool, Option<String>)> = (tabs.iter())
        .map(|(id, label, on, off)| (id.as_str(), *label, *on, off.clone()))
        .collect();
    customize::tabs(&tabs)
}

fn tab_label(tab: &str) -> &'static str {
    match tab {
        SETTINGS_CONNECTORS => "Connectors",
        SETTINGS_PLUGINS => "Plugins",
        _ => "Skills",
    }
}

/// Customize › {tab} [› Your connectors] › {item}.
fn crumbs(ui: &Ui, page: &Page, detail: &Detail, width: f32) -> Element {
    let name = match detail {
        Detail::Connector { name, .. } | Detail::Skill { name, .. } => name.clone(),
        Detail::Plugin { agent, id } => (ui.extensions.get(agent))
            .and_then(|k| k.list.plugins.iter().find(|p| &p.id == id))
            .map_or(id.clone(), |p| p.name.clone()),
    };
    let mut items = vec![
        limn::BreadcrumbItem::new("Customize").action("czp:crumb:root"),
        limn::BreadcrumbItem::new(tab_label(&page.tab)).action("czp:crumb:tab"),
    ];
    if matches!(detail, Detail::Connector { .. }) {
        items.push(limn::BreadcrumbItem::new("Your connectors").action("czp:crumb:tab"));
    }
    items.push(limn::BreadcrumbItem::new(name));
    limn::Breadcrumb::new("czp:crumbs", items, look::palette())
        .width(width)
        .render()
}

/// A tab's search, the folder its lists are read for, Discover and "+ Add ▾": the action
/// most useful now is filled, Discover while there is nothing yet and Add after.
fn toolbar(ui: &Ui, page: &Page, width: f32) -> Element {
    let search_width = (width - 330.0).clamp(140.0, 256.0);
    let (placeholder, search_id, value) = match page.tab.as_str() {
        SETTINGS_CONNECTORS if ui.connectors.discover => (
            "Search the MCP registry",
            view::SRV_SEARCH,
            ui.connectors.query.as_str(),
        ),
        SETTINGS_CONNECTORS => (
            "Search connectors",
            view::CZ_SEARCH,
            ui.customize_query.as_str(),
        ),
        SETTINGS_PLUGINS => (
            "Search plugins",
            view::CZ_SEARCH,
            ui.customize_query.as_str(),
        ),
        _ => (
            "Search skills",
            view::CZ_SEARCH,
            ui.customize_query.as_str(),
        ),
    };
    let search = limn::SearchField::new(
        search_id,
        value,
        placeholder,
        limn::ComponentPalette {
            surface: look::customize_card(),
            raised: look::customize_card(),
            border: look::settings_field_edge(),
            ..look::palette()
        },
    )
    .size(limn::ControlSize::Compact)
    .width(search_width)
    .render();
    let menu = |id: &str, label: &str, icon: Option<&'static str>, primary, entries| {
        customize::menu_button(
            customize::button(id, label, icon, true, primary),
            id,
            entries,
            ui.settings_menu.as_deref() == Some(id),
        )
    };
    let item = |id: String, label: &str, icon: &'static str| -> limn::DropdownMenuEntry {
        limn::DropdownMenuItem::new(id, label.to_owned())
            .icon(icons::of(icon))
            .into()
    };
    let empty = |agent_has: &dyn Fn(&AgentInfo) -> bool| !agents(ui).into_iter().any(agent_has);
    let mut tools = vec![search];
    match page.tab.as_str() {
        SETTINGS_CONNECTORS => {
            let none = empty(&|a| {
                (ui.connectors.listed.get(&a.id))
                    .and_then(|l| l.servers.as_ref())
                    .is_some_and(|s| !s.is_empty())
            });
            let first = agents(ui)
                .first()
                .map_or(ui.agent.clone(), |a| a.id.clone());
            tools.push(customize::button(
                "srv:tab:discover",
                "Discover",
                Some(icons::DISCOVER),
                false,
                none,
            ));
            tools.push(menu(
                "cz:add:connectors",
                "Add",
                Some(icons::ADD),
                !none,
                vec![item(
                    format!("srv:add:{first}"),
                    "Add custom connector\u{2026}",
                    icons::CONNECTORS,
                )],
            ));
        }
        SETTINGS_PLUGINS => {
            let none =
                empty(&|a| (ui.extensions.get(&a.id)).is_some_and(|k| !k.list.plugins.is_empty()));
            let first = agents(ui)
                .first()
                .map_or(ui.agent.clone(), |a| a.id.clone());
            tools.push(customize::folder_chip(
                view::PICK_FOLDER,
                &view::folder_name(&crate::customize_folder(ui)),
            ));
            tools.push(customize::button(
                &format!("cz:discover:{first}"),
                "Discover",
                Some(icons::DISCOVER),
                false,
                none,
            ));
            tools.push(menu(
                "cz:add:plugins",
                "Add",
                Some(icons::ADD),
                !none,
                vec![
                    item(
                        format!("cz:add-marketplace:{first}"),
                        "Add marketplace\u{2026}",
                        icons::STOREFRONT,
                    ),
                    item(
                        format!("cz:manage:{first}"),
                        "Manage marketplaces",
                        icons::LIST,
                    ),
                ],
            ));
        }
        _ => {
            let mut sources: Vec<String> = vec![];
            for agent in agents(ui) {
                for skill in ui
                    .extensions
                    .get(&agent.id)
                    .into_iter()
                    .flat_map(|k| &k.list.skills)
                {
                    if !sources.contains(&skill.source) {
                        sources.push(skill.source.clone());
                    }
                }
            }
            let mut order = vec![
                limn::DropdownMenuItem::new("czp:sort:source", "Where they come from")
                    .radio(!page.by_name)
                    .into(),
                limn::DropdownMenuItem::new("czp:sort:name", "Name")
                    .radio(page.by_name)
                    .into(),
                limn::DropdownMenuEntry::Separator,
                limn::DropdownMenuItem::new("czp:source:", "From anywhere")
                    .radio(page.source.is_none())
                    .into(),
            ];
            order.extend(sources.iter().map(|s| {
                limn::DropdownMenuItem::new(format!("czp:source:{s}"), format!("From {s}"))
                    .radio(page.source.as_ref() == Some(s))
                    .into()
            }));
            tools.push(customize::folder_chip(
                view::PICK_FOLDER,
                &view::folder_name(&crate::customize_folder(ui)),
            ));
            tools.push(menu("cz:sort", "Sort", Some(icons::SORT), false, order));
            tools.push(menu(
                "cz:add:skills",
                "Add",
                Some(icons::ADD),
                true,
                vec![
                    item(
                        "czp:skill-create".into(),
                        "Create with an agent",
                        icons::CHAT,
                    ),
                    item(
                        "glob:skill-new".into(),
                        "Write instructions\u{2026}",
                        icons::PENCIL,
                    ),
                    item(
                        file_action("upload", ""),
                        "Upload a file\u{2026}",
                        icons::UPLOAD,
                    ),
                    item(
                        "glob:skill-import".into(),
                        "Import a folder\u{2026}",
                        icons::FOLDER,
                    ),
                ],
            ));
        }
    }
    Element::row(tools.into_iter().map(|t| t.centered_y()).collect())
        .gap(8.0)
        .wrap()
        .size(Some(width), None)
}

// Connectors.

/// Your connectors (the "All agents" ones, then each agent's, as a list with their type and
/// status, filtered and searched) or Discover (the MCP registry's, as cards).
fn connectors(ui: &Ui, page: &Page, width: f32) -> Element {
    let c = &ui.connectors;
    let mut column = vec![customize::tabs(&[
        ("srv:tab:yours", "Your connectors", !c.discover, None),
        ("srv:tab:discover", "Discover", c.discover, None),
    ])];
    if c.discover {
        column.push(discover(ui, width));
        return Element::column(column).gap(12.0);
    }
    column.push(customize::tabs(&[
        ("czp:filter:all", "All", page.filter == Filter::All, None),
        (
            "czp:filter:connected",
            "Connected",
            page.filter == Filter::Connected,
            None,
        ),
        (
            "czp:filter:not",
            "Not connected",
            page.filter == Filter::NotConnected,
            None,
        ),
    ]));
    let wrong: usize = attention(ui).iter().map(|(_, n)| n.len()).sum();
    if wrong > 0 {
        let words = if wrong == 1 {
            "1 of your connectors needs attention".to_owned()
        } else {
            format!("{wrong} of your connectors need attention")
        };
        column.push(customize::attention(
            &words,
            customize::button(
                "czp:reconnect-all",
                "Reconnect all",
                Some(icons::RELOAD),
                false,
                false,
            ),
            width,
        ));
    }
    if page.layer.is_none() {
        column.extend(view::global_section(ui, false, width));
    }
    let query = ui.customize_query.trim().to_lowercase();
    for agent in agents(ui) {
        column.push(Element::spacer().size(None, Some(8.0)));
        let listed = c.listed.get(&agent.id);
        let servers = listed.and_then(|l| l.servers.as_ref());
        column.push(customize::agent_heading(
            icons::agent(agent.icon.as_deref()),
            &agent.name,
            servers.map(Vec::len),
            width,
        ));
        if !agent.customize.connectors {
            column.push(customize::empty(
                icons::CONNECTORS,
                &format!("{} has no connectors quill can manage.", agent.name),
                width,
            ));
            continue;
        }
        let Some(servers) = servers else {
            column.push(customize::loading(
                "srv:loading",
                "Checking connectors\u{2026}",
            ));
            continue;
        };
        let listed = listed.expect("listed");
        if listed.working {
            column.push(customize::loading("srv:loading", "Reading again\u{2026}"));
        }
        if let Some(e) = &listed.error {
            column.push(
                text(format!("Couldn\u{2019}t do that. {e}"))
                    .font_size(BODY)
                    .foreground(look::failed()),
            );
        }
        if servers.is_empty() {
            column.push(customize::hero(
                "Add your first connectors",
                &format!(
                    "Connectors give {} tools and data from other apps and services.",
                    agent.name
                ),
                Some(customize::button(
                    "srv:tab:discover",
                    "Discover connectors",
                    Some(icons::DISCOVER),
                    false,
                    true,
                )),
                width,
            ));
            continue;
        }
        let shown: Vec<&quill_model::ConfiguredServer> = (servers.iter())
            .filter(|s| match page.filter {
                Filter::All => true,
                Filter::Connected => connected(s),
                Filter::NotConnected => !connected(s),
            })
            .filter(|s| query.is_empty() || s.name.to_lowercase().contains(&query))
            .collect();
        if shown.is_empty() {
            column.push(no_results(width));
            continue;
        }
        column.push(customize::table_head("Connector", "Type", "Status", width));
        for server in shown {
            column.push(connector_row(ui, &agent.id, server, width));
        }
    }
    Element::column(column).gap(8.0)
}

/// Connected, or set up and on where the agent's list does not check.
fn connected(server: &quill_model::ConfiguredServer) -> bool {
    server.status == McpStatus::Connected
}

fn no_results(width: f32) -> Element {
    Element::column(vec![
        Element::icon(icons::of(icons::SEARCH))
            .foreground(look::muted())
            .size(Some(32.0), Some(32.0))
            .centered_x(),
        text("No connectors match your search")
            .font_size(BODY)
            .centered_x(),
        muted("Try a different term, or browse Discover.")
            .font_size(SMALL)
            .centered_x(),
    ])
    .gap(8.0)
    .padding(24.0)
    .size(Some(width), None)
}

/// A status in the agent's words for people, with its icon and ink.
fn status_of(status: McpStatus, checked: bool) -> (&'static str, &'static str, limn::Color) {
    match (status, checked) {
        (McpStatus::Connected, true) => ("Connected", icons::CHECK_CIRCLE, look::palette().text),
        (McpStatus::Connected, false) => ("On", icons::CHECK_CIRCLE, look::muted()),
        (McpStatus::NeedsAuth, _) => ("Needs sign-in", icons::WARNING, look::awaiting()),
        (McpStatus::Pending, _) => ("Pending approval", icons::HOURGLASS, look::muted()),
        (McpStatus::Disabled, _) => ("Not connected", icons::BLOCKED, look::muted()),
        (McpStatus::Failed, _) => ("Failed to connect", icons::ERROR, look::failed()),
    }
}

fn picture(ui: &Ui, key: &str, fallback: &'static str, side: f32) -> Element {
    match view::logo_picture(ui, key) {
        Some(logo) => Element::raster_image(logo.clone()).size(Some(side), Some(side)),
        None => Element::icon(icons::of(fallback))
            .foreground(look::palette().text)
            .size(Some(side * 0.67), Some(side * 0.67)),
    }
}

fn server_logo(ui: &Ui, name: &str, label: &str) -> String {
    format!("mcp:{}", crate::server_logo_name(ui, name, label))
}

fn connector_row(
    ui: &Ui,
    agent: &str,
    server: &quill_model::ConfiguredServer,
    width: f32,
) -> Element {
    let (words, icon, ink) = status_of(server.status, server.checked);
    let kind = if server.target.starts_with("http") {
        "Remote"
    } else {
        "Local"
    };
    let more = matches!(server.source, McpSource::Other).then(|| {
        let menu = format!("cz:more:srv:{agent}:{}", server.name);
        let entries = vec![limn::DropdownMenuItem::new(
            format!("srv:remove:{agent}:{}", server.name),
            "Remove",
        )
        .icon(icons::of(icons::TRASH))
        .destructive(look::failed())
        .into()];
        customize::more(
            &menu,
            entries,
            ui.settings_menu.as_deref() == Some(menu.as_str()),
        )
    });
    customize::table_row(
        &format!("czp:open:srv:{agent}:{}", server.name),
        picture(
            ui,
            &server_logo(ui, &server.name, &server.label),
            icons::PLUG,
            16.0,
        ),
        &server.name,
        kind,
        customize::status(icon, words, ink),
        more,
        width,
    )
}

/// The MCP registry's servers as Discover's cards, two or three to a row by the width.
fn discover(ui: &Ui, width: f32) -> Element {
    let c = &ui.connectors;
    let mut column = vec![muted(
        "Servers listed in the public MCP registry. Make sure you trust one before you add it: it runs on this computer.",
    )
    .font_size(BODY)];
    let offers = match &c.found {
        None => {
            column.push(customize::loading(
                "srv:finding",
                "Searching the MCP registry\u{2026}",
            ));
            return Element::column(column).gap(8.0);
        }
        Some((_, _, Some(error))) => {
            column.push(
                text(format!("Couldn\u{2019}t reach the MCP registry. {error}"))
                    .font_size(BODY)
                    .foreground(look::failed()),
            );
            return Element::column(column).gap(8.0);
        }
        Some((_, offers, None)) => offers,
    };
    if offers.is_empty() {
        column.push(no_results(width));
        return Element::column(column).gap(8.0);
    }
    let per_row = (((width + 12.0) / (280.0 + 12.0)).floor() as usize).clamp(1, 3);
    let card_width = ((width - 12.0 * (per_row as f32 - 1.0)) / per_row as f32).floor();
    let cards: Vec<Element> = (offers.iter().enumerate())
        .map(|(at, offer)| {
            let name = offer.title.clone().unwrap_or_else(|| offer.name.clone());
            let author = offer.name.split('/').next().unwrap_or_default();
            let kind = if offer.remote { "Remote" } else { "Local" };
            let action = icon_tool(
                &format!("srv:offer:{at}"),
                icons::of(icons::ADD),
                "Add",
                28.0,
                look::palette(),
            );
            customize::discover_card(
                Element::icon(icons::of(icons::CONNECTORS))
                    .foreground(look::palette().text)
                    .size(Some(18.0), Some(18.0)),
                &name,
                &format!("{author} \u{2022} {kind} \u{2022} v{}", offer.version),
                &offer.description,
                false,
                action,
                card_width,
            )
        })
        .collect();
    let mut cards = cards.into_iter();
    loop {
        let row: Vec<Element> = cards.by_ref().take(per_row).collect();
        if row.is_empty() {
            break;
        }
        column.push(Element::row(row).gap(12.0));
    }
    Element::column(column).gap(12.0)
}

/// A detail page's head: its picture on a 36 tile, its name in 18 bold over `under`, and what
/// goes at its end.
fn detail_head(
    picture: Element,
    name: &str,
    under: Element,
    end: Vec<Element>,
    width: f32,
) -> Element {
    let tile = Element::row(vec![picture.centered().flex(1.0)])
        .size(Some(36.0), Some(36.0))
        .background(look::customize_tile())
        .radius(8.0)
        .centered_y();
    let mut row = vec![
        tile,
        Element::column(vec![
            Element::truncated_text(name.to_owned())
                .font_family(FontFamily::SansBold)
                .font_size(18.0)
                .foreground(look::palette().text),
            under,
        ])
        .gap(2.0)
        .flex(1.0)
        .centered_y(),
    ];
    row.extend(end.into_iter().map(|e| e.centered_y()));
    Element::row(row).gap(12.0).size(Some(width), Some(48.0))
}

fn section_title(words: &str) -> Element {
    text(words)
        .font_family(FontFamily::SansBold)
        .font_size(15.0)
}

/// Who put it there, in the person's words.
fn added_by(source: McpSource, agent: &str) -> String {
    match source {
        McpSource::User => "You, for all your projects".into(),
        McpSource::Local => "You, for this project".into(),
        McpSource::Project => "This project's settings".into(),
        McpSource::Managed => "Your organization".into(),
        McpSource::Plugin => "A plugin".into(),
        McpSource::Account => "Your account".into(),
        McpSource::App => "quill, for its own tools".into(),
        McpSource::Global => "quill, for all agents".into(),
        McpSource::Other => format!("{agent}\u{2019}s settings"),
    }
}

/// What a connector can do, worked out from how it runs and what its tools say of themselves
/// (inferred: a server can do more than it says).
fn trust(remote: bool, tools: Option<&[McpTool]>) -> Vec<(&'static str, &'static str)> {
    let mut lines = vec![];
    if remote {
        lines.push((icons::GLOBE, "Can send data to other services"));
    } else {
        lines.push((icons::TERMINAL, "Can run code without asking"));
    }
    if let Some(tools) = tools.filter(|t| !t.is_empty()) {
        if tools.iter().all(|t| t.read_only) {
            lines.push((icons::EYE, "Only reads"));
        } else if tools.iter().any(|t| t.destructive) {
            lines.push((icons::TRASH, "Can change or delete things"));
        }
    }
    lines
}

/// A connector's page: its head, what it is and who added it, what it can do, and its tools
/// with when each may be used.
fn connector(store: &Store, ui: &Ui, agent: &str, name: &str, width: f32) -> Element {
    let configured = (ui.connectors.listed.get(agent))
        .and_then(|l| l.servers.as_ref())
        .and_then(|s| s.iter().find(|s| s.name == name));
    let global = (agent.is_empty())
        .then(|| ui.global.servers.iter().find(|s| s.name == name))
        .flatten();
    let (target, remote, source, label) = match (configured, global) {
        (Some(s), _) => (
            s.target.clone(),
            s.target.starts_with("http"),
            s.source,
            s.label.clone(),
        ),
        (None, Some(g)) => {
            let mut target = vec![g.target.clone()];
            target.extend(g.args.iter().cloned());
            (
                target.join(" "),
                g.remote,
                McpSource::Global,
                g.name.clone(),
            )
        }
        (None, None) => {
            return customize::empty(icons::CONNECTORS, "This connector is gone.", width);
        }
    };
    let agent_name = view::agent_word(ui, agent);
    let tools = tools_of(store, ui, agent, name);
    let (status, icon, ink) = match configured {
        Some(s) => status_of(s.status, s.checked),
        None => ("Added to each session", icons::GLOBE, look::muted()),
    };
    let mut end = vec![];
    let menu = format!("cz:more:detail:{agent}:{name}");
    let mut entries: Vec<limn::DropdownMenuEntry> = vec![];
    if agent.is_empty() {
        entries.push(
            limn::DropdownMenuItem::new(format!("glob:remove:server:{name}"), "Remove")
                .icon(icons::of(icons::TRASH))
                .destructive(look::failed())
                .into(),
        );
    } else if matches!(source, McpSource::Other) {
        entries.push(
            limn::DropdownMenuItem::new(format!("srv:remove:{agent}:{name}"), "Remove")
                .icon(icons::of(icons::TRASH))
                .destructive(look::failed())
                .into(),
        );
    }
    entries.insert(
        0,
        limn::DropdownMenuItem::new("czp:tools-again", "Read its tools again")
            .icon(icons::of(icons::RELOAD))
            .into(),
    );
    // On and off is the agent's own, per session (/mcp); quill's layer is per agent.
    let switch = limn::Switch::new("czp:connector-switch", true, look::palette())
        .size(limn::ControlSize::Dense)
        .disabled()
        .render();
    let why = if agent.is_empty() {
        "Turn it on or off for each agent from its row's \u{22ef}.".to_owned()
    } else {
        format!("{agent_name} turns a connector on or off in a session, with /mcp.")
    };
    end.push(limn::Tooltip::new(switch, "czp:connector-switch:why", why, look::palette()).render());
    end.push(customize::more(
        &menu,
        entries,
        ui.settings_menu.as_deref() == Some(menu.as_str()),
    ));
    let mut column = vec![detail_head(
        picture(ui, &server_logo(ui, name, &label), icons::PLUG, 24.0),
        name,
        customize::status(icon, status, ink),
        end,
        width,
    )];
    let used_by: Vec<String> = (ui.agents.iter())
        .filter(|a| view::server_on_in(ui, &a.id, &label) || agent.is_empty())
        .map(|a| a.name.clone())
        .collect();
    let facts = [
        (
            icons::LAPTOP,
            if remote {
                "Remote: reached at an address".to_owned()
            } else {
                "Local: a program on this computer".to_owned()
            },
            None,
        ),
        (icons::TERMINAL, target, None),
        (
            icons::AGENTS,
            format!("Added by {}", added_by(source, &agent_name)),
            None,
        ),
        (
            icons::CHAT,
            if used_by.is_empty() {
                "Not used yet".to_owned()
            } else {
                format!("Used by {}", used_by.join(", "))
            },
            None,
        ),
    ];
    column.extend((facts.iter().enumerate()).map(|(at, (icon, words, more))| {
        customize::fact(&format!("czp:fact:{at}"), icon, words, *more)
    }));
    column.push(Element::spacer().size(None, Some(8.0)));
    column.push(section_title("What it can do"));
    let worked_out = "Worked out from how it runs and what its tools say about themselves: a server can do more than it says.";
    column.extend(
        (trust(remote, tools.as_deref()).into_iter().enumerate()).map(|(at, (icon, words))| {
            customize::fact(&format!("czp:trust:{at}"), icon, words, Some(worked_out))
        }),
    );
    column.push(Element::spacer().size(None, Some(8.0)));
    column.push(permissions(
        ui,
        agent,
        name,
        &agent_name,
        tools.as_deref(),
        width,
    ));
    Element::column(column).gap(8.0)
}

/// Tool permissions: a group for what only reads, what changes or deletes and the rest, each
/// with its count and a menu that sets all of it, and each tool with its three choices.
fn permissions(
    ui: &Ui,
    agent: &str,
    server: &str,
    agent_name: &str,
    tools: Option<&[McpTool]>,
    width: f32,
) -> Element {
    let who = if agent.is_empty() {
        "your agents are"
    } else {
        "it is"
    };
    let mut column = vec![
        section_title("Tool permissions"),
        muted(format!(
            "Choose when {who} allowed to use these tools. It applies to new sessions; until you choose, {} decides.",
            if agent.is_empty() { "each agent" } else { agent_name }
        ))
        .font_size(SMALL),
    ];
    let offered = policies(ui, agent);
    if offered.is_empty() {
        column.push(customize::fact(
            "czp:no-policies",
            icons::LOCK,
            &format!("{agent_name} decides by itself when to use a connector\u{2019}s tools."),
            None,
        ));
        return Element::column(column).gap(8.0);
    }
    let asked = if agent.is_empty() { &ui.agent } else { agent };
    let Some(tools) = tools else {
        let error = ui.connectors.tools.get(asked).and_then(|t| t.error.clone());
        column.push(match error {
            Some(error) => Element::row(vec![
                muted(error).font_size(BODY).centered_y(),
                customize::button(
                    "czp:tools-again",
                    "Try again",
                    Some(icons::RELOAD),
                    false,
                    false,
                ),
            ])
            .gap(12.0),
            None => customize::loading("czp:tools:loading", "Looking for its tools\u{2026}"),
        });
        return Element::column(column).gap(8.0);
    };
    if tools.is_empty() {
        column.push(customize::empty(
            icons::TOOL,
            "It has no tools, or it hasn\u{2019}t connected yet.",
            width,
        ));
        return Element::column(column).gap(8.0);
    }
    let off = |p: ToolPolicy| -> Option<String> {
        (!offered.contains(&p)).then(|| format!("{agent_name} can\u{2019}t say this for one tool"))
    };
    let offs = ToolPolicy::ALL.map(off);
    for (key, title) in GROUPS {
        let group: Vec<&McpTool> = tools.iter().filter(|t| group_of(t) == key).collect();
        if group.is_empty() {
            continue;
        }
        let menu = format!("cz:more:group:{key}");
        let mut entries: Vec<limn::DropdownMenuEntry> = (ToolPolicy::ALL.iter())
            .filter(|p| offered.contains(p))
            .map(|p| {
                let (label, icon) = match p {
                    ToolPolicy::Allow => ("Always allow", icons::CHECK_CIRCLE),
                    ToolPolicy::Ask => ("Needs approval", icons::HAND),
                    ToolPolicy::Block => ("Blocked", icons::BLOCKED),
                };
                limn::DropdownMenuItem::new(format!("czp:group:{}:{key}", policy_word(*p)), label)
                    .icon(icons::of(icon))
                    .into()
            })
            .collect();
        entries.push(limn::DropdownMenuEntry::Separator);
        entries.push(
            limn::DropdownMenuItem::new(format!("czp:group:default:{key}"), "As the agent decides")
                .into(),
        );
        column.push(Element::spacer().size(None, Some(4.0)));
        column.push(customize::section(
            title,
            Some(group.len()),
            Some(customize::more(
                &menu,
                entries,
                ui.settings_menu.as_deref() == Some(menu.as_str()),
            )),
            width,
        ));
        for tool in group {
            let chosen = policy(ui, agent, server, &tool.name);
            let ids = ToolPolicy::ALL.map(|p| format!("czp:tool:{}:{}", policy_word(p), tool.name));
            let control = customize::policy_control(
                [&ids[0], &ids[1], &ids[2]],
                chosen.and_then(|c| ToolPolicy::ALL.iter().position(|p| *p == c)),
                [offs[0].as_deref(), offs[1].as_deref(), offs[2].as_deref()],
            );
            // A blocked tool is drawn faint.
            let ink = if chosen == Some(ToolPolicy::Block) {
                look::customize_edge()
            } else {
                look::muted()
            };
            let mut copy = vec![Element::truncated_text(tool.name.clone())
                .font_size(BODY)
                .foreground(ink)];
            if let Some(about) = tool.description.as_ref().filter(|d| !d.is_empty()) {
                copy.push(
                    Element::truncated_text(about.lines().next().unwrap_or_default().to_owned())
                        .font_size(12.0)
                        .foreground(look::customize_edge()),
                );
            }
            column.push(Element::column(vec![
                Element::row(vec![
                    hgap(24.0),
                    Element::column(copy).gap(2.0).flex(1.0).centered_y(),
                    control.centered_y(),
                ])
                .gap(12.0)
                .size(Some(width), Some(56.0)),
                Element::spacer()
                    .background(look::customize_line())
                    .size(Some(width), Some(1.0)),
            ]));
        }
    }
    Element::column(column).gap(8.0)
}

// Skills and plugins.

/// A skill's page: its head with its switch, what it is, and what can be done with it: an
/// "All agents" skill is quill's own file, saved, replaced or deleted here; an agent's stays
/// in that agent's folder.
fn skill(ui: &Ui, agent: &str, name: &str, width: f32) -> Element {
    let mut column = vec![];
    if agent.is_empty() {
        let Some(skill) = ui.global.skills.iter().find(|s| s.name == name) else {
            return customize::empty(icons::SKILL, "This skill is gone.", width);
        };
        let menu = format!("cz:more:skill-detail:{name}");
        let entries = vec![
            limn::DropdownMenuItem::new(file_action("download", name), "Download")
                .icon(icons::of(icons::DOWNLOAD))
                .into(),
            limn::DropdownMenuItem::new(file_action("replace", name), "Replace\u{2026}")
                .icon(icons::of(icons::UPLOAD))
                .into(),
            limn::DropdownMenuEntry::Separator,
            limn::DropdownMenuItem::new(format!("czp:skill-delete:{name}"), "Delete")
                .icon(icons::of(icons::TRASH))
                .destructive(look::failed())
                .into(),
        ];
        let off_in: Vec<&str> = (ui.agents.iter())
            .filter(|a| skill.except.contains(&a.id))
            .map(|a| a.name.as_str())
            .collect();
        let mut end = vec![];
        if let Some(layer) = ui.customize.as_ref().and_then(|p| p.layer.clone()) {
            end.push(
                limn::Switch::new(
                    format!("czp:skill-switch::{name}"),
                    !skill.except.contains(&layer),
                    look::palette(),
                )
                .size(limn::ControlSize::Dense)
                .render(),
            );
        }
        end.push(customize::more(
            &menu,
            entries,
            ui.settings_menu.as_deref() == Some(menu.as_str()),
        ));
        column.push(detail_head(
            Element::icon(icons::of(icons::SKILL)).size(Some(18.0), Some(18.0)),
            name,
            muted("For all agents").font_size(SMALL),
            end,
            width,
        ));
        column.push(text(skill.description.clone()).font_size(BODY));
        column.push(customize::fact(
            "czp:fact:by",
            icons::AGENTS,
            "Added by you, kept by quill",
            None,
        ));
        if !off_in.is_empty() {
            column.push(customize::fact(
                "czp:fact:off",
                icons::BLOCKED,
                &format!("Off in {}", off_in.join(", ")),
                None,
            ));
        }
        column.push(customize::fact("czp:trust:0", icons::FILE, "Only adds instructions", Some("A skill is words the agent reads; scripts beside it run only when the agent runs them, with its own permissions.")));
        return Element::column(column).gap(8.0);
    }
    let known = ui.extensions.get(agent);
    let Some(skill) = known.and_then(|k| k.list.skills.iter().find(|s| s.name == name)) else {
        return customize::empty(icons::SKILL, "This skill is gone.", width);
    };
    let known = known.expect("known");
    let agent_name = view::agent_word(ui, agent);
    let first = known.list.skill_states.first().map(|(s, _)| s.as_str());
    let state = (known.list.skill_states.iter())
        .find(|(s, _)| *s == skill.state)
        .map_or(skill.state.clone(), |(_, label)| label.clone());
    let switch = limn::Switch::new(
        format!("czp:skill-switch:{agent}:{name}"),
        Some(skill.state.as_str()) == first,
        look::palette(),
    )
    .size(limn::ControlSize::Dense)
    .enabled(skill.locked.is_none() && known.list.skill_states.len() > 1)
    .render();
    column.push(detail_head(
        Element::icon(icons::of(icons::SKILL)).size(Some(18.0), Some(18.0)),
        name,
        muted(state).font_size(SMALL),
        vec![switch],
        width,
    ));
    column.push(text(skill.description.clone()).font_size(BODY));
    column.push(customize::fact(
        "czp:fact:from",
        icons::FOLDER,
        &format!("From {}, in {agent_name}", skill.source),
        None,
    ));
    if let Some(why) = &skill.locked {
        column.push(customize::fact("czp:fact:locked", icons::LOCK, why, None));
    }
    column.push(customize::fact(
        "czp:fact:files",
        icons::INFO,
        &format!("{agent_name} keeps this skill in its own folder: download, replace and delete it there."),
        None,
    ));
    column.push(customize::fact("czp:trust:0", icons::FILE, "Only adds instructions", Some("A skill is words the agent reads; scripts beside it run only when the agent runs them, with its own permissions.")));
    Element::column(column).gap(8.0)
}

/// A plugin's page: what its marketplace says of it, who it comes from, what it can do as
/// far as quill sees, and the connectors and skills it brings.
fn plugin(ui: &Ui, agent: &str, id: &str, width: f32) -> Element {
    let known = ui.extensions.get(agent);
    let Some(plugin) = known.and_then(|k| k.list.plugins.iter().find(|p| p.id == id)) else {
        return customize::empty(icons::PLUGIN, "This plugin is gone.", width);
    };
    let known = known.expect("known");
    let offer = known.list.offers.iter().find(|o| o.id == plugin.id);
    let market = plugin.id.split_once('@').map_or("", |(_, m)| m);
    let mut under = vec![];
    under.extend(plugin.version.as_ref().map(|v| format!("v{v}")));
    under.push(if plugin.enabled { "On" } else { "Off" }.to_owned());
    let menu = format!("cz:more:{agent}:{}", plugin.id);
    let mut entries: Vec<limn::DropdownMenuEntry> = vec![];
    if known.list.kinds.update_plugins {
        entries.push(
            limn::DropdownMenuItem::new(format!("ext:update:{agent}:{}", plugin.id), "Update")
                .icon(icons::of(icons::RELOAD))
                .into(),
        );
    }
    if known.list.kinds.switch_plugins {
        let label = if plugin.enabled {
            "Turn off"
        } else {
            "Turn on"
        };
        entries.push(
            limn::DropdownMenuItem::new(format!("ext:switch:{agent}:{}", plugin.id), label)
                .icon(icons::of(icons::PLUGIN))
                .into(),
        );
    }
    entries.push(
        limn::DropdownMenuItem::new(format!("ext:remove:{agent}:{}", plugin.id), "Remove")
            .icon(icons::of(icons::TRASH))
            .destructive(look::failed())
            .into(),
    );
    let mut column = vec![detail_head(
        picture(ui, &format!("plugin:{}", plugin.name), icons::PLUGIN, 24.0),
        &plugin.name,
        muted(under.join(" \u{b7} ")).font_size(SMALL),
        vec![customize::more(
            &menu,
            entries,
            ui.settings_menu.as_deref() == Some(menu.as_str()),
        )],
        width,
    )];
    if let Some(offer) = offer {
        column.push(text(offer.description.clone()).font_size(BODY));
    }
    column.push(customize::fact(
        "czp:fact:market",
        icons::STOREFRONT,
        &format!("From the {market} marketplace, installed {}", plugin.scope),
        None,
    ));
    // What it brings, as the agent's lists name it: its connectors and its skills.
    let servers: Vec<&quill_model::ConfiguredServer> = (ui.connectors.listed.get(agent))
        .and_then(|l| l.servers.as_ref())
        .into_iter()
        .flatten()
        .filter(|s| s.source == McpSource::Plugin && s.name.contains(&plugin.name))
        .collect();
    let prefix = format!("{}:", plugin.name);
    let skills: Vec<&quill_model::Skill> = (known.list.skills.iter())
        .filter(|s| s.name.starts_with(&prefix))
        .collect();
    column.push(Element::spacer().size(None, Some(8.0)));
    column.push(section_title("What it can do"));
    let worked_out = "Worked out from the connectors and skills quill sees it bring; hooks and commands it may carry aren\u{2019}t listed, so it may do more.";
    let mut lines = vec![];
    if servers.is_empty() && !skills.is_empty() {
        lines.push((icons::FILE, "Only adds instructions"));
    }
    if servers.iter().any(|s| !s.target.starts_with("http")) {
        lines.push((icons::TERMINAL, "Can run code without asking"));
    }
    if servers.iter().any(|s| s.target.starts_with("http")) {
        lines.push((icons::GLOBE, "Can send data to other services"));
    }
    if lines.is_empty() {
        lines.push((icons::INFO, "quill can\u{2019}t see what it brings yet"));
    }
    column.extend((lines.into_iter().enumerate()).map(|(at, (icon, words))| {
        customize::fact(&format!("czp:trust:{at}"), icon, words, Some(worked_out))
    }));
    column.push(Element::spacer().size(None, Some(8.0)));
    column.push(section_title("Connectors & tools"));
    if servers.is_empty() && skills.is_empty() {
        column.push(muted("None that quill can see.").font_size(SMALL));
    }
    for server in servers {
        column.push(connector_row(ui, agent, server, width));
    }
    for skill in skills {
        column.push(customize::fact(
            &format!("czp:skill:{}", skill.name),
            icons::SKILL,
            &skill.name,
            (!skill.description.is_empty()).then_some(skill.description.as_str()),
        ));
    }
    Element::column(column).gap(8.0)
}
