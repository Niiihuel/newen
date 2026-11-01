//! The engine driving a fake agent that replays real captures (crates/quill-agent-claude/fixtures).

use quill_engine::Engine;
use quill_model::{AgentEvent, Decision, Event, Intent, Lane, NewSession, SessionStatus};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

fn fixture(name: &str) -> String {
    format!(
        "{}/../quill-agent-claude/fixtures/2.1.310/{name}.jsonl",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn engine() -> (Engine, Receiver<Event>) {
    let (tx, rx) = channel();
    let programs = HashMap::from([(
        "claude".to_string(),
        env!("CARGO_BIN_EXE_fake-agent").to_string(),
    )]);
    let config = quill_engine::Config {
        programs,
        database: None,
        snapshots: None,
        settings: None,
        ..Default::default()
    };
    let engine = Engine::start_with(move |event| drop(tx.send(event)), config);
    (engine, rx)
}

/// A working directory whose `.fake-agent` names the capture the fake agent replays.
fn workdir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "quill-engine-{}-{name}-{}",
        std::process::id(),
        uuid_like()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".fake-agent"), fixture(name)).unwrap();
    dir
}

fn uuid_like() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn new_session(engine: &Engine, name: &str) {
    engine.send(Intent::NewSession(NewSession {
        agent: "claude".into(),
        cwd: workdir(name),
        model: None,
        effort: None,
        mode: None,
    }));
}

/// Collect events until `done` matches one, failing after a few seconds.
fn until(rx: &Receiver<Event>, mut done: impl FnMut(&Event) -> bool) -> Vec<Event> {
    let mut seen = vec![];
    loop {
        let event = rx
            .recv_timeout(Duration::from_secs(10))
            .unwrap_or_else(|_| panic!("timed out after {seen:#?}"));
        let stop = done(&event);
        seen.push(event);
        if stop {
            return seen;
        }
    }
}

#[test]
fn a_message_keeps_its_images_and_a_text_file_among_them_is_left_out() {
    let (engine, rx) = engine();
    let dir = workdir("simple");
    engine.send(Intent::NewSession(NewSession {
        agent: "claude".into(),
        cwd: dir.clone(),
        model: None,
        effort: None,
        mode: None,
    }));
    let picture = dir.join("picture.png");
    image::RgbImage::new(4, 4).save(&picture).unwrap();
    let notes = dir.join("notes.txt");
    std::fs::write(&notes, "words").unwrap();
    engine.send(Intent::Send {
        session: 1,
        text: "look".into(),
        mentions: vec![],
        images: vec![picture.clone(), notes.clone()],
        lane: Lane::Steer,
    });
    let events = until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    let queued = events.iter().find_map(|e| match e {
        Event::Queued { images, .. } => Some(images.clone()),
        _ => None,
    });
    assert_eq!(queued, Some(vec![picture, notes]));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Notice { text, .. }
            if text == "Only PNG, JPEG, GIF, and WebP images are supported."
    )));
}

#[test]
fn a_tool_s_pictures_are_on_disk_and_stored_before_the_ui_hears_of_them() {
    use base64::Engine as _;
    let root = std::env::temp_dir().join(format!(
        "quill-engine-{}-pictures-{}",
        std::process::id(),
        uuid_like()
    ));
    let (database, pictures) = (root.join("quill.db"), root.join("pictures"));
    // simple.jsonl's turn, in which a tool answers with a picture before the turn's result.
    let mut png = std::io::Cursor::new(vec![]);
    image::RgbImage::new(4, 3)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let png = png.into_inner();
    let data = base64::engine::general_purpose::STANDARD.encode(&png);
    let call = serde_json::json!({ "dir": "in", "msg": { "type": "assistant", "parent_tool_use_id": null,
        "message": { "role": "assistant", "content": [
            { "type": "tool_use", "id": "t1", "name": "mcp__shots__take", "input": {} }] } } });
    let answer = serde_json::json!({ "dir": "in", "msg": { "type": "user", "parent_tool_use_id": null,
        "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "t1",
            "content": [{ "type": "text", "text": "Captured" }, { "type": "image",
                "source": { "type": "base64", "media_type": "image/png", "data": data } }] }] } } });
    let mut lines = vec![];
    for line in std::fs::read_to_string(fixture("simple")).unwrap().lines() {
        let entry: serde_json::Value = serde_json::from_str(line).unwrap();
        if entry["msg"]["type"] == "result" {
            lines.extend([call.to_string(), answer.to_string()]);
        }
        lines.push(line.to_owned());
    }
    let dir = workdir("simple");
    let capture = dir.join("pictures.jsonl");
    std::fs::write(&capture, lines.join("\n")).unwrap();
    std::fs::write(
        dir.join(".fake-agent"),
        capture.to_string_lossy().as_bytes(),
    )
    .unwrap();
    // The sink looks for each picture's file as the event reaches it.
    let (tx, rx) = channel();
    let config = quill_engine::Config {
        programs: HashMap::from([(
            "claude".to_string(),
            env!("CARGO_BIN_EXE_fake-agent").to_string(),
        )]),
        database: Some(database.clone()),
        pictures: Some(pictures.clone()),
        ..Default::default()
    };
    let engine = Engine::start_with(
        move |event| {
            let on_disk = match &event {
                Event::Agent {
                    event: AgentEvent::ToolResult { images, .. },
                    ..
                } => images.iter().all(|path| path.is_file()),
                _ => true,
            };
            drop(tx.send((event, on_disk)))
        },
        config,
    );
    engine.send(Intent::NewSession(NewSession {
        agent: "claude".into(),
        cwd: dir,
        model: None,
        effort: None,
        mode: None,
    }));
    engine.send(Intent::Send {
        session: 1,
        text: "Take a screenshot".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    let (images, on_disk) = loop {
        let (event, on_disk) = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the tool's result");
        if let Event::Agent {
            event: AgentEvent::ToolResult { images, .. },
            ..
        } = event
        {
            break (images, on_disk);
        }
    };
    assert!(on_disk, "the picture was written before the UI heard of it");
    assert_eq!(images.len(), 1);
    assert!(images[0].starts_with(pictures.join("1")));
    assert_eq!(std::fs::read(&images[0]).unwrap(), png);
    // Stored with the result as its path; the picture itself is not in the record.
    let stored = quill_store::Store::open(&database).unwrap().load().unwrap();
    let kept = (stored.iter().flat_map(|(_, events)| events)).find_map(|event| match event {
        Event::Agent {
            event: AgentEvent::ToolResult { images, inline, .. },
            ..
        } => Some((images.clone(), inline.is_empty())),
        _ => None,
    });
    assert_eq!(kept, Some((images, true)));
    drop(engine);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sessions_stream_turns_and_wait_for_decisions() {
    let (engine, rx) = engine();

    new_session(&engine, "simple");
    engine.send(Intent::Send {
        session: 1,
        text: "Reply with exactly the word: ok".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    let events = until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    let request = events.iter().find_map(|e| {
        if let Event::Queued { request, .. } = e {
            Some(request.clone())
        } else {
            None
        }
    });
    assert!(request.is_some());
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Status {
            status: SessionStatus::Running,
            ..
        }
    )));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Agent {
            event: AgentEvent::TextDelta { .. },
            request: Some(_),
            ..
        }
    )));
    until(&rx, |e| {
        matches!(
            e,
            Event::Status {
                session: 1,
                status: SessionStatus::Idle
            }
        )
    });

    new_session(&engine, "deny");
    engine.send(Intent::Send {
        session: 2,
        text: "Use the Bash tool to run exactly: touch denied.txt".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    let events = until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::PermissionRequest { .. },
                ..
            }
        )
    });
    let Some(Event::Agent {
        event: AgentEvent::PermissionRequest { id, .. },
        ..
    }) = events.last()
    else {
        unreachable!()
    };
    // The session already waits for the person when the card arrives.
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Status {
            session: 2,
            status: SessionStatus::Awaiting
        }
    )));
    engine.send(Intent::Decide {
        session: 2,
        permission: id.clone(),
        decision: Decision::Deny {
            message: "no".into(),
        },
    });
    let events = until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                session: 2,
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Agent {
            event: AgentEvent::ToolResult { is_error: true, .. },
            ..
        }
    )));
}

