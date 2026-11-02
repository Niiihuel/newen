//! A stand-in for an agent CLI in tests: replays the capture named in `.fake-agent` in its
//! working directory (each test session gets its own directory, so tests run side by side).
//! The file tools it replays (Write, Edit, Bash) really run there, so the folder changes as
//! it did live and the engine attribution can be tested.
//! Prints each line the real CLI sent, and where quill sent a line in the capture it
//! waits for one on stdin, so the engine's writes pace the replay as they did live.
//! `NEWEN_FAKE_PACE=<factor>` also keeps the capture's time between lines, scaled, so a
//! person can watch a turn run.
//!
//! It also answers what quill asks of a program outside a session (`mod ext`): `mcp` and
//! `plugin` subcommands, in the words of Claude Code or of Codex by the name it is run as
//! (`fake-codex` is Codex; anything else is Claude Code), and, without a capture in the working
//! directory, the app-server and control requests that list skills. A line quill sends with an
//! id that starts `quill-` is answered on its own and does not count as one of the capture's.

use std::io::{BufRead, Write};

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("auth") => return auth(),
        Some("login" | "logout") => return codex_auth(),
        Some("mcp") => return ext::mcp(),
        Some("plugin") => return ext::plugin(),
        _ => {}
    }
    if std::env::var_os("NEWEN_FAKE_APP_SERVER").is_some() {
        return app_server();
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock().lines();
    let Ok(path) = std::fs::read_to_string(".fake-agent") else {
        // No capture here: a program asked outside a session.
        while let Some(Ok(line)) = input.next() {
            if let Some(answer) = ext::answer(&line, true) {
                println!("{answer}");
                let _ = std::io::stdout().flush();
            }
        }
        return;
    };
    let path = path.trim();
    // What the engine launched it with, for tests: its arguments, then the variables the
    // "All agents" layer sets (`NEWEN_MCP_…` and the like) as `NAME=value`.
    let launched = std::env::args().skip(1).collect::<Vec<_>>().join("\n");
    let vars: String = std::env::vars()
        .filter(|(k, _)| k.starts_with("NEWEN_MCP_") || k == "GITHUB_TOKEN")
        .map(|(k, v)| format!("\n{k}={v}"))
        .collect();
    let _ = std::fs::write(".fake-launch", launched + &vars);
    let text = std::fs::read_to_string(path).expect("fixture");
    let mut out = std::io::stdout().lock();
    let pace: Option<f64> = std::env::var("NEWEN_FAKE_PACE")
        .ok()
        .and_then(|p| p.parse().ok());
    let mut last = 0u64;
    for line in text.lines() {
        let entry: serde_json::Value = serde_json::from_str(line).expect("fixture line");
        let at = entry["t_ms"].as_u64().unwrap_or(last);
        if let Some(pace) = pace {
            let wait = at.saturating_sub(last) as f64 * pace;
            std::thread::sleep(std::time::Duration::from_millis(wait as u64));
        }
        last = at;
        match entry["dir"].as_str() {
            Some("out") if next_line(&mut input).is_none() => return,
            Some("in") => {
                run_tools(&entry["msg"]);
                writeln!(out, "{}", entry["msg"]).unwrap();
                out.flush().unwrap();
            }
            _ => {}
        }
    }
    // The real CLI stays until its stdin closes, answering what quill asks of it on its own.
    for line in input.map_while(Result::ok) {
        if let Some(answer) = ext::answer(&line, false) {
            writeln!(out, "{answer}").unwrap();
            out.flush().unwrap();
        }
    }
}

