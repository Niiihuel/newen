//! The agent's browser tools (docs/browser.md §4, `newen_agent::BROWSER_TOOLS`). What each does
//! and what it answers follows Claude Desktop's: a
//! tree of the page with refs the other tools take, screenshots at most 800 px wide whose
//! pixels the clicks are given in, and a "Tab Context" footer naming the tabs.

use crate::{Browser, Owner, Run};
use newen_model::ToolContent;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The script the page tools run in the page, Newen's own: refs kept per element on the
/// page's window, so a ref read once still names its element later.
const HELPER: &str = r#"(() => {
const Q = window.__newen || (window.__newen = { next: 1, refs: new Map(), ids: new WeakMap() });
const refOf = el => { let r = Q.ids.get(el); if (!r) { r = 'ref_' + Q.next++; Q.ids.set(el, r); Q.refs.set(r, new WeakRef(el)); } return r; };
const byRef = r => { const w = Q.refs.get(r); const el = w && w.deref(); return el && el.isConnected ? el : null; };
const TAGS = {A:'link',BUTTON:'button',SELECT:'combobox',TEXTAREA:'textbox',IMG:'image',NAV:'navigation',HEADER:'banner',FOOTER:'contentinfo',MAIN:'main',FORM:'form',UL:'list',OL:'list',LI:'listitem',TABLE:'table',TR:'row',TD:'cell',TH:'columnheader',DIALOG:'dialog',SECTION:'region',ARTICLE:'article',ASIDE:'complementary',SUMMARY:'button',DETAILS:'group',OPTION:'option',LABEL:'label',P:'paragraph',H1:'heading',H2:'heading',H3:'heading',H4:'heading',H5:'heading',H6:'heading'};
const INPUTS = {checkbox:'checkbox',radio:'radio',button:'button',submit:'button',reset:'button',image:'button',file:'button',range:'slider',search:'searchbox'};
const role = el => el.getAttribute('role') || (el.tagName === 'INPUT' ? (INPUTS[el.type] || 'textbox') : TAGS[el.tagName]) || (el.isContentEditable && !(el.parentElement && el.parentElement.isContentEditable) ? 'textbox' : null);
const interactive = el => /^(A|BUTTON|INPUT|SELECT|TEXTAREA|SUMMARY)$/.test(el.tagName) || (el.isContentEditable && !(el.parentElement && el.parentElement.isContentEditable)) || el.hasAttribute('onclick') || (el.hasAttribute('tabindex') && el.tabIndex >= 0) || /^(button|link|checkbox|radio|tab|menuitem|switch|option|combobox|textbox|slider|searchbox)$/.test(el.getAttribute('role') || '');
const secret = el => el.tagName === 'INPUT' && (el.type === 'password' || el.type === 'hidden' || /one-time-code|cc-/.test(el.autocomplete || ''));
const shown = el => { const s = getComputedStyle(el); if (s.display === 'none' || s.visibility === 'hidden') return false; const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0 || s.display === 'contents'; };
const inView = el => { const r = el.getBoundingClientRect(); return r.bottom > 0 && r.right > 0 && r.top < innerHeight && r.left < innerWidth; };
const clean = t => (t || '').replace(/\s+/g, ' ').trim();
const cap = (t, n) => t.length > n ? t.slice(0, n - 1) + '…' : t;
const own = el => clean([...el.childNodes].filter(n => n.nodeType === 3).map(n => n.textContent).join(' '));
const name = el => {
  let n = el.getAttribute('aria-label');
  const by = el.getAttribute('aria-labelledby');
  if (!n && by) n = by.split(/\s+/).map(id => { const e = document.getElementById(id); return e ? e.textContent : ''; }).join(' ');
  if (!n && el.labels && el.labels.length) n = [...el.labels].map(l => l.textContent).join(' ');
  if (!n) n = el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('alt');
  if (!n && el.tagName === 'INPUT' && /^(button|submit|reset)$/.test(el.type)) n = el.value;
  if (!n && (/^(A|BUTTON|SUMMARY|OPTION|LABEL|LI|TD|TH|P|H[1-6])$/.test(el.tagName) || el.getAttribute('role'))) n = el.innerText;
  if (!n && el.tagName === 'INPUT' && el.type === 'file') n = el.files && el.files.length ? [...el.files].map(f => f.name).join(', ') : 'Choose file';
  if (!n) n = own(el);
  return cap(clean(n), 100);
};
const line = (el, depth, r) => {
  let s = ' '.repeat(depth) + r;
  const n = name(el);
  if (n) s += ' "' + n.replace(/"/g, '\\"') + '"';
  s += ' [' + refOf(el) + ']';
  if (/^H[1-6]$/.test(el.tagName)) s += ' level=' + el.tagName[1];
  if (el.tagName === 'A' && el.getAttribute('href')) s += ' href="' + cap(el.getAttribute('href'), 200) + '"';
  if (el.tagName === 'INPUT' && el.type && el.type !== 'text') s += ' type="' + el.type + '"';
  if (el.tagName === 'INPUT' && el.placeholder && n !== el.placeholder) s += ' placeholder="' + cap(el.placeholder, 80) + '"';
  if (el.type === 'checkbox' || el.type === 'radio') { if (el.checked) s += ' (checked)'; }
  else if (el.tagName === 'SELECT') { const o = el.selectedOptions[0]; if (o) s += ' value="' + cap(clean(o.textContent), 80) + '"'; }
  else if (/^(INPUT|TEXTAREA)$/.test(el.tagName) && el.value) s += secret(el) ? ' value=[redacted]' : ' value="' + cap(el.value, 80).replace(/"/g, '\\"') + '"';
  if (el.disabled) s += ' (disabled)';
  return s;
};
const tree = o => {
  const out = []; let chars = 0, count = 0, cut = false;
  const root = o.ref ? byRef(o.ref) : document.body;
  if (!root) return { error: 'There is no ' + o.ref + ' on the page any more; read the page again.' };
  const walk = (el, depth, level) => {
    if (cut || level > o.depth || !(el instanceof Element)) return;
    if (/^(SCRIPT|STYLE|NOSCRIPT|TEMPLATE|HEAD|svg)$/.test(el.tagName)) return;
    if (!shown(el) && o.filter !== 'all') return;
    let next = depth;
    let r = role(el);
    const keep = o.filter === 'interactive' ? interactive(el) && inView(el) : (r && (r !== 'paragraph' || own(el))) || interactive(el) || (own(el) && (r = 'text'));
    if (keep) {
      const l = line(el, depth, r || 'generic');
      if (chars + l.length + 1 > o.max) { cut = true; return; }
      out.push(l); chars += l.length + 1;
      if (++count >= 10000) { cut = true; return; }
      next = depth + 1;
    }
    for (const c of el.children) walk(c, next, level + 1);
    if (el.shadowRoot) for (const c of el.shadowRoot.children) walk(c, next, level + 1);
  };
  walk(root, 0, 0);
  let text = out.join('\n') || '(empty page)';
  if (cut) text += '\n[cut at ' + chars + ' characters: read part of the page with ref_id, or pass filter "interactive" or a larger max_chars]';
  return { text, width: innerWidth, height: innerHeight };
};
const text = () => {
  let best = document.body, most = 0;
  for (const e of document.querySelectorAll('article, main, [role="main"], #content, [class*="articleBody"]')) {
    const n = (e.innerText || '').length; if (n > most) { best = e; most = n; }
  }
  if (most < 200) best = document.body;
  return { title: document.title, url: location.href, text: ((best && best.innerText) || '').replace(/\n{3,}/g, '\n\n') };
};
const center = r => {
  const el = byRef(r); if (!el) return null;
  el.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' });
  const b = el.getBoundingClientRect();
  return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
};
const fill = (r, v) => {
  const el = byRef(r); if (!el) return { error: 'There is no ' + r + ' on the page any more; read the page again.' };
  el.scrollIntoView({ block: 'center', behavior: 'instant' });
  if (el.focus) el.focus();
  if (el.tagName === 'SELECT') {
    const o = [...el.options].find(o => o.value === String(v) || clean(o.textContent) === String(v));
    if (!o) return { error: 'No option "' + v + '" in ' + r + '; its options: ' + [...el.options].map(o => clean(o.textContent)).join(', ') };
    el.value = o.value;
  } else if (el.type === 'checkbox' || el.type === 'radio') {
    const want = v === true || v === 'true' || v === 'on' || v === 1;
    if (el.checked !== want) el.click();
    return { done: (want ? 'Checked ' : 'Unchecked ') + r };
  } else if (el.isContentEditable) {
    el.textContent = String(v);
  } else if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') {
    const proto = el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, String(v));
  } else return { error: r + ' is a ' + el.tagName.toLowerCase() + ', not a field' };
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return { done: 'Set ' + r + ' to "' + (secret(el) ? '[redacted]' : String(v)) + '"' };
};
return { tree, text, center, fill };
})()"#;

