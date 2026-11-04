//! An agent's model providers (docs/ui.md "Proveedores"): the ones connected, on the agent's
//! page in Settings, and "Add a provider", a dialog that finds one in the agent's list and
//! connects it with a key, or says how its own terminal connects it. What the agent offers is
//! its adapter's (`AgentInfo::providers`): nothing here names an agent or a provider.

use crate::Ui;
use limn::{Component, Element};
use newen_components::pieces::{hgap, keyed_button, muted, text};
use newen_components::{icons, look};
use newen_model::{Event, Intent, Provider};

/// An agent's providers as last listed, and a change under way.
#[derive(Clone, Debug, Default)]
pub struct Providers {
    /// `None` until its program lists them.
    pub list: Option<Vec<Provider>>,
    pub error: Option<String>,
    /// The provider being connected or disconnected.
    pub working: Option<String>,
}

/// "Add a provider", while open: for which agent, what the search holds, the provider chosen
/// and the key typed for it.
#[derive(Clone, Debug, Default)]
pub struct Adding {
    pub agent: String,
    pub query: String,
    pub chosen: Option<String>,
    pub key: String,
}

const ADD: &str = "prov:add:";
const CLOSE: &str = "prov:close";
const PICK: &str = "prov:pick:";
const BACK: &str = "prov:back";
const CONNECT: &str = "prov:connect";
const DISCONNECT: &str = "prov:disconnect:";
pub const SEARCH: &str = "prov:search";
pub const KEY: &str = "prov:key";
/// Connect, also what Enter in the key's field does.
pub const CONNECT_ID: &str = CONNECT;

/// The id that opens "Add a provider" for `agent`.
pub fn add_id(agent: &str) -> String {
    format!("{ADD}{agent}")
}

/// Its providers are asked for when the agent's page shows and they are not known yet.
pub(crate) fn wanted(ui: &mut Ui) -> Vec<Intent> {
    let Some(agent) = (ui.settings_page.as_deref())
        .and_then(|p| p.strip_prefix("agent:"))
        .filter(|a| ui.agents.iter().any(|i| i.id == *a && i.providers))
        .map(str::to_owned)
    else {
        return vec![];
    };
    if ui.providers.contains_key(&agent) {
        return vec![];
    }
    ui.providers.insert(agent.clone(), Providers::default());
    vec![Intent::Providers { agent }]
}

/// The list, after asking or a change: a provider just connected closes the dialog.
pub(crate) fn heard(ui: &mut Ui, event: &Event) {
    let Event::Providers {
        agent,
        providers,
        error,
    } = event
    else {
        return;
    };
    let connected = |id: &str| (providers.iter()).any(|p| p.id == id && !p.connected.is_empty());
    let state = ui.providers.entry(agent.clone()).or_default();
    let changed = state.working.take();
    state.list = Some(providers.clone());
    state.error = error.clone();
    if let Some(adding) = ui.adding_provider.as_mut().filter(|a| a.agent == *agent) {
        match (&adding.chosen, error) {
            (Some(chosen), None) if changed.as_deref() == Some(chosen) && connected(chosen) => {
                ui.adding_provider = None;
            }
            // A key that did not take: it stays to correct, and the error shows.
            (Some(chosen), None) if changed.as_deref() == Some(chosen) => {
                state.error = Some("It did not connect: check the key.".into());
            }
            _ => {}
        }
    }
}

pub(crate) fn input(ui: &mut Ui, id: &str, value: &str) -> bool {
    let Some(adding) = ui.adding_provider.as_mut() else {
        return false;
    };
    match id {
        SEARCH => adding.query = value.to_owned(),
        KEY => adding.key = value.to_owned(),
        _ => return false,
    }
    true
}