#[test]
fn a_kept_session_comes_back_and_continues() {
    let database = std::env::temp_dir().join(format!("quill-test-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&database);
    let start = || {
        let (tx, rx) = channel();
        let programs = HashMap::from([(
            "claude".to_string(),
            env!("CARGO_BIN_EXE_fake-agent").to_string(),
        )]);
        let config = quill_engine::Config {
            programs,
            database: Some(database.clone()),
            snapshots: None,
            settings: None,
            ..Default::default()
        };
        (
            Engine::start_with(move |event| drop(tx.send(event)), config),
            rx,
        )
    };
    let cwd = workdir("simple");

    let (engine, rx) = start();
    engine.send(Intent::NewSession(NewSession {
        agent: "claude".into(),
        cwd: cwd.clone(),
        model: None,
        effort: None,
        mode: None,
    }));
    engine.send(Intent::Send {
        session: 1,
        text: "Reply with exactly the word: ok".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    drop(engine);

    let (engine, rx) = start();
    let replayed = until(&rx, |e| {
        matches!(
            e,
            Event::Status {
                session: 1,
                status: SessionStatus::Exited
            }
        )
    });
    assert!(matches!(
        replayed.first(),
        Some(Event::SessionCreated { session: 1, .. })
    ));
    // The history is told before `Restored`, so the host notifies only what follows it.
    let mut told = replayed.clone();
    if !told.iter().any(|e| matches!(e, Event::Restored)) {
        told.extend(until(&rx, |e| matches!(e, Event::Restored)));
    }
    let at = |wanted: fn(&Event) -> bool| told.iter().position(wanted);
    assert!(
        at(|e| matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )) < at(|e| matches!(e, Event::Restored))
    );
    assert!(replayed
        .iter()
        .any(|e| matches!(e, Event::Queued { text, .. } if text.contains("ok"))));
    assert!(replayed.iter().any(|e| matches!(
        e,
        Event::Agent {
            event: AgentEvent::Text { .. },
            ..
        }
    )));
    assert!(!replayed.iter().any(|e| matches!(
        e,
        Event::Agent {
            event: AgentEvent::TextDelta { .. },
            ..
        }
    )));
    // The kept session takes a new message and runs again (resuming its conversation).
    engine.send(Intent::Send {
        session: 1,
        text: "again".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                session: 1,
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    let _ = std::fs::remove_file(&database);
}

#[test]
fn a_request_is_attributed_and_its_undo_puts_files_back() {
    let (tx, rx) = channel();
    let programs = HashMap::from([(
        "claude".to_string(),
        env!("CARGO_BIN_EXE_fake-agent").to_string(),
    )]);
    let snapshots = std::env::temp_dir().join(format!("quill-snapshots-{}", uuid_like()));
    let config = quill_engine::Config {
        programs,
        database: None,
        snapshots: Some(snapshots.clone()),
        settings: None,
        ..Default::default()
    };
    let engine = Engine::start_with(move |event| drop(tx.send(event)), config);
    let cwd = workdir("rewind");
    engine.send(Intent::NewSession(NewSession {
        agent: "claude".into(),
        cwd: cwd.clone(),
        model: None,
        effort: None,
        mode: None,
    }));
    engine.send(Intent::Send {
        session: 1,
        text: "Create rewind.txt containing the line one.".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });

    let events = until(&rx, |e| matches!(e, Event::Changes { .. }));
    let Some(Event::Changes { request, files, .. }) = events.last() else {
        unreachable!()
    };
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(
        (files[0].path.as_str(), &files[0].kind),
        ("rewind.txt", &quill_model::ChangeKind::Added)
    );
    assert!(cwd.join("rewind.txt").exists());

    engine.send(Intent::Rewind {
        session: 1,
        request: request.clone(),
    });
    let events = until(&rx, |e| matches!(e, Event::Rewound { .. }));
    assert!(matches!(events.last(), Some(Event::Rewound { files, .. }) if files.len() == 1));
    assert!(!cwd.join("rewind.txt").exists());
    // The marker the fake agent reads stays: the rewind undoes only what the request did.
    assert!(cwd.join(".fake-agent").exists());
    let _ = std::fs::remove_dir_all(&snapshots);
}

#[test]
fn a_name_and_a_pin_are_kept_and_a_deleted_session_is_gone() {
    let database = std::env::temp_dir().join(format!("quill-test-delete-{}.db", uuid_like()));
    let start = || {
        let (tx, rx) = channel();
        let programs = HashMap::from([(
            "claude".to_string(),
            env!("CARGO_BIN_EXE_fake-agent").to_string(),
        )]);
        let config = quill_engine::Config {
            programs,
            database: Some(database.clone()),
            snapshots: None,
            settings: None,
            ..Default::default()
        };
        (
            Engine::start_with(move |event| drop(tx.send(event)), config),
            rx,
        )
    };

    let (engine, rx) = start();
    new_session(&engine, "simple");
    engine.send(Intent::Send {
        session: 1,
        text: "Reply with exactly the word: ok".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    engine.send(Intent::Rename {
        session: 1,
        title: "Mine".into(),
    });
    engine.send(Intent::Pin {
        session: 1,
        pinned: true,
    });
    engine.send(Intent::Archive {
        session: 1,
        archived: true,
    });
    engine.send(Intent::MarkUnread {
        session: 1,
        unread: true,
    });
    until(&rx, |e| matches!(e, Event::Unread { .. }));
    // A fork continues the conversation as a new session that runs on its own.
    engine.send(Intent::Fork {
        session: 1,
        at: None,
    });
    let forked = until(&rx, |e| matches!(e, Event::Forked { .. }));
    assert!(forked
        .iter()
        .any(|e| matches!(e, Event::SessionCreated { session: 2, .. })));
    assert!(matches!(
        forked.last(),
        Some(Event::Forked {
            session: 2,
            from: 1,
            at: None
        })
    ));
    engine.send(Intent::Send {
        session: 2,
        text: "again".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                session: 2,
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    engine.send(Intent::Delete { session: 2 });
    until(&rx, |e| matches!(e, Event::Deleted { session: 2 }));
    drop(engine);

    let (engine, rx) = start();
    let replayed = until(&rx, |e| matches!(e, Event::Status { session: 1, .. }));
    assert!(replayed
        .iter()
        .any(|e| matches!(e, Event::Renamed { title, .. } if title == "Mine")));
    assert!(replayed
        .iter()
        .any(|e| matches!(e, Event::Pinned { pinned: true, .. })));
    assert!(replayed
        .iter()
        .any(|e| matches!(e, Event::Archived { archived: true, .. })));
    assert!(replayed
        .iter()
        .any(|e| matches!(e, Event::Unread { unread: true, .. })));
    // A running agent is stopped and its record removed.
    engine.send(Intent::Send {
        session: 1,
        text: "again".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    engine.send(Intent::Delete { session: 1 });
    until(&rx, |e| matches!(e, Event::Deleted { session: 1 }));
    drop(engine);

    let (engine, rx) = start();
    new_session(&engine, "simple");
    // The settings and the end of the retelling come at every start; they are no session's.
    let events: Vec<Event> = until(&rx, |e| matches!(e, Event::SessionCreated { .. }))
        .into_iter()
        .filter(|e| !matches!(e, Event::Settings { .. } | Event::Restored))
        .collect();
    assert_eq!(
        events.len(),
        1,
        "nothing of the deleted session came back: {events:#?}"
    );
    drop(engine);
    let _ = std::fs::remove_file(&database);
}

#[test]
fn a_proposed_task_starts_a_linked_session_that_reports_back() {
    let (engine, rx) = engine();
    new_session(&engine, "sessions");
    engine.send(Intent::Send {
        session: 1,
        text: "Propose a README task".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    // The agent's start_session call becomes a proposal; nothing runs yet.
    let events = until(&rx, |e| matches!(e, Event::Proposed { session: 1, .. }));
    let Some(Event::Proposed { proposal, .. }) = events.last() else {
        unreachable!()
    };
    assert_eq!(proposal.title, "Add a README");
    let task = proposal.id.clone();
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                session: 1,
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    // Started, it is a session of its own under the one that proposed it.
    engine.send(Intent::AnswerProposal {
        session: 1,
        task: task.clone(),
        answer: quill_model::ProposalAnswer::Start,
    });
    let events = until(&rx, |e| matches!(e, Event::ProposalAnswered { .. }));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Linked {
            session: 2,
            parent: 1,
            side: false
        }
    )));
    assert!(matches!(
        events.last(),
        Some(Event::ProposalAnswered {
            session: 1,
            outcome: quill_model::ProposalOutcome::Started { session: 2 },
            ..
        })
    ));
    // When its turn ends, what it said goes back to the one that started it.
    let events = until(&rx, |e| matches!(e, Event::Reported { session: 1, .. }));
    let Some(Event::Reported { reports, .. }) = events.last() else {
        unreachable!()
    };
    assert_eq!(reports[0].from, 2);
    assert_eq!(reports[0].title, "Add a README");
    assert_eq!(reports[0].text, "done");
}

#[test]
fn a_side_question_forks_the_session_beside_it() {
    let (engine, rx) = engine();
    new_session(&engine, "simple");
    engine.send(Intent::Send {
        session: 1,
        text: "Reply with exactly the word: ok".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                session: 1,
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    engine.send(Intent::SideChat {
        session: 1,
        text: "what is this?".into(),
    });
    let events = until(&rx, |e| matches!(e, Event::Queued { session: 2, .. }));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Linked {
            session: 2,
            parent: 1,
            side: true
        }
    )));
    assert!(events
        .iter()
        .any(|e| matches!(e, Event::Renamed { session: 2, title } if title == "what is this?")));
}

/// Events until the steer capture's turn is under way: its agent said it is ready and the
/// prompt was written. The capture then holds the turn open at its steer line, until something
/// is written there, so a queue has a busy turn to wait behind.
fn busy_steer_session(engine: &Engine, rx: &Receiver<Event>) {
    new_session(engine, "steer");
    // The agent starts with the first message; its `Ready` says whether it steers.
    engine.send(Intent::Send {
        session: 1,
        text: "Count from 1 to 40".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::Ready { .. },
                ..
            }
        )
    });
}

fn queue_send(engine: &Engine, text: &str) {
    engine.send(Intent::Send {
        session: 1,
        text: text.into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Queue,
    });
}

/// The ids of the queue as the engine last said it, after `done` matched its change.
fn queue_ids(events: &[Event]) -> Vec<(String, String)> {
    let Some(Event::QueueChanged { entries, .. }) = events
        .iter()
        .rev()
        .find(|e| matches!(e, Event::QueueChanged { .. }))
    else {
        panic!("no QueueChanged in {events:#?}");
    };
    entries
        .iter()
        .map(|m| (m.id.clone(), m.text.clone()))
        .collect()
}

#[test]
fn a_queued_message_is_not_sent_mid_turn_and_goes_out_when_the_turn_ends() {
    let (engine, rx) = engine();
    busy_steer_session(&engine, &rx);
    queue_send(&engine, "later");
    let events = until(
        &rx,
        |e| matches!(e, Event::QueueChanged { entries, .. } if entries.len() == 1),
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::Queued { text, .. } if text == "later")));
    engine.send(Intent::Interrupt { session: 1 });
    until(
        &rx,
        |e| matches!(e, Event::Queued { text, .. } if text == "later"),
    );
    // The queue is told it is empty once the message is out.
    until(
        &rx,
        |e| matches!(e, Event::QueueChanged { entries, .. } if entries.is_empty()),
    );
}

#[test]
fn queued_messages_go_out_as_one_message() {
    let (engine, rx) = engine();
    busy_steer_session(&engine, &rx);
    queue_send(&engine, "a");
    until(
        &rx,
        |e| matches!(e, Event::QueueChanged { entries, .. } if entries.len() == 1),
    );
    queue_send(&engine, "b");
    until(
        &rx,
        |e| matches!(e, Event::QueueChanged { entries, .. } if entries.len() == 2),
    );
    engine.send(Intent::Interrupt { session: 1 });
    let events = until(&rx, |e| matches!(e, Event::Queued { .. }));
    assert!(matches!(
        events.last(),
        Some(Event::Queued { text, .. }) if text == "a\n\nb"
    ));
}

#[test]
fn a_slash_command_in_the_queue_is_sent_alone() {
    let (engine, rx) = engine();
    busy_steer_session(&engine, &rx);
    queue_send(&engine, "/compact");
    until(
        &rx,
        |e| matches!(e, Event::QueueChanged { entries, .. } if entries.len() == 1),
    );
    queue_send(&engine, "c");
    until(
        &rx,
        |e| matches!(e, Event::QueueChanged { entries, .. } if entries.len() == 2),
    );
    engine.send(Intent::Interrupt { session: 1 });
    let events = until(
        &rx,
        |e| matches!(e, Event::Queued { text, .. } if text == "/compact"),
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::Queued { text, .. } if text == "c")));
    until(
        &rx,
        |e| matches!(e, Event::Queued { text, .. } if text == "c"),
    );
}

#[test]
fn a_queued_message_can_be_removed_sent_now_or_taken_back() {
    let (engine, rx) = engine();
    busy_steer_session(&engine, &rx);
    for text in ["a", "b", "c"] {
        queue_send(&engine, text);
    }
    let events = until(
        &rx,
        |e| matches!(e, Event::QueueChanged { entries, .. } if entries.len() == 3),
    );
    let [(a, _), (b, _), (c, _)] = queue_ids(&events).try_into().unwrap();
    engine.send(Intent::QueueRemove { session: 1, id: a });
    until(
        &rx,
        |e| matches!(e, Event::QueueChanged { entries, .. } if entries.len() == 2),
    );
    engine.send(Intent::QueueSendNow { session: 1, id: b });
    // Sent now, as a steer: the capture reads it at its steer line and the turn goes on.
    until(
        &rx,
        |e| matches!(e, Event::Queued { text, .. } if text == "b"),
    );
    engine.send(Intent::QueueTake { session: 1, id: c });
    let events = until(&rx, |e| matches!(e, Event::QueueTaken { .. }));
    assert!(matches!(
        events.last(),
        Some(Event::QueueTaken { text, images, .. }) if text == "c" && images.is_empty()
    ));
    engine.send(Intent::Interrupt { session: 1 });
    let events = quiet(&rx, 800);
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::Queued { text, .. } if text == "c")));
}

