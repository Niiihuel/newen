//! One `codex app-server` for all of Newen's Codex sessions, so a phone paired over Remote
//! Control reaches every one of them (docs/remote-control.md §3). Each session's adapter speaks
//! to the process as if it were its own; this shares the one connection among them.
//!
//! Requests get ids of the connection's own and their answers go back under the session's; the
//! first `initialize` answers every session; what concerns a thread goes to its session (a
//! helper agent's thread to its parent's, by `thread/started`'s `parentThreadId`), and what
//! concerns no thread to every session.

use newen_agent::Mux;
use serde_json::{json, Value};
use std::collections::HashMap;

/// The id of the connection's one `initialize`.
const INIT: &str = "newen-mux-init";

#[derive(Default)]
pub struct CodexMux {
    /// The answer to the first `initialize`, given again to every later session.
    init: Option<Value>,
    /// The `initialize` is on its way.
    initializing: bool,
    /// Sessions waiting for its answer, under their own ids.
    waiting: Vec<(u64, Value)>,
    /// The process had its `initialized`.
    initialized: bool,
    /// Requests sent for a session: the id the process saw, the session, its own id, and
    /// whether the answer opens a thread.
    requests: HashMap<String, (u64, Value, bool)>,
    /// Whose each thread is.
    threads: HashMap<String, u64>,
    /// The sessions on the process, for what is everyone's.
    sessions: Vec<u64>,
    next: u64,
}

impl Mux for CodexMux {
    fn outbound(&mut self, session: u64, line: &str) -> (Vec<String>, Vec<String>) {
        if !self.sessions.contains(&session) {
            self.sessions.push(session);
        }
        let Ok(mut msg) = serde_json::from_str::<Value>(line) else {
            return (vec![line.to_owned()], vec![]);
        };
        let method = msg["method"].as_str().map(str::to_owned);
        match (method.as_deref(), msg.get("id").cloned()) {
            (Some("initialize"), Some(id)) => {
                if let Some(result) = &self.init {
                    return (
                        vec![],
                        vec![json!({ "id": id, "result": result }).to_string()],
                    );
                }
                self.waiting.push((session, id));
                if std::mem::replace(&mut self.initializing, true) {
                    return (vec![], vec![]);
                }
                msg["id"] = json!(INIT);
                (vec![msg.to_string()], vec![])
            }
            (Some("initialized"), None) if std::mem::replace(&mut self.initialized, true) => {
                (vec![], vec![])
            }
            (Some(method), Some(id)) => {
                self.next += 1;
                let wire = format!("newen-mux-{}", self.next);
                let opens = matches!(method, "thread/start" | "thread/resume" | "thread/fork");
                self.requests.insert(wire.clone(), (session, id, opens));
                msg["id"] = json!(wire);
                (vec![msg.to_string()], vec![])
            }
            // Notifications, and answers to the process's own requests, go as they are.
            _ => (vec![line.to_owned()], vec![]),
        }
    }

    fn inbound(&mut self, line: &str) -> Vec<(u64, String)> {
        let Ok(mut msg) = serde_json::from_str::<Value>(line) else {
            return vec![];
        };
        if msg.get("method").is_none() {
            let wire = msg["id"].as_str().unwrap_or_default().to_owned();
            if wire == INIT {
                self.initializing = false;
                if msg.get("result").is_some() {
                    self.init = Some(msg["result"].clone());
                }
                return (self.waiting.drain(..))
                    .map(|(session, id)| {
                        msg["id"] = id;
                        (session, msg.to_string())
                    })
                    .collect();
            }
            let Some((session, id, opens)) = self.requests.remove(&wire) else {
                return vec![];
            };
            if let Some(thread) = msg["result"]["thread"]["id"].as_str().filter(|_| opens) {
                self.threads.insert(thread.to_owned(), session);
            }
            msg["id"] = id;
            return vec![(session, msg.to_string())];
        }
        let params = &msg["params"];
        if msg["method"] == "thread/started" {
            let thread = &params["thread"];
            let owner = (thread["parentThreadId"].as_str()).and_then(|p| self.threads.get(p));
            if let (Some(id), Some(&owner)) = (thread["id"].as_str(), owner) {
                self.threads.insert(id.to_owned(), owner);
            }
        }
        match params["threadId"].as_str() {
            // A thread no session holds (another client's) is not Newen's to show.
            Some(thread) => (self.threads.get(thread))
                .map(|&session| vec![(session, line.to_owned())])
                .unwrap_or_default(),
            None => (self.sessions.iter())
                .map(|&session| (session, line.to_owned()))
                .collect(),
        }
    }

