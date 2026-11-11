# Grok Build en Newen (`newen-agent-grok`)

Grok Build, el CLI de agente de xAI (binario `grok`), es el quinto agente de Newen. Habla ACP con `grok agent stdio`; el protocolo común está en [acp.md](acp.md). Todo lo de Grok vive en `crates/newen-agent-grok`.

## Fuente de verdad

- **Código abierto:** `xai-org/grok-build`, Apache-2.0, en Rust. El código público es `1.0.45` (`SOURCE_REV 559751f`); el binario publicado, `1.0.77`, que es el que se capturó. Lo que dice este documento sale de las capturas de `1.0.77` cuando difiere del código.
- **Versión:** no manda `agentInfo`; la da `initialize` en `_meta.agentVersion` (`Flavor::version`). `tested()` = `1.0.77`.
- **Instalación:** `curl -fsSL https://x.ai/cli/install.sh | bash` deja el binario en `~/.grok/bin` y agrega esa carpeta al `PATH` solo de bash y zsh. Newen usa `grok` del `PATH`, y si no está, `~/.grok/bin/grok`.
- **Actualización** ([agents.md](agents.md), «El programa al día»): `grok update` (`update_args`). `grok --version` imprime `grok 1.0.77 (50ca579fa996) [stable]`, de donde sale `1.0.77`, igual a lo que dice `_meta.agentVersion`. Los modelos fuera de una sesión son `grok models` (`models_runs`): después de «Available models:», uno por línea por su id, con el por defecto marcado (`  * grok-4.7 (default)`). No da nombres: el menú lo llama por su id hasta que una sesión dice el suyo. Con más de un modelo, el formato de la lista es *(inferido)*: la cuenta de la captura tiene uno solo.
- **Capturas** (2026-11-08, `crates/newen-agent-grok/fixtures/1.0.77/`): Grok 4.7 con la cuenta de la persona para `simple`, `edit-bash`, `deny`, `always`, `todos`, `ask`, `interrupt`, `resume` y `unsafe`; con el modelo de la captura (`--fake-model`, un modelo propio de Grok apuntado a ese puerto) para `host` y `plan`, porque la cuota gratuita de la cuenta se terminó a mitad. Recortadas: las notificaciones `_x.ai/models/update`, `settings/update`, `announcements/update` y `session/setup`, y los `available_commands_update` repetidos iguales (el adaptador no los usa).
- **Sin tocar la configuración de la persona:** `GROK_HOME` y `HOME` en una carpeta de prueba y `GROK_AUTH_PATH` en el `auth.json` de la persona. Con el modelo de la captura: `[model.fixture] base_url = "http://127.0.0.1:<puerto>/v1"`, `api_key`, y `[models] default = "fixture"` en el `config.toml` de esa carpeta (con un modelo así `session/new` no pide cuenta).

## Proceso

Un `grok agent stdio` por sesión. Sin cuenta, `initialize` responde pero `session/new` da `-32000` ("Authentication required"): Newen lo muestra como cuenta requerida. La cuenta es la de `grok login` (navegador, o `--device-auth`), guardada en `~/.grok/auth.json`; Newen no la maneja (sin `sign_in_methods`). Con la cuota gratuita agotada, un turno termina con `-32003` y el mensaje de Grok.

## Modos

`session/new` no trae `modes`: son de Newen (`Flavor::unlisted_modes`), y cada uno se alcanza con `session/set_mode` (que toma `default` y `plan`) más la notificación propia `_x.ai/yolo_mode_changed` (`Flavor::mode_switch`):

| Modo | Tono | Cómo |
|---|---|---|
| Ask (`default`, por defecto) | Ask | `{yolo_mode: false, permission_mode: "default"}` + `default`. Pregunta antes de editar y de cada comando (captura `edit-bash`) |
| Auto (`auto`) | Auto | `{permission_mode: "auto"}` + `default`: su chequeo de seguridad decide |
| Plan (`plan`) | Plan | `session/set_mode plan`; el plan se aprueba con `_x.ai/exit_plan_mode` |
| Always approve (`always-approve`) | Unsafe | `{yolo_mode: true}` + `default`. Nunca pregunta (captura `unsafe`: una edición sin tarjeta). Solo con "Allow bypass permissions mode" en Settings |

Grok contesta `current_mode_update` con el id de `set_mode` (`default`), que comparten tres modos de Newen: el núcleo mantiene el de la sesión. Al aprobar un plan Grok vuelve a `default` y la tarjeta pone el modo elegido.

## Herramientas