#[test]
fn a_cleared_queue_sends_nothing_at_the_turn_end() {
    let (engine, rx) = engine();
    busy_steer_session(&engine, &rx);
    queue_send(&engine, "a");
    until(&rx, |e| matches!(e, Event::QueueChanged { .. }));
    engine.send(Intent::QueueClear { session: 1 });
    until(
        &rx,
        |e| matches!(e, Event::QueueChanged { entries, .. } if entries.is_empty()),
    );
    engine.send(Intent::Interrupt { session: 1 });
    let events = quiet(&rx, 800);
    assert!(!events.iter().any(|e| matches!(e, Event::Queued { .. })));
}

/// Events for `millis`, with nothing waited for: what the engine did in that time.
fn quiet(rx: &Receiver<Event>, millis: u64) -> Vec<Event> {
    let mut seen = vec![];
    while let Ok(event) = rx.recv_timeout(Duration::from_millis(millis)) {
        seen.push(event);
    }
    seen
}

#[test]
fn a_queue_kept_across_a_restart_comes_back_held_and_waits_for_the_person() {
    let database = std::env::temp_dir().join(format!("quill-test-queue-{}.db", uuid_like()));
    let start = || {
        let (tx, rx) = channel();
        let programs = HashMap::from([(
            "claude".to_string(),
            env!("CARGO_BIN_EXE_fake-agent").to_string(),
        )]);
        let config = quill_engine::Config {
            programs,
            database: Some(database.clone()),
            snapshots: None,
            settings: None,
            ..Default::default()
        };
        (
            Engine::start_with(move |event| drop(tx.send(event)), config),
            rx,
        )
    };
    let (engine, rx) = start();
    busy_steer_session(&engine, &rx);
    queue_send(&engine, "held one");
    until(
        &rx,
        |e| matches!(e, Event::QueueChanged { entries, .. } if entries.len() == 1),
    );
    drop(engine);

    let (engine, rx) = start();
    let events = until(&rx, |e| {
        matches!(e, Event::QueueChanged { entries, .. }
            if entries.len() == 1 && entries[0].held && entries[0].text == "held one")
    });
    let [(id, _)] = queue_ids(&events).try_into().unwrap();
    // A turn ending does not send it.
    engine.send(Intent::Send {
        session: 1,
        text: "Count from 1 to 40".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Status {
                status: SessionStatus::Running,
                ..
            }
        )
    });
    engine.send(Intent::Interrupt { session: 1 });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    let events = quiet(&rx, 800);
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::Queued { text, .. } if text == "held one")));
    // The person sends it, which goes out.
    engine.send(Intent::QueueSendNow { session: 1, id });
    until(
        &rx,
        |e| matches!(e, Event::Queued { text, .. } if text == "held one"),
    );
    let _ = std::fs::remove_file(&database);
}