    fn closed(&mut self, session: u64) {
        self.sessions.retain(|&s| s != session);
        self.threads.retain(|_, &mut s| s != session);
        self.requests.retain(|_, (s, ..)| *s != session);
        self.waiting.retain(|(s, _)| *s != session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(v: Value) -> String {
        v.to_string()
    }

    fn parsed(out: &[(u64, String)]) -> Vec<(u64, Value)> {
        (out.iter())
            .map(|(s, l)| (*s, serde_json::from_str(l).unwrap()))
            .collect()
    }

    #[test]
    fn the_first_initialize_answers_every_session_and_goes_once() {
        let mut mux = CodexMux::default();
        let init = line(json!({ "id": 1, "method": "initialize", "params": {} }));
        let (sent, back) = mux.outbound(1, &init);
        assert_eq!(sent.len(), 1);
        assert!(back.is_empty());
        // A second session asks while the first is on its way: it waits.
        assert_eq!(mux.outbound(2, &init), (vec![], vec![]));
        let answer = line(json!({ "id": INIT, "result": { "userAgent": "codex/0.163.0" } }));
        let both = parsed(&mux.inbound(&answer));
        assert_eq!(both.len(), 2);
        assert!(both
            .iter()
            .all(|(_, m)| m["id"] == 1 && m["result"]["userAgent"] == "codex/0.163.0"));
        // A later one is answered here, and only one `initialized` reaches the process.
        let (sent, back) = mux.outbound(3, &init);
        assert!(sent.is_empty() && back.len() == 1);
        let ready = line(json!({ "method": "initialized" }));
        assert_eq!(mux.outbound(1, &ready).0.len(), 1);
        assert!(mux.outbound(2, &ready).0.is_empty());
    }

    #[test]
    fn each_session_gets_its_answers_and_its_threads() {
        let mut mux = CodexMux::default();
        let start = |id| line(json!({ "id": id, "method": "thread/start", "params": {} }));
        let (a, _) = mux.outbound(1, &start(5));
        let (b, _) = mux.outbound(2, &start(5));
        let wire = |sent: &[String]| serde_json::from_str::<Value>(&sent[0]).unwrap()["id"].clone();
        let (wa, wb) = (wire(&a), wire(&b));
        assert_ne!(wa, wb);
        let opened = |id: &Value, thread: &str| {
            line(json!({ "id": id, "result": { "thread": { "id": thread } } }))
        };
        assert_eq!(
            parsed(&mux.inbound(&opened(&wa, "t1"))),
            [(
                1,
                json!({ "id": 5, "result": { "thread": { "id": "t1" } } })
            )]
        );
        mux.inbound(&opened(&wb, "t2"));
        // A thread's notices and requests go to its session alone; its helper's too.
        let item =
            |thread| line(json!({ "method": "item/started", "params": { "threadId": thread } }));
        assert_eq!(
            mux.inbound(&item("t2"))
                .iter()
                .map(|(s, _)| *s)
                .collect::<Vec<_>>(),
            [2]
        );
        let helper = line(
            json!({ "method": "thread/started", "params": { "thread": { "id": "h1", "parentThreadId": "t1" } } }),
        );
        assert_eq!(mux.inbound(&helper).len(), 2);
        let ask = line(
            json!({ "id": 0, "method": "item/commandExecution/requestApproval", "params": { "threadId": "h1" } }),
        );
        assert_eq!(
            mux.inbound(&ask)
                .iter()
                .map(|(s, _)| *s)
                .collect::<Vec<_>>(),
            [1]
        );
        // Another client's thread reaches no one; what is everyone's reaches both.
        assert!(mux.inbound(&item("phone")).is_empty());
        let status = line(
            json!({ "method": "remoteControl/status/changed", "params": { "status": "connected" } }),
        );
        assert_eq!(mux.inbound(&status).len(), 2);
        // A session that left gets nothing more.
        mux.closed(2);
        assert!(mux.inbound(&item("t2")).is_empty());
        assert_eq!(mux.inbound(&status).len(), 1);
    }
}