/// The longest a screenshot is: Desktop's 800 px.
const SHOT_WIDTH: f64 = 800.0;
/// The most a page's text or a script's value says; the rest is cut.
const MOST_TEXT: usize = 100_000;

type Answer = Result<Vec<ToolContent>, String>;

impl Browser {
    /// Run the agent's tool `name` in `owner`'s browser.
    pub async fn tool(&self, owner: Owner, name: &str, input: Value) -> (Vec<ToolContent>, bool) {
        // The person's answer is not the tool's time.
        if let Err(error) = self.permitted(owner, name, &input).await {
            return (ToolContent::text(error), true);
        }
        let limit = if name == "javascript_tool" {
            45
        } else if name == "browser_batch" {
            180
        } else {
            30
        };
        match tokio::time::timeout(
            Duration::from_secs(limit),
            self.tool_inner(owner, name, &input),
        )
        .await
        {
            Ok(Ok(content)) => (content, false),
            Ok(Err(error)) => (ToolContent::text(error), true),
            Err(_) => (
                ToolContent::text(format!("{name} took longer than {limit}s.")),
                true,
            ),
        }
    }

    /// Whether the agent may do this where it would: open a site, or act in the page a tab
    /// is on (the person may have gone elsewhere in it, or a click taken it there).
    async fn permitted(&self, owner: Owner, name: &str, input: &Value) -> Result<(), String> {
        match name {
            "navigate" => match input["url"].as_str() {
                None | Some("back" | "forward") => Ok(()),
                Some(url) => self.may(owner, url, true).await,
            },
            "tabs_create" => match input["url"].as_str() {
                Some(url) => self.may(owner, url, true).await,
                None => Ok(()),
            },
            "tabs_context" | "tabs_select" | "tabs_close" | "browser_batch" => Ok(()),
            _ => {
                let url = self.running().and_then(|run| {
                    let tab = tab_id(input).or_else(|| active(&run, owner))?;
                    let state = run.state.lock().unwrap();
                    let owned = state.owners.get(&owner)?;
                    owned
                        .tabs
                        .iter()
                        .find(|t| t.id == tab)
                        .map(|t| t.url.clone())
                });
                match url {
                    Some(url) => self.may(owner, &url, false).await,
                    None => Ok(()),
                }
            }
        }
    }

