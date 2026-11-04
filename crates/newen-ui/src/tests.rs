use super::*;
use crate::customize;
use limn::{UiAction, UiTree};
use newen_model::{AgentEvent, AuthState, Theme, TranscriptView, Usage};

fn tab(title: &str, busy: bool) -> TerminalTab {
    TerminalTab {
        title: title.into(),
        busy,
        named: false,
    }
}

fn store_with_session() -> (Store, Ui) {
    let mut store = Store::default();
    store.apply(Event::SessionCreated {
        session: 1,
        agent: "claude".into(),
        cwd: "/work".into(),
    });
    store.apply(Event::Queued {
        session: 1,
        request: "r1".into(),
        text: "Fix the bug".into(),
        at: None,
        images: vec![],
    });
    let mut ui = Ui::new("/work".into());
    ui.selected = Some(1);
    // As a stand-in agent's run: the window, not the welcome.
    ui.skip_welcome = true;
    (store, ui)
}

fn agent(store: &mut Store, event: AgentEvent) {
    store.apply(Event::Agent {
        session: 1,
        request: Some("r1".into()),
        event,
    });
}

#[test]
fn thinking_is_kept_whole_before_the_answer_and_not_drawn() {
    let (mut store, ui) = store_with_session();
    for text in ["Numbers ", "divide"] {
        agent(
            &mut store,
            AgentEvent::ThinkingDelta {
                text: text.into(),
                parent: None,
            },
        );
    }
    agent(
        &mut store,
        AgentEvent::ThinkingText {
            text: "Numbers divide".into(),
            parent: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::Text {
            text: "Hello".into(),
            parent: None,
            id: None,
            at: None,
        },
    );
    let items = &store.session(1).unwrap().items;
    assert_eq!(
        items[1..],
        [
            Item::Thinking {
                text: "Numbers divide".into(),
                streaming: false,
            },
            Item::Assistant {
                text: "Hello".into(),
                streaming: false,
                id: None,
                at: None,
            },
        ]
    );
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let drawn = |words: &str| {
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    assert!(drawn("Hello"));
    assert!(!drawn("Numbers divide"));
}

#[test]
fn streamed_text_becomes_one_message() {
    let (mut store, _) = store_with_session();
    agent(
        &mut store,
        AgentEvent::TextDelta {
            text: "Hel".into(),
            parent: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::TextDelta {
            text: "lo".into(),
            parent: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::Text {
            text: "Hello".into(),
            parent: None,
            id: Some("m1".into()),
            at: Some(5),
        },
    );
    let items = &store.session(1).unwrap().items;
    assert_eq!(
        items.last(),
        Some(&Item::Assistant {
            text: "Hello".into(),
            streaming: false,
            id: Some("m1".into()),
            at: Some(5),
        })
    );
    assert_eq!(store.session(1).unwrap().title, "Fix the bug");
}

#[test]
fn an_edit_counts_one_line_each_way() {
    assert_eq!(line_changes(Some("hello\n"), Some("hello world\n")), (1, 1));
    assert_eq!(line_changes(None, Some("a\nb\n")), (2, 0));
}

#[test]
fn always_allow_applies_every_suggestion_and_leads_the_next_card() {
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::PermissionRequest {
            id: "p1".into(),
            call: Some("t1".into()),
            tool: "Bash".into(),
            input: serde_json::json!({ "command": "touch a" }),
            title: None,
            interactive: false,
            suggestions: vec![
                Suggestion::Rules(serde_json::json!({ "type": "addRules" })),
                Suggestion::Mode("acceptEdits".into()),
            ],
        },
    );
    let intents = update(
        &store,
        &mut ui,
        &UiAction::Clicked {
            id: "perm:p1:always".into(),
        },
    );
    let [Intent::Decide {
        decision: Decision::AllowAlways(kept),
        ..
    }] = intents.as_slice()
    else {
        panic!("{intents:?}")
    };
    assert_eq!(kept.len(), 2);
    assert_eq!(ui.last_allow, Allow::Always);
}

#[test]
fn ctrl_enter_answers_the_waiting_permission_from_anywhere() {
    let (mut store, ui) = store_with_session();
    assert_eq!(shortcut(&store, &ui, limn::Key::Enter, true, false), None);
    agent(
        &mut store,
        AgentEvent::PermissionRequest {
            id: "p1".into(),
            call: Some("t1".into()),
            tool: "Bash".into(),
            input: serde_json::json!({ "command": "touch a" }),
            title: None,
            interactive: false,
            suggestions: vec![],
        },
    );
    assert_eq!(
        shortcut(&store, &ui, limn::Key::Enter, true, false).as_deref(),
        Some("perm:p1:once")
    );
    assert_eq!(
        shortcut(&store, &ui, limn::Key::Enter, true, true).as_deref(),
        Some("perm:p1:always")
    );
    assert_eq!(shortcut(&store, &ui, limn::Key::Enter, false, false), None);
}

#[test]
fn the_first_message_creates_a_session_then_sends_it() {
    let store = Store::default();
    let mut ui = Ui::new("/work".into());
    let intents = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::COMPOSER.into(),
            value: "hi".into(),
        },
    );
    assert!(matches!(intents.as_slice(), [Intent::NewSession(_)]));
    let then = ui.applied(&Event::SessionCreated {
        session: 7,
        agent: "claude".into(),
        cwd: "/work".into(),
    });
    assert_eq!(
        then,
        vec![
            Intent::Send {
                session: 7,
                text: "hi".into(),
                mentions: vec![],
                images: vec![],
                lane: Lane::Steer,
            },
            Intent::WorkingTree { session: 7 }
        ]
    );
    assert_eq!(ui.selected, Some(7));
}

#[test]
fn the_window_lays_out_with_a_finished_turn_and_a_waiting_permission() {
    let (mut store, ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::TurnFinished {
            ok: true,
            interrupted: false,
            usage: Usage::default(),
            error: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::ToolUse {
            call: "t1".into(),
            name: "Bash".into(),
            input: serde_json::json!({ "command": "ls" }),
            parent: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::PermissionRequest {
            id: "p1".into(),
            call: Some("t1".into()),
            tool: "Bash".into(),
            input: serde_json::json!({ "command": "ls", "description": "List files" }),
            title: None,
            interactive: false,
            suggestions: vec![Suggestion::Rules(serde_json::json!({ "type": "addRules" }))],
        },
    );
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    let frame = tree.layout(window);
    for id in [
        "perm:p1:deny",
        "perm:p1:menu",
        "perm:p1:once",
        view::COMPOSER,
        "session:1",
    ] {
        let region = frame
            .regions
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("no {id}"));
        assert!(
            region.rect.x + region.rect.width <= 1408.0,
            "{id} outside the window"
        );
    }
}

#[test]
fn a_question_is_answered_with_the_picked_option() {
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::PermissionRequest {
            id: "p9".into(),
            call: None,
            tool: "AskUserQuestion".into(),
            input: serde_json::json!({ "questions": [{ "question": "Red or blue?", "header": "Color",
                "options": [{ "label": "Red" }, { "label": "Blue" }] }] }),
            title: None,
            interactive: true,
            suggestions: vec![],
        },
    );
    assert!(update(
        &store,
        &mut ui,
        &UiAction::Clicked {
            id: "ask:p9:0:1".into()
        }
    )
    .is_empty());
    let intents = update(
        &store,
        &mut ui,
        &UiAction::Clicked {
            id: "ask:p9:submit".into(),
        },
    );
    let [Intent::Decide {
        decision: Decision::Answer(answers),
        ..
    }] = intents.as_slice()
    else {
        panic!("{intents:?}")
    };
    assert_eq!(answers["Red or blue?"], "Blue");
}

#[test]
fn a_rewritten_line_marks_only_what_changed() {
    let lines = view::parse_patch("@@ -1,2 +1,2 @@\n-    print(\"Hola \" + nombre)\n+    print(f\"Hola, {nombre}!\")\n context\n");
    let words = view::word_ranges(&lines);
    assert_eq!(lines.len(), 3);
    // Common start "    print(" (10) and common end ")" (1).
    assert_eq!(words[0], vec![10..26]);
    assert_eq!(words[1], vec![10..28]);
    assert!(words[2].is_empty());
}

#[test]
fn pull_request_links_can_open_on_the_computer_alone() {
    use newen_model::Settings;
    let mut settings = Settings::default();
    let pr = "https://github.com/o/r/pull/12";
    assert!(!opens_outside(&settings, pr));
    settings.prs_in_browser = true;
    for url in [
        pr,
        "https://github.com/o/r/pull/12/files",
        "https://github.com/o/r/pull/12?tab=files",
        "https://gitlab.com/g/p/-/merge_requests/7",
    ] {
        assert!(opens_outside(&settings, url), "{url}");
    }
    for url in [
        "https://github.com/o/r/pulls",
        "https://example.com/pull/abc",
        "https://example.com/docs",
        "mailto:someone@example.com",
        "file:///tmp/pull/1",
    ] {
        assert!(!opens_outside(&settings, url), "{url}");
    }
    settings.links_in_browser = false;
    assert!(opens_outside(&settings, "https://example.com/docs"));
    assert!(!opens_outside(&settings, "mailto:someone@example.com"));
}

#[test]
fn the_terminal_toggles_and_takes_a_third_beside_the_conversation() {
    let store = Store::default();
    let mut ui = Ui::new("/tmp".into());
    let click = limn::UiAction::Clicked {
        id: view::TERMINAL_TOGGLE.into(),
    };
    update(&store, &mut ui, &click);
    assert!(ui.pane_open(Pane::Terminal));
    // Desktop at 1408×844: a 373 px column, its shell 12 px in from each side.
    let (cols, rows) = view::terminal_grid(&store, &ui, limn::Size::new(1408.0, 844.0));
    assert!((45..=50).contains(&cols) && rows > 40, "{cols}x{rows}");
    update(&store, &mut ui, &click);
    assert!(!ui.pane_open(Pane::Terminal));
}

#[test]
fn closing_a_terminal_with_a_running_command_asks_first() {
    let store = Store::default();
    let mut ui = Ui::new("/tmp".into());
    ui.terminal.tabs = vec![tab("Terminal 1", false), tab("Terminal 2", true)];
    let click = |ui: &mut Ui, id: &str| {
        update(&store, ui, &limn::UiAction::Clicked { id: id.into() });
    };
    click(&mut ui, "terminal:close:0");
    assert_eq!(ui.terminal.requests, [TerminalRequest::Close(0)]);
    ui.terminal.requests.clear();
    click(&mut ui, "terminal:close:1");
    assert_eq!(
        (ui.terminal.closing, ui.terminal.requests.len()),
        (Some(Closing::One(1)), 0)
    );
    click(&mut ui, view::TERMINAL_CLOSE_CANCEL);
    assert_eq!(ui.terminal.closing, None);
    click(&mut ui, "terminal:close:1");
    click(&mut ui, view::TERMINAL_CLOSE_CONFIRM);
    assert_eq!(ui.terminal.requests, [TerminalRequest::Close(1)]);
}

#[test]
fn the_terminal_pane_and_its_tabs_fit_in_the_window() {
    let store = Store::default();
    let mut ui = Ui::new("/tmp".into());
    ui.open_pane(Pane::Terminal);
    ui.terminal.tabs = vec![tab("Terminal 1", false), tab("Terminal 2", false)];
    ui.terminal.screen = Some(std::sync::Arc::new(limn::TerminalScreen {
        cols: 80,
        rows: 14,
        cells: vec![limn::TerminalCell::new('x', limn::Color::rgb(0, 0, 0)); 80 * 14],
        scrollback: vec![],
        cursor: None,
    }));
    settle(&store, &mut ui);
    let window = limn::Size::new(1400.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    let frame = tree.layout(window);
    for id in [
        view::TERMINAL,
        view::TERMINAL_NEW,
        "terminal:close:0",
        "terminal:tab:1",
    ] {
        let region = frame
            .regions
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("no {id}"));
        let r = region.rect;
        assert!(
            r.width > 0.0 && r.x + r.width <= 1400.0 && r.y + r.height <= 844.0,
            "{id} at {r:?}"
        );
    }
}

#[test]
fn one_terminal_shows_its_folder_and_more_show_as_pills() {
    let store = Store::default();
    let mut ui = Ui::new("/tmp/work".into());
    ui.open_pane(Pane::Terminal);
    ui.terminal.tabs = vec![tab("Terminal 1", false)];
    ui.terminal.screen = Some(std::sync::Arc::default());
    settle(&store, &mut ui);
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(!frame.regions.iter().any(|r| r.id == "terminal:close:0"));
    ui.terminal.tabs.push(tab("Terminal 2", false));
    ui.terminal.screen = Some(std::sync::Arc::default());
    let window = limn::Size::new(1400.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let close = frame
        .regions
        .iter()
        .find(|r| r.id == "terminal:close:0")
        .unwrap();
    assert!(close.rect.width <= 20.0, "{:?}", close.rect);
}

#[test]
fn a_permission_read_back_shows_its_answer() {
    // A session read back from the store has no intents, only events.
    let (mut store, _) = store_with_session();
    agent(
        &mut store,
        AgentEvent::PermissionRequest {
            id: "p1".into(),
            call: Some("t1".into()),
            tool: "Bash".into(),
            input: serde_json::json!({ "command": "touch a" }),
            title: None,
            interactive: false,
            suggestions: vec![],
        },
    );
    store.apply(Event::Decided {
        session: 1,
        permission: "p1".into(),
        decision: newen_model::Decision::Allow,
    });
    let decided = store
        .session(1)
        .unwrap()
        .items
        .iter()
        .find_map(|i| match i {
            Item::Permission { id, decided, .. } if id == "p1" => Some(*decided),
            _ => None,
        });
    assert_eq!(decided, Some(Some("Allowed")));
}

#[test]
fn the_sidebar_groups_sessions_by_folder_and_folds_a_group() {
    let mut store = Store::default();
    for (session, cwd) in [(1, "/work"), (2, "/other"), (3, "/work")] {
        store.apply(Event::SessionCreated {
            session,
            agent: "claude".into(),
            cwd: cwd.into(),
        });
    }
    let mut ui = Ui::new("/work".into());
    ui.group_by = GroupBy::Folder;
    let window = limn::Size::new(1408.0, 844.0);
    let ids = |store: &Store, ui: &Ui| -> Vec<String> {
        let frame = UiTree::new(view(store, ui, window)).layout(window);
        frame
            .regions
            .iter()
            .filter(|r| r.id.starts_with("group:") || r.id.starts_with("session:"))
            .map(|r| r.id.clone())
            .collect()
    };
    // The folder with the newest session comes first, its sessions newest first.
    assert_eq!(
        ids(&store, &ui),
        [
            "group:/work",
            "session:3",
            "session:1",
            "group:/other",
            "session:2"
        ]
    );
    update(
        &store,
        &mut ui,
        &UiAction::Clicked {
            id: "group:/work".into(),
        },
    );
    assert_eq!(
        ids(&store, &ui),
        ["group:/work", "group:/other", "session:2"]
    );
}

#[test]
fn a_finished_turn_closes_the_permissions_it_left_open() {
    let (mut store, ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::PermissionRequest {
            id: "p1".into(),
            call: Some("t1".into()),
            tool: "Bash".into(),
            input: serde_json::json!({ "command": "touch a" }),
            title: None,
            interactive: false,
            suggestions: vec![],
        },
    );
    agent(
        &mut store,
        AgentEvent::TurnFinished {
            ok: false,
            interrupted: true,
            usage: Usage::default(),
            error: None,
        },
    );
    assert_eq!(shortcut(&store, &ui, limn::Key::Enter, true, false), None);
}

#[test]
fn dragging_the_boundary_resizes_the_panes_within_their_minimums() {
    let store = Store::default();
    let mut ui = Ui::new("/tmp".into());
    ui.open_pane(Pane::Terminal);
    let drag = |ui: &mut Ui, x: f32| {
        update(
            &store,
            ui,
            &UiAction::DragMoved {
                id: view::COLUMN_HANDLE.into(),
                position: limn::Point::new(x, 400.0),
                over: None,
            },
        );
    };
    // From a third (373 px at 1408) to the boundary at x = 900: 508 px.
    drag(&mut ui, 900.0);
    let rest = 1408.0 - look::SIDEBAR_WIDTH;
    assert_eq!((ui.column_fraction.unwrap() * rest).round(), 508.0);
    // Never so far that the conversation gets under its minimum.
    drag(&mut ui, 300.0);
    assert_eq!(
        (ui.column_fraction.unwrap() * rest).round(),
        rest - view::MIN_CHAT
    );
}

#[test]
fn the_panel_edge_resizes_the_panel_and_a_click_hides_it() {
    let store = Store::default();
    let mut ui = Ui::new("/tmp".into());
    let drag = |ui: &mut Ui, x: f32| {
        update(
            &store,
            ui,
            &UiAction::DragMoved {
                id: view::PANEL_HANDLE.into(),
                position: limn::Point::new(x, 400.0),
                over: None,
            },
        );
    };
    drag(&mut ui, 1008.0);
    assert_eq!(ui.panel_width, 400.0);
    // No narrower than its cards are drawn for, nor wider than its limit.
    drag(&mut ui, 1300.0);
    assert_eq!(ui.panel_width, look::PANEL_WIDTH);
    drag(&mut ui, 400.0);
    assert_eq!(ui.panel_width, 560.0);
    assert!(ui.panel_open);
    update(
        &store,
        &mut ui,
        &UiAction::Clicked {
            id: view::PANEL_HANDLE.into(),
        },
    );
    assert!(!ui.panel_open);
}

#[test]
fn tab_menu_renames_closes_others_and_keys_move_tabs() {
    let store = Store::default();
    let mut ui = Ui::new("/tmp".into());
    ui.terminal.tabs = vec![tab("Terminal 1", true), tab("Terminal 2", false)];
    let click = |ui: &mut Ui, id: &str| {
        update(&store, ui, &UiAction::Clicked { id: id.into() });
    };
    click(&mut ui, "terminal:menu:rename:1");
    update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::TERMINAL_RENAME.into(),
            value: "build".into(),
        },
    );
    assert_eq!(
        ui.terminal.requests,
        [TerminalRequest::Rename(1, Some("build".into()))]
    );
    ui.terminal.requests.clear();
    // Another tab runs a command: closing the others asks first, with Desktop's wording.
    click(&mut ui, "terminal:menu:others:1");
    assert_eq!(ui.terminal.closing, Some(Closing::Others(1)));
    click(&mut ui, view::TERMINAL_CLOSE_CONFIRM);
    assert_eq!(ui.terminal.requests, [TerminalRequest::CloseOthers(1)]);
    ui.terminal.requests.clear();
    assert!(terminal_key(
        &mut ui,
        Some("terminal:tab:1"),
        limn::Key::ArrowLeft,
        true,
        true
    ));
    assert_eq!(
        ui.terminal.requests,
        [TerminalRequest::Move { from: 1, to: 0 }]
    );
    assert!(!terminal_key(
        &mut ui,
        Some("composer"),
        limn::Key::Delete,
        false,
        false
    ));
}

#[test]
fn steps_are_named_in_desktops_words() {
    use view::{group_summary, line_diff, past_tense, progressive};
    // From Desktop's transcript.
    assert_eq!(
        past_tense("Run saludo.py to verify output"),
        "Ran saludo.py to verify output"
    );
    assert_eq!(
        past_tense("Create atajo.txt in /work and confirm it exists"),
        "Created atajo.txt in /work and confirm it exists"
    );
    assert_eq!(
        progressive("Create atajo2.txt in /work"),
        "Creating atajo2.txt in /work"
    );
    assert_eq!(past_tense("python3 x.py"), "python3 x.py");
    let done = ToolResult {
        output: String::new(),
        is_error: false,
        added: 1,
        removed: 1,
        files: vec![],
        stopped: false,
        web: None,
        images: vec![],
    };
    let read = serde_json::json!({ "file_path": "/work/saludo.py" });
    let bash =
        serde_json::json!({ "command": "python3 saludo.py", "description": "Run saludo.py" });
    let steps = [
        view::Step {
            call: "1",
            name: "Read",
            input: &read,
            result: Some(&done),
            running: false,
        },
        view::Step {
            call: "2",
            name: "Edit",
            input: &read,
            result: Some(&done),
            running: false,
        },
        view::Step {
            call: "3",
            name: "Bash",
            input: &bash,
            result: Some(&done),
            running: false,
        },
    ];
    assert_eq!(
        group_summary(&steps),
        "Read and edited saludo.py, ran a command"
    );
    assert_eq!(group_summary(&steps[2..]), "Ran saludo.py");
    let lines = line_diff("a\nb\nc\n", "a\nB\nc\n");
    let kinds: Vec<_> = lines.iter().map(|l| l.kind).collect();
    use limn::CodeDiffLineKind::*;
    assert_eq!(kinds, [Context, Removed, Added, Context]);
}

#[test]
fn limits_reset_as_desktop_says_it() {
    use view::resets_in;
    let now = 1_792_900_000;
    assert_eq!(
        resets_in(now + 4 * 3_600 + 53 * 60, now, 0),
        "Resets in 4 hr 53 min"
    );
    assert_eq!(resets_in(now + 300, now, 0), "Resets in 5 min");
    // A window already over is not counted down.
    assert_eq!(resets_in(now - 60, now, 0), "");
    // 1793512800 is Sunday 2026-11-01 06:00 UTC: 3:00 AM at UTC-3.
    assert_eq!(
        resets_in(1_793_512_800, now, -3 * 3_600),
        "Resets Sun 3:00 AM"
    );
}

#[test]
fn running_clock_reads_as_desktop_writes_it() {
    assert_eq!(view::elapsed(14), "14s");
    assert_eq!(view::elapsed(9 * 60 + 4), "9m 4s");
}

#[test]
fn every_icon_is_a_complete_svg_on_the_20_grid() {
    let sheet = std::env::var_os("NEWEN_ICON_SHEET");
    for (name, svg) in icons::ALL {
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"), "{name}");
        assert!(svg.contains("viewBox='0 0 20 20'"), "{name}");
        if let Some(dir) = &sheet {
            std::fs::write(std::path::Path::new(dir).join(format!("{name}.svg")), svg).unwrap();
        }
    }
}

fn tool(store: &mut Store, call: &str, name: &str, input: serde_json::Value, parent: Option<&str>) {
    agent(
        store,
        AgentEvent::ToolUse {
            call: call.into(),
            name: name.into(),
            input,
            parent: parent.map(str::to_owned),
        },
    );
}

fn result(store: &mut Store, call: &str, output: &str) {
    agent(
        store,
        AgentEvent::ToolResult {
            call: call.into(),
            output: output.into(),
            is_error: false,
            files: vec![],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    );
}

#[test]
fn the_task_tools_keep_a_list_and_no_steps() {
    let (mut store, _) = store_with_session();
    for (call, subject) in [("c1", "Create a.txt"), ("c2", "List the files")] {
        tool(
            &mut store,
            call,
            "TaskCreate",
            serde_json::json!({ "subject": subject }),
            None,
        );
    }
    result(
        &mut store,
        "c1",
        "Task #1 created successfully: Create a.txt",
    );
    result(
        &mut store,
        "c2",
        "Task #2 created successfully: List the files",
    );
    tool(
        &mut store,
        "u1",
        "TaskUpdate",
        serde_json::json!({ "taskId": "1", "status": "completed" }),
        None,
    );
    tool(
        &mut store,
        "u2",
        "TaskUpdate",
        serde_json::json!({ "taskId": "2", "status": "in_progress" }),
        None,
    );
    let list = view::todos(&store.session(1).unwrap().items);
    let shown: Vec<_> = list
        .iter()
        .map(|t| (t.subject.as_str(), t.status.as_str()))
        .collect();
    assert_eq!(
        shown,
        [
            ("Create a.txt", "completed"),
            ("List the files", "in_progress")
        ]
    );
    assert!(view::is_task_tool("TaskUpdate") && !view::is_task_tool("Bash"));
}

#[test]
fn a_helper_agent_keeps_its_steps_and_counts_as_running_work() {
    let (mut store, _) = store_with_session();
    tool(
        &mut store,
        "a1",
        "Agent",
        serde_json::json!({ "description": "List files in folder" }),
        None,
    );
    agent(
        &mut store,
        AgentEvent::TaskStarted {
            id: "t1".into(),
            call: Some("a1".into()),
            description: "List files in folder".into(),
            kind: "agent".into(),
        },
    );
    tool(
        &mut store,
        "b1",
        "Bash",
        serde_json::json!({ "command": "ls" }),
        Some("a1"),
    );
    result(&mut store, "b1", "README.md");
    let session = store.session(1).unwrap();
    // The helper's step is its own, not the conversation's, and it got its result.
    assert!(!session
        .items
        .iter()
        .any(|i| matches!(i, Item::Tool { call, .. } if call == "b1")));
    assert!(matches!(
        &session.helpers["a1"][..],
        [Item::Tool {
            result: Some(_),
            ..
        }]
    ));
    assert!(session.task_running("a1"));
    let input = serde_json::json!({ "description": "List files in folder" });
    let step = view::Step {
        call: "a1",
        name: "Agent",
        input: &input,
        result: None,
        running: true,
    };
    assert_eq!(view::group_summary(&[step]), "Listing files in folder");
    let none = serde_json::json!({});
    let planning = view::Step {
        call: "p1",
        name: "EnterPlanMode",
        input: &none,
        result: None,
        running: true,
    };
    assert_eq!(view::group_summary(&[planning]), "Making a plan");
    let stopped = ToolResult {
        output: "Successfully stopped task: b7 (npm run dev)".into(),
        is_error: false,
        added: 0,
        removed: 0,
        files: vec![],
        stopped: false,
        web: None,
        images: vec![],
    };
    let stop = view::Step {
        call: "k1",
        name: "KillShell",
        input: &none,
        result: Some(&stopped),
        running: false,
    };
    assert_eq!(view::group_summary(&[stop]), "Stopped task npm run dev");
    let read = "<status>completed</status>\n<exit_code>0</exit_code>\n<stdout>\nready\n</stdout>";
    assert_eq!(view::tagged(read, "stdout"), Some("ready"));
    assert_eq!(view::tagged(read, "exit_code"), Some("0"));
    assert_eq!(view::tagged(read, "stderr"), None);
    agent(
        &mut store,
        AgentEvent::TaskEnded {
            id: "t1".into(),
            call: Some("a1".into()),
            summary: Some("Two files.".into()),
            ok: true,
            stopped: false,
            usage: None,
        },
    );
    assert!(!store.session(1).unwrap().task_running("a1"));
}

#[test]
fn a_finished_group_counts_agents_and_other_tools() {
    let done = ToolResult {
        output: String::new(),
        is_error: false,
        added: 0,
        removed: 0,
        files: vec![],
        stopped: false,
        web: None,
        images: vec![],
    };
    let read = serde_json::json!({ "file_path": "/work/saludo.py" });
    let agent_input = serde_json::json!({ "description": "List files" });
    let other = serde_json::json!({});
    let step = |call, name, input| view::Step {
        call,
        name,
        input,
        result: Some(&done),
        running: false,
    };
    let steps = [
        step("1", "Read", &read),
        step("2", "Agent", &agent_input),
        step("3", "WebFetch", &other),
        step("4", "Skill", &other),
    ];
    assert_eq!(
        view::group_summary(&steps),
        "Read saludo.py, ran an agent, used 2 tools"
    );
}

#[test]
fn the_running_line_writes_time_and_tokens_as_desktop() {
    assert_eq!(view::elapsed(3_723), "1h 2m 3s");
    assert_eq!(view::tokens(140), "140 tokens");
    assert_eq!(view::tokens(30_712), "30.7k tokens");
}

#[test]
fn an_answer_is_read_back_from_the_tool_result() {
    let output = "The user answered: \"Pick red or blue?\"=\"blue\". Read the answers carefully.";
    assert_eq!(
        view::answered(output, "Pick red or blue?").as_deref(),
        Some("blue")
    );
    assert_eq!(view::answered(output, "Another?"), None);
}

#[test]
fn questions_page_take_several_choices_their_own_words_and_skips() {
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::PermissionRequest {
            id: "p1".into(),
            call: None,
            tool: "AskUserQuestion".into(),
            input: serde_json::json!({ "questions": [
                { "question": "Pets?", "multiSelect": true,
                  "options": [{ "label": "Cat" }, { "label": "Dog" }, { "label": "Fish" }] },
                { "question": "Color?", "options": [{ "label": "Red" }, { "label": "Blue" }] },
                { "question": "Size?", "options": [{ "label": "S" }] }] }),
            title: None,
            interactive: true,
            suggestions: vec![],
        },
    );
    let click = |ui: &mut Ui, id: &str| update(&store, ui, &UiAction::Clicked { id: id.into() });
    click(&mut ui, "ask:p1:0:0");
    click(&mut ui, "ask:p1:0:2");
    click(&mut ui, "ask:p1:0:1");
    click(&mut ui, "ask:p1:0:1");
    assert!(click(&mut ui, "ask:p1:next").is_empty());
    // "Other" is the option after the last, and takes what the person wrote.
    click(&mut ui, "ask:p1:1:2");
    update(
        &store,
        &mut ui,
        &UiAction::InputChanged {
            id: "ask-other:p1:1".into(),
            value: "Green".into(),
        },
    );
    let click = |ui: &mut Ui, id: &str| update(&store, ui, &UiAction::Clicked { id: id.into() });
    assert!(click(&mut ui, "ask:p1:next").is_empty());
    let intents = click(&mut ui, "ask:p1:skip");
    let [Intent::Decide {
        decision: Decision::Answer(answers),
        ..
    }] = intents.as_slice()
    else {
        panic!("{intents:?}")
    };
    assert_eq!(answers["Pets?"], "Cat, Fish");
    assert_eq!(answers["Color?"], "Green");
    assert_eq!(answers["Size?"], "[No preference]");
}

/// A question may allow no "Other", mask its words, or take words only; one the agent goes on
/// without counts down on its card once it waited untouched, and is answered with nothing at
/// the end, unless the person touched it.
#[test]
fn a_question_may_be_secret_words_only_and_go_unanswered() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Awaiting,
    });
    let asked = |id: &str, blocking: bool| AgentEvent::PermissionRequest {
        id: id.into(),
        call: None,
        tool: "AskUserQuestion".into(),
        input: serde_json::json!({ "blocking": blocking, "questions": [
            { "question": "Paste the API token", "options": [], "other": false, "secret": true },
            { "question": "Red or blue?", "other": false,
              "options": [{ "label": "Red" }, { "label": "Blue" }] }] }),
        title: None,
        interactive: true,
        suggestions: vec![],
    };
    agent(&mut store, asked("p1", true));
    let typed = |ui: &mut Ui, value: &str| {
        update(
            &store,
            ui,
            &UiAction::InputChanged {
                id: "ask-other:p1:0".into(),
                value: value.into(),
            },
        )
    };
    typed(&mut ui, "hunter2");
    let window = limn::Size::new(1408.0, 844.0);
    let drawn = |store: &Store, ui: &Ui, words: &str| {
        let frame = UiTree::new(view(store, ui, window)).layout(window);
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    assert!(drawn(&store, &ui, "•••••••") && !drawn(&store, &ui, "hunter2"));
    click(&store, &mut ui, "ask:p1:next");
    assert!(!drawn(&store, &ui, "Other"));
    let intents = click(&store, &mut ui, "ask:p1:submit");
    let [Intent::Decide {
        decision: Decision::Answer(answers),
        ..
    }] = intents.as_slice()
    else {
        panic!("{intents:?}")
    };
    assert_eq!(answers["Paste the API token"], "hunter2");
    assert_eq!(answers["Red or blue?"], "Red");
    // What was typed is not kept once it is sent.
    assert!(ui.other.is_empty());
    // A blocking question never counts down.
    ui.tick(&store);
    assert!(ui.unanswered.is_empty());
    // One the agent goes on without: its clock starts as it comes, its card counts down once
    // it waited a minute, and at two it is answered with nothing.
    agent(&mut store, asked("p2", false));
    ui.tick(&store);
    assert!(matches!(ui.unanswered.get("p2"), Some((1, Some(_)))));
    assert!(!drawn(&store, &ui, "Skipped in"));
    let ago = |secs| std::time::Instant::now().checked_sub(std::time::Duration::from_secs(secs));
    ui.unanswered.insert("p2".into(), (1, ago(90)));
    assert!(drawn(&store, &ui, "Skipped in 30s"));
    assert!(ui.due().is_empty());
    ui.unanswered.insert("p2".into(), (1, ago(121)));
    assert_eq!(
        ui.due(),
        [Intent::Decide {
            session: 1,
            permission: "p2".into(),
            decision: Decision::Answer(serde_json::json!({})),
        }]
    );
    // Touched, it waits for the person.
    agent(&mut store, asked("p3", false));
    ui.tick(&store);
    ui.unanswered.insert("p3".into(), (1, ago(121)));
    click(&store, &mut ui, "ask:p3:1:1");
    assert!(ui.due().is_empty());
    assert!(!drawn(&store, &ui, "Skipped in"));
}

#[test]
fn accepting_a_plan_allows_it_and_sets_the_mode() {
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::PermissionRequest {
            id: "p2".into(),
            call: Some("x1".into()),
            tool: "ExitPlanMode".into(),
            input: serde_json::json!({ "plan": "# Plan" }),
            title: None,
            interactive: true,
            suggestions: vec![],
        },
    );
    assert_eq!(
        shortcut(&store, &ui, limn::Key::Enter, true, true).as_deref(),
        Some("planok:p2:default")
    );
    let intents = update(
        &store,
        &mut ui,
        &UiAction::Clicked {
            id: "planok:p2:acceptEdits".into(),
        },
    );
    assert!(matches!(
        intents.as_slice(),
        [
            Intent::Decide {
                decision: Decision::Allow,
                ..
            },
            Intent::SetMode { mode, .. }
        ] if mode == "acceptEdits"
    ));
    assert_eq!(
        view::plan_text(&store.session(1).unwrap().items, "p2").as_deref(),
        Some("# Plan")
    );
}

fn click(store: &Store, ui: &mut Ui, id: &str) -> Vec<Intent> {
    update(store, ui, &UiAction::Clicked { id: id.into() })
}

/// An agent in a planning mode that ends its turn with the plan (none of its own is asked):
/// the card waits under the turn; "Implement in …" goes back to the mode the session left and
/// says the words that have the plan carried out, kept as the person's message; "Revise…" says
/// what should change, still planning. Claude Code's plan, which it asks about, is not this card.
#[test]
fn a_plan_that_ends_the_turn_is_carried_out_by_a_turn_of_its_own() {
    let mut store = Store::default();
    let mut ui = Ui::new("/work".into());
    ui.selected = Some(1);
    ui.skip_welcome = true;
    let word = |id: &str, label: &str, tone| newen_model::ModeWords {
        id: id.into(),
        label: label.into(),
        about: String::new(),
        tone,
        listed: true,
    };
    use newen_model::ModeTone;
    ui.agents = vec![newen_model::AgentInfo {
        id: "codex".into(),
        name: "Codex".into(),
        tested: vec![],
        options: vec![],
        icon: None,
        sign_in_methods: vec![],
        sign_in_code: false,
        modes: vec![
            word("read-only", "Read only", ModeTone::Read),
            word("auto", "Auto", ModeTone::Auto),
            word("plan", "Plan", ModeTone::Plan),
        ],
        default_mode: "auto".into(),
        customize: Default::default(),
    }];
    store.apply(Event::SessionCreated {
        session: 1,
        agent: "codex".into(),
        cwd: "/work".into(),
    });
    let modes = ["read-only", "auto", "plan"].map(String::from).to_vec();
    agent(
        &mut store,
        AgentEvent::Ready {
            version: "0.163.0".into(),
            capabilities: AgentCapabilities {
                modes,
                ..Default::default()
            },
            mode: Some("read-only".into()),
        },
    );
    agent(
        &mut store,
        AgentEvent::ModeChanged {
            mode: "plan".into(),
        },
    );
    store.apply(Event::Queued {
        session: 1,
        request: "r1".into(),
        text: "Plan hello.txt".into(),
        at: None,
        images: vec![],
    });
    agent(
        &mut store,
        AgentEvent::ToolUse {
            call: "t1-plan".into(),
            name: "ExitPlanMode".into(),
            input: serde_json::json!({ "plan": "# Add hello.txt" }),
            parent: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::ToolResult {
            call: "t1-plan".into(),
            output: String::new(),
            is_error: false,
            files: vec![],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    );
    agent(
        &mut store,
        AgentEvent::TurnFinished {
            ok: true,
            interrupted: false,
            usage: Usage::default(),
            error: None,
        },
    );
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Idle,
    });
    let session = store.session(1).unwrap();
    assert_eq!(view::proposed_plan(session, &ui), Some("t1-plan"));
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let drawn = |words: &str| {
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    assert!(drawn("Codex proposed a plan") && drawn("Implement in Read only"));
    assert!(!drawn("Reject"));
    let intents = click(&store, &mut ui, "planok:t1-plan:read-only");
    assert_eq!(
        intents,
        [
            Intent::SetMode {
                session: 1,
                mode: "read-only".into()
            },
            Intent::Send {
                session: 1,
                text: IMPLEMENT_PLAN.into(),
                mentions: vec![],
                images: vec![],
                lane: Lane::default(),
            }
        ]
    );
    click(&store, &mut ui, "revise:t1-plan");
    let revised = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: "revise-text:t1-plan".into(),
            value: " Keep it to one line ".into(),
        },
    );
    assert!(matches!(&revised[..], [Intent::Send { text, .. }] if text == "Keep it to one line"));
    // Once the person said something, or left planning, it waits no more.
    let session = store.session(1).unwrap();
    let mut left = session.clone();
    left.mode = "auto".into();
    assert_eq!(view::proposed_plan(&left, &ui), None);
    let mut answered = session.clone();
    answered.items.push(Item::User {
        request: "r2".into(),
        text: IMPLEMENT_PLAN.into(),
        at: None,
        images: vec![],
    });
    assert_eq!(view::proposed_plan(&answered, &ui), None);
    // A plan the agent asks about is its permission's card.
    let mut asked = session.clone();
    asked.items.push(Item::Permission {
        id: "p1".into(),
        call: Some("t1-plan".into()),
        tool: "ExitPlanMode".into(),
        input: serde_json::json!({ "plan": "# Add hello.txt" }),
        title: None,
        interactive: true,
        suggestions: vec![],
        decided: Some("Allowed"),
    });
    assert_eq!(view::proposed_plan(&asked, &ui), None);
}

#[test]
fn a_session_is_renamed_pinned_and_deleted_from_its_menus() {
    let (mut store, mut ui) = store_with_session();
    // Rename starts from the current title, and Enter sends what the field holds.
    click(&store, &mut ui, "rename:1");
    assert_eq!(ui.title_edit.as_deref(), Some("Fix the bug"));
    let renamed = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::TITLE_FIELD.into(),
            value: " Bug hunt ".into(),
        },
    );
    assert_eq!(
        renamed,
        vec![Intent::Rename {
            session: 1,
            title: "Bug hunt".into()
        }]
    );
    assert!(ui.title_edit.is_none());
    store.apply(Event::Renamed {
        session: 1,
        title: "Bug hunt".into(),
    });
    assert_eq!(store.session(1).unwrap().title, "Bug hunt");

    assert_eq!(
        click(&store, &mut ui, "pin:1"),
        vec![Intent::Pin {
            session: 1,
            pinned: true
        }]
    );
    store.apply(Event::Pinned {
        session: 1,
        pinned: true,
    });
    assert_eq!(
        click(&store, &mut ui, "pin:1"),
        vec![Intent::Pin {
            session: 1,
            pinned: false
        }]
    );

    // A delete asks first; only the confirm reaches the engine.
    assert!(click(&store, &mut ui, "delete:1").is_empty());
    assert_eq!(ui.deleting, Some(1));
    assert!(click(&store, &mut ui, view::DELETE_CANCEL).is_empty());
    click(&store, &mut ui, "delete:1");
    assert_eq!(
        click(&store, &mut ui, view::DELETE_CONFIRM),
        vec![Intent::Delete { session: 1 }]
    );
    let deleted = Event::Deleted { session: 1 };
    store.apply(deleted.clone());
    ui.applied(&deleted);
    assert!(store.sessions.is_empty());
    assert_eq!(ui.selected, None);
}

#[test]
fn search_lists_waiting_sessions_first_and_enter_opens_the_first() {
    let mut store = Store::default();
    for (session, title, status) in [
        (1, "Fix login", SessionStatus::Idle),
        (2, "Fix logout", SessionStatus::Awaiting),
        (3, "Write docs", SessionStatus::Running),
    ] {
        store.apply(Event::SessionCreated {
            session,
            agent: "claude".into(),
            cwd: "/work".into(),
        });
        store.apply(Event::Renamed {
            session,
            title: title.into(),
        });
        store.apply(Event::Status { session, status });
    }
    assert_eq!(view::found(&store, ""), vec![2, 3, 1]);
    assert_eq!(view::found(&store, "FIX"), vec![2, 1]);
    let mut ui = Ui::new("/work".into());
    click(&store, &mut ui, view::SEARCH);
    update(
        &store,
        &mut ui,
        &UiAction::InputChanged {
            id: view::SEARCH_FIELD.into(),
            value: "login".into(),
        },
    );
    update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::SEARCH_FIELD.into(),
            value: "login".into(),
        },
    );
    assert_eq!(ui.selected, Some(1));
    assert!(ui.search.is_none());
}

#[test]
fn a_hidden_sidebar_gives_its_width_to_the_conversation() {
    let (store, mut ui) = store_with_session();
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    tree.layout(window);
    click(&store, &mut ui, view::SIDEBAR_TOGGLE);
    assert!(!ui.sidebar.open);
    // It slides away on a spring and has nothing left in a second.
    ui.tick(&store);
    let later = std::time::Instant::now() + std::time::Duration::from_secs(1);
    assert_eq!(ui.sidebar.shown.target(), 0.0);
    assert_eq!(ui.sidebar.shown.value(later), 0.0);
    tree.set_root(view(&store, &ui, window));
    tree.layout(window);
}

#[test]
fn an_archived_session_leaves_the_list_and_the_search_still_finds_it() {
    let (mut store, mut ui) = store_with_session();
    assert_eq!(
        click(&store, &mut ui, "archive:1"),
        vec![Intent::Archive {
            session: 1,
            archived: true
        }]
    );
    store.apply(Event::Archived {
        session: 1,
        archived: true,
    });
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    let frame = tree.layout(window);
    assert!(!frame.regions.iter().any(|r| r.id == "session:1"));
    assert_eq!(view::found(&store, "bug"), vec![1]);
    assert_eq!(
        click(&store, &mut ui, "archive:1"),
        vec![Intent::Archive {
            session: 1,
            archived: false
        }]
    );
}

#[test]
fn a_turn_that_ends_unseen_leaves_the_session_unread_until_it_is_opened() {
    let (mut store, mut ui) = store_with_session();
    let finished = Event::Agent {
        session: 1,
        request: Some("r1".into()),
        event: AgentEvent::TurnFinished {
            ok: true,
            interrupted: false,
            usage: Usage::default(),
            error: None,
        },
    };
    ui.selected = None;
    // Read back at start, a finished turn is old news.
    assert!(ui.applied(&finished).is_empty());
    ui.applied(&Event::Status {
        session: 1,
        status: SessionStatus::Running,
    });
    assert_eq!(
        ui.applied(&finished),
        vec![Intent::MarkUnread {
            session: 1,
            unread: true
        }]
    );
    store.apply(Event::Unread {
        session: 1,
        unread: true,
    });
    let opened = click(&store, &mut ui, "session:1");
    assert!(opened.contains(&Intent::MarkUnread {
        session: 1,
        unread: false
    }));
}

/// The sidebar row's dot (6 px fills inside the row) and the face of its title.
fn row_marks(store: &Store, ui: &Ui) -> (Vec<limn::Color>, Vec<limn::FontFamily>) {
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(store, ui, window)).layout(window);
    let row = region(&frame, "session:1").expect("the session's row");
    let inside = |x: f32, y: f32| {
        x >= row.x && x < row.x + row.width && y >= row.y && y < row.y + row.height
    };
    let mut dots = vec![];
    let mut faces = vec![];
    for command in &frame.commands {
        match command {
            limn::DrawCommand::FillRect { rect, color, .. }
                if rect.width == 6.0 && rect.height == 6.0 && inside(rect.x, rect.y) =>
            {
                dots.push(*color)
            }
            limn::DrawCommand::Text {
                text,
                position,
                font_family,
                ..
            } if text == "Fix the bug" && inside(position.x, position.y) => {
                faces.push(*font_family)
            }
            _ => {}
        }
    }
    (dots, faces)
}

/// A turn that ended unseen leaves a blue dot at the row's end, its title at its usual
/// weight; opening the session clears it and "Mark as unread" brings it back. One that
/// needs the person carries an amber dot.
#[test]
fn an_unread_or_waiting_session_shows_a_dot_not_a_bold_title() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Idle,
    });
    ui.selected = None;
    settle(&store, &mut ui);
    let (dots, faces) = row_marks(&store, &ui);
    assert!(dots.is_empty(), "read: no dot");
    assert_eq!(faces, [limn::FontFamily::Sans]);
    store.apply(Event::Unread {
        session: 1,
        unread: true,
    });
    let (dots, faces) = row_marks(&store, &ui);
    assert_eq!(dots, [look::theme().accent]);
    assert_eq!(faces, [limn::FontFamily::Sans], "not bold");
    // Opened, it is read.
    let opened = click(&store, &mut ui, "session:1");
    assert!(opened.contains(&Intent::MarkUnread {
        session: 1,
        unread: false
    }));
    store.apply(Event::Unread {
        session: 1,
        unread: false,
    });
    assert!(row_marks(&store, &ui).0.is_empty());
    // "Mark as unread" brings the dot back.
    assert_eq!(
        click(&store, &mut ui, "unread:1"),
        vec![Intent::MarkUnread {
            session: 1,
            unread: true
        }]
    );
    store.apply(Event::Unread {
        session: 1,
        unread: true,
    });
    assert_eq!(row_marks(&store, &ui).0, [look::theme().accent]);
    // Waiting on the person, the dot is amber.
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Awaiting,
    });
    assert_eq!(row_marks(&store, &ui).0, [look::awaiting()]);
}

#[test]
fn a_fork_starts_with_its_parents_conversation_and_opens() {
    let (mut store, mut ui) = store_with_session();
    assert_eq!(
        click(&store, &mut ui, "fork:1"),
        vec![Intent::Fork {
            session: 1,
            at: None
        }]
    );
    let created = Event::SessionCreated {
        session: 2,
        agent: "claude".into(),
        cwd: "/work".into(),
    };
    assert_eq!(
        ui.applied(&created),
        vec![Intent::WorkingTree { session: 2 }]
    );
    assert_eq!(ui.selected, Some(2));
    store.apply(created);
    store.apply(Event::Forked {
        session: 2,
        from: 1,
        at: None,
    });
    let fork = store.session(2).unwrap();
    assert_eq!(fork.items, store.session(1).unwrap().items);
    assert_eq!(fork.title, "Fix the bug (fork)");
}

#[test]
fn a_link_opens_in_the_sessions_browser_and_its_menu_closes() {
    let (store, mut ui) = store_with_session();
    assert_eq!(
        view::link_url("link:ab12:7:https://example.com/a:b"),
        Some("https://example.com/a:b")
    );
    assert_eq!(
        click(&store, &mut ui, "link:ab12:7:https://example.com"),
        vec![Intent::Browser {
            session: 1,
            op: newen_model::BrowserOp::Open {
                url: Some("https://example.com".into())
            }
        }]
    );
    assert!(ui.pane_open(Pane::Browser));
    ui.link_menu = Some(("https://example.com".into(), limn::Point::new(10.0, 10.0)));
    assert_eq!(
        click(&store, &mut ui, "link-copy:https://example.com"),
        vec![]
    );
    assert!(ui.link_menu.is_none());
}

#[test]
fn a_custom_connector_is_an_address_or_a_command_with_its_settings() {
    let mut adding = crate::AddServer {
        agents: vec!["claude".into()],
        name: " ctx ".into(),
        target: "https://mcp.context7.com/mcp".into(),
        env: "A=1\nnot a pair".into(),
        headers: "X-Key: 1".into(),
        ..Default::default()
    };
    let server = adding.server().unwrap();
    assert!(server.remote && server.name == "ctx" && server.args.is_empty());
    assert_eq!(
        (server.env, server.headers),
        (vec!["A=1".to_owned()], vec!["X-Key: 1".to_owned()])
    );
    adding.target = "npx -y @x/pg <database_url>".into();
    let server = adding.server().unwrap();
    assert!(!server.remote && server.target == "npx" && server.headers.is_empty());
    assert_eq!(server.args, ["-y", "@x/pg", "<database_url>"]);
    adding.name.clear();
    assert!(adding.server().is_none());
}

/// "Add custom connector" warns of an address that hides something, or whose host is not the
/// name's site; takes plain http only on this computer; and gives an OAuth client of the
/// person's own to the server it adds.
#[test]
fn a_custom_connectors_address_is_warned_about_and_https_unless_local() {
    let warned = |name: &str, url: &str| crate::url_warnings(name, url);
    assert!(warned("github", "https://api.githubcopilot.com/mcp/").is_empty());
    assert!(warned("my-server", "https://example.com/mcp").is_empty());
    assert!(warned("github", "https://git\u{200b}hub.com/mcp")[0].contains("hidden characters"));
    assert!(warned("github", "https://github.com@evil.example/mcp")
        .iter()
        .any(|w| w.contains("before its host")));
    assert!(warned("github", "https://evil.example/mcp")[0]
        .contains("evil.example doesn\u{2019}t look like github\u{2019}s site"));
    assert!(warned("paypal", "https://p\u{0430}ypal.com/mcp")
        .iter()
        .any(|w| w.contains("outside plain ASCII")));
    // A command is not an address.
    assert!(warned("github", "npx -y server-github").is_empty());
    let mut adding = crate::AddServer {
        agents: vec!["claude".into()],
        name: "linear".into(),
        target: "http://mcp.linear.app/mcp".into(),
        ..Default::default()
    };
    assert!(adding.refused().is_some());
    adding.target = "http://localhost:3000/mcp".into();
    assert!(adding.refused().is_none());
    adding.target = "https://mcp.linear.app/mcp".into();
    adding.client_id = " abc ".into();
    adding.client_secret = "s3cret".into();
    let server = adding.server().unwrap();
    assert_eq!(server.client_id.as_deref(), Some("abc"));
    assert_eq!(server.client_secret.as_deref(), Some("s3cret"));
    // An address refused is not added from the dialog.
    let (store, mut ui) = two_agents_on(view::SETTINGS_CONNECTORS);
    ui.connectors.adding = Some(crate::AddServer {
        target: "http://example.com/mcp".into(),
        ..adding
    });
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    for c in &frame.commands {
        if let limn::DrawCommand::Text { text, .. } = c {
            eprintln!("T {text}");
        }
    }
    assert!(drawn(&store, &ui, "Use an https:// address"));
    assert!(click(&store, &mut ui, "srv:save").is_empty());
    assert!(ui.connectors.adding.is_some());
}

#[test]
fn inactive_sessions_are_the_ones_quiet_for_the_days_chosen() {
    let (mut store, _) = store_with_session();
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Idle,
    });
    store.apply(Event::Queued {
        session: 1,
        request: "r2".into(),
        text: "later".into(),
        at: Some(10 * 86_400),
        images: vec![],
    });
    assert!(crate::inactive(&store, 0, 100 * 86_400).is_empty());
    assert_eq!(crate::inactive(&store, 7, 20 * 86_400), [1]);
    assert!(crate::inactive(&store, 30, 20 * 86_400).is_empty());
    store.apply(Event::Pinned {
        session: 1,
        pinned: true,
    });
    assert!(crate::inactive(&store, 7, 20 * 86_400).is_empty());
}

#[test]
fn an_adapters_switch_flips_and_its_list_chooses() {
    let (store, mut ui) = store_with_session();
    let option = |key: &str, choices: &[(&str, &str)], default: &str| newen_model::AgentOption {
        key: key.into(),
        label: key.into(),
        description: String::new(),
        choices: choices
            .iter()
            .map(|(a, b)| ((*a).into(), (*b).into()))
            .collect(),
        default: default.into(),
    };
    ui.agents = vec![newen_model::AgentInfo {
        id: "claude".into(),
        name: "Claude Code".into(),
        tested: vec![],
        options: vec![
            option("sandbox", &[], "false"),
            option("style", &[("", "Default"), ("x", "X")], ""),
        ],
        icon: None,
        sign_in_methods: vec![],
        sign_in_code: true,
        modes: claude_modes(),
        default_mode: "default".into(),
        customize: Default::default(),
    }];
    click(&store, &mut ui, view::SETTINGS_OPEN);
    let saved = |intents: Vec<Intent>| match intents.as_slice() {
        [Intent::SaveSettings(s)] => s.agent("claude").options,
        other => panic!("{other:?}"),
    };
    let flipped = saved(click(&store, &mut ui, "set:agent:claude:opt.sandbox"));
    assert_eq!(flipped.get("sandbox").map(String::as_str), Some("true"));
    let chosen = saved(click(&store, &mut ui, "set:agent:claude:opt.style=x"));
    assert_eq!(chosen.get("style").map(String::as_str), Some("x"));
}

#[test]
fn each_session_keeps_its_model_and_effort() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::SessionCreated {
        session: 2,
        agent: "claude".into(),
        cwd: "/work".into(),
    });
    for (session, model, effort) in [(1, "opus", "high"), (2, "haiku", "low")] {
        store.apply(Event::ModelChosen {
            session,
            model: model.into(),
        });
        store.apply(Event::EffortChosen {
            session,
            effort: effort.into(),
        });
    }
    click(&store, &mut ui, "session:2");
    assert_eq!(
        (ui.model.as_deref(), ui.effort.as_deref()),
        (Some("haiku"), Some("low"))
    );
    click(&store, &mut ui, "session:1");
    assert_eq!(
        (ui.model.as_deref(), ui.effort.as_deref()),
        (Some("opus"), Some("high"))
    );
}

#[test]
fn a_changed_file_folds_and_opens_in_files_or_elsewhere() {
    use newen_model::{ChangeKind, FileEffect};
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::Changes {
        session: 1,
        request: "r1".into(),
        files: vec![FileEffect {
            path: "src/a.rs".into(),
            kind: ChangeKind::Modified,
            added: 1,
            removed: 0,
            patch: "@@ -1,1 +4,2 @@\n a\n+b\n".into(),
        }],
        snapshot: "s".into(),
    });
    click(&store, &mut ui, "changes:r1:view");
    settle(&store, &mut ui);
    let window = limn::Size::new(1408.0, 844.0);
    let shows = |ui: &Ui, id: &str| {
        let frame = UiTree::new(view(&store, ui, window)).layout(window);
        frame.regions.iter().any(|r| r.id == id)
    };
    assert!(shows(&ui, "diff:0"));
    click(&store, &mut ui, "diff:fold:src/a.rs");
    assert!(!shows(&ui, "diff:0"));
    click(&store, &mut ui, "diff:fold:src/a.rs");
    assert!(shows(&ui, "diff:0") && shows(&ui, "doc:open-at:4:src/a.rs"));
    // Its menu lists the session's places for this file and line.
    click(&store, &mut ui, "open-file-menu:src/a.rs");
    assert_eq!(ui.diff_menu.as_deref(), Some("src/a.rs"));
    // In Newen, it opens in the Files pane, the caret going to its line once read.
    assert!(
        click(&store, &mut ui, "doc:open-at:4:src/a.rs").contains(&Intent::ReadFile {
            session: 1,
            path: "src/a.rs".into()
        })
    );
    assert!(ui.pane_open(Pane::Files));
    assert_eq!(ui.files[&1].goto, Some(("src/a.rs".into(), 3)));
}

/// A workflow that runs on after its session's turn is a row under that session with its name,
/// its phase and how many of that phase's agents ended; the session keeps a pulsing dot, the
/// row opens the phases in the Background tasks pane, and once it ends the row leaves.
#[test]
fn a_running_workflow_is_a_row_under_its_session_that_opens_its_phases() {
    use newen_model::{TaskAgent, TaskPhase, TaskState};
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Idle,
    });
    ui.selected = None;
    agent(
        &mut store,
        AgentEvent::TaskStarted {
            id: "w1".into(),
            call: None,
            description: "Read and sum up".into(),
            kind: "workflow".into(),
        },
    );
    let worker = |label: &str, state| TaskAgent {
        label: label.into(),
        state,
        doing: None,
    };
    agent(
        &mut store,
        AgentEvent::TaskPhases {
            id: "w1".into(),
            name: Some("file-words".into()),
            phases: vec![
                TaskPhase {
                    title: "Read".into(),
                    agents: vec![
                        worker("read README.md", TaskState::Done),
                        worker("ls files", TaskState::Running),
                    ],
                },
                TaskPhase {
                    title: "Sum up".into(),
                    agents: vec![],
                },
            ],
        },
    );
    settle(&store, &mut ui);
    let window = limn::Size::new(1408.0, 844.0);
    let frame = |store: &Store, ui: &Ui| UiTree::new(view(store, ui, window)).layout(window);
    let shows = |frame: &limn::Frame, words: &str| {
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    let drawn = frame(&store, &ui);
    assert!(region(&drawn, "workflow:1:w1").is_some(), "its row");
    for words in ["file-words · Read", "1/2"] {
        assert!(shows(&drawn, words), "{words}");
    }
    // Its session's turn ended, but the work goes on: the dot stays, pulsing.
    assert_eq!(row_marks(&store, &ui).0.len(), 1);
    // The row opens the phases in the pane, its session selected.
    click(&store, &mut ui, "workflow:1:w1");
    assert_eq!(ui.selected, Some(1));
    assert!(ui.pane_open(Pane::Tasks));
    settle(&store, &mut ui);
    let drawn = frame(&store, &ui);
    for words in ["Read · 1/2", "Sum up · 0/0", "ls files"] {
        assert!(shows(&drawn, words), "{words}");
    }
    // Ended, it leaves the tree and stays in the pane's history.
    agent(
        &mut store,
        AgentEvent::TaskEnded {
            id: "w1".into(),
            call: None,
            summary: None,
            ok: true,
            stopped: false,
            usage: None,
        },
    );
    let drawn = frame(&store, &ui);
    assert!(region(&drawn, "workflow:1:w1").is_none());
    assert!(shows(&drawn, "Finished 1"));
    assert!(row_marks(&store, &ui).0.is_empty(), "nothing runs: no dot");
}

#[test]
fn background_tasks_list_running_and_finished_stop_and_clear() {
    let (mut store, mut ui) = store_with_session();
    for (id, kind) in [("t1", "agent"), ("t2", "command")] {
        agent(
            &mut store,
            AgentEvent::TaskStarted {
                id: id.into(),
                call: Some(format!("c{id}")),
                description: format!("Task {id}"),
                kind: kind.into(),
            },
        );
    }
    let usage = newen_model::TaskUsage {
        tokens: 178_600,
        tool_uses: 31,
        duration_ms: 285_000,
    };
    agent(
        &mut store,
        AgentEvent::TaskEnded {
            id: "t1".into(),
            call: Some("ct1".into()),
            summary: None,
            ok: true,
            stopped: false,
            usage: Some(usage),
        },
    );
    click(&store, &mut ui, view::TASKS_OPEN);
    settle(&store, &mut ui);
    let window = limn::Size::new(1408.0, 844.0);
    let shows = |store: &Store, ui: &Ui, words: &str| {
        let frame = UiTree::new(view(store, ui, window)).layout(window);
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    for words in [
        "Running 1",
        "Finished 1",
        "Completed",
        "4m 45s",
        // The rest of the line is cut at this width; the row itself opens the transcript.
        "178.6k tokens",
    ] {
        assert!(shows(&store, &ui, words), "{words}");
    }
    assert_eq!(
        click(&store, &mut ui, "task:stop:t2"),
        vec![Intent::StopTask {
            session: 1,
            task: "t2".into()
        }]
    );
    assert_eq!(
        click(&store, &mut ui, view::TASKS_CLEAR),
        vec![Intent::ClearTasks {
            session: 1,
            tasks: vec!["t1".into()]
        }]
    );
    store.apply(Event::TasksCleared {
        session: 1,
        tasks: vec!["t1".into()],
    });
    assert!(!shows(&store, &ui, "Finished 1") && shows(&store, &ui, "Running 1"));
}

#[test]
fn a_message_rewinds_and_forks_from_the_reply_before_it() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Idle,
    });
    agent(
        &mut store,
        AgentEvent::Text {
            text: "Done".into(),
            parent: None,
            id: Some("m1".into()),
            at: Some(0),
        },
    );
    store.apply(Event::Queued {
        session: 1,
        request: "r2".into(),
        text: "And now?".into(),
        at: Some(0),
        images: vec![],
    });
    assert_eq!(copied_text(&store, "copy:1:2").as_deref(), Some("And now?"));
    // The first message has no reply before it to fork from; it rewinds to the start.
    assert_eq!(click(&store, &mut ui, "fork-from:1:0"), vec![]);
    assert_eq!(
        click(&store, &mut ui, "rewind-to:1:0"),
        vec![Intent::RewindTo {
            session: 1,
            request: "r1".into(),
            at: None
        }]
    );
    assert_eq!(
        click(&store, &mut ui, "fork-from:1:2"),
        vec![Intent::Fork {
            session: 1,
            at: Some("m1".into())
        }]
    );
    assert_eq!(
        click(&store, &mut ui, "rewind-to:1:2"),
        vec![Intent::RewindTo {
            session: 1,
            request: "r2".into(),
            at: Some("m1".into())
        }]
    );
    assert_eq!(ui.restore_prompt.as_deref(), Some("And now?"));
    store.apply(Event::RewoundTo {
        session: 1,
        request: "r2".into(),
        at: Some("m1".into()),
    });
    assert_eq!(store.session(1).unwrap().items.len(), 2);
    // Not while the agent works.
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Running,
    });
    assert_eq!(click(&store, &mut ui, "rewind-to:1:0"), vec![]);
}

#[test]
fn a_rewind_is_undone_until_the_next_message() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Idle,
    });
    agent(
        &mut store,
        AgentEvent::Text {
            text: "Done".into(),
            parent: None,
            id: Some("m1".into()),
            at: Some(0),
        },
    );
    store.apply(Event::Queued {
        session: 1,
        request: "r2".into(),
        text: "And now?".into(),
        at: Some(0),
        images: vec![],
    });
    agent(
        &mut store,
        AgentEvent::Text {
            text: "Again".into(),
            parent: None,
            id: Some("m2".into()),
            at: Some(0),
        },
    );
    let before = store.session(1).unwrap().items.clone();
    store.apply(Event::RewoundTo {
        session: 1,
        request: "r2".into(),
        at: Some("m1".into()),
    });
    assert_eq!(store.session(1).unwrap().items.len(), 2);
    assert_eq!(store.session(1).unwrap().rewound.len(), 2);
    // The words the rewind put back in the box leave with the undo.
    ui.prompt = "And now?".into();
    assert_eq!(
        click(&store, &mut ui, "rewind:undo"),
        vec![Intent::UndoRewind { session: 1 }]
    );
    assert!(ui.prompt.is_empty());
    store.apply(Event::RewindUndone { session: 1 });
    assert_eq!(store.session(1).unwrap().items, before);
    assert!(store.session(1).unwrap().rewound.is_empty());
    assert_eq!(click(&store, &mut ui, "rewind:undo"), vec![]);
    // A message sent after a rewind is the end of it: nothing is left to undo.
    store.apply(Event::RewoundTo {
        session: 1,
        request: "r2".into(),
        at: Some("m1".into()),
    });
    store.apply(Event::Queued {
        session: 1,
        request: "r3".into(),
        text: "Instead".into(),
        at: Some(0),
        images: vec![],
    });
    assert!(store.session(1).unwrap().rewound.is_empty());
    assert_eq!(click(&store, &mut ui, "rewind:undo"), vec![]);
}

#[test]
fn a_reply_copies_forks_from_itself_and_reads_aloud() {
    let (mut store, mut ui) = store_with_session();
    for (text, id) in [("First", "m1"), ("Second", "m2")] {
        agent(
            &mut store,
            AgentEvent::Text {
                text: text.into(),
                parent: None,
                id: Some(id.into()),
                at: Some(0),
            },
        );
    }
    // The person's words, then the two replies.
    assert!(click(&store, &mut ui, "copy:1:1").is_empty());
    assert!(ui.copied.is_some());
    assert_eq!(
        click(&store, &mut ui, "fork-at:1:1"),
        vec![Intent::Fork {
            session: 1,
            at: Some("m1".into())
        }]
    );
    assert!(ui.waiting_fork);
    let read = |ui: &mut Ui| click(&store, ui, "read:1:2");
    assert_eq!(
        read(&mut ui),
        vec![Intent::ReadAloud {
            session: 1,
            text: Some("Second".into())
        }]
    );
    assert_eq!(ui.reading, Some((1, 2)));
    ui.applied(&Event::Reading {
        session: 1,
        reading: false,
    });
    assert_eq!(ui.reading, None);
    // A fork from the first reply starts with what came up to it.
    store.apply(Event::SessionCreated {
        session: 2,
        agent: "claude".into(),
        cwd: "/work".into(),
    });
    store.apply(Event::Forked {
        session: 2,
        from: 1,
        at: Some("m1".into()),
    });
    assert_eq!(
        store.session(2).unwrap().items,
        store.session(1).unwrap().items[..2]
    );
    assert_eq!(view::ago_from(30), "just now");
    assert_eq!(view::ago_from(15 * 3600 + 59), "15 hours ago");
    assert_eq!(view::ago_from(86_400), "1 day ago");
}

#[test]
fn the_branch_bars_counts_open_what_is_uncommitted() {
    let (store, mut ui) = store_with_session();
    assert_eq!(
        click(&store, &mut ui, view::VIEW_CHANGES),
        vec![Intent::WorkingTree { session: 1 }]
    );
    assert!(ui.pane_open(Pane::Diff));
}

#[test]
fn the_list_groups_by_status_and_sorts_by_name() {
    let mut store = Store::default();
    for (session, title, status) in [
        (1, "beta", SessionStatus::Idle),
        (2, "alpha", SessionStatus::Awaiting),
        (3, "gamma", SessionStatus::Idle),
    ] {
        store.apply(Event::SessionCreated {
            session,
            agent: "claude".into(),
            cwd: "/work".into(),
        });
        store.apply(Event::Renamed {
            session,
            title: title.into(),
        });
        store.apply(Event::Status { session, status });
    }
    let mut ui = Ui::new("/work".into());
    click(&store, &mut ui, "group-by:status");
    click(&store, &mut ui, "sort-by:name");
    click(&store, &mut ui, "sort-way:ascending");
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    let frame = tree.layout(window);
    let y = |id: &str| frame.regions.iter().find(|r| r.id == id).unwrap().rect.y;
    // Needs input comes first, then Completed sorted A to Z.
    assert!(y("group:status:Needs input") < y("session:2"));
    assert!(y("session:2") < y("group:status:Completed"));
    assert!(y("session:1") < y("session:3"));
    click(&store, &mut ui, view::FILTER_RESET);
    assert_eq!(
        (ui.group_by, ui.sort_by, ui.ascending),
        (GroupBy::Folder, SortBy::Updated, false)
    );
}

#[test]
fn the_branch_bar_writes_its_counts_with_commas() {
    assert_eq!(view::thousands(12), "12");
    assert_eq!(view::thousands(5318), "5,318");
    assert_eq!(view::thousands(1_234_567), "1,234,567");
}

#[test]
fn what_is_uncommitted_counts_to_each_new_figure_in_the_bar_and_the_panel() {
    use newen_model::{ChangeKind, FileEffect, Repo};
    let (mut store, mut ui) = store_with_session();
    let read = |added: u32, removed: u32| Event::WorkingTree {
        session: 1,
        files: vec![FileEffect {
            path: "a.py".into(),
            kind: ChangeKind::Modified,
            added,
            removed,
            patch: String::new(),
        }],
        repo: Some(Repo {
            branch: "main".into(),
            remote: None,
            base: None,
        }),
    };
    let window = limn::Size::new(1408.0, 844.0);
    // The figures drawn, the bar's and the panel's, in order.
    let figures = |frame: &limn::Frame| {
        let mut drawn: Vec<String> = (frame.commands.iter())
            .filter_map(|command| match command {
                limn::DrawCommand::Text { text, .. } if text.starts_with(['+', '\u{2212}']) => {
                    Some(text.clone())
                }
                _ => None,
            })
            .collect();
        drawn.sort();
        drawn
    };
    // What a screen reader hears of the bar's control and the panel's.
    let heard = |frame: &limn::Frame| {
        (frame.access.iter())
            .filter(|node| node.id == view::VIEW_CHANGES || node.id == view::CHANGES_ROW)
            .map(|node| node.name.clone())
            .collect::<Vec<_>>()
    };
    let start = std::time::Instant::now();
    store.apply(read(9, 1));
    let mut tree = UiTree::new(view(&store, &ui, window));
    let frame = tree.layout_at(window, start);
    assert_eq!(figures(&frame), ["+9", "+9", "\u{2212}1", "\u{2212}1"]);
    assert_eq!(heard(&frame), ["+9 \u{2212}1", "+9 \u{2212}1"]);
    // A step's read comes in while the agent works: both count up to it, a whole number on
    // every frame, and are heard as what they count to.
    store.apply(read(1_204, 13));
    tree.set_root(view(&store, &ui, window));
    tree.layout_at(window, start);
    let counting = tree.layout_at(window, start + std::time::Duration::from_millis(100));
    let between = figures(&counting);
    let added: u32 = between[0][1..].replace(',', "").parse().unwrap();
    assert!(added > 9 && added < 1_204, "{between:?}");
    assert_eq!(heard(&counting), ["+1,204 \u{2212}13", "+1,204 \u{2212}13"]);
    let counted = tree.layout_at(window, start + std::time::Duration::from_secs(2));
    assert_eq!(
        figures(&counted),
        ["+1,204", "+1,204", "\u{2212}13", "\u{2212}13"]
    );
    // Under reduced motion the figures change at once.
    let mut still = UiTree::new(view(&store, &ui, window));
    still.set_reduced_motion(true);
    still.layout_at(window, start);
    store.apply(read(30, 2));
    still.set_root(view(&store, &ui, window));
    let frame = still.layout_at(window, start);
    assert_eq!(figures(&frame), ["+30", "+30", "\u{2212}2", "\u{2212}2"]);
    // The engine reads it after each step by itself: a step asks for nothing.
    let step = Event::Agent {
        session: 1,
        request: None,
        event: AgentEvent::ToolResult {
            call: "t1".into(),
            output: String::new(),
            is_error: false,
            files: vec![],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    };
    assert!(!ui
        .applied(&step)
        .contains(&Intent::WorkingTree { session: 1 }));
}

#[test]
fn the_effort_thumb_follows_a_drag_and_springs_to_the_level_let_go_at() {
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::Ready {
            version: "2.1.310".into(),
            capabilities: newen_model::AgentCapabilities {
                commands: vec![],
                models: vec![newen_model::ModelInfo {
                    id: "opus".into(),
                    name: "Opus 5.5".into(),
                    efforts: vec!["low".into(), "medium".into(), "high".into()],
                    modes: vec![],
                    default: false,
                }],
                ..Default::default()
            },
            mode: None,
        },
    );
    ui.model = Some("opus".into());
    let slide = |ui: &mut Ui, fraction: f32, committed: bool| {
        update(
            &store,
            ui,
            &UiAction::SliderChanged {
                id: view::EFFORT_SLIDER.into(),
                fraction,
                committed,
            },
        )
    };
    // While dragged nothing is chosen: the thumb is where the pointer holds it.
    assert!(slide(&mut ui, 0.7, false).is_empty());
    assert!(ui.effort_dragging);
    let now = std::time::Instant::now();
    assert_eq!(
        ui.effort_thumb.unwrap().value(now),
        0.7 * view::EFFORT_TRAVEL
    );
    // Let go, the nearest level is chosen and the thumb travels on to it.
    assert_eq!(
        slide(&mut ui, 1.0, true),
        vec![Intent::SetEffort {
            session: 1,
            effort: "high".into()
        }]
    );
    assert!(!ui.effort_dragging);
    assert!(ui.animating());
    assert_eq!(ui.effort_thumb.unwrap().target(), view::EFFORT_TRAVEL);
}

#[test]
fn a_started_session_is_nested_under_its_parent_and_completed_shows_five_first() {
    let mut store = Store::default();
    for session in 1..=8 {
        store.apply(Event::SessionCreated {
            session,
            agent: "claude".into(),
            cwd: "/work".into(),
        });
        store.apply(Event::Status {
            session,
            status: SessionStatus::Idle,
        });
    }
    store.apply(Event::Linked {
        session: 8,
        parent: 1,
        side: false,
    });
    let mut ui = Ui::new("/work".into());
    // Completed's "Show more" is the status groups'.
    ui.group_by = GroupBy::Status;
    let window = limn::Size::new(1408.0, 844.0);
    let regions = |store: &Store, ui: &Ui| {
        let mut tree = UiTree::new(view(store, ui, window));
        tree.layout(window)
            .regions
            .iter()
            .map(|r| (r.id.clone(), r.rect))
            .collect::<Vec<_>>()
    };
    let shown = regions(&store, &ui);
    let at = |id: &str| shown.iter().find(|r| r.0 == id).map(|r| r.1);
    // Seven top-level sessions: the five newest, then "Show more"; 1 is among those left.
    assert!(at("session:7").is_some() && at("session:3").is_some());
    assert!(at("session:2").is_none() && at("session:1").is_none());
    assert!(at(view::SHOW_MORE).is_some());
    click(&store, &mut ui, view::SHOW_MORE);
    let shown = regions(&store, &ui);
    let at = |id: &str| shown.iter().find(|r| r.0 == id).map(|r| r.1).unwrap();
    // The child sits right under its parent, further in.
    let (parent, child) = (at("session:1"), at("session:8"));
    assert!(child.y > parent.y && child.y < parent.y + 40.0);
    assert!(child.x > parent.x);
}

#[test]
fn proposals_share_a_card_that_starts_them_and_reports_read_back() {
    let (mut store, mut ui) = store_with_session();
    let proposal = |id: &str| newen_model::Proposal {
        id: id.into(),
        title: format!("Task {id}"),
        prompt: "Do it".into(),
        background: None,
        fork: false,
        agent: None,
    };
    for id in ["a", "b"] {
        store.apply(Event::Proposed {
            session: 1,
            proposal: proposal(id),
        });
    }
    let cards: Vec<_> = store
        .session(1)
        .unwrap()
        .items
        .iter()
        .filter(|i| matches!(i, Item::Proposals { .. }))
        .collect();
    assert!(matches!(cards.as_slice(), [Item::Proposals { tasks }] if tasks.len() == 2));
    // Start all asks for each waiting task.
    assert_eq!(
        click(&store, &mut ui, "proposal-all:a,b"),
        ["a", "b"]
            .map(|task| Intent::AnswerProposal {
                session: 1,
                task: task.into(),
                answer: newen_model::ProposalAnswer::Start,
            })
            .to_vec()
    );
    store.apply(Event::ProposalAnswered {
        session: 1,
        task: "a".into(),
        outcome: newen_model::ProposalOutcome::Started { session: 2 },
    });
    store.apply(Event::Reported {
        session: 1,
        request: "r2".into(),
        reports: vec![newen_model::Report {
            from: 2,
            title: "Task a".into(),
            text: "All done".into(),
            waited: None,
        }],
    });
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    let frame = tree.layout(window);
    // The started task follows its session; its report opens it beside and unfolds.
    assert!(frame.regions.iter().any(|r| r.id == "thread:2"));
    assert!(frame.regions.iter().any(|r| r.id == "report:r2:0"));
    click(&store, &mut ui, "report:r2:0");
    assert!(ui.expanded.contains(&"report:r2:0".to_string()));
}

#[test]
fn a_started_session_opens_beside_and_says_when_it_waits() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::SessionCreated {
        session: 2,
        agent: "claude".into(),
        cwd: "/work".into(),
    });
    store.apply(Event::Linked {
        session: 2,
        parent: 1,
        side: false,
    });
    store.apply(Event::Renamed {
        session: 2,
        title: "Draft release notes".into(),
    });
    store.apply(Event::Agent {
        session: 2,
        request: Some("r9".into()),
        event: AgentEvent::PermissionRequest {
            id: "p1".into(),
            call: None,
            tool: "Bash".into(),
            input: serde_json::json!({"command": "ls", "description": "List files"}),
            title: None,
            interactive: false,
            suggestions: vec![],
        },
    });
    store.apply(Event::Status {
        session: 2,
        status: SessionStatus::Awaiting,
    });
    // The session in view says the one it started waits, and opens it beside.
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(frame.regions.iter().any(|r| r.id == "thread:2"));
    click(&store, &mut ui, "thread:2");
    assert_eq!((ui.selected, ui.side_chat), (Some(1), Some(2)));
    assert!(ui.pane_open(Pane::Side));
    // Its card is answered from the pane, for it.
    assert!(matches!(
        click(&store, &mut ui, "perm:p1:once").as_slice(),
        [Intent::Decide { session: 2, .. }]
    ));
    // Closing it leaves it be: only a side chat is put away.
    assert!(click(&store, &mut ui, "pane:side:close").is_empty());
    click(&store, &mut ui, "thread:2");
    // Shown on its own, it leaves the pane.
    click(&store, &mut ui, view::SIDE_OPEN);
    assert_eq!(ui.selected, Some(2));
    assert!(!ui.pane_open(Pane::Side));
}

#[test]
fn the_sidebars_customize_and_the_persons_menu_open_settings() {
    use newen_model::AgentInfo;
    let (store, mut ui) = store_with_session();
    ui.applied(&Event::Settings {
        settings: Default::default(),
        agents: vec![AgentInfo {
            id: "claude".into(),
            name: "Claude Code".into(),
            tested: vec![],
            options: vec![],
            icon: None,
            sign_in_methods: vec![],
            sign_in_code: true,
            modes: claude_modes(),
            default_mode: "default".into(),
            customize: Default::default(),
        }],
    });
    ui.cwd = "/work".into();
    assert_eq!(
        click(&store, &mut ui, view::CUSTOMIZE_OPEN),
        vec![
            Intent::Extensions {
                agent: "claude".into(),
                folder: "/work".into()
            },
            Intent::Global(newen_model::GlobalOp::Load)
        ]
    );
    assert_eq!(
        ui.customize.as_ref().map(|p| p.tab.as_str()),
        Some(view::SETTINGS_SKILLS)
    );
    assert!(ui.settings_page.is_none());
    click(&store, &mut ui, view::ACCOUNT_MENU);
    assert!(ui.account_menu);
    click(&store, &mut ui, view::SETTINGS_OPEN);
    assert!(!ui.account_menu);
    assert_eq!(ui.settings_page.as_deref(), Some(view::SETTINGS_FIRST));
}

#[test]
fn customize_lists_and_changes_an_agents_plugins_marketplaces_and_skills() {
    use newen_model::{
        AgentInfo, ExtensionOp, Extensions, Marketplace, Plugin, PluginOffer, Skill,
    };
    let (store, mut ui) = store_with_session();
    ui.applied(&Event::Settings {
        settings: Default::default(),
        agents: vec![AgentInfo {
            id: "claude".into(),
            name: "Claude Code".into(),
            tested: vec![],
            options: vec![],
            icon: None,
            sign_in_methods: vec![],
            sign_in_code: true,
            modes: claude_modes(),
            default_mode: "default".into(),
            customize: Default::default(),
        }],
    });
    // A project folder: plugins install for it, and skills' states are kept there.
    ui.cwd = "/work".into();
    let folder = std::path::PathBuf::from("/work");
    click(&store, &mut ui, view::CUSTOMIZE_OPEN);
    let page = format!("czp:tab:{}", view::SETTINGS_PLUGINS);
    assert_eq!(
        click(&store, &mut ui, &page),
        vec![
            Intent::Extensions {
                agent: "claude".into(),
                folder: folder.clone()
            },
            Intent::Global(newen_model::GlobalOp::Load)
        ]
    );
    let window = limn::Size::new(1408.0, 844.0);
    let shows = |ui: &Ui, words: &str| {
        let frame = UiTree::new(view(&store, ui, window)).layout(window);
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    assert!(shows(&ui, "Looking for plugins"));
    let plugin = Plugin {
        id: "commit@better".into(),
        name: "commit".into(),
        version: Some("0.2.0".into()),
        scope: "user".into(),
        enabled: true,
    };
    let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
    let list = Extensions {
        plugins: vec![plugin.clone()],
        offers: vec![PluginOffer {
            id: "figma@official".into(),
            name: "figma".into(),
            description: "Figma's design files.".into(),
            marketplace: "official".into(),
            version: None,
            installs: Some(968_600),
        }],
        marketplaces: vec![Marketplace {
            name: "better".into(),
            source: "midudev/better-claude".into(),
        }],
        skills: vec![Skill {
            name: "graphify".into(),
            description: "Any input to a knowledge graph.".into(),
            source: "user".into(),
            state: "on".into(),
            locked: None,
            key: serde_json::json!({"name": "graphify"}),
        }],
        scopes: vec![
            pair("user", "Install for me"),
            pair("project", "Install for project (shared)"),
        ],
        skill_states: vec![pair("on", "On"), pair("off", "Off")],
        kinds: Default::default(),
    };
    let heard = |ui: &mut Ui, error: Option<&str>| {
        ui.applied(&Event::Extensions {
            agent: "claude".into(),
            folder: folder.clone(),
            extensions: list.clone(),
            error: error.map(str::to_owned),
        })
    };
    heard(&mut ui, None);
    // Desktop's page: the project's plugins under its folder, then the person's own.
    assert!(shows(&ui, "In this project") && shows(&ui, "Your plugins"));
    assert!(shows(&ui, "v0.2.0 \u{b7} from better"));
    let op = |op| Intent::ExtensionOp {
        agent: "claude".into(),
        folder: folder.clone(),
        op,
    };
    // Its switch asks the engine to turn it off, and says so until the lists come back.
    assert_eq!(
        click(&store, &mut ui, "ext:switch:claude:commit@better"),
        vec![op(ExtensionOp::SwitchPlugin {
            plugin: plugin.clone(),
            enabled: false
        })]
    );
    assert_eq!(
        ui.extensions["claude"].working.as_deref(),
        Some("commit@better")
    );
    heard(&mut ui, Some("could not reach the marketplace"));
    assert!(shows(&ui, "could not reach the marketplace"));
    // Discover's directory lists what the marketplaces offer; in a project, "+" opens
    // Desktop's menu of where, and a choice installs there.
    click(&store, &mut ui, "cz:discover:claude");
    assert!(shows(&ui, "Directory") && shows(&ui, "968.6K installs"));
    assert!(click(&store, &mut ui, "ext:install:claude:figma@official").is_empty());
    assert!(shows(&ui, "Install for project (shared)"));
    assert_eq!(
        click(&store, &mut ui, "ext:install:claude:figma@official=project"),
        vec![op(ExtensionOp::Install {
            id: "figma@official".into(),
            scope: Some("project".into())
        })]
    );
    heard(&mut ui, None);
    assert!(shows(&ui, "figma is installed and ready to use."));
    // Removing asks first.
    assert!(click(&store, &mut ui, "ext:remove:claude:commit@better").is_empty());
    assert!(shows(&ui, "Remove plugin?"));
    assert_eq!(
        click(&store, &mut ui, view::EXT_REMOVE_YES),
        vec![op(ExtensionOp::Uninstall(plugin))]
    );
    // A marketplace from its dialog.
    click(&store, &mut ui, "cz:add-marketplace:claude");
    assert!(shows(&ui, "A GitHub owner/repo or a Git repository URL."));
    update(
        &store,
        &mut ui,
        &UiAction::InputChanged {
            id: format!("{}claude", view::EXT_MKT_FIELD),
            value: "o/r".into(),
        },
    );
    assert_eq!(
        click(&store, &mut ui, "ext:mkt-add"),
        vec![op(ExtensionOp::AddMarketplace("o/r".into()))]
    );
    assert!(ui.marketplaces.is_none());
    click(&store, &mut ui, "cz:close");
    assert!(ui.directory.is_none());
    // A skill's state, from its menu.
    click(
        &store,
        &mut ui,
        &format!("czp:tab:{}", view::SETTINGS_SKILLS),
    );
    heard(&mut ui, None);
    assert!(shows(&ui, "graphify") && shows(&ui, "from user"));
    // Its "⋯" holds its states.
    click(&store, &mut ui, "cz:more:claude:graphify");
    assert!(shows(&ui, "Off"));
    assert_eq!(
        click(&store, &mut ui, "ext:skill:claude:graphify=off"),
        vec![op(ExtensionOp::SkillState {
            skill: list.skills[0].clone(),
            state: "off".into()
        })]
    );
    // Another folder chosen while it shows is read again, with the "All agents" layer.
    ui.cwd = "/other".into();
    assert_eq!(
        ui.due(),
        vec![
            Intent::Extensions {
                agent: "claude".into(),
                folder: "/other".into()
            },
            Intent::Global(newen_model::GlobalOp::Load)
        ]
    );
}

#[test]
fn mcp_lists_the_sessions_servers_and_reconnects_or_turns_one_on() {
    use newen_model::{McpServer, McpSource, McpStatus};
    let (mut store, mut ui) = store_with_session();
    ui.selected = Some(1);
    let typed = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::COMPOSER.into(),
            value: "/mcp".into(),
        },
    );
    assert_eq!(typed, vec![Intent::McpStatus { session: 1 }]);
    let window = limn::Size::new(1408.0, 844.0);
    let shows = |store: &Store, ui: &Ui, words: &str| {
        let frame = UiTree::new(view(store, ui, window)).layout(window);
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    // Until the agent answers, it says what it waits for.
    assert!(shows(&store, &ui, "Loading this session"));
    let server = |name: &str, status, plugin: Option<&str>| McpServer {
        name: name.into(),
        label: name.rsplit(':').next().unwrap().into(),
        status,
        source: if plugin.is_some() {
            McpSource::Plugin
        } else {
            McpSource::User
        },
        plugin: plugin.map(str::to_owned),
        error: None,
        tools: vec![Default::default(), Default::default()],
        sign_in: plugin.is_some(),
    };
    agent(
        &mut store,
        AgentEvent::McpServers(vec![
            server("rea", McpStatus::Connected, None),
            server("plugin:vercel:vercel", McpStatus::NeedsAuth, Some("vercel")),
            server("off", McpStatus::Disabled, None),
        ]),
    );
    // The list says where each comes from; how many tools, in its details.
    assert!(shows(&store, &ui, "Provided by the vercel plugin") && shows(&store, &ui, "Plugin"));
    // Each action asks for the servers again, to show what it did.
    assert_eq!(
        click(
            &store,
            &mut ui,
            &format!("{}plugin:vercel:vercel", view::MCP_RECONNECT)
        ),
        vec![
            Intent::McpReconnect {
                session: 1,
                name: "plugin:vercel:vercel".into()
            },
            Intent::McpStatus { session: 1 }
        ]
    );
    assert_eq!(
        click(&store, &mut ui, &format!("{}off", view::MCP_TURN_ON))[0],
        Intent::McpToggle {
            session: 1,
            name: "off".into(),
            enabled: true
        }
    );
    // Signing in: the agent's page opens in the browser, and the sign-in is over once the
    // server's status changes.
    let vercel = "plugin:vercel:vercel";
    assert_eq!(
        click(&store, &mut ui, &format!("{}{vercel}", view::MCP_CONNECT)),
        vec![Intent::McpAuthenticate {
            session: 1,
            name: vercel.into()
        }]
    );
    // Its row waits on a spinner, as Desktop's "Opening sign-in…".
    let phase = |ui: &Ui| ui.mcp.as_ref().unwrap().phases.get(vercel).copied();
    assert_eq!(phase(&ui), Some(crate::McpPhase::Opening));
    let heard = |ui: &mut Ui, event: AgentEvent| {
        ui.applied(&Event::Agent {
            session: 1,
            request: None,
            event,
        })
    };
    heard(
        &mut ui,
        AgentEvent::McpSignIn {
            server: vercel.into(),
            url: Some("https://example.com/authorize".into()),
            error: None,
        },
    );
    assert_eq!(phase(&ui), Some(crate::McpPhase::InBrowser));
    click(&store, &mut ui, &format!("{}{vercel}", view::MCP_ROW));
    assert!(shows(&store, &ui, "Complete sign-in in your browser"));
    assert!(shows(&store, &ui, "Finished signing in from your browser?"));
    let mut connected = server(vercel, McpStatus::Connected, Some("vercel"));
    connected.tools[0] = newen_model::McpTool {
        name: "deploy".into(),
        destructive: true,
        ..Default::default()
    };
    heard(
        &mut ui,
        AgentEvent::McpServers(vec![server(vercel, McpStatus::NeedsAuth, Some("vercel"))]),
    );
    heard(&mut ui, AgentEvent::McpServers(vec![connected.clone()]));
    agent(&mut store, AgentEvent::McpServers(vec![connected]));
    assert!(ui.mcp.as_ref().unwrap().phases.is_empty());
    // Its details: its tools by what they do, and forgetting its credentials after asking.
    assert!(shows(&store, &ui, "Write/delete tools (1)") && shows(&store, &ui, "deploy"));
    click(&store, &mut ui, &format!("{}{vercel}", view::MCP_CLEAR));
    assert!(shows(&store, &ui, "Clear authentication for vercel?"));
    assert_eq!(
        click(&store, &mut ui, view::MCP_CLEAR_YES)[0],
        Intent::McpClearAuth {
            session: 1,
            name: vercel.into()
        }
    );
    // A sign-in that needs no one reconnects at once.
    let again = heard(
        &mut ui,
        AgentEvent::McpSignIn {
            server: vercel.into(),
            url: None,
            error: None,
        },
    );
    assert!(again.contains(&Intent::McpReconnect {
        session: 1,
        name: vercel.into()
    }));
    click(&store, &mut ui, view::MCP_BACK);
    // Open, it asks again on its own; done, it stops.
    ui.mcp.as_mut().unwrap().asked = std::time::Instant::now() - crate::MCP_POLL;
    assert_eq!(ui.due(), vec![Intent::McpStatus { session: 1 }]);
    assert!(ui.due().is_empty());
    click(&store, &mut ui, view::MCP_DONE);
    assert!(ui.mcp.is_none() && !shows(&store, &ui, "MCP servers in this session"));
}

#[test]
fn btw_asks_beside_the_session_and_closing_puts_it_away() {
    let (mut store, mut ui) = store_with_session();
    let asked = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::COMPOSER.into(),
            value: "/btw what does it do?".into(),
        },
    );
    assert_eq!(
        asked,
        vec![Intent::SideChat {
            session: 1,
            text: "what does it do?".into()
        }]
    );
    let created = Event::SessionCreated {
        session: 2,
        agent: "claude".into(),
        cwd: "/work".into(),
    };
    // It opens in its pane; the session in view stays.
    assert!(ui.applied(&created).is_empty());
    store.apply(created);
    store.apply(Event::Linked {
        session: 2,
        parent: 1,
        side: true,
    });
    assert_eq!((ui.selected, ui.side_chat), (Some(1), Some(2)));
    assert!(ui.pane_open(Pane::Side));
    store.apply(Event::Status {
        session: 2,
        status: SessionStatus::Idle,
    });
    assert_eq!(
        click(&store, &mut ui, "pane:side:close"),
        vec![Intent::Archive {
            session: 2,
            archived: true
        }]
    );
    assert!(!ui.pane_open(Pane::Side));
}

#[test]
fn a_turns_changes_list_three_files_then_the_rest_and_each_opens_the_diff() {
    use newen_model::{ChangeKind, FileEffect};
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::Changes {
        session: 1,
        request: "r1".into(),
        files: (0..5)
            .map(|i| FileEffect {
                path: format!("src/f{i}.rs"),
                kind: ChangeKind::Modified,
                added: 1,
                removed: 0,
                patch: String::new(),
            })
            .collect(),
        snapshot: "s".into(),
    });
    let window = limn::Size::new(1408.0, 844.0);
    let shown = |ui: &Ui, id: &str| {
        let frame = UiTree::new(view(&store, ui, window)).layout(window);
        frame.regions.iter().any(|r| r.id == id)
    };
    assert!(shown(&ui, "changes:r1:view"));
    assert!(shown(&ui, "changes:r1:view:src/f2.rs"));
    assert!(!shown(&ui, "changes:r1:view:src/f3.rs"));
    click(&store, &mut ui, "changes:r1:more");
    assert!(shown(&ui, "changes:r1:view:src/f4.rs"));
    click(&store, &mut ui, "changes:r1:view:src/f1.rs");
    assert_eq!(ui.diff, Some((1, "r1".into())));
    assert!(ui.pane_open(Pane::Diff));
}

#[test]
fn the_trees_filter_finds_files_by_name_and_lines_after_a_question_mark() {
    use newen_files::find::Hit;
    let (store, mut ui) = store_with_session();
    ui.selected = Some(1);
    click(&store, &mut ui, view::FILES_TOGGLE);
    settle(&store, &mut ui);
    let typed = |ui: &mut Ui, value: &str| {
        update(
            &store,
            ui,
            &UiAction::InputChanged {
                id: view::FILTER_FIELD.into(),
                value: value.into(),
            },
        )
    };
    let find = |query: &str, contents: bool| Intent::FindFiles {
        session: 1,
        query: query.into(),
        contents,
    };
    assert_eq!(typed(&mut ui, "main"), [find("main", false)]);
    // "?" alone asks nothing yet; words after it search the files' contents.
    assert_eq!(typed(&mut ui, "?"), []);
    assert_eq!(typed(&mut ui, "?fn main"), [find("fn main", true)]);
    let found = |query: &str| Event::FilesFound {
        session: 1,
        query: query.into(),
        contents: true,
        paths: vec![],
        hits: vec![Hit {
            path: "src/a.rs".into(),
            row: 41,
            text: "fn main() {}".into(),
        }],
    };
    // An answer to older words is let go.
    ui.applied(&found("main"));
    let window = limn::Size::new(1408.0, 844.0);
    let shows = |ui: &Ui, id: &str| {
        let frame = UiTree::new(view(&store, ui, window)).layout(window);
        frame.regions.iter().any(|r| r.id == id)
    };
    assert!(!shows(&ui, "found:src/a.rs:41"));
    ui.applied(&found("fn main"));
    assert!(shows(&ui, "found:src/a.rs:41") && shows(&ui, "file:/work/src/a.rs"));
    // A line opens its file, to put the caret on it once read.
    assert!(
        click(&store, &mut ui, "found:src/a.rs:41").contains(&Intent::ReadFile {
            session: 1,
            path: "src/a.rs".into()
        })
    );
    click(&store, &mut ui, view::FILTER_CLEAR);
    assert!(!shows(&ui, "found:src/a.rs:41"));
}

#[test]
fn outputs_open_in_a_document_pane_that_saves_and_follows_the_agent() {
    use newen_model::{ChangeKind, FileEffect};
    let (mut store, mut ui) = store_with_session();
    let effect = |path: &str, kind: ChangeKind| FileEffect {
        path: path.into(),
        kind,
        added: 1,
        removed: 0,
        patch: String::new(),
    };
    store.apply(Event::Changes {
        session: 1,
        request: "r1".into(),
        files: vec![
            effect("docs/notes.md", ChangeKind::Added),
            effect("old.txt", ChangeKind::Deleted),
        ],
        snapshot: "s".into(),
    });
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(frame
        .regions
        .iter()
        .any(|r| r.id == "doc:open:docs/notes.md"));
    assert!(!frame.regions.iter().any(|r| r.id == "doc:open:old.txt"));
    // It opens in the Files pane, its folder shown in the tree.
    assert!(
        click(&store, &mut ui, "doc:open:docs/notes.md").contains(&Intent::ReadFile {
            session: 1,
            path: "docs/notes.md".into()
        })
    );
    assert!(ui.pane_open(Pane::Files));
    let read = |text: &str| Event::FileRead {
        session: 1,
        path: "docs/notes.md".into(),
        text: Some(text.into()),
    };
    ui.applied(&read("# Notes\n"));
    let document = ui.document().unwrap();
    assert!(document.preview && !document.dirty);
    assert_eq!(document.text().as_deref(), Some("# Notes\n"));
    // What the agent writes shows at once while the person has not edited it.
    let wrote = Event::Agent {
        session: 1,
        request: Some("r1".into()),
        event: AgentEvent::ToolResult {
            call: "t".into(),
            output: String::new(),
            is_error: false,
            files: vec![],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    };
    assert!(ui.applied(&wrote).contains(&Intent::ReadFile {
        session: 1,
        path: "docs/notes.md".into()
    }));
    // Edited, it saves what the editor holds, and the disk no longer overwrites it.
    update(
        &store,
        &mut ui,
        &UiAction::EditorChanged {
            id: crate::files::editor_id("docs/notes.md"),
            document_id: 0,
            revision: 1,
        },
    );
    assert!(ui.document().unwrap().dirty);
    assert!(!ui
        .applied(&wrote)
        .iter()
        .any(|i| matches!(i, Intent::ReadFile { .. })));
    ui.applied(&read("# Other\n"));
    assert_eq!(ui.document().unwrap().text().as_deref(), Some("# Notes\n"));
    assert_eq!(
        click(&store, &mut ui, view::DOC_SAVE),
        vec![Intent::WriteFile {
            session: 1,
            path: "docs/notes.md".into(),
            text: "# Notes\n".into()
        }]
    );
    ui.applied(&read("# Notes\n"));
    assert!(!ui.document().unwrap().dirty);
}

#[test]
fn a_passed_on_message_says_so_on_both_sides_and_new_work_is_marked() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::SessionCreated {
        session: 2,
        agent: "claude".into(),
        cwd: "/work".into(),
    });
    store.apply(Event::Linked {
        session: 2,
        parent: 1,
        side: false,
    });
    // The parent passes a message on: its tool step counts under the person's words.
    agent(
        &mut store,
        AgentEvent::ToolUse {
            call: "t1".into(),
            name: "mcp__sessions__send_to_session".into(),
            input: serde_json::json!({"session": "Notes", "message": "Monday"}),
            parent: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::ToolResult {
            call: "t1".into(),
            output: "Passed on.".into(),
            is_error: false,
            files: vec![],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    );
    let session = store.session(1).unwrap();
    assert_eq!(view::sent_from(&session.items, 0), 1);
    // The child hears it with where it came from.
    store.apply(Event::Forwarded {
        session: 2,
        from: 1,
    });
    store.apply(Event::Queued {
        session: 2,
        request: "q".into(),
        text: "Monday".into(),
        at: None,
        images: vec![],
    });
    assert!(matches!(
        store.session(2).unwrap().items.as_slice(),
        [Item::Forwarded { from: 1 }, Item::User { .. }]
    ));
    // Seen with two items, opened again with more: the new ones are marked.
    click(&store, &mut ui, "thread:2");
    ui.tick(&store);
    click(&store, &mut ui, "pane:side:close");
    agent_in(&mut store, 2, "Done.");
    click(&store, &mut ui, "thread:2");
    assert_eq!(ui.new_from.get(&2), Some(&2));
}

#[test]
fn hidden_by_a_click_the_sidebar_peeks_again_only_once_the_pointer_left_its_button() {
    let (store, mut ui) = store_with_session();
    ui.hovered = Some(view::SIDEBAR_TOGGLE.into());
    ui.pointer = Some(limn::Point::new(20.0, 18.0));
    click(&store, &mut ui, view::SIDEBAR_TOGGLE);
    assert!(!ui.sidebar.open);
    ui.tick(&store);
    assert!(!ui.sidebar.peek);
    // Over the sidebar sliding away from under it, the pointer has not left yet.
    ui.hovered = None;
    ui.tick(&store);
    assert!(!ui.sidebar.peek);
    ui.pointer = Some(limn::Point::new(700.0, 300.0));
    ui.tick(&store);
    ui.hovered = Some(view::SIDEBAR_TOGGLE.into());
    ui.pointer = Some(limn::Point::new(20.0, 18.0));
    ui.tick(&store);
    assert!(ui.sidebar.peek);
}

#[test]
fn the_last_pane_closed_keeps_what_it_shows_until_the_column_has_shut() {
    let (store, mut ui) = store_with_session();
    ui.open_pane(Pane::Agent);
    ui.helper = Some("call".into());
    ui.tick(&store);
    ui.close_pane(Pane::Agent);
    ui.tick(&store);
    assert_eq!(ui.helper.as_deref(), Some("call"));
    settle(&store, &mut ui);
    assert_eq!((ui.helper.as_deref(), ui.closing), (None, None));
    // One of two shuts on the stack's spring, still shown until it has; the column stays.
    ui.reduced_motion = false;
    ui.open_pane(Pane::Agent);
    ui.open_pane(Pane::Plan);
    ui.tick(&store);
    ui.helper = Some("call".into());
    ui.close_pane(Pane::Agent);
    ui.tick(&store);
    assert_eq!(ui.helper.as_deref(), Some("call"));
    assert_eq!(ui.stack_closing, Some((0, Pane::Agent)));
    assert!(ui.closing.is_none());
    settle(&store, &mut ui);
    assert_eq!((ui.helper.as_deref(), ui.stack_closing), (None, None));
}

/// The springs at rest, so a pane just opened is drawn whole.
fn settle(store: &Store, ui: &mut Ui) {
    ui.reduced_motion = true;
    ui.tick(store);
}

fn agent_in(store: &mut Store, session: SessionId, text: &str) {
    store.apply(Event::Agent {
        session,
        request: None,
        event: AgentEvent::Text {
            text: text.into(),
            parent: None,
            id: None,
            at: None,
        },
    });
}

#[test]
fn a_session_remembers_the_pull_request_its_agent_named() {
    let (mut store, _ui) = store_with_session();
    store.apply(Event::PullRequest {
        session: 1,
        url: "https://github.com/o/r/pull/3".into(),
    });
    let session = store.sessions.iter().find(|s| s.id == 1).unwrap();
    assert_eq!(
        session.pull_request.as_deref(),
        Some("https://github.com/o/r/pull/3")
    );
}

#[test]
fn settings_save_through_the_engine_and_shape_what_comes_next() {
    use newen_model::{AgentInfo, InterfaceFont, Settings, TextSize};
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::Ready {
            version: "2.1.310".into(),
            capabilities: newen_model::AgentCapabilities {
                commands: vec![],
                models: vec![newen_model::ModelInfo {
                    id: "haiku".into(),
                    name: "Haiku".into(),
                    efforts: vec!["low".into(), "high".into()],
                    modes: vec!["auto".into()],
                    default: false,
                }],
                modes: vec!["default".into(), "plan".into()],
                unsafe_modes: vec!["bypassPermissions".into()],
                ..Default::default()
            },
            mode: None,
        },
    );
    let heard = |ui: &mut Ui, settings: Settings| {
        ui.applied(&Event::Settings {
            settings,
            agents: vec![AgentInfo {
                id: "claude".into(),
                name: "Claude Code".into(),
                tested: vec!["2.1.310".into()],
                options: vec![],
                icon: None,
                sign_in_methods: vec![],
                sign_in_code: true,
                modes: claude_modes(),
                default_mode: "default".into(),
                customize: Default::default(),
            }],
        });
    };
    heard(&mut ui, Settings::default());
    assert_eq!(
        click(&store, &mut ui, view::SETTINGS_OPEN),
        vec![Intent::MeasureStorage]
    );
    // A switch is kept by the engine before the UI shows it.
    let saved = click(&store, &mut ui, "set:keep_awake");
    let [Intent::SaveSettings(settings)] = saved.as_slice() else {
        panic!("{saved:?}");
    };
    assert!(!settings.keep_awake && ui.settings.keep_awake);
    let saved = click(&store, &mut ui, "set:keep_awake_battery");
    let [Intent::SaveSettings(settings)] = saved.as_slice() else {
        panic!("{saved:?}");
    };
    assert!(!settings.keep_awake_on_battery && ui.settings.keep_awake_on_battery);
    let saved = click(&store, &mut ui, "set:prs_in_browser");
    let [Intent::SaveSettings(settings)] = saved.as_slice() else {
        panic!("{saved:?}");
    };
    assert!(settings.prs_in_browser && !ui.settings.prs_in_browser);
    let saved = click(&store, &mut ui, "set:archive_on_merge");
    let [Intent::SaveSettings(settings)] = saved.as_slice() else {
        panic!("{saved:?}");
    };
    assert!(settings.archive_on_merge && !ui.settings.archive_on_merge);
    // Not kept, it says so over the page until the next change.
    ui.applied(&Event::SettingsNotSaved {
        error: "read-only file system".into(),
    });
    let window = limn::Size::new(1408.0, 844.0);
    let said = |ui: &Ui| {
        let frame = UiTree::new(view(&store, ui, window)).layout(window);
        frame.access.iter().any(|node| node.name.contains("read-only file system"))
            || frame.commands.iter().any(|command| {
                matches!(command, limn::DrawCommand::Text { text, .. } if text.contains("read-only file system"))
            })
    };
    assert!(said(&ui));
    // A select opens, and a choice closes it and saves.
    click(&store, &mut ui, "set:text_size");
    assert!(!said(&ui));
    assert_eq!(ui.settings_menu.as_deref(), Some("text_size"));
    let saved = click(&store, &mut ui, "set:text_size=large");
    assert!(ui.settings_menu.is_none());
    assert!(
        matches!(saved.as_slice(), [Intent::SaveSettings(s)] if s.text_size == TextSize::Large)
    );
    // Each notification select keeps its choice, and the sound its own.
    let saved = click(&store, &mut ui, "set:notify_permission=off");
    assert!(
        matches!(saved.as_slice(), [Intent::SaveSettings(s)] if s.notify_permission == newen_model::NotifyLevel::Off)
    );
    let saved = click(&store, &mut ui, "set:notification_sound=none");
    assert!(matches!(saved.as_slice(), [Intent::SaveSettings(s)] if !s.notification_sound));
    // The interface font is a select; the code font is a field, saved by Enter.
    let saved = click(&store, &mut ui, "set:interface_font=ubuntu");
    assert!(
        matches!(saved.as_slice(), [Intent::SaveSettings(s)] if s.interface_font == InterfaceFont::Ubuntu)
    );
    let saved = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: "set:code_font".into(),
            value: " Fira Code ".into(),
        },
    );
    assert!(matches!(saved.as_slice(), [Intent::SaveSettings(s)] if s.code_font == "Fira Code"));
    let saved = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: "set:branch_prefix".into(),
            value: " team/ ".into(),
        },
    );
    assert!(matches!(saved.as_slice(), [Intent::SaveSettings(s)] if s.branch_prefix == "team"));
    // An agent's defaults: a new model drops the effort chosen for the old one.
    let mut settings = Settings::default();
    settings.agents.entry("claude".into()).or_default().effort = Some("high".into());
    heard(&mut ui, settings);
    let saved = click(&store, &mut ui, "set:agent:claude:model=haiku");
    let [Intent::SaveSettings(settings)] = saved.as_slice() else {
        panic!("{saved:?}");
    };
    let claude = settings.agent("claude");
    assert_eq!(
        (claude.model.as_deref(), claude.effort),
        (Some("haiku"), None)
    );
    // Heard back: a new session starts with them, and the text takes its size.
    let mut settings = settings.clone();
    settings.text_size = TextSize::Large;
    settings
        .agents
        .entry("claude".into())
        .or_default()
        .allow_unsafe = true;
    heard(&mut ui, settings);
    click(&store, &mut ui, view::NEW_SESSION);
    assert_eq!(ui.model.as_deref(), Some("haiku"));
    assert_eq!(look::chat(), 15.0);
    look::set_text_size(TextSize::Medium);
    // The page draws, the search finds across pages, and bypass is offered once allowed.
    let window = limn::Size::new(1408.0, 844.0);
    click(&store, &mut ui, "settings:page:agent:claude");
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(frame
        .regions
        .iter()
        .any(|r| r.id == "set:agent:claude:unsafe"));
    ui.settings_query = "awake".into();
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(frame.regions.iter().any(|r| r.id == "set:keep_awake"));
    assert!(!frame
        .regions
        .iter()
        .any(|r| r.id == "set:agent:claude:unsafe"));
    ui.selected = Some(1);
    // The agent's modes, the model's Auto, then Bypass once allowed, Desktop's order.
    assert_eq!(
        view::menu_modes(&store, store.session(1), &ui),
        ["default", "plan", "auto", "bypassPermissions"]
    );
    // A new session offers them too, from the agent's last session, before its first message.
    assert_eq!(
        view::menu_modes(&store, None, &ui),
        ["default", "plan", "auto", "bypassPermissions"]
    );
    assert_eq!(click(&store, &mut ui, view::SETTINGS_CLOSE), vec![]);
    assert!(ui.settings_page.is_none());
}

#[test]
fn the_context_breakdown_is_asked_for_while_its_card_is_open_and_after_each_turn() {
    let (store, mut ui) = store_with_session();
    // Open from the start; folded, then opened again, it asks.
    let click = |ui: &mut Ui| {
        update(
            &store,
            ui,
            &UiAction::Clicked {
                id: view::CONTEXT_CARD.into(),
            },
        )
    };
    assert!(ui.context_open());
    assert_eq!(click(&mut ui), vec![]);
    assert_eq!(click(&mut ui), vec![Intent::ContextUsage { session: 1 }]);
    let finished = Event::Agent {
        session: 1,
        request: None,
        event: AgentEvent::TurnFinished {
            ok: true,
            interrupted: false,
            usage: Usage::default(),
            error: None,
        },
    };
    assert!(ui
        .applied(&finished)
        .contains(&Intent::ContextUsage { session: 1 }));
    // Folded again, it is not asked for.
    assert_eq!(click(&mut ui), vec![]);
    assert!(!ui
        .applied(&finished)
        .contains(&Intent::ContextUsage { session: 1 }));
}

#[test]
fn token_counts_read_as_desktop_writes_them() {
    let counts: Vec<String> = [254, 999, 15_812, 999_960, 1_000_000, 1_250_000]
        .into_iter()
        .map(view::compact)
        .collect();
    assert_eq!(counts, ["254", "999", "15.8K", "1M", "1M", "1.3M"]);
}

#[test]
fn the_hidden_sidebar_peeks_from_its_button_and_goes_once_the_pointer_left() {
    let (store, mut ui) = store_with_session();
    ui.sidebar.open = false;
    ui.hovered = Some(view::SIDEBAR_TOGGLE.into());
    ui.pointer = Some(limn::Point::new(20.0, 18.0));
    ui.tick(&store);
    assert!(ui.sidebar.peek);
    // On the sidebar it stays; off it, it goes after 180 ms.
    ui.hovered = None;
    ui.pointer = Some(limn::Point::new(120.0, 300.0));
    ui.tick(&store);
    assert!(ui.sidebar.peek);
    ui.pointer = Some(limn::Point::new(700.0, 300.0));
    ui.tick(&store);
    assert!(ui.sidebar.peek);
    std::thread::sleep(std::time::Duration::from_millis(200));
    ui.tick(&store);
    assert!(!ui.sidebar.peek);
    // The window's edge opens it once the pointer rested there.
    ui.pointer = Some(limn::Point::new(4.0, 300.0));
    ui.tick(&store);
    assert!(!ui.sidebar.peek);
    std::thread::sleep(std::time::Duration::from_millis(120));
    ui.tick(&store);
    assert!(ui.sidebar.peek);
}

#[test]
fn the_browser_opens_from_its_toggle_or_when_a_tab_appears_and_navigates_from_its_address() {
    use newen_model::{BrowserOp, BrowserTab, BrowserUpdate};
    let (mut store, mut ui) = store_with_session();
    let intents = update(
        &store,
        &mut ui,
        &UiAction::Clicked {
            id: view::BROWSER_TOGGLE.into(),
        },
    );
    assert_eq!(
        intents,
        vec![Intent::Browser {
            session: 1,
            op: BrowserOp::Open { url: None }
        }]
    );
    assert!(ui.pane_open(Pane::Browser));
    ui.close_pane(Pane::Browser);
    // A tab the agent opens shows the pane again.
    let tabs = Event::Browser {
        session: 1,
        update: BrowserUpdate::Tabs {
            tabs: vec![BrowserTab {
                id: 3,
                url: "http://localhost:3000/".into(),
                title: "Acme".into(),
                ..Default::default()
            }],
            active: Some(3),
        },
    };
    ui.applied(&tabs);
    store.apply(tabs);
    assert!(ui.pane_open(Pane::Browser));
    let intents = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::BROWSER_URL.into(),
            value: "localhost:3000/checkout".into(),
        },
    );
    assert_eq!(
        intents,
        vec![Intent::Browser {
            session: 1,
            op: BrowserOp::Navigate {
                tab: 3,
                url: "localhost:3000/checkout".into()
            }
        }]
    );
    // The page's surface is laid out where the pane's body is.
    settle(&store, &mut ui);
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let page = frame
        .regions
        .iter()
        .find(|r| r.id == view::BROWSER_VIEW)
        .unwrap();
    assert!(
        page.rect.width > 300.0 && page.rect.height > 300.0,
        "{:?}",
        page.rect
    );
}

/// The Viewport button says the page's size: on while one is set, by the agent or the person,
/// a phone for a phone's width, and the size and who set it in its tooltip; the menu marks
/// the agent's choice and "Responsive" gives the page the pane's size again. A notice has a
/// strip under the page until closed. At the narrowest column the toolbar's controls stay
/// apart and inside the pane.
#[test]
fn the_viewport_button_shows_the_size_set_and_the_toolbar_fits_the_narrowest_pane() {
    use newen_model::{BrowserOp, BrowserTab, BrowserUpdate};
    let (mut store, mut ui) = store_with_session();
    let sized = |size: Option<(u32, u32)>, by_agent: bool| Event::Browser {
        session: 1,
        update: BrowserUpdate::Tabs {
            tabs: vec![BrowserTab {
                id: 3,
                url: "http://localhost:3000/".into(),
                title: "Acme".into(),
                emulated: size,
                emulated_by_agent: by_agent,
                ..Default::default()
            }],
            active: Some(3),
        },
    };
    let hear = |store: &mut Store, ui: &mut Ui, event: Event| {
        ui.applied(&event);
        store.apply(event);
        settle(store, ui);
    };
    let window = limn::Size::new(1408.0, 844.0);
    let frame = |store: &Store, ui: &Ui| UiTree::new(view(store, ui, window)).layout(window);
    // On: the active fill under the button.
    let on = |frame: &limn::Frame| {
        let button = region(frame, view::BROWSER_VIEWPORT).unwrap();
        frame.commands.iter().any(|c| {
            matches!(c, limn::DrawCommand::FillRect { rect, color, .. }
                if *color == look::active()
                    && (rect.x - button.x).abs() < 1.0
                    && (rect.width - button.width).abs() < 1.0)
        })
    };
    let tab = |store: &Store| store.session(1).unwrap().browser.tab().cloned();
    hear(&mut store, &mut ui, sized(None, false));
    assert!(!on(&frame(&store, &ui)));
    assert_eq!(view::viewport_label(tab(&store).as_ref()), "Viewport");
    hear(&mut store, &mut ui, sized(Some((1440, 900)), true));
    assert!(on(&frame(&store, &ui)));
    assert_eq!(
        view::viewport_label(tab(&store).as_ref()),
        "Viewport: 1440 \u{d7} 900 \u{b7} set by the agent"
    );
    assert_eq!(view::viewport_icon(Some((1440, 900))), icons::MONITOR);
    // No strip says it: the button does.
    assert!(region(&frame(&store, &ui), view::BROWSER_NOTICE_CLOSE).is_none());
    // The menu marks the agent's size; "Responsive" asks for the pane's size again.
    click(&store, &mut ui, view::BROWSER_VIEWPORT);
    assert!(drawn_words(&store, &ui, "Set by the agent"));
    assert_eq!(
        click(&store, &mut ui, "browser:viewport:responsive"),
        vec![Intent::Browser {
            session: 1,
            op: BrowserOp::Viewport { tab: 3, size: None }
        }]
    );
    hear(&mut store, &mut ui, sized(Some((375, 812)), false));
    assert!(on(&frame(&store, &ui)));
    assert_eq!(
        view::viewport_label(tab(&store).as_ref()),
        "Viewport: 375 \u{d7} 812"
    );
    assert_eq!(view::viewport_icon(Some((375, 812))), icons::PHONE);
    hear(&mut store, &mut ui, sized(None, false));
    assert!(!on(&frame(&store, &ui)));
    // A failure has its strip until closed.
    hear(
        &mut store,
        &mut ui,
        Event::Browser {
            session: 1,
            update: BrowserUpdate::Failed("The browser stopped.".into()),
        },
    );
    assert!(drawn_words(&store, &ui, "The browser stopped."));
    click(&store, &mut ui, view::BROWSER_NOTICE_CLOSE);
    assert!(!drawn_words(&store, &ui, "The browser stopped."));
    // The narrowest column, with a size set and a notice.
    hear(&mut store, &mut ui, sized(Some((1440, 900)), true));
    hear(
        &mut store,
        &mut ui,
        Event::Browser {
            session: 1,
            update: BrowserUpdate::Notice(
                "The page said: are you sure you want to leave this page and lose your changes?"
                    .into(),
            ),
        },
    );
    ui.column_fraction = Some(0.0);
    let window = limn::Size::new(900.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let page = region(&frame, view::BROWSER_VIEW).unwrap();
    assert!(page.width < view::MIN_COLUMN, "{page:?}");
    // Back and forward, with nowhere to go, are drawn without a press, so without a region.
    let controls = [
        view::BROWSER_RELOAD,
        view::BROWSER_URL,
        view::BROWSER_VIEWPORT,
        view::BROWSER_SELECT,
    ]
    .map(|id| (id, region(&frame, id).unwrap_or_else(|| panic!("no {id}"))));
    for (i, (id, r)) in controls.iter().enumerate() {
        assert!(
            r.x >= page.x - 0.5 && r.x + r.width <= page.x + page.width + 0.5,
            "{id} {r:?} outside {page:?}"
        );
        for (other, o) in &controls[i + 1..] {
            assert!(r.x + r.width <= o.x + 0.5, "{id} {r:?} over {other} {o:?}");
        }
    }
    let close = region(&frame, view::BROWSER_NOTICE_CLOSE).unwrap();
    assert!(close.x + close.width <= page.x + page.width + 0.5);
    let words = drawn_spans(&frame)
        .into_iter()
        .find(|s| s.0.starts_with("The page said"))
        .unwrap();
    assert!(words.3 <= close.x + 0.5, "{words:?} under {close:?}");
    assert_apart(&drawn_spans(&frame), "narrowest");
}

#[test]
fn browser_steps_read_in_desktops_words_and_fold_into_what_they_did() {
    let done = ToolResult {
        output: String::new(),
        is_error: false,
        added: 0,
        removed: 0,
        files: vec![],
        stopped: false,
        web: None,
        images: vec![],
    };
    let inputs = [
        serde_json::json!({ "url": "https://www.example.com/a" }),
        serde_json::json!({}),
        serde_json::json!({ "action": "left_click", "coordinate": [10, 10] }),
        serde_json::json!({ "action": "screenshot" }),
        serde_json::json!({ "query": "Pay" }),
    ];
    let names = [
        "mcp__browser__navigate",
        "mcp__browser__read_page",
        "mcp__browser__computer",
        "mcp__browser__computer",
        "mcp__browser__find",
    ];
    let steps: Vec<view::Step> = names
        .iter()
        .zip(&inputs)
        .map(|(name, input)| view::Step {
            call: "c",
            name,
            input,
            result: Some(&done),
            running: false,
        })
        .collect();
    assert_eq!(view::group_summary(&steps[..1]), "Opened example.com");
    // Up to three things it did, then an ellipsis.
    assert_eq!(
        view::group_summary(&steps),
        "Opened, read page, clicked\u{2026}"
    );
    let running = view::Step {
        call: "c",
        name: names[3],
        input: &inputs[3],
        result: None,
        running: true,
    };
    assert_eq!(view::group_summary(&[running]), "Capturing page");
}

#[test]
fn the_files_pane_lists_the_folder_opens_a_file_and_follows_a_rename() {
    let (store, mut ui) = store_with_session();
    let list = |dir: &str| Intent::ListDir {
        session: 1,
        dir: dir.into(),
    };
    assert_eq!(
        click(&store, &mut ui, view::FILES_TOGGLE),
        vec![list("/work")]
    );
    assert!(ui.pane_open(Pane::Files));
    let entry = |path: &str, dir| newen_files::Entry {
        path: path.into(),
        dir,
    };
    ui.applied(&Event::Listed {
        session: 1,
        dir: "/work".into(),
        entries: vec![entry("/work/src", true), entry("/work/a.md", false)],
        stamp: None,
    });
    settle(&store, &mut ui);
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(frame.regions.iter().any(|r| r.id == "file:/work/a.md"));
    // A folder opens in place, listed the first time.
    assert_eq!(
        click(&store, &mut ui, "dir:/work/src"),
        vec![list("/work/src")]
    );
    ui.applied(&Event::Listed {
        session: 1,
        dir: "/work/src".into(),
        entries: vec![],
        stamp: None,
    });
    // A file opens in the document pane.
    assert_eq!(
        click(&store, &mut ui, "file:/work/a.md"),
        vec![Intent::ReadFile {
            session: 1,
            path: "a.md".into()
        }]
    );
    assert!(ui.pane_open(Pane::Files));
    // Renamed, the document goes with it; in the trash, it closes.
    let rename = newen_files::Op::Rename {
        path: "a.md".into(),
        name: "b.md".into(),
    };
    ui.applied(&Event::FileOpDone {
        session: 1,
        op: rename,
        result: Ok(vec!["b.md".into()]),
    });
    assert_eq!(ui.document().unwrap().path, "b.md");
    let trash = newen_files::Op::Trash {
        path: "b.md".into(),
    };
    ui.applied(&Event::FileOpDone {
        session: 1,
        op: trash,
        result: Ok(vec![]),
    });
    assert!(ui.document().is_none());
    // Closed and opened again, it lists again what it had.
    click(&store, &mut ui, view::FILES_TOGGLE);
    let again = click(&store, &mut ui, view::FILES_TOGGLE);
    assert!(again.contains(&list("/work")) && again.contains(&list("/work/src")));
}

#[test]
fn code_from_an_editor_goes_to_its_folders_session_as_a_mention() {
    let mut store = Store::default();
    for (session, cwd) in [(1, "/work"), (2, "/other"), (3, "/work")] {
        store.apply(Event::SessionCreated {
            session,
            agent: "claude".into(),
            cwd: cwd.into(),
        });
    }
    let mut ui = Ui::new("/work".into());
    // The latest of the folder, unless the one on screen works there.
    assert_eq!(session_for(&store, &ui, Path::new("/work")), Some(3));
    ui.selected = Some(1);
    assert_eq!(session_for(&store, &ui, Path::new("/work")), Some(1));
    assert_eq!(session_for(&store, &ui, Path::new("/elsewhere")), None);
    let root = Path::new("/work");
    assert_eq!(
        mention(root, Path::new("/work/src/a.rs"), None, ""),
        "@src/a.rs "
    );
    assert_eq!(
        mention(
            root,
            Path::new("/work/src/a.rs"),
            Some((3, 4)),
            "let a = 1;\nlet b = 2;\n"
        ),
        "@src/a.rs (lines 3–4):\n```rs\nlet a = 1;\nlet b = 2;\n```\n"
    );
    // Code holding a fence gets a longer one.
    assert!(
        mention(root, Path::new("/work/a.md"), Some((1, 1)), "```x```")
            .starts_with("@a.md (line 1):\n````md\n")
    );
}

#[test]
fn a_wider_pane_widens_the_column_and_panes_stack_and_expand_on_springs() {
    let (store, mut ui) = store_with_session();
    ui.open_pane(Pane::Terminal);
    ui.tick(&store);
    assert_eq!(ui.column_share.target(), 1.0 / 3.0);
    // The file tile takes a reader's width, and opens under the terminal.
    ui.open_pane(Pane::Files);
    ui.tick(&store);
    assert_eq!(ui.column_share.target(), 0.5);
    assert_eq!(ui.stack_shown.target(), 1.0);
    // Expanded, it grows over the conversation, and moves while it does.
    ui.expanded_pane = Some(Pane::Files);
    ui.tick(&store);
    assert_eq!(ui.expand_shown.target(), 1.0);
    assert!(ui.animating());
}

#[test]
fn the_pr_request_names_the_branch_prefix() {
    assert!(
        pr_request(false, newen_model::Settings::default().branch_prefix()).contains("`newen/`")
    );
    let settings = newen_model::Settings {
        branch_prefix: "team/".into(),
        ..newen_model::Settings::default()
    };
    assert!(pr_request(true, settings.branch_prefix()).contains("`team/`"));
}

/// Whether the agent can show what its model thought, as its `Ready` says.
fn thinking_agent(store: &mut Store, thinking_text: bool) {
    agent(
        store,
        AgentEvent::Ready {
            version: "2.1.310".into(),
            capabilities: newen_model::AgentCapabilities {
                commands: vec![],
                thinking_text,
                ..Default::default()
            },
            mode: None,
        },
    );
}

#[test]
fn the_view_is_the_sessions_choice_then_the_default_and_thinking_falls_back() {
    use newen_model::{Settings, TranscriptView::*};
    let (mut store, _ui) = store_with_session();
    thinking_agent(&mut store, true);
    let settings = Settings {
        transcript_view: Thinking,
        ..Settings::default()
    };
    let shown =
        |store: &Store, settings: &Settings| view_of(store.session(1).unwrap(), settings, true);
    assert_eq!(shown(&store, &Settings::default()), Normal);
    assert_eq!(shown(&store, &settings), Thinking);
    store.apply(Event::ViewChosen {
        session: 1,
        view: Some(Verbose),
    });
    assert_eq!(shown(&store, &settings), Verbose);
    store.apply(Event::ViewChosen {
        session: 1,
        view: None,
    });
    assert_eq!(shown(&store, &settings), Thinking);

    // An agent that cannot show its thinking leaves Thinking to Normal.
    let (mut plain, _ui) = store_with_session();
    thinking_agent(&mut plain, false);
    assert_eq!(view_of(plain.session(1).unwrap(), &settings, false), Normal);
    // So does a session that has its answer and no thinking in it.
    agent(
        &mut store,
        AgentEvent::Text {
            text: "Hello".into(),
            parent: None,
            id: None,
            at: None,
        },
    );
    store.apply(Event::ViewChosen {
        session: 1,
        view: None,
    });
    assert_eq!(shown(&store, &settings), Normal);
}

#[test]
fn thinking_is_drawn_in_thinking_and_verbose_and_copied_as_a_quote() {
    let (mut store, ui) = store_with_session();
    thinking_agent(&mut store, true);
    for text in ["Numbers ", "divide"] {
        agent(
            &mut store,
            AgentEvent::ThinkingDelta {
                text: text.into(),
                parent: None,
            },
        );
    }
    agent(
        &mut store,
        AgentEvent::ThinkingText {
            text: "Numbers divide".into(),
            parent: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::Text {
            text: "Hello".into(),
            parent: None,
            id: None,
            at: None,
        },
    );
    let window = limn::Size::new(1408.0, 844.0);
    let drawn = |store: &Store, ui: &Ui, words: &str| {
        let frame = UiTree::new(view(store, ui, window)).layout(window);
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    assert!(!drawn(&store, &ui, "Numbers divide"));
    store.apply(Event::ViewChosen {
        session: 1,
        view: Some(TranscriptView::Thinking),
    });
    assert!(drawn(&store, &ui, "Numbers divide"));
    assert!(drawn(&store, &ui, "Thought process"));
    store.apply(Event::ViewChosen {
        session: 1,
        view: Some(TranscriptView::Verbose),
    });
    assert!(drawn(&store, &ui, "Numbers divide"));
    assert_eq!(
        copied_text(&store, "copy:1:1").as_deref(),
        Some("> Numbers divide")
    );
}

#[test]
fn verbose_opens_a_tools_output_that_normal_keeps_folded() {
    let (mut store, mut ui) = store_with_session();
    tool(
        &mut store,
        "c1",
        "Bash",
        serde_json::json!({"command": "echo hi"}),
        None,
    );
    result(&mut store, "c1", "verbose-output");
    let window = limn::Size::new(1408.0, 844.0);
    let drawn = |store: &Store, ui: &Ui| {
        let frame = UiTree::new(view(store, ui, window)).layout(window);
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains("verbose-output"))
        })
    };
    assert!(!drawn(&store, &ui));
    store.apply(Event::ViewChosen {
        session: 1,
        view: Some(TranscriptView::Verbose),
    });
    assert!(drawn(&store, &ui));
    // Normal keeps the group's and the row's own toggles.
    store.apply(Event::ViewChosen {
        session: 1,
        view: Some(TranscriptView::Normal),
    });
    assert!(!drawn(&store, &ui));
    click(&store, &mut ui, "steps:c1");
    assert!(!drawn(&store, &ui));
    click(&store, &mut ui, "tool:c1");
    assert!(drawn(&store, &ui));
}

#[test]
fn ctrl_o_cycles_the_view_and_skips_thinking_when_the_agent_lacks_it() {
    let (mut store, mut ui) = store_with_session();
    thinking_agent(&mut store, true);
    for (from, to) in [
        (None, Some(TranscriptView::Thinking)),
        (
            Some(TranscriptView::Thinking),
            Some(TranscriptView::Verbose),
        ),
        (Some(TranscriptView::Verbose), Some(TranscriptView::Normal)),
    ] {
        store.apply(Event::ViewChosen {
            session: 1,
            view: from,
        });
        assert_eq!(
            click(&store, &mut ui, view::TRANSCRIPT_VIEW_CYCLE),
            vec![Intent::ChooseView {
                session: 1,
                view: to
            }]
        );
    }

    let (mut plain, mut ui) = store_with_session();
    thinking_agent(&mut plain, false);
    assert_eq!(
        click(&plain, &mut ui, view::TRANSCRIPT_VIEW_CYCLE),
        vec![Intent::ChooseView {
            session: 1,
            view: Some(TranscriptView::Verbose)
        }]
    );
}

#[test]
fn the_session_menu_chooses_a_view_and_makes_it_the_default() {
    use newen_model::Settings;
    let (mut store, mut ui) = store_with_session();
    thinking_agent(&mut store, true);
    assert_eq!(
        click(&store, &mut ui, "view:1:verbose"),
        vec![Intent::ChooseView {
            session: 1,
            view: Some(TranscriptView::Verbose)
        }]
    );
    store.apply(Event::ViewChosen {
        session: 1,
        view: Some(TranscriptView::Verbose),
    });
    assert_eq!(
        click(&store, &mut ui, "view-default:1"),
        vec![
            Intent::SaveSettings(Settings {
                transcript_view: TranscriptView::Verbose,
                ..Settings::default()
            }),
            Intent::ChooseView {
                session: 1,
                view: None
            },
        ]
    );
    // The Settings page changes the default the same way.
    click(&store, &mut ui, view::SETTINGS_OPEN);
    let saved = click(&store, &mut ui, "set:transcript_view=thinking");
    assert!(matches!(
        saved.as_slice(),
        [Intent::SaveSettings(s)] if s.transcript_view == TranscriptView::Thinking
    ));
}

#[test]
fn a_notice_is_a_banner_only_while_its_session_is_out_of_view() {
    use newen_model::NotifyLevel;
    let (store, mut ui) = store_with_session();
    let asked = Event::Agent {
        session: 1,
        request: Some("r1".into()),
        event: AgentEvent::PermissionRequest {
            id: "p1".into(),
            call: None,
            tool: "Bash".into(),
            input: serde_json::json!({"command": "ls"}),
            title: None,
            interactive: false,
            suggestions: vec![],
        },
    };
    // In view and focused there is nothing; out of focus, a banner titled with the session.
    assert_eq!(notice(&store, &ui, &asked, true), None);
    assert_eq!(
        notice(&store, &ui, &asked, false),
        Some((
            NotifyLevel::Banner,
            Intent::Notify {
                session: 1,
                title: "Fix the bug".into(),
                body: NOTICE_PERMISSION.into(),
                sound: Some(newen_model::Chime::Ask),
            }
        ))
    );
    // Focused on another session, it is still a banner.
    ui.selected = None;
    assert!(notice(&store, &ui, &asked, true).is_some());
    // Off says nothing.
    ui.settings.notify_permission = NotifyLevel::Off;
    assert_eq!(notice(&store, &ui, &asked, false), None);
}

#[test]
fn each_notice_says_its_own_text_and_a_status_alone_is_none() {
    use newen_model::NotifyLevel;
    let (mut store, mut ui) = store_with_session();
    ui.selected = None;
    let asked = |interactive: bool| Event::Agent {
        session: 1,
        request: Some("r1".into()),
        event: AgentEvent::PermissionRequest {
            id: "q1".into(),
            call: None,
            tool: "AskUserQuestion".into(),
            input: serde_json::json!({}),
            title: None,
            interactive,
            suggestions: vec![],
        },
    };
    let text = |event: &Event, ui: &Ui| match notice(&store, ui, event, false) {
        Some((_, Intent::Notify { body, .. })) => Some(body),
        _ => None,
    };
    assert_eq!(text(&asked(true), &ui), Some(NOTICE_QUESTION.into()));
    assert_eq!(text(&asked(false), &ui), Some(NOTICE_PERMISSION.into()));
    let finished = Event::Agent {
        session: 1,
        request: None,
        event: AgentEvent::TurnFinished {
            ok: true,
            interrupted: false,
            usage: Usage::default(),
            error: None,
        },
    };
    assert_eq!(text(&finished, &ui), Some(NOTICE_DONE.into()));
    // Desktop's "Task complete" has no badge: a badge reads as a banner.
    ui.settings.notify_done = NotifyLevel::Badge;
    assert_eq!(
        notice(&store, &ui, &finished, false).map(|n| n.0),
        Some(NotifyLevel::Banner)
    );
    ui.settings.notify_done = NotifyLevel::Off;
    assert_eq!(notice(&store, &ui, &finished, false), None);
    // A side chat's turn is not a top-level one, so it is no notice.
    ui.settings.notify_done = NotifyLevel::Banner;
    store.session_mut(1).unwrap().aside = true;
    assert_eq!(notice(&store, &ui, &finished, false), None);
    // The waiting status alone is no notice: the permission request is what asks.
    let waiting = Event::Status {
        session: 1,
        status: SessionStatus::Awaiting,
    };
    assert_eq!(notice(&store, &ui, &waiting, false), None);
}

/// A finished task asks for Newen's "done" sound and a question for its "ask" one, as
/// Settings has it; none while the person looks at that very session in the focused window.
#[test]
fn a_banner_asks_for_newens_own_sound_per_settings_and_not_in_view() {
    use newen_model::Chime;
    let (store, mut ui) = store_with_session();
    let finished = Event::Agent {
        session: 1,
        request: None,
        event: AgentEvent::TurnFinished {
            ok: true,
            interrupted: false,
            usage: Usage::default(),
            error: None,
        },
    };
    let sound = |ui: &Ui, focused: bool| match notice(&store, ui, &finished, focused) {
        Some((_, Intent::Notify { sound, .. })) => Some(sound),
        _ => None,
    };
    // In view, focused: no banner and no sound; out of focus, the sound of a finished task.
    assert_eq!(sound(&ui, true), None);
    assert_eq!(sound(&ui, false), Some(Some(Chime::Done)));
    ui.selected = None;
    assert_eq!(sound(&ui, true), Some(Some(Chime::Done)));
    // "None" in Settings: the banner alone.
    ui.settings.notification_sound = false;
    assert_eq!(sound(&ui, true), Some(None));
}

/// A click on a session's banner opens that session and reads it.
#[test]
fn a_banner_click_opens_its_session() {
    let (mut store, mut ui) = store_with_session();
    ui.selected = None;
    store.apply(Event::Unread {
        session: 1,
        unread: true,
    });
    let intents = banner_clicked(&store, &mut ui, 1);
    assert_eq!(ui.selected, Some(1));
    assert!(intents.contains(&Intent::MarkUnread {
        session: 1,
        unread: false
    }));
}

fn slash_command(name: &str, aliases: &[&str]) -> newen_model::AgentSlashCommand {
    newen_model::AgentSlashCommand {
        name: name.into(),
        description: format!("{name} it"),
        argument_hint: String::new(),
        aliases: aliases.iter().map(|a| a.to_string()).collect(),
    }
}

/// The session's agent offers these commands, as its `Ready` says.
fn with_commands(store: &mut Store) {
    agent(
        store,
        AgentEvent::Ready {
            version: "2.1.310".into(),
            capabilities: newen_model::AgentCapabilities {
                commands: vec![
                    slash_command("compact", &["cp"]),
                    slash_command("cp-tool", &[]),
                    slash_command("cpx", &[]),
                    slash_command("ship-cp", &[]),
                    slash_command("zzz", &[]),
                ],
                ..Default::default()
            },
            mode: None,
        },
    );
}

fn slash_ids(store: &Store, ui: &Ui) -> Vec<String> {
    crate::slash::open(store, ui)
        .map(|(_, rows)| rows.into_iter().map(|(id, _)| id).collect())
        .unwrap_or_default()
}

fn key(key: limn::Key) -> limn::UiEvent {
    limn::UiEvent::Key {
        key,
        ctrl: false,
        shift: false,
        alt: false,
    }
}

#[test]
fn slash_query_takes_the_word_at_the_end() {
    assert_eq!(slash_query("/"), Some((0, "")));
    assert_eq!(slash_query("fix it /mo"), Some((7, "mo")));
    // Inside a word, or with a space after the query, the palette stays shut.
    assert_eq!(slash_query("a/b"), None);
    assert_eq!(slash_query("/mo "), None);
    assert_eq!(slash_query("/mo x"), None);
    let longest = "a".repeat(50);
    assert_eq!(
        slash_query(&format!("/{longest}")),
        Some((0, longest.as_str()))
    );
    assert_eq!(slash_query(&format!("/{longest}a")), None);
}

#[test]
fn slash_rows_rank_exact_then_prefix_then_substring() {
    let (mut store, mut ui) = store_with_session();
    with_commands(&mut store);
    ui.prompt = "/cp".into();
    // The exact alias, then the name prefixes in their order, then the substring.
    assert_eq!(
        slash_ids(&store, &ui),
        [
            "slash:agent:compact",
            "slash:agent:cp-tool",
            "slash:agent:cpx",
            // Newen's "mcp" holds the substring too, and comes first among equal ranks.
            "slash:newen:plus:connectors:manage",
            "slash:agent:ship-cp",
        ]
    );
}

#[test]
fn slash_typed_mo_lists_model_first() {
    let (mut store, mut ui) = store_with_session();
    with_commands(&mut store);
    ui.prompt = "/mo".into();
    let ids = slash_ids(&store, &ui);
    assert_eq!(
        ids.first().map(String::as_str),
        Some("slash:newen:argument:model")
    );
}

#[test]
fn the_pointer_over_a_palette_row_chooses_it() {
    let (mut store, mut ui) = store_with_session();
    with_commands(&mut store);
    ui.prompt = "/".into();
    let ids = slash_ids(&store, &ui);
    crate::palette_hover(&store, &mut ui, &ids[2]);
    assert_eq!(ui.slash_selected, 2);
    // Something else under the pointer leaves the choice alone.
    crate::palette_hover(&store, &mut ui, view::COMPOSER);
    assert_eq!(ui.slash_selected, 2);
}

/// The session's agent offers two models; the first is the one the efforts and modes come from.
fn with_models(store: &mut Store) {
    agent(
        store,
        AgentEvent::Ready {
            version: "2.1.310".into(),
            capabilities: newen_model::AgentCapabilities {
                models: vec![
                    newen_model::ModelInfo {
                        id: "opus".into(),
                        name: "Opus 5.5".into(),
                        efforts: vec!["low".into(), "medium".into(), "high".into()],
                        modes: vec![],
                        default: false,
                    },
                    newen_model::ModelInfo {
                        id: "haiku".into(),
                        name: "Haiku".into(),
                        efforts: vec!["low".into(), "high".into()],
                        modes: vec!["auto".into()],
                        default: false,
                    },
                ],
                modes: vec!["default".into(), "plan".into()],
                ..Default::default()
            },
            mode: None,
        },
    );
}

#[test]
fn slash_model_writes_its_argument_and_lists_the_models() {
    let (mut store, mut ui) = store_with_session();
    with_models(&mut store);
    ui.prompt = "/mo".into();
    assert_eq!(
        slash_key(
            &store,
            &mut ui,
            Some(view::COMPOSER),
            &key(limn::Key::Enter)
        ),
        Some(vec![])
    );
    // The menu does not open: the prompt gets `/model `, and the palette lists the models.
    assert_eq!(ui.restore_prompt.as_deref(), Some("/model "));
    assert_eq!(ui.open_menu, None);
    ui.prompt = "/model ".into();
    assert_eq!(
        slash_ids(&store, &ui),
        ["slash:option:model:opus", "slash:option:model:haiku"]
    );
}

#[test]
fn slash_effort_argument_applies_the_first_match() {
    let (mut store, mut ui) = store_with_session();
    with_models(&mut store);
    ui.model = Some("opus".into());
    ui.prompt = "/effort hi".into();
    assert_eq!(
        slash_key(
            &store,
            &mut ui,
            Some(view::COMPOSER),
            &key(limn::Key::Enter)
        ),
        Some(vec![Intent::SetEffort {
            session: 1,
            effort: "high".into()
        }])
    );
    assert_eq!(ui.effort.as_deref(), Some("high"));
    // The token goes; the palette has nothing left to show.
    assert_eq!(ui.restore_prompt.as_deref(), Some(""));
}

#[test]
fn slash_tab_completes_a_command_that_takes_an_argument() {
    let (store, mut ui) = store_with_session();
    ui.prompt = "/mod".into();
    assert_eq!(
        slash_key(&store, &mut ui, Some(view::COMPOSER), &key(limn::Key::Tab)),
        Some(vec![])
    );
    assert_eq!(ui.restore_prompt.as_deref(), Some("/model "));
}

#[test]
fn slash_permissions_lists_the_offered_modes() {
    let (mut store, mut ui) = store_with_session();
    with_models(&mut store);
    ui.agents = an_agent_known();
    ui.model = Some("haiku".into());
    ui.prompt = "/permissions ".into();
    assert_eq!(
        slash_ids(&store, &ui),
        [
            "slash:option:mode:default",
            "slash:option:mode:plan",
            "slash:option:mode:auto"
        ]
    );
    ui.prompt = "/permissions pl".into();
    assert_eq!(slash_ids(&store, &ui), ["slash:option:mode:plan"]);
}

#[test]
fn a_menu_picked_from_the_palette_keeps_the_palette_shut_until_it_closes() {
    let (store, mut ui) = store_with_session();
    ui.prompt = "/usage".into();
    assert!(slash_key(
        &store,
        &mut ui,
        Some(view::COMPOSER),
        &key(limn::Key::Enter)
    )
    .is_some());
    assert_eq!(ui.open_menu, Some(view::USAGE_MENU));
    // The composer keeps its text and the palette stays shut under the menu.
    assert_eq!(ui.restore_prompt, None);
    assert!(slash_ids(&store, &ui).is_empty());
    // Closing the menu gives the prompt back without the token.
    click(&store, &mut ui, view::USAGE_MENU);
    assert_eq!(ui.open_menu, None);
    assert_eq!(ui.restore_prompt.as_deref(), Some(""));
}

#[test]
fn slash_enter_on_an_agent_command_writes_its_name() {
    let (mut store, mut ui) = store_with_session();
    with_commands(&mut store);
    ui.prompt = "look /cp".into();
    assert_eq!(
        slash_key(
            &store,
            &mut ui,
            Some(view::COMPOSER),
            &key(limn::Key::Enter)
        ),
        Some(vec![])
    );
    assert_eq!(ui.restore_prompt.as_deref(), Some("look /compact "));
}

#[test]
fn slash_escape_shuts_it_until_the_slash_is_typed_again() {
    let (mut store, mut ui) = store_with_session();
    with_commands(&mut store);
    ui.prompt = "/mo".into();
    assert_eq!(
        slash_key(
            &store,
            &mut ui,
            Some(view::COMPOSER),
            &key(limn::Key::Escape)
        ),
        Some(vec![])
    );
    assert!(slash_ids(&store, &ui).is_empty());
    // Typing on the same `/` keeps it shut.
    let type_text = |store: &Store, ui: &mut Ui, text: &str| {
        update(
            store,
            ui,
            &UiAction::InputChanged {
                id: view::COMPOSER.into(),
                value: text.into(),
            },
        )
    };
    type_text(&store, &mut ui, "/mom");
    assert!(slash_ids(&store, &ui).is_empty());
    // Deleting the `/` and typing it again opens it.
    type_text(&store, &mut ui, "mo");
    type_text(&store, &mut ui, "/mo");
    assert!(!slash_ids(&store, &ui).is_empty());
}

#[test]
fn slash_with_no_match_leaves_enter_to_send() {
    let (store, mut ui) = store_with_session();
    ui.prompt = "/zq".into();
    assert_eq!(
        slash_key(
            &store,
            &mut ui,
            Some(view::COMPOSER),
            &key(limn::Key::Enter)
        ),
        None
    );
}

#[test]
fn slash_newen_command_clears_the_token_and_runs_its_action() {
    let (store, mut ui) = store_with_session();
    ui.prompt = "/diff".into();
    assert_eq!(
        slash_key(
            &store,
            &mut ui,
            Some(view::COMPOSER),
            &key(limn::Key::Enter)
        ),
        Some(vec![Intent::WorkingTree { session: 1 }])
    );
    assert_eq!(ui.restore_prompt.as_deref(), Some(""));
}

/// A session with a title, made after the others (its activity follows their order).
fn named(store: &mut Store, session: u64, title: &str, cwd: &str) {
    store.apply(Event::SessionCreated {
        session,
        agent: "claude".into(),
        cwd: cwd.into(),
    });
    store.apply(Event::Renamed {
        session,
        title: title.into(),
    });
    store.apply(Event::Queued {
        session,
        request: format!("r{session}"),
        text: "hi".into(),
        at: None,
        images: vec![],
    });
}

fn mention_ids(store: &Store, ui: &Ui) -> Vec<String> {
    crate::mention::open(store, ui)
        .map(|(_, rows)| rows.into_iter().map(|(id, _)| id).collect())
        .unwrap_or_default()
}

fn type_prompt(store: &Store, ui: &mut Ui, text: &str) -> Vec<Intent> {
    update(
        store,
        ui,
        &UiAction::InputChanged {
            id: view::COMPOSER.into(),
            value: text.into(),
        },
    )
}

#[test]
fn mention_query_takes_the_word_at_the_end() {
    assert_eq!(mention_query("@"), Some((0, "")));
    assert_eq!(mention_query("look @ma"), Some((5, "ma")));
    assert_eq!(mention_query("a@b"), None);
    assert_eq!(mention_query("@ma "), None);
    assert_eq!(mention_query("/ma"), None);
    assert_eq!(
        mention_query(&format!("@{}", "a".repeat(50))).map(|(_, q)| q.len()),
        Some(50)
    );
    assert_eq!(mention_query(&format!("@{}", "a".repeat(51))), None);
}

#[test]
fn mention_sessions_skip_archived_untitled_and_the_one_on_screen() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::Renamed {
        session: 1,
        title: "Fix login".into(),
    });
    named(&mut store, 2, "Login page", "/work/web");
    named(&mut store, 3, "Old login", "/work/old");
    store.apply(Event::Archived {
        session: 3,
        archived: true,
    });
    named(&mut store, 4, "", "/work/untitled");
    ui.prompt = "@log".into();
    assert_eq!(mention_ids(&store, &ui), ["mention:session:2"]);
}

#[test]
fn mention_lists_three_sessions_empty_and_eight_with_a_query() {
    let (mut store, mut ui) = store_with_session();
    for id in 2..=12 {
        named(&mut store, id, &format!("Task {id}"), "/work/app");
    }
    ui.prompt = "@".into();
    let empty = mention_ids(&store, &ui);
    assert_eq!(empty.len(), 3);
    assert_eq!(empty[0], "mention:session:12", "most recent first");
    ui.prompt = "@task".into();
    assert_eq!(mention_ids(&store, &ui).len(), 8);
}

#[test]
fn mention_sessions_match_title_without_spaces_and_branch() {
    let (mut store, mut ui) = store_with_session();
    named(&mut store, 2, "Rate limits", "/work/api");
    store
        .sessions
        .iter_mut()
        .find(|s| s.id == 2)
        .unwrap()
        .branch = Some("feat/quota".into());
    ui.prompt = "@ratelim".into();
    assert_eq!(mention_ids(&store, &ui), ["mention:session:2"]);
    ui.prompt = "@quota".into();
    assert_eq!(mention_ids(&store, &ui), ["mention:session:2"]);
    ui.prompt = "@zzz".into();
    assert!(mention_ids(&store, &ui).is_empty());
}

#[test]
fn mention_files_are_kept_only_for_the_query_asked() {
    let (mut store, mut ui) = store_with_session();
    named(&mut store, 2, "Login page", "/work/web");
    let found = |query: &str| Event::FilesFound {
        session: 1,
        query: query.into(),
        contents: false,
        paths: vec!["src/main.rs".into()],
        hits: vec![],
    };
    assert_eq!(
        type_prompt(&store, &mut ui, "@ma"),
        [Intent::FindFiles {
            session: 1,
            query: "ma".into(),
            contents: false,
        }]
    );
    ui.applied(&found("ma"));
    assert_eq!(mention_ids(&store, &ui), ["mention:file:src/main.rs"]);
    // Typing on asks again, and an answer to the old query is let go.
    assert_eq!(
        type_prompt(&store, &mut ui, "@mai"),
        [Intent::FindFiles {
            session: 1,
            query: "mai".into(),
            contents: false,
        }]
    );
    ui.applied(&found("ma"));
    assert!(mention_ids(&store, &ui).is_empty());
    ui.applied(&found("mai"));
    assert_eq!(mention_ids(&store, &ui), ["mention:file:src/main.rs"]);
}

#[test]
fn mention_enter_writes_a_file_and_a_session_by_its_title() {
    let (mut store, mut ui) = store_with_session();
    named(&mut store, 2, "Login page", "/work/web");
    ui.prompt = "look @log".into();
    ui.applied(&Event::FilesFound {
        session: 1,
        query: "log".into(),
        contents: false,
        paths: vec!["src/login.rs".into()],
        hits: vec![],
    });
    // The session first, then the file.
    assert_eq!(
        mention_ids(&store, &ui),
        ["mention:session:2", "mention:file:src/login.rs"]
    );
    // Tab does not pick.
    assert!(!mention_key(
        &store,
        &mut ui,
        Some(view::COMPOSER),
        &key(limn::Key::Tab)
    ));
    assert!(mention_key(
        &store,
        &mut ui,
        Some(view::COMPOSER),
        &key(limn::Key::Enter)
    ));
    assert_eq!(ui.restore_prompt.as_deref(), Some("look @[Login page] "));
    assert_eq!(ui.mentioned_sessions, [(2, "Login page".to_owned())]);
    // Down to the file, then Enter.
    assert!(mention_key(
        &store,
        &mut ui,
        Some(view::COMPOSER),
        &key(limn::Key::ArrowDown)
    ));
    assert!(mention_key(
        &store,
        &mut ui,
        Some(view::COMPOSER),
        &key(limn::Key::Enter)
    ));
    assert_eq!(ui.restore_prompt.as_deref(), Some("look @src/login.rs "));
    assert_eq!(ui.mentioned_sessions.len(), 1);
}

#[test]
fn mention_escape_stays_shut_for_the_same_query() {
    let (mut store, mut ui) = store_with_session();
    named(&mut store, 2, "Login page", "/work/web");
    ui.prompt = "@log".into();
    assert!(mention_key(
        &store,
        &mut ui,
        Some(view::COMPOSER),
        &key(limn::Key::Escape)
    ));
    assert!(mention_ids(&store, &ui).is_empty());
    type_prompt(&store, &mut ui, "@log");
    assert!(mention_ids(&store, &ui).is_empty());
    type_prompt(&store, &mut ui, "@logi");
    assert_eq!(mention_ids(&store, &ui), ["mention:session:2"]);
}

#[test]
fn slash_and_mention_never_open_together() {
    let (mut store, mut ui) = store_with_session();
    with_commands(&mut store);
    named(&mut store, 2, "Login page", "/work/web");
    ui.prompt = "/mo".into();
    assert!(mention_ids(&store, &ui).is_empty());
    assert!(!slash_ids(&store, &ui).is_empty());
    ui.prompt = "look @log".into();
    assert!(slash_ids(&store, &ui).is_empty());
    assert_eq!(mention_ids(&store, &ui), ["mention:session:2"]);
}

#[test]
fn a_sent_session_mention_goes_to_the_engine_numbered_and_not_into_the_text() {
    let (mut store, mut ui) = store_with_session();
    named(&mut store, 2, "Login page", "/work/web");
    named(&mut store, 3, "Billing", "/work/api");
    // "Gone" was picked and then deleted from the text: it is not sent.
    ui.mentioned_sessions = vec![
        (2, "Login page".to_owned()),
        (3, "Billing".to_owned()),
        (4, "Gone".to_owned()),
    ];
    let typed = "@[Billing] and @[Login page] see the form";
    let sent = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::COMPOSER.into(),
            value: typed.into(),
        },
    );
    assert_eq!(
        sent,
        [Intent::Send {
            session: 1,
            text: typed.into(),
            mentions: vec![(3, "Billing".to_owned()), (2, "Login page".to_owned())],
            images: vec![],
            lane: Lane::Steer,
        }]
    );
    assert!(ui.mentioned_sessions.is_empty());
}

#[test]
fn the_composer_takes_png_jpeg_gif_and_webp_by_name_in_any_case() {
    for name in ["a.png", "b.JPG", "c.jpeg", "d.gif", "e.WebP"] {
        assert!(is_image(Path::new(name)), "{name}");
    }
    for name in ["notes.md", "image.bmp", "png", "photo.png.txt", "dir/"] {
        assert!(!is_image(Path::new(name)), "{name}");
    }
}

#[test]
fn each_attached_image_is_a_tile_with_remove_and_open_regions() {
    let (store, mut ui) = store_with_session();
    add_images(&mut ui, vec!["/work/a.png".into(), "/work/b.jpg".into()]);
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    for id in [
        "image:remove:0",
        "image:open:0",
        "image:remove:1",
        "image:open:1",
    ] {
        assert!(frame.regions.iter().any(|r| r.id == id), "no {id}");
    }
    assert!(!frame.regions.iter().any(|r| r.id == "image:remove:2"));
}

#[test]
fn a_composer_image_opens_an_annotatable_viewer_with_close_and_annotate() {
    let (store, mut ui) = store_with_session();
    add_images(&mut ui, vec!["/work/a.png".into()]);
    click(&store, &mut ui, "image:open:0");
    let viewer = ui.viewer.as_ref().expect("the viewer opened");
    assert_eq!(viewer.path, PathBuf::from("/work/a.png"));
    assert!(viewer.annotatable);
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    for id in [view::IMAGE_CLOSE, view::IMAGE_ANNOTATE] {
        assert!(frame.regions.iter().any(|r| r.id == id), "no {id}");
    }
}

#[test]
fn a_message_image_opens_a_viewer_that_cannot_be_annotated() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::Queued {
        session: 1,
        request: "r2".into(),
        text: "what is this?".into(),
        at: None,
        images: vec!["/work/shot.png".into()],
    });
    click(&store, &mut ui, "image:msg:r2:0");
    let viewer = ui.viewer.as_ref().expect("the viewer opened");
    assert_eq!(viewer.path, PathBuf::from("/work/shot.png"));
    assert!(!viewer.annotatable);
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(frame.regions.iter().any(|r| r.id == view::IMAGE_CLOSE));
    assert!(!frame.regions.iter().any(|r| r.id == view::IMAGE_ANNOTATE));
}

#[test]
fn the_viewer_closes_on_its_close_control_and_hands_an_image_to_the_editor() {
    let (store, mut ui) = store_with_session();
    add_images(&mut ui, vec!["/work/a.png".into()]);
    click(&store, &mut ui, "image:open:0");
    click(&store, &mut ui, view::IMAGE_CLOSE);
    assert!(ui.viewer.is_none());
    click(&store, &mut ui, "image:open:0");
    click(&store, &mut ui, view::IMAGE_ANNOTATE);
    assert!(ui.viewer.is_none());
    assert_eq!(ui.annotating, Some(PathBuf::from("/work/a.png")));
}

#[test]
fn escape_closes_the_viewer_as_its_close_control_does() {
    let (store, mut ui) = store_with_session();
    add_images(&mut ui, vec!["/work/a.png".into()]);
    click(&store, &mut ui, "image:open:0");
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    tree.layout(window);
    let actions = tree.handle_event(key(limn::Key::Escape));
    assert!(actions.contains(&UiAction::Clicked {
        id: view::IMAGE_CLOSE.into()
    }));
}

#[test]
fn removing_an_image_drops_that_one_from_the_composer() {
    let (store, mut ui) = store_with_session();
    add_images(&mut ui, vec!["/work/a.png".into(), "/work/b.png".into()]);
    click(&store, &mut ui, "image:remove:0");
    assert_eq!(ui.images, vec![PathBuf::from("/work/b.png")]);
    // An index past the end changes nothing.
    click(&store, &mut ui, "image:remove:5");
    assert_eq!(ui.images.len(), 1);
}

#[test]
fn sending_carries_the_images_and_clears_the_composer() {
    let (store, mut ui) = store_with_session();
    add_images(&mut ui, vec!["/work/a.png".into()]);
    let sent = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::COMPOSER.into(),
            value: "look at this".into(),
        },
    );
    assert_eq!(
        sent,
        [Intent::Send {
            session: 1,
            text: "look at this".into(),
            mentions: vec![],
            images: vec!["/work/a.png".into()],
            lane: Lane::Steer,
        }]
    );
    assert!(ui.images.is_empty());
}

#[test]
fn ctrl_enter_in_the_composer_queues_and_a_plain_enter_steers() {
    let (store, mut ui) = store_with_session();
    let ctrl_enter = limn::UiEvent::Key {
        key: limn::Key::Enter,
        shift: false,
        ctrl: true,
        alt: false,
    };
    composer_key(&mut ui, Some(view::COMPOSER), &ctrl_enter);
    let queued = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::COMPOSER.into(),
            value: "after this".into(),
        },
    );
    assert_eq!(
        queued,
        [Intent::Send {
            session: 1,
            text: "after this".into(),
            mentions: vec![],
            images: vec![],
            lane: Lane::Queue,
        }]
    );
    // The flag is used once: the next Enter steers unless its own key says otherwise.
    composer_key(&mut ui, Some(view::COMPOSER), &key(limn::Key::Enter));
    let steered = update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::COMPOSER.into(),
            value: "now".into(),
        },
    );
    assert!(matches!(
        steered.as_slice(),
        [Intent::Send {
            lane: Lane::Steer,
            ..
        }]
    ));
}

#[test]
fn queued_messages_are_kept_per_session_and_a_taken_one_goes_back_to_the_composer() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::QueueChanged {
        session: 1,
        entries: vec![QueuedMessage {
            id: "q1".into(),
            text: "and then?".into(),
            images: vec!["/work/a.png".into()],
            mentions: vec![],
            held: false,
        }],
    });
    assert_eq!(store.session(1).unwrap().queue.len(), 1);
    let intents = ui.applied(&Event::QueueTaken {
        session: 1,
        text: "and then?".into(),
        images: vec!["/work/a.png".into()],
    });
    assert!(intents.is_empty());
    assert_eq!(ui.restore_prompt.as_deref(), Some("and then?"));
    assert_eq!(ui.images, [PathBuf::from("/work/a.png")]);
}

fn queued(store: &mut Store, texts: &[&str]) {
    let entries = texts
        .iter()
        .enumerate()
        .map(|(i, text)| QueuedMessage {
            id: format!("q{}", i + 1),
            text: (*text).into(),
            images: vec![],
            mentions: vec![],
            held: false,
        })
        .collect();
    store.apply(Event::QueueChanged {
        session: 1,
        entries,
    });
}

#[test]
fn the_queue_bar_buttons_and_row_menus_send_their_intents() {
    let (mut store, mut ui) = store_with_session();
    queued(&mut store, &["first", "second"]);
    assert_eq!(
        click(&store, &mut ui, "queue:send:q1"),
        [Intent::QueueSendNow {
            session: 1,
            id: "q1".into()
        }]
    );
    assert_eq!(
        click(&store, &mut ui, "queue:take:q2"),
        [Intent::QueueTake {
            session: 1,
            id: "q2".into()
        }]
    );
    assert_eq!(
        click(&store, &mut ui, "queue:remove:q1"),
        [Intent::QueueRemove {
            session: 1,
            id: "q1".into()
        }]
    );
    assert_eq!(
        click(&store, &mut ui, "queue:clear"),
        [Intent::QueueClear { session: 1 }]
    );
    // A row's menu opens on its "⋮", its item sends and closes the menu, and the menu's
    // dismiss closes it without an intent.
    click(&store, &mut ui, "queue:menu:q2");
    assert_eq!(ui.queue_menu.as_deref(), Some("q2"));
    assert_eq!(
        click(&store, &mut ui, "queue:send:q2"),
        [Intent::QueueSendNow {
            session: 1,
            id: "q2".into()
        }]
    );
    assert_eq!(ui.queue_menu, None);
    click(&store, &mut ui, "queue:menu:q2");
    assert_eq!(click(&store, &mut ui, "queue:menu:q2:dismiss"), []);
    assert_eq!(ui.queue_menu, None);
}

#[test]
fn the_agents_suggested_prompt_waits_in_the_empty_composer_until_taken_or_sent() {
    let (mut store, mut ui) = store_with_session();
    let set = |store: &mut Store, status| store.apply(Event::Status { session: 1, status });
    let suggest = |store: &mut Store, text: &str| {
        agent(store, AgentEvent::PromptSuggestion { text: text.into() })
    };
    // The box has the focus, as it does right after sending: its placeholder stays.
    let window = limn::Size::new(1408.0, 844.0);
    let focused = |store: &Store, ui: &Ui, words: &str| {
        let mut tree = UiTree::new(view(store, ui, window));
        tree.layout(window);
        assert!(tree.focus(view::COMPOSER));
        let frame = tree.layout(window);
        frame.commands.iter().any(
            |command| matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words)),
        )
    };
    set(&mut store, SessionStatus::Idle);
    assert!(focused(&store, &ui, "Type / for commands"));
    suggest(&mut store, "add a docstring");
    // In the placeholder's place while the box is empty, focused or not.
    assert!(focused(&store, &ui, "add a docstring"));
    assert!(drawn(&store, &ui, "add a docstring"));
    assert!(!drawn(&store, &ui, "Type / for commands"));
    ui.prompt = "run it".into();
    assert!(!drawn(&store, &ui, "add a docstring"));
    // Tab or → in the empty composer puts it there, to send or change; not over a draft, nor
    // from another control.
    let (tab, right) = (key(limn::Key::Tab), key(limn::Key::ArrowRight));
    assert!(!suggestion_key(&store, &mut ui, Some(view::COMPOSER), &tab));
    ui.prompt.clear();
    assert!(!suggestion_key(&store, &mut ui, Some("terminal"), &tab));
    assert!(suggestion_key(
        &store,
        &mut ui,
        Some(view::COMPOSER),
        &right
    ));
    assert_eq!(ui.restore_prompt.take().as_deref(), Some("add a docstring"));
    // A message sent takes it away, as do a turn that starts and an agent that stops.
    store.apply(Event::Queued {
        session: 1,
        request: "r2".into(),
        text: "add a docstring".into(),
        at: None,
        images: vec![],
    });
    assert!(!suggestion_key(&store, &mut ui, Some(view::COMPOSER), &tab));
    for status in [SessionStatus::Running, SessionStatus::Exited] {
        set(&mut store, SessionStatus::Idle);
        suggest(&mut store, "add a docstring");
        set(&mut store, status);
        assert_eq!(store.session(1).unwrap().suggestion, None);
    }
    // One that comes once the next turn began is the last turn's: it is let go.
    set(&mut store, SessionStatus::Running);
    suggest(&mut store, "stale");
    assert_eq!(store.session(1).unwrap().suggestion, None);
}

#[test]
fn an_empty_composer_takes_the_last_queued_message_back_with_arrow_up() {
    let (mut store, mut ui) = store_with_session();
    queued(&mut store, &["first", "second"]);
    let up = key(limn::Key::ArrowUp);
    assert_eq!(
        queue_key(&store, &ui, Some(view::COMPOSER), &up),
        Some(vec![Intent::QueueTake {
            session: 1,
            id: "q2".into()
        }])
    );
    // Text in the composer, another control focused, or no queue: the arrow keeps its job.
    ui.prompt = "draft".into();
    assert_eq!(queue_key(&store, &ui, Some(view::COMPOSER), &up), None);
    ui.prompt.clear();
    assert_eq!(queue_key(&store, &ui, Some("terminal"), &up), None);
    queued(&mut store, &[]);
    assert_eq!(queue_key(&store, &ui, Some(view::COMPOSER), &up), None);
}

#[test]
fn a_queue_of_two_or_more_folds_until_it_is_expanded() {
    let (mut store, mut ui) = store_with_session();
    queued(&mut store, &["first  line\n  and more", "second"]);
    let window = limn::Size::new(1408.0, 844.0);
    let drawn = |ui: &Ui, words: &str| {
        let frame = UiTree::new(view(&store, ui, window)).layout(window);
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    // Folded: the count and the next message's preview; the rows are not drawn.
    assert!(drawn(&ui, "2 messages queued"));
    assert!(drawn(&ui, "Next: first line and more"));
    assert!(!drawn(&ui, "Clear all"));
    click(&store, &mut ui, view::QUEUE_TOGGLE);
    assert!(drawn(&ui, "Clear all"));
    assert!(drawn(&ui, "second"));
    click(&store, &mut ui, view::QUEUE_TOGGLE);
    assert!(!drawn(&ui, "Clear all"));
}

#[test]
fn a_queued_message_previews_its_words_then_each_image() {
    let mut entry = QueuedMessage {
        id: "q1".into(),
        text: " fix\n\tthe   bug ".into(),
        images: vec!["/work/a.png".into(), "/work/b.png".into()],
        mentions: vec![],
        held: false,
    };
    assert_eq!(view::queue_preview(&entry), "fix the bug [Image] [Image]");
    entry.text.clear();
    entry.images.clear();
    assert_eq!(view::queue_preview(&entry), "…");
}

#[test]
fn a_message_written_before_its_session_keeps_its_images() {
    let store = Store::default();
    let mut ui = Ui::new("/work".into());
    add_images(&mut ui, vec!["/work/a.png".into()]);
    update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::COMPOSER.into(),
            value: "hi".into(),
        },
    );
    assert!(ui.images.is_empty());
    let then = ui.applied(&Event::SessionCreated {
        session: 7,
        agent: "claude".into(),
        cwd: "/work".into(),
    });
    assert!(matches!(
        then.first(),
        Some(Intent::Send { session: 7, images, .. }) if *images == [PathBuf::from("/work/a.png")]
    ));
}

#[test]
fn a_queued_message_shows_its_images_above_its_words() {
    let (mut store, ui) = store_with_session();
    store.apply(Event::Queued {
        session: 1,
        request: "r2".into(),
        text: "what is this?".into(),
        at: None,
        images: vec!["/work/shot.png".into()],
    });
    assert!(matches!(
        store.session(1).unwrap().items.last(),
        Some(Item::User { images, .. }) if *images == [PathBuf::from("/work/shot.png")]
    ));
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let find = |id: &str| {
        frame
            .regions
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("no {id}"))
            .rect
    };
    let tile = find("image:msg:r2:0");
    assert!(
        tile.y + tile.height <= find(view::COMPOSER).y,
        "the image is under the composer"
    );
    assert!(tile.x > 1408.0 / 2.0, "the image is not right-aligned");
}

#[test]
fn more_than_twenty_images_keep_the_first_twenty_and_say_so() {
    let (_, mut ui) = store_with_session();
    let paths: Vec<PathBuf> = (0..22).map(|i| format!("/work/{i}.png").into()).collect();
    add_images(&mut ui, paths.clone());
    assert_eq!(ui.images, paths[..MAX_IMAGES]);
    assert_eq!(
        ui.image_notice.as_deref(),
        Some("Maximum of 20 images allowed.")
    );
    // The notice lasts until the composer's images change again.
    add_images(&mut ui, vec![]);
    assert!(ui.image_notice.is_none());
}

fn open_editor(store: &Store, ui: &mut Ui) {
    add_images(ui, vec!["/work/a.png".into()]);
    click(store, ui, "image:open:0");
    click(store, ui, view::IMAGE_ANNOTATE);
    assert_eq!(ui.annotating, Some(PathBuf::from("/work/a.png")));
}

fn a_mark(x: f32) -> limn::SketchMark {
    limn::SketchMark::Line {
        from: limn::Point::new(x, 0.1),
        to: limn::Point::new(x, 0.9),
        color: limn::Color::rgb(0xE0, 0x31, 0x31),
        width: 4.0,
    }
}

fn sketch(store: &Store, ui: &mut Ui, mark: limn::SketchMark) {
    update(
        store,
        ui,
        &UiAction::Sketched {
            id: annotate::SKETCH.into(),
            mark,
        },
    );
}

#[test]
fn a_sketched_mark_is_kept_and_lets_the_editor_save() {
    let (store, mut ui) = store_with_session();
    open_editor(&store, &mut ui);
    assert!(annotate::start_save(&mut ui).is_none());
    sketch(&store, &mut ui, a_mark(0.2));
    assert_eq!(ui.marks, vec![a_mark(0.2)]);
    let (path, marks, _) = annotate::start_save(&mut ui).expect("the marks can be saved");
    assert_eq!(path, PathBuf::from("/work/a.png"));
    assert_eq!(marks, vec![a_mark(0.2)]);
    assert!(
        annotate::start_save(&mut ui).is_none(),
        "one save at a time"
    );
}

#[test]
fn undo_and_redo_move_the_marks_between_the_two_stacks() {
    let (store, mut ui) = store_with_session();
    open_editor(&store, &mut ui);
    sketch(&store, &mut ui, a_mark(0.1));
    sketch(&store, &mut ui, a_mark(0.2));
    click(&store, &mut ui, annotate::UNDO);
    assert_eq!(ui.marks, vec![a_mark(0.1)]);
    assert_eq!(ui.redo, vec![a_mark(0.2)]);
    click(&store, &mut ui, annotate::REDO);
    assert_eq!(ui.marks, vec![a_mark(0.1), a_mark(0.2)]);
    assert!(ui.redo.is_empty());
    click(&store, &mut ui, annotate::UNDO);
    sketch(&store, &mut ui, a_mark(0.3));
    assert!(ui.redo.is_empty(), "a new mark clears the redo");
}

#[test]
fn clear_all_moves_the_marks_to_redo_in_their_order() {
    let (store, mut ui) = store_with_session();
    open_editor(&store, &mut ui);
    sketch(&store, &mut ui, a_mark(0.1));
    sketch(&store, &mut ui, a_mark(0.2));
    click(&store, &mut ui, annotate::CLEAR);
    assert!(ui.marks.is_empty());
    click(&store, &mut ui, annotate::REDO);
    click(&store, &mut ui, annotate::REDO);
    assert_eq!(ui.marks, vec![a_mark(0.1), a_mark(0.2)]);
}

#[test]
fn close_with_marks_asks_and_discard_or_keep_editing_answers() {
    let (store, mut ui) = store_with_session();
    open_editor(&store, &mut ui);
    sketch(&store, &mut ui, a_mark(0.1));
    click(&store, &mut ui, annotate::CLOSE);
    assert!(ui.discarding);
    assert!(ui.annotating.is_some());
    click(&store, &mut ui, annotate::KEEP);
    assert!(!ui.discarding);
    assert_eq!(ui.marks.len(), 1);
    click(&store, &mut ui, annotate::BACKDROP);
    click(&store, &mut ui, annotate::DISCARD);
    assert!(ui.annotating.is_none());
    assert!(ui.marks.is_empty() && ui.redo.is_empty());
}

#[test]
fn close_without_marks_just_closes() {
    let (store, mut ui) = store_with_session();
    open_editor(&store, &mut ui);
    click(&store, &mut ui, annotate::CLOSE);
    assert!(ui.annotating.is_none());
    assert!(!ui.discarding);
}

#[test]
fn escape_asks_to_close_the_editor() {
    let (store, mut ui) = store_with_session();
    open_editor(&store, &mut ui);
    sketch(&store, &mut ui, a_mark(0.1));
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    let frame = tree.layout(window);
    assert!(frame.regions.iter().any(|r| r.id == annotate::SKETCH));
    let actions = tree.handle_event(key(limn::Key::Escape));
    assert!(actions.contains(&UiAction::Clicked {
        id: annotate::CLOSE.into()
    }));
}

#[test]
fn choosing_arrow_from_shapes_arms_it_and_the_button_shows_it() {
    let (store, mut ui) = store_with_session();
    open_editor(&store, &mut ui);
    assert_eq!(ui.last_shape, limn::SketchTool::Rectangle);
    click(&store, &mut ui, annotate::SHAPES_MENU);
    assert_eq!(ui.open_menu, Some(annotate::SHAPES_MENU));
    click(&store, &mut ui, "annotate:shape:1");
    assert_eq!(ui.tool, limn::SketchTool::Arrow);
    assert_eq!(ui.last_shape, limn::SketchTool::Arrow);
    assert_eq!(ui.open_menu, None);
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(frame.regions.iter().any(|r| r.id == annotate::SHAPE));
}

#[test]
fn ink_and_width_survive_closing_and_reopening_the_editor() {
    let (store, mut ui) = store_with_session();
    open_editor(&store, &mut ui);
    assert_eq!(ui.ink, limn::Color::rgb(0xE0, 0x31, 0x31));
    assert_eq!(ui.stroke, 4.0);
    click(&store, &mut ui, "annotate:ink:2");
    click(&store, &mut ui, "annotate:width:0");
    assert_eq!(ui.ink, limn::Color::rgb(0x2F, 0x9E, 0x44));
    assert_eq!(ui.stroke, 2.0);
    click(&store, &mut ui, annotate::CLOSE);
    open_editor(&store, &mut ui);
    assert_eq!(ui.ink, limn::Color::rgb(0x2F, 0x9E, 0x44));
    assert_eq!(ui.stroke, 2.0);
}

#[test]
fn ctrl_z_and_ctrl_shift_z_undo_and_redo_while_the_editor_is_open() {
    let (store, mut ui) = store_with_session();
    assert_eq!(annotate::key(&ui, limn::Key::Z, true, false), None);
    open_editor(&store, &mut ui);
    assert_eq!(
        annotate::key(&ui, limn::Key::Z, true, false).as_deref(),
        Some(annotate::UNDO)
    );
    assert_eq!(
        annotate::key(&ui, limn::Key::Z, true, true).as_deref(),
        Some(annotate::REDO)
    );
    assert_eq!(annotate::key(&ui, limn::Key::Z, false, false), None);
}

#[test]
fn a_saved_copy_takes_the_pictures_place_and_a_failed_one_keeps_the_editor() {
    let (store, mut ui) = store_with_session();
    open_editor(&store, &mut ui);
    sketch(&store, &mut ui, a_mark(0.1));
    annotate::start_save(&mut ui);
    annotate::finish_save(&mut ui, "/work/a.png".into(), None);
    assert!(ui.annotating.is_some());
    assert!(!ui.saving);
    assert_eq!(
        ui.image_notice.as_deref(),
        Some("Couldn't save your annotations. Try again.")
    );
    annotate::start_save(&mut ui);
    annotate::finish_save(
        &mut ui,
        "/work/a.png".into(),
        Some("/att/a-annotated.png".into()),
    );
    assert_eq!(ui.images, vec![PathBuf::from("/att/a-annotated.png")]);
    assert!(ui.annotating.is_none() && ui.marks.is_empty());
    assert_eq!(ui.image_notice, None);
}

#[test]
fn the_editor_fits_the_picture_to_the_window() {
    let window = limn::Size::new(1408.0, 844.0);
    let wide = annotate::shown(1.5, window);
    assert_eq!((wide.width, wide.height.round()), (640.0, 427.0));
    let tall = annotate::shown(0.5, window);
    assert_eq!((tall.width, tall.height), (342.0, 684.0));
    let narrow = annotate::shown(1.0, limn::Size::new(500.0, 844.0));
    assert_eq!(narrow.width, 468.0);
}

#[test]
fn a_composer_menu_closes_on_its_dismiss() {
    let (store, mut ui) = store_with_session();
    for menu in [view::MODE_MENU, view::MODEL_MENU, view::EFFORT_MENU] {
        click(&store, &mut ui, menu);
        assert_eq!(ui.open_menu, Some(menu));
        // Escape or a press outside: limn reports the menu's dismiss.
        click(&store, &mut ui, &format!("{menu}:dismiss"));
        assert_eq!(ui.open_menu, None, "{menu}");
    }
}

fn source(site: &str, title: &str) -> newen_model::WebSource {
    newen_model::WebSource {
        title: title.into(),
        url: format!("https://{site}/{title}"),
    }
}

#[test]
fn a_web_search_shows_its_pill_then_its_cards_and_the_more_card_expands_them() {
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::ToolUse {
            call: "w1".into(),
            name: "WebSearch".into(),
            input: serde_json::json!({ "query": "rust release" }),
            parent: None,
        },
    );
    let sites = [
        "blog.rust-lang.org",
        "releases.rs",
        "en.wikipedia.org",
        "a.org",
        "b.org",
        "c.org",
    ];
    let sources = sites.iter().map(|site| source(site, "Page")).collect();
    agent(
        &mut store,
        AgentEvent::ToolResult {
            call: "w1".into(),
            output: String::new(),
            is_error: false,
            files: vec![],
            command: None,
            web: Some(newen_model::WebSearchRun {
                query: "rust release".into(),
                sources,
            }),
            images: vec![],
            inline: vec![],
        },
    );
    let window = limn::Size::new(1408.0, 844.0);
    let layout = |ui: &Ui| UiTree::new(view(&store, ui, window)).layout(window);
    let drawn = |frame: &limn::Frame, words: &str| {
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    let cards = |frame: &limn::Frame| {
        frame
            .regions
            .iter()
            .filter(|r| r.id.starts_with("link:w1:"))
            .count()
    };
    let frame = layout(&ui);
    assert!(drawn(&frame, "Searched 6 sites"));
    assert_eq!(cards(&frame), 3);
    assert!(drawn(&frame, "3 more sources"));
    // The more card shows every source and a "Show less" that folds them again.
    click(&store, &mut ui, "web:w1:more");
    let frame = layout(&ui);
    assert_eq!(cards(&frame), 6);
    assert!(drawn(&frame, "Show less"));
    // A card is a link: its click opens its page in the session's browser.
    let id = format!("link:w1:0:https://{}/Page", sites[0]);
    assert_eq!(
        view::link_url(&id),
        Some(format!("https://{}/Page", sites[0]).as_str())
    );
    assert!(frame.regions.iter().any(|r| r.id == id));
    let opened = click(&store, &mut ui, &id);
    assert!(matches!(
        &opened[..],
        [Intent::Browser { op: newen_model::BrowserOp::Open { url: Some(url) }, .. }]
            if url == &format!("https://{}/Page", sites[0])
    ));
    // The pill folds the panel, and a second click opens it again.
    click(&store, &mut ui, "web:w1");
    assert_eq!(cards(&layout(&ui)), 0);
    click(&store, &mut ui, "web:w1");
    assert_eq!(cards(&layout(&ui)), 6);
}

#[test]
fn a_running_web_search_says_it_is_searching_and_an_errored_one_stays_a_step() {
    let (mut store, ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::ToolUse {
            call: "w1".into(),
            name: "WebSearch".into(),
            input: serde_json::json!({ "query": "rust release" }),
            parent: None,
        },
    );
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let drawn = |frame: &limn::Frame, words: &str| {
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    assert!(drawn(&frame, "Searching \u{201c}rust release\u{201d}"));
    assert!(!drawn(&frame, "Searched"));
    agent(
        &mut store,
        AgentEvent::ToolResult {
            call: "w1".into(),
            output: "boom".into(),
            is_error: true,
            files: vec![],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    );
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    // No pill: the failed search is a step in its group, as any failed tool is.
    assert!(!frame.regions.iter().any(|r| r.id.starts_with("web:w1")));
}

#[test]
fn a_source_names_its_site_and_its_letter_hue_is_stable() {
    assert_eq!(
        view::site_name("https://developer.mozilla.org/en-US/"),
        "mozilla"
    );
    assert_eq!(
        view::site_name("https://blog.rust-lang.org/releases"),
        "rust-lang"
    );
    assert_eq!(view::site_name("https://www.x.com/a"), "x");
    // FNV-1a of the name, in 32 bits, modulo 360.
    assert_eq!(view::site_hue("mozilla"), 199);
    assert_eq!(view::site_hue("rust-lang"), 178);
    assert_eq!(view::site_hue("x"), 135);
}

#[test]
fn only_a_turns_last_reply_has_its_actions_once_the_turn_ended() {
    let (mut store, ui) = store_with_session();
    let text = |store: &mut Store, words: &str, id: &str| {
        agent(
            store,
            AgentEvent::Text {
                text: words.into(),
                parent: None,
                id: Some(id.into()),
                at: Some(0),
            },
        )
    };
    text(&mut store, "Looking first", "m1");
    agent(
        &mut store,
        AgentEvent::ToolUse {
            call: "t1".into(),
            name: "Bash".into(),
            input: serde_json::json!({ "command": "ls" }),
            parent: None,
        },
    );
    text(&mut store, "Done", "m2");
    let window = limn::Size::new(1408.0, 844.0);
    let actions = |store: &Store| {
        let frame = UiTree::new(view(store, &ui, window)).layout(window);
        frame
            .regions
            .iter()
            // The person's own message, item 0, has its copy too.
            .filter(|r| r.id.starts_with("copy:1:") && r.id != "copy:1:0")
            .map(|r| r.id.clone())
            .collect::<Vec<_>>()
    };
    // Still working: no reply has its row yet.
    assert!(actions(&store).is_empty());
    agent(
        &mut store,
        AgentEvent::TurnFinished {
            ok: true,
            interrupted: false,
            usage: Usage::default(),
            error: None,
        },
    );
    // The reply between the steps has none; the last one, after the step, has its row.
    assert_eq!(actions(&store), ["copy:1:3"]);
}

#[test]
fn the_logos_of_connectors_and_plugins_are_asked_once_and_only_while_shown() {
    use newen_model::{AgentInfo, ConfiguredServer, LogoWanted, McpSource, McpStatus};
    let (store, mut ui) = store_with_session();
    ui.applied(&Event::Settings {
        settings: Default::default(),
        agents: vec![AgentInfo {
            id: "claude".into(),
            name: "Claude Code".into(),
            tested: vec![],
            options: vec![],
            icon: None,
            sign_in_methods: vec![],
            sign_in_code: true,
            modes: claude_modes(),
            default_mode: "default".into(),
            customize: Default::default(),
        }],
    });
    ui.applied(&Event::Servers {
        agent: "claude".into(),
        folder: "/work".into(),
        servers: vec![ConfiguredServer {
            name: "figma".into(),
            label: String::new(),
            target: "https://mcp.figma.com/mcp".into(),
            status: McpStatus::Connected,
            checked: true,
            source: McpSource::User,
        }],
        scopes: vec![],
        error: None,
    });
    // Nothing is shown yet, so nothing is asked.
    assert!(ui.logos_due(&store).is_empty());
    let page = format!("czp:tab:{}", view::SETTINGS_CONNECTORS);
    click(&store, &mut ui, view::CUSTOMIZE_OPEN);
    click(&store, &mut ui, &page);
    let wanted = LogoWanted {
        key: "mcp:figma".into(),
        name: "figma".into(),
        url: Some("https://mcp.figma.com/mcp".into()),
    };
    assert_eq!(
        ui.logos_due(&store),
        vec![Intent::Logos {
            wanted: vec![wanted]
        }]
    );
    assert!(ui.logos_due(&store).is_empty());
    // Logos off, none is asked for.
    ui.settings.logos = false;
    click(&store, &mut ui, &page);
    assert!(ui.logos_due(&store).is_empty());
}

fn ready_update(ui: &mut Ui, mandatory: bool) {
    ui.applied(&Event::Update(newen_model::UpdateState::Ready {
        version: "0.3.0".into(),
        mandatory,
    }));
}

fn set_status(store: &mut Store, id: SessionId, status: newen_model::SessionStatus) {
    store
        .sessions
        .iter_mut()
        .find(|s| s.id == id)
        .expect("the session")
        .status = status;
}

fn drawn_words(store: &Store, ui: &Ui, words: &str) -> bool {
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(store, ui, window)).layout(window);
    frame.commands.iter().any(
        |command| matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words)),
    )
}

#[test]
fn the_sidebar_strip_and_the_account_menu_restart_into_an_optional_ready_update() {
    let (store, mut ui) = store_with_session();
    assert!(!drawn_words(&store, &ui, "Relaunch to update"));
    ready_update(&mut ui, true);
    assert!(!drawn_words(&store, &ui, "Relaunch to update"));
    ready_update(&mut ui, false);
    assert!(drawn_words(&store, &ui, "Relaunch to update"));
    assert!(drawn_words(&store, &ui, "v0.3.0"));
    assert!(!drawn_words(&store, &ui, "is ready"));
    // Idle agents: the click installs at once.
    assert_eq!(
        click(&store, &mut ui, updates::UPDATE_STRIP),
        vec![Intent::InstallUpdate]
    );
    // The account menu has it too, and closes.
    click(&store, &mut ui, view::ACCOUNT_MENU);
    assert!(drawn_words(&store, &ui, "Restart to update"));
    assert_eq!(
        click(&store, &mut ui, updates::UPDATE_MENU),
        vec![Intent::InstallUpdate]
    );
    assert!(!ui.account_menu);
    // Nothing ready: no entry.
    ui.applied(&Event::Update(newen_model::UpdateState::Idle));
    click(&store, &mut ui, view::ACCOUNT_MENU);
    assert!(!drawn_words(&store, &ui, "Restart to update"));
}

/// With auto-update off, an optional version found offers its Download in the strip and
/// shows its progress there; Settings offers Download and Skip. A mandatory one, or one found
/// while auto-update is on, shows nothing in the strip.
#[test]
fn with_auto_update_off_the_strip_offers_the_download_and_settings_the_skip() {
    let (store, mut ui) = store_with_session();
    let available = |mandatory| {
        Event::Update(newen_model::UpdateState::Available {
            version: "0.3.0".into(),
            notes: String::new(),
            mandatory,
            size: 10,
        })
    };
    ui.applied(&available(false));
    assert!(!drawn_words(&store, &ui, "Update available"));
    ui.settings.auto_update = false;
    assert!(drawn_words(&store, &ui, "Update available"));
    assert!(drawn_words(&store, &ui, "v0.3.0 \u{b7} Download"));
    assert_eq!(
        click(&store, &mut ui, updates::UPDATE_STRIP_DOWNLOAD),
        vec![Intent::DownloadUpdate]
    );
    ui.applied(&Event::Update(newen_model::UpdateState::Downloading {
        version: "0.3.0".into(),
        done: 42,
        total: 100,
        mandatory: false,
    }));
    assert!(drawn_words(&store, &ui, "Downloading update"));
    assert!(drawn_words(&store, &ui, "42%"));
    ui.applied(&available(false));
    ui.settings_page = Some("system".into());
    assert!(drawn_words(&store, &ui, "Skip this version"));
    assert_eq!(
        click(&store, &mut ui, updates::UPDATE_SKIP),
        vec![Intent::SkipUpdate]
    );
    ui.settings_page = None;
    ui.applied(&available(true));
    assert!(!drawn_words(&store, &ui, "Update available"));
}

/// "Check for updates" always answers: when the last check ended, beside what is on its way,
/// or why it failed.
#[test]
fn the_updates_line_says_when_it_last_checked_or_why_that_failed() {
    let (_, mut ui) = store_with_session();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    ui.applied(&Event::Update(newen_model::UpdateState::UpToDate {
        checked_at: now,
    }));
    ui.applied(&Event::UpdateChecked {
        at: now,
        error: None,
    });
    assert_eq!(
        updates::description(&ui),
        "Newen is up to date \u{b7} Checked just now"
    );
    ui.applied(&Event::Update(newen_model::UpdateState::Downloading {
        version: "0.1.9".into(),
        done: 42,
        total: 100,
        mandatory: false,
    }));
    assert_eq!(
        updates::description(&ui),
        "Downloading 0.1.9 \u{b7} 42% \u{b7} Checked just now"
    );
    ui.applied(&Event::UpdateChecked {
        at: now,
        error: Some("Could not reach the update server.".into()),
    });
    assert_eq!(
        updates::description(&ui),
        "Downloading 0.1.9 \u{b7} 42% \u{b7} Last check failed: Could not reach the update server."
    );
    ui.applied(&Event::Update(newen_model::UpdateState::Failed {
        reason: "Could not reach the update server.".into(),
    }));
    assert_eq!(
        updates::description(&ui),
        "Could not reach the update server."
    );
}

#[test]
fn restarting_with_agents_at_work_asks_and_waiting_installs_once_they_are_idle() {
    let (mut store, mut ui) = store_with_session();
    let id = ui.selected.expect("a session");
    set_status(&mut store, id, newen_model::SessionStatus::Running);
    ready_update(&mut ui, false);
    assert!(click(&store, &mut ui, updates::UPDATE_STRIP).is_empty());
    assert!(ui.update_asking);
    assert!(drawn_words(&store, &ui, "Agents are still working"));
    // Wait: nothing is sent while the agent is busy.
    assert!(click(&store, &mut ui, updates::UPDATE_ASK_WAIT).is_empty());
    assert!(!ui.update_asking && ui.restart_when_idle);
    assert!(drawn_words(&store, &ui, "Restarting when agents finish"));
    assert!(updates::settled(&store, &mut ui).is_empty());
    set_status(&mut store, id, newen_model::SessionStatus::Idle);
    assert_eq!(
        updates::settled(&store, &mut ui),
        vec![Intent::InstallUpdate]
    );
    // Sent once.
    assert!(updates::settled(&store, &mut ui).is_empty());
}

#[test]
fn update_anyway_installs_and_cancel_forgets_the_wait() {
    let (mut store, mut ui) = store_with_session();
    let id = ui.selected.expect("a session");
    set_status(&mut store, id, newen_model::SessionStatus::Awaiting);
    ready_update(&mut ui, false);
    click(&store, &mut ui, updates::UPDATE_STRIP);
    assert_eq!(
        click(&store, &mut ui, updates::UPDATE_ASK_NOW),
        vec![Intent::InstallUpdate]
    );
    click(&store, &mut ui, updates::UPDATE_STRIP);
    click(&store, &mut ui, updates::UPDATE_ASK_WAIT);
    click(&store, &mut ui, updates::UPDATE_WAIT_CANCEL);
    assert!(!ui.restart_when_idle);
}

#[test]
fn a_hidden_sidebar_says_a_restart_is_needed() {
    let (store, mut ui) = store_with_session();
    ui.sidebar.open = false;
    assert!(!drawn_words(&store, &ui, "Restart needed"));
    ready_update(&mut ui, false);
    // The words ride on the toggle's tooltip, which is built with the toggle.
    assert!(view::sidebar_toggle_label(&ui).starts_with("Restart needed"));
}

#[test]
fn the_welcome_shows_once_the_settings_are_known_until_get_started() {
    let (store, mut ui) = store_with_session();
    ui.skip_welcome = false;
    // Before the settings arrive, nothing says whether it was seen.
    assert!(!crate::welcoming(&ui));
    ui.applied(&Event::Settings {
        settings: Settings::default(),
        agents: vec![newen_model::AgentInfo {
            id: "claude".into(),
            name: "Claude Code".into(),
            tested: vec![],
            options: vec![],
            icon: None,
            sign_in_methods: vec![],
            sign_in_code: true,
            modes: claude_modes(),
            default_mode: "default".into(),
            customize: Default::default(),
        }],
    });
    assert!(crate::welcoming(&ui));
    // The mark's horizon is drawn first, a frame at a time, then the rest comes in; under
    // reduced motion all of it is there at once.
    assert!(!drawn(&store, &ui, "Get started"));
    assert_eq!(
        ui.repaint_after(),
        Some(std::time::Duration::from_millis(16))
    );
    ui.reduced_motion = true;
    assert!(drawn(&store, &ui, "Get started"));
    let saved = click(&store, &mut ui, crate::welcome::START);
    assert!(
        matches!(&saved[..], [Intent::SaveSettings(s)] if s.welcomed),
        "{saved:?}"
    );
    assert!(!crate::welcoming(&ui));
    // A stand-in agent's run skips it.
    let mut test = Ui::new("/work".into());
    test.skip_welcome = true;
    test.agents = ui.agents.clone();
    assert!(!crate::welcoming(&test));
}

#[test]
fn the_welcome_leads_to_the_appearance_step_then_the_tour_with_each_choice_saved() {
    let (store, mut ui) = store_with_session();
    ui.skip_welcome = false;
    ui.applied(&Event::Settings {
        settings: Settings::default(),
        agents: an_agent_known(),
    });
    assert!(crate::welcoming(&ui) && !crate::appearance::looking(&ui));
    let saved = click(&store, &mut ui, crate::welcome::START);
    assert!(matches!(&saved[..], [Intent::SaveSettings(s)] if s.welcomed && !s.looked));
    // The step shows next, ahead of the sign-in screen and the tour.
    assert!(!crate::welcoming(&ui) && crate::appearance::looking(&ui));
    assert!(!crate::account::visible(&ui));
    assert_eq!(ui.tour, None);
    // Echoed back by the engine, the step is still to show.
    ui.applied(&Event::Settings {
        settings: Settings {
            welcomed: true,
            ..Settings::default()
        },
        agents: an_agent_known(),
    });
    assert!(crate::appearance::looking(&ui));
    // Choosing applies the theme at once and is saved; Continue ends the step, once.
    let saved = click(&store, &mut ui, "look:dark");
    assert!(matches!(&saved[..], [Intent::SaveSettings(s)] if s.theme == Theme::Dark));
    assert_eq!(ui.settings.theme, Theme::Dark);
    click(&store, &mut ui, "look:light");
    assert_eq!(ui.settings.theme, Theme::Light);
    assert!(crate::appearance::looking(&ui));
    let saved = click(&store, &mut ui, crate::appearance::CONTINUE);
    assert!(
        matches!(&saved[..], [Intent::SaveSettings(s)] if s.looked && s.theme == Theme::Light),
        "{saved:?}"
    );
    assert!(!crate::appearance::looking(&ui));
    assert_eq!(ui.tour, Some(0));
    let window = limn::Size::new(1408.0, 844.0);
    // The step draws all three choices and its button.
    ui.settings.looked = false;
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    for id in ["look:system", "look:light", "look:dark", "look:continue"] {
        assert!(frame.regions.iter().any(|r| r.id == id), "{id}");
    }
}

#[test]
fn a_person_welcomed_before_the_step_existed_is_not_asked_and_a_signed_out_one_signs_in_after_it() {
    let (_, mut old) = store_with_session();
    old.skip_welcome = false;
    old.applied(&Event::Settings {
        settings: Settings {
            welcomed: true,
            ..Settings::default()
        },
        agents: an_agent_known(),
    });
    assert!(!crate::appearance::looking(&old));
    let (store, mut fresh) = store_with_session();
    fresh.skip_welcome = false;
    fresh.applied(&Event::Settings {
        settings: Settings::default(),
        agents: an_agent_known(),
    });
    fresh
        .account
        .states
        .insert("claude".into(), AuthState::SignedOut);
    click(&store, &mut fresh, crate::welcome::START);
    assert!(!crate::account::visible(&fresh));
    click(&store, &mut fresh, crate::appearance::CONTINUE);
    assert!(crate::account::visible(&fresh));
}

fn an_agent_known() -> Vec<newen_model::AgentInfo> {
    vec![newen_model::AgentInfo {
        id: "claude".into(),
        name: "Claude Code".into(),
        tested: vec![],
        options: vec![],
        icon: None,
        sign_in_methods: vec![],
        sign_in_code: true,
        modes: claude_modes(),
        default_mode: "default".into(),
        customize: Default::default(),
    }]
}

#[test]
fn get_started_starts_the_tour_which_moves_by_its_buttons_and_saves_when_skipped() {
    let (store, mut ui) = store_with_session();
    ui.skip_welcome = false;
    ui.applied(&Event::Settings {
        settings: Settings::default(),
        agents: an_agent_known(),
    });
    click(&store, &mut ui, crate::welcome::START);
    click(&store, &mut ui, crate::appearance::CONTINUE);
    assert_eq!(ui.tour, Some(0));
    assert!(click(&store, &mut ui, "tour:next").is_empty());
    assert_eq!(ui.tour, Some(1));
    assert!(click(&store, &mut ui, "tour:back").is_empty());
    assert_eq!(ui.tour, Some(0));
    let saved = click(&store, &mut ui, "tour:skip");
    assert_eq!(ui.tour, None);
    assert!(
        matches!(&saved[..], [Intent::SaveSettings(s)] if s.toured && s.welcomed),
        "{saved:?}"
    );
}

#[test]
fn the_tour_ends_after_its_last_step_and_on_escape_and_saves_that_it_was_toured() {
    let (store, mut ui) = store_with_session();
    ui.tour = Some(0);
    for _ in 0..5 {
        click(&store, &mut ui, "tour:next");
    }
    assert_eq!(ui.tour, Some(5));
    let saved = click(&store, &mut ui, "tour:next");
    assert!(
        matches!(&saved[..], [Intent::SaveSettings(s)] if s.toured),
        "{saved:?}"
    );
    assert_eq!(ui.tour, None);
    // Escape reports the tour's own ID, which ends it as Skip does.
    ui.tour = Some(2);
    let saved = click(&store, &mut ui, "tour");
    assert!(
        matches!(&saved[..], [Intent::SaveSettings(s)] if s.toured),
        "{saved:?}"
    );
    assert_eq!(ui.tour, None);
}

#[test]
fn the_account_menu_takes_the_tour_again() {
    let (store, mut ui) = store_with_session();
    ui.settings.toured = true;
    ui.account_menu = true;
    assert!(click(&store, &mut ui, crate::tour::START).is_empty());
    assert_eq!(ui.tour, Some(0));
    assert!(!ui.account_menu);
}

#[test]
fn a_welcomed_person_who_never_toured_sees_the_tour_once_at_the_start() {
    let (_, mut ui) = store_with_session();
    ui.skip_welcome = false;
    ui.applied(&Event::Settings {
        settings: Settings {
            welcomed: true,
            ..Settings::default()
        },
        agents: an_agent_known(),
    });
    assert_eq!(ui.tour, Some(0));
    // A toured person's start shows nothing, and a stand-in run's never does.
    let (_, mut toured) = store_with_session();
    toured.skip_welcome = false;
    toured.applied(&Event::Settings {
        settings: Settings {
            welcomed: true,
            toured: true,
            ..Settings::default()
        },
        agents: an_agent_known(),
    });
    assert_eq!(toured.tour, None);
    let (_, mut stand_in) = store_with_session();
    stand_in.applied(&Event::Settings {
        settings: Settings {
            welcomed: true,
            ..Settings::default()
        },
        agents: an_agent_known(),
    });
    assert_eq!(stand_in.tour, None);
}

#[test]
fn a_step_whose_control_is_not_in_view_is_passed_and_only_the_current_one_is() {
    let (_, mut ui) = store_with_session();
    ui.tour = Some(0);
    assert_eq!(crate::tour::skip_missing(&mut ui, "new-session"), None);
    assert_eq!(ui.tour, Some(0));
    assert_eq!(
        crate::tour::skip_missing(&mut ui, view::COMPOSER),
        Some(vec![])
    );
    assert_eq!(ui.tour, Some(1));
    // The last step passed is the end of the tour, which is saved as seen.
    ui.tour = Some(5);
    let saved = crate::tour::skip_missing(&mut ui, view::ACCOUNT_MENU);
    assert!(
        matches!(saved.as_deref(), Some([Intent::SaveSettings(s)]) if s.toured),
        "{saved:?}"
    );
    assert_eq!(ui.tour, None);
}

#[test]
fn every_step_points_at_a_control_the_window_lays_out() {
    let (store, mut ui) = store_with_session();
    let window = limn::Size::new(1408.0, 844.0);
    for step in 0..6 {
        ui.tour = Some(step);
        let frame = UiTree::new(view(&store, &ui, window)).layout(window);
        assert_eq!(frame.missing_coach_target(), None, "step {step}");
    }
}

#[test]
fn a_ready_update_puts_restart_in_the_settings_row() {
    let (store, mut ui) = store_with_session();
    ui.applied(&Event::Update(newen_model::UpdateState::Ready {
        version: "9.9.9".into(),
        mandatory: false,
    }));
    ui.settings_page = Some("system".into());
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    // The row's two buttons share a line, Restart beside Check, both drawn.
    let at = |words: &str| {
        frame.commands.iter().find_map(|command| match command {
            limn::DrawCommand::Text { position, text, .. } if text == words => Some(*position),
            _ => None,
        })
    };
    let restart = at("Restart to update").expect("the row draws Restart to update");
    let check = at("Check for updates").expect("the row draws Check for updates");
    assert!((restart.y - check.y).abs() < 0.5 && restart.x < check.x);
    // And the row's own button answers, as the strip does.
    assert_eq!(
        click(&store, &mut ui, updates::UPDATE_RESTART_ROW),
        vec![Intent::InstallUpdate]
    );
}

fn claude_with_sign_in() -> Vec<newen_model::AgentInfo> {
    let mut agents = an_agent_known();
    agents[0].sign_in_methods = vec![
        ("claudeai".into(), "Claude subscription".into()),
        ("console".into(), "Anthropic Console".into()),
    ];
    agents
}

fn signed_in() -> newen_model::AuthState {
    newen_model::AuthState::SignedIn {
        method: "claude.ai".into(),
        email: Some("ada@example.com".into()),
        org: Some("Analytical Engines".into()),
        plan: Some("max".into()),
    }
}

#[test]
fn the_first_settings_ask_each_agent_who_is_signed_in_and_signed_out_shows_the_sign_in_screen() {
    let (store, mut ui) = store_with_session();
    ui.skip_welcome = true;
    let asked = ui.applied(&Event::Settings {
        settings: Settings::default(),
        agents: claude_with_sign_in(),
    });
    assert!(
        matches!(&asked[..], [Intent::CheckSignIn { agent }, Intent::CheckGithub] if agent == "claude"),
        "{asked:?}"
    );
    ui.agents = claude_with_sign_in();
    assert!(!crate::account::visible(&ui));
    ui.applied(&Event::Auth {
        agent: "claude".into(),
        state: newen_model::AuthState::SignedOut,
    });
    assert!(crate::account::visible(&ui));
    // The method buttons start the sign-in; the page's address brings the code field.
    let started = click(&store, &mut ui, "account:method:console");
    assert!(
        matches!(&started[..], [Intent::SignIn { agent, method }] if agent == "claude" && method == "console"),
        "{started:?}"
    );
    ui.applied(&Event::SignInUrl {
        agent: "claude".into(),
        url: "https://example.com/authorize".into(),
    });
    click(&store, &mut ui, crate::account::COPY_LINK);
    assert_eq!(
        ui.files_copy.as_deref(),
        Some("https://example.com/authorize")
    );
    // Nothing goes with an empty code; the pasted one goes once.
    assert!(click(&store, &mut ui, "account:continue").is_empty());
    let field = format!("{}{}", crate::account::CODE, ui.account.attempt);
    update(
        &store,
        &mut ui,
        &UiAction::InputChanged {
            id: field,
            value: " abc#def \n".into(),
        },
    );
    let sent = click(&store, &mut ui, "account:continue");
    assert!(
        matches!(&sent[..], [Intent::SubmitSignInCode { code, .. }] if code == "abc#def"),
        "{sent:?}"
    );
    assert!(click(&store, &mut ui, "account:continue").is_empty());
    // A failure shows up with Try again; signing in takes the screen away.
    ui.applied(&Event::SignInFailed {
        agent: "claude".into(),
        message: "Login failed".into(),
    });
    assert_eq!(
        ui.account.phase,
        crate::account::Phase::Failed("Login failed".into())
    );
    click(&store, &mut ui, "account:retry");
    ui.applied(&Event::Auth {
        agent: "claude".into(),
        state: signed_in(),
    });
    assert!(!crate::account::visible(&ui));
    assert_eq!(ui.account.phase, crate::account::Phase::Choose);
}

#[test]
fn cancelling_a_sign_in_tells_the_engine_and_the_tour_waits_behind_the_screen() {
    let (store, mut ui) = store_with_session();
    ui.skip_welcome = true;
    ui.agents = claude_with_sign_in();
    ui.account.states.insert("claude".into(), signed_out());
    ui.tour = Some(0);
    assert_eq!(crate::tour::skip_missing(&mut ui, view::COMPOSER), None);
    assert_eq!(ui.tour, Some(0));
    click(&store, &mut ui, "account:method:claudeai");
    let cancelled = click(&store, &mut ui, "account:cancel");
    assert!(matches!(&cancelled[..], [Intent::CancelSignIn { .. }]));
    assert_eq!(ui.account.phase, crate::account::Phase::Choose);
}

fn signed_out() -> newen_model::AuthState {
    newen_model::AuthState::SignedOut
}

/// A GitHub account as `gh` reports it.
fn github_in() -> newen_model::GithubState {
    newen_model::GithubState::SignedIn {
        login: "ada".into(),
        name: Some("Ada Lovelace".into()),
        avatar: None,
    }
}

fn a_week() -> newen_model::ContributionCalendar {
    let day = |date: &str, count, level| {
        Some(newen_model::ContributionDay {
            date: date.into(),
            count,
            level,
        })
    };
    newen_model::ContributionCalendar {
        login: "ada".into(),
        total: 4,
        weeks: vec![[
            day("2026-09-06", 1, 1),
            day("2026-09-07", 3, 2),
            None,
            None,
            None,
            None,
            None,
        ]],
    }
}

#[test]
fn the_foot_shows_the_github_identity_and_its_menu_connects_or_disconnects() {
    let (store, mut ui) = store_with_session();
    ui.skip_welcome = true;
    ui.name = Some("Grace Hopper".into());
    // Not asked or not connected: Newen's user, and the menu offers to connect.
    let (initials, name, avatar) = crate::github::identity(&ui);
    assert_eq!(
        (initials.as_str(), name.as_str(), avatar),
        ("GH", "Grace Hopper", None)
    );
    ui.github.state = Some(newen_model::GithubState::SignedOut);
    assert_eq!(crate::github::menu_action(&ui).1, "Connect GitHub");
    // Connected: GitHub's name, whatever the Newen user is called.
    ui.github.state = Some(github_in());
    let (initials, name, _) = crate::github::identity(&ui);
    assert_eq!((initials.as_str(), name.as_str()), ("AL", "Ada Lovelace"));
    assert_eq!(crate::github::menu_action(&ui).1, "Disconnect GitHub");
    // Without a name the login stands in.
    ui.github.state = Some(newen_model::GithubState::SignedIn {
        login: "ada".into(),
        name: None,
        avatar: Some("/tmp/ada.png".into()),
    });
    let (initials, name, avatar) = crate::github::identity(&ui);
    assert_eq!((initials.as_str(), name.as_str()), ("A", "ada"));
    assert!(avatar.is_some());
    assert!(crate::image_paths(&store, &ui).contains(&"/tmp/ada.png".into()));

    // Disconnecting asks first, and also says it signs the terminal out.
    ui.github.state = Some(github_in());
    ui.account_menu = true;
    assert!(click(&store, &mut ui, crate::github::DISCONNECT).is_empty());
    assert!(ui.github.confirming && !ui.account_menu);
    assert!(click(&store, &mut ui, "github:disconnect:no").is_empty());
    assert!(!ui.github.confirming);
    click(&store, &mut ui, crate::github::DISCONNECT);
    let out = click(&store, &mut ui, "github:disconnect:yes");
    assert!(
        matches!(&out[..], [Intent::DisconnectGithub { login }] if login == "ada"),
        "{out:?}"
    );
}

#[test]
fn github_connects_with_a_code_that_can_be_copied_cancelled_or_fail() {
    let (store, mut ui) = store_with_session();
    ui.github.state = Some(newen_model::GithubState::SignedOut);
    let started = click(&store, &mut ui, crate::github::CONNECT);
    assert_eq!(started, vec![Intent::ConnectGithub]);
    assert_eq!(
        ui.github.phase,
        crate::github::Phase::Waiting { code: None }
    );
    // Copy has no code to copy yet.
    click(&store, &mut ui, crate::github::COPY_CODE);
    assert!(ui.files_copy.is_none());
    ui.applied(&Event::GithubCode {
        code: "1A2B-3C4D".into(),
        url: "https://github.com/login/device".into(),
    });
    click(&store, &mut ui, crate::github::COPY_CODE);
    assert_eq!(ui.files_copy.as_deref(), Some("1A2B-3C4D"));
    assert_eq!(
        click(&store, &mut ui, crate::github::CANCEL),
        vec![Intent::CancelGithub]
    );
    assert_eq!(ui.github.phase, crate::github::Phase::Idle);
    ui.applied(&Event::GithubFailed {
        message: "expired".into(),
    });
    assert_eq!(
        ui.github.phase,
        crate::github::Phase::Failed("expired".into())
    );
    // Signing in clears it.
    ui.applied(&Event::Github(github_in()));
    assert_eq!(ui.github.phase, crate::github::Phase::Idle);
    // Without gh, Connect says what Newen can do before anything is downloaded.
    ui.github.state = Some(newen_model::GithubState::Missing("install gh".into()));
    assert!(click(&store, &mut ui, crate::github::CONNECT).is_empty());
    assert_eq!(ui.github.phase, crate::github::Phase::Offer);
    assert!(drawn(&store, &ui, "Install and connect") && drawn(&store, &ui, "install gh"));
    // Install and connect: the download, how far it got, then the sign-in on its own.
    assert_eq!(
        click(&store, &mut ui, crate::github::INSTALL),
        [Intent::InstallGithub]
    );
    let words = "Downloading GitHub CLI 2.103.3\u{2026}";
    ui.applied(&Event::Github(newen_model::GithubState::Installing(
        words.into(),
    )));
    assert!(drawn(&store, &ui, words));
    assert_eq!(
        ui.applied(&Event::Github(newen_model::GithubState::SignedOut)),
        [Intent::ConnectGithub]
    );
    assert_eq!(
        ui.github.phase,
        crate::github::Phase::Waiting { code: None }
    );
    // A failed download says why, and Try again offers it once more.
    ui.github.phase = crate::github::Phase::Installing;
    ui.applied(&Event::GithubFailed {
        message: "Could not reach GitHub".into(),
    });
    ui.applied(&Event::Github(newen_model::GithubState::Missing(
        "install gh".into(),
    )));
    assert!(drawn(&store, &ui, "Could not reach GitHub"));
    click(&store, &mut ui, crate::github::CONNECT);
    assert_eq!(ui.github.phase, crate::github::Phase::Offer);
}

#[test]
fn the_first_settings_ask_github_once_and_the_accounts_page_asks_for_the_calendar() {
    let (store, mut ui) = store_with_session();
    let settings = Event::Settings {
        settings: Default::default(),
        agents: vec![],
    };
    let first = ui.applied(&settings);
    assert!(first.contains(&Intent::CheckGithub), "{first:?}");
    assert!(!ui.applied(&settings).contains(&Intent::CheckGithub));
    // Signed in, with the page closed: nothing is asked of the network.
    assert!(ui.applied(&Event::Github(github_in())).is_empty());
    let opened = click(&store, &mut ui, view::SETTINGS_OPEN);
    assert!(!opened
        .iter()
        .any(|i| matches!(i, Intent::GithubContributions { .. })));
    let page = click(
        &store,
        &mut ui,
        &format!("settings:page:{}", view::SETTINGS_ACCOUNTS),
    );
    assert_eq!(
        page,
        vec![Intent::GithubContributions {
            login: "ada".into()
        }]
    );
    assert_eq!(ui.github.calendar, crate::github::Calendar::Loading);
    // Asked once; the answer is kept, and Refresh asks again.
    assert!(click(
        &store,
        &mut ui,
        &format!("settings:page:{}", view::SETTINGS_ACCOUNTS)
    )
    .is_empty());
    ui.applied(&Event::GithubContributions(Ok(a_week())));
    assert!(matches!(
        ui.github.calendar,
        crate::github::Calendar::Ready(_)
    ));
    assert_eq!(click(&store, &mut ui, crate::github::REFRESH).len(), 1);
    ui.applied(&Event::GithubContributions(Err("nope".into())));
    assert_eq!(
        ui.github.calendar,
        crate::github::Calendar::Failed("nope".into())
    );
    // Another account's login drops the old calendar.
    ui.applied(&Event::Github(newen_model::GithubState::SignedOut));
    assert_eq!(ui.github.calendar, crate::github::Calendar::Idle);
}

fn titles(page: &newen_components::settings::SettingsPage, group: &str) -> Vec<String> {
    page.groups
        .iter()
        .find(|(name, _)| name == group)
        .map(|(_, rows)| rows.iter().map(|r| r.title.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn the_accounts_page_has_github_then_a_section_per_agent_with_its_usage() {
    let (mut store, mut ui) = store_with_session();
    ui.agents = claude_with_sign_in();
    ui.agent = "claude".into();
    let palette = newen_components::look::palette();
    // Nothing known yet.
    let page = crate::accounts::page(&store, &ui, palette);
    assert_eq!(page.key, view::SETTINGS_ACCOUNTS);
    assert_eq!(titles(&page, "GitHub"), vec!["GitHub"]);
    // Connected, loading its calendar.
    ui.github.state = Some(github_in());
    let page = crate::accounts::page(&store, &ui, palette);
    assert_eq!(
        titles(&page, "GitHub"),
        vec!["Signed in as @ada", "Contributions"]
    );
    ui.github.calendar = crate::github::Calendar::Ready(a_week());
    let page = crate::accounts::page(&store, &ui, palette);
    assert_eq!(
        titles(&page, "GitHub"),
        vec!["Signed in as @ada", "4 contributions in the last year"]
    );
    // gh missing and signed out say what to do.
    ui.github.state = Some(newen_model::GithubState::Missing("install gh".into()));
    assert_eq!(
        titles(&crate::accounts::page(&store, &ui, palette), "GitHub"),
        vec!["GitHub CLI not installed"]
    );
    ui.github.state = Some(newen_model::GithubState::Installing("Unpacking".into()));
    assert_eq!(
        titles(&crate::accounts::page(&store, &ui, palette), "GitHub"),
        vec!["Installing GitHub CLI"]
    );
    ui.github.state = Some(newen_model::GithubState::SignedOut);
    assert_eq!(
        titles(&crate::accounts::page(&store, &ui, palette), "GitHub"),
        vec!["GitHub"]
    );

    // The agent: signed in, and its usage once a session of it reported limits.
    let name = ui.agents[0].name.clone();
    ui.account.states.insert("claude".into(), signed_in());
    let page = crate::accounts::page(&store, &ui, palette);
    assert_eq!(titles(&page, &name), vec!["Signed in", "Usage"]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    store.apply(Event::Agent {
        session: 1,
        request: None,
        event: AgentEvent::Limits(newen_model::PlanLimits {
            session: Some(newen_model::UsageWindow {
                utilization: 0.8,
                resets_at: now + 3 * 3600,
            }),
            weekly: Some(newen_model::UsageWindow {
                utilization: 0.1,
                resets_at: now + 3 * 86_400,
            }),
        }),
    });
    let page = crate::accounts::page(&store, &ui, palette);
    assert_eq!(
        titles(&page, &name),
        vec!["Signed in", "5-hour limit", "Weekly"]
    );
    // Signed out: it can sign in again from here.
    ui.account.states.insert("claude".into(), signed_out());
    assert_eq!(
        titles(&crate::accounts::page(&store, &ui, palette), &name)[0],
        "Not signed in"
    );
}

#[test]
fn the_usage_bar_warns_from_75_and_turns_red_from_90() {
    use newen_components::look;
    let (normal, warn, danger) = (
        crate::accounts::bar_fill(0.5),
        crate::accounts::bar_fill(0.75),
        crate::accounts::bar_fill(0.9),
    );
    assert_eq!(normal, look::handle());
    assert_eq!(warn, look::manual());
    assert_eq!(danger, look::removed());
    assert_eq!(crate::accounts::bar_fill(0.749), normal);
}

#[test]
fn an_agents_sign_out_from_settings_asks_first_and_signs_in_targets_that_agent() {
    let (store, mut ui) = store_with_session();
    ui.skip_welcome = true;
    ui.agents = claude_with_sign_in();
    ui.account.states.insert("claude".into(), signed_in());
    assert!(click(
        &store,
        &mut ui,
        &format!("{}claude", crate::account::SIGN_OUT_OF)
    )
    .is_empty());
    assert_eq!(ui.account.confirming.as_deref(), Some("claude"));
    assert!(click(&store, &mut ui, "account:sign-out:no").is_empty());
    assert!(ui.account.confirming.is_none());
    click(
        &store,
        &mut ui,
        &format!("{}claude", crate::account::SIGN_OUT_OF),
    );
    let out = click(&store, &mut ui, "account:sign-out:yes");
    assert!(
        matches!(&out[..], [Intent::SignOut { agent }] if agent == "claude"),
        "{out:?}"
    );
    // "Sign in" in Settings opens the sign-in screen for that agent.
    assert!(!crate::account::visible(&ui));
    click(
        &store,
        &mut ui,
        &format!("{}claude", crate::account::SIGN_IN_OF),
    );
    assert!(crate::account::visible(&ui));
}

#[test]
fn a_turn_that_needed_a_sign_in_offers_it_and_the_button_opens_the_screen() {
    let (store, mut ui) = store_with_session();
    ui.skip_welcome = true;
    ui.agents = claude_with_sign_in();
    ui.account.states.insert("claude".into(), signed_in());
    let mut store = store;
    for event in [
        AgentEvent::AuthRequired,
        AgentEvent::TurnFinished {
            ok: false,
            interrupted: false,
            usage: Default::default(),
            error: None,
        },
    ] {
        store.apply(Event::Agent {
            session: 1,
            request: Some("r1".into()),
            event,
        });
    }
    let last = store.session(1).unwrap().items.last().cloned();
    assert!(
        matches!(last, Some(Item::TurnEnd { sign_in: true, .. })),
        "{last:?}"
    );
    assert!(!crate::account::visible(&ui));
    click(&store, &mut ui, crate::account::SIGN_IN);
    assert!(crate::account::visible(&ui));
}

/// Claude's mode words as its adapter gives them (the UI itself names no agent's).
fn claude_modes() -> Vec<newen_model::ModeWords> {
    use newen_model::ModeTone::*;
    let w = |id: &str, label: &str, about: &str, tone, listed| newen_model::ModeWords {
        id: id.into(),
        label: label.into(),
        about: about.into(),
        tone,
        listed,
    };
    vec![
        w(
            "default",
            "Manual",
            "Always ask before making changes",
            Ask,
            true,
        ),
        w(
            "acceptEdits",
            "Accept edits",
            "Automatically accept all file edits",
            Edits,
            true,
        ),
        w(
            "plan",
            "Plan",
            "Create a plan before making changes",
            Plan,
            true,
        ),
        w(
            "auto",
            "Auto",
            "The agent handles permission decisions",
            Auto,
            false,
        ),
        w(
            "bypassPermissions",
            "Bypass permissions",
            "Accepts all permissions",
            Unsafe,
            false,
        ),
    ]
}

fn codex_known() -> newen_model::AgentInfo {
    use newen_model::ModeTone::*;
    let mut codex = an_agent_known().remove(0);
    codex.id = "codex".into();
    codex.name = "Codex".into();
    codex.default_mode = "auto".into();
    let w = |id: &str, label: &str, about: &str, tone, listed| newen_model::ModeWords {
        id: id.into(),
        label: label.into(),
        about: about.into(),
        tone,
        listed,
    };
    codex.modes = vec![
        w(
            "read-only",
            "Read only",
            "Asks before anything but reading",
            Read,
            true,
        ),
        w(
            "auto",
            "Auto",
            "Works in this folder; asks to go beyond it",
            Auto,
            true,
        ),
        w(
            "full-access",
            "Full access",
            "No sandbox, never asks",
            Unsafe,
            false,
        ),
    ];
    codex
}

fn two_agents() -> (Store, Ui) {
    let mut ui = Ui::new("/work".into());
    ui.skip_welcome = true;
    let mut agents = an_agent_known();
    agents[0].id = "claude".into();
    agents.push(codex_known());
    ui.applied(&Event::Settings {
        settings: Settings::default(),
        agents,
    });
    (Store::default(), ui)
}

#[test]
fn the_agent_chip_shows_only_with_two_agents_and_the_first_is_the_default() {
    let (store, mut ui) = two_agents();
    assert_eq!((ui.agent.as_str(), ui.mode.as_str()), ("claude", "default"));
    let window = limn::Size::new(1408.0, 844.0);
    let name = ui.agents[0].name.clone();
    let draws = |ui: &Ui| {
        let frame = UiTree::new(view(&store, ui, window)).layout(window);
        frame
            .commands
            .iter()
            .any(|c| matches!(c, limn::DrawCommand::Text { text, .. } if *text == name))
    };
    assert!(draws(&ui));
    ui.agents.truncate(1);
    assert!(!draws(&ui));
}

#[test]
fn choosing_an_agent_takes_its_mode_drops_the_model_and_is_kept_in_settings() {
    let (store, mut ui) = two_agents();
    ui.model = Some("opus".into());
    ui.effort = Some("high".into());
    assert!(click(&store, &mut ui, view::AGENT_MENU).is_empty());
    assert_eq!(ui.open_menu, Some(view::AGENT_MENU));
    let saved = click(&store, &mut ui, "pick-agent:codex");
    assert_eq!(ui.open_menu, None);
    assert_eq!(
        (
            ui.agent.as_str(),
            ui.mode.as_str(),
            ui.model.clone(),
            ui.effort.clone()
        ),
        ("codex", "auto", None, None)
    );
    let [Intent::SaveSettings(settings)] = saved.as_slice() else {
        panic!("the choice is saved: {saved:?}");
    };
    assert_eq!(settings.default_agent.as_deref(), Some("codex"));
    // A later settings (the saved ones) keeps it, and a restart starts from it.
    let remembered = Settings {
        default_agent: Some("codex".into()),
        ..Default::default()
    };
    let agents = ui.agents.clone();
    ui.applied(&Event::Settings {
        settings: remembered.clone(),
        agents: agents.clone(),
    });
    assert_eq!(ui.agent, "codex");
    let mut restarted = Ui::new("/work".into());
    restarted.applied(&Event::Settings {
        settings: remembered,
        agents,
    });
    assert_eq!(
        (restarted.agent.as_str(), restarted.mode.as_str()),
        ("codex", "auto")
    );
}

/// What a new conversation was given stays when settings saved for something else echo back
/// (a panel folded, an option): only the agent's own defaults changing, or its unsafe modes
/// no longer allowed, take them again; opening a new conversation in a folder starts from them.
#[test]
fn a_new_conversations_mode_model_and_effort_survive_a_settings_echo() {
    let (store, mut ui) = two_agents();
    let mut allowed = Settings::default();
    allowed.agents.insert(
        "claude".into(),
        newen_model::AgentSettings {
            allow_unsafe: true,
            ..Default::default()
        },
    );
    let agents = ui.agents.clone();
    let echo = |ui: &mut Ui, settings: &Settings| {
        ui.applied(&Event::Settings {
            settings: settings.clone(),
            agents: agents.clone(),
        })
    };
    echo(&mut ui, &allowed);
    click(&store, &mut ui, "mode:bypassPermissions");
    ui.model = Some("opus".into());
    ui.effort = Some("high".into());
    let chosen = |ui: &Ui| (ui.mode.clone(), ui.model.clone(), ui.effort.clone());
    let picked = chosen(&ui);
    let mut folded = allowed.clone();
    folded.default_agent = Some("claude".into());
    echo(&mut ui, &folded);
    assert_eq!(chosen(&ui), picked);
    // The default model changed in Settings: the new conversation takes it.
    let mut new_default = folded.clone();
    new_default.agents.get_mut("claude").unwrap().model = Some("haiku".into());
    echo(&mut ui, &new_default);
    assert_eq!(chosen(&ui), ("default".into(), Some("haiku".into()), None));
    // Bypass no longer allowed: it is not kept.
    click(&store, &mut ui, "mode:bypassPermissions");
    let mut refused = new_default.clone();
    refused.agents.get_mut("claude").unwrap().allow_unsafe = false;
    echo(&mut ui, &refused);
    assert_eq!(ui.mode, "default");
    click(&store, &mut ui, "mode:acceptEdits");
    click(&store, &mut ui, "new-in:/elsewhere");
    assert_eq!(chosen(&ui), ("default".into(), Some("haiku".into()), None));
}

#[test]
fn the_mode_words_are_the_chosen_agents_and_unsafe_ones_wait_for_settings() {
    let (store, mut ui) = two_agents();
    assert_eq!(
        view::menu_modes(&store, None, &ui),
        ["default", "acceptEdits", "plan"]
    );
    let words = |ui: &Ui, agent: &str, mode: &str| {
        view::mode_entry(ui, agent, mode).map(|w| (w.label.clone(), w.about.clone()))
    };
    assert_eq!(
        words(&ui, "claude", "default"),
        Some(("Manual".into(), "Always ask before making changes".into()))
    );
    click(&store, &mut ui, "pick-agent:codex");
    assert_eq!(view::menu_modes(&store, None, &ui), ["read-only", "auto"]);
    assert_eq!(
        words(&ui, "codex", "read-only"),
        Some((
            "Read only".into(),
            "Asks before anything but reading".into()
        ))
    );
    // Claude's words mean nothing to Codex.
    assert_eq!(words(&ui, "codex", "default"), None);
    ui.settings.agents.insert(
        "codex".into(),
        newen_model::AgentSettings {
            allow_unsafe: true,
            ..Default::default()
        },
    );
    let mut store = store;
    store.apply(Event::SessionCreated {
        session: 1,
        agent: "codex".into(),
        cwd: "/work".into(),
    });
    store.apply(Event::Agent {
        session: 1,
        request: None,
        event: AgentEvent::Ready {
            version: "0.163.0".into(),
            capabilities: newen_model::AgentCapabilities {
                modes: vec!["read-only".into(), "auto".into()],
                unsafe_modes: vec!["full-access".into()],
                ..Default::default()
            },
            mode: Some("auto".into()),
        },
    });
    assert_eq!(
        view::menu_modes(&store, None, &ui),
        ["read-only", "auto", "full-access"]
    );
}

#[test]
fn the_account_is_the_chosen_agents() {
    let (store, mut ui) = two_agents();
    ui.applied(&Event::Auth {
        agent: "claude".into(),
        state: newen_model::AuthState::SignedIn {
            method: "claude.ai".into(),
            email: Some("ada@example.com".into()),
            org: None,
            plan: None,
        },
    });
    ui.applied(&Event::Auth {
        agent: "codex".into(),
        state: newen_model::AuthState::SignedOut,
    });
    assert!(!crate::account::visible(&ui));
    click(&store, &mut ui, "pick-agent:codex");
    assert!(crate::account::visible(&ui));
    // Signing in is the chosen agent's; the other can be used instead from the screen.
    ui.agents[1].sign_in_methods = vec![("chatgpt".into(), "ChatGPT account".into())];
    assert_eq!(
        click(&store, &mut ui, "account:method:chatgpt"),
        [Intent::SignIn {
            agent: "codex".into(),
            method: "chatgpt".into()
        }]
    );
}

#[test]
fn a_permission_card_names_the_agent_that_asks() {
    let (_, ui) = two_agents();
    assert_eq!(view::agent_word(&ui, "claude"), "Claude");
    assert_eq!(view::agent_word(&ui, "codex"), "Codex");
    assert_eq!(view::agent_word(&ui, "other"), "the agent");
}

#[test]
fn a_new_session_offers_the_models_its_agent_told_before_any_session() {
    let store = Store::default();
    let mut ui = Ui::new("/tmp".into());
    ui.agent = "codex".into();
    assert!(view::offered_models(&store, None, &ui).is_empty());
    let model = newen_model::ModelInfo {
        id: "gpt-6-luna".into(),
        name: "GPT-6-Luna".into(),
        efforts: vec!["low".into()],
        modes: vec![],
        default: false,
    };
    ui.applied(&Event::AgentModels {
        agent: "codex".into(),
        models: vec![model],
    });
    assert_eq!(view::offered_models(&store, None, &ui)[0].id, "gpt-6-luna");
}

#[test]
fn a_new_codex_session_never_takes_a_claude_model() {
    let store = Store::default();
    let mut ui = Ui::new("/work".into());
    let info = |id: &str, mode: &str| newen_model::AgentInfo {
        id: id.into(),
        name: id.into(),
        tested: vec![],
        options: vec![],
        icon: None,
        sign_in_methods: vec![],
        sign_in_code: true,
        modes: vec![],
        default_mode: mode.into(),
        customize: Default::default(),
    };
    ui.agents = vec![info("claude", "default"), info("codex", "auto")];
    ui.settings.agents.insert(
        "claude".into(),
        newen_model::AgentSettings {
            model: Some("haiku".into()),
            effort: Some("ultracode".into()),
            ..Default::default()
        },
    );
    ui.agent = "claude".into();
    click(&store, &mut ui, view::NEW_SESSION);
    assert_eq!(ui.model.as_deref(), Some("haiku"));
    click(&store, &mut ui, "pick-agent:codex");
    click(&store, &mut ui, view::NEW_SESSION);
    assert_eq!((ui.model.as_deref(), ui.effort.as_deref()), (None, None));
    assert_eq!(ui.mode, "auto");
}

#[test]
fn both_programs_top_effort_is_drawn_alive() {
    assert!(view::ultra_effort("ultracode") && view::ultra_effort("ultra"));
    assert!(!view::ultra_effort("max"));
}

#[test]
fn full_access_is_offered_once_allowed_even_before_any_codex_session() {
    let store = Store::default();
    let mut ui = Ui::new("/work".into());
    let word = |id: &str, tone| newen_model::ModeWords {
        id: id.into(),
        label: id.into(),
        about: String::new(),
        tone,
        listed: !matches!(tone, newen_model::ModeTone::Unsafe),
    };
    ui.agents = vec![newen_model::AgentInfo {
        id: "codex".into(),
        name: "Codex".into(),
        tested: vec![],
        options: vec![],
        icon: None,
        sign_in_methods: vec![],
        sign_in_code: false,
        modes: vec![
            word("auto", newen_model::ModeTone::Auto),
            word("full-access", newen_model::ModeTone::Unsafe),
        ],
        default_mode: "auto".into(),
        customize: Default::default(),
    }];
    ui.agent = "codex".into();
    assert!(!view::menu_modes(&store, None, &ui).contains(&"full-access".to_owned()));
    ui.settings
        .agents
        .entry("codex".into())
        .or_default()
        .allow_unsafe = true;
    assert!(view::menu_modes(&store, None, &ui).contains(&"full-access".to_owned()));
}

#[test]
fn one_agents_plan_limits_never_show_for_another() {
    let mut store = Store::default();
    for (session, agent) in [(1, "claude"), (2, "codex")] {
        store.apply(Event::SessionCreated {
            session,
            agent: agent.into(),
            cwd: "/work".into(),
        });
    }
    let window = |utilization| newen_model::UsageWindow {
        utilization,
        resets_at: 0,
    };
    store.apply(Event::Agent {
        session: 1,
        request: None,
        event: AgentEvent::Limits(newen_model::PlanLimits {
            session: Some(window(0.6)),
            weekly: None,
        }),
    });
    let mut ui = Ui::new("/work".into());
    ui.selected = Some(1);
    assert!(view::agent_limits(&store, &ui).is_some());
    ui.selected = Some(2);
    assert!(view::agent_limits(&store, &ui).is_none());
}

#[test]
fn the_steps_of_a_review_a_compaction_a_wait_and_an_image_say_what_they_did() {
    let none = serde_json::json!({});
    let label = |name: &'static str, input: &serde_json::Value, done: bool| {
        let result = ToolResult {
            output: String::new(),
            is_error: false,
            added: 0,
            removed: 0,
            files: vec![],
            stopped: false,
            web: None,
            images: vec![],
        };
        let step = view::Step {
            call: "s1",
            name,
            input,
            result: done.then_some(&result),
            running: !done,
        };
        view::group_summary(&[step])
    };
    assert_eq!(label("Compact", &none, true), "Compacted context");
    assert_eq!(label("Compact", &none, false), "Compacting context");
    let review = serde_json::json!({ "review": "current changes" });
    assert_eq!(
        label("EnterReviewMode", &review, true),
        "Started a review of current changes"
    );
    assert_eq!(label("EnterReviewMode", &none, true), "Started a review");
    assert_eq!(label("ExitReviewMode", &none, true), "Finished the review");
    assert_eq!(
        label("Sleep", &serde_json::json!({ "duration_ms": 1000 }), true),
        "Waited 1s"
    );
    assert_eq!(
        label("Sleep", &serde_json::json!({ "duration_ms": 2500 }), false),
        "Waiting 2.5s"
    );
    assert_eq!(
        label("Sleep", &serde_json::json!({ "duration_ms": 300 }), true),
        "Waited 300ms"
    );
    assert_eq!(label("ImageGeneration", &none, true), "Generated an image");
    assert_eq!(
        label("ImageGeneration", &none, false),
        "Generating an image"
    );
    assert_eq!(label("WaitAgent", &none, true), "Waited for agents");
    assert_eq!(label("lookup", &none, true), "Used lookup");
}

#[test]
fn a_generated_image_opens_in_the_viewer_and_a_read_one_too() {
    let (mut store, mut ui) = store_with_session();
    tool(
        &mut store,
        "g1",
        "ImageGeneration",
        serde_json::json!({}),
        None,
    );
    agent(
        &mut store,
        AgentEvent::ToolResult {
            call: "g1".into(),
            output: "A quill pen".into(),
            is_error: false,
            files: vec![newen_model::FileChange {
                path: "/work/gen.png".into(),
                before: None,
                after: None,
            }],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    );
    tool(
        &mut store,
        "v1",
        "Read",
        serde_json::json!({ "file_path": "/work/logo.png" }),
        None,
    );
    result(&mut store, "v1", "");
    tool(
        &mut store,
        "v2",
        "Read",
        serde_json::json!({ "file_path": "/work/main.rs" }),
        None,
    );
    result(&mut store, "v2", "fn main() {}");
    // The pictures are loaded as a message's are; a source file is not a picture.
    assert_eq!(
        image_paths(&store, &ui),
        [
            PathBuf::from("/work/gen.png"),
            PathBuf::from("/work/logo.png")
        ]
    );
    // Its picture is on screen without opening anything; the prompt waits behind its line.
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(frame.regions.iter().any(|r| r.id == "image:tool:g1"));
    let drawn = |ui: &Ui, words: &str| {
        let frame = UiTree::new(view(&store, ui, window)).layout(window);
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    assert!(drawn(&ui, "Generated an image"));
    assert!(!drawn(&ui, "A quill pen"));
    click(&store, &mut ui, "tool:g1");
    assert!(drawn(&ui, "A quill pen"));
    click(&store, &mut ui, "image:tool:g1");
    let viewer = ui.viewer.as_ref().expect("the viewer opened");
    assert_eq!(viewer.path, PathBuf::from("/work/gen.png"));
    assert!(!viewer.annotatable);
    click(&store, &mut ui, view::IMAGE_CLOSE);
    click(&store, &mut ui, "image:tool:v1");
    assert_eq!(
        ui.viewer.as_ref().unwrap().path,
        PathBuf::from("/work/logo.png")
    );
    // A failed one is a step, with no picture.
    tool(
        &mut store,
        "g2",
        "ImageGeneration",
        serde_json::json!({}),
        None,
    );
    agent(
        &mut store,
        AgentEvent::ToolResult {
            call: "g2".into(),
            output: "Image generation limit reached".into(),
            is_error: true,
            files: vec![],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    );
    assert_eq!(image_paths(&store, &ui).len(), 2);
}

/// A tool's result that gave back `images`, as the engine kept them.
fn pictured(store: &mut Store, call: &str, images: &[&str], is_error: bool) {
    agent(
        store,
        AgentEvent::ToolResult {
            call: call.into(),
            output: String::new(),
            is_error,
            files: vec![],
            command: None,
            web: None,
            images: images.iter().map(PathBuf::from).collect(),
            inline: vec![],
        },
    );
}

fn screenshot(store: &mut Store, call: &str, images: &[&str]) {
    let input = serde_json::json!({ "action": "screenshot" });
    tool(store, call, "mcp__browser__computer", input, None);
    pictured(store, call, images, false);
}

fn region(frame: &limn::Frame, id: &str) -> Option<limn::Rect> {
    frame.regions.iter().find(|r| r.id == id).map(|r| r.rect)
}

/// The step pictures a frame shows, in order.
fn thumbnails(frame: &limn::Frame) -> Vec<String> {
    let mut ids: Vec<String> = vec![];
    for region in &frame.regions {
        if region.id.starts_with("image:step:") && !ids.contains(&region.id) {
            ids.push(region.id.clone());
        }
    }
    ids
}

/// The frame of `view`, its motion settled at once.
fn still(store: &Store, ui: &Ui) -> limn::Frame {
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(store, ui, window));
    tree.set_reduced_motion(true);
    tree.layout(window)
}

fn painted(frame: &limn::Frame, words: &str) -> Option<limn::Point> {
    frame.commands.iter().find_map(|command| match command {
        limn::DrawCommand::Text { text, position, .. } if text == words => Some(*position),
        _ => None,
    })
}

#[test]
fn a_run_s_pictures_sit_at_its_line_s_end_and_the_viewer_steps_through_them() {
    let (mut store, mut ui) = store_with_session();
    screenshot(&mut store, "c1", &["/work/a.png"]);
    tool(
        &mut store,
        "c2",
        "Bash",
        serde_json::json!({ "command": "ls" }),
        None,
    );
    result(&mut store, "c2", "a.png");
    screenshot(&mut store, "c3", &["/work/b.png"]);
    // A failed step's picture is not shown, as Desktop's.
    let input = serde_json::json!({ "action": "screenshot" });
    tool(&mut store, "c4", "mcp__browser__computer", input, None);
    pictured(&mut store, "c4", &["/work/failed.png"], true);
    // A message ends the run: what follows is counted apart.
    agent(
        &mut store,
        AgentEvent::Text {
            text: "Here it is".into(),
            parent: None,
            id: None,
            at: None,
        },
    );
    screenshot(&mut store, "c5", &["/work/later.png"]);
    // Their thumbnails are decoded as a message's pictures are.
    assert_eq!(
        image_paths(&store, &ui),
        ["/work/a.png", "/work/b.png", "/work/later.png"].map(PathBuf::from)
    );
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert_eq!(
        thumbnails(&frame),
        ["image:step:c1:0", "image:step:c3:0", "image:step:c5:0"]
    );
    // At the end of the run's line, 24 px tall, 4 px apart.
    let line = region(&frame, "steps:c1").expect("the run's line");
    let (a, b) = (
        region(&frame, "image:step:c1:0").unwrap(),
        region(&frame, "image:step:c3:0").unwrap(),
    );
    assert!(a.x > line.x + line.width, "{a:?} after {line:?}");
    assert_eq!(a.height, 24.0);
    assert!((b.x - (a.x + a.width) - 4.0).abs() < 0.5, "{a:?} {b:?}");
    let middle = |r: limn::Rect| r.y + r.height / 2.0;
    assert!((middle(a) - middle(line)).abs() < 1.0);
    // A click opens the viewer on it, among the run's pictures, each named by its step. The
    // host has read their sizes, which fit them in the window.
    for path in ["/work/a.png", "/work/b.png", "/work/later.png"] {
        ui.sizes.insert(path.into(), (1280.0, 720.0));
    }
    click(&store, &mut ui, "image:step:c3:0");
    let viewer = ui.viewer.as_ref().expect("the viewer opened");
    assert_eq!(
        (viewer.path.clone(), viewer.index, viewer.annotatable),
        (PathBuf::from("/work/b.png"), 1, false)
    );
    let named = |path: &str| (PathBuf::from(path), "Captured page".to_owned());
    assert_eq!(viewer.gallery, [named("/work/a.png"), named("/work/b.png")]);
    let frame = still(&store, &ui);
    assert!(painted(&frame, "Captured page").is_some());
    assert!(region(&frame, view::IMAGE_PREVIOUS).is_some());
    // The run's thumbnails under it say where it is among them.
    assert!(region(&frame, "image:at:0").is_some() && region(&frame, "image:at:1").is_some());
    // Its arrows step through them and wrap around the ends, as limn's lightbox does.
    click(&store, &mut ui, view::IMAGE_NEXT);
    let viewer = ui.viewer.as_ref().unwrap();
    assert_eq!(
        (viewer.index, viewer.path.clone()),
        (0, PathBuf::from("/work/a.png"))
    );
    click(&store, &mut ui, view::IMAGE_PREVIOUS);
    assert_eq!(ui.viewer.as_ref().unwrap().index, 1);
    click(&store, &mut ui, "image:at:0");
    assert_eq!(ui.viewer.as_ref().unwrap().index, 0);
    let frame = still(&store, &ui);
    assert!(region(&frame, view::IMAGE_NEXT).is_some());
    // A picture alone in its run has no arrows and no thumbnails.
    click(&store, &mut ui, view::IMAGE_CLOSE);
    click(&store, &mut ui, "image:step:c5:0");
    assert_eq!(ui.viewer.as_ref().unwrap().gallery.len(), 1);
    let frame = still(&store, &ui);
    assert!(region(&frame, view::IMAGE_NEXT).is_none() && region(&frame, "image:at:0").is_none());
}

/// A stand-in picture whose digest is made of `mark`.
fn picture(mark: char) -> limn::RasterImage {
    let digest = mark.to_string().repeat(64);
    limn::RasterImage::new(
        digest,
        limn::RasterImageFormat::Png,
        4,
        "AAAA".into(),
        "shot",
    )
    .unwrap()
}

/// Where the viewer draws a picture, and whether it is drawn at full resolution.
fn shown_picture(frame: &limn::Frame, image: &limn::RasterImage) -> Option<(limn::Rect, bool)> {
    frame.commands.iter().find_map(|command| match command {
        limn::DrawCommand::RasterImage {
            rect, image: shown, ..
        } if shown.sha256() == image.sha256() => Some((*rect, shown.is_full_resolution())),
        _ => None,
    })
}

/// The viewer open on `/work/shot.png` of `size` pixels, on a screen of `scale` pixels a point.
fn viewing(size: (f32, f32), scale: f32) -> (Store, Ui) {
    let (store, mut ui) = store_with_session();
    let path = PathBuf::from("/work/shot.png");
    ui.sizes.insert(path.clone(), size);
    ui.thumbs.insert(path.clone(), picture('b'));
    ui.full.insert(path.clone(), picture('f'));
    ui.pixels_per_point = scale;
    ui.viewer = Some(Viewer {
        path,
        ..Default::default()
    });
    (store, ui)
}

const BIG_WINDOW: limn::Size = limn::Size::new(2000.0, 1200.0);

fn viewer_frame(store: &Store, ui: &Ui) -> limn::Frame {
    let mut tree = UiTree::new(view(store, ui, BIG_WINDOW));
    tree.set_reduced_motion(true);
    tree.layout(BIG_WINDOW)
}

#[test]
fn the_viewer_fills_the_window_with_a_large_screenshot_at_its_proportions() {
    let (store, ui) = viewing((2880.0, 1800.0), 1.0);
    let (rect, _) =
        shown_picture(&viewer_frame(&store, &ui), &picture('f')).expect("the picture is drawn");
    // As tall as the window less limn's lightbox margins (36 above and below) and its top bar
    // (40 and a 12 gap): 1076, and 1.6 times as wide.
    assert!((rect.height - 1076.0).abs() < 1.0, "{rect:?}");
    assert!((rect.width / rect.height - 1.6).abs() < 0.01, "{rect:?}");
    // Centred across the window.
    assert!((rect.x + rect.width / 2.0 - 1000.0).abs() < 1.0, "{rect:?}");
}

#[test]
fn the_viewer_does_not_enlarge_a_small_picture_past_its_pixels() {
    let (store, ui) = viewing((400.0, 300.0), 1.0);
    let (rect, _) = shown_picture(&viewer_frame(&store, &ui), &picture('f')).unwrap();
    assert_eq!((rect.width, rect.height), (400.0, 300.0));
    // On a screen of two pixels a point, one of its pixels on each of the screen's.
    let (store, ui) = viewing((400.0, 300.0), 2.0);
    let (rect, _) = shown_picture(&viewer_frame(&store, &ui), &picture('f')).unwrap();
    assert_eq!((rect.width, rect.height), (200.0, 150.0));
}

#[test]
fn the_viewer_shows_the_full_picture_and_its_thumbnail_only_until_it_arrives() {
    let (store, mut ui) = viewing((2880.0, 1800.0), 1.0);
    let frame = viewer_frame(&store, &ui);
    assert_eq!(
        shown_picture(&frame, &picture('f')).map(|(_, full)| full),
        Some(true)
    );
    assert!(shown_picture(&frame, &picture('b')).is_none());
    ui.full.clear();
    let frame = viewer_frame(&store, &ui);
    assert_eq!(
        shown_picture(&frame, &picture('b')).map(|(_, full)| full),
        Some(false)
    );
}

#[test]
fn the_viewer_zooms_pans_and_fits_and_another_picture_opens_fitted() {
    let (mut store, mut ui) = store_with_session();
    screenshot(&mut store, "c1", &["/work/a.png"]);
    screenshot(&mut store, "c2", &["/work/b.png"]);
    for path in ["/work/a.png", "/work/b.png"] {
        ui.sizes.insert(path.into(), (2880.0, 1800.0));
        ui.full.insert(path.into(), picture('f'));
    }
    click(&store, &mut ui, "image:step:c1:0");
    let mut tree = UiTree::new(view(&store, &ui, BIG_WINDOW));
    tree.set_reduced_motion(true);
    let frame = tree.layout(BIG_WINDOW);
    let id = format!("{}:view:0:Captured page", view::IMAGE_CLOSE);
    let fit = tree.zoom_scale(&id).expect("a zoom view");
    let fitted = shown_picture(&frame, &picture('f')).unwrap().0;
    let middle = limn::Point::new(1000.0, 600.0);
    let send = |tree: &mut UiTree, ui: &mut Ui, event: limn::UiEvent| {
        for action in tree.handle_event(event) {
            update(&store, ui, &action);
        }
        tree.set_root(view(&store, ui, BIG_WINDOW));
        tree.layout(BIG_WINDOW)
    };
    let key = |key| limn::UiEvent::Key {
        key,
        shift: false,
        ctrl: false,
        alt: false,
    };
    // + zooms in by a step, which the viewer keeps; 0 fits again; − at the fit stays there.
    send(&mut tree, &mut ui, key(limn::Key::Plus));
    let (scale, at_fit) = ui.viewer.as_ref().unwrap().zoom.expect("the zoom is kept");
    assert!((scale - fit * 1.5).abs() < 0.001 && !at_fit);
    // Zoomed in, the arrows hide.
    let frame = send(&mut tree, &mut ui, limn::UiEvent::PointerMove(middle));
    assert!(region(&frame, view::IMAGE_NEXT).is_none());
    // A drag pans it.
    let before = shown_picture(&frame, &picture('f')).unwrap().0;
    send(&mut tree, &mut ui, limn::UiEvent::PointerDown(middle));
    let to = limn::Point::new(middle.x + 80.0, middle.y + 20.0);
    send(&mut tree, &mut ui, limn::UiEvent::PointerMove(to));
    let frame = send(&mut tree, &mut ui, limn::UiEvent::PointerUp(to));
    let after = shown_picture(&frame, &picture('f')).unwrap().0;
    assert!((after.x - before.x - 80.0).abs() < 0.5 && (after.y - before.y - 20.0).abs() < 0.5);
    // The wheel zooms about the pointer.
    send(
        &mut tree,
        &mut ui,
        limn::UiEvent::Scroll {
            position: to,
            delta_y: 30.0,
        },
    );
    assert!(ui.viewer.as_ref().unwrap().zoom.unwrap().0 > scale);
    // 0 fits it again; the run's thumbnails come back under it.
    let frame = send(&mut tree, &mut ui, key(limn::Key::Num0));
    assert!(ui.viewer.as_ref().unwrap().zoom.unwrap().1);
    assert_eq!(tree.zoom_scale(&id), Some(fit));
    assert_eq!(shown_picture(&frame, &picture('f')).unwrap().0, fitted);
    send(&mut tree, &mut ui, key(limn::Key::Minus));
    assert_eq!(tree.zoom_scale(&id), Some(fit));
    // Another picture opens fitted, its zoom forgotten.
    send(&mut tree, &mut ui, key(limn::Key::Plus));
    click(&store, &mut ui, view::IMAGE_NEXT);
    assert_eq!(ui.viewer.as_ref().unwrap().zoom, None);
    tree.set_root(view(&store, &ui, BIG_WINDOW));
    tree.layout(BIG_WINDOW);
    let next = format!("{}:view:1:Captured page", view::IMAGE_CLOSE);
    assert_eq!(tree.zoom_scale(&next), Some(fit));
}

#[test]
fn resting_on_a_thumbnail_shows_it_larger_and_a_long_run_keeps_its_last_three() {
    let (mut store, mut ui) = store_with_session();
    for n in 1..=5 {
        screenshot(&mut store, &format!("c{n}"), &[&format!("/work/{n}.png")]);
    }
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    let frame = tree.layout(window);
    // The last three, and how many come before them.
    assert_eq!(
        thumbnails(&frame),
        ["image:step:c3:0", "image:step:c4:0", "image:step:c5:0"]
    );
    assert!(painted(&frame, "+2").is_some());
    // Under the pointer, a card above it: the step's label and the picture's place in the run.
    let thumb = region(&frame, "image:step:c4:0").unwrap();
    assert!(painted(&frame, "4 of 5").is_none());
    let center = limn::Point::new(thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);
    tree.handle_event(limn::UiEvent::PointerMove(center));
    let frame = tree.layout(window);
    let count = painted(&frame, "4 of 5").expect("the card counts it");
    let caption = (frame.commands.iter()).any(|command| {
        matches!(command, limn::DrawCommand::Text { text, position, .. }
            if text == "Captured page" && position.y == count.y && position.x < count.x)
    });
    assert!(caption, "the card names it by its step");
    // Below it here: the run is too near the top for the card to fit above.
    assert!(count.y > thumb.y + thumb.height && count.x < thumb.x + thumb.width);
    // Open, each step's row shows its own picture, and the run's line none.
    click(&store, &mut ui, "steps:c1");
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let ids = thumbnails(&frame);
    assert_eq!(ids.len(), 5, "{ids:?}");
    assert!(painted(&frame, "+2").is_none());
    let rows: Vec<f32> = (ids.iter())
        .map(|id| region(&frame, id).unwrap().y)
        .collect();
    assert!(
        rows.windows(2).all(|pair| pair[1] > pair[0] + 24.0),
        "{rows:?}"
    );
}

#[test]
fn a_helper_row_opens_its_pane_and_is_not_the_agent_picker() {
    // The picker's ids were `agent:<id>` as the helper's `agent:<call>` are: a click on a
    // helper chose an agent named after the call and saved it as the default.
    let (mut store, mut ui) = store_with_session();
    tool(
        &mut store,
        "call_9",
        "Agent",
        serde_json::json!({ "description": "Ping" }),
        None,
    );
    let before = ui.agent.clone();
    let saved = click(&store, &mut ui, "agent:call_9");
    assert!(saved.is_empty());
    assert_eq!(ui.agent, before);
    assert_eq!(ui.helper.as_deref(), Some("call_9"));
    assert_eq!(ui.settings.default_agent, None);
}

/// Claude Code and Codex, as the engine tells the agents, the folder `/work` a project and the
/// Customize page `page` open.
fn two_agents_on(page: &str) -> (Store, Ui) {
    let (store, mut ui) = store_with_session();
    let agent = |id: &str, name: &str| newen_model::AgentInfo {
        id: id.into(),
        name: name.into(),
        tested: vec![],
        options: vec![],
        icon: None,
        sign_in_methods: vec![],
        sign_in_code: true,
        modes: claude_modes(),
        default_mode: "default".into(),
        customize: Default::default(),
    };
    ui.applied(&Event::Settings {
        settings: Default::default(),
        agents: vec![agent("claude", "Claude Code"), agent("codex", "Codex")],
    });
    ui.cwd = "/work".into();
    click(&store, &mut ui, view::CUSTOMIZE_OPEN);
    click(&store, &mut ui, &format!("czp:tab:{page}"));
    (store, ui)
}

fn drawn(store: &Store, ui: &Ui, words: &str) -> bool {
    drawn_words(store, ui, words)
}

fn skill(name: &str, source: &str, state: &str) -> newen_model::Skill {
    newen_model::Skill {
        name: name.into(),
        description: format!("The {name} skill."),
        source: source.into(),
        state: state.into(),
        locked: None,
        key: serde_json::json!({ "name": name }),
    }
}

fn heard_extensions(ui: &mut Ui, agent: &str, list: newen_model::Extensions) {
    ui.applied(&Event::Extensions {
        agent: agent.into(),
        folder: "/work".into(),
        extensions: list,
        error: None,
    });
}

#[test]
fn skills_show_one_section_per_agent_and_one_with_no_scopes_does_not_wait_forever() {
    let (store, mut ui) = two_agents_on(view::SETTINGS_SKILLS);
    // Until each agent's program answers, its own section says it looks.
    assert!(drawn(&store, &ui, "Claude Code") && drawn(&store, &ui, "Codex"));
    assert!(drawn(&store, &ui, "Looking for skills"));
    let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
    heard_extensions(
        &mut ui,
        "claude",
        newen_model::Extensions {
            skills: vec![skill("review", "user", "on")],
            scopes: vec![pair("user", "Install for me")],
            skill_states: vec![pair("on", "On"), pair("off", "Off")],
            ..Default::default()
        },
    );
    // Codex lists its skills and no scope to install plugins in: its section is not stuck.
    heard_extensions(
        &mut ui,
        "codex",
        newen_model::Extensions {
            skills: vec![
                skill("review", "user", "on"),
                skill("imagegen", "system", "on"),
            ],
            skill_states: vec![pair("on", "On"), pair("off", "Off")],
            kinds: newen_model::ExtensionKinds {
                skills_per_folder: false,
                switch_plugins: false,
                update_plugins: false,
            },
            ..Default::default()
        },
    );
    assert!(!drawn(&store, &ui, "Looking for skills"));
    assert!(drawn(&store, &ui, "imagegen") && drawn(&store, &ui, "System"));
    // A skill both have is marked with both agents in each row; one only Codex has, not.
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    for marks in ["cz:marks:claude:review:1", "cz:marks:codex:review:1"] {
        assert!(region(&frame, marks).is_some(), "{marks}");
    }
    assert!(region(&frame, "cz:marks:codex:imagegen:0").is_none());
    // Codex keeps a skill's state for the person: no project folder is asked for.
    ui.cwd = std::path::PathBuf::new();
    click(&store, &mut ui, "cz:more:codex:review");
    assert!(!drawn(&store, &ui, "Choose a project folder"));
    click(&store, &mut ui, "cz:more:claude:review");
    assert!(drawn(&store, &ui, "Choose a project folder"));
    // Its state change acts on Codex only.
    ui.cwd = "/work".into();
    let sent = click(&store, &mut ui, "ext:skill:codex:review=off");
    assert!(matches!(
        sent.as_slice(),
        [Intent::ExtensionOp { agent, op: newen_model::ExtensionOp::SkillState { state, .. }, .. }]
            if agent == "codex" && state == "off"
    ));
}

#[test]
fn connectors_list_each_agents_servers_apart_and_an_empty_one_names_its_agent() {
    let (store, mut ui) = two_agents_on(view::SETTINGS_CONNECTORS);
    let server = |name: &str, checked| newen_model::ConfiguredServer {
        name: name.into(),
        label: name.into(),
        target: "npx -y x".into(),
        status: newen_model::McpStatus::Connected,
        checked,
        source: newen_model::McpSource::Other,
    };
    ui.applied(&Event::Servers {
        agent: "claude".into(),
        folder: "/work".into(),
        servers: vec![server("github", true)],
        scopes: vec![("user".into(), "All my projects".into())],
        error: None,
    });
    ui.applied(&Event::Servers {
        agent: "codex".into(),
        folder: "/work".into(),
        servers: vec![],
        scopes: vec![("user".into(), "All my projects".into())],
        error: None,
    });
    // A list with each one's type and status; an agent without any says how to start.
    assert!(drawn(&store, &ui, "github") && drawn(&store, &ui, "Connected"));
    assert!(drawn(&store, &ui, "Local") && drawn(&store, &ui, "Status"));
    assert!(drawn(&store, &ui, "Connectors give Codex tools"));
    assert!(!drawn(&store, &ui, "Connectors give Claude Code tools"));
    // What its list did not check is not said to be connected.
    ui.applied(&Event::Servers {
        agent: "codex".into(),
        folder: "/work".into(),
        servers: vec![server("node_repl", false)],
        scopes: vec![],
        error: Some("Codex can\u{2019}t do that.".into()),
    });
    assert!(drawn(&store, &ui, "On") && drawn(&store, &ui, "Codex can\u{2019}t do that."));
    // The same server on both marks both agents on its row.
    ui.applied(&Event::Servers {
        agent: "claude".into(),
        folder: "/work".into(),
        servers: vec![server("node_repl", true)],
        scopes: vec![],
        error: None,
    });
    let both: Vec<String> = view::marked_agents(&ui, |a| view::server_on_in(&ui, a, "node_repl"))
        .map(|a| a.id.clone())
        .collect();
    assert_eq!(both, ["claude", "codex"]);
}

#[test]
fn the_add_dialog_asks_for_which_agents_and_adds_with_each_ones_own_program() {
    let (store, mut ui) = two_agents_on(view::SETTINGS_CONNECTORS);
    ui.agent = "codex".into();
    for (agent, scopes) in [
        (
            "claude",
            vec![
                ("local", "This project, for me"),
                ("user", "All my projects"),
            ],
        ),
        ("codex", vec![("user", "All my projects")]),
    ] {
        ui.applied(&Event::Servers {
            agent: agent.into(),
            folder: "/work".into(),
            servers: vec![],
            scopes: scopes
                .iter()
                .map(|(a, b)| ((*a).into(), (*b).into()))
                .collect(),
            error: None,
        });
    }
    // The header's Add starts with the agent a new session would run, not the first one.
    click(&store, &mut ui, "srv:add:");
    assert_eq!(ui.connectors.adding.as_ref().unwrap().agents, ["codex"]);
    assert!(drawn(&store, &ui, "Add to") && drawn(&store, &ui, "Add custom connector"));
    let typed = |ui: &mut Ui, key: &str, value: &str| {
        update(
            &store,
            ui,
            &UiAction::InputChanged {
                id: format!("{}{key}", view::SRV_FIELD),
                value: value.into(),
            },
        )
    };
    typed(&mut ui, "name", "pg");
    typed(&mut ui, "target", "npx -y @x/pg");
    // Ticking Claude Code adds its scopes to choose from; unticking the last one is not done.
    click(&store, &mut ui, "srv:to:claude");
    assert!(drawn(&store, &ui, "Where in Claude Code") && !drawn(&store, &ui, "Where in Codex"));
    click(&store, &mut ui, "srv:scope:claude:user");
    click(&store, &mut ui, "srv:to:codex");
    click(&store, &mut ui, "srv:to:claude");
    assert_eq!(ui.connectors.adding.as_ref().unwrap().agents, ["claude"]);
    click(&store, &mut ui, "srv:to:codex");
    let sent = click(&store, &mut ui, "srv:save");
    let folder = std::path::PathBuf::from("/work");
    let add = |agent: &str, scope: Option<&str>| Intent::ExtensionOp {
        agent: agent.into(),
        folder: folder.clone(),
        op: newen_model::ExtensionOp::AddServer(newen_model::NewServer {
            name: "pg".into(),
            target: "npx".into(),
            args: vec!["-y".into(), "@x/pg".into()],
            scope: scope.map(str::to_owned),
            ..Default::default()
        }),
    };
    // One intent a program, each with the scope chosen for it, and both sections wait.
    assert_eq!(sent, vec![add("claude", Some("user")), add("codex", None)]);
    assert!(ui.connectors.listed["claude"].working && ui.connectors.listed["codex"].working);
    assert!(ui.connectors.adding.is_none());
}

#[test]
fn all_agents_is_the_first_section_and_its_dialog_keeps_secrets_apart() {
    use newen_model::{AppliedTo, Applies, GlobalOp, GlobalServer, GlobalState, GlobalVar};
    let (store, mut ui) = two_agents_on(view::SETTINGS_CONNECTORS);
    let applied = |agent: &str, applies| AppliedTo {
        agent: agent.into(),
        name: "github".into(),
        skill: false,
        applies,
    };
    ui.applied(&Event::Global(GlobalState {
        servers: vec![GlobalServer {
            name: "github".into(),
            target: "npx".into(),
            except: vec!["codex".into()],
            ..Default::default()
        }],
        applied: vec![
            applied("claude", Applies::Replaced),
            applied("codex", Applies::Excluded),
        ],
        keychain: true,
        ..Default::default()
    }));
    assert!(drawn(&store, &ui, "All agents") && drawn(&store, &ui, "Replaced in Claude Code"));
    assert!(drawn(&store, &ui, "off in Codex"));
    // Turning it back on for Codex asks the engine; removing it too.
    assert_eq!(
        click(&store, &mut ui, "glob:use:server:github:codex"),
        vec![Intent::Global(GlobalOp::Use {
            skill: false,
            name: "github".into(),
            agent: "codex".into(),
            on: true
        })]
    );
    assert_eq!(
        click(&store, &mut ui, "glob:remove:server:github"),
        vec![Intent::Global(GlobalOp::RemoveServer("github".into()))]
    );
    // The dialog's "All agents" sends a global server, its secret lines marked.
    click(&store, &mut ui, "srv:add:");
    click(&store, &mut ui, "srv:to:*");
    assert!(drawn(&store, &ui, "Secret variables"));
    for (key, value) in [
        ("name", "pg"),
        ("target", "npx -y @x/pg"),
        ("env", "LOG=warn"),
        ("secret-env", "TOKEN=abc"),
    ] {
        update(
            &store,
            &mut ui,
            &UiAction::InputChanged {
                id: format!("{}{key}", view::SRV_FIELD),
                value: value.into(),
            },
        );
    }
    let var = |key: &str, value: &str, secret| GlobalVar {
        key: key.into(),
        value: Some(value.into()),
        secret,
    };
    assert_eq!(
        click(&store, &mut ui, "srv:save"),
        vec![Intent::Global(GlobalOp::SetServer(GlobalServer {
            name: "pg".into(),
            target: "npx".into(),
            args: vec!["-y".into(), "@x/pg".into()],
            env: vec![var("LOG", "warn", false), var("TOKEN", "abc", true)],
            ..Default::default()
        }))]
    );
}

#[test]
fn a_global_skill_is_made_or_imported_from_its_dialog() {
    use newen_model::{GlobalOp, GlobalSkill, GlobalState};
    let (store, mut ui) = two_agents_on(view::SETTINGS_SKILLS);
    ui.applied(&Event::Global(GlobalState {
        skills: vec![GlobalSkill {
            name: "release-notes".into(),
            description: "Writes notes".into(),
            except: vec![],
        }],
        ..Default::default()
    }));
    assert!(drawn(&store, &ui, "release-notes") && drawn(&store, &ui, "All agents"));
    click(&store, &mut ui, "glob:skill-new");
    assert!(drawn(&store, &ui, "Write a skill"));
    for (key, value) in [
        ("name", " my-skill "),
        ("description", "Does it"),
        ("instructions", "Do it well."),
    ] {
        update(
            &store,
            &mut ui,
            &UiAction::InputChanged {
                id: format!("{}{key}", view::GSK_FIELD),
                value: value.into(),
            },
        );
    }
    assert_eq!(
        click(&store, &mut ui, "glob:skill-save"),
        vec![Intent::Global(GlobalOp::NewSkill {
            name: "my-skill".into(),
            description: "Does it".into(),
            instructions: "Do it well.".into(),
        })]
    );
    click(&store, &mut ui, "glob:skill-import");
    assert!(drawn(&store, &ui, "Import skill"));
    click(&store, &mut ui, "glob:skill-cancel");
    assert!(ui.skill_dialog.is_none());
}

#[test]
fn a_marketplace_is_added_to_every_agent_ticked_and_managed_by_agent() {
    let (store, mut ui) = two_agents_on(view::SETTINGS_PLUGINS);
    let market = |name: &str| newen_model::Marketplace {
        name: name.into(),
        source: format!("o/{name}"),
    };
    for (agent, name) in [("claude", "better"), ("codex", "curated")] {
        heard_extensions(
            &mut ui,
            agent,
            newen_model::Extensions {
                marketplaces: vec![market(name)],
                ..Default::default()
            },
        );
    }
    // From the page it starts with the agent a new session runs.
    ui.agent = "claude".into();
    click(&store, &mut ui, "cz:add-marketplace:");
    assert_eq!(ui.market_targets, ["claude"]);
    update(
        &store,
        &mut ui,
        &UiAction::InputChanged {
            id: format!("{}all", view::EXT_MKT_FIELD),
            value: "o/r".into(),
        },
    );
    click(&store, &mut ui, "cz:to:codex");
    let sent = click(&store, &mut ui, "ext:mkt-add");
    let folder = std::path::PathBuf::from("/work");
    let add = |agent: &str| Intent::ExtensionOp {
        agent: agent.into(),
        folder: folder.clone(),
        op: newen_model::ExtensionOp::AddMarketplace("o/r".into()),
    };
    assert_eq!(sent, vec![add("claude"), add("codex")]);
    assert!(ui.extensions["codex"].working.as_deref() == Some("o/r"));
    // Managing lists each agent's, under its name.
    click(&store, &mut ui, "cz:manage:");
    assert!(drawn(&store, &ui, "better") && drawn(&store, &ui, "curated"));
    assert!(drawn(&store, &ui, "Codex") && drawn(&store, &ui, "Claude Code"));
}

#[test]
fn a_plugin_menu_offers_only_what_the_agents_program_can_do() {
    let (store, mut ui) = two_agents_on(view::SETTINGS_PLUGINS);
    let plugin = |name: &str| newen_model::Plugin {
        id: format!("{name}@m"),
        name: name.into(),
        version: None,
        scope: "user".into(),
        enabled: true,
    };
    heard_extensions(
        &mut ui,
        "claude",
        newen_model::Extensions {
            plugins: vec![plugin("pdf")],
            ..Default::default()
        },
    );
    heard_extensions(
        &mut ui,
        "codex",
        newen_model::Extensions {
            plugins: vec![plugin("pdf")],
            kinds: newen_model::ExtensionKinds {
                skills_per_folder: false,
                switch_plugins: false,
                update_plugins: false,
            },
            ..Default::default()
        },
    );
    click(&store, &mut ui, "cz:more:claude:pdf@m");
    assert!(drawn(&store, &ui, "Turn off") && drawn(&store, &ui, "Update"));
    click(&store, &mut ui, "cz:more:claude:pdf@m");
    click(&store, &mut ui, "cz:more:codex:pdf@m");
    assert!(
        drawn(&store, &ui, "Remove")
            && !drawn(&store, &ui, "Turn off")
            && !drawn(&store, &ui, "Update")
    );
    // The directory shows whose it is, and another agent's is one click away.
    click(&store, &mut ui, "cz:discover:claude");
    assert_eq!(ui.directory.as_deref(), Some("claude"));
    click(&store, &mut ui, "cz:dir:codex");
    assert_eq!(ui.directory.as_deref(), Some("codex"));
}

#[test]
fn an_agents_skills_join_its_slash_menu_and_its_mcp_switch_waits_for_a_capability() {
    use newen_model::{McpServer, McpSource, McpStatus};
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::Ready {
            version: "0.163.0".into(),
            capabilities: newen_model::AgentCapabilities {
                mcp: true,
                ..Default::default()
            },
            mode: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::Skills(vec![
            skill("review", "user", "on"),
            skill("old", "user", "off"),
        ]),
    );
    // A later `Ready` (Codex says it twice) does not take them away.
    agent(
        &mut store,
        AgentEvent::Ready {
            version: "0.163.0".into(),
            capabilities: newen_model::AgentCapabilities {
                mcp: true,
                ..Default::default()
            },
            mode: None,
        },
    );
    ui.selected = Some(1);
    ui.prompt = "/rev".into();
    let (_, rows) = crate::slash::open(&store, &ui).expect("the palette");
    assert!(rows.iter().any(|(id, _)| id == "slash:agent:review"));
    assert!(!rows.iter().any(|(id, _)| id == "slash:agent:old"));
    // A server that cannot be turned off here shows no switch, and no "Turn on".
    agent(
        &mut store,
        AgentEvent::McpServers(vec![McpServer {
            name: "scratch".into(),
            label: "scratch".into(),
            status: McpStatus::Disabled,
            source: McpSource::Other,
            plugin: None,
            error: None,
            tools: vec![],
            sign_in: false,
        }]),
    );
    ui.mcp = Some(crate::McpDialog::new(1));
    assert!(drawn(&store, &ui, "Off in its config") && !drawn(&store, &ui, "Turn on"));
}

#[test]
fn remote_control_is_offered_by_agents_that_have_it_and_shows_the_link() {
    let (mut store, mut ui) = store_with_session();
    ui.prompt = "/rc".into();
    assert!(!slash_ids(&store, &ui).contains(&"slash:newen:remote:command".into()));
    agent(
        &mut store,
        AgentEvent::Ready {
            version: "2.1.316".into(),
            capabilities: newen_model::AgentCapabilities {
                remote: true,
                ..Default::default()
            },
            mode: None,
        },
    );
    assert_eq!(
        slash_ids(&store, &ui).first().map(String::as_str),
        Some("slash:newen:remote:command")
    );
    // The switch asks the agent, named as the session is, and shows it connecting.
    assert_eq!(
        click(&store, &mut ui, remote::TOGGLE),
        [Intent::Remote {
            session: 1,
            on: true,
            name: Some("Fix the bug".into())
        }]
    );
    assert_eq!(ui.remote_asked, Some(1));
    let on = Event::Agent {
        session: 1,
        request: None,
        event: AgentEvent::Remote {
            on: true,
            url: Some("https://example.invalid/session_1".into()),
            error: None,
        },
    };
    store.apply(on.clone());
    ui.applied(&on);
    assert_eq!(ui.remote_asked, None);
    // The dialog shows the link, which the host draws as a QR code; a later word keeps it.
    assert_eq!(remote::shown_url(&store, &ui), None);
    click(&store, &mut ui, remote::OPEN);
    agent(
        &mut store,
        AgentEvent::Remote {
            on: true,
            url: None,
            error: None,
        },
    );
    assert_eq!(
        remote::shown_url(&store, &ui),
        Some("https://example.invalid/session_1")
    );
    click(&store, &mut ui, "remote:copy");
    assert_eq!(
        ui.files_copy.as_deref(),
        Some("https://example.invalid/session_1")
    );
    assert_eq!(
        click(&store, &mut ui, "remote:off"),
        [Intent::Remote {
            session: 1,
            on: false,
            name: Some("Fix the bug".into())
        }]
    );
    // Why it could not be turned on is the agent's to say.
    agent(
        &mut store,
        AgentEvent::Remote {
            on: false,
            url: None,
            error: Some("not signed in".into()),
        },
    );
    assert_eq!(
        store.session(1).unwrap().remote,
        Remote::Failed("not signed in".into())
    );
    click(&store, &mut ui, "remote:close");
    assert_eq!(ui.remote, None);
    // Settings' new sessions, once the engine told the settings.
    ui.applied(&Event::Settings {
        settings: Settings::default(),
        agents: vec![],
    });
    click(&store, &mut ui, view::SETTINGS_OPEN);
    let saved = click(&store, &mut ui, "set:remote_new");
    assert!(matches!(saved.as_slice(), [Intent::SaveSettings(s)] if s.remote_new_sessions));
}

#[test]
fn a_permission_answered_on_another_device_leaves_no_card() {
    let (mut store, _) = store_with_session();
    agent(
        &mut store,
        AgentEvent::PermissionRequest {
            id: "p1".into(),
            call: Some("t1".into()),
            tool: "Bash".into(),
            input: serde_json::json!({ "command": "touch a" }),
            title: None,
            interactive: false,
            suggestions: vec![],
        },
    );
    agent(&mut store, AgentEvent::Withdrawn { id: "p1".into() });
    let decided = (store.session(1).unwrap().items.iter()).find_map(|i| match i {
        Item::Permission { decided, .. } => Some(*decided),
        _ => None,
    });
    assert_eq!(decided, Some(Some("Withdrawn")));
}

#[test]
fn an_agent_that_pairs_shows_a_code_to_type_on_the_phone() {
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::Ready {
            version: "0.163.0".into(),
            capabilities: newen_model::AgentCapabilities {
                remote: true,
                remote_pairing: true,
                ..Default::default()
            },
            mode: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::Remote {
            on: true,
            url: None,
            error: None,
        },
    );
    click(&store, &mut ui, remote::OPEN);
    assert_eq!(
        click(&store, &mut ui, "remote:pair"),
        [Intent::RemotePair { session: 1 }]
    );
    agent(
        &mut store,
        AgentEvent::RemotePairing {
            code: "ABCD-EFGH".into(),
            expires_at: 1,
        },
    );
    assert_eq!(
        store.session(1).unwrap().remote_code,
        Some(("ABCD-EFGH".into(), 1))
    );
    // Off, the code is gone with it.
    agent(
        &mut store,
        AgentEvent::Remote {
            on: false,
            url: None,
            error: None,
        },
    );
    assert_eq!(store.session(1).unwrap().remote_code, None);
}

#[test]
fn the_agents_default_model_reads_by_name() {
    let (mut store, mut ui) = store_with_session();
    // Haiku is the one the agent runs when none is chosen.
    store.apply(Event::Agent {
        session: 1,
        request: None,
        event: AgentEvent::Ready {
            version: "2.1.316".into(),
            capabilities: newen_model::AgentCapabilities {
                models: vec![
                    newen_model::ModelInfo {
                        id: "opus".into(),
                        name: "Opus 5.5".into(),
                        efforts: vec!["low".into(), "high".into()],
                        ..Default::default()
                    },
                    newen_model::ModelInfo {
                        id: "haiku".into(),
                        name: "Haiku 5.5".into(),
                        efforts: vec!["low".into()],
                        default: true,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            mode: None,
        },
    });
    let window = limn::Size::new(1408.0, 844.0);
    let drawn = |ui: &Ui, words: &str| {
        let frame = UiTree::new(view(&store, ui, window)).layout(window);
        (frame.commands.iter())
            .any(|command| matches!(command, limn::DrawCommand::Text { text, .. } if text == words))
    };
    ui.model = None;
    assert!(drawn(&ui, "Haiku 5.5 (default)"));
    // Its efforts are the composer's, and another model can be chosen.
    assert_eq!(view::efforts(&store, &ui), ["low"]);
    ui.model = Some("opus".into());
    assert!(drawn(&ui, "Opus 5.5"));
    assert!(!drawn(&ui, "Haiku 5.5 (default)"));
}

#[test]
fn a_command_left_running_is_listed_stopped_and_followed_in_a_terminal() {
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::ToolUse {
            call: "c1".into(),
            name: "Bash".into(),
            input: serde_json::json!({ "command": "npm run dev", "run_in_background": true }),
            parent: None,
        },
    );
    agent(
        &mut store,
        AgentEvent::ToolResult {
            call: "c1".into(),
            output: "Command running in background with ID: b1. Output is being written to: /tmp/q/tasks/b1.output".into(),
            is_error: false,
            files: vec![],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    );
    agent(
        &mut store,
        AgentEvent::TaskStarted {
            id: "b1".into(),
            call: Some("c1".into()),
            description: "Start the dev server".into(),
            kind: "command".into(),
        },
    );
    let session = store.session(1).unwrap();
    assert_eq!(
        terminals::output_file(session, "c1"),
        Some(PathBuf::from("/tmp/q/tasks/b1.output"))
    );
    assert_eq!(terminals::rows(session, 200.0).map(|r| r.len()), Some(1));
    // Its output opens in the Terminal pane, in a tab of its own.
    assert_eq!(click(&store, &mut ui, "terminals:open:b1"), []);
    assert!(ui.pane_open(Pane::Terminal));
    assert_eq!(
        ui.terminal.requests,
        [TerminalRequest::Output {
            name: "Start the dev server".into(),
            path: "/tmp/q/tasks/b1.output".into()
        }]
    );
    assert_eq!(
        click(&store, &mut ui, "terminals:stop:b1"),
        [Intent::StopTask {
            session: 1,
            task: "b1".into()
        }]
    );
}

#[test]
fn a_diff_opens_its_files_and_a_large_one_starts_folded() {
    let (mut store, mut ui) = store_with_session();
    let file = |n: usize| {
        newen_model::FileEffect {
        path: format!("src/f{n}.txt"),
        kind: newen_model::ChangeKind::Added,
        added: 1,
        removed: 0,
        // The first is an image: git has no lines for it. The second is a new file too long
        // to show: it keeps its count and has no hunk.
        patch: match n {
            0 => "diff --git a/src/f0.txt b/src/f0.txt\nBinary files /dev/null and b/src/f0.txt differ\n"
                .into(),
            1 => "diff --git a/src/f1.txt b/src/f1.txt\nnew file mode 100644\n".into(),
            _ => format!("@@ -0,0 +1 @@\n+line-{n}\n"),
        },
    }
    };
    for (request, count) in [("r1", 15), ("r2", 301)] {
        store.apply(Event::Changes {
            session: 1,
            request: request.into(),
            files: (0..count).map(file).collect(),
            snapshot: String::new(),
        });
    }
    let window = limn::Size::new(1408.0, 4000.0);
    let frame = |ui: &Ui| UiTree::new(view(&store, ui, window)).layout(window);
    let drawn = |ui: &Ui, words: &str| {
        (frame(ui).commands.iter()).any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    let folds = |ui: &Ui, path: &str| {
        let id = format!("{}{path}", view::DIFF_FOLD);
        frame(ui).regions.iter().any(|r| r.id == id)
    };
    // A small diff opens every file; a binary one says so and has nothing to open.
    click(&store, &mut ui, "changes:r1:view");
    settle(&store, &mut ui);
    assert!(drawn(&ui, "line-2"));
    assert!(drawn(&ui, "line-14"));
    assert!(drawn(&ui, "Binary file"));
    assert!(drawn(&ui, "Too large to show"));
    assert!(folds(&ui, "src/f2.txt"));
    assert!(!folds(&ui, "src/f0.txt") && !folds(&ui, "src/f1.txt"));
    // Over 300 files, every file starts folded and the pane says so; a click opens one.
    click(&store, &mut ui, "changes:r2:view");
    settle(&store, &mut ui);
    assert!(drawn(&ui, "Files are collapsed for large diffs"));
    assert!(!drawn(&ui, "line-2"));
    click(&store, &mut ui, &format!("{}src/f2.txt", view::DIFF_FOLD));
    assert!(drawn(&ui, "line-2"));
}

#[test]
fn each_conversation_keeps_its_own_draft() {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::SessionCreated {
        session: 2,
        agent: "claude".into(),
        cwd: "/work".into(),
    });
    let click = |store: &Store, ui: &mut Ui, id: &str| {
        update(store, ui, &UiAction::Clicked { id: id.into() });
    };
    update(
        &store,
        &mut ui,
        &UiAction::InputChanged {
            id: view::COMPOSER.into(),
            value: "Half a thought".into(),
        },
    );
    ui.images.push("/tmp/shot.png".into());
    click(&store, &mut ui, "session:2");
    assert_eq!(ui.prompt, "");
    assert!(ui.images.is_empty());
    assert!(std::mem::take(&mut ui.draft_switched));
    // The new conversation has its own, too.
    click(&store, &mut ui, view::NEW_SESSION);
    ui.prompt = "A new task".into();
    click(&store, &mut ui, "session:1");
    assert_eq!(ui.prompt, "Half a thought");
    assert_eq!(ui.images, [PathBuf::from("/tmp/shot.png")]);
    click(&store, &mut ui, view::NEW_SESSION);
    assert_eq!(ui.prompt, "A new task");
}

#[test]
fn each_conversation_keeps_its_own_panes() {
    use newen_model::{BrowserTab, BrowserUpdate};
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::SessionCreated {
        session: 2,
        agent: "claude".into(),
        cwd: "/work".into(),
    });
    let click = |store: &Store, ui: &mut Ui, id: &str| {
        update(store, ui, &UiAction::Clicked { id: id.into() });
    };
    click(&store, &mut ui, view::BROWSER_TOGGLE);
    click(&store, &mut ui, "pane:browser:expand");
    ui.helper = Some("call-1".into());
    ui.tick(&store);
    // Another conversation never opened before has none of them, at once, not on springs.
    click(&store, &mut ui, "session:2");
    assert!(ui.panes.is_empty());
    assert_eq!((ui.expanded_pane, ui.helper.as_deref()), (None, None));
    ui.tick(&store);
    assert!(!ui.animating());
    // The agent opening a page in the first doesn't open the browser here; it waits there.
    let tab = |session| Event::Browser {
        session,
        update: BrowserUpdate::Tabs {
            tabs: vec![BrowserTab {
                id: 3,
                ..Default::default()
            }],
            active: Some(3),
        },
    };
    ui.close_pane(Pane::Browser);
    ui.applied(&tab(1));
    assert!(!ui.pane_open(Pane::Browser));
    // The new conversation has its own, too.
    click(&store, &mut ui, view::NEW_SESSION);
    ui.open_pane(Pane::Terminal);
    click(&store, &mut ui, "session:1");
    assert_eq!(ui.panes, [Pane::Browser]);
    assert_eq!(ui.expanded_pane, Some(Pane::Browser));
    assert_eq!(ui.helper.as_deref(), Some("call-1"));
    // A tab that appears in a session not on screen opens its browser for when it shows.
    ui.applied(&tab(2));
    assert_eq!(ui.panes, [Pane::Browser]);
    click(&store, &mut ui, "session:2");
    assert_eq!(ui.panes, [Pane::Browser]);
    // A side chat asked from the first, made while this one shows, opens beside the first.
    ui.side_waiting = Some(1);
    let made = Event::SessionCreated {
        session: 3,
        agent: "claude".into(),
        cwd: "/work".into(),
    };
    ui.applied(&made);
    store.apply(made);
    assert_eq!(
        (ui.panes.as_slice(), ui.side_chat),
        ([Pane::Browser].as_slice(), None)
    );
    click(&store, &mut ui, "session:1");
    assert_eq!(
        (ui.panes.as_slice(), ui.side_chat),
        ([Pane::Browser, Pane::Side].as_slice(), Some(3))
    );
    click(&store, &mut ui, view::NEW_SESSION);
    assert_eq!(ui.panes, [Pane::Terminal]);
    // A deleted session's panes are gone with it.
    ui.applied(&Event::Deleted { session: 1 });
    assert!(!ui.kept_panes.contains_key(&Some(1)));
    assert!(ui.kept_panes.contains_key(&Some(2)));
}

#[test]
fn deleting_the_shown_session_shows_the_new_conversations_own_panes() {
    let (store, mut ui) = store_with_session();
    update(
        &store,
        &mut ui,
        &UiAction::Clicked {
            id: view::BROWSER_TOGGLE.into(),
        },
    );
    ui.applied(&Event::Deleted { session: 1 });
    assert_eq!(ui.selected, None);
    assert!(ui.panes.is_empty());
    assert!(!ui.kept_panes.contains_key(&Some(1)));
}

/// Two sessions working at once keep their places while they stream; a turn ending, a
/// question or a message sent moves that one up.
#[test]
fn streaming_sessions_keep_their_order_until_something_happens() {
    let mut store = Store::default();
    for session in [1, 2] {
        store.apply(Event::SessionCreated {
            session,
            agent: "claude".into(),
            cwd: "/work".into(),
        });
        store.apply(Event::Queued {
            session,
            request: format!("r{session}"),
            text: "Go".into(),
            at: None,
            images: vec![],
        });
    }
    let ui = Ui::new("/work".into());
    let say = |store: &mut Store, session, event| {
        store.apply(Event::Agent {
            session,
            request: Some(format!("r{session}")),
            event,
        })
    };
    assert_eq!(view::order(&store, &ui), [2, 1]);
    for round in 0..3 {
        for session in [1, 2] {
            say(
                &mut store,
                session,
                AgentEvent::TextDelta {
                    text: "word ".into(),
                    parent: None,
                },
            );
            say(
                &mut store,
                session,
                AgentEvent::ToolUse {
                    call: format!("c{round}"),
                    name: "Read".into(),
                    input: serde_json::json!({}),
                    parent: None,
                },
            );
            assert_eq!(view::order(&store, &ui), [2, 1]);
        }
    }
    let finished = AgentEvent::TurnFinished {
        ok: true,
        interrupted: false,
        usage: Usage::default(),
        error: None,
    };
    say(&mut store, 1, finished.clone());
    assert_eq!(view::order(&store, &ui), [1, 2]);
    say(
        &mut store,
        2,
        AgentEvent::PermissionRequest {
            id: "p1".into(),
            call: None,
            tool: "Bash".into(),
            input: serde_json::json!({}),
            title: None,
            interactive: false,
            suggestions: vec![],
        },
    );
    assert_eq!(view::order(&store, &ui), [2, 1]);
    store.apply(Event::Queued {
        session: 1,
        request: "r3".into(),
        text: "And this".into(),
        at: None,
        images: vec![],
    });
    assert_eq!(view::order(&store, &ui), [1, 2]);
}

#[test]
fn the_sidebar_keeps_its_order_while_the_pointer_is_over_it() {
    let mut store = Store::default();
    for session in [1, 2] {
        store.apply(Event::SessionCreated {
            session,
            agent: "claude".into(),
            cwd: "/work".into(),
        });
    }
    let mut ui = Ui::new("/work".into());
    assert_eq!(view::order(&store, &ui), [2, 1]);
    // Over the list, a turn of the lower session leaves it where it is.
    ui.pointer = Some(limn::Point::new(100.0, 300.0));
    ui.tick(&store);
    store.apply(Event::Queued {
        session: 1,
        request: "r1".into(),
        text: "Go on".into(),
        at: None,
        images: vec![],
    });
    ui.tick(&store);
    assert_eq!(view::order(&store, &ui), [2, 1]);
    // A sort the person picks applies at once.
    ui.sort_by = SortBy::Created;
    ui.ascending = true;
    ui.tick(&store);
    assert_eq!(view::order(&store, &ui), [1, 2]);
    ui.sort_by = SortBy::Updated;
    ui.ascending = false;
    ui.tick(&store);
    assert_eq!(view::order(&store, &ui), [1, 2]);
    store.apply(Event::Queued {
        session: 2,
        request: "r2".into(),
        text: "And this".into(),
        at: None,
        images: vec![],
    });
    ui.tick(&store);
    assert_eq!(view::order(&store, &ui), [1, 2]);
    // Once the pointer leaves, the list takes the order of its activity.
    ui.pointer = Some(limn::Point::new(800.0, 300.0));
    ui.tick(&store);
    assert_eq!(view::order(&store, &ui), [2, 1]);
}

#[test]
fn sessions_started_by_sessions_nest_under_them_as_a_tree_at_every_depth() {
    let mut store = Store::default();
    for session in 1..=5 {
        store.apply(Event::SessionCreated {
            session,
            agent: "claude".into(),
            cwd: "/work".into(),
        });
    }
    // 1 started 2 and 4, 2 started 3; 5 was started by 4, which was put away.
    for (session, parent) in [(2, 1), (3, 2), (4, 1), (5, 4)] {
        store.apply(Event::Linked {
            session,
            parent,
            side: false,
        });
    }
    store.apply(Event::Archived {
        session: 4,
        archived: true,
    });
    let ui = Ui::new("/work".into());
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let rows: Vec<(&str, limn::Rect)> = (frame.regions.iter())
        .filter(|r| r.id.starts_with("session:"))
        .map(|r| (r.id.as_str(), r.rect))
        .collect();
    let ids: Vec<&str> = rows.iter().map(|r| r.0).collect();
    assert_eq!(ids, ["session:5", "session:1", "session:2", "session:3"]);
    let x = |id: &str| rows.iter().find(|r| r.0 == id).unwrap().1.x;
    assert!(x("session:1") < x("session:2") && x("session:2") < x("session:3"));
    assert_eq!(x("session:5"), x("session:1"));
}

/// A turn with every kind of transcript row, its words long enough to run out of any column.
fn crowded_turn() -> (Store, Ui) {
    let (mut store, mut ui) = store_with_session();
    let file = |name: &str| serde_json::json!({ "file_path": format!("/work/{name}") });
    tool(
        &mut store,
        "r1",
        "Read",
        file("assets/rune-discord-banner.png"),
        None,
    );
    result(&mut store, "r1", "ok");
    for i in 0..4 {
        let call = format!("b{i}");
        let input = serde_json::json!({
            "command": "ls",
            "description": format!("List the generated icon folders under assets/icons/rune-{i}"),
        });
        tool(&mut store, &call, "Bash", input, None);
        result(&mut store, &call, "ok");
    }
    // What the person has not seen begins between two groups of steps.
    let new_from = store.session(1).unwrap().items.len();
    for name in [
        "crates/server/src/uploads.rs",
        "assets/rune-small.svg",
        "assets/rune-windows.svg",
        "assets/rune.svg",
        "AGENTS.md",
        "crates/server/src/routes/module.rs",
    ] {
        let call = format!("e:{name}");
        let mut input = file(name);
        input["old_string"] = "a".into();
        input["new_string"] = "b".into();
        tool(&mut store, &call, "Edit", input, None);
        result(&mut store, &call, "ok");
    }
    let long = "/work/crates/server/src/routes/very/deep/module/with/a/path/that/never/ends.rs";
    agent(
        &mut store,
        AgentEvent::Text {
            text: format!(
                "Done: see {long} and https://example.com/a/very/long/url/that/has/no/spaces/at/all                  and `{long}`.\n\n```\nlet path = \"{long}{long}\";\n```"
            ),
            parent: None,
            id: None,
            at: None,
        },
    );
    let failing = serde_json::json!({
        "command": "false",
        "description": "Run a command that checks every generated icon in the repository for its size",
    });
    tool(&mut store, "f1", "Bash", failing, None);
    agent(
        &mut store,
        AgentEvent::ToolResult {
            call: "f1".into(),
            output: "boom".into(),
            is_error: true,
            files: vec![],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    );
    let query = "how do the tab and footer string keys of every crate in the workspace load";
    tool(
        &mut store,
        "w1",
        "WebSearch",
        serde_json::json!({ "query": query }),
        None,
    );
    let subject = "Find usages of tab and footer string keys in every crate of the workspace";
    tool(
        &mut store,
        "c1",
        "TaskCreate",
        serde_json::json!({ "subject": subject }),
        None,
    );
    result(&mut store, "c1", "Task #1 created successfully: x");
    for i in 0..3 {
        agent(
            &mut store,
            AgentEvent::TaskStarted {
                id: format!("k{i}"),
                call: Some(format!("a{i}")),
                description: "Find usages of tab and footer string keys".into(),
                kind: "agent".into(),
            },
        );
    }
    // A page the agent opened: its card, with "Open" at its end.
    use newen_model::{BrowserTab, BrowserUpdate};
    let url = "https://code.claude.com/docs/en/overview";
    store.apply(Event::Browser {
        session: 1,
        update: BrowserUpdate::Tabs {
            tabs: vec![BrowserTab {
                id: 7,
                url: url.into(),
                title: "Overview - Claude Code Docs".into(),
                ..Default::default()
            }],
            active: Some(7),
        },
    });
    tool(
        &mut store,
        "n1",
        "mcp__browser__navigate",
        serde_json::json!({ "url": url }),
        None,
    );
    let card = serde_json::json!({ "type": "browser", "tabId": 7, "url": url });
    result(&mut store, "n1", &card.to_string());
    // Two helpers at once: the first in a group, the last on the running line.
    for call in ["a8", "a9"] {
        let description = "REA: voice preview + timer, and a long account of what it does";
        tool(
            &mut store,
            call,
            "Agent",
            serde_json::json!({ "description": description }),
            None,
        );
    }
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Running,
    });
    ui.new_from.insert(1, new_from);
    (store, ui)
}

/// Each line of text drawn, as far as its clip lets it show: its words, left, top, right and
/// height. The tree measures text as 8 px a character at 14 px when no fonts are attached.
fn text_spans(frame: &limn::Frame) -> Vec<(String, f32, f32, f32, f32)> {
    let mut clips: Vec<limn::Rect> = vec![];
    let mut spans = vec![];
    for command in &frame.commands {
        match command {
            limn::DrawCommand::PushClip(rect) => {
                let inner = match clips.last() {
                    Some(outer) => rect
                        .intersection(*outer)
                        .unwrap_or(limn::Rect::new(rect.x, rect.y, 0.0, 0.0)),
                    None => *rect,
                };
                clips.push(inner);
            }
            limn::DrawCommand::PopClip => {
                clips.pop();
            }
            limn::DrawCommand::Text {
                position,
                text,
                font_size,
                ..
            } => {
                let scale = font_size / 14.0;
                let mut right = position.x + text.chars().count() as f32 * 8.0 * scale;
                if let Some(clip) = clips.last() {
                    if clip.width <= 0.0 {
                        continue;
                    }
                    right = right.min(clip.x + clip.width);
                }
                spans.push((text.clone(), position.x, position.y, right, 18.0 * scale));
            }
            _ => {}
        }
    }
    spans
}

/// In a narrow center dock (360 px) as in wide ones, every line of the transcript keeps to
/// its column, and none is drawn over another or crossed by a hairline: the step summaries
/// and steps, a long query, the task list, the running line and the "New" line between two
/// groups. A paragraph's long path and URL wrap inside it.
#[test]
fn transcript_rows_keep_to_their_column_at_any_width() {
    let (store, mut ui) = crowded_turn();
    // The group with the running helper open, so its rows are drawn too.
    ui.expanded.push("steps:a8".into());
    ui.expanded.push("steps:r1".into());
    settle(&store, &mut ui);
    for width in [648.0, 800.0, 1000.0, 1600.0, 2400.0] {
        let window = limn::Size::new(width, 2000.0);
        let frame = UiTree::new(view(&store, &ui, window)).layout(window);
        let dock = frame
            .regions
            .iter()
            .find(|r| r.id == "transcript:1")
            .unwrap()
            .rect;
        let spans: Vec<_> = text_spans(&frame)
            .into_iter()
            .filter(|s| s.1 >= dock.x && s.1 < dock.x + dock.width)
            .filter(|s| s.2 >= dock.y && s.2 < dock.y + dock.height)
            .collect();
        let left = spans
            .iter()
            .find(|s| s.0.starts_with("Started on"))
            .unwrap()
            .1;
        // The column is centered in the dock.
        let right = dock.x + dock.width - (left - dock.x);
        if width == 648.0 {
            assert_eq!(right - left, 264.0, "a 360 px dock");
        }
        if std::env::var_os("SPANS").is_some() {
            for s in &spans {
                eprintln!("{s:?}");
            }
        }
        for drawn in [
            "New",
            "Running agent",
            "Failed",
            "Find usages",
            "Searching",
            "Open",
        ] {
            assert!(
                spans.iter().any(|s| s.0.starts_with(drawn)),
                "{width}: no {drawn}"
            );
        }
        for s in &spans {
            assert!(s.3 <= right + 0.5, "{width}: {s:?} runs past {right}");
        }
        // The browser card's "Open" keeps inside the card, and the card to the column.
        let open = region(&frame, "browser:card:7").expect("the card's Open");
        assert!(
            open.x + open.width <= right - 12.0 + 0.5,
            "{width}: {open:?}"
        );
        for (i, a) in spans.iter().enumerate() {
            for b in &spans[i + 1..] {
                // A fading or shimmering line is drawn as bands of the same words.
                if a.0 == b.0 && a.2 == b.2 {
                    continue;
                }
                let across = a.1 < b.3 - 0.5 && b.1 < a.3 - 0.5;
                let down = a.2 < b.2 + b.4 - 0.5 && b.2 < a.2 + a.4 - 0.5;
                assert!(!(across && down), "{width}: {a:?} over {b:?}");
            }
        }
        for command in &frame.commands {
            let limn::DrawCommand::FillRect { rect, .. } = command else {
                continue;
            };
            if rect.height > 1.5 || rect.x < dock.x || rect.x >= dock.x + dock.width {
                continue;
            }
            for s in &spans {
                let across = rect.x < s.3 && s.1 < rect.x + rect.width;
                let down = rect.y > s.2 && rect.y < s.2 + s.4;
                assert!(
                    !(across && down),
                    "{width}: a hairline at {rect:?} crosses {s:?}"
                );
            }
        }
    }
}

/// The composer's ring fills as the Context card says the context is, for a 200k window and
/// a 1M one, and its hint gives the same share and figures.
#[test]
fn the_composer_s_ring_fills_as_the_context_card_says() {
    for (used, window, card) in [
        (512_900, 1_000_000, "51% used"),
        (51_000, 200_000, "26% used"),
    ] {
        let (mut store, mut ui) = store_with_session();
        store.sessions[0].context_tokens = used;
        store.sessions[0].context_window = window;
        settle(&store, &mut ui);
        let window_size = limn::Size::new(2000.0, 900.0);
        let mut tree = UiTree::new(view(&store, &ui, window_size));
        let frame = tree.layout(window_size);
        assert!(painted(&frame, card).is_some(), "the card says {card}");
        let anchor = region(&frame, view::USAGE_MENU).expect("the ring");
        let ring = frame.commands.iter().find_map(|command| match command {
            limn::DrawCommand::ProgressRing { rect, fraction, .. }
                if anchor.contains(limn::Point::new(rect.x + 1.0, rect.y + 1.0)) =>
            {
                Some(*fraction)
            }
            _ => None,
        });
        let fraction = ring.flatten().expect("the ring's arc");
        assert!(
            (fraction - used as f32 / window as f32).abs() < 1e-4,
            "{window}: {fraction}"
        );
        assert_eq!(format!("{}% used", (fraction * 100.0).round() as u32), card);
        // Its hint, once the pointer rests on it.
        let at = limn::Point::new(
            anchor.x + anchor.width / 2.0,
            anchor.y + anchor.height / 2.0,
        );
        tree.handle_event(limn::UiEvent::PointerMove(at));
        let start = std::time::Instant::now();
        tree.layout_at(window_size, start);
        let frame = tree.layout_at(window_size, start + std::time::Duration::from_millis(900));
        let share = card.trim_end_matches(" used");
        let hint = format!(
            "Context {share} \u{b7} {} / {}",
            view::compact(used),
            view::compact(window)
        );
        assert!(painted(&frame, &hint).is_some(), "{hint}");
    }
}

/// A background task's title, in the card and in its pane, is drawn in the face and size of
/// the right panel's other rows, not the monospace an element has of its own.
#[test]
fn a_background_task_s_title_is_in_the_panel_s_face() {
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::TaskStarted {
            id: "t1".into(),
            call: None,
            description: "REA Discord stream".into(),
            kind: "command".into(),
        },
    );
    settle(&store, &mut ui);
    let window = limn::Size::new(2000.0, 900.0);
    let face = |frame: &limn::Frame, words: &str| {
        frame.commands.iter().find_map(|command| match command {
            limn::DrawCommand::Text {
                text,
                font_family,
                font_size,
                ..
            } if text == words => Some((*font_family, *font_size)),
            _ => None,
        })
    };
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let row = face(&frame, "This computer").expect("the Environment row");
    assert_eq!(row, (limn::FontFamily::Sans, look::BODY));
    assert_eq!(face(&frame, "REA Discord stream"), Some(row), "the card's");
    click(&store, &mut ui, view::TASKS_OPEN);
    settle(&store, &mut ui);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert_eq!(face(&frame, "REA Discord stream"), Some(row), "the pane's");
}

/// A running task's Stop, in the Background tasks card and in its pane, is the composer's:
/// the same 28 px button with the same square, and the card's row lights as a sidebar row
/// does, as tall and as round.
#[test]
fn a_task_s_stop_is_the_composer_s_and_its_row_lights_as_the_sidebar_s() {
    let (mut store, mut ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::TaskStarted {
            id: "t1".into(),
            call: None,
            description: "REA Discord stream".into(),
            kind: "command".into(),
        },
    );
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Running,
    });
    settle(&store, &mut ui);
    let window = limn::Size::new(2000.0, 900.0);
    // Each Stop's size and the icon drawn in it.
    let stop = |frame: &limn::Frame, id: &str| {
        let rect = region(frame, id).unwrap_or_else(|| panic!("no {id}"));
        let inside = |at: &limn::Rect| {
            rect.contains(limn::Point::new(
                at.x + at.width / 2.0,
                at.y + at.height / 2.0,
            ))
        };
        let icon = frame.commands.iter().find_map(|command| match command {
            limn::DrawCommand::Icon {
                rect: at,
                icon,
                tint,
            } if inside(at) => Some((at.width, at.height, *icon, *tint)),
            _ => None,
        });
        ((rect.width, rect.height), icon.expect("its square"))
    };
    let mut tree = UiTree::new(view(&store, &ui, window));
    tree.set_reduced_motion(true);
    let frame = tree.layout(window);
    let composer = stop(&frame, view::STOP);
    assert_eq!(composer.0, (28.0, 28.0));
    assert_eq!(stop(&frame, "terminals:stop:t1"), composer, "the card's");
    // Under the pointer, the card's row and the sidebar's session row light the same.
    let mut lit = |id: &str| {
        let frame = tree.layout(window);
        let row = region(&frame, id).unwrap_or_else(|| panic!("no {id}"));
        let at = limn::Point::new(row.x + 4.0, row.y + row.height / 2.0);
        tree.handle_event(limn::UiEvent::PointerMove(at));
        let frame = tree.layout(window);
        let fill = frame.commands.iter().find_map(|command| match command {
            limn::DrawCommand::FillRect {
                rect,
                color,
                radius,
            } if [look::row_hover(), look::selected_hover()].contains(color)
                && rect.contains(at) =>
            {
                Some((rect.height, *radius, rect.x - row.x))
            }
            _ => None,
        });
        fill.unwrap_or_else(|| {
            let fills: Vec<_> = frame
                .commands
                .iter()
                .filter(
                    |c| matches!(c, limn::DrawCommand::FillRect { rect, .. } if rect.contains(at)),
                )
                .collect();
            panic!("{id} at {row:?} is not lit: {fills:?}")
        })
    };
    let sidebar = lit("session:1");
    assert_eq!(sidebar.0, 32.0);
    assert_eq!(lit("terminals:open:t1"), sidebar);
    click(&store, &mut ui, view::TASKS_OPEN);
    settle(&store, &mut ui);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert_eq!(stop(&frame, "task:stop:t1"), composer, "the pane's");
}

/// A background task's row keeps its description on one line that fades out inside the
/// card; resting on it shows the whole of it with the command, how long it has run and where.
#[test]
fn a_background_task_row_fades_its_long_title_and_its_hint_has_the_rest() {
    let (mut store, mut ui) = store_with_session();
    let description = "Find usages of tab and footer string keys in every crate of the workspace";
    let input = serde_json::json!({ "command": "rg -n 'tab_key|footer_key' crates", "description": description });
    tool(&mut store, "c1", "Bash", input, None);
    agent(
        &mut store,
        AgentEvent::TaskStarted {
            id: "t1".into(),
            call: Some("c1".into()),
            description: description.into(),
            kind: "command".into(),
        },
    );
    settle(&store, &mut ui);
    let window = limn::Size::new(1600.0, 900.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    let start = std::time::Instant::now();
    let frame = tree.layout_at(window, start);
    let row = region(&frame, "terminals:open:t1").expect("the task's row");
    let bands: Vec<_> = text_spans(&frame)
        .into_iter()
        .filter(|s| s.0 == description)
        .collect();
    assert!(
        bands.len() > 1,
        "it fades rather than ending in an ellipsis"
    );
    assert!(bands
        .iter()
        .all(|s| s.2 == bands[0].2 && s.3 <= row.x + row.width));
    assert!(painted(&frame, "rg -n 'tab_key|footer_key' crates").is_none());
    let center = limn::Point::new(row.x + row.width / 2.0, row.y + row.height / 2.0);
    tree.handle_event(limn::UiEvent::PointerMove(center));
    tree.layout_at(window, start);
    let frame = tree.layout_at(window, start + std::time::Duration::from_millis(600));
    let hint = |words: &str| {
        frame.commands.iter().any(|command| {
            matches!(command, limn::DrawCommand::Text { text, .. } if text.contains(words))
        })
    };
    assert!(hint("rg -n 'tab_key|footer_key' crates"), "its command");
    assert!(
        hint("Command \u{b7} running for"),
        "what it is and how long it ran"
    );
    assert!(hint("/work"), "its folder");
}

/// A helper that still runs keeps its shimmer on its own row, in an open group as on the
/// running line: helpers launched together leave all but the last in a group.
#[test]
fn a_running_helper_shimmers_in_its_open_group() {
    let (mut store, mut ui) = store_with_session();
    for call in ["a1", "a2"] {
        let input = serde_json::json!({ "description": "Look into the voice preview" });
        tool(&mut store, call, "Agent", input, None);
    }
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Running,
    });
    ui.expanded.push("steps:a1".into());
    settle(&store, &mut ui);
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    let start = std::time::Instant::now();
    tree.layout_at(window, start);
    let frame = tree.layout_at(window, start + std::time::Duration::from_millis(500));
    let drawn: Vec<(f32, limn::Color)> = (frame.commands.iter())
        .filter_map(|command| match command {
            limn::DrawCommand::Text {
                text,
                color,
                position,
                ..
            } if text.ends_with("Look into the voice preview") => Some((position.y, *color)),
            _ => None,
        })
        .collect();
    // The group's row is above the running line; its words sweep a highlight as those do.
    let top = drawn.iter().map(|d| d.0).fold(f32::INFINITY, f32::min);
    let mut inks = vec![];
    for (_, color) in drawn.iter().filter(|d| d.0 == top) {
        if !inks.contains(color) {
            inks.push(*color);
        }
    }
    assert!(inks.len() > 1, "{drawn:?}");
}

const LONG_TITLE: &str =
    "hola como estas, implement the release notes and every change the release needs";

/// Session 1 (Claude) waits for session 2 (Codex), which works on a long-titled task.
fn waiting_store() -> (Store, Ui) {
    let (mut store, mut ui) = store_with_session();
    store.apply(Event::SessionCreated {
        session: 2,
        agent: "codex".into(),
        cwd: "/work".into(),
    });
    store.apply(Event::Queued {
        session: 2,
        request: "q2".into(),
        text: LONG_TITLE.into(),
        at: None,
        images: vec![],
    });
    store.apply(Event::Status {
        session: 2,
        status: SessionStatus::Running,
    });
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Idle,
    });
    store.apply(Event::Waiting {
        session: 1,
        wait: newen_model::Wait {
            on: 2,
            title: "hola".into(),
            then: "Cut version 0.1.0".into(),
            at: 0,
        },
    });
    ui.agents = vec![newen_model::AgentInfo {
        id: "codex".into(),
        name: "Codex".into(),
        tested: vec![],
        options: vec![],
        icon: None,
        sign_in_methods: vec![],
        sign_in_code: false,
        modes: vec![],
        default_mode: "auto".into(),
        customize: Default::default(),
    }];
    settle(&store, &mut ui);
    (store, ui)
}

/// Under the turns, "Waiting for «…» · Codex" with the agent's name from its list: it sweeps a
/// highlight while the other session works, and in a narrow column fades out before Cancel.
#[test]
fn a_wait_shimmers_under_the_turns_and_fades_before_its_cancel() {
    let (store, ui) = waiting_store();
    let words = "Waiting for \u{ab}hola como estas";
    for width in [648.0, 1408.0] {
        let window = limn::Size::new(width, 844.0);
        let mut tree = UiTree::new(view(&store, &ui, window));
        let start = std::time::Instant::now();
        tree.layout_at(window, start);
        let frame = tree.layout_at(window, start + std::time::Duration::from_millis(500));
        let cancel = region(&frame, "wait-cancel:1:2").expect("its Cancel");
        let bands: Vec<(f32, f32, limn::Color)> = (frame.commands.iter())
            .filter_map(|command| match command {
                limn::DrawCommand::Text {
                    text,
                    color,
                    position,
                    ..
                } if text.starts_with(words) => Some((position.x, position.y, *color)),
                _ => None,
            })
            .collect();
        assert!(!bands.is_empty(), "{width}: no wait row");
        let mut inks = vec![];
        for band in &bands {
            if !inks.contains(&band.2) {
                inks.push(band.2);
            }
        }
        assert!(inks.len() > 1, "{width}: it shimmers");
        let spans: Vec<_> = (text_spans(&frame).into_iter())
            .filter(|s| s.0.starts_with(words))
            .collect();
        assert!(spans.iter().all(|s| s.3 <= cancel.x), "{width}: {spans:?}");
        if width == 1408.0 {
            assert!(spans.iter().any(|s| s.0.ends_with("\u{bb} \u{b7} Codex")));
        }
    }
}

/// Cancel asks the engine to end the wait; while the other session asks the person, the row
/// says so; once the wait ends the row goes.
#[test]
fn a_wait_s_cancel_sends_its_intent_and_the_row_follows_the_other_session() {
    let (mut store, mut ui) = waiting_store();
    assert_eq!(
        click(&store, &mut ui, "wait-cancel:1:2"),
        [Intent::CancelWait { session: 1, on: 2 }]
    );
    store.apply(Event::Status {
        session: 2,
        status: SessionStatus::Awaiting,
    });
    assert!(drawn(&store, &ui, "\u{bb} is asking you something"));
    store.apply(Event::WaitEnded {
        session: 1,
        on: 2,
        outcome: newen_model::WaitOutcome::Canceled,
        text: String::new(),
    });
    assert!(store.session(1).unwrap().waits.is_empty());
    assert!(!drawn(&store, &ui, "is asking you something"));
    // A wait that ended with the other's task done is heard as such.
    store.apply(Event::Reported {
        session: 1,
        request: "w1".into(),
        reports: vec![newen_model::Report {
            from: 2,
            title: "hola".into(),
            text: "Merged.".into(),
            waited: Some((newen_model::WaitOutcome::Finished, "Cut 0.1.0".into())),
        }],
    });
    assert!(drawn(&store, &ui, "Waited for"));
}

/// In the sidebar a waiting session nests under the one it waits for, on the tree's guide, and
/// ends in an hourglass; once the wait ends it is a row of its own again.
#[test]
fn a_waiting_session_nests_under_the_one_it_waits_for_with_an_hourglass() {
    let (mut store, ui) = waiting_store();
    let rows = |store: &Store| {
        let window = limn::Size::new(1408.0, 844.0);
        let frame = UiTree::new(view(store, &ui, window)).layout(window);
        let rows: Vec<(String, limn::Rect)> = (frame.regions.iter())
            .filter(|r| r.id.starts_with("session:"))
            .map(|r| (r.id.clone(), r.rect))
            .collect();
        let hourglass = (frame.commands.iter()).find_map(|command| match command {
            limn::DrawCommand::Icon {
                icon: limn::Icon::Custom(svg),
                rect,
                ..
            } if *svg == crate::icons::HOURGLASS => Some(*rect),
            _ => None,
        });
        (rows, hourglass)
    };
    let (listed, hourglass) = rows(&store);
    let ids: Vec<&str> = listed.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(ids, ["session:2", "session:1"]);
    assert!(listed[1].1.x > listed[0].1.x, "nested");
    let mark = hourglass.expect("the waiting mark");
    assert!(mark.y >= listed[1].1.y && mark.y < listed[1].1.y + listed[1].1.height);
    store.apply(Event::WaitEnded {
        session: 1,
        on: 2,
        outcome: newen_model::WaitOutcome::Finished,
        text: "Merged.".into(),
    });
    let (listed, hourglass) = rows(&store);
    assert_eq!(listed[0].1.x, listed[1].1.x);
    assert!(hourglass.is_none());
}

/// A running helper's Stop sits at its row's right edge, clear of the row's words, which
/// fade before it; a running command's the same.
#[test]
fn a_running_tasks_stop_sits_at_the_right_edge_clear_of_its_words() {
    let (mut store, mut ui) = store_with_session();
    for (id, kind, description) in [
        ("t1", "agent", "Extract Discord icons for each glyph"),
        ("t2", "command", "Watch the build"),
    ] {
        agent(
            &mut store,
            AgentEvent::TaskStarted {
                id: id.into(),
                call: Some(format!("c{id}")),
                description: description.into(),
                kind: kind.into(),
            },
        );
        agent(
            &mut store,
            AgentEvent::TaskProgress {
                id: id.into(),
                usage: newen_model::TaskUsage {
                    tokens: 134_500,
                    tool_uses: 25,
                    duration_ms: 484_000,
                },
            },
        );
    }
    // More than the pane holds, so its list scrolls.
    for i in 0..16 {
        agent(
            &mut store,
            AgentEvent::TaskStarted {
                id: format!("more{i}"),
                call: None,
                description: format!("Watch the build {i}"),
                kind: "command".into(),
            },
        );
    }
    click(&store, &mut ui, view::TASKS_OPEN);
    settle(&store, &mut ui);
    for width in [1408.0, 2000.0, 2600.0] {
        let window = limn::Size::new(width, 900.0);
        let frame = UiTree::new(view(&store, &ui, window)).layout(window);
        let rect = |id: &str| frame.regions.iter().find(|r| r.id == id).unwrap().rect;
        let pane = rect("tasks:scroll");
        let last = text_spans(&frame)
            .into_iter()
            .find(|s| s.0 == "Watch the build 15");
        assert!(
            last.is_none_or(|s| s.2 > pane.y + pane.height),
            "{width}: the list scrolls"
        );
        for (id, words) in [("t1", "Extract Discord"), ("t2", "Watch the build")] {
            let stop = rect(&format!("task:stop:{id}"));
            assert!(
                (stop.x + stop.width - (pane.x + pane.width - 16.0)).abs() < 1.0,
                "{width}: {id}'s Stop at {stop:?} in {pane:?}"
            );
            let spans = text_spans(&frame);
            let title = spans.iter().find(|s| s.0.starts_with(words)).unwrap();
            for s in spans
                .iter()
                .filter(|s| s.2 >= title.2 && s.2 < title.2 + 40.0)
            {
                assert!(s.3 <= stop.x + 0.5, "{width}: {s:?} runs under {stop:?}");
            }
        }
    }
}

/// A running helper's transcript, opened under Background tasks in a narrow pane: a command
/// whose output is one long listing with no spaces, then a paragraph, then another step.
#[test]
fn a_helpers_transcript_keeps_its_rows_apart_in_a_narrow_pane() {
    let (mut store, mut ui) = store_with_session();
    let description = "Extract Discord icons for each glyph";
    tool(
        &mut store,
        "h1",
        "Agent",
        serde_json::json!({ "description": description, "prompt": "Extract the icons." }),
        None,
    );
    agent(
        &mut store,
        AgentEvent::TaskStarted {
            id: "k1".into(),
            call: Some("h1".into()),
            description: description.into(),
            kind: "agent".into(),
        },
    );
    let names: String = (0..60).map(|i| format!("Icon{i}Neutral")).collect();
    let listing = format!(
        "{names}YoutubeNeutralIcon\n/run/current-system/sw/bin/convert\nModuleNotFoundError: No module named 'cairosvg'\n{}",
        (0..40).map(|i| format!("line {i}\n")).collect::<String>()
    );
    let bash =
        serde_json::json!({ "command": "python3 extract.py", "description": "List the icons" });
    tool(&mut store, "hb1", "Bash", bash, Some("h1"));
    result(&mut store, "hb1", &listing);
    agent(
        &mut store,
        AgentEvent::Text {
            text: "ImageMagick is available for contact-sheet checks. Next I'll look at the raw module source for `fillRule`, secondary colours, and local-function icons.".into(),
            parent: Some("h1".into()),
            id: None,
            at: None,
        },
    );
    let read = serde_json::json!({ "file_path": "/work/discord/icons.js" });
    tool(&mut store, "hr1", "Read", read, Some("h1"));
    result(&mut store, "hr1", "export const a = 1;");
    let long = "a-path/that/never/ends/".repeat(12);
    let edit = serde_json::json!({ "file_path": "/work/extract.py", "old_string": "a", "new_string": "b" });
    tool(&mut store, "he1", "Edit", edit, Some("h1"));
    agent(
        &mut store,
        AgentEvent::ToolResult {
            call: "he1".into(),
            output: "ok".into(),
            is_error: false,
            files: vec![newen_model::FileChange {
                path: "/work/extract.py".into(),
                before: Some("import os\n".into()),
                after: Some(format!("import os\nprint(os.listdir('{long}'))\n")),
            }],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    );
    click(&store, &mut ui, view::TASKS_OPEN);
    click(&store, &mut ui, "agent:h1");
    settle(&store, &mut ui);
    for open in [false, true] {
        if open {
            for key in ["steps:hb1", "hb1", "steps:hr1", "hr1", "he1"] {
                ui.expanded.push(key.into());
            }
            settle(&store, &mut ui);
        }
        for width in [1408.0, 2000.0, 2600.0] {
            let window = limn::Size::new(width, 1200.0);
            let at = format!("{open} {width}");
            let seen = assert_rows_apart(&store, &ui, window, "agent:h1:scroll", &at);
            let mut words = vec!["ImageMagick", "icons.js"];
            if open {
                words.extend(["ModuleNotFoundError", "line 39", "listdir"]);
            }
            for words in words {
                assert!(seen.iter().any(|s| s.contains(words)), "{at}: no {words}");
            }
        }
    }
}

/// [`text_spans`], and the lines the tree lays out itself and draws as glyphs.
fn drawn_spans(frame: &limn::Frame) -> Vec<(String, f32, f32, f32, f32)> {
    let mut spans = text_spans(frame);
    for block in frame.text_layouts() {
        for run in block.runs {
            // Text painted by the backend has a run of its own for selection.
            let at = run.rect;
            let painted = |s: &(String, f32, f32, f32, f32)| {
                s.0 == run.text && (s.1 - at.x).abs() < 0.5 && (s.2 - at.y).abs() < 4.0
            };
            let r = run.visible.unwrap_or(at);
            if r.width > 0.0 && r.height > 0.0 && !spans.iter().any(painted) {
                spans.push((run.text, r.x, r.y, r.x + r.width, r.height));
            }
        }
    }
    spans
}

/// No line drawn over another; a fading or shimmering line is drawn as bands of the same words.
fn assert_apart(spans: &[(String, f32, f32, f32, f32)], at: &str) {
    for (i, a) in spans.iter().enumerate() {
        for b in &spans[i + 1..] {
            if a.0 == b.0 && a.2 == b.2 {
                continue;
            }
            let across = a.1 < b.3 - 0.5 && b.1 < a.3 - 0.5;
            let down = a.2 < b.2 + b.4 - 0.5 && b.2 < a.2 + a.4 - 0.5;
            assert!(!(across && down), "{at}: {a:?} over {b:?}");
        }
    }
}

/// A helper's transcript opens at its end and follows what it says while the person stays
/// there; scrolled up, it holds still, and back at the end it follows again. The app keeps
/// the scroll area `helper_scroll` names at its end as it does the session's.
#[test]
fn a_helpers_transcript_follows_its_end_while_the_person_is_there() {
    let (mut store, mut ui) = store_with_session();
    tool(
        &mut store,
        "h1",
        "Agent",
        serde_json::json!({ "description": "Extract icons" }),
        None,
    );
    let mut said = 0;
    let mut say = |store: &mut Store| {
        for _ in 0..12 {
            said += 1;
            agent(
                store,
                AgentEvent::Text {
                    text: format!("Step {said}: looked at another module of icons."),
                    parent: Some("h1".into()),
                    id: Some(format!("m{said}")),
                    at: None,
                },
            );
        }
    };
    say(&mut store);
    assert_eq!(ui.helper_scroll(), None);
    click(&store, &mut ui, "agent:h1");
    settle(&store, &mut ui);
    let scroll = ui.helper_scroll().unwrap();
    let window = limn::Size::new(1408.0, 900.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    tree.set_reduced_motion(true);
    // The frame it opens on goes to the end.
    tree.layout(window);
    tree.scroll_to_end(&scroll);
    tree.layout(window);
    assert!(region(&tree.layout(window), &scroll).is_some());
    // One frame as the app has it: kept at the end while it is there.
    let frame = |tree: &mut UiTree, store: &Store| {
        let kept = tree.is_scroll_at_end(&scroll, 0.5);
        tree.set_root(view(store, &ui, window));
        if kept {
            tree.scroll_to_end(&scroll);
        }
        tree.layout(window);
    };
    say(&mut store);
    frame(&mut tree, &store);
    assert!(tree.is_scroll_at_end(&scroll, 0.5), "it follows");
    tree.set_scroll_offset(&scroll, 0.0);
    tree.layout(window);
    say(&mut store);
    frame(&mut tree, &store);
    assert!(
        !tree.is_scroll_at_end(&scroll, 8.0),
        "scrolled up, it holds"
    );
    tree.scroll_to_end(&scroll);
    tree.layout(window);
    say(&mut store);
    frame(&mut tree, &store);
    assert!(
        tree.is_scroll_at_end(&scroll, 0.5),
        "back at the end, it follows"
    );
}

/// What a transcript holds that grows: an answered question with long options, a group of
/// steps with a long command, its output with a line that never breaks, an edit's diff and a
/// reply's code block, then `pending`, the card that waits on the person.
fn growing_turn(pending: &str) -> (Store, Ui) {
    let (mut store, ui) = store_with_session();
    let long = "a-path/that/never/ends/".repeat(12);
    let question = "Which of these ways of extracting the icons should the helper agent use for every glyph in the set?";
    let options: Vec<_> = (0..3)
        .map(|i| {
            serde_json::json!({
                "label": format!("Option {i}: render each glyph through the module source and keep its fill rule"),
                "description": format!("Longer account {i} of what this choice does to every icon, its colours and its local functions."),
            })
        })
        .collect();
    let questions =
        serde_json::json!({ "questions": [{ "question": question, "options": options }] });
    tool(&mut store, "q1", "AskUserQuestion", questions.clone(), None);
    result(
        &mut store,
        "q1",
        &format!("The user answered: \"{question}\"=\"Option 1: render each glyph through the module source and keep its fill rule\"."),
    );
    let bash = serde_json::json!({
        "command": format!("python3 extract.py --out /work/{long} && ls -la /work/{long}"),
        "description": "Extract every icon of the set into its own folder",
    });
    tool(&mut store, "b1", "Bash", bash, None);
    let names: String = (0..60).map(|i| format!("Icon{i}Neutral")).collect();
    result(
        &mut store,
        "b1",
        &format!("{names}\nModuleNotFoundError: No module named 'cairosvg'"),
    );
    let edit = serde_json::json!({ "file_path": "/work/extract.py", "old_string": "a", "new_string": "b" });
    tool(&mut store, "e1", "Edit", edit, None);
    agent(
        &mut store,
        AgentEvent::ToolResult {
            call: "e1".into(),
            output: "ok".into(),
            is_error: false,
            files: vec![newen_model::FileChange {
                path: "/work/extract.py".into(),
                before: Some("import os\nprint(os.listdir('.'))\n".into()),
                after: Some(format!("import os\nprint(os.listdir('{long}'))\n")),
            }],
            command: None,
            web: None,
            images: vec![],
            inline: vec![],
        },
    );
    agent(
        &mut store,
        AgentEvent::Text {
            text: format!("Ran it. The listing:\n\n```\n{names}\n```\n\nand the error is above."),
            parent: None,
            id: None,
            at: None,
        },
    );
    let (tool_name, input) = match pending {
        "question" => ("AskUserQuestion", questions),
        "plan" => (
            "ExitPlanMode",
            serde_json::json!({ "plan": format!("# Plan\n\n1. Extract the icons into `{long}`.\n\n```\n{names}\n```") }),
        ),
        _ => (
            "Bash",
            serde_json::json!({ "command": format!("rm -rf /work/{long} && python3 extract.py"), "description": "Clear the folder and extract again" }),
        ),
    };
    store.apply(Event::Status {
        session: 1,
        status: SessionStatus::Awaiting,
    });
    agent(
        &mut store,
        AgentEvent::PermissionRequest {
            id: "p9".into(),
            call: Some("x9".into()),
            tool: tool_name.into(),
            input,
            title: None,
            interactive: true,
            suggestions: vec![],
        },
    );
    (store, ui)
}

/// Every line in the scroll area `scroll` keeps to it and none is drawn over another, from
/// its top to its end, in the stand-in measure and in the fonts the app lays text out in.
/// What it drew is given back.
fn assert_rows_apart(
    store: &Store,
    ui: &Ui,
    window: limn::Size,
    scroll: &str,
    at: &str,
) -> Vec<String> {
    let mut seen = vec![];
    for laid_out in [false, true] {
        let mut tree = UiTree::new(view(store, ui, window));
        tree.set_text_layout(laid_out);
        tree.set_reduced_motion(true);
        for step in 0..40 {
            tree.set_scroll_offset(scroll, step as f32 * 180.0);
            let frame = tree.layout(window);
            let Some(pane) = region(&frame, scroll) else {
                panic!("{at}: no {scroll}");
            };
            let spans: Vec<_> = drawn_spans(&frame)
                .into_iter()
                .filter(|s| s.1 >= pane.x && s.1 < pane.x + pane.width)
                .filter(|s| s.2 + s.4 > pane.y && s.2 < pane.y + pane.height)
                .collect();
            if std::env::var_os("SPANS").is_some() {
                eprintln!("{at} {laid_out} {step}: {pane:?}");
                for s in &spans {
                    eprintln!("{s:?}");
                }
            }
            for s in &spans {
                assert!(
                    s.3 <= pane.x + pane.width + 0.5,
                    "{at} {laid_out}: {s:?} runs past {pane:?}"
                );
            }
            assert_apart(&spans, &format!("{at} {laid_out} {step}"));
            seen.extend(spans.into_iter().map(|s| s.0));
            if tree.is_scroll_at_end(scroll, 0.5) {
                break;
            }
        }
    }
    seen
}

/// The question card with long options, the answered one, a plan's card and a command's,
/// a group of steps folded and open with its command, output and diff, and a reply's code
/// block: no line over another, in the transcript and in a helper's pane, narrow and wide.
#[test]
fn cards_that_grow_keep_their_rows_apart_open_and_folded() {
    // The crowded turn too, all the way down rather than its first screen.
    let (store, mut ui) = crowded_turn();
    ui.expanded.extend(["steps:a8".into(), "steps:r1".into()]);
    settle(&store, &mut ui);
    for width in [648.0, 800.0, 1600.0] {
        let window = limn::Size::new(width, 900.0);
        assert_rows_apart(
            &store,
            &ui,
            window,
            "transcript:1",
            &format!("crowded {width}"),
        );
    }
    for pending in ["question", "plan", "command"] {
        for open in [false, true] {
            let (store, mut ui) = growing_turn(pending);
            if open {
                for key in ["steps:b1", "b1", "e1"] {
                    ui.expanded.push(key.into());
                }
            }
            settle(&store, &mut ui);
            for width in [648.0, 1000.0, 1600.0, 2400.0] {
                let window = limn::Size::new(width, 900.0);
                let at = format!("{pending} {open} {width}");
                let seen = assert_rows_apart(&store, &ui, window, "transcript:1", &at);
                let mut words = vec!["Which of these", "Ran it", "Icon59Neutral"];
                words.push(match pending {
                    "question" => "Option 2",
                    "plan" => "proposed a plan",
                    _ => "rm -rf",
                });
                if open {
                    words.extend(["ModuleNotFoundError", "listdir"]);
                }
                for words in words {
                    assert!(seen.iter().any(|s| s.contains(words)), "{at}: no {words}");
                }
            }
            // The plan in its own pane, beside.
            if pending == "plan" {
                ui.plan = Some("p9".into());
                ui.open_pane(Pane::Plan);
                settle(&store, &mut ui);
                for width in [1408.0, 2000.0] {
                    let window = limn::Size::new(width, 900.0);
                    let at = format!("plan pane {open} {width}");
                    let seen = assert_rows_apart(&store, &ui, window, "plan:scroll", &at);
                    assert!(seen.iter().any(|s| s.contains("Icon59")), "{at}: no plan");
                }
            }
        }
    }
}

/// An open group of steps folds from its end as from its header: a chevron at the box's
/// bottom right, painted while the pointer is on the box, named "Collapse" and in the Tab
/// order. Verbose holds the group open and has none.
#[test]
fn an_open_group_folds_from_a_chevron_at_its_end() {
    let (mut store, mut ui) = store_with_session();
    for call in ["b1", "b2"] {
        let input = serde_json::json!({ "command": "ls", "description": "List the icons" });
        tool(&mut store, call, "Bash", input, None);
        result(&mut store, call, "ok");
    }
    click(&store, &mut ui, "steps:b1");
    settle(&store, &mut ui);
    let fold = format!("{}steps:b1", view::GROUP_FOLD);
    let window = limn::Size::new(1408.0, 900.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    tree.set_reduced_motion(true);
    let frame = tree.layout(window);
    let button = region(&frame, &fold).expect("the chevron");
    let last = drawn_spans(&frame)
        .into_iter()
        .filter(|s| s.0.contains("List the icons") || s.0 == "ls")
        .map(|s| s.2)
        .fold(0.0, f32::max);
    assert!(button.y > last, "under the last row: {button:?}");
    let shows = |frame: &limn::Frame| {
        frame.commands.iter().any(|c| {
            matches!(c, limn::DrawCommand::Icon { rect, tint, .. }
                if tint.a > 0 && rect.x >= button.x && rect.x < button.x + button.width
                    && rect.y >= button.y && rect.y < button.y + button.height)
        })
    };
    assert!(!shows(&frame), "hidden at rest");
    let center = limn::Point::new(button.x + button.width / 2.0, button.y - 20.0);
    tree.handle_event(limn::UiEvent::PointerMove(center));
    assert!(shows(&tree.layout(window)), "shown on the box");
    let frame = tree.layout(window);
    assert!(
        frame.access.iter().any(|node| node.name == "Collapse"),
        "named for the keys"
    );
    assert!(click(&store, &mut ui, &fold).is_empty());
    assert!(!ui.expanded.iter().any(|k| k == "steps:b1"), "folded");
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(region(&frame, &fold).is_none());
}

/// The browser's address shows its whole line, in the middle of its field, in the stand-in
/// measure and in the fonts the app lays text out in.
#[test]
fn the_browser_address_shows_its_whole_line() {
    use newen_model::{BrowserTab, BrowserUpdate};
    let (mut store, mut ui) = store_with_session();
    let tabs = Event::Browser {
        session: 1,
        update: BrowserUpdate::Tabs {
            tabs: vec![BrowserTab {
                id: 3,
                url: "https://code.claude.com/docs/en/overview".into(),
                title: "Overview".into(),
                ..Default::default()
            }],
            active: Some(3),
        },
    };
    ui.applied(&tabs);
    store.apply(tabs);
    settle(&store, &mut ui);
    let window = limn::Size::new(1408.0, 844.0);
    for laid_out in [false, true] {
        let mut tree = UiTree::new(view(&store, &ui, window));
        tree.set_text_layout(laid_out);
        let frame = tree.layout(window);
        let field = region(&frame, view::BROWSER_URL).expect("the address");
        let mut clips = vec![];
        let mut drawn = vec![];
        for command in &frame.commands {
            match command {
                limn::DrawCommand::PushClip(rect) => clips.push(*rect),
                limn::DrawCommand::PopClip => {
                    clips.pop();
                }
                limn::DrawCommand::Text {
                    text,
                    position,
                    font_size,
                    ..
                } if text.contains("claude.com") => {
                    drawn.push((position.y, *font_size, clips.last().copied()))
                }
                limn::DrawCommand::Glyphs(run) if run.text.contains("claude.com") => {
                    drawn.push((run.origin.y, run.size, clips.last().copied()))
                }
                _ => {}
            }
        }
        assert!(!drawn.is_empty(), "{laid_out}: the address is drawn");
        for (top, size, clip) in drawn {
            let line = 18.0 * size / 14.0;
            let clip = clip.expect("clipped to its field");
            assert!(clip.height >= line - 0.5, "{laid_out}: {clip:?}");
            assert!(clip.y >= field.y && clip.y + clip.height <= field.y + field.height);
            let middle = top + line / 2.0 - field.y;
            assert!(
                (middle - field.height / 2.0).abs() < 1.0,
                "{laid_out}: {middle}"
            );
        }
    }
}

/// A thumbnail's card goes with the viewer it opens: closed by the click, not there while the
/// viewer is, and after it only on a fresh rest of the pointer on the thumbnail.
#[test]
fn a_thumbnail_s_card_does_not_outlive_the_viewer() {
    let (mut store, mut ui) = store_with_session();
    screenshot(&mut store, "c1", &["/work/a.png"]);
    screenshot(&mut store, "c2", &["/work/b.png"]);
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    tree.set_reduced_motion(true);
    let frame = tree.layout(window);
    let thumb = region(&frame, "image:step:c2:0").expect("the thumbnail");
    let center = limn::Point::new(thumb.x + thumb.width / 2.0, thumb.y + thumb.height / 2.0);
    tree.handle_event(limn::UiEvent::PointerMove(center));
    assert!(
        painted(&tree.layout(window), "2 of 2").is_some(),
        "the card"
    );
    // As the app runs: what the tree reports goes to `update`, and the view is built again.
    let press = |tree: &mut UiTree, ui: &mut Ui, at: limn::Point| {
        let mut actions = tree.handle_event(limn::UiEvent::PointerDown(at));
        actions.extend(tree.handle_event(limn::UiEvent::PointerUp(at)));
        for action in actions {
            update(&store, ui, &action);
        }
        tree.set_root(view(&store, ui, window));
        tree.layout(window)
    };
    let frame = press(&mut tree, &mut ui, center);
    assert!(ui.viewer.is_some(), "the viewer opened");
    // Closed as a person does, through the tree, the pointer gone from the thumbnail (to the
    // viewer's corner, where its close is): Esc, since a press on the picture zooms it.
    let _ = frame;
    tree.handle_event(limn::UiEvent::PointerMove(limn::Point::new(1380.0, 30.0)));
    for action in tree.handle_event(key(limn::Key::Escape)) {
        update(&store, &mut ui, &action);
    }
    tree.set_root(view(&store, &ui, window));
    let frame = tree.layout(window);
    assert!(ui.viewer.is_none(), "the viewer closed");
    assert!(
        painted(&frame, "2 of 2").is_none(),
        "no card once it closed"
    );
    tree.handle_event(limn::UiEvent::PointerMove(limn::Point::new(700.0, 400.0)));
    assert!(
        painted(&tree.layout(window), "2 of 2").is_none(),
        "nor elsewhere"
    );
    tree.handle_event(limn::UiEvent::PointerMove(center));
    assert!(
        painted(&tree.layout(window), "2 of 2").is_some(),
        "a fresh rest shows it"
    );
}

/// A release's notes with an announcement whose picture is at `/notes/{version}/shot.png`.
fn release_notes(version: &str, try_it: Option<&str>) -> newen_model::ReleaseNotes {
    newen_model::ReleaseNotes {
        version: version.into(),
        date: "2026-11-10".into(),
        new: vec!["A browser beside the chat".into()],
        improved: vec!["Faster `git` status in large folders".into()],
        fixed: vec![],
        announcement: Some(newen_model::Announcement {
            title: "A browser beside the chat".into(),
            body: "Open any page next to the session and point the agent at it.".into(),
            bullets: vec!["Pages keep their sign-ins".into()],
            media: Some(newen_model::Media::Image {
                path: format!("/notes/{version}/shot.png").into(),
            }),
            try_it: try_it.map(Into::into),
        }),
    }
}

fn whats_new_event(ui: &mut Ui, whats_new: newen_model::WhatsNew) -> Vec<Intent> {
    ui.applied(&Event::WhatsNew(whats_new))
}

/// After an update the engine says to announce this version once: the card shows with Got it
/// and Try it, and closes; a later report that does not say so opens nothing.
#[test]
fn the_announcement_shows_once_after_an_update_and_try_it_goes_there() {
    let (store, mut ui) = store_with_session();
    let notes = release_notes("0.3.0", Some("settings:system"));
    let mut told = newen_model::WhatsNew {
        releases: vec![notes.clone()],
        ready: None,
        announce: true,
        unopened: true,
    };
    whats_new_event(&mut ui, told.clone());
    assert!(drawn_words(&store, &ui, "A browser beside the chat"));
    assert!(drawn_words(&store, &ui, "Got it") && drawn_words(&store, &ui, "Try it"));
    assert!(!drawn_words(&store, &ui, "Restart and update"));
    assert!(click(&store, &mut ui, whats_new::GOT_IT).is_empty());
    assert!(!drawn_words(&store, &ui, "Point the agent"));
    assert_eq!(ui.announcing, None);
    // The engine marks it seen before it says so: the next report does not announce.
    told.announce = false;
    whats_new_event(&mut ui, told.clone());
    assert_eq!(ui.announcing, None);
    // Try it opens where the announcement says.
    told.announce = true;
    whats_new_event(&mut ui, told.clone());
    click(&store, &mut ui, whats_new::TRY_IT);
    assert_eq!(
        (ui.announcing.as_deref(), ui.settings_page.as_deref()),
        (None, Some("system"))
    );
    // Somewhere the interface does not know: no Try it.
    told.releases = vec![release_notes("0.3.0", Some("somewhere:else"))];
    whats_new_event(&mut ui, told);
    ui.settings_page = None;
    assert!(drawn_words(&store, &ui, "Got it") && !drawn_words(&store, &ui, "Try it"));
}

fn ready_with_notes(ui: &mut Ui) {
    ready_update(ui, false);
    whats_new_event(
        ui,
        newen_model::WhatsNew {
            ready: Some(release_notes("0.3.0", None)),
            ..Default::default()
        },
    );
}

/// The strip opens the ready version's announcement; Later keeps the strip, and Restart and
/// update installs at once with no agent at work.
#[test]
fn the_strip_opens_the_ready_versions_announcement_and_later_keeps_the_strip() {
    let (store, mut ui) = store_with_session();
    ready_with_notes(&mut ui);
    assert!(click(&store, &mut ui, updates::UPDATE_STRIP).is_empty());
    assert!(drawn_words(&store, &ui, "Restart and update") && drawn_words(&store, &ui, "Later"));
    assert!(click(&store, &mut ui, whats_new::LATER).is_empty());
    assert!(!drawn_words(&store, &ui, "Restart and update"));
    assert!(drawn_words(&store, &ui, "Relaunch to update"));
    // Escape (the card's close) does the same.
    click(&store, &mut ui, updates::UPDATE_STRIP);
    assert!(click(&store, &mut ui, whats_new::CARD).is_empty());
    assert!(drawn_words(&store, &ui, "Relaunch to update"));
    click(&store, &mut ui, updates::UPDATE_STRIP);
    assert_eq!(
        click(&store, &mut ui, whats_new::RESTART),
        vec![Intent::InstallUpdate]
    );
}

/// With an agent at work, Restart and update asks first, as the strip does.
#[test]
fn restart_and_update_with_agents_at_work_asks_first() {
    let (mut store, mut ui) = store_with_session();
    let id = ui.selected.expect("a session");
    set_status(&mut store, id, newen_model::SessionStatus::Running);
    ready_with_notes(&mut ui);
    click(&store, &mut ui, updates::UPDATE_STRIP);
    assert!(click(&store, &mut ui, whats_new::RESTART).is_empty());
    assert!(ui.update_asking && ui.announcing.is_none());
    assert!(drawn_words(&store, &ui, "Agents are still working"));
    assert!(click(&store, &mut ui, updates::UPDATE_ASK_WAIT).is_empty());
    assert!(drawn_words(&store, &ui, "Restarting when agents finish"));
}

/// Settings' switch off: the strip restarts as before, and an announcement the engine sends
/// is not one it would (it reads the same switch); the notes still open from the account menu.
#[test]
fn the_switch_turns_the_announcements_off_and_the_notes_stay() {
    let (store, mut ui) = store_with_session();
    click(&store, &mut ui, view::SETTINGS_OPEN);
    let saved = click(&store, &mut ui, "set:whats_new");
    assert!(matches!(&saved[..], [Intent::SaveSettings(s)] if !s.whats_new));
    ui.settings.whats_new = false;
    click(&store, &mut ui, view::SETTINGS_CLOSE);
    ready_with_notes(&mut ui);
    assert_eq!(
        click(&store, &mut ui, updates::UPDATE_STRIP),
        vec![Intent::InstallUpdate]
    );
    assert!(!drawn_words(&store, &ui, "Restart and update"));
    click(&store, &mut ui, view::ACCOUNT_MENU);
    click(&store, &mut ui, whats_new::OPEN);
    assert!(ui.notes_open && !ui.account_menu);
}

/// The sidebar's What's new shows until this version's notes are opened: the click opens the
/// dialog, the engine keeps it, and once it says so the button goes.
#[test]
fn the_whats_new_button_goes_once_the_notes_are_opened() {
    let (store, mut ui) = store_with_session();
    let mut told = newen_model::WhatsNew {
        releases: vec![
            release_notes("0.3.0", None),
            newen_model::ReleaseNotes {
                version: "0.2.0".into(),
                date: "2026-10-01".into(),
                fixed: vec!["The strip no longer flickers".into()],
                ..Default::default()
            },
        ],
        ready: None,
        announce: false,
        unopened: true,
    };
    whats_new_event(&mut ui, told.clone());
    let button = |ui: &Ui| region(&still(&store, ui), whats_new::BUTTON).is_some();
    assert!(button(&ui));
    assert!(drawn_words(&store, &ui, "What's new"));
    assert_eq!(
        click(&store, &mut ui, whats_new::BUTTON),
        vec![Intent::WhatsNewOpened]
    );
    for words in [
        "IMPROVED",
        "FIXED",
        "v0.2.0",
        "The strip no longer flickers",
    ] {
        assert!(drawn_words(&store, &ui, words), "{words}");
    }
    // Until the engine has kept it, the button stays; then it goes.
    assert!(button(&ui));
    told.unopened = false;
    whats_new_event(&mut ui, told);
    assert!(!button(&ui));
    click(&store, &mut ui, whats_new::NOTES);
    assert!(!ui.notes_open);
    // A version waiting takes the place: the strip, not the button.
    let mut ui2 = Ui::new("/work".into());
    ui2.skip_welcome = true;
    whats_new_event(
        &mut ui2,
        newen_model::WhatsNew {
            releases: vec![release_notes("0.3.0", None)],
            unopened: true,
            ..Default::default()
        },
    );
    ready_update(&mut ui2, false);
    assert!(region(&still(&store, &ui2), whats_new::BUTTON).is_none());
}

/// The card at a small window and a large one: the picture when it has room (dropped when
/// not), no line over another, all of it inside the card and the card inside the window, the
/// buttons below the words.
#[test]
fn the_announcement_card_keeps_its_parts_apart_at_small_and_large_windows() {
    let (_, mut ui) = store_with_session();
    let notes = release_notes("0.3.0", Some("settings:system"));
    let picture = limn::RasterImage::new(
        "a".repeat(64),
        limn::RasterImageFormat::Png,
        3,
        std::sync::Arc::from("cG5n"),
        "shot",
    )
    .unwrap();
    ui.full.insert("/notes/0.3.0/shot.png".into(), picture);
    whats_new_event(
        &mut ui,
        newen_model::WhatsNew {
            releases: vec![notes],
            announce: true,
            unopened: true,
            ..Default::default()
        },
    );
    let long = |ui: &mut Ui| {
        let mut notes = release_notes("0.3.0", Some("settings:system"));
        let a = notes.announcement.as_mut().unwrap();
        a.title =
            "A browser beside the chat, and the agent can use it too. ".repeat(2)[..60].into();
        a.body =
            "Open any page next to the session and point the agent at it. ".repeat(5)[..280].into();
        a.bullets = vec![
            "Pages keep their sign-ins across sessions and restarts of Newen. "
                .repeat(3)[..160]
                .into();
            5
        ];
        ui.whats_new.releases = vec![notes];
    };
    let windows = [
        (360.0, 520.0),
        (648.0, 480.0),
        (1408.0, 844.0),
        (2400.0, 1400.0),
    ];
    for (width, height, longest) in windows
        .iter()
        .flat_map(|(w, h)| [(*w, *h, false), (*w, *h, true)])
    {
        if longest {
            long(&mut ui);
        }
        let window = limn::Size::new(width, height);
        for laid_out in [false, true] {
            // Over nothing, so only the card's own lines are drawn.
            let card = whats_new::dialogs(&ui, limn::Element::spacer(), window, look::palette());
            let mut tree = UiTree::new(card);
            tree.set_text_layout(laid_out);
            tree.set_reduced_motion(true);
            let frame = tree.layout(window);
            let at = format!("{width}x{height} {laid_out} {longest}");
            let card = (frame.access.iter())
                .find(|n| n.id == format!("{}:dialog", whats_new::CARD))
                .unwrap_or_else(|| panic!("{at}: no card"))
                .rect;
            assert!(
                card.x >= 0.0
                    && card.y >= 0.0
                    && card.x + card.width <= width
                    && card.y + card.height <= height,
                "{at}: {card:?}"
            );
            let inside = |r: limn::Rect| {
                r.x >= card.x - 0.5
                    && r.y >= card.y - 0.5
                    && r.x + r.width <= card.x + card.width + 0.5
                    && r.y + r.height <= card.y + card.height + 0.5
            };
            // The words scroll in their own area, which ends over the buttons.
            let words = region(&frame, "whats_new:words").expect("the words");
            let got_it = region(&frame, whats_new::GOT_IT).expect("Got it");
            let try_it = region(&frame, whats_new::TRY_IT).expect("Try it");
            assert!(
                inside(words) && words.y + words.height <= got_it.y + 0.5,
                "{at}: {words:?}"
            );
            let spans: Vec<_> = (drawn_spans(&frame).into_iter())
                .filter(|s| {
                    let shown = s.2 + s.4 > words.y && s.2 < words.y + words.height;
                    let on = |b: &limn::Rect| {
                        s.1 >= b.x
                            && s.3 <= b.x + b.width + 0.5
                            && s.2 >= b.y
                            && s.2 < b.y + b.height
                    };
                    shown || on(&got_it) || on(&try_it)
                })
                .collect();
            if std::env::var_os("SPANS").is_some() {
                eprintln!("{at} {card:?}");
                for s in &spans {
                    eprintln!("{s:?}");
                }
            }
            assert_apart(&spans, &at);
            for s in &spans {
                assert!(
                    s.3 <= card.x + card.width + 0.5,
                    "{at}: {s:?} runs past {card:?}"
                );
            }
            let title = spans
                .iter()
                .find(|s| s.0.contains("browser"))
                .expect("the title");
            for button in [got_it, try_it] {
                assert!(inside(button), "{at}: {button:?}");
                assert!(
                    spans
                        .iter()
                        .all(|s| s.2 + s.4 <= button.y + 0.5 || s.2 >= button.y),
                    "{at}"
                );
            }
            assert!(got_it.x + got_it.width <= try_it.x, "{at}");
            let close = region(&frame, "whats_new:close").expect("the close");
            assert!(inside(close), "{at}: {close:?}");
            let shown = frame.commands.iter().find_map(|c| match c {
                limn::DrawCommand::RasterImage { rect, .. } => Some(*rect),
                _ => None,
            });
            match shown {
                Some(rect) => {
                    assert!(
                        inside(rect) && rect.y + rect.height <= title.2,
                        "{at}: {rect:?}"
                    );
                    assert!((rect.width / rect.height - 16.0 / 9.0).abs() < 0.02, "{at}");
                }
                None => assert!(height < 600.0, "{at}: the picture had room"),
            }
        }
    }
}

fn configured(
    name: &str,
    target: &str,
    status: newen_model::McpStatus,
) -> newen_model::ConfiguredServer {
    newen_model::ConfiguredServer {
        name: name.into(),
        label: name.into(),
        target: target.into(),
        status,
        checked: true,
        source: newen_model::McpSource::Other,
    }
}

/// Two agents' connectors: Claude Code's github (connected), figma (to sign in to) and one
/// with a long name; Codex's linear (failing).
fn customize_connectors() -> (Store, Ui) {
    use newen_model::McpStatus;
    let (store, mut ui) = two_agents_on(view::SETTINGS_CONNECTORS);
    for (agent, servers) in [
        (
            "claude",
            vec![
                configured("github", "npx -y server-github", McpStatus::Connected),
                configured("figma", "https://mcp.figma.com/mcp", McpStatus::NeedsAuth),
                configured(
                    "a-connector-with-a-name-long-enough-to-be-cut-in-a-narrow-window",
                    "uvx long",
                    McpStatus::Connected,
                ),
            ],
        ),
        (
            "codex",
            vec![configured(
                "linear",
                "https://mcp.linear.app/mcp",
                McpStatus::Failed,
            )],
        ),
    ] {
        ui.applied(&Event::Servers {
            agent: agent.into(),
            folder: "/work".into(),
            servers,
            scopes: vec![],
            error: None,
        });
    }
    (store, ui)
}

fn tool_named(name: &str, read_only: bool, destructive: bool) -> newen_model::McpTool {
    newen_model::McpTool {
        name: name.into(),
        description: Some(format!(
            "What {name} does, said at some length so it runs long"
        )),
        read_only,
        destructive,
    }
}

/// Customize's page, its list with a strip to mend, a connector open with its tools, Discover's
/// cards and Skills: no line over another, none past the page, header and tabs included,
/// narrow and wide.
fn customize_crowded() -> (Store, Ui) {
    let (store, mut ui) = customize_connectors();
    ui.applied(&Event::ServerTools {
        agent: "claude".into(),
        servers: vec![newen_model::McpServer {
            name: "github".into(),
            label: "github".into(),
            status: newen_model::McpStatus::Connected,
            source: newen_model::McpSource::User,
            plugin: None,
            error: None,
            tools: vec![
                tool_named("list_issues", true, false),
                tool_named("delete_repo", false, true),
                tool_named("open_pull_request_with_a_long_name", false, false),
            ],
            sign_in: false,
        }],
        error: None,
    });
    ui.applied(&Event::ServersFound {
        query: String::new(),
        offers: (0..5)
            .map(|n| newen_model::ServerOffer {
                name: format!("io.example/server-{n}"),
                title: Some(format!("A server with a title long enough to wrap {n}")),
                description: "It does a great many things, and says so at length, over more than two lines of a card's words.".into(),
                version: "1.0.0".into(),
                remote: n % 2 == 0,
                target: "https://example.com/mcp".into(),
                ..Default::default()
            })
            .collect(),
        error: None,
    });
    (store, ui)
}

#[test]
fn the_customize_page_keeps_its_rows_apart_narrow_and_wide() {
    type Then = fn(&Store, &mut Ui);
    let states: [(&str, Then); 4] = [
        ("list", |_, _| {}),
        ("detail", |store, ui| {
            click(store, ui, "czp:open:srv:claude:github");
        }),
        ("discover", |store, ui| {
            click(store, ui, "srv:tab:discover");
        }),
        ("skills", |store, ui| {
            click(store, ui, &format!("czp:tab:{}", view::SETTINGS_SKILLS));
            heard_extensions(
                ui,
                "claude",
                skills_list(vec![
                    skill("graphify", "user", "on"),
                    skill("a-skill-with-a-very-long-name-indeed", "plugin", "off"),
                ]),
            );
        }),
    ];
    for (name, set) in states {
        let (store, mut ui) = customize_crowded();
        set(&store, &mut ui);
        for width in [700.0, 1000.0, 1600.0, 2400.0] {
            let window = limn::Size::new(width, 900.0);
            let at = format!("{name} {width}");
            assert_rows_apart(&store, &ui, window, "czp:scroll", &at);
            // Over the list: the header, the tabs, the crumbs and the tools.
            let mut tree = UiTree::new(view(&store, &ui, window));
            tree.set_text_layout(true);
            tree.set_reduced_motion(true);
            let frame = tree.layout(window);
            let pane = region(&frame, "czp:scroll").expect("the page scrolls");
            let spans: Vec<_> = (drawn_spans(&frame).into_iter())
                .filter(|s| s.1 >= pane.x - 1.0 && s.2 + s.4 <= pane.y + 0.5)
                .collect();
            assert!(spans.iter().any(|s| s.0 == "Customize"), "{at}");
            for s in &spans {
                assert!(
                    s.3 <= window.width + 0.5,
                    "{at}: {s:?} runs past the window"
                );
            }
            assert_apart(&spans, &at);
        }
    }
}

/// limn-video's test pattern, 2 s at 320×180.
const PATTERN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../limn-quill-integration/crates/limn-video/assets/pattern.mp4"
));

/// An announcement whose medium is a clip, up after an update.
fn announcing_a_clip(ui: &mut Ui) {
    let mut notes = release_notes("0.3.0", None);
    notes.announcement.as_mut().unwrap().media = Some(newen_model::Media::Video {
        poster: "/notes/0.3.0/poster.png".into(),
        clip: "/notes/0.3.0/clip.mp4".into(),
    });
    whats_new_event(
        ui,
        newen_model::WhatsNew {
            releases: vec![notes],
            announce: true,
            ..Default::default()
        },
    );
}

fn toggle_shown(store: &Store, ui: &Ui) -> bool {
    let toggle = limn::Video::toggle_id(whats_new::VIDEO);
    region(&still(store, ui), &toggle).is_some()
}

/// The card asks once whether to download Cisco's codec, and never fetches it itself: its
/// answer goes to the engine, which keeps it first. Asked, it is not asked again.
#[test]
fn the_card_asks_before_the_codec_is_downloaded_and_only_once() {
    let (store, mut ui) = store_with_session();
    announcing_a_clip(&mut ui);
    assert!(drawn_words(
        &store,
        &ui,
        "downloads the OpenH264 video codec from Cisco"
    ));
    assert!(drawn_words(&store, &ui, limn_video::cisco::NOTICE));
    let asked = click(&store, &mut ui, whats_new::CODEC_YES);
    assert!(matches!(&asked[..], [Intent::SaveSettings(s)] if s.video_codec == Some(true)));
    let declined = click(&store, &mut ui, whats_new::CODEC_NO);
    assert!(matches!(&declined[..], [Intent::SaveSettings(s)] if s.video_codec == Some(false)));
    // The engine's echo: answered, not asked again.
    ui.settings.video_codec = Some(false);
    assert!(!drawn_words(
        &store,
        &ui,
        "downloads the OpenH264 video codec from Cisco"
    ));
    // An image asks nothing.
    ui.settings.video_codec = None;
    whats_new_event(
        &mut ui,
        newen_model::WhatsNew {
            releases: vec![release_notes("0.3.0", None)],
            announce: true,
            ..Default::default()
        },
    );
    assert!(!drawn_words(
        &store,
        &ui,
        "downloads the OpenH264 video codec from Cisco"
    ));
}

/// Without a codec the clip is its poster, with no play button; with Cisco's codec loaded it
/// plays, with the button, and its frames decode. The second half needs Cisco's library at
/// `LIMN_VIDEO_OPENH264` and is skipped without it.
#[test]
fn the_card_shows_the_poster_without_a_codec_and_plays_with_one() {
    let (store, mut ui) = store_with_session();
    announcing_a_clip(&mut ui);
    assert_eq!(whats_new::clip(&ui), Some("/notes/0.3.0/clip.mp4".into()));
    let clip = limn_video::Clip::from_mp4(PATTERN).unwrap();
    ui.video = Some(limn_video::VideoPlayer::new(
        whats_new::VIDEO,
        clip.clone(),
        true,
        None,
    ));
    let frame = still(&store, &ui);
    assert!(frame
        .access
        .iter()
        .any(|n| n.name == "A browser beside the chat"));
    assert!(!toggle_shown(&store, &ui));
    // Closed, the clip is no more to play.
    click(&store, &mut ui, whats_new::GOT_IT);
    assert_eq!(whats_new::clip(&ui), None);
    let Some(path) = std::env::var_os("LIMN_VIDEO_OPENH264") else {
        eprintln!("skipped: set LIMN_VIDEO_OPENH264 to Cisco's OpenH264 2.6.0 library");
        return;
    };
    let codec = limn_video::Codec::load(path).expect("Cisco's library loads");
    announcing_a_clip(&mut ui);
    let mut player = limn_video::VideoPlayer::new(whats_new::VIDEO, clip, true, Some(codec));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut now = std::time::Instant::now();
    loop {
        let tick = player.tick(now, true);
        if tick.frame.is_some() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "no frame decoded");
        std::thread::sleep(std::time::Duration::from_millis(20));
        now += tick
            .repaint_after
            .unwrap_or(std::time::Duration::from_millis(20));
    }
    ui.video = Some(player);
    assert!(toggle_shown(&store, &ui));
}

/// Settings › System: the switch turns Cisco's codec on, off and on again, beside Cisco's
/// notice; the licenses dialog there has Cisco's license whole.
#[test]
fn the_codec_switch_sits_by_ciscos_notice_and_its_license_is_reachable() {
    let (store, mut ui) = store_with_session();
    click(&store, &mut ui, view::SETTINGS_OPEN);
    click(&store, &mut ui, "settings:page:system");
    assert!(drawn_words(&store, &ui, "Play videos in What's new"));
    assert!(drawn_words(&store, &ui, "Cisco Systems, Inc."));
    let on = click(&store, &mut ui, "set:video_codec");
    assert!(matches!(&on[..], [Intent::SaveSettings(s)] if s.video_codec == Some(true)));
    ui.settings.video_codec = Some(true);
    ui.applied(&Event::Codec(newen_model::CodecState::Downloading));
    assert!(drawn_words(&store, &ui, "Downloading"));
    let off = click(&store, &mut ui, "set:video_codec");
    assert!(matches!(&off[..], [Intent::SaveSettings(s)] if s.video_codec == Some(false)));
    ui.settings.video_codec = Some(false);
    let again = click(&store, &mut ui, "set:video_codec");
    assert!(matches!(&again[..], [Intent::SaveSettings(s)] if s.video_codec == Some(true)));
    // Cisco's license, as Cisco publishes it, with its conditions.
    assert!(licenses::OPENH264.contains("AVC/H.264 Patent Portfolio License Conditions"));
    assert!(licenses::OPENH264.contains(limn_video::cisco::NOTICE));
    click(&store, &mut ui, licenses::OPEN);
    assert!(ui.licenses_open);
    assert!(drawn_words(&store, &ui, "Licenses"));
    assert!(drawn_words(
        &store,
        &ui,
        "About The Cisco-Provided Binary of OpenH264"
    ));
    click(&store, &mut ui, "licenses");
    assert!(!ui.licenses_open);
}

/// A tab none of the agents shown has is off, says why, and does nothing when clicked; with
/// "All agents" it is on as long as one agent has it.
#[test]
fn a_tab_no_agent_shown_has_is_off_with_why() {
    let (store, mut ui) = two_agents_on(view::SETTINGS_SKILLS);
    ui.agents[1].customize.plugins = false;
    assert_eq!(customize::tab_off(&ui, view::SETTINGS_PLUGINS), None);
    click(&store, &mut ui, "czp:layer:=codex");
    assert_eq!(
        customize::tab_off(&ui, view::SETTINGS_PLUGINS).as_deref(),
        Some("Codex has no plugins Newen can manage.")
    );
    assert!(click(&store, &mut ui, "czp:tab:plugins").is_empty());
    assert_eq!(ui.customize.as_ref().unwrap().tab, view::SETTINGS_SKILLS);
    // The layer shows one agent's sections only.
    assert_eq!(
        customize::agents(&ui)
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        ["codex"]
    );
    click(&store, &mut ui, "czp:layer:=");
    assert!(!click(&store, &mut ui, "czp:tab:plugins").is_empty());
    assert_eq!(ui.customize.as_ref().unwrap().tab, view::SETTINGS_PLUGINS);
}

/// Connectors filter to the connected ones or the others, search by name and say when nothing
/// matches; a strip counts those that need the person, and "Reconnect all" reconnects them
/// in the sessions that loaded them and lists each agent's again.
#[test]
fn connectors_filter_search_and_reconnect_all() {
    use newen_model::{McpServer, McpSource, McpStatus};
    let (mut store, mut ui) = customize_connectors();
    agent(
        &mut store,
        AgentEvent::McpServers(vec![McpServer {
            name: "figma".into(),
            label: "figma".into(),
            status: McpStatus::NeedsAuth,
            source: McpSource::User,
            plugin: None,
            error: None,
            tools: vec![],
            sign_in: true,
        }]),
    );
    assert!(drawn(&store, &ui, "2 of your connectors need attention"));
    click(&store, &mut ui, "czp:filter:not");
    assert!(drawn(&store, &ui, "figma") && drawn(&store, &ui, "linear"));
    assert!(!drawn(&store, &ui, "github"));
    click(&store, &mut ui, "czp:filter:connected");
    assert!(drawn(&store, &ui, "github") && !drawn(&store, &ui, "figma"));
    click(&store, &mut ui, "czp:filter:all");
    update(
        &store,
        &mut ui,
        &UiAction::InputChanged {
            id: view::CZ_SEARCH.into(),
            value: "zzz".into(),
        },
    );
    assert!(drawn(&store, &ui, "No connectors match your search"));
    assert_eq!(
        click(&store, &mut ui, "czp:reconnect-all"),
        vec![
            Intent::McpReconnect {
                session: 1,
                name: "figma".into()
            },
            Intent::Servers {
                agent: "claude".into(),
                folder: "/work".into()
            },
            Intent::Servers {
                agent: "codex".into(),
                folder: "/work".into()
            },
        ]
    );
}

/// A connector's page asks its agent's program for its tools when no session said them; each
/// tool's choice, and a group's from its menu, is kept once, a moment after the last change;
/// a choice the agent cannot say is not taken.
#[test]
fn a_connectors_tool_permissions_are_kept_after_the_last_change() {
    use newen_model::{GlobalOp, ToolChoice, ToolPolicy};
    let (store, mut ui) = customize_connectors();
    assert_eq!(
        click(&store, &mut ui, "czp:open:srv:claude:github"),
        vec![Intent::ServerTools {
            agent: "claude".into(),
            folder: "/work".into()
        }]
    );
    assert!(drawn(&store, &ui, "Looking for its tools"));
    ui.applied(&Event::ServerTools {
        agent: "claude".into(),
        servers: vec![newen_model::McpServer {
            name: "github".into(),
            label: "github".into(),
            status: newen_model::McpStatus::Connected,
            source: newen_model::McpSource::User,
            plugin: None,
            error: None,
            tools: vec![
                tool_named("list_issues", true, false),
                tool_named("delete_repo", false, true),
                tool_named("merge", false, true),
            ],
            sign_in: false,
        }],
        error: None,
    });
    assert!(drawn(&store, &ui, "Read-only") && drawn(&store, &ui, "Write/delete"));
    assert!(drawn(&store, &ui, "Can run code without asking"));
    assert!(click(&store, &mut ui, "czp:tool:allow:list_issues").is_empty());
    click(&store, &mut ui, "czp:group:block:write");
    assert_eq!(
        customize::policy(&ui, "claude", "github", "merge"),
        Some(ToolPolicy::Block)
    );
    // Not yet: the last change was just now.
    assert!(ui.due().is_empty());
    ui.tool_changed = Some(std::time::Instant::now() - customize::KEEP_AFTER);
    let choice = |tool: &str, policy| ToolChoice {
        agent: "claude".into(),
        server: "github".into(),
        tool: tool.into(),
        policy: Some(policy),
    };
    assert_eq!(
        ui.due(),
        vec![Intent::Global(GlobalOp::Tools(vec![
            choice("list_issues", ToolPolicy::Allow),
            choice("delete_repo", ToolPolicy::Block),
            choice("merge", ToolPolicy::Block),
        ]))]
    );
    assert!(ui.tool_changes.is_empty() && ui.tool_changed.is_none());
    // An agent that cannot ask per tool: that choice is not taken.
    ui.agents[0].customize.tool_policies = vec![ToolPolicy::Allow, ToolPolicy::Block];
    click(&store, &mut ui, "czp:tool:ask:merge");
    assert!(ui.tool_changes.is_empty());
}

/// Skills: on and off from its row, in the agent's first and last states; a file chosen to
/// upload, to replace one, or to save one into; and deleting Newen's own.
#[test]
fn skills_switch_upload_replace_download_and_delete() {
    use newen_model::{ExtensionOp, GlobalOp};
    let (store, mut ui) = two_agents_on(view::SETTINGS_SKILLS);
    heard_extensions(
        &mut ui,
        "claude",
        skills_list(vec![skill("graphify", "user", "on")]),
    );
    assert_eq!(
        click(&store, &mut ui, "czp:skill-switch:claude:graphify"),
        vec![Intent::ExtensionOp {
            agent: "claude".into(),
            folder: "/work".into(),
            op: ExtensionOp::SkillState {
                skill: skill("graphify", "user", "on"),
                state: "off".into()
            }
        }]
    );
    // The host answers "Upload a file…" with the file the person chose.
    let upload = customize::file_action("upload", "");
    assert!(click(&store, &mut ui, &upload).is_empty());
    assert_eq!(
        customize::file_chosen(&mut ui, &upload, "/tmp/review.skill".into()),
        vec![Intent::Global(GlobalOp::UploadSkill {
            path: "/tmp/review.skill".into(),
            replace: None
        })]
    );
    assert_eq!(
        customize::file_chosen(
            &mut ui,
            &customize::file_action("replace", "review"),
            "/tmp/new.zip".into()
        ),
        vec![Intent::Global(GlobalOp::UploadSkill {
            path: "/tmp/new.zip".into(),
            replace: Some("review".into())
        })]
    );
    assert_eq!(
        customize::file_chosen(
            &mut ui,
            &customize::file_action("download", "review"),
            "/home/me/review.zip".into()
        ),
        vec![Intent::Global(GlobalOp::ExportSkill {
            name: "review".into(),
            to: "/home/me/review.zip".into()
        })]
    );
    ui.applied(&Event::Global(newen_model::GlobalState {
        skills: vec![newen_model::GlobalSkill {
            name: "review".into(),
            description: "Reviews".into(),
            except: vec![],
        }],
        ..Default::default()
    }));
    click(&store, &mut ui, "czp:open:skill::review");
    click(&store, &mut ui, "cz:more:skill-detail:review");
    assert!(drawn(&store, &ui, "Download") && drawn(&store, &ui, "Replace"));
    assert_eq!(
        click(&store, &mut ui, "czp:skill-delete:review"),
        vec![Intent::Global(GlobalOp::RemoveSkill("review".into()))]
    );
    assert!(ui.customize.as_ref().unwrap().detail.is_none());
}

fn skills_list(skills: Vec<newen_model::Skill>) -> newen_model::Extensions {
    newen_model::Extensions {
        skills,
        skill_states: vec![("on".into(), "On".into()), ("off".into(), "Off".into())],
        ..Default::default()
    }
}

/// A new session's page holds the greeting and the composer as one block, centered across
/// the column with the greeting a little over its middle, its "+" and send inside the card
/// and the chips and pickers under it; once the session exists the composer glides down to
/// its dock.
#[test]
fn a_new_page_centers_the_greeting_over_the_card_then_the_composer_docks() {
    use std::time::{Duration, Instant};
    let store = Store::default();
    let mut ui = Ui::new("/work".into());
    ui.skip_welcome = true;
    ui.name = Some("Ana".into());
    assert!(ui.sidebar.open);
    for (width, height) in [(2000.0, 1200.0), (900.0, 600.0)] {
        let window = limn::Size::new(width, height);
        let at = format!("{width}×{height}");
        let (left, top) = (view::sidebar_width(&ui), look::TOP_BAR + 1.0);
        let middle = (left + width) / 2.0;
        for laid_out in [false, true] {
            let mut tree = UiTree::new(view(&store, &ui, window));
            tree.set_text_layout(laid_out);
            tree.set_reduced_motion(true);
            let frame = tree.layout(window);
            let spans: Vec<_> = drawn_spans(&frame)
                .into_iter()
                .filter(|s| s.1 >= left)
                .collect();
            assert_apart(&spans, &at);
            let greeting = spans
                .iter()
                .find(|s| s.0.contains(", Ana"))
                .unwrap_or_else(|| panic!("{at}: no greeting in {spans:?}"));
            // The words alone, centered.
            let line = (greeting.1, greeting.3);
            assert!(
                ((line.0 + line.1) / 2.0 - middle).abs() < 2.0,
                "{at}: greeting {line:?} not centered on {middle}"
            );
            let share = (greeting.2 + greeting.4 - top) / (height - top);
            assert!(
                (0.36..=0.48).contains(&share),
                "{at}: greeting at {share} of the column"
            );
            assert!(greeting.2 > top + 20.0, "{at}: greeting at the top");
            let get = |id: &str| region(&frame, id).unwrap_or_else(|| panic!("{at}: no {id}"));
            let editor = get(view::COMPOSER);
            let card = editor.x + editor.width / 2.0;
            assert!((card - middle).abs() < 12.0, "{at}: card at {card}");
            assert!(
                editor.y >= greeting.2 + greeting.4 + 20.0,
                "{at}: card under the greeting"
            );
            assert!(
                editor.height >= 44.0,
                "{at}: editor {editor:?} under two lines"
            );
            let (plus, send) = (get(view::PLUS_MENU), get(view::SEND));
            for button in [plus, send] {
                assert!(
                    button.y >= editor.y + editor.height,
                    "{at}: {button:?} beside the editor"
                );
            }
            assert!(
                plus.x < editor.x + 20.0 && send.x + send.width > editor.x + editor.width - 20.0
            );
            for id in [
                view::FOLDER_MENU,
                view::MODE_MENU,
                view::MODEL_MENU,
                view::USAGE_MENU,
            ] {
                let r = get(id);
                assert!(
                    r.y >= send.y + send.height,
                    "{at}: {id} {r:?} not under the card"
                );
                assert!(
                    r.x >= left && r.x + r.width <= width + 0.5,
                    "{at}: {id} {r:?} outside"
                );
                assert!(
                    r.y + r.height <= height,
                    "{at}: {id} {r:?} below the window"
                );
            }
            let (folder, model) = (get(view::FOLDER_MENU), get(view::MODEL_MENU));
            assert!(folder.x < model.x, "{at}: chips not on the left");
        }
    }

    // The first message: the session starts and the composer docks at the bottom, gliding.
    let window = limn::Size::new(2000.0, 1200.0);
    let start = Instant::now();
    let mut tree = UiTree::new(view(&store, &ui, window));
    let centered = region(&tree.layout_at(window, start), view::COMPOSER)
        .unwrap()
        .y;
    update(
        &store,
        &mut ui,
        &UiAction::Submitted {
            id: view::COMPOSER.into(),
            value: "hi".into(),
        },
    );
    let mut store = store;
    let created = Event::SessionCreated {
        session: 7,
        agent: "claude".into(),
        cwd: "/work".into(),
    };
    store.apply(created.clone());
    ui.applied(&created);
    tree.set_root(view(&store, &ui, window));
    let at = |tree: &mut UiTree, ms| {
        let frame = tree.layout_at(window, start + Duration::from_millis(ms));
        region(&frame, view::COMPOSER).unwrap().y
    };
    assert_eq!(at(&mut tree, 0), centered);
    let midway = at(&mut tree, 60);
    let docked = at(&mut tree, 2000);
    assert!(
        centered < midway && midway < docked,
        "{centered} {midway} {docked}"
    );
    assert!(docked > 1200.0 - 140.0, "composer not docked: {docked}");
}

/// One frame of the window's input, as egui-winit 0.37 hands a Ctrl+V over (the key press
/// kept, its release left): limn-egui's translation, the tree, `update`, the view again.
fn egui_frame(
    store: &Store,
    ui: &mut Ui,
    tree: &mut UiTree,
    events: Vec<eframe::egui::Event>,
) -> Vec<UiAction> {
    use eframe::egui;
    let ctx = egui::Context::default();
    let mut translated = Vec::new();
    let mut output = ctx.run_ui(
        egui::RawInput {
            events,
            ..Default::default()
        },
        |ui| {
            limn_egui::normalize_keyboard_input(ui.ctx());
            translated = limn_egui::events(ui.ctx(), egui::Pos2::ZERO);
        },
    );
    output.textures_delta.clear();
    let mut actions = Vec::new();
    for event in translated {
        for action in tree.handle_event(event) {
            update(store, ui, &action);
            actions.push(action);
        }
    }
    tree.set_root(view(store, ui, BIG_WINDOW));
    tree.layout(BIG_WINDOW);
    actions
}

fn ctrl_v_release() -> eframe::egui::Event {
    eframe::egui::Event::Key {
        key: eframe::egui::Key::V,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: eframe::egui::Modifiers::CTRL,
    }
}

/// Text pasted into `field`, focused in the window `store` and `ui` show.
fn paste_into(store: &Store, ui: &mut Ui, field: &str, text: &str) {
    let mut tree = UiTree::new(view(store, ui, BIG_WINDOW));
    tree.set_reduced_motion(true);
    tree.layout(BIG_WINDOW);
    assert!(tree.focus(field), "{field} takes the focus");
    egui_frame(
        store,
        ui,
        &mut tree,
        vec![eframe::egui::Event::Paste(text.into()), ctrl_v_release()],
    );
    // A frame later, as the window goes on: the composer keeps what was pasted.
    egui_frame(store, ui, &mut tree, vec![]);
}

#[test]
fn text_pastes_into_the_composer_of_a_new_conversation() {
    let store = Store::default();
    let mut ui = Ui::new("/work".into());
    ui.skip_welcome = true;
    paste_into(&store, &mut ui, view::COMPOSER, "pegado ñ\nsecond line");
    assert_eq!(ui.prompt, "pegado ñ\nsecond line");
}

#[test]
fn text_pastes_into_the_composer_of_a_session() {
    let (store, mut ui) = store_with_session();
    paste_into(&store, &mut ui, view::COMPOSER, "pasted");
    assert_eq!(ui.prompt, "pasted");
}

#[test]
fn text_pastes_into_the_title_and_the_search() {
    let (store, mut ui) = store_with_session();
    ui.title_edit = Some(String::new());
    paste_into(&store, &mut ui, view::TITLE_FIELD, "New title");
    assert_eq!(ui.title_edit.as_deref(), Some("New title"));
    let (store, mut ui) = store_with_session();
    ui.search = Some(String::new());
    paste_into(&store, &mut ui, view::SEARCH_FIELD, "bug");
    assert_eq!(ui.search.as_deref(), Some("bug"));
}

/// An image copied with no text: egui-winit hands it over as a pasted picture, which joins the
/// composer's images (the host saves it, off the window's thread).
#[test]
fn a_pasted_picture_is_the_composer_s() {
    let (store, mut ui) = store_with_session();
    let mut tree = UiTree::new(view(&store, &ui, BIG_WINDOW));
    tree.set_reduced_motion(true);
    tree.layout(BIG_WINDOW);
    let image = eframe::egui::ColorImage::new([1, 1], vec![eframe::egui::Color32::RED]);
    let paste = || {
        vec![eframe::egui::Event::PasteImage(std::sync::Arc::new(
            image.clone(),
        ))]
    };
    // Nowhere to put it with the composer not focused.
    let actions = egui_frame(&store, &mut ui, &mut tree, paste());
    assert!(actions.iter().all(|a| pasted_image(a).is_none()));
    assert!(tree.focus(view::COMPOSER));
    let actions = egui_frame(&store, &mut ui, &mut tree, paste());
    let pictures: Vec<_> = actions.iter().filter_map(pasted_image).collect();
    assert_eq!(pictures, [(1, 1, &std::sync::Arc::from([255, 0, 0, 255]))]);
}

/// A session whose answer has paragraphs, a list with a link and code.
fn answered() -> (Store, Ui) {
    let (mut store, ui) = store_with_session();
    agent(
        &mut store,
        AgentEvent::Text {
            text: "First paragraph.\n\nSecond paragraph:\n\n- one\n- see [docs](https://example.com)\n\n```\nlet a = 1;\n```\n\nLast words."
                .into(),
            parent: None,
            id: Some("m1".into()),
            at: None,
        },
    );
    (store, ui)
}

/// The tree of the window `store` and `ui` show, laid out, and the start and end of the
/// laid-out text that starts with `words`.
fn text_ends(tree: &mut UiTree, words: &str) -> (limn::Point, limn::Point) {
    let frame = tree.layout(BIG_WINDOW);
    let block = frame
        .text_layouts()
        .into_iter()
        .find(|block| block.source.starts_with(words))
        .unwrap_or_else(|| panic!("no text starts with {words:?}"));
    let (first, last) = (block.runs[0].rect, block.runs[block.runs.len() - 1].rect);
    (
        limn::Point::new(first.x + 1.0, first.y + first.height / 2.0),
        limn::Point::new(last.x + last.width - 0.5, last.y + last.height / 2.0),
    )
}

fn transcript_tree(store: &Store, ui: &Ui) -> UiTree {
    let mut tree = UiTree::new(view(store, ui, BIG_WINDOW));
    tree.set_reduced_motion(true);
    tree.set_text_measurer(|text, _, _| text.chars().count() as f32 * 7.0);
    tree.layout(BIG_WINDOW);
    tree
}

const ANSWER_COPIED: &str =
    "First paragraph.\n\nSecond paragraph:\n\n• one\n• see docs\n\nlet a = 1;\n\nLast words.";

#[test]
fn a_drag_selects_the_answer_across_its_paragraphs_and_ctrl_c_copies_it() {
    let (store, ui) = answered();
    let mut tree = transcript_tree(&store, &ui);
    let (start, _) = text_ends(&mut tree, "First paragraph");
    let (_, end) = text_ends(&mut tree, "Last words");
    tree.handle_event(limn::UiEvent::PointerDown(start));
    tree.handle_event(limn::UiEvent::PointerMove(end));
    tree.handle_event(limn::UiEvent::PointerUp(end));
    tree.layout(BIG_WINDOW);
    assert_eq!(tree.selected_text().as_deref(), Some(ANSWER_COPIED));
    let copy = limn::UiEvent::Key {
        key: limn::Key::C,
        shift: false,
        ctrl: true,
        alt: false,
    };
    assert_eq!(
        tree.handle_event(copy),
        vec![UiAction::CopyRequested {
            text: ANSWER_COPIED.into()
        }]
    );
}

#[test]
fn shift_and_a_press_extend_the_selection_from_the_question_into_the_answer() {
    let (store, ui) = answered();
    let mut tree = transcript_tree(&store, &ui);
    let (start, _) = text_ends(&mut tree, "Fix the bug");
    tree.handle_event(limn::UiEvent::PointerDown(start));
    tree.handle_event(limn::UiEvent::PointerUp(start));
    tree.layout(BIG_WINDOW);
    let (_, end) = text_ends(&mut tree, "Second paragraph");
    tree.handle_event(limn::UiEvent::PointerDownModified {
        position: end,
        shift: true,
    });
    tree.handle_event(limn::UiEvent::PointerUp(end));
    tree.layout(BIG_WINDOW);
    let selected = tree.selected_text().unwrap_or_default();
    assert!(selected.starts_with("Fix the bug\n\n"), "{selected:?}");
    assert!(
        selected.ends_with("First paragraph.\n\nSecond paragraph:"),
        "{selected:?}"
    );
}

#[test]
fn ctrl_a_in_the_transcript_selects_all_of_it_and_not_the_composer() {
    let (store, ui) = answered();
    let mut tree = transcript_tree(&store, &ui);
    let (point, _) = text_ends(&mut tree, "Second paragraph");
    tree.handle_event(limn::UiEvent::PointerDown(point));
    tree.handle_event(limn::UiEvent::PointerUp(point));
    tree.layout(BIG_WINDOW);
    tree.handle_event(limn::UiEvent::Key {
        key: limn::Key::A,
        shift: false,
        ctrl: true,
        alt: false,
    });
    tree.layout(BIG_WINDOW);
    let selected = tree.selected_text().unwrap_or_default();
    assert!(
        selected.starts_with("Started on this computer"),
        "{selected:?}"
    );
    assert!(selected.contains("Fix the bug\n\n"), "{selected:?}");
    assert!(selected.contains(ANSWER_COPIED), "{selected:?}");
    // In the composer, Ctrl+A is the draft's own.
    assert!(tree.focus(view::COMPOSER));
    tree.handle_event(limn::UiEvent::Text("draft".into()));
    let ctrl = |key| limn::UiEvent::Key {
        key,
        shift: false,
        ctrl: true,
        alt: false,
    };
    tree.handle_event(ctrl(limn::Key::A));
    assert_eq!(
        tree.handle_event(ctrl(limn::Key::C)),
        vec![UiAction::CopyRequested {
            text: "draft".into()
        }]
    );
}

#[test]
fn a_link_in_a_selectable_answer_still_opens() {
    let (store, mut ui) = answered();
    let mut tree = transcript_tree(&store, &ui);
    let frame = tree.layout(BIG_WINDOW);
    let link = frame
        .regions
        .iter()
        .find(|region| view::link_url(&region.id).is_some_and(|url| url.contains("example.com")))
        .expect("the link")
        .rect;
    let at = limn::Point::new(link.x + 2.0, link.y + link.height / 2.0);
    let mut actions = tree.handle_event(limn::UiEvent::PointerDown(at));
    actions.extend(tree.handle_event(limn::UiEvent::PointerUp(at)));
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, UiAction::Clicked { id } if view::link_url(id).is_some())),
        "{actions:?}"
    );
    for action in &actions {
        update(&store, &mut ui, action);
    }
    assert_eq!(tree.selected_text(), None);
}

#[test]
fn a_proposal_for_another_agent_names_it_on_its_card() {
    let (_, mut ui) = two_agents();
    let (mut store, _) = store_with_session();
    ui.selected = Some(1);
    store.apply(Event::Proposed {
        session: 1,
        proposal: newen_model::Proposal {
            id: "a".into(),
            title: "Port it".into(),
            prompt: "Do it".into(),
            background: None,
            fork: false,
            agent: Some("codex".into()),
        },
    });
    assert!(drawn_words(&store, &ui, "Runs in Codex"));
}

/// The layer chip, pressed as a person does through the tree, opens its menu with every
/// agent; choosing one closes it and shows that agent's connectors only.
#[test]
fn the_layer_chip_opens_its_menu_and_an_agent_chosen_filters_the_page() {
    let (store, mut ui) = customize_connectors();
    let window = limn::Size::new(1408.0, 844.0);
    let mut tree = UiTree::new(view(&store, &ui, window));
    tree.set_reduced_motion(true);
    let frame = tree.layout(window);
    let press = |tree: &mut UiTree, ui: &mut Ui, frame: &limn::Frame, id: &str| {
        let rect = region(frame, id).unwrap_or_else(|| panic!("{id} is not on screen"));
        let at = limn::Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        let mut actions = tree.handle_event(limn::UiEvent::PointerDown(at));
        actions.extend(tree.handle_event(limn::UiEvent::PointerUp(at)));
        for action in actions {
            update(&store, ui, &action);
        }
        tree.set_root(view(&store, ui, window));
        tree.layout(window)
    };
    assert!(region(&frame, "czp:layer:=codex").is_none());
    // Nothing else in the header: Settings is the sidebar's.
    assert!(region(&frame, "czp:settings").is_none());
    let frame = press(&mut tree, &mut ui, &frame, "cz:layer");
    assert_eq!(ui.settings_menu.as_deref(), Some("cz:layer"));
    assert!(region(&frame, "czp:layer:=").is_some(), "All agents");
    assert!(region(&frame, "czp:layer:=claude").is_some(), "Claude Code");
    let frame = press(&mut tree, &mut ui, &frame, "czp:layer:=codex");
    assert_eq!(ui.settings_menu, None);
    let page = ui.customize.as_ref().expect("the page");
    assert_eq!(page.layer.as_deref(), Some("codex"));
    assert_eq!(page.tab, view::SETTINGS_CONNECTORS);
    assert!(region(&frame, "czp:open:srv:codex:linear").is_some());
    assert!(region(&frame, "czp:open:srv:claude:github").is_none());
    assert!(drawn(&store, &ui, "Codex") && !drawn(&store, &ui, "All agents 0"));
    // Skills' Sort menu opens the same way.
    click(
        &store,
        &mut ui,
        &format!("czp:tab:{}", view::SETTINGS_SKILLS),
    );
    click(&store, &mut ui, "cz:sort");
    assert_eq!(ui.settings_menu.as_deref(), Some("cz:sort"));
}

/// The strip of connectors that need the person is amber through and through: its words and
/// its "Reconnect all" in the amber ink, on a translucent amber fill inside an amber edge.
#[test]
fn the_attention_strip_is_amber() {
    let (store, ui) = customize_connectors();
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let ink = |words: &str| {
        frame.commands.iter().find_map(|c| match c {
            limn::DrawCommand::Text { text, color, .. } if text.contains(words) => Some(*color),
            _ => None,
        })
    };
    assert_eq!(ink("need attention"), Some(look::manual()));
    assert_eq!(ink("Reconnect all"), Some(look::manual()));
    let amber = look::awaiting();
    let fill = frame.commands.iter().any(|c| {
        matches!(c, limn::DrawCommand::FillRect { color, rect, .. }
            if (color.r, color.g, color.b) == (amber.r, amber.g, amber.b)
                && color.a < 255 && rect.width > 600.0)
    });
    assert!(fill, "a translucent amber fill");
    let edge = frame.commands.iter().any(|c| {
        matches!(c, limn::DrawCommand::StrokeRect { color, rect, .. }
            if *color == amber && rect.width > 600.0)
    });
    assert!(edge, "an amber edge");
}

/// Over Connectors' list: the title with its chip, the tabs, and one row of tools (search,
/// the status filter as a menu, Discover, Add); no second row of tabs. Discover is the
/// button, and a button at the row's start leads back to the list.
#[test]
fn connectors_have_one_row_of_tools_and_discover_leads_back() {
    let (store, mut ui) = customize_connectors();
    let window = limn::Size::new(1408.0, 844.0);
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    let top = |id: &str| region(&frame, id).unwrap_or_else(|| panic!("{id}")).y;
    let row = top(view::CZ_SEARCH);
    for id in ["cz:status", "srv:tab:discover", "cz:add:connectors"] {
        assert!((top(id) - row).abs() < 8.0, "{id} is off the search's row");
    }
    for gone in ["srv:tab:yours", "czp:filter:all", "czp:filter:connected"] {
        assert!(region(&frame, gone).is_none(), "{gone} still shows");
    }
    // The status filter is a menu: open, it chooses; chosen, it closes and filters.
    click(&store, &mut ui, "cz:status");
    assert!(drawn(&store, &ui, "Not connected"));
    click(&store, &mut ui, "czp:filter:connected");
    assert_eq!(ui.settings_menu, None);
    assert_eq!(
        ui.customize.as_ref().unwrap().filter,
        customize::Filter::Connected
    );
    assert!(drawn(&store, &ui, "github") && !drawn(&store, &ui, "figma"));
    // Discover, then back.
    click(&store, &mut ui, "srv:tab:discover");
    let frame = UiTree::new(view(&store, &ui, window)).layout(window);
    assert!(region(&frame, "srv:tab:yours").is_some(), "the way back");
    click(&store, &mut ui, "srv:tab:yours");
    assert!(!ui.connectors.discover);
}

/// Customize with 300 connectors and 300 skills for each agent: a frame builds and lays out
/// about the rows in view, not all of them, in a bounded time.
#[test]
fn a_long_customize_page_lays_out_only_the_rows_in_view() {
    use newen_model::McpStatus;
    let (store, mut ui) = two_agents_on(view::SETTINGS_CONNECTORS);
    for agent in ["claude", "codex"] {
        ui.applied(&Event::Servers {
            agent: agent.into(),
            folder: "/work".into(),
            servers: (0..300)
                .map(|n| {
                    let status = if n % 7 == 0 {
                        McpStatus::NeedsAuth
                    } else {
                        McpStatus::Connected
                    };
                    configured(&format!("conn-{agent}-{n:03}"), "npx -y x", status)
                })
                .collect(),
            scopes: vec![],
            error: None,
        });
        heard_extensions(
            &mut ui,
            agent,
            skills_list(
                (0..300)
                    .map(|n| skill(&format!("skill-{agent}-{n:03}"), "user", "on"))
                    .collect(),
            ),
        );
    }
    let window = limn::Size::new(1408.0, 844.0);
    // A frame as the app makes one: the view built, then laid out with real text.
    let rows = |store: &Store, ui: &Ui| {
        let mut tree = UiTree::new(view(store, ui, window));
        tree.set_text_layout(true);
        tree.layout(window);
        let started = std::time::Instant::now();
        customize::BUILT.with(|b| b.set(0));
        for _ in 0..5 {
            tree.set_root(view(store, ui, window));
            tree.layout(window);
        }
        (
            customize::BUILT.with(|b| b.get()) / 5,
            started.elapsed() / 5,
        )
    };
    let (connectors, connectors_time) = rows(&store, &ui);
    click(
        &store,
        &mut ui,
        &format!("czp:tab:{}", view::SETTINGS_SKILLS),
    );
    let (skills, skills_time) = rows(&store, &ui);
    eprintln!("connectors: {connectors} blocks built a frame, in {connectors_time:?}");
    eprintln!("skills: {skills} blocks built a frame, in {skills_time:?}");
    // Of more than 600 rows, the 15 or so that fit in 844, with the headings among them.
    assert!((5..=40).contains(&connectors), "{connectors} built");
    assert!((5..=40).contains(&skills), "{skills} built");
    // Before the page built only what shows, a frame took about 25 ms here, unoptimized.
    let most = std::time::Duration::from_millis(if cfg!(debug_assertions) { 25 } else { 10 });
    assert!(connectors_time < most, "{connectors_time:?} a frame");
    assert!(skills_time < most, "{skills_time:?} a frame");
}
