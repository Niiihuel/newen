//! Settings' Accounts page: GitHub (who is signed in, and the last year of contributions as a
//! heatmap) and one section per agent (its sign-in, and the usage its sessions reported). The
//! heatmap's design is openADE's (`settings_ui/activity_charts.rs`, the user's own code, MIT OR
//! Apache-2.0): `contribution_scale`, `calendar_chart` and `github_card`, with Newen's strings
//! in English. Desktop's Settings shows GitHub as a connector row, "Signed in as @{login}",
//! and usage rows "5-hour limit" and "Weekly · all models" with 4 px bars; the grouping by
//! agent is Newen's.

use crate::github::{self, Calendar};
use crate::view;
use crate::{Store, Ui};
use limn::{Color, Component, ComponentPalette, Element};
use newen_components::pieces::{hgap, muted};
use newen_components::settings::{setting, setting_button, Setting, SettingsPage};
use newen_components::{icons, look};
use newen_model::{AuthState, ContributionCalendar, GithubState, PlanLimits, UsageWindow};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
/// The heatmap's width: 53 weeks of cells of at most 11 px and a 3 px gap, and the day labels.
const HEATMAP: f32 = 720.0;
/// A usage bar's width in a settings row.
const BAR: f32 = 180.0;

/// GitHub's own greens, level 0 (none) to 4, on the page's ground in each theme.
fn contribution_scale() -> limn::HeatmapScale {
    let rgb = |r, g, b| Color::rgb(r, g, b);
    if look::is_dark() {
        limn::HeatmapScale::new([
            rgb(0x2a, 0x2a, 0x2a),
            rgb(0x0e, 0x44, 0x29),
            rgb(0x00, 0x6d, 0x32),
            rgb(0x26, 0xa6, 0x41),
            rgb(0x39, 0xd3, 0x53),
        ])
    } else {
        limn::HeatmapScale::new([
            rgb(0xeb, 0xed, 0xf0),
            rgb(0x9b, 0xe9, 0xa8),
            rgb(0x40, 0xc4, 0x63),
            rgb(0x30, 0xa1, 0x4e),
            rgb(0x21, 0x6e, 0x39),
        ])
    }
}

/// "3 contributions on Sep 06" from a `YYYY-MM-DD` date.
fn tooltip(date: &str, count: u32) -> String {
    let month = date
        .get(5..7)
        .and_then(|m| m.parse::<usize>().ok())
        .and_then(|m| MONTHS.get(m.wrapping_sub(1)))
        .unwrap_or(&"?");
    let day = date.get(8..10).unwrap_or("??");
    let noun = if count == 1 {
        "contribution"
    } else {
        "contributions"
    };
    format!("{count} {noun} on {month} {day}")
}

fn calendar_chart(calendar: &ContributionCalendar, width: f32) -> Element {
    let mut months: Vec<limn::ChartAxisLabel> = Vec::new();
    let mut previous_month: Option<&str> = None;
    let columns = calendar
        .weeks
        .iter()
        .enumerate()
        .map(|(column, week)| {
            week.iter()
                .map(|day| {
                    let day = day.as_ref()?;
                    let month = day.date.get(..7).unwrap_or_default();
                    if previous_month != Some(month) {
                        let name = day
                            .date
                            .get(5..7)
                            .and_then(|m| m.parse::<usize>().ok())
                            .and_then(|m| MONTHS.get(m.wrapping_sub(1)))
                            .unwrap_or(&"");
                        let tick = limn::ChartAxisLabel::new(column, *name);
                        // A month that starts in the same column replaces the one before.
                        match months.last_mut() {
                            Some(last) if last.column == column => *last = tick,
                            _ => months.push(tick),
                        }
                        previous_month = Some(month);
                    }
                    Some(limn::HeatmapCell::new(
                        day.date.clone(),
                        day.level,
                        tooltip(&day.date, day.count),
                    ))
                })
                .collect()
        })
        .collect();
    limn::HeatmapChart::new("github-contributions", columns, look::palette())
        .width(width)
        .row_labels(["", "Mon", "", "Wed", "", "Fri", ""])
        .column_labels(months)
        .scale(contribution_scale())
        .render()
}

