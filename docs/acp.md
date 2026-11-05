# Agentes ACP en Newen (`newen-agent-acp`)

OpenCode y Hermes Agent hablan el **Agent Client Protocol** (ACP, agentclientprotocol.com): JSON-RPC 2.0 por stdio, un mensaje por línea, el mismo transporte que Codex. Newen los maneja con un núcleo común y un adaptador fino por CLI:

- `crates/newen-agent-acp`: el protocolo ACP, sin nombres de proveedor (AGENTS.md). `Acp` hace `encode`/`decode`; `Flavor` es lo que cada agente dice a su manera (cómo se leen sus herramientas, `_meta` propios, notificaciones que se callan, mensajes que reenvía enteros). `replay` reproduce capturas para los tests de cada agente.
- `crates/newen-agent-opencode` ([opencode.md](opencode.md)) y `crates/newen-agent-hermes` ([hermes.md](hermes.md)): identidad, programa, modos, logo, opciones de Settings, Customize y su `Flavor`. Cada uno implementa `AgentAdapter` y le pasa `encode`/`decode` al núcleo.

Newen se presenta como `clientInfo {name: "newen"}`. Declara `elicitation.form` (para las preguntas) y **no** ofrece `fs/*` ni `terminal/*`: los dos agentes leen, escriben y corren comandos ellos mismos (verificado en sus fuentes; ninguno los pide), y un pedido de esos se contesta `-32601`.

## Mapa del contrato

| Newen | ACP |
|---|---|
| `Start` → `Session` + `Ready` | `initialize` → `session/new {cwd, mcpServers}`; con `resume`: `session/resume` (o `session/load`), y con `fork`, `session/fork`. Lo que un `load`/`resume` repite de la historia antes de su respuesta no se muestra: ya es de Newen |
| `Ready.capabilities` | modelos, esfuerzos y modos salen de `configOptions` (categorías `model`, `thought_level`, `mode`) o de `models`/`modes` cuando el agente los da. Los esfuerzos son los del modelo actual; los de otro llegan al elegirlo. Los comandos `/`, de `available_commands_update` (llega después de abrir: `Ready` se repite) |
| `SetMode` / `SetModel` / `SetEffort` | `session/set_mode` (modos de `modes`) o `session/set_config_option`; `session/set_model` si el modelo solo viene en `models`. Lo pedido durante un turno espera a que termine (Hermes los rechaza con un turno en curso). Lo elegido en el spec se manda al abrir, y el primer prompt espera esas respuestas |
| `Prompt` | `session/prompt {prompt: [image…, text]}`; `Running` con la primera señal del turno. Sin `steer`: los dos agentes son un turno a la vez (OpenCode rechaza otro), así que el motor encola |
| `TextDelta` / `Text`, `Thinking*` | `agent_message_chunk` / `agent_thought_chunk`; un mensaje termina cuando cambia su `messageId`, llega una herramienta o termina el turno |
| `ToolUse` / `ToolResult` | `tool_call` / `tool_call_update`. Una llamada se dibuja cuando corre (su `status` deja de ser `pending`: OpenCode manda el input completo recién en `in_progress`). El `Flavor` la traduce a las herramientas que la UI ya dibuja (`Bash`, `Read`, `Edit`, `Write`, `Grep`, `Glob`, `WebFetch`, `WebSearch`, `TodoWrite`, `Agent`, `Skill`, `mcp__<servidor>__<tool>`); sin traducción, `by_kind` usa el `kind` de ACP. Un bloque `diff` es un `FileChange`; el texto, la salida (`CommandRun` en `Bash`) |
| `plan` | la lista de tareas, como `TodoWrite` (igual que Codex) |
| `PermissionRequest` → `Decide` | `session/request_permission` → `{outcome: {outcome: "selected", optionId}}` eligiendo por `kind` (`allow_once`, `allow_always` como sugerencia "Always", `reject_once`). Las herramientas de Newen se permiten solas |
| Pregunta (`AskUserQuestion`) | `elicitation/create` en modo `form`: cada propiedad del esquema es una pregunta (`oneOf`/`enum` son sus opciones; `<clave>_custom`, la respuesta en palabras propias) y se contesta `{action: "accept", content}` |
| `Interrupt` | `session/cancel`; las preguntas pendientes se contestan `cancelled` y se retiran (`Withdrawn`) al terminar el turno. `$/cancel_request` del agente retira una tarjeta |
| `TurnFinished` | la respuesta de `session/prompt`: `end_turn` (ok), `cancelled` (interrumpido: también cuando la persona niega un permiso en OpenCode, que corta el paso), `-32000` es `AuthRequired`. `usage` + el último `usage_update` (contexto usado, tamaño, costo acumulado → el del turno) |

