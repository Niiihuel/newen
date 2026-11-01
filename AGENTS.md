# Rules for agents working on quill

- The UI is limn only. Anything generic that limn lacks (a component, a layout, a fix) is a pull request to limn from its own worktree, `git -C ~/projects/personal/lienzo worktree add ../limn-<topic> -b feat/<topic> main`. quill never carries a copy or a patch of a limn component.
- Agents are adapters: generic code (`quill-model`, `quill-agent`, `quill-engine`, `quill-store`, `quill-git`, UI) never names a vendor. Vendor protocols live only in `quill-agent-<id>` (`quill-agent-claude` first). The UI builds model, effort and mode pickers from `AgentCapabilities`.
- Every adapter offers all of quill's own tools (`quill_agent::HOST_TOOLS`: sessions that start, read, message, list and wait for each other) through its CLI's own mechanism (Claude: an in-process MCP server; Codex: `dynamicTools`), answers them with the engine's results, and has a test that every one is offered. Sessions of different CLIs work together only because each one has them.
- Layers point down only: `quill-ui` never depends on `quill-engine`; they meet in `quill-model` (`Event` up, `Intent` down).
- Nothing blocking on the UI thread: processes, git, disk and PTYs live in `quill-engine`.
- Persist before notifying: an important state change is written to `quill-store` before the UI sees it.
- Unknown CLI messages are kept as `Unknown`, never a crash. A CLI version outside the tested range is reported in the UI.
- quill does not identify itself as another client (e.g. Claude Desktop) and uses no vendor code, fonts, icons or branding. One exception: an adapter (`quill-agent-<id>`) may ship its agent's logo, only to name that agent where quill lists agents; generic code never holds one.
- Logos of connectors and plugins are never shipped: the engine looks them up at run time (svgl.app by name, else the server's site), keeps them in the person's data dir, and a Settings switch turns that off.
- Every new dependency gets a one-line comment in `Cargo.toml` saying why.
- Behavior modeled on Claude Desktop is described by what the user sees; its reverse-engineering notes stay private and are never cited or copied into this repo. What is inferred is said to be inferred.
- Before a commit: `scripts/check.sh` (fmt, clippy `-D warnings`, tests).