    async fn tool_inner(&self, owner: Owner, name: &str, input: &Value) -> Answer {
        if name == "browser_batch" {
            return Box::pin(self.batch(owner, input)).await;
        }
        let run = self.run().await?;
        match name {
            "tabs_context" => Ok(ToolContent::text(context(&run, owner, None))),
            "tabs_create" => {
                let tab = self.new_tab(&run, owner, input["url"].as_str()).await?;
                if let Some(session) = run.session(owner, tab) {
                    settle(&run, &session, Duration::from_secs(10)).await;
                }
                Ok(ToolContent::text(format!(
                    "{}\nOpened tab {tab}.{}",
                    card(&run, owner, tab),
                    context(&run, owner, Some(tab))
                )))
            }
            "tabs_select" => {
                let tab = tab_id(input).ok_or("tabs_select needs a tabId.")?;
                self.select(&run, owner, tab)?;
                Ok(ToolContent::text(format!(
                    "Tab {tab} is on screen.{}",
                    context(&run, owner, Some(tab))
                )))
            }
            "tabs_close" => {
                let tab = tab_id(input).ok_or("tabs_close needs a tabId.")?;
                run.session(owner, tab)
                    .ok_or(format!("There is no tab {tab}."))?;
                self.close_tab(&run, owner, tab).await;
                Ok(ToolContent::text(format!(
                    "Closed tab {tab}.{}",
                    context(&run, owner, None)
                )))
            }
            "navigate" => self.navigate_tool(&run, owner, input).await,
            _ => {
                let (tab, session) = acting(&run, owner, input)?;
                let answer = match name {
                    "read_page" => read_page(&run, &session, input).await,
                    "find" => find(&run, &session, input).await,
                    "get_page_text" => page_text(&run, &session).await,
                    "form_input" => form_input(&run, &session, input).await,
                    "computer" => self.computer(&run, owner, tab, &session, input).await,
                    "javascript_tool" => javascript(&run, &session, input).await,
                    "read_console_messages" => console(&run, owner, tab, input),
                    "read_network_requests" => network(&run, owner, tab, input),
                    "resize_window" => self.resize(&run, owner, tab, input).await,
                    _ => Err(format!("There is no browser tool {name}.")),
                }?;
                Ok(with_context(answer, &run, owner, tab))
            }
        }
    }

    async fn navigate_tool(&self, run: &Arc<Run>, owner: Owner, input: &Value) -> Answer {
        let url = input["url"]
            .as_str()
            .ok_or("navigate needs a url.")?
            .trim()
            .to_owned();
        let (tab, new) = match tab_id(input).or_else(|| active(run, owner)) {
            Some(tab) => (tab, false),
            None => (self.new_tab(run, owner, None).await?, true),
        };
        let session = run
            .session(owner, tab)
            .ok_or(format!("There is no tab {tab}."))?;
        match url.as_str() {
            "back" => self.history(run, owner, tab, -1).await?,
            "forward" => self.history(run, owner, tab, 1).await?,
            _ => self.navigate(run, owner, tab, &url).await?,
        }
        self.select(run, owner, tab)?;
        settle(run, &session, Duration::from_secs(10)).await;
        let at = run
            .evaluate(&session, "location.href")
            .await
            .unwrap_or_default();
        let at = at.as_str().unwrap_or(&url);
        // A tab opened for it shows in the conversation as a card.
        let lead = if new {
            format!("{}\n", card(run, owner, tab))
        } else {
            String::new()
        };
        Ok(with_context(
            ToolContent::text(format!("{lead}Opened {at}.")),
            run,
            owner,
            tab,
        ))
    }

