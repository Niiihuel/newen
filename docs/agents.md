# Adaptadores de agente (`quill-agent`)

quill maneja cualquier agente de código por CLI mediante un adaptador (spec.md D11). Este documento fija el contrato común y cómo se proyecta cada CLI sobre él. Solo el adaptador de Claude Code está verificado con capturas (engine.md §11.4). Las filas de otros CLIs son **hipótesis de diseño a verificar** con su propia fase de captura antes de implementarlos. La columna de Codex ya está verificada con capturas de `codex-cli 0.163.0` (`app-server` v2); el detalle está en [codex.md](codex.md).

## Contrato

El adaptador es **puro**: no abre procesos ni sockets. El motor (`quill-engine`) lanza el proceso según el `LaunchPlan`, y por cada línea que llega llama a `decode`:

```text
LaunchPlan  = programa, args, env (a poner y a quitar), cwd
decode(line) → Decoded { events: Vec<AgentEvent>, replies: Vec<String> }   // replies: respuestas automáticas (p. ej. hooks)
encode(cmd)  → Vec<String>                                                  // líneas a escribir
```

| Concepto | `AgentEvent` / `AgentCommand` | Claude Code (verificado) | Codex 0.163.0 (verificado) | OpenCode (hipótesis) |
|---|---|---|---|---|
| Arranque | `Command::Start` → `Event::Ready{session, capabilities}` | `control_request initialize` → respuesta con `models`, `commands`, modo actual; `system/init` con `session_id` y `capabilities` | `codex app-server` (JSON-RPC por stdio): `initialize` → `initialized` → `thread/start` / `thread/resume` / `thread/fork`; los modelos salen de `model/list` | `opencode serve` (HTTP + SSE): crear sesión por API |
| Prompt | `Command::Prompt{request, text}` | `user` con `uuid` = `RequestId` | `turn/start` (con un turno en curso, `turn/steer`) | `POST /session/:id/message` |
| Texto en streaming | `Event::TextDelta` / `Event::Text` | `stream_event` `text_delta`; `assistant` completo | `item/agentMessage/delta` y `item/completed` | evento SSE de parte de mensaje |
| Herramienta | `Event::ToolUse{call, name, input}` / `Event::ToolResult{call, output, is_error, files, command}` | `assistant.tool_use`; `user.tool_result` + `tool_use_result` estructurado | `item/started` / `item/completed` de `commandExecution`, `fileChange`, `mcpToolCall`, `webSearch`; el plan, con `turn/plan/updated` | partes `tool` del mensaje |
| Permiso | `Event::PermissionRequest{id, call, tool, input, suggestions}` → `Command::Decide{id, Decision}` | `control_request can_use_tool` → `control_response` allow/deny | pedidos del servidor `item/commandExecution/requestApproval` y `item/fileChange/requestApproval`; se contestan `accept` / `acceptForSession` / `decline` | evento `permission.updated` → responder por API |
| Pregunta al usuario | `PermissionRequest` con `interactive: true`, respuesta en `Decision::Answer` | AskUserQuestion vía `can_use_tool` + `updatedInput.answers` | `item/tool/requestUserInput`, contestado con `{answers:{id:{answers:[…]}}}` | — |
| Interrumpir | `Command::Interrupt` → `Event::TurnFinished{interrupted}` | `control_request interrupt` | `turn/interrupt` → `turn/completed` interrumpido | `POST /session/:id/abort` |
| Fin de turno | `Event::TurnFinished{ok, usage, cost}` | `result` sin `parent_tool_use_id` | `turn/completed` | sesión idle |
| Atribución por herramienta | `capabilities.tool_hooks` | `hook_callback` PostToolUse con `prompt_id` | no hay hooks: atribución por turno | partes `patch` |
| Cambios de archivo estructurados | `ToolResult.files` | Write/Edit: `structuredPatch`, `originalFile` | `fileChange`: solo el diff por archivo (el hunk de un cambio, el contenido de un archivo nuevo) | parte `patch` |
| Reanudar / fork | `capabilities.resume/fork` | `--resume`, `--fork-session` | `thread/resume`, `thread/fork` | sesiones persistidas del servidor |
| Rewind propio del CLI | `capabilities.rewind` | `rewind_files` | — | `revert` |
| Desglose del contexto | `Command::ContextUsage` → `Event::ContextUsage{used, max, categories, groups}` (sin respuesta si el agente no lo sabe) | `control_request get_context_usage` (`detail: "full"`) | solo el total: `thread/tokenUsage/updated`, sin categorías | — |
| Siguiente prompt sugerido | `Event::PromptSuggestion{text}` tras el fin de turno; efímero, el store no lo guarda | con la opción "Prompt suggestions" del adaptador (prendida por defecto), `initialize` lleva `promptSuggestions: true` y llega `prompt_suggestion{suggestion}` ~1 s después del `result`, nunca tras el primer turno (2.1.316, fixture `2.1.316/suggest`) | no tiene: `thread/prediction/request` es experimental y la 0.163.0 lo contesta "not implemented yet" | — |

