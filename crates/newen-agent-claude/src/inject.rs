//! What quill's "All agents" layer adds to a Claude Code session without writing the CLI's
//! files: `--mcp-config` with the servers (V1, docs/agents.md: the CLI expands `${VAR}` in the
//! JSON, so a secret is only the name of a variable of the process) and `--plugin-dir` with a
//! plugin of Newen's whose skills are the global ones, shown as `newen:<name>` (V4).

use newen_agent::{InjectedServer, SessionSpec};
use serde_json::{json, Map, Value};
use std::path::Path;

/// `{"mcpServers": {name: {...}}}`; a secret is written as `${VARIABLE}`.
pub(crate) fn mcp_config(servers: &[InjectedServer]) -> Value {
    let mut all = Map::new();
    for s in servers {
        let pairs = |plain: &[(String, String)], vars: &[(String, String)]| -> Value {
            let mut map = Map::new();
            for (k, v) in plain {
                map.insert(k.clone(), json!(v));
            }
            for (k, var) in vars {
                map.insert(k.clone(), json!(format!("${{{var}}}")));
            }
            Value::Object(map)
        };
        let mut entry = if s.remote {
            json!({ "type": if s.sse { "sse" } else { "http" }, "url": s.target })
        } else {
            json!({ "type": "stdio", "command": s.target, "args": s.args })
        };
        if s.remote && !(s.headers.is_empty() && s.header_vars.is_empty()) {
            entry["headers"] = pairs(&s.headers, &s.header_vars);
        }
        if !(s.remote || s.env.is_empty() && s.env_vars.is_empty()) {
            entry["env"] = pairs(&s.env, &s.env_vars);
        }
        all.insert(s.name.clone(), entry);
    }
    json!({ "mcpServers": all })
}

/// The arguments for the session's extras. The plugin's `plugin.json` is written beside the
/// skills folder (`<plugin>/skills`), and again whenever it says something else (quill's,
/// named `quill`).
pub(crate) fn args(spec: &SessionSpec) -> Vec<String> {
    let mut args = vec![];
    if !spec.servers.is_empty() {
        args.extend(["--mcp-config".into(), mcp_config(&spec.servers).to_string()]);
    }
    if let Some(plugin) = spec.skill_roots.first().and_then(|root| root.parent()) {
        if write_plugin(plugin) {
            args.extend(["--plugin-dir".into(), plugin.to_string_lossy().into_owned()]);
        }
    }
    args
}

fn write_plugin(plugin: &Path) -> bool {
    let manifest = plugin.join(".claude-plugin/plugin.json");
    let body = json!({ "name": "newen", "description": "Skills for all agents, kept by Newen" })
        .to_string();
    if std::fs::read_to_string(&manifest).is_ok_and(|kept| kept == body) {
        return true;
    }
    std::fs::create_dir_all(plugin.join(".claude-plugin")).is_ok()
        && std::fs::write(manifest, body).is_ok()
}
