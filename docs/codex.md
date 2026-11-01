# Codex en quill (`quill-agent-codex`)

Plan para sumar el CLI de OpenAI Codex como segundo agente, con la misma interfaz de quill. Todo lo de Codex vive en `crates/quill-agent-codex`: el código genérico no lo nombra (AGENTS.md).

## Fuente de verdad

- **Código abierto:** [`openai/codex`](https://github.com/openai/codex), Apache-2.0, en Rust. El protocolo se lee del propio CLI con `codex app-server generate-json-schema --out <dir>`, que escribe el contrato completo (105 pedidos del cliente, 85 notificaciones, 10 pedidos del servidor en `codex-cli 0.163.0`). Las capturas de la fase 1 confirman cómo se usa.
- **Versión probada:** `codex-cli 0.163.0`. La última release es la 0.165.0. `app-server` figura como `[experimental]` en el `--help`, así que el adaptador declara su rango en `tested()` y quill avisa fuera de él, como con Claude.
- **Observado el 2026-10-28** con `initialize` + `model/list` + `account/read`:
  - la respuesta de `initialize` trae `userAgent`, `codexHome`, `platformFamily` y `platformOs`;
  - `model/list` devuelve 8 modelos, cada uno con sus esfuerzos (`low` … `max`, y `ultra` en algunos) y el esfuerzo por defecto;
  - la cuenta es `chatgpt`, plan `plus`.
- quill se presenta como `clientInfo { name: "quill" }`, nunca como otro cliente.

## Transporte

Se usa `codex app-server` (JSON-RPC por stdio, una línea por mensaje), el que usa la extensión de VS Code. Encaja en el contrato del adaptador sin cambios: `encode` escribe pedidos con `id` y respuestas a pedidos del servidor, y `decode` lee respuestas, notificaciones y pedidos del servidor. El adaptador guarda qué `id` espera cada respuesta.

Se descartan:
- `codex exec --json`: corre un solo turno y no tiene permisos interactivos.
- `codex mcp-server`: no expone el transcript.

Un proceso por sesión de quill, con un thread cada uno, igual que con Claude.

## Mapa del contrato

| quill (`AgentCommand` / `AgentEvent`) | Codex app-server |
|---|---|
| `Start` → `Ready` + `Session{id}` | `initialize` → `initialized`. Después `thread/start {cwd, model, approvalPolicy, sandbox}`, o `thread/resume {threadId}`, o `thread/fork`. El `thread.id` es el id de sesión. Las capacidades salen de `model/list` |
| `Prompt` | `turn/start {threadId, input:[{type:"text",text}], model, effort, approvalPolicy, sandboxPolicy}`. Con un turno en curso, `turn/steer` (`capabilities.steer`) |
| `Interrupt` | `turn/interrupt {threadId, turnId}` → `turn/completed` interrumpido |
| `TextDelta` / `Text` | `item/agentMessage/delta`, y `item/completed` con `agentMessage` |
| `ThinkingDelta` / `ThinkingText` | `item/reasoning/summaryTextDelta`, `item/reasoning/textDelta` |
| `ToolUse` / `ToolResult` | `item/started` / `item/completed` según el tipo de ítem (ver abajo). La salida en vivo llega con `item/commandExecution/outputDelta` |
| `PermissionRequest` → `Decide` | Pedidos del servidor `item/commandExecution/requestApproval` y `item/fileChange/requestApproval`. Se contestan con `accept`, `acceptForSession`, `decline` o `cancel` |
| Pregunta al usuario (tarjeta de AskUserQuestion) | `item/tool/requestUserInput`: preguntas con `id`, `header`, `question`, `options{label, description}`, `isOther` e `isSecret`, la misma forma que la tarjeta de quill. Se contesta `{answers:{<id>:{answers:[<label>]}}}`. Solo existe en modo Plan o con la feature `default_mode_request_user_input`. Texto libre, secretos, solo texto y `isBlocking`: ver "Preguntas" |
| `TurnFinished` | `turn/completed` |
| `Tokens` / `ContextUsage` | `thread/tokenUsage/updated`: lo usado sobre la ventana del modelo, sin desglose por categoría |
| `Limits` | `account/rateLimits/updated` / `account/rateLimits/read`: ventana primaria y semanal, igual que la tarjeta Usage |
| `TaskStarted` / `TaskEnded` (subagentes) | `collabAgentToolCall`, `subAgentActivity` |
| `McpServers` / `McpStatus` | `mcpServerStatus/list`, `mcpServer/startupStatus/updated` |
| `ModeChanged` | Ningún pedido cambia el modo de un thread: viaja en cada `turn/start`. `SetMode` lo cuenta con la respuesta de un `account/read` (id `quill-mode`). `thread/settings/updated` solo repite los ajustes que mandó un `turn/start` y se ignora (ver "Modo Plan") |
| `AuthRequired` | `account/read` con `account: null` y `requiresOpenaiAuth: true`, o `turn/completed` fallido con `codexErrorInfo: unauthorized` (inferido de la fuente, ver Fase 1) |
| Desconocido | Se guarda como `Unknown` y nunca rompe (AGENTS.md) |

### Ítems → las herramientas que quill ya dibuja

El adaptador traduce cada ítem al nombre de herramienta que la interfaz ya conoce. Así se reusan tal cual las filas, los cuerpos desplegados, los diffs, la búsqueda web y la lista de tareas, sin pantallas nuevas.

| Ítem de Codex | Se muestra como | Notas |
|---|---|---|
| `commandExecution` | `Bash` (`command`, `description`) | `commandActions` clasifica el comando. `read` pasa a `Read`, `search` a `Grep`, `listFiles` a `LS`, y el resto queda en `Bash` |
| `fileChange` | `Edit` / `Write` por archivo (`add` → creado, `update` → editado, `delete`) | Codex da un diff unificado. `add` trae el contenido completo como `diff`; `update` trae solo el hunk. El antes y el después de `ToolResult.files` se reconstruyen con el repo sombra del motor (confirmado, ver Fase 1) |
| `mcpToolCall` | `mcp__{server}__{tool}` | Etiqueta y logo del conector como hoy; las imágenes de su `result.content` (bloques de imagen de MCP, *inferido*) van al final de la fila, como las de Claude (engine.md §3.6) |
| `webSearch` | `WebSearch` con `WebSearchRun` | El bloque de búsqueda web actual |
| `turn/plan/updated` | La lista de tareas (forma de `TodoWrite`) | El panel de tareas sobre el composer. Es la notificación de `update_plan`, que el thread debe activar (ver Fase 1); el ítem `plan` es otra cosa (fila siguiente) |
| `plan` (modo Plan) | `ExitPlanMode` con `{plan}` | El plan propuesto: "Proposed plan ›", que lo abre en su panel, y la tarjeta para implementarlo al final del turno (ver "Modo Plan") |
| `subAgentActivity` (`started`) / `collabAgentToolCall` `spawnAgent` | `Agent` + `TaskStarted` | El mismo `Agent` de Claude: la fila "Ran agent", el panel del subagente y el panel de tareas. Ver "Ítems restantes" |
| `subAgentActivity` (`completed`) | `TaskEnded` + el `ToolResult` del `Agent` | El resumen es el último mensaje del hilo del subagente |
| `collabAgentToolCall` (`wait`, `sendInput`, …) | `WaitAgent`, `MessageAgent`, `CloseAgent` | "Waited for agents", "Messaged an agent", "Closed an agent" |
| `imageView` | `Read` de una imagen | La fila "Read logo.png" y, al abrirla, la imagen |
| `imageGeneration` | `ImageGeneration` | La imagen en su propio bloque, bajo una línea que abre el prompt revisado |
| `contextCompaction` | `Compact` | Una línea "Context compacted" entre dos filetes |
| `enteredReviewMode` / `exitedReviewMode` | `EnterReviewMode` / `ExitReviewMode` | "Started a review of current changes" / "Finished the review", con los hallazgos al abrirla |
| `sleep` | `Sleep` | "Waited 1s" |
| `dynamicToolCall` | La herramienta con su propio nombre | "Used {tool}" (inferido: quill no declara herramientas dinámicas) |

## Modos de permiso

Codex no tiene un modo único sino dos ejes: `approvalPolicy` (`untrusted`, `on-request`, `never`) y `sandbox` (`read-only`, `workspace-write`, `danger-full-access`). El adaptador los ofrece como tres modos, los mismos presets que muestra Codex:

| Modo en quill | `approvalPolicy` | `sandbox` |
|---|---|---|
| Read only | `untrusted` (pide aprobación antes de cualquier cosa que no sea leer) | `read-only` |
| Auto (por defecto) | `on-request` | `workspace-write` |
| Plan (el modo Plan de Codex, ver "Modo Plan") | `on-request` | `read-only` |
| Full access (inseguro, en `unsafe_modes`) | `never` | `danger-full-access` |

El selector del composer ya se arma desde `AgentCapabilities`. La exención del servidor de desarrollo hoy mira los nombres de Claude (`bypassPermissions`, `auto`); con Codex pasa a ser una pregunta al adaptador ("¿este modo exime?"), para que el motor no nombre modos de ningún proveedor.

## Cuenta

Para la v1 se reusa el camino del motor (`auth.rs`) con los subcomandos:
- `codex login status` → `AuthState`;
- `codex login` (navegador, con la vuelta en localhost a cargo de Codex) → `login_url` lee la URL que imprime;
- `codex logout`.

Métodos de inicio de sesión: "ChatGPT account" (`codex login`) y "Device code" (`codex login --device-auth`), si su salida se puede leer (resuelto en la fase 2: solo el primero, ver "Fases 2 y 3"). La clave de API queda fuera de la v1, porque necesita un campo seguro. La salida exacta de los tres comandos se verifica en la fase 1, solo con `status` contra la configuración real.

Mejora posterior: el login por el protocolo (`account/login/start` → `authUrl`, `account/login/completed`).

## Lo que Codex no tiene o se degrada

Se declara en `AgentCapabilities` y la interfaz lo muestra degradado, como dice `agents.md`:
- **Atribución por herramienta con hooks:** se hace por turno con el repo sombra. `fileChange` ya trae los archivos.
- **Desglose del contexto por categoría:** solo el total.
- **Rewind de archivos del CLI:** Codex no restaura archivos (`capabilities.rewind` queda en false); los restaura el motor con sus instantáneas. La conversación sí se puede devolver, ver «Rewind» abajo.
- **Comandos `/`:** Codex tiene skills y prompts propios (`skills/list`). Se muestran en el menú `/` en una fase posterior.
- **Sugerencia del siguiente prompt:** no la tiene. El app-server declara como experimentales `thread/prediction/request` y `thread/prediction/updated`, pero la 0.163.0 contesta el pedido con "thread/prediction/request is not implemented yet" (`codex-rs/app-server/src/message_processor.rs`). El adaptador no ofrece la opción ni manda `PromptSuggestion`.

## Elegir el agente

Con dos agentes registrados, la página de sesión nueva elige uno con un chip junto a "Local" / carpeta (logo y nombre, y un menú limn). Es lo único visual nuevo; el resto reusa lo que hay. Hecho en la fase 5, ver abajo.

## Fases

1. **Capturas.** `tools/capture` suma `--agent codex`: un driver JSON-RPC que graba `{t_ms, dir, msg}` como el de Claude. Cada escenario corre en un repo git nuevo, con `gpt-6-luna` y esfuerzo `low`, con la cuenta de la persona. Escenarios:
   - simple, comando + edición con aprobación, denegar, aceptar por sesión;
   - plan/tareas, steer, interrumpir, reanudar, fork;
   - razonamiento, búsqueda web, una herramienta MCP, un subagente;
   - pregunta al usuario, modelos y cuenta.

   Las capturas se anonimizan (`/work/`, email) y quedan en `crates/quill-agent-codex/fixtures/0.163.0/`. Codex guarda esos threads en `~/.codex/sessions` como cualquier sesión suya.
2. **Adaptador** `quill-agent-codex`: `launch`, `encode` y `decode`, capacidades, la traducción de ítems y los modos. Tests que reproducen cada captura, como los de Claude.
3. **Motor:** registrar `codex` y hacer que la exención de modo sea del adaptador. El agente falso ya reproduce cualquier captura (no depende del protocolo).
4. **Cuenta:** `login status` / `login` / `logout` con el agente falso para los tests. (Hecho.)
5. **Interfaz:** el chip para elegir agente, las palabras de los modos desde el adaptador, el nombre "Codex" y la verificación en pantalla de cada escenario, en tema claro y oscuro. (Hecho.)
6. **Después:** el login por el protocolo, la historia de sesiones (`thread/list`), skills en el menú `/`, y los diseños de review, imágenes y herramientas dinámicas.

6. **Después:** el login por el protocolo, la historia de sesiones (`thread/list`), skills en el menú `/`, `thread/revert`. Los diseños de review, imágenes, compactación y subagentes están hechos (ver "Ítems restantes").

## Fase 1: lo observado

Capturas de `codex-cli 0.163.0` (2026-10-28) con `quill-capture --agent codex <escenario>`, modelo `gpt-6-luna`, esfuerzo `low`, cuenta ChatGPT plus. Los fixtures están en `crates/quill-agent-codex/fixtures/0.163.0/<escenario>.jsonl` (`archivo:línea` abajo). Se gastaron 25 turnos (más 4 de `rewind`, abajo). El formato es el de Claude: `{t_ms, dir, msg}` con `dir` = `out`, `in`, `stderr` o `exit`. Los mensajes JSON-RPC no llevan `"jsonrpc"`. Las claves de cada línea están ordenadas alfabéticamente por la anonimización, no en el orden del cable.

### Escenarios

| Escenario | Resultado |
|---|---|
| `simple`, `edit-bash`, `deny-patch`, `deny`, `always`, `steer`, `interrupt`, `resume`, `fork`, `websearch`, `ask`, `plan`, `account` | Comportamiento buscado |
| `think` | **No emitió razonamiento.** Con `gpt-6-luna` en `low` y `summary: "detailed"` no hubo ítems `reasoning` ni `item/reasoning/*` (dos intentos). Las formas de `summaryTextDelta` / `textDelta` salen solo del esquema. Hace falta repetirlo en un esfuerzo mayor o con otro modelo |
| MCP y subagente | Omitidos: no hay un servidor MCP trivial ni un modo de subagente sin configuración extra. `account` sí registra `mcpServerStatus/list` |

Cómo se llegó a cada uno:
- `deny-patch` es el rechazo de un `apply_patch` (`fileChange`). `deny` y `always` son de comandos y usan `approvalPolicy: untrusted`.
- `plan` y `ask` fijan una `config` por thread en `thread/start` (no tocan `~/.codex/config.toml`): `{"tools":{"update_plan":{"enabled":true}}}` y `{"features":{"default_mode_request_user_input":true}}`.
- `account` está recortado: `mcpServerStatus/list` trae las herramientas de todos los conectores (4 MB), así que quedan 2 herramientas y 1 recurso por servidor.
- Las instrucciones globales de la persona (`~/.codex/AGENTS.md` → `rtk …`) se cuelan en los comandos y en las `instructionSources` (`edit-bash.jsonl:54`). Es ruido de la cuenta, no del protocolo.

### Preguntas del plan

1. **`item/started` de un comando, ¿antes o después de su pedido de aprobación?** Antes. El ítem llega `inProgress`, después el pedido del servidor, la respuesta, `serverRequest/resolved` y por último `item/completed` (`deny.jsonl:26` started, `:27` requestApproval, `:28` respuesta, `:29` resolved, `:30` completed con `status: "declined"`). Igual con `fileChange` (`edit-bash.jsonl:58` started, `:60` requestApproval). `serverRequest/resolved` llega antes que `item/completed`.
2. **`fileChange`: ¿diff o contenido completo?** Solo diff por archivo, sin el contenido de antes. `changes[{path, kind, diff}]`: con `kind.type: "add"` el `diff` es el contenido entero del archivo nuevo (`"hello\n"`, `edit-bash.jsonl:58`); con `update` (`kind.move_path` puede ser null) es un hunk sin cabecera (`"@@ -1 +1 @@\n-hello\n+hello world\n"`, `:69`). El antes y el después hay que reconstruirlos con el repo sombra. Además `turn/diff/updated` manda el diff git acumulado de todo el turno, con cabecera `diff --git` (`:65`, `:76`).
3. **`turn/steer`.** Pedido `{threadId, expectedTurnId, input}` y respuesta `{turnId}` (el mismo turno, `steer.jsonl:30` y `:32`). No hay `turn/started` nuevo: llega otro ítem `userMessage` con el texto del steer dentro del mismo turno (`:47`, `:48`) y el modelo lo acata (`1..5`).
4. **`thread/resume`.** La respuesta repite la historia: `thread.turns` trae el turno anterior con `itemsView: "full"` y los ítems completos, y `initialTurnsPage` es null (`resume.jsonl:52`). Antes manda `deprecationNotice` ("Full-history hydration is deprecated for paginated threads; use `excludeTurns: true`, then page with `thread/turns/list`", `resume.jsonl:41`). Con `excludeTurns: true` más `thread/turns/list {threadId}` la historia llega paginada (`data`, `nextCursor`, `backwardsCursor`), pero cada turno viene con `itemsView: "summary"` (el `userMessage` y la respuesta final; no se vio si incluye comandos, porque el turno no tenía herramientas, `resume.jsonl:56`). Para la historia con herramientas hay `thread/items/list` (no probado). `thread/fork` devuelve la misma forma, con `forkedFromId` y los turnos completos (`fork.jsonl:42`), y emite `thread/started` del thread nuevo (`:45`).
5. **Un turno sin cuenta.** No se probó (no se cerró la sesión). Por el esquema y la fuente: `account/read` devuelve `account: null` y `requiresOpenaiAuth: true` (con cuenta, `requiresOpenaiAuth` también es true y `account` viene lleno, `account.jsonl:9`; así que la señal es `account == null`). Un turno falla con una notificación `error` cuyo `codexErrorInfo` puede ser `unauthorized` y un `turn/completed` con `status: "failed"` y `turn.error`; el esquema de `TurnStatus` es `completed | interrupted | failed | inProgress`. Es inferido; confirmarlo en la fase 4 con una `CODEX_HOME` vacía, sin tocar la sesión real.
6. **`codex login status`.** Escribe en **stderr** (stdout vacío): `Logged in using ChatGPT` y sale 0. Sin sesión, la fuente imprime `Not logged in` y sale 1 (`codex-rs/cli/src/login.rs`, `run_login_status`). Otras variantes: `API key configured - <clave recortada>`, `Logged in using workload identity`. No incluye el email: el email sale de `account/read`.
7. **`codex login` / `--device-auth`.** No se ejecutaron. `codex login --help` y `codex login --device-auth --help` imprimen la misma ayuda (`--device-auth` es un flag de `login`, sin ayuda propia); opciones: `--with-api-key` y `--with-access-token` (leen de stdin), `--device-auth`, `-c`, `--enable`, `--disable`, subcomando `status`. Por la fuente, el login por navegador imprime en **stderr** `Starting local login server on http://localhost:<puerto>.` / `If your browser did not open, navigate to this URL to authenticate:` / la URL / `On a remote or headless machine? Use \`codex login --device-auth\` instead.`, y al terminar `LOGIN_SUCCESS_MESSAGE`. `login_url` debe leer la línea que sigue a esa frase. Falta ver la salida real del código de dispositivo.
8. **`codex logout --help`.** `Remove stored authentication credentials`, sin argumentos propios más allá de `-c`, `--enable`, `--disable`. No se ejecutó `logout`.

### Forma exacta de las aprobaciones

- **Comando**, pedido del servidor `item/commandExecution/requestApproval` (`deny.jsonl:27`): `params {threadId, turnId, itemId, command, cwd, commandActions, availableDecisions, proposedExecpolicyAmendment, kind: "command", environmentId, startedAtMs}`. `availableDecisions` fue `["accept", {"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["touch","denied.txt"]}}, "cancel"]`: **no ofrece `acceptForSession` ni `decline`**. Aun así `decline` funcionó (`status: "declined"`, `deny.jsonl:30`; stderr: `rejected by user`) y `acceptForSession` también: no hubo segundo pedido para el mismo comando (`always.jsonl:43` pedido, `:44` respuesta, y el segundo `item/started` sin pedido). El adaptador no debería limitarse a `availableDecisions`.
- **Archivo**, `item/fileChange/requestApproval` (`edit-bash.jsonl:60`): `params {threadId, turnId, itemId, reason, grantRoot, startedAtMs}`. El `itemId` es el del ítem `fileChange`. Rechazado, el ítem termina `status: "declined"` y el modelo sigue con el turno (`deny-patch.jsonl:64`); el turno cierra `completed` (`:100`).
- **Respuesta** a ambos: `{"id": <mismo id>, "result": {"decision": "accept" | "acceptForSession" | "decline" | "cancel"}}`. Los `id` de los pedidos del servidor son enteros desde 0, aparte de los del cliente. `cancel` además interrumpe el turno (esquema).
- **Pregunta**, `item/tool/requestUserInput` (`ask.jsonl:30`): `params {threadId, turnId, itemId, questions[{id, header, question, options[{label, description}], isOther, isSecret}], isBlocking: false, autoResolutionMs: null}`. Respuesta `{"result":{"answers":{"color_choice":{"answers":["Red"]}}}}` (`:31`). No hay ítem aparte para la pregunta: solo el pedido, y `serverRequest/resolved` (`:32`). El modelo la usó con `gpt-6-luna`, pero **solo con la feature activada** (la CLI avisa con `warning` en `ask.jsonl:9`).
- `item/permissions/requestApproval` y `mcpServer/elicitation/request` no aparecieron.

### Otros hallazgos que cambian el diseño

- **`commandActions`** viaja en el ítem `commandExecution` (y en el pedido): `{type: "read", command, name, path}` para `cat <archivo>` simple (`deny-patch.jsonl:53`), `{type: "listFiles", command, path}` para `ls` (`plan.jsonl:69`), y `{type: "unknown", command}` para el resto: `touch`, `pwd`, y también `cat … && pwd` o `rtk cat …` (`edit-bash.jsonl:54`, `:80`). `search` no apareció. El comando completo viene envuelto: `/run/current-system/sw/bin/bash -lc '<cmd>'`; el texto útil está en `commandActions[].command`.
- **Aprobaciones y el modo Read only.** Con `on-request` + `read-only` Codex **no** pide aprobación para un comando que el sandbox bloquea: el modelo recibe el fallo y lo cuenta. Solo `apply_patch` pide aprobación (`edit-bash`, `deny-patch`). Para ver aprobaciones de comandos hubo que usar `untrusted`. Por eso "Read only" usa `untrusted`: pregunta antes de correr cualquier cosa que no sea leer.
- **Resume, fork y los valores por defecto del usuario.** `thread/resume` y `thread/fork` **sin** `approvalPolicy` / `sandbox` volvieron con `sandbox: {type: "dangerFullAccess"}`, tomado de la configuración de la persona (primer intento, descartado y no incluido). El adaptador debe mandar `approvalPolicy` y `sandbox` en `thread/start`, `thread/resume` y `thread/fork`, siempre. Lo mismo vale para el esfuerzo: `thread/start` respondió `reasoningEffort: "max"` (el de la persona), y el `effort` sale en cada `turn/start`.
- **`update_plan` está apagado por defecto.** Sin config, el modelo dice que no existe la herramienta (dos intentos). Con `tools.update_plan.enabled` en la `config` del thread llega `turn/plan/updated` `{turnId, explanation, plan[{step, status}]}` con `status` `pending | inProgress | completed`; la primera notificación llega antes de los comandos (`plan.jsonl:52`, `:59`, `:66`, `:73`). Es la lista completa en cada notificación.
- **Razonamiento.** Ver `think`: con `gpt-6-luna` en `low` no hay resumen. Además, `turn/start.summary` hay que mandarlo (`auto` | `concise` | `detailed`) para tenerlos, y `model/list` da los esfuerzos válidos por modelo (`low` … `max`; `gpt-6-luna` ofrece `low, medium, high, xhigh, max`, `account.jsonl:7`). `model/list` vino en una sola página (`nextCursor: null`), 8 modelos.
- **Orden del arranque de un turno.** La respuesta de `turn/start` (`{turn:{id, status:"inProgress", items:[], itemsView:"notLoaded"}}`, `simple.jsonl:15`) llega antes que `turn/started` (`:20`). Los ítems del turno vienen con `item/started` / `item/completed` y `startedAtMs`; el primero es siempre el `userMessage`. Los `agentMessage` traen `phase`: `commentary` (los avisos del modelo entre herramientas) o `final_answer`. `item/agentMessage/delta` da `{delta, itemId}`.
- **`turn/interrupt`.** Pedido `{threadId, turnId}`, respuesta `{}`, y `turn/completed` con `status: "interrupted"` y `items: []` (`interrupt.jsonl:30`, `:34`, `:36`). `turn/completed.turn.items` siempre viene vacío (`itemsView: "notLoaded"`): los ítems se arman con `item/*`.
- **`webSearch`.** `item/started` con `query: ""` y `action: null`; `item/completed` con `query`, `action {type: "search", query, queries}` y `results[{title, url, snippet, domain, ref_id, type: "text_result"}]` (`websearch.jsonl:44`, `:45`). Funcionó con la búsqueda cacheada que viene por defecto.
- **Uso de tokens.** `thread/tokenUsage/updated {tokenUsage:{last, total, modelContextWindow}}`, cada uno con `inputTokens`, `cachedInputTokens`, `outputTokens`, `reasoningOutputTokens`, `totalTokens` (`simple.jsonl:31`; ventana de 258400 para `gpt-6-luna`).
- **Ruido que el adaptador debe tolerar** (queda como `Unknown` o se ignora): `remoteControl/status/changed` (salvo en el proceso compartido de Remote Control, [remote-control.md](remote-control.md) §3), `mcpServer/startupStatus/updated` (uno por servidor, antes y durante el primer turno), `account/updated`, `account/rateLimits/updated`, `thread/status/changed`, `thread/goal/cleared`, `hook/started` / `hook/completed` (hooks de la configuración de la persona, `interrupt.jsonl`), `warning`, `deprecationNotice`, y `emittedAtMs` en casi toda notificación.
- **Cuenta.** `account/read` → `{account:{type:"chatgpt", email, planType}, requiresOpenaiAuth, workspaceRouting{...}}`; `account/rateLimits/read` → `rateLimits{primary{usedPercent, windowDurationMins:300, resetsAt}, secondary{…10080…}, credits, planType}` (`account.jsonl:9`, `:11`). El CLI sale con código 0 al cerrar stdin (`exit` en cada fixture).
- **Repetir un proceso** (`resume`): el fixture es un solo archivo con el `exit` del primer proceso a mitad, seguido de otro `initialize` con `id: 1` (el cliente reinicia su contador). El agente falso ignora el `exit`, así que el adaptador de la fase 2 tiene que volver a mandar `initialize` en la misma conversación para reproducirlo.

### Pendiente

- Razonamiento en vivo (otro modelo o esfuerzo), `thread/items/list`, `search` en `commandActions`, `item/permissions/requestApproval` y MCP. (Subagentes: ver "Ítems restantes". Un turno sin cuenta: solo sintético.)
- El esquema usa `cancel` y `acceptWithExecpolicyAmendment`: no se probaron.

## Fases 2 y 3: lo que cambió respecto del plan

El adaptador está en `crates/quill-agent-codex` (`Codex`, con `tested() = ["0.163.0"]`, exacto como el de Claude). Cada test reproduce un fixture: lo que el adaptador escribe tiene que ser el mismo número de líneas y los mismos métodos, ids y respuestas que los `out` de la captura, y lo que decodifica no deja ningún `Unknown`. El motor registra `codex` junto a `claude` y hay un test del motor que reproduce `simple` y `deny` con el agente falso.

- **Arranque en cuatro tiempos.** `encode(Start)` escribe solo `initialize`. Con su respuesta, `decode` devuelve en `replies` `initialized`, `model/list` y `thread/start` | `thread/resume` | `thread/fork` (`SessionSpec.resume` / `fork`). Así no se manda nada al servidor antes de que se inicialice. El contador de ids vuelve a 1 en cada `Start` (el `resume` tiene dos procesos). El `SessionSpec` entra por `launch` (que guarda una copia): es la única forma de que `Start` sepa el cwd, el modelo y la sesión a reanudar.
- **`model/list` va con el id de texto `"quill-models"`**, para no correr los números de las demás líneas. Las capturas no lo tenían (salvo `account`): se agregó a cada una después de `initialized`, con la respuesta real de `account.jsonl:7` bajo ese id. Es la única línea sintética de las fixtures. Así el agente falso no necesita casos especiales y en pantalla aparecen los 8 modelos. Mientras no llega la respuesta, `Ready` trae el modelo del thread con los esfuerzos `low`…`xhigh`; cuando llega, se vuelve a emitir `Ready` con los modelos (el predeterminado primero, sin los `hidden`) y sus esfuerzos.
- **`thread/resume` pide `excludeTurns: true`** (evita la hidratación completa, `resume.jsonl:41`). quill no necesita la historia de Codex: la suya está en `quill-store`. La consulta `thread/turns/list` del driver de captura se quitó de `resume.jsonl` (con su respuesta, renumerando los ids siguientes del segundo proceso).
- **Siempre `approvalPolicy` y `sandbox`**, en `thread/start`, `thread/resume` y `thread/fork`, y `approvalPolicy` + `sandboxPolicy` en cada `turn/start` (así un cambio de modo vale desde el turno siguiente), más la `config` del thread con `update_plan` y `default_mode_request_user_input` (la CLI avisa con un `warning`, ignorado). `full-access` solo se acepta si `SessionSpec.allow_unsafe`; si no, el modo vuelve a `auto`.
- **Los modos son los tres presets del plan, sin cambios**, con los nombres `read-only`, `auto` y `full-access` (`unsafe_modes`). Pendiente de decidir: con `on-request` el modo `read-only` nunca pide permiso para un comando (Codex le deja al modelo el fallo del sandbox, ver Fase 1) y `auto` solo pregunta por lo que sale del sandbox o por un `apply_patch` fuera de él. Para ver tarjetas de aprobación de comandos hace falta `untrusted`. Propuesta: que `read-only` sea `untrusted` + `read-only`. No se cambió. Además, `capabilities.modes` son solo ids: la interfaz arma sus etiquetas con los nombres de Claude (`view.rs`), así que los de Codex se muestran crudos hasta la fase 5.
- **`ModeChanged`.** `encode` solo devuelve líneas, y Codex no tiene un pedido que cambie los ajustes de un thread (viajan en el `turn/start`). `SetMode` escribe un `account/read` (verificado en `account.jsonl`) y cuenta el cambio con su respuesta. `Ready.mode` trae el inicial.
- **Un `Prompt` antes de que exista el thread** se encola y sale como `turn/start` con la respuesta de `thread/start`. `steer: true` con un turno en curso es `turn/steer`; el `Running` de la respuesta lista todos los pedidos del turno.
- **Mapa de ítems** (`commandExecution`, `fileChange`, `mcpToolCall`, `webSearch`, `turn/plan/updated`, `agentMessage`, `reasoning`) como en la tabla de arriba. Precisiones: un comando con una sola acción `read`, `listFiles` o `search` es `Read` / `LS` / `Grep`, y todo lo demás (incluidas cadenas de acciones) es `Bash` con el comando sin la envoltura `bash -lc '…'`; el `Edit` / `Write` de un `fileChange` toma la llamada del ítem para el primer archivo y `<id>#<n>` para los siguientes; `webSearch` se dibuja al completarse (empieza con `query` vacía); el plan es un `TodoWrite` con su `ToolResult` al instante (`inProgress` → `in_progress`). `ToolDone` no se emite (`tool_hooks: false`: la atribución es por turno con el repo sombra).
- **`ToolResult.files` de una edición** sale del hunk: `before` / `after` son las líneas viejas y nuevas del diff con su contexto, **no el archivo entero** (comentado en el código como atajo). Un `add` es todo `after`; un `delete` es todo `before` (inferido: ninguna captura lo tiene). No hay conteo de agregadas / quitadas en el evento: `FileChange` no lo tiene, la interfaz lo saca de `before`/`after`.
- **Aprobaciones.** Comando: `PermissionRequest` con tool `Bash` e input `{command}`; archivo: la herramienta y el input del ítem (`Edit` / `Write`); pregunta: `AskUserQuestion` con `{questions:[{question, header, options, multiSelect: false}]}`. Se ofrece una sugerencia (`acceptForSession`) para que la tarjeta muestre "Siempre". `Allow` → `accept`, `AllowAlways` → `acceptForSession`, `Deny` → `decline` (el mensaje se pierde: Codex no lo lleva). `cancel` no se usa. La tarjeta contesta por el texto de la pregunta y Codex quiere el `id`: el adaptador los empareja (una pregunta es de opción única; un "Other" escrito va tal cual). Lo que quill no sabe contestar (`item/permissions/requestApproval`, `mcpServer/elicitation/request`, `item/tool/call`, refresco de tokens…) se contesta con un error JSON-RPC `-32601` y queda como `Unknown`, para que Codex no espere.
- **Tokens.** `Tokens` es lo escrito por el modelo en el turno (suma de `last.outputTokens`, inferido), `ContextUsage` es `last.totalTokens` sobre la ventana, sin categorías, y `TurnFinished.usage` suma entrada y salida del turno. No hay costo.
- **Cuenta.** Solo "ChatGPT account": `codex login --device-auth` imprime en stdout un código que la persona tiene que escribir en la página, y quill solo sabe mostrar una URL. `login_url` toma la línea que empieza con `https://` (la URL de autenticación viene sola en su línea y en stderr; la de `http://localhost:<puerto>` no sirve). `auth_query = ["login", "status"]`; `auth_state` lee `Logged in using …` / `Not logged in` (sin email: eso sale de `account/read`). `logout_run = ["logout"]`. **El motor ahora pasa las dos salidas:** `auth_state` recibe stdout, o stderr si stdout está vacío (no se concatenan: el JSON de Claude no tiene que ensuciarse con avisos de stderr), y el login lee también las líneas de stderr para encontrar la URL.
- **Motor.** `waives_server_consent` ya no nombra modos: pregunta al adaptador (`AgentAdapter::mode_waives_prompts`). Claude: `bypassPermissions` y `auto`; Codex: solo `full-access` (su `auto` sí pregunta, no es el de Claude). El mapa de modos del motor ahora guarda (agente, modo).
- **Fuera de alcance, queda `Unknown` o sin respuesta:** `collabAgentToolCall`, `subAgentActivity`, `imageView`, `contextCompaction`, `plan` (ítem), `dynamicToolCall`, `imageGeneration`, review, `sleep`, `functionCallOutput`; `Rewind`, `ContextUsage`, `McpStatus` y compañía, `Skills`, herramientas del anfitrión (`HostTool`, navegador) (`SessionSpec.resume_at` sí se soporta: ver «Rewind»). Las imágenes del prompt van como `{type:"image", url:"data:…;base64,…"}` (inferido del esquema).

- **Fuera de alcance, queda `Unknown` o sin respuesta** (los ítems de subagente, imagen, compactación, review, `sleep` y `dynamicToolCall` se resolvieron después, ver "Ítems restantes"; el ítem `plan`, en "Modo Plan"): `hookPrompt`, `functionCallOutput`; `Rewind`, `ContextUsage`, `McpStatus` y compañía, `Skills`, herramientas del anfitrión (`HostTool`, navegador) y `SessionSpec.resume_at` (fork desde un mensaje: `thread/fork` tiene `lastTurnId`, pero el evento `Text` no lleva el id del turno). Las imágenes del prompt van como `{type:"image", url:"data:…;base64,…"}` (inferido del esquema).
- **Replay.** `fork` pasa dentro de un solo proceso en la captura, pero quill forkea lanzando uno nuevo (`fork: true`): el test lo comprueba como dos corridas. Por el mismo motivo, el agente falso no puede reproducir `fork` entero (el segundo `thread/fork` de la captura no sale nunca). `resume` sí, porque el motor relanza el programa.

## Fases 4 y 5: cuenta e interfaz

Hecho en `feat/codex`. Lo nuevo es genérico (nada de la UI nombra un proveedor); lo de Codex vive en su adaptador.

- **Palabras de los modos desde el adaptador.** `AgentAdapter::modes()` y `default_mode()` (ver `docs/agents.md`). Se quitó el `match` fijo de `view.rs`: el menú, el chip, el menú `/permissions` y la página del agente buscan en las palabras del agente elegido. Claude devuelve las de siempre (Manual, Accept edits, Plan, Auto, Bypass permissions, mismos íconos y tintas). Codex: **Read only** ("Asks before anything but reading", tono `Read`, ojo), **Auto** ("Works in this folder; asks to go beyond it", tono `Auto`, por defecto) y **Full access** ("No sandbox, never asks", tono `Unsafe`, solo con "allow unsafe" en Settings, como el bypass de Claude). Las palabras de Codex son de quill (inferido: el CLI no da textos para estos presets).
- **Selector de agente** (`docs/ui.md` §2.3). El logo de Codex es el de svgl.app ("Codex", marca de OpenAI, recoloreado a `currentColor`), igual que el de Claude: `crates/quill-agent-codex/assets/codex.svg`, citado en `Codex::icon`. `Settings.default_agent` recuerda la elección; el agente de arranque era `"claude"` fijo y ahora es el recordado o el primero disponible. Antes de que exista una sesión, los modelos y esfuerzos del selector salen de la última sesión del mismo agente (`agent_capabilities`): con Codex, la primera vez, el chip de modelo dice "Default" hasta que arranca una sesión.
- **Nombre del agente en las tarjetas.** Las tarjetas decían "Allow Claude to…" fijo. Ahora usan la primera palabra del nombre del agente, y un comando sin descripción (los de Codex) dice "Allow Codex to run this command?" con el comando en la tarjeta. "Claude is working" del botón de PR pasó a "The agent is working".
- **Cuenta por agente.** La pantalla de inicio de sesión y el menú de la cuenta ya seguían a `ui.agent`; ahora la pantalla, si ese agente no tiene sesión, ofrece "Use {otro} instead" (si no, quien tiene dos agentes y uno sin sesión no podría volver al otro). `AgentAdapter::login_takes_code()` (Codex: no) quita el campo del código: el navegador vuelve solo al servidor local de `codex login`, así que la pantalla dice "Waiting for the browser…" con Copy link y Cancel.
- **Agente falso** (`fake-agent`): contesta `login status` ("Logged in using ChatGPT" por stderr y 0; "Not logged in" y 1 con `QUILL_FAKE_AUTH=signed-out` o `QUILL_FAKE_AUTH_CODEX`), `login` (las líneas de stderr de la fuente `codex-rs/cli/src/login.rs`, con la URL sola en su línea, espera `QUILL_FAKE_LOGIN_MS` y da la sesión por iniciada con el marcador) y `logout`. Las salidas de `login` y `logout` salen de la fuente, no de una corrida (inferido). La ruta de Claude (`auth …`) no cambió. Test del motor: `codex_signs_in_and_out_through_its_own_commands`.
- **Tests de la UI:** el chip solo con 2 o más agentes, elegir resetea modo/modelo/esfuerzo y guarda `default_agent` (y un arranque lo recuerda), las palabras de cada agente (Claude igual, las de Codex, `full-access` solo con Settings), y la cuenta del agente elegido.

### Verificación en pantalla (fase 5)

Con el agente falso reproduciendo los fixtures de `0.163.0` (`QUILL_PROGRAMS=claude=…,codex=…`, Xvfb 1408×844), elegido Codex en el selector y mandado el prompt de cada captura. Mirado en tema oscuro, y claro para el selector y una tarjeta de aprobación:

| Escenario | Resultado |
|---|---|
| `simple` | Respuesta, contexto y límites de uso de Codex (258.4K de ventana) |
| `edit-bash` | Tarjeta de aprobación de un archivo nuevo y de una edición (con su diff), la fila "Created and edited notes.txt, ran 2 commands", y las filas Bash y Edit desplegadas con el diff |
| `deny` | Tarjeta "Allow Codex to run this command?", Deny, la fila "Run a command Stopped" y la respuesta del modelo |
| `plan` | La lista de tareas (Create a.txt, Create b.txt, List the files) sobre el composer |
| `websearch` | "Searched 12 sites" con las tarjetas de fuentes |
| `ask` | La tarjeta de pregunta con Red / Blue / Other; contestada, el modelo responde "Red". No queda la tarjeta de "respuestas dadas" que Claude deja (el pedido de Codex no es una herramienta) |
| `steer` | El mensaje entra en el turno en curso y el modelo termina en 5 |
| `interrupt` | "Interrupted" tras parar el turno |
| Modos | Read only (ojo) y Auto con sus textos, marca de Auto |
| Modelos | Los 8 modelos de `model/list` |
| Selector | Logos y nombres de Claude Code y Codex, el elegido marcado |
| Cuenta | Con Codex sin sesión: "Sign in to Codex" con "ChatGPT account" y "Use Claude Code instead"; tras el clic, "Continue in your browser" sin campo de código; al iniciar, la página nueva. Menú de la cuenta con Sign out |

Sin verificar en pantalla ni con un CLI real: `resume` y `fork` (`fork` no se reproduce entero con el agente falso, ver Fases 2 y 3), que `default_agent` sobreviva a un reinicio (con `QUILL_PROGRAMS` quill no lee ni guarda Settings; lo cubre un test de la UI), el menú de modos con Full access (lo cubre un test), y `codex login` / `codex logout` reales (no se corrieron). El icono de saludo de la página nueva ("What's up next") sigue siendo la chispa de Desktop para cualquier agente.

## Contexto y Full access (2026-10-28)

- **Desglose del contexto.** Codex no nombra categorías como Claude Code. `thread/tokenUsage/updated` trae los tokens del último pedido (`edit-bash.jsonl:58`), y el adaptador los separa en "Cached input", "New input", "Reasoning" y "Output", más "Free space" hasta `modelContextWindow`. Que la entrada incluya la parte en caché y la salida incluya el razonamiento es la convención de uso de OpenAI (inferido).
- **Full access.** El motor decide si se puede pasar a un modo que salta todas las aprobaciones, mirando Settings en el momento del cambio (`skips_every_approval`, con el tono `Unsafe` de las palabras del adaptador). Codex acepta cualquier política por turno, así que una sesión abierta antes de habilitarlo también puede pasar a Full access. Claude Code sigue necesitando la bandera de arranque (`--allow-dangerously-skip-permissions`): una sesión lanzada sin ella rechaza el bypass (`ModeRefused`), y el motor la relanza en ese modo, retomando la conversación (al terminar el turno si está trabajando), cuando Settings lo permite ahora. Settings llega a las sesiones ya abiertas (`Input::AllowUnsafe`) y se aplica en cada lanzamiento: un modo inseguro que ya no se permite arranca en el modo propio del agente. El menú lo ofrece aunque todavía no haya ninguna sesión de Codex, con los modos inseguros de las palabras del adaptador.

## Rewind (2026-10-28)

Capturas `rewind.jsonl` (`codex-cli 0.163.0`, `gpt-6-luna`, esfuerzo `low`, `workspace-write`, 4 turnos): dos turnos («Remember the word: lantern», «Also remember the word: harbor»), un `thread/fork` con `lastTurnId` = turno 1 y un turno en la copia, un `thread/revert` del original con `beforeTurnId` = turno 2 y un turno en el original.

- **`thread/fork {threadId, lastTurnId}`.** Crea un thread nuevo con los turnos hasta `lastTurnId` inclusive (`rewind.jsonl:48`, respuesta `:56`, `thread/started` `:59`); el original queda entero. Pregunta en la copia: «Which words did I ask you to remember?» responde `lantern`, sin `harbor` (`rewind.jsonl:72`). El esquema pide que ese turno no esté en curso.
- **`thread/revert {threadId, beforeTurnId}`.** Reemplaza el historial durable del mismo thread por el prefijo anterior a `beforeTurnId` (`:77`); responde con el thread sin turnos y cursores, y emite `thread/reverted` (`:87`). Pregunta después de revertir: responde `lantern` solo (`:101`). Es destructivo y en el lugar: el thread pierde los turnos.
- **Archivos.** El esquema de `ThreadRevertParams` dice que «only changes persisted conversation history. It does not revert local file changes» (`ClientRequest.json`, solo esquema; la captura no tocó archivos, así que que no los revierte es lo que dice el esquema, no una prueba) *(inferido)*. `thread/fork` tampoco toca archivos *(inferido)*. Por eso **gana quill**: los archivos son siempre de sus instantáneas (`Intent::Rewind`, motor), y Codex nunca los toca, así que no se pisan. `capabilities.rewind` ("el agente restaura archivos solo") queda en false, y `AgentCommand::Rewind` no se manda a Codex.
- **Mapeo.** El motor ya hace «Rewind to here» y «Fork from here» con `SessionSpec { resume, fork: true, resume_at }` (Claude: `--resume-session-at`). En Codex, `resume_at` es el id de un **turno**: el evento `Text` lleva `id` = turno del `item/completed` del mensaje (`rewind.jsonl:30`: el `turnId` del turno 1), y la UI toma el último mensaje del agente anterior al del usuario. El siguiente arranque hace `thread/fork {threadId, lastTurnId: resume_at}` (con `approvalPolicy` y `sandbox`, como siempre). Sin mensaje anterior el motor arranca un thread nuevo. «Undo rewind» vuelve al thread original, que sigue entero. Sin `fork`, `resume_at` se ignora.
- **No usado: `thread/revert`.** Es destructivo y no se puede deshacer, y el motor ya resuelve el rewind con un fork (que además conserva el original para «Undo rewind»). Queda como opción si se quisiera no acumular threads.
- **Sin probar:** que `lastTurnId` con un turno de varias herramientas y archivos funcione igual (el turno de la captura no tenía herramientas); la tarjeta de Changes tras un rewind con Codex real.
- **Pantalla (agente falso, Xvfb).** El selector eligió Codex; bajo el mensaje del usuario aparecen copiar, rewind y fork; el rewind recorta la conversación desde ese mensaje, devuelve el texto al composer y muestra «Rewound to an earlier message. Undo rewind». El agente falso reproduce la captura sin entender su contenido, así que el `lastTurnId` real lo comprueba el test del adaptador.

## Ítems restantes (2026-10-28)

Capturas nuevas con `quill-capture --agent codex <escenario>` (`gpt-6-luna`, esfuerzo `low`, `workspace-write`, sin tocar `~/.codex/config.toml`; `multi_agent`, `image_generation`, `view_image` y `sleep_tool` ya vienen activos en `codex features list`): `subagent`, `image`, `view-sleep`, `compact` y `review`. Turnos gastados: 7, contando `thread/compact/start` y `review/start` (el primer `subagent` se descartó: el driver terminó con el `turn/completed` del hilo hijo, y ahora distingue por `threadId`). Fixtures anonimizados como los demás (`/work`, `/home/user`); en `image.jsonl` el `result` (1,3 MB de PNG en base64) se reemplazó por un PNG de 24×24, y el `savedPath` quedó con la forma `~/.codex/generated_images/<thread>/<item>.png`.

### Subagentes
- **Lo observado** (`subagent.jsonl`). Con `multi_agent`, crear un subagente **no** es un `collabAgentToolCall` `spawnAgent`: es un ítem `subAgentActivity` `kind: "started"` con `agentPath` (`/root/ping`, el nombre que le dio el modelo), `agentThreadId`, `model` y `reasoningEffort` (`:31`). No trae el prompt: Codex no lo dice. El `wait` sí es un `collabAgentToolCall` (`:39`, `:49`) con `agentsStates` y `receiverThreadIds` vacíos. Al terminar llega otro `subAgentActivity` `kind: "completed"` con un id nuevo (`subagent-completed-<turno>`, `:45`).
- **Cómo se distingue el hilo hijo.** Sus notificaciones viajan por la misma conexión con su propio `threadId` (`:35` `turn/started`, `:47` `turn/completed`); no hay `thread/started` del hijo ni ítem `userMessage`. Se ata al `agentThreadId` del `subAgentActivity`. Antes de ese ítem llegan avisos sueltos del hijo (`thread/status/changed`, `:30`).
- **Mapa.** `started` → `ToolUse` `Agent` con `description` (el nombre del subagente, con la primera letra en mayúscula: "Ping"), `model` y `effort`, y `TaskStarted { kind: "agent", call }`. Los mensajes y pasos del hijo salen con `parent = call` (la UI ya los junta en el panel). Su `thread/tokenUsage/updated` es el `TaskProgress` (tokens totales y usos de herramientas contados por quill). `completed` → `TaskEnded` (el resumen es su último mensaje, `pong`) y el `ToolResult` del `Agent`. `interrupted` → `TaskEnded { stopped }`. `interacted` no dibuja nada. El `turn/started` y el `turn/completed` del hijo no abren ni cierran el turno de la sesión.
- **UI.** La fila "Ran agent Ping", el panel (con "Model GPT-6-Luna · low", y sin la burbuja del prompt cuando no hay) y el panel de tareas ("Agent · Completed · 2s · 27.4k tokens · 0 tool uses") son los de Claude. El esfuerzo en el panel es nuevo.
- **Inferido, sin captura:** `collabAgentToolCall` `spawnAgent` con `prompt`, `model`, `receiverThreadIds` y `agentsStates` (de `ThreadItem` del esquema): `Agent` con el `prompt`, y `TaskEnded` por cada hijo cuando su estado es `completed`, `errored`, `interrupted` o `shutdown`. `sendInput`, `sendMessage`, `followupTask`, `closeAgent`, `interruptAgent`, `resumeAgent` y `listAgents` son pasos sueltos (solo se vio `wait`). La aprobación que pida un hijo (su `threadId` es otro) no se probó.
- **Choque de ids arreglado.** El selector de agente usaba `agent:<id>` y la fila del subagente `agent:<llamada>`: un clic en el subagente "elegía" un agente con el id de la llamada y lo guardaba como predeterminado. El selector ahora es `pick-agent:<id>` (test: `a_helper_row_opens_its_pane_and_is_not_the_agent_picker`).

### Compactación
- **Observado** (`compact.jsonl`): `thread/compact/start` → `{}`, y un turno propio con `contextCompaction` (`:37` started, `:43` completed). `thread/compacted` (obsoleto, "usa el ítem") no llegó; se ignora para no dibujarlo dos veces.
- **Qué es en la UI:** la compactación de Claude Code (`system/compact_boundary`, `docs/engine.md`) hoy **no se dibuja**: no había nada que reusar. El diseño es nuevo y vale para ambos cuando se conecte el de Claude: `ToolUse` `Compact` → una línea con filetes ("Compacting context" mientras corre, "Context compacted" al terminar). Inferido: Desktop no está capturado. La entrada de Claude queda pendiente.

### Review
- **Observado** (`review.jsonl`): `review/start {threadId, target: {type: "uncommittedChanges"}}` (`:9`) responde con el turno y `reviewThreadId` (el mismo hilo, inline); `enteredReviewMode` trae `review: "current changes"` (`:17`) y `exitedReviewMode` trae el texto de los hallazgos (`:63`), seguido de un `agentMessage` con el mismo texto. Los dos llegan completos (started y completed iguales).
- **Mapa:** `EnterReviewMode` / `ExitReviewMode`. "Started a review of current changes" y "Finished the review", esta con los hallazgos en markdown al abrirla. Las palabras son de quill, en el estilo de "Started planning".
- quill no tiene aún cómo *pedir* un review (`review/start`): solo dibuja el que Codex emite.

### Imágenes
- **`imageView`** (`view-sleep.jsonl:26`) es `{id, path}`: se muestra como el `Read` de la imagen ("Read logo.png"), y al abrir la fila la imagen como miniatura, que se abre en el visor. Esto vale también para el `Read` de Claude de un archivo `png`/`jpg`/`jpeg`/`gif`/`webp` (antes solo decía "Read x.png").
- **`imageGeneration`** (`image.jsonl:40-41`): `started` con todo vacío y `completed` con `status: "completed"`, `revisedPrompt`, `savedPath` (bajo `~/.codex/generated_images/`) y `result` (el PNG en base64, que quill no usa: lee el archivo). Es un bloque propio, no una fila dentro del grupo: la línea "Generated an image ›" y, debajo, la miniatura de 168 px (la de los mensajes, más grande); el clic abre el visor, que crece desde ella. El prompt revisado es un brief de una docena de líneas, así que espera detrás de la línea, que lo abre con el resorte de los pasos (a 480 px de ancho). El paso se abre con la entrada vacía (la ruta recién llega al terminar), así que el `ToolResult` lleva el prompt como salida y el archivo en `files`. Mientras corre, es la línea de estado ("Generating an image 9s") con un cuadrado de 168 px que late en el lugar de la imagen (`Skeleton`); si falla, es una fila.
- **Inferido:** `failure` (`usageLimitExceeded`, único valor del esquema) y cualquier `status` que no sea `completed` o `in_progress` terminan el paso como error ("Image generation limit reached" / "The image could not be made"), sin imagen. Diseño visual inferido.

### `sleep` y `dynamicToolCall`
- `sleep` (`view-sleep.jsonl:33`) es `{id, durationMs}` → "Waited 1s" ("Waiting 1s" mientras corre; "300ms" y "2.5s" para los demás).
- `dynamicToolCall`: no hay captura. Son herramientas que declara el cliente: quill declara las suyas (`HOST_TOOLS`: `start_session`, `read_session`, `list_sessions`, `wait_for_session`…) como `dynamicTools` del `thread/start` (`{name, description, inputSchema}`; un hilo reanudado conserva las que tenía). Codex las llama con el pedido `item/tool/call` (`{threadId, turnId, callId, tool, arguments}`), que el adaptador pasa al motor como `HostTool` y responde con `{contentItems: [{type: "inputText", text}], success}`. El ítem se llama `sessions__<tool>`, como lo sirve el adaptador de Claude, y la UI lo dibuja como herramienta de quill; otra herramienta se dibuja "Used {tool}" con el texto como salida (una imagen queda `[not text]`). Todo inferido del esquema del app-server.

### Sin cuenta ("Sign in")
- `fixtures/0.163.0/synthetic-unauthorized.jsonl` es **sintético**: es `simple.jsonl` hasta el mensaje de la persona, más un `error` y un `turn/completed` `failed` con la forma del esquema (`ErrorNotification`, `TurnError`, `CodexErrorInfo: "unauthorized"`). Esas dos líneas llevan `"synthetic": true`. El texto del error lo inventó quill, sobre el modelo del aviso de renovación de token; Codex puede decir otra cosa. No se cerró la sesión real para obtener uno de verdad.
- Resultado en pantalla: el turno cierra con "Not signed in" y el botón **Sign in** (`AuthRequired` una sola vez, aunque lleguen el `error` y el `turn/completed`).

### Verificación en pantalla
Agente falso con los fixtures de `0.163.0`, Codex elegido en el selector, 1408×844; para `image` y `view-sleep` una copia del fixture con la ruta apuntando a un archivo real (el fixture anonimizado apunta a `/home/user/...`). Oscuro: los cinco ítems y el aviso de sesión. Claro: `image`, `subagent` (panel) y `compact`. Para `compact` y `review` el fixture tiene un pedido de más (`thread/compact/start`, `review/start`) que quill no escribe: en pantalla el segundo mensaje de la persona hace de pedido de compactar, y en `review` el primero hace de `review/start`.

## Extensiones (2026-10-28)

Servidores MCP, plugins, marketplaces y skills de Codex, con el mismo detalle que Claude Code en Customize. El código está en `crates/quill-agent-codex/src/extensions.rs`; el contrato y la comparación, en [agents.md](agents.md) "Extensiones por CLI". Capturas anonimizadas en `fixtures/0.163.0/{mcp-list,plugin-list,marketplace-list,skills-list,mcp-status}.json`.

- **`codex mcp list --json`:** una lista de `{name, enabled, disabled_reason, transport, auth_status}`; `transport` es `stdio {command, args, env, env_vars, cwd}` o `streamable_http {url, bearer_token_env_var, http_headers, …}`. No dice de qué capa viene cada uno, ni los arranca. `auth_status` vale `unsupported`, `unknown` o `not_logged_in`, entre otros.
- **`codex plugin list --json --available`:** `{installed: […], available: […]}` (con 8811 ofertas en una cuenta real). Cada fila trae `pluginId` (`nombre@marketplace`), `name`, `marketplaceName`, `version`, `installed` y `enabled`, sin descripción ni instalaciones. `plugin marketplace list --json` es `{marketplaces: [{name, root, marketplaceSource?}]}`.
- **`skills/list {cwds, forceReload}`** sin thread: `{data: [{cwd, skills: [{name, description, path, scope, enabled, pluginId, interface?}], errors}]}`. Una cuenta real dio 87 skills (82 `user`, 5 `system`). `~/.agents/skills` los lee Codex también para `user`.
- **`skills/config/write {path, enabled}`** escribe `[[skills.config]]` en el `config.toml` (verificado en una carpeta de prueba); `skills/list` muestra después `enabled: false`.
- **`mcpServerStatus/list {threadId?, detail}`** da `runtimeStatus` (`connected`, `starting`, `authenticationRequired`, `failed`, `disabled`; `null` fuera de un thread), `authStatus`, `pluginId`, `toolsError` y `tools` (un mapa con `annotations.readOnlyHint` y `destructiveHint`). Aparece `codex_apps`, el servidor de las apps de la cuenta de ChatGPT, que `codex mcp list` no trae.
- **Reconectar:** `config/mcpServer/reload` (contesta `{}`); no hay reconexión de un servidor.
- **Skills en el menú `/`:** al estar listo el agente, el motor corre `skills/list` aparte (otro proceso, sin tocar las líneas de la sesión) y avisa `AgentEvent::Skills`. La UI los suma a los comandos. Al mandar uno, el adaptador cambia `/nombre` por `$nombre` (mención explícita, inferido de la TUI).
- **Sin verificar:** `plugin add|remove`, `plugin marketplace add|upgrade|remove` (no se corrieron, solo sus `--help`), `mcpServer/oauth/login` y `config/mcpServer/reload` dentro de una sesión real, y que `$nombre` active el skill.

## Modo Plan (2026-10-31)

Captura `plan-mode.jsonl` (`codex-cli 0.163.0`, `gpt-6-luna`, esfuerzo `low`, 2 turnos, con `quill-capture --agent codex plan-mode`): un turno en modo Plan que pregunta primero y propone un plan de tres puntos, y otro en modo Default con "Implement the plan.", que crea el archivo.

- **Protocolo** (API experimental; quill ya pide `experimentalApi` en `initialize`): `turn/start.collaborationMode = {mode: "plan" | "default", settings: {model, reasoning_effort, developer_instructions}}`. Con `developer_instructions: null` Codex usa sus propias instrucciones para el modo (`codex-rs/collaboration-mode-templates`), y `settings` manda sobre el `model` y el `effort` del turno. `collaborationMode/list` respondió `[{name: "Plan", mode: "plan", model: null, reasoning_effort: "medium"}, {name: "Default", mode: "default", model: null, reasoning_effort: null}]`. quill no lo pide: en esta versión los modos son esos dos (`ModeKind`).
- **El modo queda en el thread** hasta que otro `turn/start` lo cambie (`thread/resume` lo devuelve en `collaborationMode`). Por eso cada `turn/start` de quill lleva el suyo: `plan` en el modo Plan y `default` en los demás, con el modelo y el esfuerzo elegidos. Si la persona no eligió esfuerzo va el del thread (`reasoningEffort` de `thread/start`), para no pisar el de su config. Codex contesta cada cambio con `thread/settings/updated` (`:17`, `:131`): los ajustes completos, con el texto de las instrucciones del modo. Se ignora.
- **Plan en quill** es un cuarto modo del selector: `plan`, tono `Plan` (el pergamino), "Explores read-only and proposes a plan", con `on-request` y el sandbox `read-only`. Explora sin pedir permiso y nada que corra puede cambiar un archivo; si el modelo pide salir del sandbox, aparece la tarjeta de aprobación de siempre. En Codex el modo Plan no cambia los permisos (son otros campos del mismo `turn/start`): que no se toquen archivos lo piden sus instrucciones. En quill además es de solo lectura.
- **El plan propuesto** es un ítem `plan` `{id: "<turno>-plan", text}`: `:73` started con el texto vacío, los `item/plan/delta` en `:74-119` y `:120` completed. El texto es lo que el modelo escribió entre `<proposed_plan>` y `</proposed_plan>`. El mensaje del agente no lo repite: en la captura el turno no tuvo `agentMessage` con el plan, y la fuente (`codex-rs/utils/stream-parser`) lo saca del texto visible. El adaptador lo dibuja como el `ExitPlanMode` de Claude Code, al terminar: la línea "Proposed plan ›", que abre el plan en su panel. Los deltas se ignoran, porque el esquema avisa que no tienen por qué sumar el texto final.
- **La pregunta del modo Plan** llegó con `isBlocking: true` (`:67`); en Default (`ask.jsonl`) llega `false`. Codex la marca bloqueante solo en Plan (`codex-rs/core/src/tools/handlers/request_user_input.rs`).
- **"Implement this plan?"** Al terminar el turno Codex no espera nada: el plan solo queda propuesto. La tarjeta es la del plan de Claude Code (`plan_card`, `docs/ui.md` §2.7), sin Reject. Si un turno termina con un plan que el agente no pidió aprobar y la sesión sigue en un modo de tono `Plan`, la tarjeta "Codex proposed a plan" queda bajo el turno con "Open plan", "Revise…" e "Implement in {modo}".
  - Implementar pasa la sesión al modo que tenía antes del Plan, si se ofrece, o si no al modo por defecto del agente (Auto). Después manda "Implement the plan." como mensaje de la persona, que queda en el transcript. El menú del botón ofrece los demás modos que no planifican.
  - "Revise…" manda lo escrito como el mensaje siguiente, todavía en Plan. Escribir en el composer hace lo mismo.
  - "Implement the plan." es lo que manda la TUI de código abierto de Codex al implementar (`codex-rs/tui/src/chatwidget/plan_implementation.rs`).
- **Un modo elegido sin proceso se perdía**: el `SetMode` de una sesión leída al arrancar, antes de su primer mensaje, iba a un proceso que no existía. El motor lo guarda ahora en el `SessionSpec`, como el modelo y el esfuerzo, y la sesión arranca en él (test `a_mode_chosen_before_the_agent_runs_is_the_one_it_starts_in`). Sin esto, "Implement" después de reiniciar quill corría en modo Plan.
- **El aviso del modo** (`SetMode` → `account/read`) usa el id `quill-mode`, como `quill-models`. Así no corre la numeración de las capturas y el agente falso lo contesta solo. Con un pedido en camino, otro cambio no manda un segundo pedido: se cuenta el último modo.
- **Ruido nuevo, ignorado:** `turn/moderationMetadata` (las marcas de moderación de cada turno) y `thread/readState/changed`. No aparecían en las capturas del 2026-10-28.
- **Fixture.** Anonimizado como los demás (`/work`, `/home/user`, el id de instalación fijo). Además:
  - Se quitaron el `collaborationMode/list` del driver y su respuesta, y se renumeraron los ids que siguen.
  - Se agregó la línea `quill-models` de siempre.
  - Un comando del modelo que listó el home de la persona quedó solo con la línea de `/work`, sin sus `outputDelta`.
  - Los deltas del último mensaje, que partían una ruta en pedazos, son uno solo.
- **Inferido:** que volver a Auto sea lo esperado cuando la sesión empezó en Plan, y el diseño de la tarjeta sin Reject.
- **Verificación en pantalla** (agente falso con `plan-mode.jsonl`, Xvfb 1408×844, oscuro): el selector con Read only, Auto y Plan; la pregunta bloqueante con Other; "Proposed plan ›" y la tarjeta "Codex proposed a plan" con "Revise…" e "Implement in Auto" (en el menú, "Implement in Read only"); "Open plan" abre el plan en su panel. Al implementar, "Implement the plan." queda como mensaje, el chip pasa a Auto y corre el segundo turno.

## Preguntas: texto libre, secretos, solo texto y las que no bloquean (2026-10-31)

`item/tool/requestUserInput` trae por pregunta `isOther` (se puede contestar con palabras propias), `isSecret` (la respuesta se oculta) y `options`, que puede ser `null` (solo texto). Por pedido trae `isBlocking`. El adaptador lo pasa a la tarjeta de pregunta de quill con palabras propias de la tarjeta:
- `other: false` quita la fila "Other", que era fija.
- `secret: true` enmascara el campo (`TextField::password` de limn).
- Sin opciones, la pregunta es solo un campo, "Type your answer".
- `blocking: false` la marca como una pregunta sin la que el agente sigue. Sin `isBlocking` vale `true`, el valor por defecto del esquema.

- **Lo que manda codex 0.163.0.** Su herramienta `request_user_input` exige opciones (2 o 3), fuerza `isOther: true` y no ofrece `isSecret` (`codex-rs/core/src/tools/handlers/request_user_input_spec.rs`). `isBlocking` es `true` solo en modo Plan (`plan-mode.jsonl:67`; en Default, `ask.jsonl:30`, es `false`). Las preguntas de solo texto y las secretas existen en el protocolo pero no salen de esta versión: sus tests son sintéticos, con la forma del esquema *(inferido)*.
- **La respuesta.** Una pregunta que la tarjeta no contesta queda fuera de `answers`; si no contesta ninguna, va `{answers: {}}`, lo mismo que manda la TUI de Codex al vencer el plazo. "Skip" sigue mandando la pregunta con `answers: []`.
- **Las que no bloquean se resuelven solas.** Si la persona no toca la tarjeta durante un minuto, la tarjeta cuenta otro minuto ("Skipped in 42s", junto a Skip) y al terminar se contesta sin respuestas: el modelo sigue con su mejor criterio, como le piden las instrucciones del modo Default de Codex. Tocar la tarjeta (elegir, escribir, pasar de página) apaga la cuenta y la pregunta espera a la persona.
  - Los tiempos son de quill. Se tomaron de las constantes de la TUI de código abierto de Codex (60 s ocultos y 60 s visibles, `codex-rs/tui/src/bottom_pane/request_user_input/mod.rs`), que también apaga la cuenta con cualquier tecla.
  - El reloj corre desde que quill ve la pregunta, esté o no en pantalla. Con la ventana sin pintar (minimizada, inferido) se resuelve en el cuadro siguiente.
  - Una pregunta que bloquea nunca vence.
- **Secretos.** Lo escrito se enmascara y, al enviarlo, se borra de la memoria de la UI. Las respuestas ya no se guardan con la sesión, para ningún agente: el `Event::Decided` que se persiste dice solo que la tarjeta se contestó (test del motor `an_answer_is_not_kept_with_the_session`). La respuesta sí queda en el registro propio de Codex (`~/.codex/sessions`), fuera del alcance de quill.
- **limn.** Un `TextField` con `password(true)` que tiene texto hace que el ícono "›" de la tarjeta (página siguiente) se pinte como un cuadrado blanco. Los comandos de dibujo son los correctos (el `Icon` lleva el SVG del caret), así que el problema está al pintar, en `limn-egui`. Escribir caracteres nuevos en un campo normal no lo provoca. Hace falta un PR a limn; quill no lo parchea.
- **Verificación en pantalla** (agente falso, Xvfb 1408×844, oscuro):
  - `ask.jsonl`, que no bloquea: la tarjeta con Red / Blue / Other; al minuto "Skipped in 40s" junto a Skip; al vencer, la tarjeta se va y el turno sigue.
  - Una copia de `ask.jsonl`, fuera del repo, con una pregunta secreta de solo texto y otra sin "Other": el campo enmascarado ("•••"), Next habilitado al escribir, y la segunda página con Red / Blue sin Other.