Lo que un CLI no tenga se declara en `AgentCapabilities` y la UI lo muestra degradado. Por ejemplo, sin hooks la atribución se hace por turno con el repo sombra, y sin permisos interactivos no hay tarjeta de permiso.

## Transportes

El contrato cubre CLIs por **stdio con líneas JSON**: Claude Code (stream-json) y Codex (`app-server`, JSON-RPC). Un CLI que expone un **servidor HTTP** (OpenCode) necesita otro transporte en el motor, que entrega al adaptador cada evento SSE como una "línea". Ese segundo transporte se agrega cuando se implemente el primer adaptador HTTP.

## Un proceso para todas las sesiones (`mux`)

Un agente puede correr todas sus sesiones en un solo proceso cuando las opciones de la persona lo piden: `mux(spec)` devuelve un `Mux`, puro como el adaptador, que reparte ese proceso. El motor lo arranca con la primera sesión que lo necesita (`launch` de un spec sin sesión), le pasa cada línea de cada sesión por `outbound` y cada línea del proceso por `inbound`, y cada sesión sigue hablando como si el proceso fuera suyo. Las variables de una sola sesión (el puente de `$BROWSER`) van en `SessionSpec::env`, y el adaptador las pone donde su CLI las toma por sesión. Hoy lo usa Codex para Remote Control ([remote-control.md](remote-control.md) §3).

## Cómo trabajar con la persona (`WORK_GUIDE`)

Cada agente recibe la misma guía, escrita por quill (`quill_agent::WORK_GUIDE`): preguntar con su herramienta de preguntas, con opciones y la recomendada primero, antes de trabajar en algo que deja dudas; proponer un plan antes de un cambio grande o riesgoso (plan mode si lo tiene); llevar una lista de tareas en lo que tiene varios pasos, con un último paso que compruebe el resultado; comprobar antes de decir que algo anda; y cerrar con qué cambió, cómo comprobarlo y qué no se verificó, más una sola pregunta si queda algo por decidir. No nombra herramientas de ningún CLI. Claude Code la recibe primera en `appendSystemPrompt`; Codex, como `developerInstructions` del hilo (al abrirlo, reanudarlo o bifurcarlo; codex 0.163.0 la acepta), que se suman a sus propias instrucciones.

## Modos de permiso (`modes`, `default_mode`)

El adaptador da las palabras de sus modos, que la UI no conoce: `modes() → Vec<ModeWords{id, label, about, tone, listed}>` (con los inseguros incluidos) y `default_mode()`. `tone` es uno de `Ask | Edits | Plan | Auto | Read | Unsafe` y decide ícono y tinta; viaja a la UI en `AgentInfo`. Claude: Manual, Accept edits, Plan, Auto, Bypass permissions (las palabras de Desktop). Codex: Read only, Auto, Full access.

