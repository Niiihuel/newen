//! Against the Chromium on this machine; skipped where there is none.

use newen_browser::{Browser, Frames, Grant, Options, Permit};
use newen_model::{BrowserInput, BrowserOp, BrowserUpdate, BrowserView, Modifiers, PointerButton};
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PAGE: &str = "<!doctype html><title>Acme</title><body style='margin:0'>\
<input id=email style='position:absolute;left:10px;top:10px;width:200px;height:30px'>\
<p id=words style='position:absolute;top:80px'>hello Newen</p>\
<script>console.log('ready', 42)</script></body>";

/// The person allows every site once.
fn allow_once() -> Permit {
    Arc::new(|_, _| {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let _ = tx.send(Grant::Once);
        rx
    })
}

/// A one-page server on a free local port.
fn serve() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{PAGE}",
                PAGE.len()
            );
        }
    });
    port
}

async fn until<T>(what: &str, mut check: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(found) = check() {
            return found;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "waited too long for {what}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_opens_a_page_sees_its_frames_and_types_into_it() {
    let dir = std::env::temp_dir().join(format!("newen-browser-test-{}", std::process::id()));
    let frames = Frames::default();
    let heard: Arc<Mutex<Vec<(u64, BrowserUpdate)>>> = Arc::default();
    let log = heard.clone();
    let browser = Browser::new(
        Options {
            program: None,
            profile: dir.join("profile"),
            downloads: dir.join("downloads"),
            frames: frames.clone(),
            permit: allow_once(),
            native: None,
        },
        Arc::new(move |owner, update| log.lock().unwrap().push((owner, update))),
    );
    let port = serve();
    browser
        .apply(
            7,
            BrowserOp::Open {
                url: Some(format!("localhost:{port}/")),
            },
        )
        .await;
    if heard
        .lock()
        .unwrap()
        .iter()
        .any(|(_, u)| matches!(u, BrowserUpdate::Failed(e) if e.starts_with("No Chromium")))
    {
        eprintln!("no Chromium here: skipped");
        return;
    }
    let tab = until("the page's title", || {
        heard
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|(owner, update)| match update {
                BrowserUpdate::Tabs { tabs, .. } if *owner == 7 => {
                    tabs.iter().find(|t| t.title == "Acme").map(|t| t.id)
                }
                _ => None,
            })
    })
    .await;
    browser
        .apply(
            7,
            BrowserOp::Show {
                view: Some(BrowserView {
                    tab,
                    width: 400,
                    height: 300,
                }),
            },
        )
        .await;
    let frame = until("a frame", || {
        frames
            .take()
            .into_iter()
            .find(|(id, _)| *id == format!("browser:7:{tab}"))
    })
    .await
    .1;
    assert_eq!((frame.width, frame.height), (400, 300));
    // A click in the field, then typing, as the pane sends them.
    let keys = Modifiers::default();
    for input in [
        BrowserInput::Down {
            x: 50.0,
            y: 25.0,
            button: PointerButton::Left,
            clicks: 1,
            keys,
        },
        BrowserInput::Up {
            x: 50.0,
            y: 25.0,
            button: PointerButton::Left,
            keys,
        },
        BrowserInput::Text("alex@acme.test".into()),
    ] {
        browser.apply(7, BrowserOp::Input { tab, input }).await;
    }
    // The person's input is sent without waiting for Chromium to take it.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let typed = browser.evaluate(7, tab, "email.value").await.unwrap();
    assert_eq!(typed.as_str(), Some("alex@acme.test"));
    // What is selected is copied.
    browser
        .evaluate(7, tab, "getSelection().selectAllChildren(words)")
        .await
        .unwrap();
    browser.apply(7, BrowserOp::Copy { tab }).await;
    assert!(heard
        .lock()
        .unwrap()
        .iter()
        .any(|(_, u)| *u == BrowserUpdate::Copied("hello Newen".into())));
    // The tab says who set its size: the agent's resize, or the person's Viewport menu.
    let sized = || {
        heard
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|(owner, update)| match update {
                BrowserUpdate::Tabs { tabs, .. } if *owner == 7 => tabs
                    .iter()
                    .find(|t| t.id == tab)
                    .map(|t| (t.emulated, t.emulated_by_agent)),
                _ => None,
            })
    };
    let (_, failed) = browser
        .tool(
            7,
            "resize_window",
            serde_json::json!({ "width": 1440, "height": 900, "tabId": tab }),
        )
        .await;
    assert!(!failed);
    assert_eq!(sized(), Some((Some((1440, 900)), true)));
    let size = Some((375, 812));
    browser.apply(7, BrowserOp::Viewport { tab, size }).await;
    assert_eq!(sized(), Some((size, false)));
    browser
        .apply(7, BrowserOp::Viewport { tab, size: None })
        .await;
    assert_eq!(sized(), Some((None, false)));
    // Another session has cookies of its own.
    browser
        .evaluate(7, tab, "document.cookie='who=seven'")
        .await
        .unwrap();
    browser
        .apply(
            8,
            BrowserOp::Open {
                url: Some(format!("localhost:{port}/")),
            },
        )
        .await;
    let other = until("session 8's tab", || {
        heard
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|(owner, update)| match update {
                BrowserUpdate::Tabs { tabs, .. } if *owner == 8 => tabs.first().map(|t| t.id),
                _ => None,
            })
    })
    .await;
    let cookie = browser.evaluate(8, other, "document.cookie").await.unwrap();
    assert_eq!(cookie.as_str(), Some(""));
    browser.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