Su primer `tool_call` nombra la herramienta en el título (`search_replace`) sin estado, y el núcleo la dibuja ahí; los siguientes traen `rawInput.variant` (`SearchReplace`) y un título legible. El `Flavor` lee las dos formas:

| Grok | Newen |
|---|---|
| `run_terminal_command` / `Bash` | `Bash` (`command`, `description`) |
| `read_file` / `ReadFile` | `Read` (`target_file`) |
| `search_replace` / `SearchReplace` | `Edit`; con `old_string` vacío (un archivo nuevo), `Write` |
| `write` / `Write`, `grep` / `Grep` | `Write`, `Grep` |
| `list_dir` / `ListDir` | `LS` |
| `web_search`, `web_fetch` | `WebSearch`, `WebFetch` |
| `spawn_subagent` / `Task` | `Agent` |
| `search_tool` | `ToolSearch` |
| `use_tool` (`tool_name: "<servidor>__<tool>"`) | `mcp__<servidor>__<tool>` (el servidor más largo de la sesión con ese prefijo) |
| `todo_write` | nada: la lista llega como `plan` de ACP |
| `ask_user_question`, `exit_plan_mode` | nada: son las tarjetas de abajo |

La salida de una herramienta está en `rawOutput` (el `content` de un comando es su descripción): `output_for_prompt`, `tool_output_for_prompt`, `summary_for_prompt` o `content`, arriba o bajo su etiqueta (`{"EditsApplied": {…}}`) (`Flavor::output`). Los tokens del turno vienen en el `_meta` de la respuesta de `session/prompt` (el núcleo los lee ahí cuando no hay `usage`).

## Preguntas y planes

Son pedidos propios, no `elicitation/create` (`Flavor::ask` / `Flavor::answer`):

- `_x.ai/ask_user_question {questions: [{question, options: [{label, description}], multiSelect}]}` → tarjeta `AskUserQuestion`. Se contesta `{outcome: "accepted", answers: {<pregunta>: [labels]}}`; palabras propias van como `["Other"]` con `annotations[<pregunta>].notes`; detenido, `{outcome: "cancelled"}` (captura `ask`).
- `_x.ai/exit_plan_mode {planContent}` → tarjeta `ExitPlanMode` con el plan. Aceptado, `{outcome: "approved"}`; "Revise…", `{outcome: "cancelled", feedback}` (captura `plan`).

## Herramientas de Newen

`session/new` toma servidores MCP stdio, HTTP y SSE (`SessionExtras::host`): los tres de Newen corren en el proceso de Grok, que no los muestra al modelo directamente sino por `search_tool` y `use_tool` (captura `host`: `sessions__list_sessions`, sin tarjeta).

## Opciones (Settings)

Para las sesiones nuevas. `grok agent stdio` toma pocas opciones de línea de comandos; el resto, sus variables (`1`/`0`), que van solo cuando la opción deja su valor por defecto, así su `config.toml` decide si no:

| Opción | Por defecto | Cómo |
|---|---|---|
| Sandbox | como esté Grok | `GROK_SANDBOX`: `off`, `workspace` ("This folder"), `read-only`, `strict` |
| Web search and fetch | sí | `--disable-web-search` antes de `agent` |
| Subagents | sí | `GROK_SUBAGENTS` |
| Memory (experimental) | no | `GROK_MEMORY` |
| Background workflows | sí | `GROK_WORKFLOWS` |
| Skip ignored files | no | `GROK_RESPECT_GITIGNORE` |
| Usage data | sí | `GROK_TELEMETRY_ENABLED` |

Verificado con el 1.0.77: arranca y abre sesión con todas puestas.

## Logo

Su marca de svgl.app (`grok-dark.svg`) en `currentColor` (`assets/grok.svg`): solo donde Newen nombra a sus agentes (AGENTS.md).

## Customize

- **Conectores:** `grok mcp list --json` (`[{name, scope, enabled, command, args, env}` o `{…, url, headers}]`, lee su config sin conectar). Agregar: `grok mcp add [-s user|project] [-e K=V]… <nombre> -- <comando> [args…]`, o `-t http [-H "N: v"]… <nombre> <url>`. Quitar: `grok mcp remove <nombre>`. Alcances "All my projects" (`user`, `~/.grok/config.toml`) y "This project" (`project`, `.grok/config.toml`).
- **Skills y plugins:** no se listan.

## Lo que no incluye

- **Cuenta en Newen:** iniciar y cerrar sesión es `grok login` / `grok logout` en la terminal.
- **Fork:** no tiene `session/fork` de ACP (solo una extensión propia); un fork de Newen abre una sesión que la reanuda.
