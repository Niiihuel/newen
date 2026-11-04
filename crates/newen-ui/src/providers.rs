//! An agent's model providers (docs/ui.md "Proveedores"): the ones connected, on the agent's
//! page in Settings, and "Add a provider", a dialog that finds one in the agent's list and
//! connects it with a key or signs in to it (a page in the browser, or this computer's
//! credentials), after its few questions. What the agent offers is its adapter's
//! (`AgentInfo::providers`): nothing here names an agent or a provider.

use crate::Ui;
use limn::{Component, Element};
use newen_components::pieces::{hgap, keyed_button, muted, text};
use newen_components::{icons, look};
use newen_model::{Event, FormField, Intent, Provider, ProviderSignIn, SignInMethod};

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
    /// The way in chosen other than a key, and its form's answers.
    pub method: Option<String>,
    pub answers: Vec<(String, String)>,
    /// Its sign-in as the engine follows it, and a code typed to paste back.
    pub signing: Option<ProviderSignIn>,
    pub code: String,
}

const ADD: &str = "prov:add:";
const CLOSE: &str = "prov:close";
const PICK: &str = "prov:pick:";
const BACK: &str = "prov:back";
const CONNECT: &str = "prov:connect";
const DISCONNECT: &str = "prov:disconnect:";
const METHOD: &str = "prov:method:";
const CHOICE: &str = "prov:choice:";
const START: &str = "prov:start";
const STOP: &str = "prov:stop";
const SEND_CODE: &str = "prov:send-code";
pub const SEARCH: &str = "prov:search";
pub const KEY: &str = "prov:key";
/// A form's typed answer: `{FIELD}{key}`.
pub const FIELD: &str = "prov:field:";
pub const CODE: &str = "prov:code";
/// Send the pasted code, also what Enter in its field does.
pub const SEND_CODE_ID: &str = SEND_CODE;
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

/// The list, after asking or a change: a provider just connected closes the dialog. A sign-in
/// as it goes shows in the dialog of its provider; done, the dialog closes.
pub(crate) fn heard(ui: &mut Ui, event: &Event) {
    if let Event::ProviderSignIn {
        agent,
        provider,
        state,
    } = event
    {
        let ours = |a: &Adding| a.agent == *agent && a.chosen.as_deref() == Some(provider);
        if let Some(adding) = ui.adding_provider.as_mut().filter(|a| ours(a)) {
            match state {
                ProviderSignIn::Done => ui.adding_provider = None,
                state => adding.signing = Some(state.clone()),
            }
        }
        return;
    }
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
        CODE => adding.code = value.to_owned(),
        _ => match id.strip_prefix(FIELD) {
            Some(key) => answer_with(&mut adding.answers, key, value),
            None => return false,
        },
    }
    true
}

/// Set `key`'s answer to `value`.
fn answer_with(answers: &mut Vec<(String, String)>, key: &str, value: &str) {
    answers.retain(|(k, _)| k != key);
    answers.push((key.to_owned(), value.to_owned()));
}

fn answer<'a>(answers: &'a [(String, String)], key: &str) -> Option<&'a str> {
    (answers.iter())
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
}

/// The questions of `method` asked now: each whose `when` the answers so far meet.
fn asked<'a>(method: &'a SignInMethod, answers: &[(String, String)]) -> Vec<&'a FormField> {
    (method.form.iter())
        .filter(|f| (f.when.iter()).all(|(k, v)| answer(answers, k) == Some(v.as_str())))
        .collect()
}

/// Every question asked that must be answered is.
fn answered(method: &SignInMethod, answers: &[(String, String)]) -> bool {
    (asked(method, answers).iter())
        .all(|f| !f.required || answer(answers, &f.key).is_some_and(|v| !v.trim().is_empty()))
}