pub(crate) fn clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    if let Some(agent) = id.strip_prefix(ADD) {
        ui.adding_provider = Some(Adding {
            agent: agent.to_owned(),
            ..Adding::default()
        });
        if let Some(state) = ui.providers.get_mut(agent) {
            state.error = None;
        }
        // Its list may be older than what its program holds now.
        return Some(vec![Intent::Providers {
            agent: agent.to_owned(),
        }]);
    }
    if let Some(rest) = id.strip_prefix(DISCONNECT) {
        let (agent, provider) = rest.split_once(':')?;
        let state = ui.providers.get_mut(agent)?;
        let credentials = (state.list.iter().flatten())
            .find(|p| p.id == provider)?
            .connected
            .clone();
        state.working = Some(provider.to_owned());
        state.error = None;
        return Some(vec![Intent::DisconnectProvider {
            agent: agent.to_owned(),
            provider: provider.to_owned(),
            credentials,
        }]);
    }
    let adding = ui.adding_provider.as_mut()?;
    match id {
        CLOSE => ui.adding_provider = None,
        BACK => {
            adding.chosen = None;
            adding.key.clear();
        }
        CONNECT => {
            let provider = adding.chosen.clone()?;
            let key = adding.key.trim().to_owned();
            if key.is_empty() {
                return Some(vec![]);
            }
            let agent = adding.agent.clone();
            let state = ui.providers.entry(agent.clone()).or_default();
            state.working = Some(provider.clone());
            state.error = None;
            return Some(vec![Intent::ConnectProvider {
                agent,
                provider,
                key,
            }]);
        }
        _ => {
            let provider = id.strip_prefix(PICK)?;
            adding.chosen = Some(provider.to_owned());
            adding.key.clear();
        }
    }
    Some(vec![])
}

/// A provider's logo, looked up as connectors' are (`provider:{name}`), else its initial.
fn avatar(ui: &Ui, provider: &Provider, side: f32) -> Element {
    match crate::view::logo_picture(ui, &logo_key(provider)) {
        Some(logo) => crate::view::logo_tile(logo, side),
        None => limn::Avatar::new(&provider.name, look::palette())
            .size(side)
            .render(),
    }
}

pub(crate) fn logo_key(provider: &Provider) -> String {
    format!("provider:{}", provider.name)
}

/// How it stands, in a line: connected by Newen, by a variable, or how it connects.
fn standing(provider: &Provider) -> String {
    if !provider.connected.is_empty() {
        "Connected".into()
    } else if let Some(variable) = &provider.env_set {
        format!("Connected by {variable}")
    } else if provider.key {
        "API key".into()
    } else if provider.terminal.is_some() {
        "Signs in from a terminal".into()
    } else {
        String::new()
    }
}

/// The agent page's "Providers" row: "Add a provider", and under it the ones connected, each
/// with Disconnect when Newen can take it away.
pub(crate) fn setting(
    ui: &Ui,
    agent: &newen_model::AgentInfo,
) -> newen_components::settings::Setting {
    let palette = look::palette();
    let state = ui.providers.get(&agent.id);
    let mut rows = vec![];
    match state.and_then(|s| s.list.as_ref()) {
        None => rows.push(
            Element::row(vec![
                limn::Spinner::new("prov:loading", look::muted())
                    .size(14.0)
                    .render(),
                muted("Looking for its providers\u{2026}").centered_y(),
            ])
            .gap(8.0),
        ),
        Some(list) => {
            let connected: Vec<&Provider> = (list.iter())
                .filter(|p| !p.connected.is_empty() || p.env_set.is_some())
                .collect();
            if connected.is_empty() {
                rows.push(muted(format!(
                    "No provider connected: {} runs only the models it offers by itself.",
                    agent.name
                )));
            }
            for provider in connected {
                let working = state.and_then(|s| s.working.as_deref()) == Some(&provider.id);
                let action = if working {
                    limn::Spinner::new(format!("prov:wait:{}", provider.id), look::muted())
                        .size(14.0)
                        .render()
                } else if provider.connected.is_empty() {
                    hgap(0.0)
                } else {
                    keyed_button(
                        &format!("{DISCONNECT}{}:{}", agent.id, provider.id),
                        "Disconnect",
                        "",
                        false,
                        true,
                        palette,
                    )
                };
                rows.push(
                    Element::row(vec![
                        avatar(ui, provider, 28.0).centered_y(),
                        Element::column(vec![
                            text(provider.name.clone()),
                            muted(standing(provider)).font_size(look::SMALL),
                        ])
                        .gap(2.0)
                        .flex(1.0)
                        .centered_y(),
                        action.centered_y(),
                    ])
                    .gap(12.0),
                );
            }
        }
    }
    if let Some(error) = state.and_then(|s| s.error.clone()) {
        if ui.adding_provider.is_none() {
            rows.push(muted(error).font_size(look::SMALL));
        }
    }
    let add = keyed_button(
        &add_id(&agent.id),
        "Add a provider",
        "",
        false,
        true,
        palette,
    );
    newen_components::settings::setting(
        "Providers",
        &format!(
            "The model providers {} uses. Connecting one adds its models to the model menu.",
            agent.name
        ),
        add.centered_y(),
    )
    .below(Element::column(rows).gap(10.0))
}