/// An engine whose fake agent starts signed out, with the marker that flips it in a folder of
/// the test's own (the agent's environment setting reaches the account commands too).
fn signed_out_engine(name: &str) -> (Engine, Receiver<Event>, std::path::PathBuf) {
    let (engine, rx) = engine();
    let marker = workdir(name).join("marker");
    let mut settings = quill_model::Settings::default();
    settings.agents.insert(
        "claude".into(),
        quill_model::AgentSettings {
            env: vec![
                "QUILL_FAKE_AUTH=signed-out".into(),
                format!("QUILL_FAKE_AUTH_MARKER={}", marker.display()),
            ],
            ..Default::default()
        },
    );
    engine.send(Intent::SaveSettings(settings));
    (engine, rx, marker)
}

fn state(rx: &Receiver<Event>) -> quill_model::AuthState {
    let seen = until(rx, |e| matches!(e, Event::Auth { .. }));
    match seen.last() {
        Some(Event::Auth { state, .. }) => state.clone(),
        _ => unreachable!(),
    }
}

#[test]
fn signing_in_through_the_program_ends_signed_in_and_signing_out_ends_signed_out() {
    let (engine, rx, _) = signed_out_engine("auth-flow");
    let agent = "claude".to_string();
    engine.send(Intent::CheckSignIn {
        agent: agent.clone(),
    });
    assert_eq!(state(&rx), quill_model::AuthState::SignedOut);

    engine.send(Intent::SignIn {
        agent: agent.clone(),
        method: "claudeai".into(),
    });
    let seen = until(&rx, |e| matches!(e, Event::SignInUrl { .. }));
    // The OSC 8 wrapper is gone: a plain address.
    assert!(matches!(
        seen.last(),
        Some(Event::SignInUrl { url, .. }) if url.starts_with("https://claude.com/cai/oauth/authorize?")
            && !url.contains('\x1b') && !url.contains('\x07')
    ));
    engine.send(Intent::SubmitSignInCode {
        agent: agent.clone(),
        code: "abc#state".into(),
    });
    assert!(
        matches!(state(&rx), quill_model::AuthState::SignedIn { email: Some(e), .. } if e == "ada@example.com")
    );

    engine.send(Intent::SignOut { agent });
    assert_eq!(state(&rx), quill_model::AuthState::SignedOut);
}

#[test]
fn a_rejected_code_says_what_the_program_said_and_cancel_ends_the_login() {
    let (engine, rx, marker) = signed_out_engine("auth-fail");
    let agent = "claude".to_string();
    let sign_in = || Intent::SignIn {
        agent: "claude".into(),
        method: "console".into(),
    };
    engine.send(sign_in());
    until(&rx, |e| matches!(e, Event::SignInUrl { .. }));
    engine.send(Intent::SubmitSignInCode {
        agent: agent.clone(),
        code: "fail".into(),
    });
    let seen = until(&rx, |e| matches!(e, Event::SignInFailed { .. }));
    assert!(matches!(
        seen.last(),
        Some(Event::SignInFailed { message, .. }) if message == "Login failed: the code was not accepted"
    ));

    // Cancelled, the program is killed before it can accept a code: nothing follows.
    engine.send(sign_in());
    until(&rx, |e| matches!(e, Event::SignInUrl { .. }));
    engine.send(Intent::CancelSignIn {
        agent: agent.clone(),
    });
    engine.send(Intent::SubmitSignInCode {
        agent,
        code: "abc#state".into(),
    });
    assert!(rx.recv_timeout(Duration::from_millis(700)).is_err());
    assert!(!marker.exists());
}

/// Codex's account through the fake agent's `login status` / `login` / `logout`: the address
/// is the line on stderr, the browser coming back signs in by itself (no code), and the state
/// is asked again after each.
#[test]
fn codex_signs_in_and_out_through_its_own_commands() {
    let (tx, rx) = channel();
    let fake = env!("CARGO_BIN_EXE_fake-agent").to_string();
    let programs = HashMap::from([("codex".to_string(), fake)]);
    let config = quill_engine::Config {
        programs,
        database: None,
        snapshots: None,
        settings: None,
        ..Default::default()
    };
    let engine = Engine::start_with(move |event| drop(tx.send(event)), config);
    let marker = workdir("codex-auth").join("marker");
    let mut settings = quill_model::Settings::default();
    settings.agents.insert(
        "codex".into(),
        quill_model::AgentSettings {
            env: vec![
                "QUILL_FAKE_AUTH=signed-out".into(),
                "QUILL_FAKE_LOGIN_MS=100".into(),
                format!("QUILL_FAKE_AUTH_MARKER={}", marker.display()),
            ],
            ..Default::default()
        },
    );
    engine.send(Intent::SaveSettings(settings));
    let agent = "codex".to_string();
    engine.send(Intent::CheckSignIn {
        agent: agent.clone(),
    });
    assert_eq!(state(&rx), quill_model::AuthState::SignedOut);
    engine.send(Intent::SignIn {
        agent: agent.clone(),
        method: "chatgpt".into(),
    });
    let seen = until(&rx, |e| matches!(e, Event::SignInUrl { .. }));
    assert!(matches!(
        seen.last(),
        Some(Event::SignInUrl { url, .. }) if url.starts_with("https://auth.openai.com/oauth/authorize?")
    ));
    assert!(matches!(
        state(&rx),
        quill_model::AuthState::SignedIn { .. }
    ));
    engine.send(Intent::SignOut { agent });
    assert_eq!(state(&rx), quill_model::AuthState::SignedOut);
}

/// Codex's captures through the same fake agent: the adapter's lines pace the replay, one per
/// `out` of the capture (its `model/list` is answered by the fake agent itself).
#[test]
fn codex_captures_replay_through_the_engine() {
    let (tx, rx) = channel();
    let programs = HashMap::from([(
        "codex".to_string(),
        env!("CARGO_BIN_EXE_fake-agent").to_string(),
    )]);
    let config = quill_engine::Config {
        programs,
        database: None,
        snapshots: None,
        settings: None,
        ..Default::default()
    };
    let engine = Engine::start_with(move |event| drop(tx.send(event)), config);
    let session = |name: &str, id: u64, text: &str| {
        let dir = workdir("simple");
        let capture = format!(
            "{}/../quill-agent-codex/fixtures/0.163.0/{name}.jsonl",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::write(dir.join(".fake-agent"), capture).unwrap();
        engine.send(Intent::NewSession(NewSession {
            agent: "codex".into(),
            cwd: dir,
            model: Some("gpt-6-luna".into()),
            effort: Some("low".into()),
            mode: None,
        }));
        engine.send(Intent::Send {
            session: id,
            text: text.into(),
            mentions: vec![],
            images: vec![],
            lane: Lane::Steer,
        });
    };
    let finished = |e: &Event| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { ok: true, .. },
                ..
            }
        )
    };

    session("simple", 1, "Reply with exactly the word: ok");
    let events = until(&rx, finished);
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Agent { event: AgentEvent::Text { text, .. }, .. } if text == "ok"
    )));

    session(
        "deny",
        2,
        "Use the shell tool to run exactly: touch denied.txt",
    );
    let events = until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::PermissionRequest { .. },
                ..
            }
        )
    });
    let Some(Event::Agent {
        event: AgentEvent::PermissionRequest { id, tool, .. },
        ..
    }) = events.last()
    else {
        unreachable!()
    };
    assert_eq!(tool, "Bash");
    engine.send(Intent::Decide {
        session: 2,
        permission: id.clone(),
        decision: Decision::Deny {
            message: "no".into(),
        },
    });
    until(&rx, finished);
}