/// The answers to the questions asked now, as the sign-in takes them.
fn sent(method: &SignInMethod, answers: &[(String, String)]) -> Vec<(String, String)> {
    (asked(method, answers).iter())
        .filter_map(|f| {
            let value = answer(answers, &f.key)?.trim();
            (!value.is_empty()).then(|| (f.key.clone(), value.to_owned()))
        })
        .collect()
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
    // Leaving a sign-in under way cancels it.
    let waiting = matches!(adding.signing, Some(ProviderSignIn::Waiting { .. }));
    let cancel = |adding: &Adding| Intent::CancelProviderSignIn {
        agent: adding.agent.clone(),
        provider: adding.chosen.clone().unwrap_or_default(),
    };
    let method_of = |ui: &Ui, adding: &Adding, id: &str| -> Option<SignInMethod> {
        let list = ui.providers.get(&adding.agent)?.list.as_ref()?;
        let provider = list
            .iter()
            .find(|p| Some(&p.id) == adding.chosen.as_ref())?;
        provider.sign_ins.iter().find(|m| m.id == id).cloned()
    };
    match id {
        CLOSE | BACK | STOP if waiting => {
            let intent = cancel(adding);
            match id {
                CLOSE => ui.adding_provider = None,
                BACK => {
                    adding.chosen = None;
                    adding.signing = None;
                }
                _ => adding.signing = None,
            }
            return Some(vec![intent]);
        }
        STOP => adding.signing = None,
        SEND_CODE => {
            let code = adding.code.trim().to_owned();
            if code.is_empty() {
                return Some(vec![]);
            }
            adding.code.clear();
            return Some(vec![Intent::ProviderSignInCode {
                agent: adding.agent.clone(),
                provider: adding.chosen.clone()?,
                code,
            }]);
        }
        START => {
            let method = adding.method.clone()?;
            let ways = method_of(ui, ui.adding_provider.as_ref()?, &method)?;
            let adding = ui.adding_provider.as_mut()?;
            if !answered(&ways, &adding.answers) {
                return Some(vec![]);
            }
            adding.signing = None;
            return Some(vec![Intent::ProviderSignIn {
                agent: adding.agent.clone(),
                provider: adding.chosen.clone()?,
                method,
                answers: sent(&ways, &adding.answers),
            }]);
        }
        CLOSE => ui.adding_provider = None,
        BACK => {
            adding.chosen = None;
            adding.key.clear();
            adding.method = None;
            adding.signing = None;
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
        _ if id.starts_with(METHOD) => {
            let method = id.strip_prefix(METHOD)?.to_owned();
            let ways = method_of(ui, ui.adding_provider.as_ref()?, &method)?;
            let adding = ui.adding_provider.as_mut()?;
            adding.answers = (ways.form.iter())
                .filter_map(|f| Some((f.key.clone(), f.default.clone()?)))
                .collect();
            adding.method = Some(method);
            adding.signing = None;
            // Nothing to ask: it starts at once.
            if ways.form.is_empty() {
                return clicked(ui, START);
            }
        }
        _ if id.starts_with(CHOICE) => {
            let (key, value) = id.strip_prefix(CHOICE)?.split_once('=')?;
            answer_with(&mut adding.answers, key, value);
        }
        _ => {
            let provider = id.strip_prefix(PICK)?;
            adding.chosen = Some(provider.to_owned());
            adding.key.clear();
            adding.method = None;
            adding.signing = None;
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
    } else if provider.sign_ins.iter().any(|m| m.browser) {
        "Signs in in the browser".into()
    } else if !provider.sign_ins.is_empty() {
        "Uses this computer's credentials".into()
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
            let waiting = match &adding.signing {
                Some(ProviderSignIn::Waiting {
                    url,
                    instructions,
                    code,
                }) => Some((url, instructions, *code)),
                _ => None,
            };
            if let Some((url, instructions, code)) = waiting {
                // The sign-in leads: what to do on its page, then a code to paste back.
                column.truncate(2);
                column.push(signing_panel(url, instructions, inner));
                if code {
                    column.push(
                        limn::TextField::new(CODE, adding.code.as_str(), "Paste the code", palette)
                            .width(inner)
                            .render(),
                    );
                }
            } else {
                column.extend(sign_ins(provider, adding, inner));
            }
            if let Some(ProviderSignIn::Failed(why)) = &adding.signing {
                column.push(muted(format!("It did not sign in: {why}")).font_size(look::SMALL));
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
            match waiting {
                Some((_, _, code)) => {
                    footer.push(keyed_button(
                        STOP,
                        "Cancel sign-in",
                        "",
                        false,
                        true,
                        palette,
                    ));
                    if code {
                        let typed = !adding.code.trim().is_empty();
                        footer.push(keyed_button(
                            SEND_CODE, "Continue", "", true, typed, palette,
                        ));
                    }
                }
                None if provider.key => {
                    footer.push(keyed_button(CONNECT, label, "", true, ready, palette))
                }
                None => {}
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

/// What to do on the sign-in's page: its instructions (a code to enter, large), and that the
/// page opened in the browser.
fn signing_panel(url: &Option<String>, instructions: &Option<String>, inner: f32) -> Element {
    let mut rows = vec![Element::row(vec![
        limn::Spinner::new("prov:signing", look::muted())
            .size(14.0)
            .render(),
        muted("Finish signing in on the page that opened in your browser.").centered_y(),
    ])
    .gap(8.0)];
    if let Some(instructions) = instructions {
        rows.push(
            text(instructions.clone())
                .font_family(limn::FontFamily::MonospaceBold)
                .font_size(20.0),
        );
    }
    if let Some(url) = url {
        rows.push(muted(url.clone()).font_size(look::SMALL));
    }
    Element::panel(Element::column(rows).gap(10.0))
        .background(look::chart_tint())
        .radius(8.0)
        .padding(14.0)
        .size(Some(inner), None)
}

/// A provider's ways in other than a key: a button for each, and under the chosen one its
/// questions (choices as buttons, the rest typed), each asked only when it applies, then
/// Continue.
fn sign_ins(provider: &Provider, adding: &Adding, inner: f32) -> Vec<Element> {
    let palette = look::palette();
    let mut rows = vec![];
    if provider.sign_ins.is_empty() {
        return rows;
    }
    if provider.key {
        rows.push(muted("Or sign in instead:").font_size(look::SMALL));
    }
    for method in &provider.sign_ins {
        let chosen = adding.method.as_deref() == Some(method.id.as_str());
        rows.push(Element::row(vec![keyed_button(
            &format!("{METHOD}{}", method.id),
            &method.label,
            "",
            chosen && method.form.is_empty(),
            true,
            palette,
        )]));
        if !chosen || method.form.is_empty() {
            continue;
        }
        for field in asked(method, &adding.answers) {
            let mut question = vec![text(field.title.clone()).font_size(look::SMALL)];
            if let Some(about) = &field.description {
                question.push(muted(about.clone()).font_size(look::SMALL));
            }
            if field.choices.is_empty() {
                question.push(
                    limn::TextField::new(
                        format!("{FIELD}{}", field.key),
                        answer(&adding.answers, &field.key).unwrap_or_default(),
                        field.placeholder.as_deref().unwrap_or_default(),
                        palette,
                    )
                    .width(inner)
                    .render(),
                );
            } else {
                let picked = answer(&adding.answers, &field.key);
                question.push(
                    Element::row(
                        (field.choices.iter())
                            .map(|choice| {
                                keyed_button(
                                    &format!("{CHOICE}{}={}", field.key, choice.value),
                                    &choice.label,
                                    "",
                                    picked == Some(choice.value.as_str()),
                                    true,
                                    palette,
                                )
                            })
                            .collect(),
                    )
                    .gap(8.0)
                    .wrap(),
                );
            }
            rows.push(Element::column(question).gap(6.0));
        }
        rows.push(Element::row(vec![keyed_button(
            START,
            "Continue",
            "",
            true,
            answered(method, &adding.answers),
            palette,
        )]));
    }
    rows
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
        let mut browser = provider("copilot", false, &[]);
        browser.sign_ins = vec![SignInMethod {
            id: "oauth:device".into(),
            label: "Login".into(),
            browser: true,
            form: vec![],
        }];
        assert_eq!(standing(&browser), "Signs in in the browser");
    }

    /// A question asked only for one answer of another, and Continue only once each asked one
    /// that must be answered is.
    #[test]
    fn a_question_is_asked_when_its_condition_holds() {
        let field = |key: &str, when: &[(&str, &str)]| FormField {
            key: key.into(),
            title: key.into(),
            required: true,
            when: when
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..FormField::default()
        };
        let method = SignInMethod {
            id: "oauth:device".into(),
            label: "Login".into(),
            browser: true,
            form: vec![field("kind", &[]), field("url", &[("kind", "enterprise")])],
        };
        let mut answers = vec![];
        assert_eq!(asked(&method, &answers).len(), 1);
        assert!(!answered(&method, &answers));
        answer_with(&mut answers, "kind", "public");
        assert!(answered(&method, &answers));
        answer_with(&mut answers, "kind", "enterprise");
        assert_eq!(asked(&method, &answers).len(), 2);
        assert!(!answered(&method, &answers));
        answer_with(&mut answers, "url", " ghe.example.com ");
        assert_eq!(
            sent(&method, &answers),
            [
                ("kind".to_owned(), "enterprise".to_owned()),
                ("url".to_owned(), "ghe.example.com".to_owned())
            ]
        );
    }
}