    fn select(&self, run: &Arc<Run>, owner: Owner, tab: u32) -> Result<(), String> {
        {
            let mut state = run.state.lock().unwrap();
            let owned = state.owners.get_mut(&owner).ok_or("No tab is open.")?;
            if !owned.tabs.iter().any(|t| t.id == tab) {
                return Err(format!("There is no tab {tab}."));
            }
            owned.active = Some(tab);
        }
        self.publish(run, owner);
        Ok(())
    }

    async fn computer(
        &self,
        run: &Arc<Run>,
        owner: Owner,
        tab: u32,
        session: &str,
        input: &Value,
    ) -> Answer {
        let action = input["action"]
            .as_str()
            .ok_or("computer needs an action.")?;
        // Coordinates are in the last screenshot's pixels.
        let scale = shot_scale(run, session).await;
        let point = |key: &str| -> Option<(f64, f64)> {
            let at = input[key].as_array()?;
            Some((at.first()?.as_f64()? / scale, at.get(1)?.as_f64()? / scale))
        };
        let target = || async {
            if let Some(reference) = input["ref"].as_str() {
                let at = helper(run, session, &format!("center({})", json!(reference))).await?;
                return match (at["x"].as_f64(), at["y"].as_f64()) {
                    (Some(x), Some(y)) => Ok((x, y)),
                    _ => Err(format!(
                        "There is no {reference} on the page any more; read the page again."
                    )),
                };
            }
            point("coordinate").ok_or_else(|| format!("{action} needs a coordinate or a ref."))
        };
        let done = |words: String| Ok(ToolContent::text(words));
        match action {
            "screenshot" => screenshot(run, session).await,
            "left_click" | "right_click" | "double_click" | "triple_click" => {
                let (x, y) = target().await?;
                let (button, clicks) = match action {
                    "right_click" => ("right", 1),
                    "double_click" => ("left", 2),
                    "triple_click" => ("left", 3),
                    _ => ("left", 1),
                };
                let modifiers = chord_modifiers(input["text"].as_str().unwrap_or_default());
                mouse(run, session, "mouseMoved", x, y, "none", 0, modifiers).await?;
                for count in 1..=clicks {
                    mouse(run, session, "mousePressed", x, y, button, count, modifiers).await?;
                    mouse(
                        run,
                        session,
                        "mouseReleased",
                        x,
                        y,
                        button,
                        count,
                        modifiers,
                    )
                    .await?;
                }
                settle(run, session, Duration::from_secs(3)).await;
                done(format!(
                    "Clicked at ({}, {}).",
                    (x * scale).round(),
                    (y * scale).round()
                ))
            }
            "hover" => {
                let (x, y) = target().await?;
                mouse(run, session, "mouseMoved", x, y, "none", 0, 0).await?;
                done("Moved the pointer there.".into())
            }
            "type" => {
                let text = input["text"].as_str().ok_or("type needs text.")?;
                run.cdp
                    .call("Input.insertText", json!({ "text": text }), Some(session))
                    .await
                    .map_err(|e| e.0)?;
                done(format!("Typed {} characters.", text.chars().count()))
            }
            "key" => {
                let keys = input["text"]
                    .as_str()
                    .ok_or("key needs text, such as \"Enter\" or \"ctrl+a\".")?;
                for chord in keys.split_whitespace() {
                    press(run, session, chord).await?;
                }
                settle(run, session, Duration::from_secs(3)).await;
                done(format!("Pressed {keys}."))
            }
            "scroll" => {
                let (x, y) = match point("coordinate") {
                    Some(at) => at,
                    None => {
                        let size = viewport(run, session).await;
                        (size.0 / 2.0, size.1 / 2.0)
                    }
                };
                let ticks = input["scroll_amount"]
                    .as_f64()
                    .unwrap_or(3.0)
                    .clamp(1.0, 30.0)
                    * 100.0;
                let (dx, dy) = match input["scroll_direction"].as_str().unwrap_or("down") {
                    "up" => (0.0, -ticks),
                    "left" => (-ticks, 0.0),
                    "right" => (ticks, 0.0),
                    _ => (0.0, ticks),
                };
                run.cdp
                    .call(
                        "Input.dispatchMouseEvent",
                        json!({ "type": "mouseWheel", "x": x, "y": y, "deltaX": dx, "deltaY": dy }),
                        Some(session),
                    )
                    .await
                    .map_err(|e| e.0)?;
                tokio::time::sleep(Duration::from_millis(300)).await;
                done(format!(
                    "Scrolled {}.",
                    input["scroll_direction"].as_str().unwrap_or("down")
                ))
            }
            "scroll_to" => {
                let reference = input["ref"].as_str().ok_or("scroll_to needs a ref.")?;
                helper(run, session, &format!("center({})", json!(reference))).await?;
                done(format!("Scrolled {reference} into view."))
            }
            "left_click_drag" => {
                let from =
                    point("start_coordinate").ok_or("left_click_drag needs a start_coordinate.")?;
                let to = point("coordinate").ok_or("left_click_drag needs a coordinate.")?;
                mouse(run, session, "mouseMoved", from.0, from.1, "none", 0, 0).await?;
                mouse(run, session, "mousePressed", from.0, from.1, "left", 1, 0).await?;
                for step in 1..=8 {
                    let t = f64::from(step) / 8.0;
                    let (x, y) = (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
                    mouse(run, session, "mouseMoved", x, y, "left", 0, 0).await?;
                }
                mouse(run, session, "mouseReleased", to.0, to.1, "left", 1, 0).await?;
                done("Dragged.".into())
            }
            "wait" => {
                let seconds = input["duration"].as_f64().unwrap_or(1.0).clamp(0.0, 10.0);
                tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
                done(format!("Waited {seconds} seconds."))
            }
            _ => {
                let _ = (owner, tab);
                Err(format!("computer has no action {action}."))
            }
        }
    }

    async fn resize(&self, run: &Arc<Run>, owner: Owner, tab: u32, input: &Value) -> Answer {
        let (width, height) = (
            input["width"].as_u64().unwrap_or(0) as u32,
            input["height"].as_u64().unwrap_or(0) as u32,
        );
        let size = (width > 0 && height > 0).then_some((width, height));
        let shown = self.emulate(run, owner, tab, size, true).await?;
        // The tab's state says it (`emulated_by_agent`): the pane's bar shows it, no notice.
        self.publish(run, owner);
        Ok(ToolContent::text(match size {
            Some((width, height)) => format!("The viewport is {width}x{height} now."),
            None => format!(
                "The viewport follows the pane again: {}x{}.",
                shown.0, shown.1
            ),
        }))
    }

    async fn batch(&self, owner: Owner, input: &Value) -> Answer {
        let actions = input["actions"]
            .as_array()
            .ok_or("browser_batch needs actions.")?;
        if actions.len() > 25 {
            return Err("browser_batch runs at most 25 actions.".into());
        }
        let mut answer = vec![];
        for (step, action) in actions.iter().enumerate() {
            let tool = action["tool"].as_str().unwrap_or_default();
            if tool == "browser_batch" {
                return Err("A batch cannot hold another.".into());
            }
            let input = action.get("input").cloned().unwrap_or(json!({}));
            let ran = match self.permitted(owner, tool, &input).await {
                Ok(()) => self.tool_inner(owner, tool, &input).await,
                Err(error) => Err(error),
            };
            match ran {
                Ok(content) => {
                    answer.push(ToolContent::Text(format!("{}. {tool}:", step + 1)));
                    answer.extend(content);
                }
                Err(error) => {
                    answer.push(ToolContent::Text(format!(
                        "{}. {tool} failed: {error}\nThe actions after it did not run.",
                        step + 1
                    )));
                    return Ok(answer);
                }
            }
        }
        Ok(answer)
    }
}

fn tab_id(input: &Value) -> Option<u32> {
    match &input["tabId"] {
        Value::Number(n) => n.as_u64().map(|n| n as u32),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn active(run: &Arc<Run>, owner: Owner) -> Option<u32> {
    run.state.lock().unwrap().owners.get(&owner)?.active
}

/// The tab a page tool acts in: the one named or the one on screen. A page opened by another
/// is the person's: the agent does not act in it, as in Desktop.
fn acting(run: &Arc<Run>, owner: Owner, input: &Value) -> Result<(u32, String), String> {
    let tab = tab_id(input)
        .or_else(|| active(run, owner))
        .ok_or("No tab is open: open one with navigate first.")?;
    let mut state = run.state.lock().unwrap();
    let page = state
        .tab_mut(owner, tab)
        .ok_or(format!("There is no tab {tab}."))?;
    if page.popup {
        return Err(format!(
            "Tab {tab} was opened by a page; only the user can act in it."
        ));
    }
    Ok((tab, page.session.clone()))
}

/// Desktop's footer of tab results: which tab ran it and the tabs there are.
fn context(run: &Arc<Run>, owner: Owner, ran: Option<u32>) -> String {
    let state = run.state.lock().unwrap();
    let Some(owned) = state.owners.get(&owner) else {
        return "\n\nNo tab is open.".into();
    };
    let mut text = String::from("\n\nTab Context:");
    if let Some(tab) = ran {
        text.push_str(&format!("\n- Executed on tabId: {tab}"));
    }
    text.push_str("\n- Available tabs:");
    for tab in &owned.tabs {
        let title: String = tab.title.chars().take(200).collect();
        let shown = if Some(tab.id) == owned.active {
            " (on screen)"
        } else {
            ""
        };
        text.push_str(&format!(
            "\n  • tabId {}: \"{}\" ({}){shown}",
            tab.id, title, tab.url
        ));
    }
    if let Some(tab) = owned.tabs.iter().find(|t| Some(t.id) == ran) {
        if let Some((w, h)) = tab.agent_view {
            text.push_str(&format!(
                "\n- Viewport: emulating {w}x{h}, set by resize_window"
            ));
        }
    }
    text
}

/// The line a tab the agent opened leads its answer with, for the conversation's card, as
/// Desktop's results lead with theirs.
fn card(run: &Arc<Run>, owner: Owner, tab: u32) -> String {
    let state = run.state.lock().unwrap();
    let url = state
        .owners
        .get(&owner)
        .and_then(|o| o.tabs.iter().find(|t| t.id == tab))
        .map(|t| t.url.clone())
        .unwrap_or_default();
    json!({ "type": "browser", "tabId": tab, "url": url }).to_string()
}

fn with_context(
    mut answer: Vec<ToolContent>,
    run: &Arc<Run>,
    owner: Owner,
    tab: u32,
) -> Vec<ToolContent> {
    answer.push(ToolContent::Text(
        context(run, owner, Some(tab)).trim_start().to_owned(),
    ));
    answer
}

/// What the helper's `call` gives back in the page.
async fn helper(run: &Arc<Run>, session: &str, call: &str) -> Result<Value, String> {
    let value = run
        .evaluate(session, &format!("{HELPER}.{call}"))
        .await
        .map_err(|e| e.0)?;
    match value["error"].as_str() {
        Some(error) => Err(error.to_owned()),
        None => Ok(value),
    }
}

/// Wait, at most `limit`, for the page to be loaded, and a little longer for what it does
/// right after.
async fn settle(run: &Arc<Run>, session: &str, limit: Duration) {
    let start = Instant::now();
    tokio::time::sleep(Duration::from_millis(100)).await;
    while start.elapsed() < limit {
        let state = run
            .cdp
            .call_within(
                "Runtime.evaluate",
                json!({ "expression": "document.readyState", "returnByValue": true }),
                Some(session),
                Duration::from_secs(2),
            )
            .await;
        if state.is_ok_and(|s| s["result"]["value"] == "complete") {
            tokio::time::sleep(Duration::from_millis(150)).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

async fn viewport(run: &Arc<Run>, session: &str) -> (f64, f64) {
    let size = run
        .evaluate(session, "[innerWidth, innerHeight]")
        .await
        .unwrap_or_default();
    (
        size[0].as_f64().unwrap_or(1280.0),
        size[1].as_f64().unwrap_or(800.0),
    )
}

/// Screenshot pixels per CSS pixel: the page at most 800 px wide.
async fn shot_scale(run: &Arc<Run>, session: &str) -> f64 {
    (SHOT_WIDTH / viewport(run, session).await.0).min(1.0)
}

async fn screenshot(run: &Arc<Run>, session: &str) -> Answer {
    let (width, height) = viewport(run, session).await;
    let scale = (SHOT_WIDTH / width).min(1.0);
    let ratio = run
        .evaluate(session, "devicePixelRatio")
        .await
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let scroll = run
        .evaluate(session, "[scrollX, scrollY]")
        .await
        .unwrap_or_default();
    let shot = run
        .cdp
        .call(
            "Page.captureScreenshot",
            json!({ "format": "jpeg", "quality": 75, "clip": {
                "x": scroll[0].as_f64().unwrap_or(0.0), "y": scroll[1].as_f64().unwrap_or(0.0),
                "width": width, "height": height, "scale": scale / ratio } }),
            Some(session),
        )
        .await
        .map_err(|e| e.0)?;
    let data = shot["data"].as_str().unwrap_or_default().to_owned();
    let (w, h) = ((width * scale).round(), (height * scale).round());
    Ok(vec![
        ToolContent::Image {
            mime: "image/jpeg".into(),
            data,
        },
        ToolContent::Text(format!(
            "Screenshot {w}x{h}; the page's viewport is {width}x{height}. Coordinates for clicks are in the screenshot's pixels."
        )),
    ])
}

#[allow(clippy::too_many_arguments)]
async fn mouse(
    run: &Arc<Run>,
    session: &str,
    kind: &str,
    x: f64,
    y: f64,
    button: &str,
    clicks: u8,
    modifiers: u8,
) -> Result<(), String> {
    let buttons = if kind == "mouseReleased" || button == "none" {
        0
    } else if button == "right" {
        2
    } else {
        1
    };
    run.cdp
        .call(
            "Input.dispatchMouseEvent",
            json!({ "type": kind, "x": x, "y": y, "button": button, "buttons": buttons,
                    "clickCount": clicks, "modifiers": modifiers }),
            Some(session),
        )
        .await
        .map(drop)
        .map_err(|e| e.0)
}

/// CDP's modifier bits of a chord such as "ctrl+shift": Alt 1, Ctrl 2, Meta 4, Shift 8.
fn chord_modifiers(chord: &str) -> u8 {
    chord.split('+').fold(0, |bits, part| {
        bits | match part.trim().to_lowercase().as_str() {
            "alt" | "option" => 1,
            "ctrl" | "control" => 2,
            "meta" | "cmd" | "command" | "super" => 4,
            "shift" => 8,
            _ => 0,
        }
    })
}

/// A key by the names xdotool and the DOM use: its `key`, `code` and Windows key code.
fn named_key(name: &str) -> Option<(String, String, u32)> {
    let named =
        |key: &str, code: &str, key_code: u32| Some((key.to_owned(), code.to_owned(), key_code));
    match name.to_lowercase().as_str() {
        "enter" | "return" => named("Enter", "Enter", 13),
        "tab" => named("Tab", "Tab", 9),
        "escape" | "esc" => named("Escape", "Escape", 27),
        "backspace" => named("Backspace", "Backspace", 8),
        "delete" | "del" => named("Delete", "Delete", 46),
        "space" => named(" ", "Space", 32),
        "up" | "arrowup" => named("ArrowUp", "ArrowUp", 38),
        "down" | "arrowdown" => named("ArrowDown", "ArrowDown", 40),
        "left" | "arrowleft" => named("ArrowLeft", "ArrowLeft", 37),
        "right" | "arrowright" => named("ArrowRight", "ArrowRight", 39),
        "home" => named("Home", "Home", 36),
        "end" => named("End", "End", 35),
        "pageup" | "page_up" => named("PageUp", "PageUp", 33),
        "pagedown" | "page_down" => named("PageDown", "PageDown", 34),
        lower => {
            if let Some(n) = lower
                .strip_prefix('f')
                .and_then(|n| n.parse::<u32>().ok())
                .filter(|n| (1..=12).contains(n))
            {
                return Some((format!("F{n}"), format!("F{n}"), 111 + n));
            }
            let mut chars = name.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            let upper = c.to_ascii_uppercase();
            let code = match c {
                'a'..='z' | 'A'..='Z' => format!("Key{upper}"),
                '0'..='9' => format!("Digit{c}"),
                _ => String::new(),
            };
            let key_code = if c.is_ascii_alphanumeric() {
                u32::from(upper)
            } else {
                0
            };
            Some((c.to_string(), code, key_code))
        }
    }
}

/// Press and release one chord: "Enter", "ctrl+a", "shift+Tab".
async fn press(run: &Arc<Run>, session: &str, chord: &str) -> Result<(), String> {
    let key = chord.rsplit('+').next().unwrap_or(chord);
    let modifiers = chord_modifiers(chord);
    let (key, code, key_code) = named_key(key).ok_or(format!("There is no key {key}."))?;
    // A character typed with no Ctrl or Alt types itself; Enter types a line break.
    let text = match key.as_str() {
        "Enter" => "\r".to_owned(),
        k if k.chars().count() == 1 && modifiers & 3 == 0 => {
            if modifiers & 8 != 0 {
                k.to_uppercase()
            } else {
                k.to_owned()
            }
        }
        _ => String::new(),
    };
    let mut down = json!({ "type": if text.is_empty() { "rawKeyDown" } else { "keyDown" },
        "key": key, "code": code, "windowsVirtualKeyCode": key_code,
        "nativeVirtualKeyCode": key_code, "modifiers": modifiers });
    if !text.is_empty() {
        down["text"] = json!(text);
        down["unmodifiedText"] = json!(text);
    }
    run.cdp
        .call("Input.dispatchKeyEvent", down, Some(session))
        .await
        .map_err(|e| e.0)?;
    run.cdp
        .call(
            "Input.dispatchKeyEvent",
            json!({ "type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": key_code,
                    "nativeVirtualKeyCode": key_code, "modifiers": modifiers }),
            Some(session),
        )
        .await
        .map(drop)
        .map_err(|e| e.0)
}

async fn read_page(run: &Arc<Run>, session: &str, input: &Value) -> Answer {
    let options = json!({
        "filter": if input["filter"] == "interactive" { "interactive" } else { "all" },
        "ref": input["ref_id"].as_str(),
        "depth": input["depth"].as_u64().unwrap_or(15).min(60),
        "max": input["max_chars"].as_u64().unwrap_or(50_000).min(500_000),
    });
    let page = helper(run, session, &format!("tree({options})")).await?;
    Ok(ToolContent::text(format!(
        "{}\n\nViewport: {}x{}",
        page["text"].as_str().unwrap_or_default(),
        page["width"],
        page["height"]
    )))
}

async fn find(run: &Arc<Run>, session: &str, input: &Value) -> Answer {
    let query = input["query"]
        .as_str()
        .ok_or("find needs a query.")?
        .to_lowercase();
    let options = json!({ "filter": "all", "depth": 60, "max": 5_000_000 });
    let page = helper(run, session, &format!("tree({options})")).await?;
    let found: Vec<&str> = page["text"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("[ref_") && line.to_lowercase().contains(&query))
        .take(20)
        .map(str::trim)
        .collect();
    Ok(ToolContent::text(if found.is_empty() {
        format!("Nothing on the page matches \"{query}\".")
    } else {
        format!(
            "Found {} for \"{query}\":\n- {}",
            found.len(),
            found.join("\n- ")
        )
    }))
}

async fn page_text(run: &Arc<Run>, session: &str) -> Answer {
    let page = helper(run, session, "text()").await?;
    let text: String = page["text"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(MOST_TEXT)
        .collect();
    Ok(ToolContent::text(format!(
        "Title: {}\nURL: {}\n\n{}",
        page["title"].as_str().unwrap_or_default(),
        page["url"].as_str().unwrap_or_default(),
        text
    )))
}

async fn form_input(run: &Arc<Run>, session: &str, input: &Value) -> Answer {
    let reference = input["ref"].as_str().ok_or("form_input needs a ref.")?;
    let value = input.get("value").cloned().unwrap_or(Value::Null);
    let done = helper(
        run,
        session,
        &format!("fill({}, {})", json!(reference), value),
    )
    .await?;
    Ok(ToolContent::text(done["done"].as_str().unwrap_or("Done.")))
}

async fn javascript(run: &Arc<Run>, session: &str, input: &Value) -> Answer {
    let script = input["text"]
        .as_str()
        .ok_or("javascript_tool needs the script in text.")?;
    let reply = run
        .cdp
        .call_within(
            "Runtime.evaluate",
            // A REPL's rules: top-level await, and the last expression's value.
            json!({ "expression": script, "returnByValue": true, "awaitPromise": true, "replMode": true }),
            Some(session),
            Duration::from_secs(45),
        )
        .await
        .map_err(|e| e.0)?;
    if let Some(exception) = reply.get("exceptionDetails") {
        return Err(exception["exception"]["description"]
            .as_str()
            .or(exception["text"].as_str())
            .unwrap_or("The script failed.")
            .to_owned());
    }
    let result = &reply["result"];
    let text = match (&result["value"], result["type"].as_str()) {
        (Value::Null, Some("undefined")) => "undefined".to_owned(),
        (Value::String(text), _) => text.clone(),
        (Value::Null, _) => result["description"].as_str().unwrap_or("null").to_owned(),
        (value, _) => serde_json::to_string_pretty(value).unwrap_or_default(),
    };
    Ok(ToolContent::text(
        text.chars().take(MOST_TEXT).collect::<String>(),
    ))
}

fn console(run: &Arc<Run>, owner: Owner, tab: u32, input: &Value) -> Answer {
    let mut state = run.state.lock().unwrap();
    let page = state.tab_mut(owner, tab).ok_or("That tab is closed.")?;
    let pattern = input["pattern"].as_str().unwrap_or_default().to_lowercase();
    let errors = input["only_errors"] == true;
    let lines: Vec<&String> = page
        .console
        .iter()
        .filter(|line| !errors || line.starts_with("[error]") || line.starts_with("[exception]"))
        .filter(|line| line.to_lowercase().contains(&pattern))
        .collect();
    let text = if lines.is_empty() {
        "No console messages.".to_owned()
    } else {
        lines
            .iter()
            .map(|l| l.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    if input["clear"] == true {
        page.console.clear();
    }
    Ok(ToolContent::text(text))
}

fn network(run: &Arc<Run>, owner: Owner, tab: u32, input: &Value) -> Answer {
    let mut state = run.state.lock().unwrap();
    let page = state.tab_mut(owner, tab).ok_or("That tab is closed.")?;
    let pattern = input["url_pattern"].as_str().unwrap_or_default();
    let lines: Vec<String> = page
        .network
        .iter()
        .filter(|r| r.url.contains(pattern))
        .map(|r| {
            format!(
                "{} {} {}",
                r.method,
                r.status.as_deref().unwrap_or("pending"),
                r.url
            )
        })
        .collect();
    if input["clear"] == true {
        page.network.clear();
    }
    Ok(ToolContent::text(if lines.is_empty() {
        "No requests.".to_owned()
    } else {
        lines.join("\n")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_name_their_keys_and_modifiers() {
        assert_eq!(chord_modifiers("ctrl+shift+a"), 2 | 8);
        assert_eq!(
            named_key("Enter"),
            Some(("Enter".into(), "Enter".into(), 13))
        );
        assert_eq!(named_key("a"), Some(("a".into(), "KeyA".into(), 65)));
        assert_eq!(named_key("F5"), Some(("F5".into(), "F5".into(), 116)));
        assert_eq!(named_key("7"), Some(("7".into(), "Digit7".into(), 55)));
        assert_eq!(named_key("nonsense"), None);
    }
}