/// An engine whose Claude Code and Codex are the fake agent, run as `fake-claude` and
/// `fake-codex` so that it answers in each one's words; what is added and removed is kept in a
/// folder of its own (beside the links).
fn two_agents() -> (Engine, Receiver<Event>, std::path::PathBuf) {
    let dir = workdir("simple");
    let link = |program: &str| {
        let path = dir.join(program);
        #[cfg(unix)]
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-agent"), &path).unwrap();
        path.display().to_string()
    };
    let programs = HashMap::from([
        ("claude".to_string(), link("fake-claude")),
        ("codex".to_string(), link("fake-codex")),
    ]);
    let (tx, rx) = channel();
    let config = quill_engine::Config {
        programs,
        database: None,
        snapshots: None,
        settings: None,
        ..Default::default()
    };
    let engine = Engine::start_with(move |event| drop(tx.send(event)), config);
    (engine, rx, dir)
}

fn servers_of(rx: &Receiver<Event>, agent: &str) -> (Vec<String>, Option<String>) {
    let seen = until(
        rx,
        |e| matches!(e, Event::Servers { agent: a, .. } if a == agent),
    );
    match seen.last() {
        Some(Event::Servers { servers, error, .. }) => (
            servers.iter().map(|s| s.name.clone()).collect(),
            error.clone(),
        ),
        _ => unreachable!(),
    }
}

/// Codex's connectors through `codex mcp list --json`, `add` and `remove`, each CLI acting on
/// its own: the same server added to both lands in both lists, and what Codex cannot (a header)
/// is said in its own section, not in Claude Code's.
#[test]
fn connectors_are_listed_added_and_removed_in_each_cli_on_its_own() {
    use quill_model::{ExtensionOp, NewServer};
    let (engine, rx, dir) = two_agents();
    let list = |agent: &str| {
        engine.send(Intent::Servers {
            agent: agent.into(),
            folder: dir.clone(),
        })
    };
    list("codex");
    let (names, error) = servers_of(&rx, "codex");
    assert!(error.is_none(), "{error:?}");
    assert_eq!(names, ["node_repl", "docs", "scratch"]);
    list("claude");
    let (names, _) = servers_of(&rx, "claude");
    assert_eq!(names, ["github", "plugin:vercel:vercel", "claude.ai Figma"]);

    let add = |agent: &str, headers: Vec<String>| {
        engine.send(Intent::ExtensionOp {
            agent: agent.into(),
            folder: dir.clone(),
            op: ExtensionOp::AddServer(NewServer {
                name: "pg".into(),
                target: "https://pg.example.com/mcp".into(),
                remote: true,
                headers,
                ..Default::default()
            }),
        });
    };
    // One after the other: their answers come in any order.
    add("claude", vec![]);
    let (claude, error) = servers_of(&rx, "claude");
    assert!(claude.contains(&"pg".to_owned()) && error.is_none());
    add("codex", vec![]);
    let (codex, error) = servers_of(&rx, "codex");
    assert!(codex.contains(&"pg".to_owned()) && error.is_none());

    // Codex has no header option: it says so, and the list stays.
    engine.send(Intent::ExtensionOp {
        agent: "codex".into(),
        folder: dir.clone(),
        op: ExtensionOp::AddServer(NewServer {
            name: "keyed".into(),
            target: "https://k.example.com/mcp".into(),
            remote: true,
            headers: vec!["X-Key: 1".into()],
            ..Default::default()
        }),
    });
    let (names, error) = servers_of(&rx, "codex");
    assert!(!names.contains(&"keyed".to_owned()));
    assert_eq!(
        error.as_deref(),
        Some("Codex can\u{2019}t add a server with headers.")
    );

    // Removing takes it from its own CLI; one that is not there is said, though Codex exits well.
    let remove = |agent: &str, name: &str| {
        engine.send(Intent::ExtensionOp {
            agent: agent.into(),
            folder: dir.clone(),
            op: ExtensionOp::RemoveServer(name.into()),
        })
    };
    remove("codex", "pg");
    let (names, error) = servers_of(&rx, "codex");
    assert!(!names.contains(&"pg".to_owned()) && error.is_none());
    remove("codex", "pg");
    let (_, error) = servers_of(&rx, "codex");
    assert_eq!(error.as_deref(), Some("No MCP server named 'pg' found."));
    let (claude, _) = {
        list("claude");
        servers_of(&rx, "claude")
    };
    assert!(claude.contains(&"pg".to_owned()));
}

/// Codex's skills through its app-server (no thread, no turn), and a state written with
/// `skills/config/write` over an app-server kept open until it answers: the next read shows it.
#[test]
fn codex_skills_are_read_from_its_app_server_and_a_state_is_written() {
    use quill_model::ExtensionOp;
    let (engine, rx, dir) = two_agents();
    // No capture in this folder: the fake agent answers the app-server's requests alone.
    let folder = dir.join("bare");
    std::fs::create_dir_all(&folder).unwrap();
    let extensions = |engine: &Engine| {
        engine.send(Intent::Extensions {
            agent: "codex".into(),
            folder: folder.clone(),
        });
        let seen = until(&rx, |e| matches!(e, Event::Extensions { .. }));
        match seen.last() {
            Some(Event::Extensions {
                extensions, error, ..
            }) => (extensions.clone(), error.clone()),
            _ => unreachable!(),
        }
    };
    let (known, error) = extensions(&engine);
    assert!(error.is_none(), "{error:?}");
    assert_eq!(known.skills.len(), 5);
    let review = known
        .skills
        .iter()
        .find(|s| s.name == "review")
        .unwrap()
        .clone();
    assert_eq!(
        (review.state.as_str(), review.source.as_str()),
        ("on", "user")
    );
    assert!(!known.kinds.skills_per_folder && !known.kinds.switch_plugins);
    assert_eq!(known.skill_states[0].0, "on");
    // Its plugins and marketplaces, in Codex's shape.
    assert_eq!(known.plugins.len(), 2);
    assert_eq!(known.marketplaces.len(), 2);

    engine.send(Intent::ExtensionOp {
        agent: "codex".into(),
        folder: folder.clone(),
        op: ExtensionOp::SkillState {
            skill: review,
            state: "off".into(),
        },
    });
    let seen = until(&rx, |e| matches!(e, Event::Extensions { .. }));
    let Some(Event::Extensions {
        extensions, error, ..
    }) = seen.last()
    else {
        unreachable!()
    };
    assert!(error.is_none(), "{error:?}");
    let review = extensions
        .skills
        .iter()
        .find(|s| s.name == "review")
        .unwrap();
    assert_eq!(review.state, "off");

    // A plugin's switch is not Codex's to make, and it says so.
    let plugin = extensions.plugins[0].clone();
    engine.send(Intent::ExtensionOp {
        agent: "codex".into(),
        folder,
        op: ExtensionOp::SwitchPlugin {
            plugin,
            enabled: false,
        },
    });
    let seen = until(&rx, |e| matches!(e, Event::Extensions { .. }));
    let Some(Event::Extensions { error, .. }) = seen.last() else {
        unreachable!()
    };
    assert_eq!(
        error.as_deref(),
        Some("Codex can\u{2019}t turn a plugin off or on.")
    );
}

/// A Codex session offers its skills in the `/` menu: once it is ready, the engine asks them of
/// the program apart and tells `AgentEvent::Skills`, without a line more in the session's own.
#[test]
fn a_codex_session_is_told_its_skills_once_it_is_ready() {
    let (engine, rx, dir) = two_agents();
    let capture = format!(
        "{}/../quill-agent-codex/fixtures/0.163.0/simple.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::write(dir.join(".fake-agent"), capture).unwrap();
    engine.send(Intent::NewSession(NewSession {
        agent: "codex".into(),
        cwd: dir,
        model: Some("gpt-6-luna".into()),
        effort: Some("low".into()),
        mode: None,
    }));
    engine.send(Intent::Send {
        session: 1,
        text: "Reply with exactly the word: ok".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    let seen = until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::Skills(_),
                ..
            }
        )
    });
    let Some(Event::Agent {
        event: AgentEvent::Skills(skills),
        ..
    }) = seen.last()
    else {
        unreachable!()
    };
    assert!(skills.iter().any(|s| s.name == "review"));
}