Lo que ACP no tiene y queda degradado: desglose del contexto por categoría (solo el total), rewind de archivos del CLI (los restaura el motor con sus instantáneas), estado de MCP por sesión. **Limitación:** `resume_at` no tiene equivalente: un fork copia la conversación entera, y un rewind a mitad deja en la memoria del agente lo que vino después (`shortcut:` en `Acp::open`).

## Las herramientas de Newen (`SessionExtras::host`)

Ninguno de los dos acepta herramientas del cliente por el propio ACP (OpenCode rechaza el transporte MCP `acp`; Hermes no lo implementa), pero los dos arrancan los servidores MCP stdio que les llegan en `session/new`. Así que las herramientas de Newen van como **tres servidores MCP propios**, `sessions` (`HOST_TOOLS`), `browser` (`BROWSER_TOOLS`) y `terminal` (`TERMINAL_TOOLS`, [terminal.md](terminal.md)):

1. Un adaptador con `SessionExtras::host` recibe esos tres servidores en `SessionSpec::servers`: el motor los agrega en `inject` (`Bridge::servers`), con el programa de Newen y `--mcp-host <servidor>`, y en su entorno el socket y la clave de la sesión (las mismas variables del puente de `$BROWSER`).
2. `newen --mcp-host <servidor>` (apps/newen) pasa su stdio al socket del puente con la clave.
3. El puente (`newen-engine/src/bridge.rs`, `serve`) contesta el handshake y `tools/list`, y cada `tools/call` corre como una llamada del propio agente: `Internal::HostTool` con un id `bridge-…`, y la respuesta vuelve por `Relayed` (`Router::host_answer`) en vez de ir al stdin del agente.

El test `every_one_of_newens_tools_is_offered_and_runs_for_its_session` (bridge.rs) comprueba que el servidor ofrece todas; los de cada adaptador, que sus llamadas se leen como `mcp__sessions__<tool>` y nunca piden permiso; las capturas `host`, que el CLI real las ve y las llama. El puente es un socket Unix: en Windows estos dos agentes no tienen las herramientas de Newen (`shortcut:` en `mcp_host`). El relé es el propio programa de Newen, que carga sus bibliotecas (CEF) por su `RUNPATH $ORIGIN`: Hermes arranca los servidores con un entorno limpio (sin `LD_LIBRARY_PATH`), así que en una compilación de desarrollo sin `libcef.so` junto al binario el servidor de Newen no arranca en Hermes (instalado, sí; con OpenCode, que pasa su entorno, también).

**Verificado de punta a punta (2026-11-03)** con el motor real (`Engine` con su puente), una sesión de cada agente y "Call the list_sessions tool…": OpenCode 2.2.5 (`big-pickle`) llamó `tools.sessions.list_sessions()` y Hermes `mcp__editor_sessions_…__list_sessions`; los dos recibieron la respuesta de Newen ("No other session works in this project."). La prueba fue un ejemplo temporal, no está en el repo.

## Secretos de los servidores "All agents" (`SessionExtras::inline_secrets`)

ACP solo lleva valores en `env` y `headers`, y los servidores de Hermes ni heredan su entorno (corren en su daemon). Con `inline_secrets`, el motor pone el valor del llavero en el servidor que va en `session/new` (por stdin, nunca en argv ni en disco: el spec no se guarda) en vez de una variable del proceso. Vale para encabezados también.

## Capturas

`newen-capture --agent opencode|hermes <escenario>` (tools/capture/src/acp.rs) conduce el CLI real **con el propio adaptador**, como el motor, y graba cada línea más `spec` y `cmd`; el test la reproduce con `replay` y exige que el adaptador escriba lo mismo, línea por línea. Las herramientas de Newen las atiende la propia captura (`--mcp-stub`). Escenarios: `simple`, `edit-bash`, `deny`, `always`, `todos`, `ask`, `interrupt`, `resume`, `fork`, `plan`, `ask-mode`, `risky`, `host`. Con `--fake-model <puerto>` la captura hace de modelo compatible con OpenAI con un guion por escenario (model.rs), para un agente apuntado a ese puerto. Las rutas quedan como `/work`, `/home/user` y `/bin/newen`.

Cada CLI se corre con su configuración en una carpeta aparte (ver cada doc): nunca la de la persona.