const FORM: &str = "<!doctype html><meta charset=utf-8><title>Checkout</title><body>\
<h1>ACME</h1><label>Email <input id=email></label>\
<label><input type=radio name=ship value=std>Standard</label>\
<label><input type=radio name=ship value=exp>Express</label>\
<button onclick=\"done.textContent='Paid with '+document.querySelector('[name=ship]:checked').value;console.error('paid')\">Pay</button>\
<p id=done></p><input type=password value=hunter2></body>";

fn serve_page(page: &'static str) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}",
                page.len()
            );
        }
    });
    port
}

fn words(content: &[newen_model::ToolContent]) -> String {
    content
        .iter()
        .filter_map(|c| match c {
            newen_model::ToolContent::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_reads_fills_clicks_and_sees_a_page() {
    use serde_json::json;
    let dir = std::env::temp_dir().join(format!("newen-browser-tools-{}", std::process::id()));
    let browser = Browser::new(
        Options {
            program: None,
            profile: dir.join("profile"),
            downloads: dir.join("downloads"),
            frames: Frames::default(),
            permit: allow_once(),
            native: None,
        },
        Arc::new(|_, _| {}),
    );
    let port = serve_page(FORM);
    let (opened, failed) = browser
        .tool(5, "navigate", json!({ "url": format!("localhost:{port}") }))
        .await;
    if failed && words(&opened).starts_with("No Chromium") {
        eprintln!("no Chromium here: skipped");
        return;
    }
    assert!(!failed, "{}", words(&opened));
    assert!(
        words(&opened).contains("Tab Context:"),
        "{}",
        words(&opened)
    );
    let (page, _) = browser.tool(5, "read_page", json!({})).await;
    let page = words(&page);
    // The password's value is never read out.
    assert!(
        page.contains("value=[redacted]") && !page.contains("hunter2"),
        "{page}"
    );
    let line = |name: &str| {
        page.lines()
            .find(|l| l.contains(name))
            .and_then(|l| l.split('[').nth(1))
            .and_then(|r| r.split(']').next())
            .unwrap_or_else(|| panic!("no {name} in {page}"))
            .to_owned()
    };
    let (email, express, pay) = (
        line("textbox \"Email\""),
        line("radio \"Express\""),
        line("button \"Pay\""),
    );
    let (set, failed) = browser
        .tool(
            5,
            "form_input",
            json!({ "ref": email, "value": "alex@acme.test" }),
        )
        .await;
    assert!(!failed, "{}", words(&set));
    browser
        .tool(5, "form_input", json!({ "ref": express, "value": true }))
        .await;
    let (clicked, failed) = browser
        .tool(5, "computer", json!({ "action": "left_click", "ref": pay }))
        .await;
    assert!(!failed, "{}", words(&clicked));
    let (done, _) = browser
        .tool(
            5,
            "javascript_tool",
            json!({ "text": "done.textContent + ' / ' + email.value" }),
        )
        .await;
    assert!(
        words(&done).starts_with("Paid with exp / alex@acme.test"),
        "{}",
        words(&done)
    );
    let (found, _) = browser.tool(5, "find", json!({ "query": "pay" })).await;
    assert!(words(&found).contains(&pay), "{}", words(&found));
    let (console, _) = browser
        .tool(5, "read_console_messages", json!({ "only_errors": true }))
        .await;
    assert!(
        words(&console).contains("[error] paid"),
        "{}",
        words(&console)
    );
    let (shot, failed) = browser
        .tool(5, "computer", json!({ "action": "screenshot" }))
        .await;
    assert!(
        !failed && matches!(shot[0], newen_model::ToolContent::Image { .. }),
        "{}",
        words(&shot)
    );
    // A batch stops at its first failure.
    let (batch, _) = browser
        .tool(
            5,
            "browser_batch",
            json!({ "actions": [
                { "tool": "get_page_text" },
                { "tool": "form_input", "input": { "ref": "ref_999", "value": "x" } },
                { "tool": "tabs_context" },
            ]}),
        )
        .await;
    let batch = words(&batch);
    assert!(
        batch.contains("2. form_input failed") && !batch.contains("3. tabs_context"),
        "{batch}"
    );
    browser.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pages_file_field_asks_for_files_and_takes_the_ones_chosen() {
    use serde_json::json;
    let dir = std::env::temp_dir().join(format!("newen-browser-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("logo.txt");
    std::fs::write(&file, "acme").unwrap();
    let heard: Arc<Mutex<Vec<BrowserUpdate>>> = Arc::default();
    let log = heard.clone();
    let browser = Browser::new(
        Options {
            program: None,
            profile: dir.join("profile"),
            downloads: dir.join("downloads"),
            frames: Frames::default(),
            permit: allow_once(),
            native: None,
        },
        Arc::new(move |_, update| log.lock().unwrap().push(update)),
    );
    let port = serve_page("<!doctype html><input type=file id=f>");
    let (opened, failed) = browser
        .tool(9, "navigate", json!({ "url": format!("localhost:{port}") }))
        .await;
    if failed && words(&opened).starts_with("No Chromium") {
        eprintln!("no Chromium here: skipped");
        return;
    }
    let (page, _) = browser.tool(9, "find", json!({ "query": "button" })).await;
    let reference = words(&page)
        .split('[')
        .nth(1)
        .and_then(|r| r.split(']').next())
        .unwrap_or_else(|| panic!("no field in {}", words(&page)))
        .to_owned();
    browser
        .tool(
            9,
            "computer",
            json!({ "action": "left_click", "ref": reference }),
        )
        .await;
    let (tab, node) = until("the page to ask for files", || {
        heard.lock().unwrap().iter().find_map(|u| match u {
            BrowserUpdate::ChooseFiles { tab, node, .. } => Some((*tab, *node)),
            _ => None,
        })
    })
    .await;
    browser
        .apply(
            9,
            BrowserOp::Files {
                tab,
                node,
                paths: vec![file.clone()],
            },
        )
        .await;
    let name = browser
        .evaluate(9, tab, "f.files[0] && f.files[0].name")
        .await
        .unwrap();
    assert_eq!(name.as_str(), Some("logo.txt"));
    browser.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sessions_tabs_from_before_open_again_with_its_browser() {
    let dir = std::env::temp_dir().join(format!("newen-browser-kept-{}", std::process::id()));
    let heard: Arc<Mutex<Vec<(u64, BrowserUpdate)>>> = Arc::default();
    let log = heard.clone();
    let browser = Browser::new(
        Options {
            program: None,
            profile: dir.join("profile"),
            downloads: dir.join("downloads"),
            frames: Frames::default(),
            permit: allow_once(),
            native: None,
        },
        Arc::new(move |owner, update| log.lock().unwrap().push((owner, update))),
    );
    let url = format!("http://localhost:{}/", serve());
    browser.remember(4, vec![url.clone()]);
    browser.apply(4, BrowserOp::Open { url: None }).await;
    if heard
        .lock()
        .unwrap()
        .iter()
        .any(|(_, u)| matches!(u, BrowserUpdate::Failed(e) if e.starts_with("No Chromium")))
    {
        eprintln!("no Chromium here: skipped");
        return;
    }
    until("the tab from before", || {
        heard.lock().unwrap().iter().find_map(|(_, u)| match u {
            BrowserUpdate::Kept(urls) if urls == &vec![url.clone()] => Some(()),
            _ => None,
        })
    })
    .await;
    browser.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}
