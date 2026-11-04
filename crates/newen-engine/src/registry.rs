//! The public MCP registry (registry.modelcontextprotocol.io), where Newen's Customize ›
//! Connectors finds servers to add for any agent that speaks MCP: what it lists for some words,
//! each turned into how to run it.

use newen_model::ServerOffer;
use serde_json::Value;

/// The latest version of each server whose name or words hold `query`, at most 50.
pub fn url(query: &str) -> String {
    let query: String = query
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            b' ' => "+".into(),
            other => format!("%{other:02X}"),
        })
        .collect();
    format!("https://registry.modelcontextprotocol.io/v0/servers?version=latest&limit=50&search={query}")
}

/// What the registry lists for `query`, asked with `curl`.
pub async fn find(query: &str) -> Result<Vec<ServerOffer>, String> {
    let run = newen_agent::ProgramRun {
        args: vec!["-sSfL".into(), "--max-time".into(), "20".into(), url(query)],
        stdin: None,
        timeout: 25,
        until: None,
    };
    let body = crate::run_program("curl", &run, &std::env::temp_dir()).await?;
    offers(&body)
}

/// The offers in what the registry answered: each server reached at its first address, or
/// run from its first package, its required arguments as `<hint>` for the person to fill.
pub fn offers(body: &str) -> Result<Vec<ServerOffer>, String> {
    let body: Value =
        serde_json::from_str(body).map_err(|e| format!("the registry's answer: {e}"))?;
    let servers = body["servers"]
        .as_array()
        .ok_or("the registry listed nothing")?;
    Ok(servers
        .iter()
        .filter_map(|entry| offer(&entry["server"]))
        .collect())
}

fn offer(server: &Value) -> Option<ServerOffer> {
    let text = |v: &Value| v.as_str().map(str::to_owned);
    let required = |list: &Value, with_value: bool| -> Vec<String> {
        (list.as_array().into_iter().flatten())
            .filter(|v| v["isRequired"] == true)
            .filter_map(|v| {
                let name = v["name"].as_str()?;
                Some(if with_value {
                    format!("{name}=")
                } else {
                    format!("{name}: ")
                })
            })
            .collect()
    };
    let mut offer = ServerOffer {
        name: text(&server["name"])?,
        title: text(&server["title"]),
        description: text(&server["description"]).unwrap_or_default(),
        version: text(&server["version"]).unwrap_or_default(),
        repository: text(&server["repository"]["url"]),
        ..ServerOffer::default()
    };
    if let Some(remote) = server["remotes"].as_array().and_then(|r| r.first()) {
        offer.remote = true;
        offer.target = text(&remote["url"])?;
        offer.headers = required(&remote["headers"], false);
        return Some(offer);
    }
    let package = server["packages"].as_array()?.first()?;
    let id = text(&package["identifier"])?;
    let (target, mut args) = match package["registryType"].as_str()? {
        "npm" => ("npx", vec!["-y".to_owned(), id]),
        "pypi" => ("uvx", vec![id]),
        "oci" => ("docker", vec!["run".into(), "-i".into(), "--rm".into(), id]),
        _ => return None,
    };
    args.extend(
        (package["packageArguments"].as_array().into_iter().flatten())
            .filter(|a| a["isRequired"] == true && a["type"] == "positional")
            .map(|a| format!("<{}>", a["valueHint"].as_str().unwrap_or("value"))),
    );
    offer.target = target.into();
    offer.args = args;
    offer.env = required(&package["environmentVariables"], true);
    Some(offer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_runs_from_its_address_or_its_package_with_what_it_needs() {
        let body = serde_json::json!({ "servers": [
            { "server": { "name": "io.x/remote", "description": "R", "version": "1",
                "remotes": [{ "type": "streamable-http", "url": "https://x.io/mcp",
                    "headers": [{ "name": "Authorization", "isRequired": true }] }] } },
            { "server": { "name": "io.x/pg", "description": "P", "version": "2",
                "packages": [{ "registryType": "npm", "identifier": "@x/pg",
                    "packageArguments": [{ "type": "positional", "isRequired": true, "valueHint": "database_url" }],
                    "environmentVariables": [{ "name": "PG_TOKEN", "isRequired": true }, { "name": "OPT" }] }] } },
            { "server": { "name": "io.x/none" } }
        ]});
        let offers = offers(&body.to_string()).unwrap();
        assert_eq!(offers.len(), 2);
        assert!(offers[0].remote && offers[0].target == "https://x.io/mcp");
        assert_eq!(offers[0].headers, ["Authorization: "]);
        assert_eq!(
            (offers[1].target.as_str(), offers[1].args.clone()),
            (
                "npx",
                vec!["-y".to_owned(), "@x/pg".into(), "<database_url>".into()]
            )
        );
        assert_eq!(offers[1].env, ["PG_TOKEN="]);
        assert!(url("postgres db").ends_with("search=postgres+db"));
    }
}