## Cuenta (`sign_in_methods`, `auth_query`, `login_run`, `logout_run`)

El adaptador declara cómo se conecta la persona; el motor corre esos comandos fuera del hilo de la UI (`quill-engine/src/auth.rs`) y la UI sólo ve `AuthState` y los eventos `Auth`, `SignInUrl` y `SignInFailed`. Opcionales: un agente sin ellos no muestra pantalla de inicio de sesión.

| Concepto | Contrato | Claude Code 2.1.312 (observado, `cli-auth.md`) |
|---|---|---|
| Métodos | `sign_in_methods()` → (clave, etiqueta) | `claudeai` "Claude subscription", `console` "Anthropic Console" |
| Estado | `auth_query()` + `auth_state(stdout, exit)` | `auth status --json`; sale con 1 sin sesión pero imprime el mismo JSON (`loggedIn`, `authMethod`, `email`, `orgName`, `subscriptionType`) |
| Código | `login_takes_code()` (por defecto sí) | Claude: la página muestra un código que se pega de vuelta. Codex: no, el navegador vuelve solo al servidor local de `codex login`, y la pantalla solo espera |
| Login | `login_run(método)`, `login_url(línea)`, `login_code(código)` | `auth login --claudeai\|--console`: imprime la URL dentro de un hipervínculo OSC 8 (se quita) y espera en stdin `CODE#STATE`; espera sin límite, quill corta a los 600 s |
| Logout | `logout_run()` | `auth logout` (cierra también la sesión del CLI en la terminal) |
| Turno sin sesión | `AgentEvent::AuthRequired` | `assistant` con `error: "authentication_failed"` antes del `result` |

El motor abre la URL con `quill_files::open_in_system` (como la de un servidor MCP: `xdg-open` en Linux, `open` en macOS, `rundll32 url.dll,FileProtocolHandler` en Windows) y lanza el login con `BROWSER=true` para que el CLI no abra otra pestaña. El estado se pregunta con el mismo entorno que las sesiones (las variables del agente en Settings incluidas): con `ANTHROPIC_API_KEY` el CLI dice `api_key`. Cancelar mata el proceso hijo por su PID.

Codex 0.163.0 (verificado, ver [codex.md](codex.md)): un solo método, `chatgpt` "ChatGPT account"; `auth_query` es `login status` (escribe en stderr, el motor entrega el texto de la salida que tenga algo), `login` imprime la URL en stderr y vuelve por un servidor local, sin código; `logout` es `logout`. `--device-auth` no se ofrece: pide escribir un código que quill no muestra.

## Cuenta de GitHub (no es un agente)

GitHub es la identidad de la persona en quill (el pie de la sidebar y Settings › Accounts, ver [ui.md](ui.md) §2.14b). No pasa por `AgentAdapter`: el código genérico no nombra proveedor, y `gh` es una herramienta de la computadora, como `git`. El motor (`quill-engine/src/github.rs`) corre estos comandos fuera del hilo de la UI y la UI solo ve `GithubState`, `ContributionCalendar` y los eventos `Github`, `GithubCode`, `GithubFailed` y `GithubContributions`.

| Concepto | Comando | Notas |
|---|---|---|
| Estado | `gh auth status --hostname github.com`, luego `gh api user` | sale 0 si hay sesión; `api user` da `login`, `name`, `avatar_url` (forma confirmada con la cuenta real, solo lectura). "error connecting" no es "signed out": es `Unknown` |
| Sin `gh` | (el `spawn` falla con `NotFound`) | `GithubState::Missing` con la instrucción por sistema: `winget install GitHub.cli`, `brew install gh`, el gestor de paquetes en Linux |
| Conectar | `gh auth login --web --hostname github.com --git-protocol https --scopes read:user` | imprime `! First copy your one-time code: XXXX-XXXX`; el motor lo lee de stdout o stderr, abre la página y lo manda a la UI. Se lanza con `GH_BROWSER=true` y `GH_PROMPT_DISABLED=1`. No verificado contra un login real (no se corrió) |
| Desconectar | `gh auth logout --hostname github.com --user {login}` | cierra también el `gh` de la terminal; `--user` porque con varias cuentas `gh` no elige solo |
| Contribuciones | `gh api graphql -f query=…` | la misma consulta de openADE (`viewer { login contributionsCollection { contributionCalendar … } }`), validada en `parse_calendar` |