/// The card under the account row: the year's total with a refresh, the chart and its
/// legend; a skeleton while it loads; an alert with Retry and Reconnect when it failed.
fn github_card(ui: &Ui, palette: ComponentPalette) -> Setting {
    let body = match &ui.github.calendar {
        Calendar::Ready(calendar) => {
            let refresh = setting_button(github::REFRESH, "Refresh");
            let total = view::thousands(calendar.total);
            let legend =
                limn::ChartLegend::new(contribution_scale().colors, "Less", "More", palette)
                    .render();
            let title = if calendar.total == 1 {
                "1 contribution in the last year".to_owned()
            } else {
                format!("{total} contributions in the last year")
            };
            return setting(&title, "Your GitHub activity.", refresh).below(
                Element::column(vec![
                    calendar_chart(calendar, HEATMAP),
                    Element::row(vec![Element::spacer().flex(1.0), legend])
                        .size(Some(HEATMAP), None),
                ])
                .gap(10.0),
            );
        }
        Calendar::Idle | Calendar::Loading => Element::column(vec![
            muted("Loading contributions\u{2026}"),
            limn::Skeleton::new("github-contributions-loading", palette)
                .width(HEATMAP)
                .height(110.0)
                .radius(4.0)
                .render(),
        ])
        .gap(10.0),
        Calendar::Failed(error) => {
            limn::Alert::new("Could not load the contributions", error.clone(), palette)
                .tone(limn::AlertTone::Error)
                .compact(true)
                .action(
                    Element::row(vec![
                        setting_button(github::REFRESH, "Retry"),
                        setting_button(github::CONNECT, "Reconnect GitHub"),
                    ])
                    .gap(6.0),
                )
                .render()
                .size(Some(HEATMAP), None)
        }
    };
    setting("Contributions", "", hgap(0.0)).below(body)
}

/// The GitHub section's rows.
fn github_rows(ui: &Ui, palette: ComponentPalette) -> Vec<Setting> {
    let buttons = |labels: &[(&str, &str)]| {
        Element::row(
            labels
                .iter()
                .map(|(id, label)| setting_button(id, label))
                .collect(),
        )
        .gap(8.0)
    };
    match (&ui.github.state, github::signed_in(ui)) {
        (_, Some((login, name, _))) => {
            let profile = format!("{}{}{login}", view::LINK_EXTERNAL, github::PROFILE_PREFIX);
            vec![
                setting(
                    &format!("Signed in as @{login}"),
                    name.unwrap_or("GitHub"),
                    buttons(&[
                        (&profile, "View profile"),
                        (github::CONNECT, "Reconnect"),
                        (github::DISCONNECT, "Disconnect"),
                    ]),
                ),
                github_card(ui, palette),
            ]
        }
        (Some(GithubState::Missing(hint)), _) => vec![setting(
            "GitHub CLI not installed",
            &format!(
                "Newen signs in to GitHub through the GitHub CLI. It can download the official \
                 release, check it and keep it in its own folder, with no administrator \
                 rights. {hint}"
            ),
            buttons(&[
                (github::INSTALL, "Install and connect"),
                (github::CHECK, "Check again"),
            ]),
        )],
        (Some(GithubState::Installing(words)), _) => {
            vec![setting("Installing GitHub CLI", words, hgap(0.0))]
        }
        (Some(GithubState::SignedOut), _) => vec![setting(
            "GitHub",
            "Connect your account to show who you are in Newen and your activity here. \
             Newen signs in through the GitHub CLI, which keeps the credentials.",
            buttons(&[(github::CONNECT, "Connect GitHub")]),
        )],
        (Some(GithubState::Unknown(why)), _) => vec![setting(
            "GitHub",
            why,
            buttons(&[(github::CHECK, "Try again")]),
        )],
        _ => vec![setting("GitHub", "Checking\u{2026}", hgap(0.0))],
    }
}