/// quill's "All agents" layer: kept (and written) before it is announced, shadowed by an agent's
/// own server of the same name, added to the sessions of the agents that can use it with the
/// secret only in the process's environment, and excluded per agent.
#[test]
fn global_servers_are_kept_then_injected_per_session_with_secrets_only_in_the_environment() {
    use quill_model::{Applies, GlobalOp, GlobalServer, GlobalVar};
    let dir = workdir("simple");
    let config_dir = std::env::temp_dir().join(format!("quill-global-{}", uuid_like()));
    let file = config_dir.join("quill/extensions.json");
    let link = |program: &str| {
        let path = dir.join(program);
        #[cfg(unix)]
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_fake-agent"), &path).unwrap();
        path.display().to_string()
    };
    let (tx, rx) = channel();
    let config = quill_engine::Config {
        programs: HashMap::from([
            ("claude".to_string(), link("fake-claude")),
            ("codex".to_string(), link("fake-codex")),
        ]),
        extensions: Some(file.clone()),
        secrets: Some(std::sync::Arc::new(quill_engine::MemorySecrets::default())),
        ..Default::default()
    };
    let engine = Engine::start_with(move |event| drop(tx.send(event)), config);
    let var = |key: &str, value: &str, secret| GlobalVar {
        key: key.into(),
        value: Some(value.into()),
        secret,
    };
    let server = |name: &str| GlobalServer {
        name: name.into(),
        target: "npx".into(),
        args: vec!["-y".into(), "tool".into()],
        env: vec![
            var("GITHUB_TOKEN", "ghp_topsecret", true),
            var("LOG", "warn", false),
        ],
        ..Default::default()
    };
    let global = |rx: &Receiver<Event>| match until(rx, |e| matches!(e, Event::Global(_))).pop() {
        Some(Event::Global(state)) => state,
        _ => unreachable!(),
    };
    // Written before announced, and without the secret.
    engine.send(Intent::Global(GlobalOp::SetServer(server("github"))));
    let state = global(&rx);
    let text = std::fs::read_to_string(&file).expect("written before the event");
    assert!(
        !text.contains("ghp_topsecret") && state.error.is_none(),
        "{text} {state:?}"
    );
    engine.send(Intent::Global(GlobalOp::SetServer(server("extra"))));
    global(&rx);
    // Claude lists a "github" of its own: it wins, and "extra" is still added.
    engine.send(Intent::Servers {
        agent: "claude".into(),
        folder: dir.clone(),
    });
    let state = global(&rx);
    let how = |state: &quill_model::GlobalState, agent: &str, name: &str| {
        (state.applied.iter())
            .find(|a| a.agent == agent && a.name == name)
            .map(|a| a.applies.clone())
    };
    assert_eq!(how(&state, "claude", "github"), Some(Applies::Replaced));
    assert_eq!(how(&state, "claude", "extra"), Some(Applies::Active));
    assert_eq!(how(&state, "codex", "github"), Some(Applies::Active));
    // Per-agent exclusion.
    engine.send(Intent::Global(GlobalOp::Use {
        skill: false,
        name: "extra".into(),
        agent: "codex".into(),
        on: false,
    }));
    assert_eq!(how(&global(&rx), "codex", "extra"), Some(Applies::Excluded));
    // A Claude session gets "extra" in --mcp-config with only the variable's name, and the
    // value in its environment; "github" (its own wins) is not added.
    engine.send(Intent::NewSession(NewSession {
        agent: "claude".into(),
        cwd: dir.clone(),
        model: None,
        effort: None,
        mode: None,
    }));
    engine.send(Intent::Send {
        session: 1,
        text: "hi".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    let launched = std::fs::read_to_string(dir.join(".fake-launch")).unwrap();
    let config = launched
        .lines()
        .skip_while(|l| *l != "--mcp-config")
        .nth(1)
        .unwrap();
    assert!(config.contains("${QUILL_MCP_EXTRA_GITHUB_TOKEN}") && !config.contains("\"github\""));
    assert!(!launched
        .lines()
        .any(|l| l.starts_with("--") && l.contains("topsecret")));
    assert!(!config.contains("topsecret"));
    assert!(
        launched.contains("QUILL_MCP_EXTRA_GITHUB_TOKEN=ghp_topsecret"),
        "{launched}"
    );
    let _ = std::fs::remove_dir_all(config_dir);
}

/// Global skills live in quill's folder; a session gets them through a plugin (Claude) or an
/// extra root (Codex), and an agent's own skill of the same name wins.
#[test]
fn global_skills_are_made_and_added_to_the_session() {
    use quill_model::{Applies, GlobalOp};
    let dir = workdir("simple");
    let config_dir = std::env::temp_dir().join(format!("quill-global-{}", uuid_like()));
    let (tx, rx) = channel();
    let engine = Engine::start_with(
        move |event| drop(tx.send(event)),
        quill_engine::Config {
            programs: HashMap::from([(
                "claude".to_string(),
                env!("CARGO_BIN_EXE_fake-agent").to_string(),
            )]),
            extensions: Some(config_dir.join("quill/extensions.json")),
            secrets: Some(std::sync::Arc::new(quill_engine::MemorySecrets::default())),
            ..Default::default()
        },
    );
    let global = |rx: &Receiver<Event>| match until(rx, |e| matches!(e, Event::Global(_))).pop() {
        Some(Event::Global(state)) => state,
        _ => unreachable!(),
    };
    for name in ["release-notes", "review"] {
        engine.send(Intent::Global(GlobalOp::NewSkill {
            name: name.into(),
            description: format!("The {name} skill"),
        }));
        assert!(global(&rx).error.is_none());
    }
    assert!(global_skill_file(&config_dir, "release-notes"));
    engine.send(Intent::Global(GlobalOp::NewSkill {
        name: "Bad".into(),
        description: String::new(),
    }));
    assert!(global(&rx).error.is_some());
    // Claude lists a "review" of its own.
    engine.send(Intent::Extensions {
        agent: "claude".into(),
        folder: dir.clone(),
    });
    let state = global(&rx);
    let how = |name: &str| {
        (state.applied.iter())
            .find(|a| a.skill && a.name == name)
            .map(|a| a.applies.clone())
    };
    assert_eq!(
        (how("review"), how("release-notes")),
        (Some(Applies::Replaced), Some(Applies::Active))
    );
    engine.send(Intent::NewSession(NewSession {
        agent: "claude".into(),
        cwd: dir.clone(),
        model: None,
        effort: None,
        mode: None,
    }));
    engine.send(Intent::Send {
        session: 1,
        text: "hi".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    let launched = std::fs::read_to_string(dir.join(".fake-launch")).unwrap();
    let plugin = launched
        .lines()
        .skip_while(|l| *l != "--plugin-dir")
        .nth(1)
        .unwrap();
    assert!(plugin.ends_with("injected/claude"), "{plugin}");
    assert!(std::path::Path::new(plugin)
        .join("skills/release-notes/SKILL.md")
        .is_file());
    assert!(!std::path::Path::new(plugin).join("skills/review").exists());
    assert!(std::path::Path::new(plugin)
        .join(".claude-plugin/plugin.json")
        .is_file());
    let _ = std::fs::remove_dir_all(config_dir);
}

fn global_skill_file(config_dir: &std::path::Path, name: &str) -> bool {
    config_dir
        .join(format!("quill/skills/{name}/SKILL.md"))
        .is_file()
}

#[test]
fn a_session_reached_from_a_phone_shows_its_messages_as_the_persons() {
    // Remote Control on CLI 2.1.316: the phone sent two messages and approved a permission.
    let (tx, rx) = channel();
    let programs = HashMap::from([(
        "claude".to_string(),
        env!("CARGO_BIN_EXE_fake-agent").to_string(),
    )]);
    let snapshots = std::env::temp_dir().join(format!("quill-snapshots-{}", uuid_like()));
    let config = quill_engine::Config {
        programs,
        database: None,
        snapshots: Some(snapshots.clone()),
        settings: None,
        ..Default::default()
    };
    let engine = Engine::start_with(move |event| drop(tx.send(event)), config);
    let cwd = workdir("remote");
    std::fs::write(
        cwd.join(".fake-agent"),
        fixture("remote").replace("2.1.310", "2.1.316"),
    )
    .unwrap();
    engine.send(Intent::NewSession(NewSession {
        agent: "claude".into(),
        cwd,
        model: None,
        effort: None,
        mode: None,
    }));
    engine.send(Intent::Send {
        session: 1,
        text: "Reply with exactly the word: ok".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    engine.send(Intent::Remote {
        session: 1,
        on: true,
        name: Some("quill capture".into()),
    });
    let on = |e: &Event| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::Remote {
                    on: true,
                    url: Some(_),
                    ..
                },
                ..
            }
        )
    };
    let events = until(
        &rx,
        |e| matches!(e, Event::Queued { text, .. } if text.starts_with("Escribí")),
    );
    assert!(events.iter().any(on), "{events:#?}");
    let phone = events
        .iter()
        .find_map(|e| match e {
            Event::Queued { request, text, .. } if text.contains("touch remoto.txt") => {
                Some(request.clone())
            }
            _ => None,
        })
        .expect("the phone's first message");
    // Its permission, approved on the phone, leaves quill's card, and its turn is its own.
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Agent {
            event: AgentEvent::Withdrawn { .. },
            ..
        }
    )));
    assert!(events.iter().any(|e| matches!(e,
        Event::Changes { request, .. } if *request == phone)));
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    engine.send(Intent::Remote {
        session: 1,
        on: false,
        name: None,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::Remote { on: false, .. },
                ..
            }
        )
    });
    let _ = std::fs::remove_dir_all(&snapshots);
}

