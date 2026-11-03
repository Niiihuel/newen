//! A model of the capture's own for an agent that can take an OpenAI-compatible endpoint
//! (`--fake-model <port>`): chat completions answered from a script per scenario, so the
//! agent's protocol is captured as it really is while what "the model" does is known. Hermes's
//! own wire tests do the same (docs/hermes.md "Capturas"). Streams when asked to.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

/// What the model does next: words, or a call of one of the agent's tools.
enum Reply {
    Text(String),
    /// Words sent slowly, a line every tenth of a second, for a turn to be stopped.
    Slow(String),
    Tool(String, Value),
}

/// Serve on `port` until the process ends.
pub fn serve(port: u16, scenario: String) {
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("the fake model's port");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let scenario = scenario.clone();
            std::thread::spawn(move || {
                let _ = answer(stream, &scenario);
            });
        }
    });
}

fn answer(stream: TcpStream, scenario: &str) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let mut out = stream;
    if first.starts_with("GET") {
        let models =
            json!({ "object": "list", "data": [{ "id": "fixture-model", "object": "model" }] });
        return respond(&mut out, "application/json", &models.to_string());
    }
    let body: Value = serde_json::from_slice(&body).unwrap_or_default();
    // What the agent offered the model, for whoever writes a script (`NEWEN_FAKE_MODEL_LOG`).
    if let Ok(log) = std::env::var("NEWEN_FAKE_MODEL_LOG") {
        let names: Vec<&str> = (body["tools"].as_array().into_iter().flatten())
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        let line = format!(
            "{}\n",
            json!({ "tools": names, "messages": body["messages"] })
        );
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .and_then(|mut f| f.write_all(line.as_bytes()));
    }
    let reply = script(scenario, &body);
    if body["stream"] != true {
        let message = match &reply {
            Reply::Text(words) | Reply::Slow(words) => {
                json!({ "role": "assistant", "content": words })
            }
            Reply::Tool(name, args) => json!({ "role": "assistant", "content": null,
                "tool_calls": [{ "id": "call_1", "type": "function", "function": { "name": name, "arguments": args.to_string() } }] }),
        };
        let done = json!({ "id": "fixture", "object": "chat.completion", "created": 1, "model": "fixture-model",
            "choices": [{ "index": 0, "message": message, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110 } });
        return respond(&mut out, "application/json", &done.to_string());
    }
    write!(
        out,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
    )?;
    let chunk = |delta: Value, finish: Value| {
        let chunk = json!({ "id": "fixture", "object": "chat.completion.chunk", "created": 1, "model": "fixture-model",
            "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }] });
        format!("data: {chunk}\n\n")
    };
    match reply {
        Reply::Text(words) => out.write_all(
            chunk(
                json!({ "role": "assistant", "content": words }),
                json!("stop"),
            )
            .as_bytes(),
        )?,
        Reply::Slow(words) => {
            for line in words.lines() {
                out.write_all(
                    chunk(json!({ "content": format!("{line}\n") }), Value::Null).as_bytes(),
                )?;
                out.flush()?;
                std::thread::sleep(Duration::from_millis(100));
            }
            out.write_all(chunk(json!({}), json!("stop")).as_bytes())?;
        }
        Reply::Tool(name, args) => {
            let call = json!({ "role": "assistant", "tool_calls": [{ "index": 0, "id": format!("call_{}", calls(&body) + 1),
                "type": "function", "function": { "name": name, "arguments": args.to_string() } }] });
            out.write_all(chunk(call, Value::Null).as_bytes())?;
            out.write_all(chunk(json!({}), json!("tool_calls")).as_bytes())?;
        }
    }
    let usage = json!({ "id": "fixture", "object": "chat.completion.chunk", "created": 1, "model": "fixture-model",
        "choices": [], "usage": { "prompt_tokens": 100, "completion_tokens": 10, "total_tokens": 110 } });
    write!(out, "data: {usage}\n\ndata: [DONE]\n\n")?;
    out.flush()
}

fn respond(out: &mut TcpStream, kind: &str, body: &str) -> std::io::Result<()> {
    write!(
        out,
        "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// The tool calls the conversation already had since the person's last message.
fn calls(body: &Value) -> usize {
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    let last_user = messages
        .iter()
        .rposition(|m| m["role"] == "user")
        .unwrap_or(0);
    messages[last_user..]
        .iter()
        .filter(|m| m["role"] == "tool")
        .count()
}

/// The last tool result's words.
fn last_result(body: &Value) -> String {
    let messages = body["messages"].as_array().cloned().unwrap_or_default();
    (messages.iter().rev())
        .find(|m| m["role"] == "tool")
        .map(|m| match &m["content"] {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

/// A tool's name that ends with `suffix`, among those offered or named in what the agent wrote
/// (a deferred tool is named in its search tool's description or a search's result).
fn named(body: &Value, suffix: &str) -> Option<String> {
    let text = body.to_string().replace("\\n", " ");
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .find(|word| word.ends_with(suffix) && word.len() > suffix.len())
        .map(str::to_owned)
}

/// A call of `name`: by itself when the agent offers it, else through its tool-call bridge.
fn call_of(body: &Value, name: &str, args: Value) -> Reply {
    let offered =
        (body["tools"].as_array().into_iter().flatten()).any(|t| t["function"]["name"] == name);
    if offered {
        return Reply::Tool(name.into(), args);
    }
    Reply::Tool(
        "tool_call".into(),
        json!({ "calls": [{ "name": name, "arguments": args }] }),
    )
}

fn script(scenario: &str, body: &Value) -> Reply {
    let step = calls(body);
    let text = |words: &str| Reply::Text(words.into());
    let tool = |name: &str, args: Value| Reply::Tool(name.into(), args);
    let said: Vec<String> = (body["messages"].as_array().into_iter().flatten())
        .filter(|m| m["role"] == "user")
        .map(|m| m["content"].as_str().unwrap_or_default().to_owned())
        .collect();
    match (scenario, step) {
        ("edit-bash", 0) => tool(
            "write_file",
            json!({ "path": "notes.txt", "content": "hello\n" }),
        ),
        ("edit-bash", 1) => tool(
            "patch",
            json!({ "path": "notes.txt", "old_string": "hello", "new_string": "hello world" }),
        ),
        ("edit-bash", 2) => tool(
            "terminal",
            json!({ "command": "mv notes.txt moved.txt && rm moved.txt" }),
        ),
        ("deny" | "always", 0) => tool(
            "write_file",
            json!({ "path": "denied.txt", "content": "hi\n" }),
        ),
        ("plan", 0) => tool(
            "write_file",
            json!({ "path": "planned.txt", "content": "hi\n" }),
        ),
        ("risky", 0) => tool("terminal", json!({ "command": "rm -rf build" })),
        ("todos", 0) => call_of(
            body,
            "todo_list",
            json!({ "todos": [
                { "id": "1", "content": "Create a.txt", "status": "in_progress" },
                { "id": "2", "content": "Create b.txt", "status": "pending" },
                { "id": "3", "content": "List the folder", "status": "pending" }] }),
        ),
        ("todos", 1) => tool("write_file", json!({ "path": "a.txt", "content": "a\n" })),
        ("todos", 2) => call_of(
            body,
            "todo_list",
            json!({ "merge": true, "todos": [
                { "id": "1", "content": "Create a.txt", "status": "completed" },
                { "id": "2", "content": "Create b.txt", "status": "cancelled" },
                { "id": "3", "content": "List the folder", "status": "completed" }] }),
        ),
        ("host", 0) => match named(body, "__list_sessions") {
            Some(name) => call_of(body, &name, json!({})),
            None => tool(
                "tool_search",
                json!({ "queries": ["sessions list sessions"] }),
            ),
        },
        ("host", 1) if !last_result(body).contains("No other sessions") => {
            match named(body, "__list_sessions") {
                Some(name) => call_of(body, &name, json!({})),
                None => text("The list_sessions tool is not offered."),
            }
        }
        ("host", _) => Reply::Text(last_result(body)),
        ("interrupt", _) => Reply::Slow(
            (1..=200)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        ("resume" | "fork", _) => {
            let word = (said.iter())
                .find_map(|s| s.split("Remember the word: ").nth(1))
                .and_then(|rest| rest.split('.').next());
            match (said.len(), word) {
                (n, Some(word)) if n > 1 => text(word),
                _ => text("ok"),
            }
        }
        ("simple", _) => text("ok"),
        _ => text("Done."),
    }
}
