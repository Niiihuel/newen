# Claude Code 2.1.311: MCP, plugins and skills

Captured 2026-10-26 from `claude` 2.1.311 in an empty git repository, with the person's own
`~/.claude` (user, project and local setting sources), and no prompt: these control requests
answer before the first turn.

- `extensions.jsonl`: `initialize`, then `mcp_status` twice (at once, servers `pending`; 15 s
  later, `connected` with their tools), `mcp_reconnect`, `get_skills_dialog`,
  `reload_plugins` and `reload_skills`. One `{t_ms, dir, msg}` per line, as `quill-capture`
  writes. Trimmed: two tools per server, two skills per source, a few commands; the home
  folder reads `/home/user`, the account `user@example.com`, connector ids are placeholders.
- `websearch.jsonl`: `claude -p` stream-json with a WebSearch turn (haiku, 2026-10-27), not the
  `{t_ms, dir, msg}` of `quill-capture`: each line is wrapped as `{"dir":"in","msg":<line>,"t_ms":0}`
  for the replay. Two searches, their results and the final answer. Trimmed: `system/init`
  empties `mcp_servers` and `plugins`, keeps two `skills` and `slash_commands` and three
  `agents`; `commands_changed` keeps two commands; the home folder reads `/home/user`, the
  socket path is a placeholder.
- `plugin-list.json`: `claude plugin list --json`, the installed plugins.

Request shapes, from the CLI's own schemas (`strings` of the 2.1.311 binary):

| Request | Fields | Reply |
|---|---|---|
| `mcp_status` | — | `{mcpServers: [{name, status: connected\|failed\|needs-auth\|pending\|disabled, serverInfo?, error?, error_code?, config?, transport?, sessionProxied?, urlHost?, scope?, source?, tools?: [{name, description?, annotations?}]}]}` |
| `mcp_reconnect` | `serverName` | `null` |
| `mcp_toggle` | `serverName`, `enabled` | (not captured: it writes the person's settings) |
| `get_skills_dialog` | — | `{skills: [{name, display_name, description, source, tokens, state: on\|name-only\|user-invocable-only\|off, locked_by?: author\|plugin\|policy\|flag\|reserved-name, advertised, handles}]}`. Read-only: a state is written with `claude edit-skill-overrides --json` (one `{name, state, handles?}` on stdin, into the project's local settings) |
| `reload_plugins` | — | `{commands, agents, plugins: [{name, path, source, version}], mcpServers, error_count}` |
| `reload_skills` | — | `{skills: [{name, description, argumentHint}]}` (the slash list) |

Plugins are installed, enabled and disabled with `claude plugin install|enable|disable
<id> --scope user|project|local --json`.
- `plugin-list-available.json`: `claude plugin list --json --available`, trimmed to two
  installed plugins and the two most installed offers of each marketplace:
  `{installed: [...], available: [{pluginId, name, description, marketplaceName, source,
  version?, installCount?}]}`.
- `marketplace-list.json`: `claude plugin marketplace list --json`:
  `[{name, source: github|git|…, repo|url|path, installLocation}]`.

Changes go through the CLI too, each printing one `--json` result line last:
`plugin install <id> [--scope project|local]`, `plugin uninstall <id> --scope <s>`,
`plugin update <id>`, `plugin marketplace add <source>`, `plugin marketplace update|remove
<name>`, and `edit-skill-overrides --json` with `{name, state, handles?}` on stdin, which
writes `skillOverrides` into the folder's `.claude/settings.local.json`. A plugin's result is
`{command, outcome: ok|failed|…, pluginId, scope, message, …}`.

`think.jsonl`: `quill-capture think --model sonnet --settings project,local` with
`--thinking-display summarized` (2.1.311, 2026-10-27). One question with a thinking block: the
`thinking_delta`s stream its text, and the final `assistant` message carries it whole, trimmed
of the trailing blank lines by quill. Anonymised: the home folder reads `/home/user` and the
account `user@example.com`.