/// `NEWEN_FAKE_APP_SERVER`: a Codex app-server for any number of threads, as the one quill's
/// Codex sessions share (docs/remote-control.md §3). Each turn answers "echo: <its text>".
fn app_server() {
    use serde_json::json;
    let mut threads = 0;
    let mut out = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let (id, params) = (msg["id"].clone(), &msg["params"]);
        let mut said = vec![];
        let result = match msg["method"].as_str() {
            _ if id.is_null() => continue,
            Some("initialize") => json!({ "userAgent": "codex/0.163.0 (Fake)" }),
            Some("model/list") => json!({ "data": [] }),
            Some("thread/start") => {
                threads += 1;
                let thread = json!({ "id": format!("t{threads}"), "parentThreadId": null });
                said.push(json!({ "method": "thread/started", "params": { "thread": thread } }));
                json!({ "thread": thread, "model": "gpt-6-luna" })
            }
            Some("turn/start") => {
                let thread = params["threadId"].clone();
                let words = params["input"][0]["text"].as_str().unwrap_or_default();
                let turn = json!({ "id": format!("u-{}", thread.as_str().unwrap_or_default()) });
                said.push(json!({ "method": "turn/started", "params": { "threadId": thread, "turn": turn } }));
                said.push(json!({ "method": "item/completed", "params": { "threadId": thread,
                    "item": { "type": "agentMessage", "id": "a1", "text": format!("echo: {words}") } } }));
                said.push(
                    json!({ "method": "turn/completed", "params": { "threadId": thread,
                    "turn": { "id": turn["id"], "status": "completed", "items": [] } } }),
                );
                json!({ "turn": turn })
            }
            Some("remoteControl/enable") => {
                said.push(json!({ "method": "remoteControl/status/changed",
                    "params": { "status": "connected", "serverName": "fake", "installationId": "i", "environmentId": "e" } }));
                json!({ "status": "connecting", "serverName": "fake", "installationId": "i", "environmentId": null })
            }
            _ => json!({}),
        };
        writeln!(out, "{}", json!({ "id": id, "result": result })).unwrap();
        for line in said {
            writeln!(out, "{line}").unwrap();
        }
        out.flush().unwrap();
    }
}

/// The next line quill wrote that belongs to the capture: the ones with ids of quill's own
/// (`ext::answer`) are answered on the way.
fn next_line(input: &mut impl Iterator<Item = std::io::Result<String>>) -> Option<String> {
    loop {
        let line = input.next()?.ok()?;
        match ext::answer(&line, false) {
            Some(answer) => {
                println!("{answer}");
                let _ = std::io::stdout().flush();
            }
            None => return Some(line),
        }
    }
}

/// `auth status|login|logout` as Claude Code 2.1.312 answers them. The
/// account starts as `NEWEN_FAKE_AUTH` says (`signed-in`, the default, or `signed-out`); a login
/// or a logout flips it through a marker file (`NEWEN_FAKE_AUTH_MARKER`, else `.fake-auth` in the
/// working directory), so the next `status` agrees.
fn auth() {
    let args: Vec<String> = std::env::args().collect();
    let marker = std::env::var("NEWEN_FAKE_AUTH_MARKER").unwrap_or_else(|_| ".fake-auth".into());
    let base = std::env::var("NEWEN_FAKE_AUTH").as_deref() != Ok("signed-out");
    let set = |signed_in: bool| {
        if signed_in == base {
            let _ = std::fs::remove_file(&marker);
        } else {
            let _ = std::fs::write(&marker, "");
        }
    };
    let signed_in = base != std::path::Path::new(&marker).exists();
    match args.get(2).map(String::as_str) {
        Some("status") if signed_in => {
            println!(
                r#"{{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","email":"ada@example.com","orgName":"Analytical Engines","subscriptionType":"max"}}"#
            );
        }
        Some("status") => {
            println!(r#"{{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}}"#);
            std::process::exit(1);
        }
        Some("login") => {
            // The real one wraps the address in an OSC 8 link even without a terminal, and
            // its last line has no newline.
            let url = "https://claude.com/cai/oauth/authorize?code=true&client_id=fake&state=fake";
            println!("Opening browser to sign in\u{2026}");
            println!("If the browser didn't open, visit: \x1b]8;;{url}\x07{url}\x1b]8;;\x07");
            print!("Paste code here if prompted > ");
            let _ = std::io::stdout().flush();
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                // `fail…` stands for a rejected code: the program gives up.
                if line.starts_with("fail") {
                    eprintln!("Login failed: the code was not accepted");
                    std::process::exit(1);
                }
                if line.contains('#') {
                    set(true);
                    println!("Login successful.");
                    return;
                }
                eprintln!("Invalid code. Please make sure the full code was copied.");
            }
            std::process::exit(130);
        }
        Some("logout") => {
            set(false);
            println!("Successfully logged out from your Anthropic account.");
        }
        _ => std::process::exit(2),
    }
}

