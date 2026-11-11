# Rules for agents working on Newen

- The UI is limn only. Anything generic that limn lacks (a component, a layout, a fix) is a pull request to limn from its own worktree, `git -C ~/projects/personal/lienzo worktree add ../limn-<topic> -b feat/<topic> main`. Newen never carries a copy or a patch of a limn component.
- Agents are adapters: generic code (`newen-model`, `newen-agent`, `newen-engine`, `newen-store`, `newen-git`, UI) never names a vendor. Vendor protocols live only in `newen-agent-<id>` (`newen-agent-claude` first). The UI builds model, effort and mode pickers from `AgentCapabilities`.
- Every adapter offers all of Newen's own tools (`newen_agent::HOST_TOOLS`: sessions that start, read, message, list and wait for each other) through its CLI's own mechanism (Claude: an in-process MCP server; Codex: `dynamicTools`; ACP agents: Newen's own MCP servers in `session/new`, `SessionExtras::host`), answers them with the engine's results, and has a test that every one is offered. Sessions of different CLIs work together only because each one has them.
- Every adapter says how its agent's program updates itself (`AgentAdapter::update_args`: its CLI's own updater; Newen downloads no agent itself) and, where the program lists its models apart from a session, how (`models_query` or `models_runs`); the engine has a test that every one says the first (docs/agents.md, «El programa al día»).
- Layers point down only: `newen-ui` never depends on `newen-engine`; they meet in `newen-model` (`Event` up, `Intent` down).
- Nothing blocking on the UI thread: processes, git, disk and PTYs live in `newen-engine`.
- Persist before notifying: an important state change is written to `newen-store` before the UI sees it.
- Unknown CLI messages are kept as `Unknown`, never a crash. A CLI version outside the tested range is reported in the UI.
- Newen does not identify itself as another client (e.g. Claude Desktop) and uses no vendor code, fonts, icons or branding. One exception: an adapter (`newen-agent-<id>`) may ship its agent's logo, only to name that agent where Newen lists agents; generic code never holds one.
- Logos of connectors, plugins and model providers are never shipped: the engine looks them up at run time (svgl.app by name, else the logo address its agent's catalogue gives, else the server's site), keeps them in the person's data dir, and a Settings switch turns that off.
- Every new dependency gets a one-line comment in `Cargo.toml` saying why.
- Newen was called quill until 0.1.9. quill's names (its folders, `quill.db`, `QUILL_*` variables, keychain service, socket, settings values, launchers, repository and manifest) appear only where Newen moves a person's data or keeps quill's copies, launchers and updates working: `newen_engine::migrate`, the fallbacks docs/UPDATES.md lists under «Venir de quill», their tests and that doc. No other code names them.
- Behavior modeled on Claude Desktop is described by what the user sees; its reverse-engineering notes stay private and are never cited or copied into this repo. What is inferred is said to be inferred.
- Before a commit: `scripts/check.sh` (fmt, clippy `-D warnings`, tests).