/// The colour of a usage bar's fill: the usual blue, amber from 75 % and red from 90 %.
pub(crate) fn bar_fill(share: f32) -> Color {
    if share >= 0.9 {
        look::removed()
    } else if share >= 0.75 {
        look::manual()
    } else {
        look::handle()
    }
}

/// A 4 px bar filled to `share`, toned by how much is spent.
pub(crate) fn tone_bar(share: f32, width: f32) -> Element {
    let fill = (width * share.clamp(0.0, 1.0)).max(if share > 0.0 { 4.0 } else { 0.0 });
    Element::row(vec![Element::spacer()
        .background(bar_fill(share))
        .radius(2.0)
        .size(Some(fill), Some(4.0))])
    .background(look::chart_tint())
    .radius(2.0)
    .size(Some(width), Some(4.0))
}

/// The usage rows of one agent: "5-hour limit" and "Weekly", each with its share, bar and
/// when it starts again.
fn usage_rows(limits: Option<&PlanLimits>, now: u64, offset: i64) -> Vec<Setting> {
    let windows: Vec<(&str, UsageWindow)> = limits
        .into_iter()
        .flat_map(|l| [("5-hour limit", l.session), ("Weekly", l.weekly)])
        .filter_map(|(label, w)| Some((label, w?)))
        .collect();
    if windows.is_empty() {
        return vec![setting(
            "Usage",
            "Shown once a session of this agent reports its limits.",
            hgap(0.0),
        )];
    }
    windows
        .into_iter()
        .map(|(label, w)| {
            let share = view::spent(&w, now);
            let reset = view::resets_in(w.resets_at, now, offset);
            setting(
                label,
                &reset,
                Element::row(vec![
                    muted(format!("{}%", (share * 100.0).round() as u32)).centered_y(),
                    tone_bar(share, BAR).centered_y(),
                ])
                .gap(10.0),
            )
        })
        .collect()
}

/// "max" as the plan reads: "Max".
fn title_case(word: &str) -> String {
    let mut chars = word.trim().chars();
    chars.next().map_or(String::new(), |c| {
        c.to_uppercase().collect::<String>() + chars.as_str()
    })
}

/// An agent's rows: its sign-in, then its usage.
fn agent_rows(store: &Store, ui: &Ui, agent: &newen_model::AgentInfo, now: u64) -> Vec<Setting> {
    let mut rows = vec![];
    let can_sign_in = !agent.sign_in_methods.is_empty();
    match ui.account.states.get(&agent.id) {
        Some(AuthState::SignedIn {
            email, org, plan, ..
        }) => {
            let about: Vec<String> = [email.clone(), org.clone(), plan.as_deref().map(title_case)]
                .into_iter()
                .flatten()
                .filter(|s| !s.is_empty())
                .collect();
            rows.push(setting(
                "Signed in",
                &about.join(" \u{b7} "),
                setting_button(
                    &format!("{}{}", crate::account::SIGN_OUT_OF, agent.id),
                    "Sign out",
                ),
            ));
        }
        Some(AuthState::SignedOut) => rows.push(setting(
            "Not signed in",
            &format!("Sign in to use {}.", agent.name),
            if can_sign_in {
                setting_button(
                    &format!("{}{}", crate::account::SIGN_IN_OF, agent.id),
                    "Sign in",
                )
            } else {
                hgap(0.0)
            },
        )),
        Some(AuthState::Unknown(why)) => rows.push(setting("Account", why, hgap(0.0))),
        None => rows.push(setting("Account", "Checking\u{2026}", hgap(0.0))),
    }
    rows.extend(usage_rows(
        store.limits_by_agent.get(&agent.id),
        now,
        ui.utc_offset,
    ));
    rows
}

/// The Accounts page.
pub(crate) fn page(store: &Store, ui: &Ui, palette: ComponentPalette) -> SettingsPage {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut groups = vec![("GitHub".to_owned(), github_rows(ui, palette))];
    for agent in &ui.agents {
        groups.push((agent.name.clone(), agent_rows(store, ui, agent, now)));
    }
    SettingsPage {
        key: view::SETTINGS_ACCOUNTS.into(),
        title: "Accounts".into(),
        icon: icons::CLOUD,
        groups,
    }
}