/// "Add a provider": the agent's providers to search, then the one chosen with its key field,
/// the variable it reads instead, or the command its terminal connects it with.
pub(crate) fn dialog(ui: &Ui, base: Element, window: limn::Size) -> Element {
    let Some(adding) = &ui.adding_provider else {
        return base;
    };
    let palette = look::palette();
    let width = limn::Modal::fit_width(window.width, 560.0);
    let inner = limn::Modal::body_width(width);
    let agent = (ui.agents.iter()).find(|a| a.id == adding.agent);
    let name = agent.map_or(adding.agent.as_str(), |a| a.name.as_str());
    let state = ui.providers.get(&adding.agent);
    let list = state.and_then(|s| s.list.as_ref());
    let chosen = adding
        .chosen
        .as_ref()
        .and_then(|id| list.into_iter().flatten().find(|p| p.id == *id));
    let close = || keyed_button(CLOSE, "Cancel", "", false, true, palette);
    let (title, about, body, footer) = match chosen {
        None => {
            let query = adding.query.trim().to_lowercase();
            let found: Vec<&Provider> = (list.into_iter().flatten())
                .filter(|p| {
                    query.is_empty()
                        || p.name.to_lowercase().contains(&query)
                        || p.id.contains(&query)
                })
                .collect();
            let mut items: Vec<Element> = (found.iter())
                .map(|provider| {
                    Element::clickable(
                        format!("{PICK}{}", provider.id),
                        Element::row(vec![
                            avatar(ui, provider, 24.0).centered_y(),
                            text(provider.name.clone()).flex(1.0).centered_y(),
                            muted(standing(provider))
                                .font_size(look::SMALL)
                                .centered_y(),
                            Element::icon(icons::of(icons::CARET_RIGHT))
                                .foreground(look::muted())
                                .size(Some(12.0), Some(12.0))
                                .centered_y(),
                        ])
                        .gap(10.0)
                        .padding(8.0)
                        .size(Some(inner), Some(40.0)),
                    )
                    .accessibility_label(provider.name.clone())
                })
                .map(|row| look::palette().interactive_surface(row))
                .collect();
            if list.is_none() {
                items.push(muted("Looking for its providers\u{2026}").padding(8.0));
            } else if items.is_empty() {
                items.push(
                    muted(format!(
                        "No provider called \u{201c}{}\u{201d}.",
                        adding.query.trim()
                    ))
                    .padding(8.0),
                );
            }
            let search =
                limn::SearchField::new(SEARCH, adding.query.as_str(), "Search providers", palette)
                    .render()
                    .size(Some(inner), None);
            let body = Element::column(vec![
                search,
                Element::scroll("prov:list", Element::column(items).gap(2.0))
                    .size(Some(inner), Some((window.height * 0.5).clamp(200.0, 420.0))),
            ])
            .gap(10.0);
            (
                format!("Add a provider to {name}"),
                "Its models join the model menu once it is connected.".to_owned(),
                body,
                Element::row(vec![Element::spacer().flex(1.0), close()]),
            )
        }
        Some(provider) => {
            let working = state.and_then(|s| s.working.as_deref()) == Some(&provider.id);
            let back = Element::clickable(
                BACK,
                Element::row(vec![
                    Element::icon(icons::of(icons::ARROW_LEFT))
                        .foreground(look::palette().text)
                        .size(Some(14.0), Some(14.0))
                        .centered_y(),
                    text("All providers").font_size(look::SMALL).centered_y(),
                ])
                .gap(6.0),
            );
            let mut column = vec![
                back,
                Element::row(vec![
                    avatar(ui, provider, 36.0).centered_y(),
                    Element::column(vec![
                        text(provider.name.clone()).font_family(limn::FontFamily::SansBold),
                        muted(standing(provider)).font_size(look::SMALL),
                    ])
                    .gap(2.0)
                    .centered_y(),
                ])
                .gap(12.0),
            ];
            if provider.key {
                column.push(
                    limn::TextField::new(KEY, adding.key.as_str(), "API key", palette)
                        .password(true)
                        .width(inner)
                        .render(),
                );
                if let Some(variable) = provider.env.first() {
                    column.push(
                        muted(format!(
                            "Or set {variable} where Newen starts, and it is used as it is."
                        ))
                        .font_size(look::SMALL),
                    );
                }
            }
            if let Some(command) = &provider.terminal {
                column.push(
                    muted(if provider.key {
                        "To sign in instead, run this in a terminal:"
                    } else {
                        "It signs in from a terminal: run this there, then come back."
                    })
                    .font_size(look::SMALL),
                );
                column.push(
                    Element::panel(
                        text(command.clone())
                            .font_family(limn::FontFamily::Monospace)
                            .font_size(look::SMALL)
                            .foreground(look::command_ink()),
                    )
                    .background(look::command_box())
                    .radius(6.0)
                    .padding(10.0)
                    .size(Some(inner), None),
                );
            }
            if let Some(error) = state.and_then(|s| s.error.clone()) {
                column.push(muted(error).font_size(look::SMALL));
            }
            let ready = provider.key && !adding.key.trim().is_empty() && !working;
            let label = if working {
                "Connecting\u{2026}"
            } else {
                "Connect"
            };
            let mut footer = vec![Element::spacer().flex(1.0), close()];
            if provider.key {
                footer.push(keyed_button(CONNECT, label, "", true, ready, palette));
            }
            (
                format!("Connect {}", provider.name),
                String::new(),
                Element::column(column).gap(12.0),
                Element::row(footer).gap(8.0),
            )
        }
    };
    limn::Modal::new(base, true, &title, &about, CLOSE, palette)
        .width(width)
        .body(body)
        .footer(footer)
        .scrim(look::scrim())
        .render()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(id: &str, key: bool, connected: &[&str]) -> Provider {
        Provider {
            id: id.into(),
            name: id.to_uppercase(),
            connected: connected.iter().map(|c| c.to_string()).collect(),
            key,
            ..Provider::default()
        }
    }

    #[test]
    fn a_provider_says_how_it_stands_in_a_line() {
        assert_eq!(
            standing(&provider("groq", true, &["newen-groq"])),
            "Connected"
        );
        assert_eq!(standing(&provider("groq", true, &[])), "API key");
        let mut env = provider("anthropic", true, &[]);
        env.env_set = Some("ANTHROPIC_API_KEY".into());
        assert_eq!(standing(&env), "Connected by ANTHROPIC_API_KEY");
        let mut terminal = provider("copilot", false, &[]);
        terminal.terminal = Some("x auth login copilot".into());
        assert_eq!(standing(&terminal), "Signs in from a terminal");
    }
}
