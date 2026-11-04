# Newen — Especificación técnica: Motor de agente (borrador)

> **Nombres (2026-10-25):** Newen es multi-agente. Lo que este borrador llama `newen-protocol` es el adaptador `newen-agent-claude`; lo genérico va en `newen-agent` (trait `AgentAdapter` + `AgentCapabilities`) y en `newen-engine`, sin nombres de proveedor. Ver spec.md D11.

> Estado: borrador v0, 2026-10-25. Alcance: MVP de la surface "Code" (sesiones locales que manejan el CLI `claude` como subproceso). Resumen y decisiones finales: [`spec.md`](spec.md).
> No se copia código ni prompts de Anthropic. Lo que dice "Desktop" describe cómo se comporta Claude Desktop a ojos de la persona; todo lo que dice "Newen" es diseño propio.

## 0. Convenciones de este documento

| Marca | Significado |
|---|---|
| **Desktop** | Comportamiento visible de Claude Desktop. |
| **Newen** | Diseño propuesto. |
| *(inferido)* | Deducción, sin evidencia directa. |
| *(verificar)* | Hay que confirmarlo en ejecución contra un CLI real antes de depender de ello (ver §11). |

Arquitectura: UI → `Intent`/`Event` → motor → proceso `claude` por stdin/stdout en stream-json. La app **no** implementa el loop del agente: lo hace el CLI. La app orquesta: arma opciones/env, encola entradas, traduce mensajes, intercepta permisos y gestiona el proceso.

**Decisión base de Newen:** no hay Agent SDK en Rust, así que Newen implementa directamente la capa delgada que el SDK pone entre la app y el CLI (framing JSON por línea, multiplexado de control, callbacks de hooks y permisos). Es la parte de §3. El resto es el gestor de sesiones del motor.

---

## 1. Ciclo de vida de sesión

### 1.2 Newen

**Modelo: estado derivado de hechos, no máquina de estados almacenada.** Persistimos hechos y calculamos la vista.

```text
SessionFacts {
  process: None | Spawning{since} | Live{pid, initialized: bool}
  turn: Option<OpenTurn>          // §6
  pending_permissions: Vec<PermId>
  archived: bool
  parked: Option<CrashLoopInfo>   // crash-loop guard
  last_error: Option<SessionError>
  engine_session_id: Option<Uuid> // id del CLI
}
SessionStatus (vista) = Draft | Starting | Running | AwaitingPermission | Idle | Paused | Parked | Errored | Archived
```