#[test]
fn codex_sessions_share_one_app_server_a_phone_can_reach() {
    // Codex's Remote Control option on, and the fake answering as an app-server.
    let dir = workdir("simple");
    let settings = dir.join("settings.json");
    let chosen = serde_json::json!({ "agents": { "codex": {
        "options": { "remote": "true" }, "env": ["QUILL_FAKE_APP_SERVER=1"] } } });
    std::fs::write(&settings, chosen.to_string()).unwrap();
    let programs = HashMap::from([(
        "codex".to_string(),
        env!("CARGO_BIN_EXE_fake-agent").to_string(),
    )]);
    let (tx, rx) = channel();
    let config = quill_engine::Config {
        programs,
        database: None,
        snapshots: None,
        settings: Some(settings),
        ..Default::default()
    };
    let engine = Engine::start_with(move |event| drop(tx.send(event)), config);
    for _ in 0..2 {
        engine.send(Intent::NewSession(NewSession {
            agent: "codex".into(),
            cwd: dir.clone(),
            model: None,
            effort: None,
            mode: None,
        }));
    }
    for session in [1, 2] {
        engine.send(Intent::Send {
            session,
            text: format!("hello {session}"),
            mentions: vec![],
            images: vec![],
            lane: Lane::Steer,
        });
    }
    // One process: each session has a thread of its own in it, and gets its own answers.
    let (mut threads, mut answered) = (HashMap::new(), HashMap::new());
    until(&rx, |e| {
        match e {
            Event::Agent {
                session,
                event: AgentEvent::Session { id },
                ..
            } => drop(threads.insert(*session, id.clone())),
            Event::Agent {
                session,
                event: AgentEvent::Text { text, .. },
                ..
            } => drop(answered.insert(*session, text.clone())),
            _ => {}
        }
        answered.len() == 2
    });
    assert_eq!(answered[&1], "echo: hello 1");
    assert_eq!(answered[&2], "echo: hello 2");
    let mut ids: Vec<&String> = threads.values().collect();
    ids.sort();
    assert_eq!(ids, ["t1", "t2"]);
    // Reaching one reaches both: the process tells every session.
    engine.send(Intent::Remote {
        session: 1,
        on: true,
        name: None,
    });
    let mut on = std::collections::HashSet::new();
    until(&rx, |e| {
        if let Event::Agent {
            session,
            event: AgentEvent::Remote { on: true, .. },
            ..
        } = e
        {
            on.insert(*session);
        }
        on.len() == 2
    });
}

#[test]
fn what_is_uncommitted_is_read_after_each_step_and_a_burst_is_read_once() {
    use serde_json::{json, Value};
    let (engine, rx) = engine();
    let dir = workdir("simple");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(["-c", "user.name=q", "-c", "user.email=q@q"])
            .args(args)
            .current_dir(&dir)
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    // The replay's own files are not the project's: they would count as new ones.
    let replay_files = "capture.jsonl\n.fake-agent\n.fake-launch\n";
    std::fs::write(dir.join(".git/info/exclude"), replay_files).unwrap();
    std::fs::write(dir.join("a.txt"), "one\n").unwrap();
    git(&["add", "a.txt"]);
    git(&["commit", "-qm", "one"]);
    // The capture: a plain turn whose answer is followed by a command that adds a line, a
    // permission the test answers when it has seen the read, then three more in a burst.
    let step = |id: &str, line: &str| {
        let message = |role: &str, content: Value| {
            json!({ "dir": "in", "msg": { "type": role, "session_id": "s",
                "parent_tool_use_id": null, "message": { "role": role, "content": [content] },
                "tool_use_result": { "stdout": "", "stderr": "", "interrupted": false } } })
        };
        let command = format!("printf '{line}\\n' >> a.txt");
        [
            message(
                "assistant",
                json!({ "type": "tool_use", "id": id, "name": "Bash",
                    "input": { "command": command } }),
            ),
            message(
                "user",
                json!({ "type": "tool_result", "tool_use_id": id, "content": "" }),
            ),
        ]
    };
    let mut steps: Vec<Value> = step("t1", "two").into();
    steps.push(
        json!({ "dir": "in", "msg": { "type": "control_request", "request_id": "p2",
        "request": { "subtype": "can_use_tool", "tool_name": "Bash", "tool_use_id": "t2",
            "input": { "command": "printf" } } } }),
    );
    steps.push(json!({ "dir": "out", "msg": {} }));
    for (id, line) in [("t2", "three"), ("t3", "four"), ("t4", "five")] {
        steps.extend(step(id, line));
    }
    let mut capture = vec![];
    for line in std::fs::read_to_string(fixture("simple")).unwrap().lines() {
        let entry: Value = serde_json::from_str(line).unwrap();
        if entry["msg"]["type"] == "result" {
            capture.extend(steps.drain(..).map(|s| s.to_string()));
        }
        capture.push(line.to_owned());
    }
    let replay = dir.join("capture.jsonl");
    std::fs::write(&replay, capture.join("\n")).unwrap();
    std::fs::write(dir.join(".fake-agent"), replay.display().to_string()).unwrap();
    engine.send(Intent::NewSession(NewSession {
        agent: "claude".into(),
        cwd: dir.clone(),
        model: None,
        effort: None,
        mode: None,
    }));
    engine.send(Intent::Send {
        session: 1,
        text: "add lines".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    let counts = |e: &Event| match e {
        Event::WorkingTree { files, repo, .. } if repo.is_some() => Some(
            files
                .iter()
                .map(|f| (f.path.clone(), f.added, f.removed))
                .collect::<Vec<_>>(),
        ),
        _ => None,
    };
    // Nobody asked for it: after the step the engine reads the folder, which waits with the
    // agent for the person, and reads it once.
    let first = until(&rx, |e| counts(e).is_some());
    assert_eq!(
        first.iter().find_map(counts),
        Some(vec![("a.txt".to_owned(), 1, 0)])
    );
    let mut waiting = first.iter().any(|e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::PermissionRequest { .. },
                ..
            }
        )
    });
    while let Ok(e) = rx.recv_timeout(Duration::from_millis(600)) {
        assert_eq!(counts(&e), None, "one step, one read");
        waiting |= matches!(
            e,
            Event::Agent {
                event: AgentEvent::PermissionRequest { .. },
                ..
            }
        );
    }
    assert!(waiting);
    engine.send(Intent::Decide {
        session: 1,
        permission: "p2".into(),
        decision: Decision::Allow,
    });
    // Three steps and the turn's end, each a reason to read, come in a burst that settles
    // into one read (two, at most, on a busy machine).
    let mut reads = vec![];
    until(&rx, |e| {
        reads.extend(counts(e));
        reads
            .last()
            .is_some_and(|r| r == &[("a.txt".to_owned(), 4, 0)])
    });
    while let Ok(e) = rx.recv_timeout(Duration::from_millis(800)) {
        reads.extend(counts(&e));
    }
    assert!(reads.len() <= 2, "{reads:?}");
    assert_eq!(reads.last(), Some(&vec![("a.txt".to_owned(), 4, 0)]));
}

/// A mode chosen while the agent is not running (a session read back, or one not yet sent
/// to) is the one it starts in: a plan carried out after a restart leaves planning.
#[test]
fn a_mode_chosen_before_the_agent_runs_is_the_one_it_starts_in() {
    let (engine, rx) = engine();
    let dir = workdir("simple");
    engine.send(Intent::NewSession(NewSession {
        agent: "claude".into(),
        cwd: dir.clone(),
        model: None,
        effort: None,
        mode: None,
    }));
    engine.send(Intent::SetMode {
        session: 1,
        mode: "plan".into(),
    });
    engine.send(Intent::Send {
        session: 1,
        text: "hi".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::TurnFinished { .. },
                ..
            }
        )
    });
    let launched = std::fs::read_to_string(dir.join(".fake-launch")).unwrap();
    let mode = (launched.lines())
        .skip_while(|l| *l != "--permission-mode")
        .nth(1);
    assert_eq!(mode, Some("plan"), "{launched}");
}