El token no sale del llavero de `gh` (Linux, Windows y macOS); no se imprime ni se guarda. `GH_TOKEN` y `GITHUB_TOKEN` se quitan del entorno de estos comandos para que la cuenta sea la del llavero.

## Extensiones por CLI (Skills, Plugins y Connectors)

Customize muestra lo de **cada CLI por separado** y los cambios se hacen con los comandos propios de cada uno, así que quedan también en la terminal (decisión del dueño §9.1 de [`extensiones.md`](extensiones.md)). La capa "All agents" (sección siguiente) es de quill y se inyecta por sesión. Lo que cada adaptador declara:

| Concepto | Contrato | Claude Code 2.1.312 | Codex 0.163.0 |
|---|---|---|---|
| Servidores MCP | `servers_query`, `servers`, `server_scopes` | `claude mcp list` (texto, revisa la salud); alcances local, project, user | `codex mcp list --json` (verificado); solo user: `mcp add` escribe `config.toml`, y el de un proyecto se edita a mano |
| Agregar y quitar | `ExtensionOp::AddServer`, `RemoveServer` | `claude mcp add\|remove` | `codex mcp add\|remove`: stdio con `--env`, o `--url`. **Sin encabezados**: no hay opción, así que el motor dice "Codex can't add a server with headers." |
| Plugins y marketplaces | `plugins_query`, `marketplaces_query`, `ExtensionOp` | `claude plugin …` | `codex plugin list --json --available`, `plugin add\|remove`, `plugin marketplace add\|list\|upgrade\|remove` |
| Apagar un plugin, actualizar uno | `ExtensionKinds.switch_plugins`, `update_plugins` | sí | **no**: no hay comando (el interruptor es del `config.toml`); el menú no los ofrece |
| Skills | `skills_query` + `skills_answer` (sin sesión), o `AgentCommand::Skills` | `get_skills_dialog` en una sesión de un solo uso | `skills/list` del `app-server` sin thread (verificado). Alcances `user`, `repo` (project), `system` y `admin`, con `enabled` |
| Estado de un skill | `ExtensionOp::SkillState`, `ExtensionKinds.skills_per_folder` | `edit-skill-overrides`, por carpeta | `skills/config/write {path, enabled}`: solo on u off, **de la persona** (no pide carpeta). Verificado contra un `CODEX_HOME` de prueba |
| Skills en el menú `/` | `AgentEvent::Skills` | vienen en `initialize` | el motor los pide aparte al estar listo (`skills/list`) y los suma a `capabilities.commands`; elegir uno manda `$nombre` (inferido, la mención explícita de la TUI) |
| MCP de la sesión | `AgentCommand::McpStatus`, `McpReconnect`, `McpAuthenticate`; `capabilities.mcp`, `mcp_toggle`, `mcp_clear_auth` | los tres pedidos de control, el interruptor y borrar credenciales | `mcpServerStatus/list {threadId}` (estado, herramientas, `authStatus`); `config/mcpServer/reload` para reconectar; `mcpServer/oauth/login`. **Sin apagado por sesión ni logout**: `mcp_toggle` y `mcp_clear_auth` en false y la UI no los muestra |

