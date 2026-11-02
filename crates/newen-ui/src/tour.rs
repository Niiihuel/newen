//! The tour after the welcome: a bubble on each of six of the window's own controls, in order.
//! Its steps and look are Newen's own; Desktop's onboarding was not captured, so nothing here
//! is copied from it (inferred). The host passes a step whose control is not laid out, so the
//! tour never waits on a control that is not there.

use crate::view::{
    ACCOUNT_MENU, BROWSER_TOGGLE, COMPOSER, DIFF_TOGGLE, EFFORT_MENU, MODEL_MENU, MODE_MENU,
    NEW_SESSION, PANEL_TOGGLE, TERMINAL_TOGGLE,
};
use crate::Ui;
use limn::{CoachAlign, CoachSide, Component, ComponentPalette, Element, Tour, TourStep};
use newen_model::{Event, Intent};

/// The account menu's entry that starts the tour again.
pub const START: &str = "tour:start";
/// The tour's own ID: its close control and Escape report it, its actions are `tour:next`,
/// `tour:back` and `tour:skip`.
const TOUR: &str = "tour";
const NEXT: &str = "tour:next";
const BACK: &str = "tour:back";
const SKIP: &str = "tour:skip";

/// One step: the controls its bubble points at (the first is the one a missing check names),
/// what it says and where the bubble sits.
struct Step {
    targets: &'static [&'static str],
    title: &'static str,
    body: &'static str,
    side: CoachSide,
    align: CoachAlign,
}

const STEPS: [Step; 6] = [
    Step {
        targets: &[COMPOSER],
        title: "Tell the agent what to do",
        body: "Describe a task. Enter sends it; while the agent works, Enter steers it and Ctrl+Enter queues it for later.",
        side: CoachSide::Above,
        align: CoachAlign::Center,
    },
    Step {
        targets: &[MODEL_MENU, EFFORT_MENU],
        title: "Pick the model and its effort",
        body: "Choose which model works and how much it thinks before it answers.",
        side: CoachSide::Above,
        align: CoachAlign::End,
    },
    Step {
        targets: &[MODE_MENU],
        title: "Decide what it may do alone",
        body: "From asking before every change to working on its own; change it any time.",
        side: CoachSide::Above,
        align: CoachAlign::Start,
    },
    Step {
        targets: &[NEW_SESSION],
        title: "Sessions run side by side",
        body: "Start another session for other work; each keeps its history, and a session can start others.",
        side: CoachSide::Right,
        align: CoachAlign::Center,
    },
    // The right panel's toggle leads; the browser, terminal and changes buttons are laid out
    // beside it, and a button not in view (no session yet) is left out of the ring.
    Step {
        targets: &[PANEL_TOGGLE, BROWSER_TOGGLE, TERMINAL_TOGGLE, DIFF_TOGGLE],
        title: "Everything beside the chat",
        body: "Its folder and context, the browser to see your app, a terminal, the changes it made.",
        side: CoachSide::Below,
        align: CoachAlign::Center,
    },
    Step {
        targets: &[ACCOUNT_MENU],
        title: "Settings and updates",
        body: "Agents, connectors and plugins, appearance, and Newen's updates.",
        side: CoachSide::Right,
        align: CoachAlign::End,
    },
];

/// Settings were read at a start: a person who welcomed before and never toured sees the tour
/// once now, as the welcome would have shown it. A stand-in run keeps no settings, so it skips.
pub(crate) fn heard(ui: &mut Ui, event: &Event) {
    let Event::Settings { settings, agents } = event else {
        return;
    };
    if ui.agents.is_empty()
        && !agents.is_empty()
        && settings.welcomed
        && !settings.toured
        && !ui.skip_welcome
    {
        ui.tour = Some(0);
    }
}

/// After Get started the tour starts, unless it was already finished.
pub(crate) fn after_welcome(ui: &mut Ui) {
    ui.tour = (!ui.settings.toured).then_some(0);
}

/// The clicks of the tour and its menu entry, or `None` for another's.
pub(crate) fn clicked(ui: &mut Ui, id: &str) -> Option<Vec<Intent>> {
    Some(match id {
        START => {
            ui.account_menu = false;
            ui.tour = Some(0);
            vec![]
        }
        // Escape and the close control end the tour as Skip does: the person has seen enough.
        TOUR | SKIP => finish(ui),
        NEXT => advance(ui),
        BACK => {
            ui.tour = ui.tour.map(|step| step.saturating_sub(1));
            vec![]
        }
        _ => return None,
    })
}

/// The host moves past the current step when its control is not laid out, as its own Next
/// would. `None` when `target` is not the current step's.
pub fn skip_missing(ui: &mut Ui, target: &str) -> Option<Vec<Intent>> {
    let step = STEPS.get(ui.tour?)?;
    // Behind the sign-in screen no control is laid out: the tour waits for it to go.
    if crate::account::visible(ui) {
        return None;
    }
    (step.targets[0] == target).then(|| advance(ui))
}

/// The next step, or the end after the last one: Done saves that the tour was seen.
fn advance(ui: &mut Ui) -> Vec<Intent> {
    match ui.tour {
        Some(step) if step + 1 < STEPS.len() => {
            ui.tour = Some(step + 1);
            vec![]
        }
        _ => finish(ui),
    }
}

fn finish(ui: &mut Ui) -> Vec<Intent> {
    ui.tour = None;
    ui.settings.toured = true;
    vec![Intent::SaveSettings(ui.settings.clone())]
}

/// `root` with the current step's bubble over it while the tour runs; the rest of the window
/// takes no input meanwhile.
pub(crate) fn layer(ui: &Ui, root: Element, palette: ComponentPalette) -> Element {
    let steps = STEPS
        .iter()
        .map(|step| {
            let mut tour_step = TourStep::new(step.targets[0], step.title, step.body)
                .placement(step.side, step.align);
            for target in &step.targets[1..] {
                tour_step = tour_step.also(*target);
            }
            tour_step
        })
        .collect();
    // The labels are Newen's own: limn's defaults are Spanish.
    Tour::new(root, TOUR, steps, ui.tour, palette)
        .coach_mark()
        .counter(format!("{} of {}", ui.tour.unwrap_or(0) + 1, STEPS.len()))
        .next_label("Next")
        .done_label("Done")
        .back_label("Back")
        .skip("Skip tour")
        .render()
}