| Operación | Newen |
|---|---|
| `create(cwd, opts)` | Valida que `cwd` exista y sea dir; crea fila `sessions` con `id = uuid v7` (ordenable); `status = Draft`. Worktree opcional (§8). No hace spawn si no hay mensaje. |
| `send(first message)` | Spawn (§2) → `initialize` → escribe `user`. |
| Id del CLI | Pasar `--session-id=<uuid propio>` en el primer spawn *(verificar que el CLI lo acepte con stream-json input)*, de modo que `engine_session_id` se conozca antes de `system/init`. Igual se valida contra `system/init.session_id` y, si difiere, gana el del CLI). |
| Pausar | Actor de sesión cierra stdin y espera salida (timeout → SIGTERM → SIGKILL al grupo). Gobernador global: tope `max(6, RAM/3 GiB)`; idle timeout configurable (proponemos 15 min). |
| Reanudar | Spawn con `--resume=<engine_session_id>`. La UI se pinta desde el store propio (no relee el JSONL). Mantener el `cwd` del registro: el JSONL vive en `projects/<cwd saneado>/`; si el cwd cambió (worktree movido), copiar el JSONL o rechazar *(verificar algoritmo de saneado)*. |
| Fork | `--resume=<padre> --fork-session` (+ `--resume-session-at=<uuid>` para fork en un punto) en vez de copiar el JSONL. Así no dependemos del formato/ubicación interna del JSONL. El CLI devuelve un `session_id` nuevo en `system/init` *(verificar)*. |
| Rewind conversación | Como en Desktop ("Rewind to here" en el mensaje del usuario). `Intent::RewindTo { request, at }` detiene el CLI y el próximo spawn usa `--resume --resume-session-at=<at> --fork-session`, donde `at` es la respuesta anterior al mensaje. Si no hay respuesta anterior, arranca de cero. `Event::RewoundTo` queda guardado: la UI recorta desde ese mensaje, el texto vuelve al composer y, si Newen se reinicia antes del próximo mensaje, `Restored` retoma desde ese punto. `Intent::UndoRewind` (Desktop's "Undo rewind", ubicación inferida) devuelve los mensajes y el agente retoma la conversación anterior, hasta que la persona envía un mensaje; tras un reinicio, deshacer vuelve a la última conversación del CLI. |
| Archivar | Detiene proceso, marca `archived`; limpieza de worktree solo si no hay cambios sin commitear (§8). |
| Borrar | Borra filas propias y snapshots. **No** borra el JSONL del CLI por defecto (opción explícita "borrar también transcript del CLI"). |
| Importar del CLI | Fuera del MVP. Si se hace: solo lectura del JSONL + adopción con `--resume`, con chequeos de identidad del archivo, liveness y confianza. |
| Side chat | Fuera del MVP. |

---

## 2. Lanzamiento del CLI

### 2.4 Newen

**Resolución del binario (MVP):**

1. `config.cli_path` si está.
2. `claude` del PATH **del login shell** (`$SHELL -l -i -c 'env -0'` con timeout 4 s, una vez al arrancar y cacheado).
3. Si no hay, error accionable ("instalá Claude Code") — **sin descarga propia en el MVP**.
4. Preflight por spawn: `claude --version` cacheado por (ruta, mtime, tamaño); rechazo si `< MIN_CLI_VERSION` (piso global) y gating por feature (p. ej. `auto` ≥ 2.1.71).

Descarga gestionada (post-MVP): bajar `manifest.json` + `.sig` de releases, verificar firma con la clave pública embebida (crate `sequoia-openpgp` o `pgp`), verificar SHA-256 del binario, escribir a `.partial` → rename → marcador `.verified` con el hash (no solo existencia, a diferencia de). Ubicación `$XDG_DATA_HOME/newen/cli/<ver>/claude`.

**Línea de comando de Newen:**

```text
claude --output-format stream-json --verbose --input-format stream-json
       --permission-prompt-tool stdio
       --include-partial-messages
       --thinking-display summarized
       --setting-sources=user,project,local
       --replay-user-messages                        # los mensajes del celular (remote-control.md)
       --permission-mode <mode>
       [--model <model>] [--effort <level>]
       [--session-id=<uuid>]                         # primer spawn
       [--resume=<id> [--fork-session] [--resume-session-at=<uuid>]]
       [--add-dir <dir>]...
cwd = worktree_path || cwd
```

**Entorno de Newen:** login-shell env + `CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING=true`, `CLAUDE_CODE_ENABLE_ASK_USER_QUESTION_TOOL=true`, `CLAUDE_CODE_EMIT_TOOL_USE_SUMMARIES=false`, `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1`, `MCP_CONNECTION_NONBLOCKING=true`, `CLAUDE_CODE_SDK_READS_SESSION_STATE=1`, `CLAUDE_CODE_EAGER_FLUSH=1`, `NEWEN_SESSION_ID=<id>`; quitar `NODE_OPTIONS`.

- `CLAUDE_CODE_ENTRYPOINT`: **no** usar `claude-desktop` (sería hacerse pasar por Desktop). Opciones: dejarla sin definir o un valor propio *(verificar qué acepta el CLI y si cambia comportamiento)*.
- **Autenticación delegada al CLI**: Newen no maneja tokens OAuth en el MVP; el CLI usa su propio login (`~/.claude`). Se responde `oauth_token_refresh` con error si llega. Evita guardar secretos.
- stderr del CLI → log rotado por sesión (`logs/cli-<session>.log`).

---

## 3. Protocolo SDK↔CLI (stream-json)

Todo es **un objeto JSON por línea** en stdin (app→CLI) y stdout (CLI→app). El lector separa: `control_response` (resuelve pedidos pendientes), `control_request` (callbacks), `control_cancel_request`, `keep_alive` (descartar), `transcript_mirror`; el resto son mensajes de conversación.

### 3.1 Mensajes de conversación (CLI → app)

Tipos de mensaje que el adaptador reconoce: `user`, `assistant`, `system`, `result`, `stream_event`, `tool_use_summary`, `tool_progress`, `auth_status`, `prompt_suggestion`, `rate_limit_event`. Campos clave:

| type / subtype | Campos clave | Uso |
|---|---|---|
| `system/init` | `session_id`, `claude_code_version`, `cwd`, `plugins`, `plugin_errors`, … | id del CLI |
| `user` | `uuid`, `session_id`, `parent_tool_use_id`, `message{role, content[text\|image\|tool_result{tool_use_id, content, is_error}]}`, `isReplay?`, `timestamp`, `priority?` | eco del prompt (`isReplay:true`, mismo uuid) y resultados de herramientas |
| `assistant` | `uuid`, `session_id`, `parent_tool_use_id`, `message{model, content[text\|thinking\|tool_use{id,name,input}], usage}` | respuesta y tool calls |
| `stream_event` | `event` (`content_block_start/delta/stop`, `message_*`), `parent_tool_use_id`, `uuid` | deltas `text_delta`, `input_json_delta`, `thinking_delta` |
| `result` | `subtype` (`success`\|`error_*`), `is_error`, `result?`, `errors?`, `duration_ms`, `num_turns`, `queued_turn_count?`, `parent_tool_use_id?`, `uuid`; `usage`/`total_cost_usd` *(inferido)* | cierre de turno si no tiene `parent_tool_use_id` |
| `system/session_state_changed` | `state ∈ {idle, running, requires_action}` | fin de corrida = `result` + `idle` |
| `system/status` | `status`, `permissionMode?` | cambio de modo originado en el CLI; `compacting` |
| `system/post_turn_summary` | `title`, `description`, `status_category`, `status_detail`, `recent_action`, `needs_action`, `is_noteworthy`, `summarizes_uuid` | título/resumen de sesión |
| `system/task_started\|task_progress\|task_notification\|task_updated` | `task_id`, `tool_use_id?`; un workflow: `workflow_name` al empezar y, de a ratos, `workflow_progress` (fases y agentes) | tareas de fondo / subagentes; `TaskPhases` |
| `system/commands_changed` | `commands[]` | refrescar slash commands |
| `system/vcs_state_changed`, `code_change_published` | `kind`, `cwd` / `url`, `action`, `branch` | refrescar git, PR |
| `system/compact_boundary` | `compactMetadata{trigger, preTokens, postTokens, …}`, `logicalParentUuid` | compactación |
| `system/hook_started\|hook_progress\|hook_response`, `api_retry`, … | — | informativos |
| `prompt_suggestion` | `suggestion`, `uuid`, `session_id` | el siguiente prompt sugerido, en el composer (`AgentEvent::PromptSuggestion`, que el store no guarda). Llega ~1 s después del `result` y nunca tras el primer turno (verificado en 2.1.316, fixture `2.1.316/suggest`) |

Subagentes: un mensaje es de subagente si `isSidechain === true` o `parent_tool_use_id` no vacío.

### 3.2 Mensaje `user` (app → CLI)

```json
{"type":"user","uuid":"<uuid cliente>","session_id":"<id o \"\">","parent_tool_use_id":null,
 "message":{"role":"user","content":[{"type":"text","text":"…"}]},
 "priority":"next|later|now (opcional)","timestamp":"…"}
```

**Newen no envía `client_platform:"desktop_app"`**. Imágenes como bloques `image` base64 de la Messages API *(inferido)*.

### 3.3 Protocolo de control

Envoltorio (en ambos sentidos):

```json
{"type":"control_request","request_id":"<id>","request":{"subtype":"<subtipo>", …}}
{"type":"control_response","response":{"subtype":"success","request_id":"<id>","response":{…}}}
{"type":"control_response","response":{"subtype":"error","request_id":"<id>","error":"<texto>"}}
{"type":"control_cancel_request","request_id":"<id>"}     // CLI → app: abortar callback en vuelo
```

**App → CLI**:

| subtype | request | respuesta | Uso en Newen MVP |
|---|---|---|---|
| `initialize` | `hooks{<Evento>:[{matcher, hookCallbackIds:["hook_0",…], timeout}]}`, `sdkMcpServers`, `systemPrompt`/`appendSystemPrompt`, `agents`, `promptSuggestions`, `supportedDialogKinds`, `perTaskStopAffordance`, `workspaceTrust`, `plugins`, `planModeInstructions`, `jsonSchema` | success con datos de sesión (pid, commands, models…, forma exacta *(verificar)*) | **sí**, siempre primero; con `promptSuggestions: true` mientras la opción "Prompt suggestions" del adaptador esté prendida (lo está por defecto) |
| `interrupt` | `cancel_queued?` | `still_queued` / `cancelled` | **sí** |
| `set_permission_mode` | `mode` | success/error | **sí** |
| `set_model` | `model` | success | **sí** |
| `set_max_thinking_tokens` | `max_thinking_tokens` | success | post-MVP |
| `apply_flag_settings` | `settings` (p. ej. `{effortLevel}`) | success | **sí** (esfuerzo en caliente) |
| `rewind_files` | `user_message_id`, `dry_run` | `{canRewind, filesChanged[], insertions, deletions, skippedLinks?, error?}` (forma cruda del CLI *(verificar)*) | **sí** (fallback, §6) |
| `cancel_async_message` | `message_uuid` | cancelado / demasiado tarde | **sí** (cancelar steer) |
| `get_context_usage` | `detail: "summary"\|"full"` | `{totalTokens, rawMaxTokens, categories[{name, tokens, kind: used\|deferred\|buffer\|free}], memoryFiles[], mcpTools[], agents[], skills, …}` (verificado en 2.1.310, fixture `context`) | **sí** (desglose de la tarjeta Context) |
| `stop_task` | `task_id` | — | post-MVP |
| `background_tasks` | `tool_use_id` | — | post-MVP |
| `get_context_usage` | — | uso de contexto | post-MVP |
| `get_skills_dialog`, `reload_plugins` | — | `{skills: [{name, display_name, description, source, tokens, state, locked_by?, advertised, handles}]}`; `{commands, agents, plugins, mcpServers, error_count}` (2.1.311) | **sí**: los skills, desde un arranque corto del CLI en `$HOME` sin prompt (`Intent::Extensions`); `reload_plugins` a las sesiones corriendo cuando cambia un plugin |
| `mcp_status`, `mcp_reconnect`, `mcp_toggle`, `mcp_authenticate`, `mcp_clear_auth` | —; `serverName`; `serverName`, `enabled`; `serverName` (sin `redirectUri`: el CLI escucha en localhost); `serverName` | `{mcpServers: [{name, status, scope, source, error?, tools?…}]}`; `null`; — (verificado en 2.1.311, fixture `2.1.311/extensions`) | **sí** (`/mcp`, ui.md §2.11b) |
| `remote_control` | `enabled`, `name?` | encendido: `{session_url, connect_url, environment_id, bridge_epoch, bridge_session_id}`; apagado: success sin cuerpo (verificado en 2.1.316, fixture `2.1.316/remote`) | **sí** ([remote-control.md](remote-control.md)) |
| `rename_session`, `generate_session_title`, `side_question`, `seed_read_state`, `set_cwd`, otros `mcp_*`, `get_settings`, `update_settings`, `list_permission_rules` | varios | varios | no |

**Plugins, marketplaces y skills fuera de la sesión.** Se manejan con los comandos del CLI, sin editar sus archivos. Las salidas `--json` están en `fixtures/2.1.311/README.md`.
- **Listar:** `claude plugin list --json --available` y `claude plugin marketplace list --json`.
- **Cambiar:** `plugin enable|disable|uninstall <id> --scope <s>`, `plugin install <id> [--scope project|local]`, `plugin update <id>`, `plugin marketplace add|update|remove` y `edit-skill-overrides --json` (con el cambio por stdin).

El adaptador dice qué correr (`AgentAdapter::plugins_query`, `marketplaces_query`, `extension_op`) y lee las salidas (`plugins`, `marketplaces`, `extension_failed`). El engine lo corre en la carpeta de Customize, con tiempo máximo. Si el cambio sale bien, manda `reload_plugins` o `reload_skills` a las sesiones de ese agente que estén corriendo, y vuelve a leer todo.

**CLI → app**:

| subtype | request | respuesta esperada | Newen MVP |
|---|---|---|---|
| `can_use_tool` | `tool_name`, `input`, `tool_use_id`, `permission_suggestions`, `blocked_path`, `decision_reason`, `title`, `display_name`, `description`, `default_to_no`, `suppress_always_allow_rule`, `agent_id`, `requires_user_interaction?`, `matched_ask_rule?`, `mcp_server?` | allow: `{behavior:"allow", updatedInput, updatedPermissions?, toolUseID}`; deny: `{behavior:"deny", message, interrupt?, toolUseID}` | **sí** (§4) |
| `hook_callback` | `callback_id`, `input{hook_event_name, tool_name, tool_input, tool_response, tool_use_id, agent_id, cwd}`, `tool_use_id` | `{}` / `{hookSpecificOutput:{hookEventName, permissionDecision\|additionalContext\|updatedInput}}` / `{decision:"block", reason}` | **sí** (§6) |
| `mcp_message` | `server_name`, `message` (JSON-RPC) | `{mcp_response}` | post-MVP (tools propias, p. ej. terminal) |
| `elicitation` | `mcp_server_name`, `message`, `mode`, `url`, `elicitation_id`, `requested_schema` | decline *(inferido)* | responder decline |
| `request_user_dialog` | `dialog_kind`, `payload`, `tool_use_id` | según `supportedDialogKinds` | declarar `[]` y responder error |
| `oauth_token_refresh`, `host_auth_token_refresh`, `remote_control_work_secret` | — | — | error (auth delegada al CLI; con Remote Control prendido desde Newen no llegó ninguno, 2.1.316) |

Reglas de transporte (SDK):

- Las herramientas propias de Newen (`HOST_TOOLS`: `start_session`, `dismiss_task`, `send_to_session`, `read_session`, `list_sessions`, `wait_for_session`, `suggest_connectors`) llegan por `mcp_message` y el router las responde; se permiten sin preguntar. `read_session` lee los eventos de otra sesión del store con `spawn_blocking`, fuera del router, y `list_sessions` corre `git` igual. `wait_for_session` y `suggest_connectors` (que espera una tarjeta, fuera del router) se describen en [agents.md](agents.md).
- `null` del callback deja el pedido sin responder (caso de tools servidas por otra máquina). Newen: siempre responder.
- Con callbacks bidireccionales el stdin no se cierra hasta el fin de la corrida.
- Con `prompt` string el SDK manda un `user` y cierra (single-turn); Newen: siempre streaming input.

### 3.4 Newen: implementación del protocolo

- `newen-protocol`: tipos serde con **fallback que preserva lo desconocido**: `enum Inbound { Known(KnownMsg), Unknown(serde_json::Value) }`; cada tipo conocido conserva `extra: serde_json::Map` (`#[serde(flatten)]`) para no perder campos al persistir. Nunca fallar el stream por un campo nuevo.
- Codec: `tokio::io::BufReader::lines()` sobre stdout con tope por línea (p. ej. 64 MiB; los `tool_result` pueden ser grandes); escritura serializada por un único `mpsc` → task escritor.
- Pedidos salientes: `request_id = "q_<n>"`; `HashMap<RequestId, oneshot::Sender<Result<Value>>>` con timeout por subtipo (initialize 60 s, interrupt 10 s, rewind_files 30 s).
- Pedidos entrantes: cada uno en su task con `CancellationToken`; `control_cancel_request` cancela el token.
- `initialize`: el adaptador guarda los `commands` del agente en `AgentCapabilities::commands` (ordenados por nombre; si faltan, usa los nombres de `system/init`) para el "/" del compositor.
- `stream_event`: coalescer deltas por (índice de bloque, `parent_tool_use_id`) cada 16 ms antes de cruzar a la UI; **no** se persisten (el `assistant` completo sí).

---

### 3.5 Imágenes en un mensaje

Un mensaje lleva archivos de imagen (`Intent::Send.images`, `Event::Queued.images`): PNG, JPEG, GIF o WebP, reconocidos por su contenido y no por la extensión, hasta 20 por mensaje y 30 MB cada una; lo demás se omite con «Only PNG, JPEG, GIF, and WebP images are supported.», lo que pasa de 30 MB con «Some images were too large to attach. Maximum size is 30 MB.» y el resto del límite de 20 con «Maximum of 20 images allowed.». Una imagen de a lo sumo 2000×2000 px y 500 KB va tal cual; si no, se recodifica con lado máximo 2000 px, JPEG a calidad 0.9 y luego 0.7, reduciendo el lado ×0.75 hasta 500 px hasta caber en `min(500 KB, tamaño original)`, y se conserva el original si la recodificación no es menor y el original cabía en 500 KB. Desktop prefiere WebP cuando el canvas lo codifica; Newen usa JPEG (sobre blanco si hay transparencia) porque el codificador WebP del crate `image` es solo sin pérdida (equivalencia *inferida*). El procesamiento corre en un hilo bloqueante, después de guardar el `Queued` con las rutas originales; los avisos llegan como `Event::Notice`, que no se guarda. Al CLI, el mensaje va con los bloques `image` base64 (`{"type":"image","source":{"type":"base64","media_type","data"}}`) antes del bloque `text`, o solo el `text` sin imágenes; que el CLI 2.1.310 acepte esos bloques no está verificado con una captura *(inferido)*.

### 3.6 Imágenes en el resultado de una herramienta

Una herramienta puede responder con imágenes: el `Read` de una imagen, una captura del navegador de Newen, una herramienta MCP. El adaptador las lee de su protocolo y las deja en `AgentEvent::ToolResult.inline` como `ToolContent::Image` (base64): Claude Code, los bloques `image` de la Messages API (`{"type":"image","source":{"type":"base64","media_type","data"}}`) dentro del `content` del `tool_result`; Codex, los bloques de imagen de MCP (`{"type":"image","data","mimeType"}`) en `result.content` de un `mcpToolCall` *(inferido en los dos: ninguna captura tiene uno)*. Un bloque con otra fuente (una dirección) se omite. El `imageView` de Codex no trae la imagen, solo su ruta: es el `Read` de ese archivo.

El actor guarda las imágenes antes de emitir el evento: en un hilo bloqueante (`images::keep`), las decodifica, reconoce el formato por su contenido (PNG, JPEG, GIF o WebP; lo demás se omite) y escribe cada una en `<Config::pictures>/<sesión>/<uuid>.<ext>`, con nombres propios de Newen (nada de lo que manda el agente nombra un archivo). Recién entonces el evento sigue, con las rutas en `images` e `inline` vacío: así se guarda y así le llega a la interfaz. `inline` nunca se serializa (`#[serde(skip)]`), de modo que el registro guarda rutas y no imágenes. Sin `Config::pictures` (las pruebas) no se guarda ninguna. Al borrar una sesión se borra su carpeta de imágenes.

## 4. Permisos

### 4.2 Newen

| Aspecto | Diseño |
|---|---|
| Broker | `PermissionBroker` por sesión dentro del actor. Atajos locales antes de preguntar: (a) regla de sesión que matchea `tool_name` (+ `ruleContent` si hay; matching simple por prefijo de comando para Bash *(verificar semántica de reglas del CLI; si no la reproducimos fielmente, solo atajar coincidencias exactas)*), deshabilitado en `plan`; (b) `decision_reason` aceptado con "always". |
| Pedido | `PermissionRequest{perm_id, session_id, turn_id, engine_request_id, tool_use_id, agent_id, tool_name, input, suggestions, decision_reason, blocked_path, title, description, requested_at}` persistido **antes** de emitir a la UI. |
| Decisiones UI | `Deny{feedback?}`, `AllowOnce{updated_input?}`, `AllowAlways{updated_input?}`, `AnswerQuestions{answers}` (→ AllowOnce con `{questions, answers}`), `ApprovePlan{target_mode}` (→ AllowOnce + `setMode`). Es el contrato que el CLI espera. |
| Always | Devuelve las sugerencias **tal cual** en `updatedPermissions`. Guarda en store las de destino `session`. Las otras quedan a cargo del CLI. Mostrar en la tarjeta el destino de cada sugerencia (el usuario debe saber que "always" puede escribir `.claude/settings.local.json`). |
| Re-aplicar al relanzar | Reglas `session` → `--settings '<json con permissions.allow>'` y `--add-dir` *(verificar flag `--settings` con JSON inline y precedencia)*. Alternativa: `--allowedTools`. |
| Abort | El `CancellationToken` del turno resuelve pendientes como `deny "Request aborted"`. `control_cancel_request` → marcar `cancelled` y retirar tarjeta. |
| Estancados | Re-notificación a los 300 s; sin deny automático en el MVP. |
| Modo | `set_mode(m)`: validar (bypass prohibido como root; sin gate → error), `set_permission_mode`, persistir al ack. `system/status.permissionMode` actualiza el registro con `source: cli`. |
| Hooks guardia | PreToolUse propio solo para el worktree-write-guard cuando la sesión tiene worktree (§8). Carpetas permitidas: post-MVP. |
| Auditoría | Toda decisión queda en `permission_requests` con `decided_at`, `decision`, `decided_by` (user/rule/abort). Newen lo guarda local. |

---

## 5. Cola de mensajes, steering, interrupción y recuperación

### 5.2 Newen

**Una cola persistente por sesión** (tabla `engine_inputs`), lane derivada:

```text
EngineInput { input_id, session_id, request_ids[], priority: Next|Later|Now, position, state }
EngineInputAttempt { attempt_id, input_id, engine_uuid, prev_engine_uuid?, state: queued|sent|echoed|cancelled|withdrawn|failed, sent_at, echoed_at }
lane = if priority ∈ {Next, Now} { Steer } else { Queued }
```

| Caso | Comportamiento |
|---|---|
| Ocioso + send | abrir turno, escribir `user` (attempt `sent`). |
| Turno activo + `Next` | escribir `user` con `priority:"next"` ya (steer). Esperar eco; si el eco llega antes de la primera respuesta del turno, sumar el request al turno actual (`turn.request_ids += r`). |
| Turno activo + `Later` | quedar `queued`; al cerrar turno (`result` raíz) drenar **uno** y abrir el siguiente turno. |
| Promover / degradar / reordenar | cambia `priority`/`position`, nunca el `request_id`. |
| Cancelar steer ya enviado | `cancel_async_message(engine_uuid)`; si OK → attempt `cancelled`; si "too late" → informar, el mensaje sigue en el turno. |
| `Now` (escalada) | post-MVP. |

**Implementado: steer y cola** (como el steer y la cola de Desktop). Un mensaje enviado con turno activo lleva un `Lane`. `Steer` (Enter) se escribe al CLI con `priority:"next"` si el agente declara `steer` en sus capacidades; el adaptador traduce el flag, y la cadena del CLI solo vive en `newen-agent-claude`. Si el agente no lo declara, el mensaje va a la cola. `Queue` (Ctrl+Enter) nunca se escribe mientras corre el turno: queda en la cola de la sesión, que se emite entera en cada cambio (`Event::QueueChanged`) y el store la guarda antes de que la UI la vea. Al terminar el turno (`TurnFinished`), si la cola no está retenida, se envía: si hay varios mensajes y ninguno empieza por `/`, se unen en uno solo con una línea en blanco entre textos (imágenes y menciones en orden); si no, sale el primero y el resto espera al siguiente fin de turno. Interrumpir no vacía la cola, que se drena al terminar el turno interrumpido; si el agente sale, espera al siguiente fin de turno. Al reiniciar, la cola guardada vuelve **retenida** (`held`): no se envía sola, y solo `QueueSendNow` la libera. Todavía no hay recibo de lectura (`command_lifecycle`).

**Interrupción:** `interrupt` (timeout 10 s) → si no hay ack: SIGTERM al grupo de procesos, 3 s, SIGKILL; marcar turno `interrupted`; deny a permisos pendientes. Un `interrupt` con `cancel_queued` *(verificar semántica)* para vaciar los steers no consumidos.

**Recuperación:**

| Situación | Newen |
|---|---|
| CLI muere con turno abierto | turno `failed{reason: stream_ended_no_result}`; sesión `Errored`; próximo send relanza con `--resume`. |
| Crash-loop | 3 caídas con < 30 s de vida → `Parked`; solo un send explícito del usuario re-arma (como). |
| App cerrada con turnos abiertos | al cerrar: marcar `interrupted_by_quit_at`; al abrir: no auto-reanudar en el MVP (mostrar "interrumpido, ¿continuar?"). |
| Sin `result` 120 s tras `session_state_changed:idle` o sin actividad | cerrar turno como `timed_out` (backstop como). |
| CLI muere antes de `system/init` | reintentar el spawn una vez con el mismo input. |
| Mensajes encolados no entregados | sobreviven en SQLite; se re-presentan al reanudar (no se reenvían solos). |

---

## 6. Atribución por petición y checkpoints/rewind

### 6.2 Newen: modelo de datos

Separar **petición del usuario** (estable) de **mensaje entregado al motor** (uuid que puede cambiar) y de **turno** (ejecución):

```text
Request       { request_id, session_id, text_ref, origin: user|bang|system, created_at, status }
EngineInput / EngineInputAttempt   (§5.2; mapa engine_uuid → request_id)
Turn          { turn_id, session_id, request_ids[], primary_request_id, opened_at, first_response_at?,
                closed_at?, status: running|completed|interrupted|failed|timed_out,
                result_json?, pre_snapshot_id, post_snapshot_id? }
ToolCall      { tool_use_id, turn_id, parent_tool_use_id?, agent_id?, name, input_json,
                started_at, finished_at?, status: pending|ok|error|denied|interrupted,
                result_ref?, permission_id?, pre_snapshot_id?, post_snapshot_id? }
CommandRun    { command_id, turn_id?, tool_use_id?, origin: agent_bash|user_bang|user_terminal,
                cwd, command, exit_code?, started_at, finished_at?, output_ref? }
FileEffect    { effect_id, turn_id?, tool_use_id?, command_id?, actor: agent|user|external|unknown,
                path, prev_path?, kind: create|modify|delete|rename|mode, additions, deletions,
                before_blob?, after_blob?, detected_by: snapshot_diff|tool_input|watcher, confidence }
Snapshot      { snapshot_id, session_id, tree_oid, kind: pre_turn|post_tool|post_turn, created_at }
LineageEvent  { kind: rekeyed|coalesced|withdrawn|forked|rewound|compacted, source_ids[], target_ids[], at }
```

### 6.3 Newen: reglas

1. `request_id` se crea al aceptar el envío y no cambia nunca; rekey/cancel crean un attempt nuevo.
2. Turno abierto al escribir el `user` (ocioso) o al drenar la cola; se confirma con el eco; se cierra con el `result` sin `parent_tool_use_id`. Todo `assistant`/`user(tool_result)`/`system/*` entre medio pertenece al turno abierto.
3. ToolCall: alta al ver `tool_use` en `assistant`; cierre al ver `tool_result` con ese `tool_use_id`; subagentes se cuelgan por `parent_tool_use_id`. Al cerrar el turno, tool calls sin resultado → `interrupted`.
4. **Efectos de archivo por snapshot diff**, no por inputs de herramientas:
   - snapshot `pre_turn` al abrir turno;
   - hook **PostToolUse** registrado en `initialize` con matcher `Edit|Write|MultiEdit|NotebookEdit|Bash` (y `.*` para tools MCP que escriben *(decidir)*): en el `hook_callback` se toma snapshot `post_tool` y se calcula diff contra el snapshot anterior → `FileEffect`s con `tool_use_id`. Responder `{}` rápido.
   - snapshot `post_turn` al cerrar; un diff residual (cambios entre tools no explicados) → efectos `actor: unknown`.
   - el watcher del árbol (§8) marca cambios fuera de ventanas de herramienta como `actor: external` (o `user` si coincide con un comando OSC 133 de la terminal, §9).
   - `Edit/Write` `input.file_path` y `toolUseResult.structuredPatch` se guardan como evidencia secundaria (`detected_by: tool_input`), útil si no hay snapshot.
5. Concurrencia: dos escritores en el mismo intervalo → `confidence: shared`; no inventar autoría.
6. Tareas de fondo (`run_in_background`, `task_*`): sus efectos se atribuyen al turno que lanzó el `tool_use_id` aunque ocurran después del cierre.
7. Inyección de contexto al modelo (patrón `<desktop-app-activity>`): en `UserPromptSubmit`, si hubo rewind o cambios externos desde el último turno, `additionalContext` con una nota propia (texto escrito por Newen, no copiado).

### 6.4 Newen: checkpoints y rewind

**Snapshots en un repo git "sombra" por workspace**, no en el repo del usuario:

```text
$XDG_DATA_HOME/newen/snapshots/<hash(cwd)>.git   (bare)
GIT_DIR=<sombra> GIT_WORK_TREE=<cwd> GIT_INDEX_FILE=<sombra>/index-<session>
snapshot = git add -A && git write-tree   → tree_oid
ref      = refs/newen/<session>/<n> (commit-tree) para que gc no lo borre
```

- Funciona también en carpetas sin git. Respeta los `.gitignore` del árbol; excluir siempre `.git/` y usar `objects/info/alternates` al repo real si existe (velocidad/espacio).
- Cubre lo que hace Bash (borrados, renombres, archivos nuevos), a diferencia del checkpoint del CLI.
- Topes: desactivar snapshots si cwd = `$HOME` o `/`, o si el primer snapshot supera N archivos / M MB (configurable); en ese caso caer al checkpoint del CLI.

**Rewind de código (Newen):** `rewind_code(request_id, dry_run)`:
- rechaza si hay turno abierto ("busy") o restore en vuelo (como);
- objetivo = `pre_turn` del turno del request; dry-run = `git diff --numstat <objetivo> <snapshot actual>` → `{files[], insertions, deletions}`;
- real: tomar snapshot "pre_rewind" (permite deshacer), restaurar con `read-tree` + `checkout-index -a -f` y borrar archivos que no existen en el objetivo; reportar `skipped[]` (symlinks fuera del árbol, sin permisos);
- registra `RewindAction` y la nota para el próximo `UserPromptSubmit`.

**Rewind de conversación:** con `--resume --resume-session-at=<uuid assistant previo> --fork-session`. Ofrecer "código y conversación" / "solo conversación" / "solo código", como el diálogo de Desktop.

**Checkpoint del CLI**: se deja activo (`CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING=true`) como respaldo y para carpetas sin snapshot; se usa `rewind_files` solo si no hay snapshot propio.

---

## 7. Persistencia

### 7.2 Newen

| Qué | Dónde | Formato |
|---|---|---|
| Config editable | `$XDG_CONFIG_HOME/newen/config.toml` | TOML validado (`serde`), errores por entrada |
| Estado | `$XDG_DATA_HOME/newen/newen.db` | SQLite (WAL, `foreign_keys=ON`, migraciones versionadas), 0600 |
| Payloads grandes (`tool_result` > 256 KiB, salidas de comandos) | `$XDG_DATA_HOME/newen/blobs/<sha256>` | archivos direccionados por contenido; la fila guarda `*_ref` |
| Snapshots | `$XDG_DATA_HOME/newen/snapshots/<hash(cwd)>.git` | repo git sombra (§6.4) |
| Imágenes de las herramientas | `$XDG_DATA_HOME/newen/pictures/<sesión>/<uuid>.<ext>` (`Config::pictures`) | los archivos tal como llegaron (§3.6); el evento guarda sus rutas |
| Logs | `$XDG_STATE_HOME/newen/logs/{newen.log, cli-<session>.log}` | `tracing-appender`, rotación diaria, retención N días |
| Transcript del CLI | lo sigue escribiendo el CLI en `~/.claude/projects/…`; Newen lo usa **solo** para `--resume` (vía CLI) e importación | no se parsea en el camino normal |

Tablas (mínimo MVP): `sessions`, `requests`, `engine_inputs`, `engine_input_attempts`, `turns`, `events` (cada línea de stdout excepto `stream_event`/`keep_alive`, con `seq`, `turn_id?`, `type`, `subtype`, `uuid?`, `json` o `json_ref`), `tool_calls`, `permission_requests`, `session_permission_rules`, `command_runs`, `file_effects`, `snapshots`, `rewinds`, `lineage_events`, `worktrees`.

Decisiones:
- **La UI se pinta desde `events` de Newen**, no desde el JSONL: el formato del JSONL es interno del CLI y tiene varias proyecciones incompatibles. Guardar el evento crudo + su proyección permite re-derivar sin pérdida.
- Escritura: un único task escritor por DB (canal `mpsc`) con transacciones por lote (≤16 ms o 256 filas). Los hechos que gobiernan la UI (turno abierto/cerrado, permiso pendiente) se commitean **antes** de emitir el evento a la UI.
- Sin cuentas/organizaciones en el MVP (el CLI maneja la identidad). Columna `profile` reservada.
- Sin secretos persistidos en el MVP; si aparecen (env vars de sesión), crate `keyring` y rechazo si no hay Secret Service.

---

## 8. Git, diff y worktrees

### 8.2 Newen

| Tema | Diseño |
|---|---|
| Backend git | **CLI `git` vía `tokio::process`** para diff, worktrees, merge-base y status (misma semántica que el git del usuario: config, hooks, attributes, LFS). `gix` opcional post-MVP para lecturas calientes (rama, HEAD, ahead/behind). No `git2` (dependencia C, soporte parcial de worktrees). |
| Estado | `GitState{cwd, branch, head_oid, conflicted, ahead, behind, dirty_count}`; recálculo con debounce. |
| Lo sin commitear, en vivo | El motor vuelve a leer lo sin commitear de la carpeta de una sesión por su cuenta, sin que la UI lo pida: tras cada paso que escribió archivos o corrió un comando (`ToolResult` con `files` o `command`), cuando termina una tarea de fondo, al final de cada turno y tras deshacer un pedido. Cada sesión tiene un lector propio (`follow_tree`): deja que la carpeta se asiente 150 ms (una ráfaga de pasos, o varias herramientas a la vez, dan una sola lectura) y lee fuera del hilo del router; lo que llega mientras lee da una lectura más al terminar, así que git nunca corre dos veces a la vez para una sesión ni se acumulan procesos. Lo que pide la UI (`Intent::WorkingTree`, al elegir una sesión o abrir el diff) pasa por el mismo lector. Una lectura es un solo `git diff HEAD -M --raw -p`, partido por archivo en cada línea `diff --git` (un cambio de tipo escribe sus dos partes bajo la misma); solo si las partes no coinciden con la lista (como con un diff externo) se pide un `git diff` por archivo, como antes. Después, `git ls-files --others --exclude-standard -z`: lo que git no sigue ni ignora cuenta como archivo agregado, con el parche que git escribe para uno nuevo. Topes, inferidos de Desktop: se leen los primeros 200, hasta 1 MiB cada uno; uno con un byte NUL es binario (+0 −0, con la línea de git para un binario) y uno más grande queda sin líneas; hasta 2000 líneas lleva su parche entero y uno más largo solo su cuenta, sin hunk; los 200 siguientes se listan sin leer (+0 −0) y el resto queda afuera. Un archivo sacado del índice pero conservado cuenta una vez, como lo da el diff; un repositorio dentro de otro no cuenta. Todo se lee desde la raíz del repositorio, así que una sesión abierta en una de sus carpetas ve lo mismo y con las mismas rutas. Las líneas que cuentan son las de los hunks: una línea agregada `++i;` o una quitada `-- nota` ya no se toman por cabeceras. El historial del pedido (`Shadow::changes`) se parte igual. *Shortcut: no hay watcher de archivos; lo que cambia fuera de un paso (el editor de la persona, un `git commit` en su terminal) se ve en el paso siguiente o al volver a elegir la sesión. Si hace falta, un watcher del gitDir avisaría al mismo lector.* |
| Diff de sesión | Árbol (incl. no rastreados) vs `merge-base(base, HEAD)`, `--numstat -M` + `--name-status`, forma compare-API, parches lazy por archivo, tope inline 5000 líneas. Base: `origin/<source_branch>` si existe, si no la local. Sin merge-base → `base_unknown`. |
| Diff del turno | **Persistido**: `diff(pre_turn_snapshot, post_turn_snapshot)` del repo sombra (§6.4) + `file_effects` por tool. Incluye cambios de Bash. Botón "deshacer turno" = rewind de código al `pre_turn` (no un prompt, a diferencia de). |
| Worktrees (MVP) | Opcional al crear sesión: `git worktree add -b newen/<slug>-<6hex> <repo>/.newen/worktrees/<nombre> <base>`; agregar `.newen/worktrees/` a `.git/info/exclude`; lease por sesión en tabla `worktrees`. Creación anticipada (no perezosa). |
| Guardia | PreToolUse propio con matcher `Edit|Write|MultiEdit|NotebookEdit`: si `file_path` resuelto (canonicalizado) cae fuera del worktree → `{decision:"block", reason}`. |
| `EnterWorktree`/`ExitWorktree` del CLI | seguir el cwd real como *(verificar forma del resultado)*. |
| Limpieza | al archivar: `git status --porcelain` con timeout; si limpio y sin commits sin pushear → `git worktree remove`; si no, conservar y avisar. Nunca `--force` sin confirmación. |
| PR / GitHub | Con "Auto-archive after PR merge" activo (inferido), la dirección del pull request sale de lo último que dijo el agente en un turno y, cada 5 minutos, `gh pr view` pregunta su estado; si está `MERGED`, la sesión se archiva. Solo GitHub, con `gh`. |

---

## 9. Terminal PTY y comandos `!`

### 9.2 Newen

| Tema | Diseño |
|---|---|
| Crate | `newen-pty` con `portable-pty`; un hilo lector por PTY → `tokio::mpsc`. En proceso (no proceso aparte en el MVP); `catch_unwind` + reinicio por PTY. *Shortcut: mismo proceso que la UI; mover a proceso hijo si un PTY puede tumbar la app.* |
| Claves | `(session_id, tab_id)`; `opened_by: user|agent`. Tope 16 PTY global. |
| Shell | `$SHELL -l` en `worktree || cwd`; env login-shell + `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=newen`. |
| Salida | ring buffer 256 KiB por pestaña; coalescido 16 ms hacia la UI (el emulador es de limn, fuera de este documento). Sin scrollback en disco en el MVP. |
| OSC 133 | Scripts de integración **propios** para bash y zsh (mismo mecanismo: rcfile/`ZDOTDIR` que primero carga los dotfiles del usuario). Parser con `vte` (callback `osc_dispatch`). Nonce por PTY para descartar marcas falsificadas; si aparece fuera de marca, dejar de confiar. Produce `CommandRun{origin: user_terminal}` atribuido al turno abierto o al "último turno" con `actor: user`. |
| Comandos `!` | MVP: un PTY oculto por sesión (reusa `newen-pty`) para que `cd`/`export` persistan entre comandos. Recorte por marcas C/D, tope 100 KiB, timeout 30 s sin datos, Ctrl-C → SIGKILL al grupo. Resultado → `CommandRun{origin: user_bang}` + mensaje a la conversación. |
| `!` → modelo | Enviar como `user` con el comando y la salida en un bloque de texto con formato propio de Newen (p. ej. "El usuario ejecutó `cmd` (exit N): …"). Cómo lo hace el CLI nativamente (modo bash de la TUI) *(verificar)*; no copiar etiquetas internas sin confirmar qué espera el modelo. |
| Agente ↔ terminal | Aviso de actividad del usuario al modelo vía `UserPromptSubmit.additionalContext` (MVP). Tools MCP de terminal vía `mcp_message`: post-MVP. |

---

## 10. Crates propuestos

```text
newen-protocol   ← sin IO de procesos; tipos + codec
newen-store      ← SQLite + blobs
newen-git        ← git CLI, repo sombra, watcher
newen-pty        ← PTY + OSC 133
newen-engine     ← orquesta todo; API para la UI (limn)
```

| Crate | Responsabilidad | Tipos principales | Externos |
|---|---|---|---|
| `newen-protocol` | Tipos serde de stream-json y control protocol; codec por línea; fallback `Unknown(Value)`; builders de flags/env (puro, testeable) | `Inbound`, `Outbound`, `SdkMessage{User, Assistant, System(SystemMsg), Result, StreamEvent, …}`, `ControlRequest{subtype…}`, `ControlResponse`, `CanUseTool`, `PermissionResult{Allow{updated_input, updated_permissions}, Deny{message, interrupt}}`, `PermissionUpdate`, `HookInput`, `HookOutput`, `PermissionMode`, `LaunchSpec` | `serde`, `serde_json`, `uuid`, `thiserror` |
| `newen-engine` | Actor por sesión (task tokio dueño del proceso y del estado); `ProcessTransport` (spawn, stdin writer, stdout reader, stderr log, kill de grupo); `ControlMux` (request_id ↔ oneshot, cancelación); `PermissionBroker`; `InputQueue` (§5); `TurnTracker` (§6); `HookHost` (callbacks PreToolUse/PostToolUse/UserPromptSubmit/Stop); `Governor` (tope, idle, crash-loop); `CliLocator` (PATH login-shell, versión); fachada `Engine` para la UI con `EngineEvent` (equivalente a `session_updated`/`message`/`tool_permission_request`…) | `Engine`, `SessionHandle`, `SessionCmd`, `SessionView`, `SessionStatus`, `EngineEvent`, `TurnId`, `RequestId`, `PermId` | `tokio` (process, sync, time, io-util), `tokio-util` (`CancellationToken`), `tracing`, `nix` (setpgid, killpg, signals), `sysinfo` (RAM), `which` |
| `newen-store` | Esquema, migraciones, escritor único por lotes, consultas de UI (timeline de sesión, efectos por request, historial de archivo), blobs por contenido | `Store`, `SessionRow`, `TurnRow`, `ToolCallRow`, `FileEffect`, `EventRow`, `BlobRef` | `rusqlite` (`bundled`), `rusqlite_migration`, `sha2`, `directories` |
| `newen-git` | Wrapper de `git` (timeouts, parseo `--numstat -z`/`--name-status -z`/porcelain v2); diff compare-API; repo sombra (snapshot/restore/diff); worktrees; watcher de `.git` y árbol | `GitRepo`, `GitState`, `SessionDiff`, `DiffFile`, `ShadowRepo`, `SnapshotId`, `WorktreeLease`, `GitWatch` | `tokio::process`, `notify`, `notify-debouncer-full`, `ignore`; (`gix` opcional) |
| `newen-pty` | PTYs por (sesión, pestaña), ring buffer, coalescido, parser OSC 133, integración bash/zsh, PTY oculto para `!` | `PtyManager`, `PtyKey`, `PtyEvent{Data, Exit, CommandStarted, CommandFinished}`, `ShellIntegration`, `BangResult` | `portable-pty`, `vte`, `tokio` |

Dependencias entre crates: `engine → {protocol, store, git, pty}`; `store`, `git`, `pty`, `protocol` no dependen entre sí (salvo ids compartidos, que pueden vivir en `newen-protocol` o en un módulo `ids` de `newen-store`).

Fuera del motor (otros documentos): índice de archivos para menciones `@` (`ignore` + `nucleo`, patrón de), emulador de terminal en limn, MCP propios.

**Pruebas del motor:**
- `newen-protocol`: round-trip de fixtures JSONL capturados de un CLI real (anonimizados) por versión de CLI soportada.
- `newen-engine`: un **CLI falso** (binario de test que lee stdin y responde un guion de stream-json) para cubrir: turno simple, steer antes de primera respuesta, `can_use_tool` allow/deny/abort, `control_cancel_request`, crash antes de `init`, stream sin `result`, crash-loop.
- `newen-git`: snapshots/rewind sobre repos temporales (`tempfile`), incluyendo borrado y rename por "Bash".

---

## 11. Riesgos, desconocidos y verificaciones en ejecución

### 11.1 Riesgos

| Riesgo | Impacto | Mitigación |
|---|---|---|
| El protocolo stream-json/control es el contrato interno entre el SDK TS y el CLI, no una API pública estable | Un update del CLI rompe Newen | Piso y techo de versión probados; fixtures por versión; tipos con fallback `Unknown`; ignorar subtipos desconocidos; tests de contrato en CI contra el CLI instalado. |
| Términos de uso del CLI / identificación del cliente | No hacerse pasar por Desktop | No usar `CLAUDE_CODE_ENTRYPOINT=claude-desktop` ni `client_platform:"desktop_app"`; auth propia del CLI; revisar términos antes de distribuir. |
| Costo de snapshots por tool en repos grandes | Latencia en cada tool (el hook es síncrono para el CLI) | Índice temporal persistente (stat cache), topes por tamaño, desactivar en `$HOME`; medir; hook con timeout y fallback a snapshot solo por turno. |
| Repo sombra y `.gitignore` | Archivos ignorados no se snapshotean (p. ej. `.env`) | Documentar; rewind no los toca; opcional incluir con lista blanca. |
| Rewind propio vs checkpoint del CLI divergen | El modelo cree un estado de archivos distinto | Nota de procedencia en el siguiente `UserPromptSubmit`; un solo mecanismo activo por sesión. |
| Reglas "always" que el CLI escribe en `.claude/settings*.json` | Cambios en el repo del usuario no esperados | Mostrar destino en la tarjeta; preferir sugerencias de destino `session` si existen. |
| PTY en el mismo proceso | Un panic/bloqueo afecta la app | Hilos aislados + `catch_unwind`; plan para proceso aparte. |
| Steering mid-turn | Semántica del CLI al recibir `user` durante un turno (cuándo lo consume) no documentada | Tratar el eco `isReplay` como única confirmación; nunca asumir entrega por escribir en stdin. |

### 11.2 Desconocidos y qué verificar

| # | Qué verificar |
|---|---|
| V1 | Forma exacta de la respuesta de `initialize` (commands, models, account, pid…) |
| V2 | `--session-id=<uuid>` aceptado junto con stream-json input; `system/init.session_id` igual a ese uuid |
| V3 | `--fork-session` + `--resume` devuelve un `session_id` nuevo en `system/init` y dónde queda el JSONL |
| V4 | Algoritmo de "cwd saneado" del directorio `projects/` y comportamiento de `--resume` desde otro cwd |
| V5 | `can_use_tool` para AskUserQuestion: el CLI acepta `updatedInput = {questions, answers}` |
| V6 | Si el CLI usa `decisionClassification` |
| V7 | Valores aceptados de `--effort`, `--thinking`, y de `apply_flag_settings {effortLevel}` |
| V8 | Flags reales para `allowedTools`/`disallowedTools`/`settings` inline y su precedencia |
| V9 | Forma cruda de la respuesta de `rewind_files` y de `interrupt` (`still_queued`/`cancelled`) |
| V10 | Que un `user` escrito con turno activo se consuma como steer, con `priority:"next"` vs sin prioridad, y el comportamiento de `priority:"later"` del lado CLI |
| V11 | Contenido de `tool_response` en PostToolUse de Bash con `bashEditDiffEnabled` |
| V12 | Latencia y timeout del `hook_callback` (cuánto tolera el CLI antes de seguir) |
| V13 | `result.usage` / `total_cost_usd` presentes |
| V14 | Qué valor de `CLAUDE_CODE_ENTRYPOINT` es aceptable y si cambia comportamiento (p. ej. AskUserQuestion, títulos) |
| V15 | Cómo representar un `!` para el modelo (formato del modo bash de la TUI) |
| V16 | Qué borra `delete` (registro, JSONL, `file-history`) |
| V17 | Qué pasa con `control_request` entrantes no soportados (si el CLI espera respuesta o hace timeout) |
| V18 | Retención/limpieza de `file-history/` por el CLI |

### 11.3 Plan de verificación mínimo

1. Script de captura: lanzar el CLI instalado con la línea de §2.4 en un repo temporal, con un cliente mínimo que loguee cada línea (cuenta propia del desarrollador, prompts triviales). Escenarios: turno simple; Edit + Bash con `rm`; permiso allow/deny/always; AskUserQuestion; plan → ExitPlanMode; steer durante un turno; interrupt; `--resume`; `--fork-session`; `rewind_files` dry-run.
2. Guardar las capturas como fixtures versionados por versión de CLI (sin datos personales).
3. Cerrar V1–V13 antes de congelar `newen-protocol` v1.

### 11.4 Resultados de la captura (2026-10-25, Claude Code 2.1.310)

Capturas con `tools/capture` (`--model haiku`, `--setting-sources=project,local`), en un repo git temporal por escenario. Fixtures anonimizados en `crates/newen-agent-claude/fixtures/2.1.310/` (se regeneran con `scripts/fixtures.mjs`). `[F:<fixture>]` cita el fixture.

| # | Resultado |
|---|---|
| V1 | **Verificado.** La respuesta de `initialize` trae `account{apiProvider, email, organization, subscriptionType}`, `agents[]`, `available_output_styles`, `capabilities`, `claude_code_version`, `commands[]`, `current_permission_mode`, `fast_mode_state`, `hooks_applied`, `models[{resolvedModel, displayName, description, supportedEffortLevels}]`, `pid`, `session_state`, `remote_control_*`, y en el sobre `pending_permission_requests[]` y `pending_user_dialog_requests[]`. **Los selectores de modelo y esfuerzo se arman desde `models`.** |
| V2 | Pendiente (no se probó `--session-id`). El `session_id` sale de `system/init`. |
| V3 | Parcial. `--resume=<id> --fork-session` corre bien y conserva el contexto; falta comparar el `session_id` nuevo. |
| V4 | **Verificado.** `--resume` funciona desde otro cwd. El directorio de proyecto del CLI es la ruta con cada `/` reemplazado por `-` (`~/.claude/projects/-work/`). |
| V5 | **Verificado.** AskUserQuestion llega como `can_use_tool` con `requires_user_interaction:true`; se responde `allow` con `updatedInput = input + answers{<pregunta>: <label>}` y el modelo recibe "The user answered…". |
| V6 | No apareció `decisionClassification`. Sí aparece `decision_reason_type` (p. ej. `subcommandResults`) y `blocked_path`. |
| V7 | **Verificado** para esfuerzo: cada modelo declara `supportedEffortLevels` (`low…max`). El flag `--effort` no se probó. |
| V8 | Pendiente. |
| V9 | **Verificado.** `rewind_files` con `dry_run:true` responde, crudo, `{canRewind, filesChanged[], insertions, deletions}`. `interrupt` responde `{still_queued: []}`. |
| V10 | **Aclarado.** Un `user` con `priority:"next"` enviado durante un turno sin herramientas **no** se inyecta: corre como turno siguiente, con su propio `system/init` y su `result`. Falta probar el steer entre herramientas. |
| V11 | **Mejor de lo esperado.** Cada `user` con `tool_result` trae `tool_use_result` estructurado: Write `{filePath, content, originalFile, structuredPatch}`, Edit `{filePath, oldString, newString, originalFile, structuredPatch}`, Bash `{stdout, stderr, interrupted, noOutputExpected}`. Para Write y Edit la atribución sale del protocolo; el snapshot del repo sombra hace falta solo para Bash y otras herramientas opacas. |
| V12 | `hook_callback` PostToolUse llega 15–120 ms después de la herramienta y el CLI espera la respuesta antes del `tool_result`. Su input trae `prompt_id`, `permission_mode`, `effort{level}`, `duration_ms`, `tool_name`, `tool_input`, `tool_response`, `session_id` y `cwd`. **`prompt_id` liga cada herramienta a su petición.** |
| V13 | **Verificado.** `result` trae `total_cost_usd`, `usage{input_tokens, output_tokens, cache_*…}`, `num_turns` y `duration_ms`. Un turno interrumpido termina en `result/error_during_execution` con `is_error:true` y costo 0, y el CLI sale con código 1 al cerrar stdin. |

Otros hallazgos de la captura:

- **Mensajes nuevos.** `command_lifecycle`; `system/status{status:"requesting", user_message_uuids[]}` (qué petición está corriendo); `system/thinking_tokens{estimated_tokens, user_message_uuid}`; `system/status{permissionMode}` cuando cambia el modo. `system/init` se emite de nuevo en cada turno.
- **Capacidades del CLI.** `system/init.capabilities` declara funciones opcionales (`interrupt_receipt_v1`, `interrupt_cancel_queued_v1`, `interrupt_send_now_v1`, `interrupt_requeue_taken_v1`, `msg_lifecycle_v1`, `request_marker_lists_v1`…). El adaptador las mapea a `AgentCapabilities`.
- **`permission_suggestions` mezcla reglas y modos.** Por ejemplo `[{type:"addRules", behavior:"allow", rules:[{toolName, ruleContent}], destination:"localSettings"}, {type:"addDirectories", directories[], destination:"session"}, {type:"setMode", mode:"acceptEdits", destination:"session"}]`. Si se devuelven todas en `updatedPermissions`, la sesión cambia de modo (`[F:always]`). **"Always allow" de Newen debe devolver solo las reglas**; el cambio de modo es otra acción.
- **Comandos seguros.** El CLI no pide permiso para comandos de solo lectura como `echo`. Las reglas de la configuración del usuario también evitan pedidos, así que las capturas corren sin `user` en `--setting-sources`.
- **Plan mode.** `ExitPlanMode` llega como `can_use_tool` con `input.plan`; al aprobarlo el modo vuelve a `default`. El plan se escribe en `~/.claude/plans/`, fuera del repo.
- **Datos personales en el protocolo.** `initialize` devuelve email y organización, y `system/init` incluye rutas del home. Los fixtures se anonimizan siempre (`scripts/fixtures.mjs` falla si queda algo).
- **Sugerencia del siguiente prompt** (2026-10-31, Claude Code 2.1.316, escenario `suggest` de `tools/capture`, `[F:suggest]` en `fixtures/2.1.316/`). Con `promptSuggestions: true` en `initialize`, el CLI manda `{type:"prompt_suggestion", suggestion, uuid, session_id}` unos 1,1 s después del `result` del segundo turno ("add a docstring" tras pedir una función de una línea). Tras el primer turno no manda ninguna, y la respuesta de `initialize` no cambia. En una captura anterior, con dos preguntas triviales, tampoco llegó tras el segundo turno *(inferido: el CLI calla cuando no tiene nada que valga sugerir)*.

## 12. Logos de conectores y plugins

Newen no trae ningún logo: los de los conectores (servidores MCP) y los plugins se buscan en la red la primera vez que la interfaz los pide (`Intent::Logos`, al abrir Connectors, Plugins o el diálogo `/mcp`), y quedan en la carpeta de datos de la persona, nunca en la sesión (`Event::Logo` no se guarda en el registro).

- **Búsqueda, en `newen-engine/src/logos.rs`, en un hilo bloqueante por logo.** Primero svgl.app por nombre: `GET https://api.svgl.app?search=<nombre>`. Se toma la entrada cuyo `title` es igual al nombre sin distinguir mayúsculas (sin coincidencias parciales: "Google" no sirve para "Google Drive"). Su `route` es una dirección o `{light, dark}`; el tema claro usa `light` y el oscuro `dark`. Un error o una lista sin ese título no dan nada.
- **Si svgl no tiene el nombre, el sitio del servidor.** Con una dirección `https://`, se toma el host sin una etiqueta inicial `mcp.` o `api.` (`mcp.context7.com` → `context7.com`) y se prueba `https://<dominio>/apple-touch-icon.png`, luego `/favicon.ico`. Se acepta PNG o ICO, que `image` decodifica.
- **Dibujo.** Los SVG se rasterizan con `resvg` (sin texto ni fuentes, sin recursos externos) y las imágenes se escalan; ambos quedan en un PNG de 64×64, centrado y transparente.
- **Límites.** Solo `https`, diez segundos por respuesta y 512 KB como máximo. Una imagen que decodifica a más de 2048 px o 64 MB se descarta. Las respuestas se tratan como datos no confiables: nada se ejecuta ni se sigue fuera del archivo.
- **Caché**, en `$XDG_DATA_HOME/newen/logos/` (`Config::logos`): `<clave>-light.png` y `<clave>-dark.png` (el mismo archivo dos veces si el logo es uno solo). Una búsqueda sin resultado deja `<clave>.none`, y se vuelve a buscar pasados siete días. Un logo guardado se responde desde disco, sin red.
- **Apagado.** El ajuste `logos` (Settings › General) está encendido por defecto; apagado, la interfaz no pide ninguno. Las pruebas no usan red: `Config::logos` es `None` en ellas.
