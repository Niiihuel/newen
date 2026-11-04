//! Replays a capture of a real CLI, for each agent's tests (`newen-capture --agent <id>`):
//! `{t_ms, dir, msg}` a line, `dir` being `spec` (the session the adapter is launched for), `cmd`
//! (what Newen asked of it), `out` (what it wrote), `in` (what the agent said), `stderr` or `exit`.

use newen_agent::{AgentAdapter, AgentCommand, AgentEvent, SessionSpec};
use serde_json::Value;

/// What the adapter told and wrote over a capture.
#[derive(Debug, Default)]
pub struct Replayed {
    pub events: Vec<AgentEvent>,
    pub written: Vec<Value>,
    /// What the agent was sent then, in order: the adapter must write it again.
    pub expected: Vec<Value>,
}

impl Replayed {
    /// The events of the first `TurnFinished` on, up to the next, and so on: one list a turn.
    pub fn turns(&self) -> Vec<&[AgentEvent]> {
        let mut turns = vec![];
        let mut start = 0;
        for (i, event) in self.events.iter().enumerate() {
            if matches!(event, AgentEvent::TurnFinished { .. }) {
                turns.push(&self.events[start..=i]);
                start = i + 1;
            }
        }
        turns
    }
}

pub fn replay(adapter: &mut dyn AgentAdapter, capture: &str) -> Replayed {
    let mut replayed = Replayed::default();
    for line in capture.lines().filter(|l| !l.trim().is_empty()) {
        let entry: Value = serde_json::from_str(line).expect("a capture line");
        let msg = &entry["msg"];
        let written: Vec<String> = match entry["dir"].as_str() {
            Some("spec") => {
                let spec: SessionSpec = serde_json::from_value(msg.clone()).expect("a spec");
                adapter.launch(&spec);
                vec![]
            }
            Some("cmd") => {
                let command: AgentCommand = serde_json::from_value(msg.clone()).expect("a command");
                adapter.encode(command)
            }
            Some("in") => {
                let decoded = adapter.decode(&msg.to_string());
                replayed.events.extend(decoded.events);
                decoded.replies
            }
            Some("out") => {
                // The capture names the Newen that recorded it; the adapter names this one.
                let mut msg = msg.clone();
                if let Some(version) = msg.pointer_mut("/params/clientInfo/version") {
                    *version = env!("CARGO_PKG_VERSION").into();
                }
                replayed.expected.push(msg);
                vec![]
            }
            _ => vec![],
        };
        replayed.written.extend(
            written
                .iter()
                .map(|l| serde_json::from_str::<Value>(l).expect("JSON")),
        );
    }
    replayed
}
