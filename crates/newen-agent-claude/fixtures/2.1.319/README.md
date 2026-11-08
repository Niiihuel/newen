# Claude Code 2.1.319: a resumed session that lost a background command

Captured 2026-11-08 from `claude` 2.1.319 with Newen's stream-json switches,
`--replay-user-messages`, `--model haiku` and bypass. A first process ran `sleep 900` with the
Bash tool's `run_in_background` and was killed after its turn; a second one resumed it with
`--resume=<id> --fork-session`, Newen's prompt written right after `initialize`.

- `orphaned.jsonl`: the second process. The CLI tells the background command gone
  (`task_notification`, `stopped`), queues the prompt (`command_lifecycle` `queued`), ends a
  turn of its own with no step (`result` with `num_turns: 0` and
  `origin: {kind: "task-notification"}`), and only then starts the prompt (`started`), echoes
  it and answers. One `{t_ms, dir, msg}` per line. Trimmed: `system/init` keeps its ids, mode
  and version (the folder reads `/home/user/work`), `result` its counts, usage, origin and
  ids, `assistant` its content and usage; hook, `commands_changed`, `control_response`,
  `stream_event` and `rate_limit_event` lines are left out.