/// An answer goes to the agent and nowhere else: what is kept of it only says the card was
/// answered, since one may be a secret.
#[test]
fn an_answer_is_not_kept_with_the_session() {
    let (engine, rx) = engine();
    new_session(&engine, "ask");
    engine.send(Intent::Send {
        session: 1,
        text: "Ask me".into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
    let events = until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                event: AgentEvent::PermissionRequest { .. },
                ..
            }
        )
    });
    let Some(Event::Agent {
        event: AgentEvent::PermissionRequest { id, .. },
        ..
    }) = events.last()
    else {
        unreachable!()
    };
    engine.send(Intent::Decide {
        session: 1,
        permission: id.clone(),
        decision: Decision::Answer(serde_json::json!({ "Pick red or blue?": "hunter2" })),
    });
    let events = until(&rx, |e| matches!(e, Event::Decided { .. }));
    assert!(matches!(
        events.last(),
        Some(Event::Decided {
            decision: Decision::Answer(serde_json::Value::Null),
            ..
        })
    ));
}

/// A folder whose capture is sessions.jsonl with its start_session call made a call of
/// wait_for_session on `on`: the agent waits, then says "done" and ends its turn.
fn waiting_workdir(on: u64) -> std::path::PathBuf {
    let dir = workdir("sessions");
    let started = r#"{"initiation": "own_initiative", "title": "Add a README", "prompt": "Write a short README.md"}"#;
    let waits = format!(r#"{{"session_id": {on}, "then": "Cut version 0.1.0"}}"#);
    let capture = std::fs::read_to_string(fixture("sessions"))
        .unwrap()
        .replace(started, &waits)
        .replace("start_session", "wait_for_session");
    std::fs::write(dir.join("wait.jsonl"), capture).unwrap();
    std::fs::write(
        dir.join(".fake-agent"),
        dir.join("wait.jsonl").to_string_lossy().as_bytes(),
    )
    .unwrap();
    dir
}

fn send(engine: &Engine, session: u64, text: &str) {
    engine.send(Intent::Send {
        session,
        text: text.into(),
        mentions: vec![],
        images: vec![],
        lane: Lane::Steer,
    });
}

fn turn_finished(session: u64) -> impl Fn(&Event) -> bool {
    move |e| matches!(e, Event::Agent { session: s, event: AgentEvent::TurnFinished { .. }, .. } if *s == session)
}

/// The wait the agent of session 2 asks for on session 1, as stored: session 2's turn ends
/// with it set, and nothing wakes it yet.
fn waits_for_one(engine: &Engine, rx: &Receiver<Event>, agent: &str) -> Vec<Event> {
    engine.send(Intent::NewSession(NewSession {
        agent: agent.into(),
        cwd: waiting_workdir(1),
        model: None,
        effort: None,
        mode: None,
    }));
    send(engine, 2, "Cut 0.1.0 when the other conversation finishes");
    let events = until(rx, turn_finished(2));
    let waits: Vec<&quill_model::Wait> = (events.iter())
        .filter_map(|e| match e {
            Event::Waiting { session: 2, wait } => Some(wait),
            _ => None,
        })
        .collect();
    assert_eq!(waits.len(), 1, "{events:#?}");
    assert_eq!(
        (waits[0].on, waits[0].then.as_str()),
        (1, "Cut version 0.1.0")
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::WaitEnded { .. } | Event::Reported { .. })));
    events
}

/// The report that woke session 2: its wait on session 1 ended first, as stored.
fn woken(rx: &Receiver<Event>) -> quill_model::Report {
    let events = until(rx, |e| matches!(e, Event::Reported { session: 2, .. }));
    let ended = events.iter().position(|e| {
        matches!(
            e,
            Event::WaitEnded {
                session: 2,
                on: 1,
                outcome: quill_model::WaitOutcome::Finished,
                ..
            }
        )
    });
    assert!(ended.is_some(), "{events:#?}");
    let Some(Event::Reported { reports, .. }) = events.last() else {
        unreachable!()
    };
    assert_eq!(reports.len(), 1);
    reports[0].clone()
}

#[test]
fn a_wait_is_kept_across_a_restart_and_wakes_its_session_when_the_other_finishes() {
    let database = std::env::temp_dir().join(format!("quill-test-wait-{}.db", uuid_like()));
    let start = || {
        let (tx, rx) = channel();
        let programs = HashMap::from([(
            "claude".to_string(),
            env!("CARGO_BIN_EXE_fake-agent").to_string(),
        )]);
        let config = quill_engine::Config {
            programs,
            database: Some(database.clone()),
            ..Default::default()
        };
        (
            Engine::start_with(move |event| drop(tx.send(event)), config),
            rx,
        )
    };
    let (engine, rx) = start();
    // Session 1 holds its turn open at the capture's steer line.
    busy_steer_session(&engine, &rx);
    waits_for_one(&engine, &rx, "claude");
    drop(engine);

    // Told again at the start, and armed: nothing ended it.
    let (engine, rx) = start();
    let replayed = until(&rx, |e| matches!(e, Event::Restored));
    assert!(replayed
        .iter()
        .any(|e| matches!(e, Event::Waiting { session: 2, wait } if wait.on == 1)));
    assert!(!replayed
        .iter()
        .any(|e| matches!(e, Event::WaitEnded { .. })));
    // Session 1 goes on and finishes; session 2 hears what it said last.
    send(&engine, 1, "Count from 1 to 40");
    until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                session: 1,
                event: AgentEvent::Ready { .. },
                ..
            }
        )
    });
    send(&engine, 1, "Change of plans: stop at 5.");
    let report = woken(&rx);
    assert_eq!(report.from, 1);
    assert_eq!(
        report.waited,
        Some((
            quill_model::WaitOutcome::Finished,
            "Cut version 0.1.0".into()
        ))
    );
    assert!(report.text.starts_with("1\n2\n3"));
    // Woken, session 2's agent runs its capture again and asks to wait once more: session 1 is
    // done by then, so nothing is kept, and the wait that was used up wakes no one again.
    let events = until(&rx, turn_finished(2));
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::Reported { .. } | Event::Waiting { .. })));
    drop(engine);
    let _ = std::fs::remove_file(&database);
}

/// Claude's session waits on Codex's: the wait holds while Codex asks the person, and wakes
/// once its turn ends.
#[test]
fn a_wait_holds_while_the_other_agent_asks_and_wakes_across_agents() {
    let (tx, rx) = channel();
    let fake = env!("CARGO_BIN_EXE_fake-agent").to_string();
    let programs = HashMap::from([
        ("claude".to_string(), fake.clone()),
        ("codex".to_string(), fake),
    ]);
    let config = quill_engine::Config {
        programs,
        ..Default::default()
    };
    let engine = Engine::start_with(move |event| drop(tx.send(event)), config);
    let dir = workdir("simple");
    let capture = format!(
        "{}/../quill-agent-codex/fixtures/0.163.0/deny.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::write(dir.join(".fake-agent"), capture).unwrap();
    engine.send(Intent::NewSession(NewSession {
        agent: "codex".into(),
        cwd: dir,
        model: Some("gpt-6-luna".into()),
        effort: Some("low".into()),
        mode: None,
    }));
    send(
        &engine,
        1,
        "Use the shell tool to run exactly: touch denied.txt",
    );
    let events = until(&rx, |e| {
        matches!(
            e,
            Event::Agent {
                session: 1,
                event: AgentEvent::PermissionRequest { .. },
                ..
            }
        )
    });
    let Some(Event::Agent {
        event: AgentEvent::PermissionRequest { id, .. },
        ..
    }) = events.last()
    else {
        unreachable!()
    };
    let permission = id.clone();
    waits_for_one(&engine, &rx, "claude");
    engine.send(Intent::Decide {
        session: 1,
        permission,
        decision: Decision::Deny {
            message: "no".into(),
        },
    });
    let report = woken(&rx);
    assert_eq!(report.from, 1);
    assert!(!report.text.is_empty());
}