- **`ProgramRun.until`.** El `app-server` de Codex termina al cerrarse stdin **sin contestar** lo que quedó en vuelo (verificado: salió en 0,1 s sin responder). Un cambio que solo existe como pedido del `app-server` (el estado de un skill) pide `until`, y el motor mantiene stdin abierto hasta ver esa línea en stdout y entonces mata el proceso.
- **Pedidos propios con id de texto.** Los pedidos de quill que no están en las capturas (`quill-skills`, `quill-mcp-status`, …) llevan un id de texto, para que los números de los demás sigan siendo los de las capturas, y el agente falso los contesta aparte.
- **Un servidor de un plugin** se lista en `codex mcp list` como uno propio. El adaptador lo marca `Plugin` por la carpeta de su programa (`/.codex/plugins/`, inferido) para no ofrecer quitarlo.
- **La lista de Codex no arranca los servidores**: dice que están configurados, no que conectan. `ConfiguredServer.checked` en false, y la página escribe "On" en vez de "Connected".
- **Probado contra el CLI real, sin tocar la configuración de la persona:** `CODEX_HOME` y `CLAUDE_CONFIG_DIR` apuntados a carpetas de prueba (los dos la respetan; la `~/.claude.json` real no cambió). `codex mcp add|remove` (incluido quitar uno inexistente: sale 0 y lo dice en stdout) y `skills/config/write` contra una carpeta de prueba. De solo lectura contra la real: `mcp list --json`, `plugin list --json --available`, `plugin marketplace list --json`, `skills/list` y `mcpServerStatus/list`. `plugin add|remove` y `marketplace add|upgrade|remove` no se corrieron: se cubren con tests de los argumentos.

## La capa "All agents" (servidores MCP y skills globales)

Es de quill, no de ningún CLI: **se inyecta en cada sesión y no se escribe en `~/.claude.json`, `~/.codex/config.toml` ni en ninguna carpeta de skills de un CLI** (decisión del dueño §9.1 y §9.2). Los cambios valen para las sesiones **nuevas**.

- **Dónde vive.** `config_dir()/quill/extensions.json` (servidores y qué agentes tienen apagado cada skill) y `config_dir()/quill/skills/<nombre>/SKILL.md` (los skills). Lo escribe solo el motor (`quill-engine/src/global.rs`), a un archivo temporal que renombra, y **guarda antes de avisar** con `Event::Global`.
- **Secretos.** Los valores marcados como secretos (variables o encabezados) van al llavero del sistema (`keyring`, servicio `quill`, cuenta `mcp/<servidor>/env:<CLAVE>` o `…/header:<CLAVE>`). En el archivo y en la UI quedan como `secret: true` sin valor. Llegan al agente **solo como variable de entorno** de su proceso (`QUILL_MCP_<SERVIDOR>_<CLAVE>`; los encabezados llevan `H_` delante). El llavero se lee al lanzar la sesión, nunca para dibujar la página. Sin llavero (Secret Service) no se puede guardar un secreto y el diálogo lo dice. *No verificado:* el backend real del llavero no se ejercitó (las pruebas usan `MemorySecrets`).
- **Regla de nombres (decisión §9.4).** Si el agente ya tiene un servidor o un skill con ese nombre, **gana el del agente**, quill no inyecta el global en ese agente y la fila dice "Replaced in {agente}". El motor decide con las últimas listas que leyó de cada agente (`Event::Servers` y `Event::Extensions`, guardadas en `Router.own`). **Límite:** si todavía no leyó la lista de un agente (nadie abrió Customize desde que arrancó quill), inyecta igual, y con Claude eso hace ganar al global (verificado en V3). Abrir Connectors o Skills antes de la primera sesión basta.
- **Por agente.** Cada servidor o skill tiene `except: [agentes]`; un agente nuevo recibe todo lo compatible. Lo que el agente no puede usar queda como "Not in {agente}" con el motivo del adaptador (Codex: SSE).
- **Contrato** (`quill-agent`, sin nombres de proveedor): `SessionSpec.servers: Vec<InjectedServer>` y `skill_roots`; `AgentAdapter::session_extras() -> SessionExtras { servers, sse, env_by_name, skills }`; `McpSource::Global` (lo pone el motor al reconocer los nombres inyectados en `AgentEvent::McpServers`, nunca el adaptador). Las carpetas de skills de cada agente las arma el motor: `quill/injected/<agente>/skills/<nombre>`, enlaces a los skills que ese agente usa.

