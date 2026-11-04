//! `suggest_connectors` (docs/agents.md): the session's connectors its agent names, offered on a
//! card the turn waits on, then how each stands once the person is done with the card.

use crate::{Ask, Input};
use newen_agent::{McpServer, McpSource, McpStatus};
use newen_model::{ServerOffer, SessionId};
use tokio::sync::mpsc::UnboundedSender;

/// How long the agent has to list its servers: one that cannot list them never answers.
const LISTED_WITHIN: std::time::Duration = std::time::Duration::from_secs(10);

/// What to answer the agent that asked for `input["names"]`, and whether that is an error. The
/// ones not connected show on a card ([`newen_model::SITE_PERMISSION`], kind `connectors`) and
/// the call waits for it: the person connects them there, or closes it. A name the session has
/// no server for is looked up in the MCP registry when its agent can add one while it runs: the
/// card offers it (`offers`), to be added to the session and to the agent's config.
pub(crate) async fn connectors(
    actor: &UnboundedSender<Input>,
    ask: &Ask,
    session: SessionId,
    input: &serde_json::Value,
) -> (String, bool) {
    let names: Vec<&str> = (input["names"].as_array().into_iter().flatten())
        .filter_map(|name| name.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty() {
        return (
            "suggest_connectors needs the names of the connectors.".into(),
            true,
        );
    }
    let Some((mut servers, can_add)) = listed(actor).await else {
        return (
            "This session's agent did not list its connectors, so none can be offered.".into(),
            true,
        );
    };
    let offered: Vec<String> = (servers.iter())
        .filter(|s| s.status != McpStatus::Connected && names.iter().any(|n| named(s, n)))
        .map(|s| s.name.clone())
        .collect();
    let (mut offers, mut from_registry) = (vec![], vec![]);
    let missing = (names.iter()).filter(|n| !servers.iter().any(|s| named(s, n)));
    for name in missing.filter(|_| can_add) {
        let found = crate::registry::find(name).await.unwrap_or_default();
        if let Some(offer) = pick(found, name) {
            offers.push(offer);
            from_registry.push(*name);
        }
    }
    let carded = !offered.is_empty() || !offers.is_empty();
    if carded {
        let card =
            serde_json::json!({ "kind": "connectors", "servers": offered, "offers": offers });
        // Its answer only closes the card: how the servers stand is asked again. A card the
        // turn left open (interrupted) answers no one.
        if ask(session, card, None).await.is_err() {
            return (
                "The turn ended before the user answered the card.".into(),
                false,
            );
        }
        if let Some((now, _)) = listed(actor).await {
            servers = now;
        }
    }
    (said(&names, &servers, carded, &from_registry), false)
}

/// The agent's servers, asked of its actor, and whether it adds one while it runs; `None` when
/// it does not say in time.
async fn listed(actor: &UnboundedSender<Input>) -> Option<(Vec<McpServer>, bool)> {
    let (answer, heard) = tokio::sync::oneshot::channel();
    actor.send(Input::Servers(answer)).ok()?;
    let (servers, can_add) = tokio::time::timeout(LISTED_WITHIN, heard)
        .await
        .ok()?
        .ok()?;
    // Newen's own servers are not connectors to offer.
    let servers = servers.into_iter().filter(|s| s.source != McpSource::App);
    Some((servers.collect(), can_add))
}

/// The registry's server called `name` that runs with nothing to fill in: its name's last part
/// or its title is `name` (not a fork that only holds it, as "context7fork"), and it needs no
/// header, variable or argument of the person's.
fn pick(offers: Vec<ServerOffer>, name: &str) -> Option<ServerOffer> {
    let name = key(name);
    offers.into_iter().find(|offer| {
        let last = offer.name.rsplit(['/', '.']).next().unwrap_or(&offer.name);
        (key(last) == name || offer.title.as_deref().map(key).as_ref() == Some(&name))
            && offer.env.is_empty()
            && offer.headers.is_empty()
            && !offer.args.iter().any(|a| a.starts_with('<'))
    })
}

/// A name by its letters and digits only, in lower case.
fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether `server` is the one called `name`: its name, label or plugin holds it, letters and
/// digits only, any case ("Context7" is `plugin:context7:context7`, "gmail" is "claude.ai Gmail").
fn named(server: &McpServer, name: &str) -> bool {
    let name = key(name);
    !name.is_empty()
        && [
            Some(&server.name),
            Some(&server.label),
            server.plugin.as_ref(),
        ]
        .into_iter()
        .flatten()
        .any(|n| key(n).contains(&name))
}

/// The answer: which of `names` are connected, which the person left as they were (`carded`:
/// they saw a card), which the card offered from the registry (`from_registry`) and they did not
/// add, and which this session does not have, with the ones it has.
fn said(names: &[&str], servers: &[McpServer], carded: bool, from_registry: &[&str]) -> String {
    let state = |s: &McpServer| match s.status {
        McpStatus::Connected => "connected",
        McpStatus::Pending => "connecting",
        McpStatus::NeedsAuth => "needs sign-in",
        McpStatus::Failed => "failed",
        McpStatus::Disabled => "off",
    };
    let (on, off): (Vec<&McpServer>, Vec<&McpServer>) = (servers.iter())
        .filter(|s| names.iter().any(|n| named(s, n)))
        .partition(|s| s.status == McpStatus::Connected);
    let list = |servers: &[&McpServer]| {
        (servers.iter())
            .map(|s| format!("{} ({})", s.label, state(s)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut lines = vec![];
    if !on.is_empty() {
        lines.push(format!(
            "Connected: {}. Use their tools for what the user asked.",
            list(&on)
        ));
    }
    if !off.is_empty() {
        lines.push(format!(
            "Not connected: {}. {}",
            list(&off),
            if carded {
                "The user closed the card without connecting them: go on without them, and do not offer them again unless the user asks."
            } else {
                "Go on without them."
            }
        ));
    }
    let (left, missing): (Vec<&str>, Vec<&str>) = (names.iter().copied())
        .filter(|n| !servers.iter().any(|s| named(s, n)))
        .partition(|n| from_registry.contains(n));
    if !left.is_empty() {
        lines.push(format!(
            "Not added: {}. The user closed the card without adding it: go on without it, and do not offer it again unless the user asks.",
            left.join(", ")
        ));
    }
    if !missing.is_empty() {
        let all: Vec<&McpServer> = servers.iter().collect();
        lines.push(format!(
            "Not in this session: {}. {} The user can add one in Customize \u{203a} Connectors; go on without it.",
            missing.join(", "),
            if all.is_empty() {
                "This session has no connectors.".to_owned()
            } else {
                format!("Its connectors: {}.", list(&all))
            }
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(name: &str, label: &str, status: McpStatus) -> McpServer {
        McpServer {
            name: name.into(),
            label: label.into(),
            status,
            source: McpSource::User,
            plugin: None,
            error: None,
            tools: vec![],
            sign_in: false,
        }
    }

    /// An actor that lists `servers` (the first time; `then` after), adding none.
    fn actor(first: Vec<McpServer>, then: Vec<McpServer>) -> UnboundedSender<Input> {
        let (actor, mut inbox) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut lists = [first, then].into_iter();
            let mut last = vec![];
            while let Some(Input::Servers(answer)) = inbox.recv().await {
                last = lists.next().unwrap_or(last);
                let _ = answer.send((last.clone(), false));
            }
        });
        actor
    }

    #[test]
    fn a_name_is_found_in_the_name_label_or_plugin_in_any_case() {
        let mut docs = server("plugin:context7:context7", "context7", McpStatus::Failed);
        assert!(named(&docs, "Context7"));
        assert!(named(
            &server("claude.ai Gmail", "Gmail", McpStatus::NeedsAuth),
            "gmail"
        ));
        assert!(!named(&docs, "linear") && !named(&docs, " - "));
        docs.plugin = Some("upstash-docs".into());
        assert!(named(&docs, "Upstash"));
    }

    /// What the registry listed for "context7" (2026-11-03): two forks that only hold the name,
    /// then the server itself, reached at its address with no header it requires.
    #[test]
    fn the_registry_offer_is_the_server_itself_with_nothing_to_fill_in() {
        let offer = |name: &str, title: Option<&str>, headers: &[&str]| ServerOffer {
            name: name.into(),
            title: title.map(str::to_owned),
            remote: true,
            target: format!("https://{name}/mcp"),
            headers: headers.iter().map(|h| h.to_string()).collect(),
            ..ServerOffer::default()
        };
        let found = vec![
            offer("ai.smithery/renCosta2025-context7fork", None, &[]),
            offer("com.clauxel.context7docs/context7docs-mcp", None, &[]),
            offer("io.github.upstash/context7", Some("Context7"), &[]),
        ];
        let picked = pick(found.clone(), "Context7").map(|o| o.name);
        assert_eq!(picked.as_deref(), Some("io.github.upstash/context7"));
        // One that needs the person's key is for Customize's form, not a click.
        let keyed = vec![offer("io.x/linear", Some("Linear"), &["Authorization: "])];
        assert_eq!(pick(keyed, "linear"), None);
        assert_eq!(pick(found, "jira"), None);
    }

    #[test]
    fn the_answer_tells_connected_left_and_missing_apart() {
        let servers = [
            server("linear", "Linear", McpStatus::Connected),
            server("claude.ai Gmail", "Gmail", McpStatus::NeedsAuth),
        ];
        let words = said(
            &["linear", "gmail", "docs", "jira"],
            &servers,
            true,
            &["docs"],
        );
        let lines: Vec<&str> = words.lines().collect();
        assert_eq!(
            lines[0],
            "Connected: Linear (connected). Use their tools for what the user asked."
        );
        assert!(lines[1].starts_with("Not connected: Gmail (needs sign-in). The user closed"));
        assert!(lines[2].starts_with("Not added: docs. The user closed the card"));
        assert!(lines[3].starts_with("Not in this session: jira. Its connectors: Linear"));
        assert_eq!(
            said(&["jira"], &[], false, &[]),
            "Not in this session: jira. This session has no connectors. The user can add one in Customize \u{203a} Connectors; go on without it."
        );
    }

    #[tokio::test]
    async fn connected_ones_are_answered_without_a_card() {
        let linear = vec![server("linear", "Linear", McpStatus::Connected)];
        let actor = actor(linear, vec![]);
        let ask: Ask = std::sync::Arc::new(|_, _, _| panic!("no card for a connected one"));
        let (words, error) =
            connectors(&actor, &ask, 1, &serde_json::json!({ "names": ["Linear"] })).await;
        assert!(!error && words.starts_with("Connected: Linear"));
    }

    #[tokio::test]
    async fn a_card_the_turn_left_open_answers_at_once() {
        let actor = actor(vec![server("linear", "Linear", McpStatus::Failed)], vec![]);
        // The engine dropped where its answer goes.
        let ask: Ask = std::sync::Arc::new(|_, _, _| tokio::sync::oneshot::channel().1);
        let (words, error) =
            connectors(&actor, &ask, 1, &serde_json::json!({ "names": ["linear"] })).await;
        assert!(!error && words.starts_with("The turn ended"));
    }

    #[tokio::test]
    async fn one_not_connected_waits_on_its_card_then_says_how_it_stands() {
        let gmail = |status| vec![server("claude.ai Gmail", "Gmail", status)];
        // Signed in on the card, by the next time it is asked.
        let actor = actor(gmail(McpStatus::NeedsAuth), gmail(McpStatus::Connected));
        let carded = std::sync::Arc::new(std::sync::Mutex::new(None));
        let ask: Ask = {
            let carded = carded.clone();
            std::sync::Arc::new(move |_, card, _| {
                *carded.lock().unwrap() = Some(card);
                let (answer, heard) = tokio::sync::oneshot::channel();
                let _ = answer.send(newen_browser::Grant::Once);
                heard
            })
        };
        let (words, _) =
            connectors(&actor, &ask, 1, &serde_json::json!({ "names": ["gmail"] })).await;
        assert_eq!(
            carded.lock().unwrap().take(),
            Some(serde_json::json!({
                "kind": "connectors", "servers": ["claude.ai Gmail"], "offers": []
            }))
        );
        assert!(words.starts_with("Connected: Gmail (connected)"));
    }
}
