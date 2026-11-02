//! The DevTools protocol over Chromium's pipe: commands matched to their replies by id,
//! events handed on with the session they belong to. Only `serde_json::Value`s cross it;
//! what a method means is the caller's.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// A command that failed: Chromium's own error, no reply in time, or Chromium gone.
#[derive(Clone, Debug, PartialEq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// An event Chromium sent, with the page session it came from (`None` for the browser's).
#[derive(Clone, Debug)]
pub struct Message {
    pub method: String,
    pub params: Value,
    pub session: Option<String>,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, Error>>>>>;

#[derive(Clone)]
pub struct Cdp {
    out: mpsc::UnboundedSender<Vec<u8>>,
    pending: Pending,
    next: Arc<AtomicU64>,
    /// Cleared, before what waits is failed, once Chromium's end of the pipe closes.
    alive: Arc<AtomicBool>,
}

/// How long a command may go unanswered; Desktop gives its tools 30 s.
pub const TIMEOUT: Duration = Duration::from_secs(30);

impl Cdp {
    /// Read from `reader` and write to `writer` on threads of their own. Events go to
    /// `events`, which closes when Chromium does.
    pub fn start(
        mut reader: impl Read + Send + 'static,
        mut writer: impl Write + Send + 'static,
        events: mpsc::UnboundedSender<Message>,
    ) -> Self {
        let pending: Pending = Arc::default();
        let (out, mut lines) = mpsc::unbounded_channel::<Vec<u8>>();
        std::thread::Builder::new()
            .name("quill-cdp-write".into())
            .spawn(move || {
                while let Some(mut line) = lines.blocking_recv() {
                    line.push(0);
                    if writer.write_all(&line).is_err() {
                        return;
                    }
                }
            })
            .expect("cdp writer thread");
        let replies = pending.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let living = alive.clone();
        std::thread::Builder::new()
            .name("quill-cdp-read".into())
            .spawn(move || {
                let mut buffer = Vec::new();
                let mut chunk = vec![0; 1 << 16];
                loop {
                    let read = match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => read,
                    };
                    buffer.extend_from_slice(&chunk[..read]);
                    while let Some(end) = buffer.iter().position(|b| *b == 0) {
                        let raw: Vec<u8> = buffer.drain(..=end).collect();
                        let Ok(message) = serde_json::from_slice::<Value>(&raw[..end]) else {
                            continue;
                        };
                        dispatch(message, &replies, &events);
                    }
                }
                // Chromium is gone: nothing waiting will be answered.
                living.store(false, Ordering::SeqCst);
                for (_, waiter) in replies.lock().unwrap().drain() {
                    let _ = waiter.send(Err(Error("the browser stopped".into())));
                }
            })
            .expect("cdp reader thread");
        Self {
            out,
            pending,
            next: Arc::default(),
            alive,
        }
    }

    /// Send `method` to `session` (the browser's own with `None`) and wait for its reply.
    pub async fn call(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value, Error> {
        self.call_within(method, params, session, TIMEOUT).await
    }

    pub async fn call_within(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
        limit: Duration,
    ) -> Result<Value, Error> {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        // Registered after the reader gave up, it would wait for nothing.
        if !self.alive.load(Ordering::SeqCst)
            || self.out.send(encode(id, method, params, session)).is_err()
        {
            self.pending.lock().unwrap().remove(&id);
            return Err(Error("the browser stopped".into()));
        }
        match tokio::time::timeout(limit, rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err(Error("the browser stopped".into())),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(Error(format!(
                    "{method} took longer than {}s",
                    limit.as_secs()
                )))
            }
        }
    }

    /// Whether Chromium's end of the pipe is still open.
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Send `method` without waiting: input and frame acknowledgements, in order.
    pub fn send(&self, method: &str, params: Value, session: Option<&str>) {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.out.send(encode(id, method, params, session));
    }
}

fn encode(id: u64, method: &str, params: Value, session: Option<&str>) -> Vec<u8> {
    let mut message = json!({ "id": id, "method": method, "params": params });
    if let Some(session) = session {
        message["sessionId"] = json!(session);
    }
    message.to_string().into_bytes()
}

fn dispatch(message: Value, replies: &Pending, events: &mpsc::UnboundedSender<Message>) {
    if let Some(id) = message["id"].as_u64() {
        let Some(waiter) = replies.lock().unwrap().remove(&id) else {
            return;
        };
        let reply = match message.get("error") {
            Some(error) => Err(Error(
                error["message"]
                    .as_str()
                    .unwrap_or("the browser refused")
                    .to_owned(),
            )),
            None => Ok(message["result"].clone()),
        };
        let _ = waiter.send(reply);
        return;
    }
    let Some(method) = message["method"].as_str() else {
        return;
    };
    let _ = events.send(Message {
        method: method.to_owned(),
        session: message["sessionId"].as_str().map(str::to_owned),
        params: message["params"].clone(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A pipe end that is written by the test and read by the reader thread.
    struct Feed(std::sync::mpsc::Receiver<Vec<u8>>, Cursor<Vec<u8>>);

    impl Read for Feed {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            loop {
                let read = self.1.read(out)?;
                if read > 0 {
                    return Ok(read);
                }
                match self.0.recv() {
                    Ok(next) => self.1 = Cursor::new(next),
                    Err(_) => return Ok(0),
                }
            }
        }
    }

    struct Sink(std::sync::mpsc::Sender<Vec<u8>>);

    impl Write for Sink {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            let _ = self.0.send(data.to_vec());
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn replies_find_their_call_and_events_keep_their_session() {
        let (feed, from_browser) = std::sync::mpsc::channel();
        let (sink, to_browser) = std::sync::mpsc::channel();
        let (events, mut heard) = mpsc::unbounded_channel();
        let cdp = Cdp::start(Feed(from_browser, Cursor::new(vec![])), Sink(sink), events);
        let call = tokio::spawn({
            let cdp = cdp.clone();
            async move {
                cdp.call(
                    "Page.navigate",
                    json!({"url": "http://localhost:3000"}),
                    Some("S1"),
                )
                .await
            }
        });
        let written = tokio::task::spawn_blocking(move || to_browser.recv().unwrap())
            .await
            .unwrap();
        assert_eq!(written.last(), Some(&0));
        let sent: Value = serde_json::from_slice(&written[..written.len() - 1]).unwrap();
        assert_eq!(sent["method"], "Page.navigate");
        assert_eq!(sent["sessionId"], "S1");
        // An event and the reply arrive in one read, split by NULs.
        let mut bytes =
            br#"{"method":"Page.loadEventFired","params":{"timestamp":1},"sessionId":"S1"}"#
                .to_vec();
        bytes.push(0);
        bytes.extend(format!(r#"{{"id":{},"result":{{"frameId":"F"}}}}"#, sent["id"]).into_bytes());
        bytes.push(0);
        feed.send(bytes).unwrap();
        assert_eq!(call.await.unwrap(), Ok(json!({"frameId": "F"})));
        let event = heard.recv().await.unwrap();
        assert_eq!(
            (event.method.as_str(), event.session.as_deref()),
            ("Page.loadEventFired", Some("S1"))
        );
        // Chromium's refusal is the call's error; its going away fails what still waits.
        let failing = tokio::spawn({
            let cdp = cdp.clone();
            async move { cdp.call("Nope.nothing", json!({}), None).await }
        });
        drop(feed);
        assert_eq!(
            failing.await.unwrap(),
            Err(Error("the browser stopped".into()))
        );
        assert!(heard.recv().await.is_none());
    }
}