| | Claude Code 2.1.312 | Codex 0.163.0 |
|---|---|---|
| Servidores | `--mcp-config '<json>'` en argv, **sin secretos**: un secreto es `"${QUILL_MCP_…}"` y el CLI lo expande con la variable de su entorno. Sin `--strict-mcp-config`, así que suma a los de la persona. stdio, http y sse | `thread/start.config.mcp_servers` (viaja por stdin). stdio: `command`, `args`, `env`, `env_vars`; http: `url`, `http_headers`, `env_http_headers`. **Sin SSE.** `env_vars` solo reenvía una variable con su propio nombre, así que el secreto de un servidor stdio va en una variable **con el nombre de la clave** (`GITHUB_TOKEN`), y dos servidores globales que pidan el mismo nombre con valores distintos chocan (el segundo queda "not available") |
| Skills | `--plugin-dir quill/injected/claude`: el adaptador escribe `.claude-plugin/plugin.json` (`name: quill`) y los skills salen como `/quill:<nombre>` | `skills/extraRoots/set {extraRoots}` justo después de `initialize`, antes de `thread/start`; los skills salen en `skills/list` con alcance `user` |
| Estado en la sesión | `mcp_status` los muestra con `scope: "dynamic"`; el motor los marca Global | `mcpServerStatus/list` los muestra |

**Verificaciones con los CLIs reales (2026-10-28, sin turnos de modelo, sin tocar las configuraciones de la persona).** Claude Code con `CLAUDE_CONFIG_DIR` en una carpeta de prueba y un servidor MCP local que registra su argv y su entorno; Codex con `CODEX_HOME` de prueba y `app-server` (`initialize`, `thread/start`, `mcpServerStatus/list`, `skills/list`), sin `turn/start`.

- **V1, Claude expande `${VAR}` dentro de `--mcp-config`: sí.** Tanto con un archivo como con el JSON en el argumento. El servidor recibió `args ["--tok","<valor>"]` y `env TEST_SECRET=<valor>` con el valor de la variable del proceso de Claude. `mcp_status` informa `scope: "dynamic"`, `source: "dynamic"`, `status: connected`. **Ojo:** `mcp_status` devuelve en `config.args` el valor ya expandido, así que quill no debe mostrar nunca ese `config`.
- **V2, Codex acepta `mcp_servers` en `config` de `thread/start`: sí.** El servidor stdio conectó (`runtimeStatus: connected`, con sus herramientas) y recibió la variable de `env_vars`; uno http con `env_http_headers` se listó (falló al conectar por ser una dirección inventada, no por la configuración). `skills/extraRoots/set` contesta `{}` y el skill aparece en `skills/list`; también con la carpeta hecha de enlaces simbólicos.
- **V3, quién gana con el mismo nombre.** En Claude, **el de `--mcp-config` gana sobre el de la persona** (con un servidor de usuario del mismo nombre, el de la persona ni arrancó). Por eso la regla de nombres la tiene que aplicar quill. En Codex no se probó (`sessionFlags` va por encima de la configuración, §4.4 de la spec: igual que Claude, inferido).
- **V4, nombre de los skills de `--plugin-dir`: `quill:<nombre>`** (visto en los `commands` de `initialize`), también con la carpeta de skills hecha de enlaces. `--add-dir <carpeta>` **sí** carga `<carpeta>/.claude/skills` (apareció como `dskill`).
- **V7, el `app-server` termina al cerrarse stdin: sí** (salió con 0 al instante).
- **V5 (`claude mcp get`) y V6 (`mcp_authenticate` con un servidor de `--mcp-config`) no se verificaron:** no bloquean esta fase (V6 necesita un servidor OAuth; V5 es de "Make global", fase 4).
