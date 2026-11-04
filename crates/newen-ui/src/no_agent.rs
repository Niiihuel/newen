//! The screen shown while none of the agents Newen has adapters for is installed: Newen runs
//! each agent's own program, so without one there is nothing to start. Full window like the
//! sign-in screen: Newen's mark, a title, each agent with the command that installs it, and
//! Check again.

use crate::account::{finish, wide};
use crate::Ui;
use limn::{Element, FontFamily, Size};
use newen_components::icons;
use newen_components::look::{self, SMALL};
use newen_components::pieces::{muted, text, tool_surface};
use newen_model::{AgentInfo, Intent};

/// An agent's install command's Copy, followed by the agent's id.
pub const COPY: &str = "no-agent:copy:";
pub const CHECK: &str = "no-agent:check";

/// Whether the screen shows: the engine said which agents there are, and none is installed.
pub fn showing(ui: &Ui) -> bool {
    ui.agents.is_empty() && !ui.missing_agents.is_empty()
}

pub fn view(ui: &Ui, window: Size) -> Element {
    let mut column = vec![
        crate::welcome::mark(56.0).centered_x(),
        Element::spacer().size(None, Some(20.0)),
        text("No agent installed").font_size(22.0).centered_x(),
        Element::spacer().size(None, Some(8.0)),
        // Centered by hand: limn's text has no alignment, so each line is its own.
        muted("Newen works through an agent\u{2019}s own program.").centered_x(),
        muted("Install one, then check again.").centered_x(),
        Element::spacer().size(None, Some(28.0)),
    ];
    column.push(Element::column(ui.missing_agents.iter().map(|a| card(ui, a)).collect()).gap(10.0));
    column.push(Element::spacer().size(None, Some(24.0)));
    column.push(wide(CHECK, "Check again", true).centered_x());
    finish(column, 440.0, window)
}

/// An agent's logo and name over the command that installs it, with its Copy.
fn card(ui: &Ui, agent: &AgentInfo) -> Element {
    let id = format!("{COPY}{}", agent.id);
    let copied = (ui.copied.as_ref())
        .is_some_and(|(c, since)| *c == id && since.elapsed() < crate::view::COPIED_FOR);
    let copy = tool_surface(
        &id,
        icons::of(if copied { icons::CHECK } else { icons::COPY }),
        if copied { "Copied" } else { "Copy command" },
        24.0,
        None,
        look::tool_hover(),
        None,
    );
    let logo = Element::icon(icons::of(icons::agent(agent.icon.as_deref())))
        .foreground(look::palette().text)
        .size(Some(16.0), Some(16.0))
        .centered_y();
    let command = Element::text(agent.install.clone())
        .font_family(FontFamily::Monospace)
        .font_size(SMALL)
        .foreground(look::muted())
        .centered_y();
    Element::column(vec![
        Element::row(vec![logo, text(agent.name.clone()).centered_y()]).gap(8.0),
        Element::row(vec![command, Element::spacer().flex(1.0), copy]).gap(8.0),
    ])
    .gap(6.0)
    .padding(12.0)
    .border(look::composer_border(), 1.0)
    .radius(10.0)
}

/// The clicks of the screen, or `None` for another's.
pub(crate) fn clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    if id == CHECK {
        // Saving the settings as they are has the engine look for the programs again.
        return Some(vec![Intent::SaveSettings(ui.settings.clone())]);
    }
    let agent = id.strip_prefix(COPY)?;
    let install = (ui.missing_agents.iter())
        .find(|a| a.id == agent)?
        .install
        .clone();
    // The host puts it on the clipboard on its next frame.
    ui.files_copy = Some(install);
    ui.copied = Some((id.to_owned(), std::time::Instant::now()));
    Some(vec![])
}