/// `login status|(none)` and `logout` as Codex 0.163.0 answers them (docs/codex.md "Fase 1" 6-8,
/// from `codex-rs/cli/src/login.rs`; `login` and `logout` were not run live): all on stderr.
/// The account state is `auth()`'s, so `NEWEN_FAKE_AUTH` and the marker work the same
/// (`NEWEN_FAKE_AUTH_CODEX` sets Codex's alone). `login`
/// prints the address like the real one, waits `NEWEN_FAKE_LOGIN_MS` (the browser coming back to
/// Codex's local server, 1500 by default) and signs in, or ends early when stdin closes (a
/// cancel).
fn codex_auth() {
    let args: Vec<String> = std::env::args().collect();
    let marker = std::env::var("NEWEN_FAKE_AUTH_MARKER").unwrap_or_else(|_| ".fake-auth".into());
    let base = std::env::var("NEWEN_FAKE_AUTH_CODEX")
        .or_else(|_| std::env::var("NEWEN_FAKE_AUTH"))
        .as_deref()
        != Ok("signed-out");
    let set = |signed_in: bool| {
        if signed_in == base {
            let _ = std::fs::remove_file(&marker);
        } else {
            let _ = std::fs::write(&marker, "");
        }
    };
    let signed_in = base != std::path::Path::new(&marker).exists();
    match (args[1].as_str(), args.get(2).map(String::as_str)) {
        ("login", Some("status")) if signed_in => eprintln!("Logged in using ChatGPT"),
        ("login", Some("status")) => {
            eprintln!("Not logged in");
            std::process::exit(1);
        }
        ("login", _) => {
            eprintln!("Starting local login server on http://localhost:1455.");
            eprintln!("If your browser did not open, navigate to this URL to authenticate:");
            eprintln!();
            eprintln!("https://auth.openai.com/oauth/authorize?response_type=code&client_id=fake&state=fake");
            eprintln!();
            eprintln!("On a remote or headless machine? Use `codex login --device-auth` instead.");
            let wait = std::env::var("NEWEN_FAKE_LOGIN_MS")
                .ok()
                .and_then(|ms| ms.parse().ok())
                .unwrap_or(1500);
            std::thread::sleep(std::time::Duration::from_millis(wait));
            set(true);
            eprintln!("Successfully logged in");
        }
        ("logout", _) => {
            set(false);
            eprintln!("Successfully logged out");
        }
        _ => std::process::exit(2),
    }
}

/// Do what the captured tool calls did, in this directory. Paths under `/work/` (the
/// anonymized capture folder) are taken from here.
fn run_tools(msg: &serde_json::Value) {
    if msg["type"] != "assistant" {
        return;
    }
    let local = |path: &str| path.strip_prefix("/work/").unwrap_or(path).to_owned();
    let text = |value: &serde_json::Value| value.as_str().unwrap_or_default().to_owned();
    for block in msg["message"]["content"].as_array().into_iter().flatten() {
        if block["type"] != "tool_use" {
            continue;
        }
        let input = &block["input"];
        match block["name"].as_str() {
            Some("Write") => {
                let _ = std::fs::write(local(&text(&input["file_path"])), text(&input["content"]));
            }
            Some("Edit") => {
                let path = local(&text(&input["file_path"]));
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let edited = content.replacen(
                        &text(&input["old_string"]),
                        &text(&input["new_string"]),
                        1,
                    );
                    let _ = std::fs::write(&path, edited);
                }
            }
            Some("Bash") => {
                let command = text(&input["command"]).replace("/work/", "");
                let _ = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .status();
            }
            _ => {}
        }
    }
}

/// What the two CLIs answer outside a session, with made-up content: the shapes are those the
/// adapters read (`codex mcp list --json`, `claude mcp list`, `plugin list --json --available`,
/// `skills/list`, `mcpServerStatus/list`). `NEWEN_FAKE_EXT=<dir>` keeps what is added and
/// removed there, so the next list shows it.
mod ext {
    use serde_json::{json, Value};

    fn codex() -> bool {
        let program = std::env::args().next().unwrap_or_default();
        program
            .rsplit('/')
            .next()
            .is_some_and(|n| n.contains("codex"))
    }

    /// Where this agent keeps what was added: `NEWEN_FAKE_EXT`, else beside the program as run
    /// (each test links its own).
    fn kept() -> Option<std::path::PathBuf> {
        let dir = match std::env::var("NEWEN_FAKE_EXT") {
            Ok(dir) => std::path::PathBuf::from(dir),
            Err(_) => {
                let program = std::path::PathBuf::from(std::env::args().next()?);
                program.parent()?.join(".fake-ext")
            }
        };
        let name = if codex() { "codex" } else { "claude" };
        Some(dir.join(name))
    }

    /// The servers added here: `name<TAB>target` a line.
    fn added() -> Vec<(String, String)> {
        let text = kept().and_then(|p| std::fs::read_to_string(p.with_extension("servers")).ok());
        (text.unwrap_or_default().lines())
            .filter_map(|l| l.split_once('\t'))
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
            .collect()
    }

    fn keep(servers: &[(String, String)]) {
        if let Some(path) = kept() {
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let text: String = servers.iter().map(|(a, b)| format!("{a}\t{b}\n")).collect();
            let _ = std::fs::write(path.with_extension("servers"), text);
        }
    }

    pub fn mcp() {
        let args: Vec<String> = std::env::args().collect();
        let mut servers = added();
        match args.get(2).map(String::as_str) {
            Some("list") if codex() => {
                let mut rows = vec![
                    json!({"name": "node_repl", "enabled": true, "disabled_reason": null,
                        "transport": {"type": "stdio", "command": "node_repl", "args": [], "env": null, "env_vars": [], "cwd": null},
                        "auth_status": "unsupported"}),
                    json!({"name": "docs", "enabled": true, "disabled_reason": null,
                        "transport": {"type": "streamable_http", "url": "https://docs.example.com/mcp", "bearer_token_env_var": null, "http_headers": null, "env_http_headers": null},
                        "auth_status": "not_logged_in"}),
                    json!({"name": "scratch", "enabled": false, "disabled_reason": "user",
                        "transport": {"type": "stdio", "command": "npx", "args": ["-y", "scratch-mcp"], "env": null, "env_vars": [], "cwd": null},
                        "auth_status": "unsupported"}),
                ];
                for (name, target) in &servers {
                    let (command, rest) = target.split_once(' ').unwrap_or((target, ""));
                    let args: Vec<&str> = rest.split_whitespace().collect();
                    rows.push(json!({"name": name, "enabled": true, "disabled_reason": null,
                        "transport": {"type": "stdio", "command": command, "args": args, "env": null, "env_vars": [], "cwd": null},
                        "auth_status": "unsupported"}));
                }
                println!("{}", Value::Array(rows));
            }
            Some("list") => {
                println!("Checking MCP server health\u{2026}\n");
                println!("github: npx -y @modelcontextprotocol/server-github - \u{2713} Connected");
                println!(
                    "plugin:vercel:vercel: https://mcp.vercel.com (HTTP) - ! Needs authentication"
                );
                println!("claude.ai Figma: https://mcp.figma.com/mcp - \u{2713} Connected");
                for (name, target) in &servers {
                    println!("{name}: {target} - \u{2713} Connected");
                }
            }
            Some("add") => {
                // `add [options] name [-- command args]`: Codex puts the name first, Claude Code
                // after its options; the name is the first word that is not an option.
                let mut words: Vec<String> = vec![];
                let (mut tail, mut url) = (vec![], None);
                let mut rest = args[3..].iter();
                while let Some(arg) = rest.next() {
                    match arg.as_str() {
                        "--" => {
                            tail = rest.by_ref().cloned().collect();
                        }
                        "--url" => url = rest.next().cloned(),
                        "-s"
                        | "--scope"
                        | "-t"
                        | "--transport"
                        | "-e"
                        | "--env"
                        | "-H"
                        | "--header"
                        | "--bearer-token-env-var" => drop(rest.next()),
                        other if other.starts_with('-') => {}
                        other => words.push(other.to_owned()),
                    }
                }
                let name = words.first().cloned().unwrap_or_default();
                let target = match url {
                    Some(url) => url,
                    None => words
                        .get(1)
                        .into_iter()
                        .chain(&tail)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" "),
                };
                servers.push((name.clone(), target));
                keep(&servers);
                println!("Added MCP server {name}.");
            }
            Some("remove") => {
                let name = args.get(3).cloned().unwrap_or_default();
                if servers.iter().any(|(n, _)| *n == name) {
                    servers.retain(|(n, _)| *n != name);
                    keep(&servers);
                    println!("Removed MCP server {name}.");
                } else if codex() {
                    println!("No MCP server named '{name}' found.");
                } else {
                    eprintln!("No MCP server found with name: {name}");
                    std::process::exit(1);
                }
            }
            _ => std::process::exit(2),
        }
    }

    pub fn plugin() {
        let args: Vec<String> = std::env::args().collect();
        let (c, installed, offers) = (codex(), "installed", "available");
        let row = |name: &str, market: &str, description: &str, on: bool| {
            if c {
                json!({"pluginId": format!("{name}@{market}"), "name": name, "marketplaceName": market,
                    "version": "1.2.0", "installed": on, "enabled": on})
            } else {
                json!({"id": format!("{name}@{market}"), "pluginId": format!("{name}@{market}"), "name": name,
                    "marketplaceName": market, "description": description, "version": "1.2.0",
                    "installCount": 4200, "scope": "user", "enabled": on})
            }
        };
        match (
            args.get(2).map(String::as_str),
            args.get(3).map(String::as_str),
        ) {
            (Some("list"), _) => {
                let (have, offer): (Vec<Value>, Vec<Value>) = if c {
                    (
                        vec![
                            row(
                                "documents",
                                "primary-runtime",
                                "Create and edit documents",
                                true,
                            ),
                            row("pdf", "primary-runtime", "Read and fill PDFs", true),
                        ],
                        vec![
                            row("latex", "bundled", "Typeset papers", false),
                            row("charts", "bundled", "Draw charts from data", false),
                        ],
                    )
                } else {
                    (
                        vec![
                            row(
                                "vercel",
                                "claude-plugins-official",
                                "Deploy to Vercel",
                                true,
                            ),
                            row(
                                "frontend-design",
                                "claude-plugins-official",
                                "Build distinctive interfaces",
                                true,
                            ),
                        ],
                        vec![row(
                            "figma",
                            "claude-plugins-official",
                            "Read Figma files",
                            false,
                        )],
                    )
                };
                let list = json!({ installed: have, offers: offer });
                println!("{list}");
            }
            (Some("marketplace"), Some("list")) if c => {
                println!(
                    r#"{{"marketplaces":[{{"name":"primary-runtime","root":"/opt/codex/plugins/primary-runtime","marketplaceSource":{{"sourceType":"local","source":"/opt/codex/plugins/primary-runtime"}}}},{{"name":"bundled","root":"/opt/codex/plugins/bundled"}}]}}"#
                );
            }
            (Some("marketplace"), Some("list")) => {
                println!(
                    r#"[{{"name":"claude-plugins-official","repo":"anthropics/claude-plugins-official"}}]"#
                );
            }
            _ => println!(r#"{{"outcome":"ok","message":"done"}}"#),
        }
    }

    /// The answer to a line quill wrote, if it is one this answers. `bare` is on when no
    /// capture is being replayed: only then are all of Claude Code's control requests these
    /// (the skills' list is answered always: no capture asks it).
    pub fn answer(line: &str, bare: bool) -> Option<String> {
        let msg: Value = serde_json::from_str(line).ok()?;
        let id = msg["id"].as_str();
        let result = match (msg["method"].as_str(), id) {
            (Some("initialize"), Some("quill-ext")) => json!({
                "userAgent": "quill/0.163.0 (Fake; x86_64)", "codexHome": "/home/fake/.codex",
                "platformFamily": "unix", "platformOs": "linux"}),
            (Some("skills/list"), Some(_)) => {
                let off = kept()
                    .and_then(|p| std::fs::read_to_string(p.with_extension("off")).ok())
                    .unwrap_or_default();
                let skill = |name: &str, scope: &str, description: &str| {
                    json!({"name": name, "description": description,
                        "path": format!("/home/fake/.agents/skills/{name}/SKILL.md"), "scope": scope,
                        "enabled": !off.lines().any(|l| l == name), "pluginId": null})
                };
                json!({"data": [{"cwd": "/work", "errors": [], "skills": [
                    skill("review", "user", "Review the diff for bugs before it is merged"),
                    skill("release-notes", "user", "Write release notes from the git log"),
                    skill("graphify", "user", "Turn any input into a knowledge graph"),
                    skill("repo-conventions", "repo", "How this repository names and tests things"),
                    skill("skill-creator", "system", "Create or update a skill"),
                ]}]})
            }
            (Some("skills/config/write"), Some(_)) => {
                let name = msg["params"]["path"].as_str().unwrap_or_default();
                let name = name.rsplit('/').nth(1).unwrap_or_default();
                if let Some(path) = kept() {
                    let _ = std::fs::create_dir_all(path.parent().unwrap());
                    let off = path.with_extension("off");
                    let mut lines: Vec<String> = std::fs::read_to_string(&off)
                        .unwrap_or_default()
                        .lines()
                        .map(str::to_owned)
                        .collect();
                    lines.retain(|l| l != name);
                    if msg["params"]["enabled"] == false {
                        lines.push(name.to_owned());
                    }
                    let _ = std::fs::write(&off, lines.join("\n"));
                }
                json!({"effectiveEnabled": msg["params"]["enabled"]})
            }
            (Some("mcpServerStatus/list"), Some(_)) => {
                let tool = |name: &str, description: &str, read_only: bool| {
                    (
                        name.to_owned(),
                        json!({"name": name, "description": description,
                        "annotations": {"readOnlyHint": read_only, "destructiveHint": !read_only}}),
                    )
                };
                let tools = |list: Vec<(String, Value)>| Value::Object(list.into_iter().collect());
                json!({"nextCursor": null, "data": [
                    {"name": "node_repl", "runtimeStatus": "connected", "pluginId": null, "authStatus": "unsupported",
                     "toolsError": null, "tools": tools(vec![
                        tool("js", "Run JavaScript in a persistent session", false),
                        tool("js_reset", "Forget the session's state", true)])},
                    {"name": "docs", "runtimeStatus": "authenticationRequired", "pluginId": null, "authStatus": "notLoggedIn",
                     "toolsError": null, "tools": {}},
                    {"name": "scratch", "runtimeStatus": "disabled", "pluginId": null, "authStatus": "unsupported",
                     "toolsError": null, "tools": {}},
                    {"name": "cua_repl", "runtimeStatus": "failed", "pluginId": "computer-use@bundled", "authStatus": "unsupported",
                     "toolsError": "MCP startup failed: connection closed", "tools": {}},
                    {"name": "codex_apps", "runtimeStatus": "connected", "pluginId": null, "authStatus": "bearerToken",
                     "toolsError": null, "tools": tools(vec![tool("search", "Search the connected apps", true)])},
                ]})
            }
            (Some("config/mcpServer/reload"), Some(_)) => json!({}),
            // Codex's adapter tells a mode back once this is answered.
            (Some("account/read"), Some(_)) => json!({}),
            (Some("mcpServer/oauth/login"), Some(_)) => {
                json!({"authorizationUrl": "https://example.invalid/oauth/authorize?state=fake"})
            }
            // A mode switch, as Claude Code 2.1.316 answers it after its capture: bypass is
            // refused unless the session was launched in it or allowed it.
            _ if msg["type"] == "control_request"
                && msg["request"]["subtype"] == "set_permission_mode" =>
            {
                let mode = &msg["request"]["mode"];
                let args: Vec<String> = std::env::args().collect();
                let launched_in = args
                    .windows(2)
                    .any(|w| w[0] == "--permission-mode" && w[1] == "bypassPermissions");
                let allowed = launched_in
                    || args
                        .iter()
                        .any(|a| a == "--allow-dangerously-skip-permissions");
                if mode == "bypassPermissions" && !allowed {
                    return Some(json!({"type": "control_response", "response": {"subtype": "error",
                        "request_id": msg["request_id"],
                        "error": "Cannot set permission mode to bypassPermissions because the session was not launched with --dangerously-skip-permissions",
                        "error_code": "bypass_not_launched"}}).to_string());
                }
                let taken = json!({"type": "control_response", "response": {"subtype": "success",
                    "request_id": msg["request_id"], "response": {"mode": mode}}});
                let status = json!({"type": "system", "subtype": "status", "status": null, "permissionMode": mode});
                return Some(format!("{taken}\n{status}"));
            }
            // Claude Code's control requests, only when there is no capture to replay.
            _ if msg["type"] == "control_request"
                && (bare || msg["request"]["subtype"] == "get_skills_dialog") =>
            {
                let body = match msg["request"]["subtype"].as_str() {
                    Some("get_skills_dialog") => json!({"skills": [
                        {"name": "review", "display_name": "review", "description": "Review the diff for bugs", "source": "user", "state": "on", "handles": []},
                        {"name": "frontend-design", "display_name": "frontend-design", "description": "Build distinctive interfaces", "source": "plugin", "state": "on", "locked_by": "plugin", "handles": []},
                        {"name": "graphify", "display_name": "graphify", "description": "Turn any input into a knowledge graph", "source": "user", "state": "name-only", "handles": []},
                    ]}),
                    _ => json!({}),
                };
                return Some(
                    json!({"type": "control_response", "response": {"subtype": "success",
                    "request_id": msg["request_id"], "response": body}})
                    .to_string(),
                );
            }
            _ => return None,
        };
        // Only ids of quill's own are answered in a replay; a control request has none.
        id.map(|id| json!({"id": id, "result": result}).to_string())
    }
}
