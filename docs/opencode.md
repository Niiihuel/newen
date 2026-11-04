# OpenCode en Newen (`newen-agent-opencode`)

OpenCode es el tercer agente de Newen. Habla ACP con `opencode acp`; el protocolo común está en [acp.md](acp.md). Todo lo de OpenCode vive en `crates/newen-agent-opencode`.

## Fuente de verdad

- **Código abierto:** `anomalyco/opencode`, MIT, en TypeScript (OpenCode V2). Leído en el commit `c3c5531` (2026-11-03).
- **Versión probada:** `opencode v2.2.5` (`@opencode/cli` en npm, binario `@opencode/cli-linux-x64`). `initialize` dice `agentInfo.version: "2.2.5"`.
- **Capturas** (2026-11-03): `crates/newen-agent-opencode/fixtures/2.2.5/`, con el modelo gratuito `opencode/big-pickle` de OpenCode Zen, sin cuenta. Escenarios: `simple`, `edit-bash`, `deny`, `always`, `todos`, `ask`, `interrupt`, `resume`, `fork`, `plan`, `ask-mode`, `host`. Más `debug-config.json` (Customize).
- **Sin tocar la configuración de la persona:** cada comando corrió con `HOME`, `OPENCODE_TEST_HOME`, `OPENCODE_CONFIG_DIR`, `OPENCODE_DB` y `XDG_*` en una carpeta de prueba (la receta de los propios tests de OpenCode).

## Proceso

`opencode acp` en la carpeta de la sesión, un proceso por sesión: cada uno levanta su propio servidor privado de OpenCode y lo cierra al cerrarse stdin (sale con 0). Las sesiones quedan en su base SQLite, compartida con la TUI. Por stdout solo va JSON-RPC.

Su config de ese proceso (`OPENCODE_CONFIG_CONTENT`, una capa sobre la de la persona; Newen no escribe ningún archivo de OpenCode) lleva:
- el agente `ask` (ver "Modos");
- los skills "All agents" de Newen: `skills: [newen/injected/opencode/skills]` (verificado por la investigación con una carpeta fuera de todas las de OpenCode).

**Límite:** si la persona tiene su propio `OPENCODE_CONFIG_CONTENT` en el entorno, el de Newen lo reemplaza en estas sesiones.

## Modos

Los modos de OpenCode son sus agentes primarios, en la opción de config `mode` (no hay objeto `modes` ni `current_mode_update`): se cambian con `session/set_config_option`.

| Modo | Tono | Qué hace |
|---|---|---|
| Build (`build`, por defecto) | Auto | Todo, preguntando solo donde lo dicen los permisos de la persona: por defecto, únicamente al leer un `.env` |
| Plan (`plan`) | Plan | Lee y planea; niega las ediciones (captura `plan`) |
| Ask (`ask`) | Ask | De Newen: `build` preguntando antes de cada edición y comando (`edit`/`shell` en `ask`). Captura `ask-mode`: la escritura pidió permiso y se negó |

`mode_waives_prompts` es `build`: arranca un servidor de desarrollo sin tarjeta. Modelos: `provider/model` desde la opción `model`; esfuerzos desde `effort` (`thought_level`), solo del modelo actual y sin `default`.

## Herramientas

`tool_call` trae `name`, el nombre propio de OpenCode, y su input completo llega en `in_progress`:

| OpenCode | Newen |
|---|---|
| `shell` (`command`) | `Bash`; el título es el comando |
| `read` (`path`) | `Read` (también lee carpetas) |
| `edit` (`path`, `oldString`, `newString`) | `Edit`; al terminar trae el `diff` con el archivo entero antes y después |
| `write` (`path`, `content`) | `Write`; al terminar no trae diff (lo atribuye el repo sombra) |
| `patch` | `Edit` del primer archivo |
| `glob`, `grep` | `Glob`, `Grep` |
| `webfetch`, `websearch` | `WebFetch`, `WebSearch` |
| `todowrite` | `TodoWrite`; una tarea `cancelled` cuenta como hecha (no retiene un `wait_for_session`) |
| `subagent` (`task`) | `Agent` |
| `skill` (`id`) | `Skill` |
| `question` | la fila del tool; la pregunta en sí llega por `elicitation/create` (captura `ask`) |
| `execute` (Code Mode) | ver abajo |

**Code Mode.** OpenCode no ofrece las herramientas MCP como herramientas sueltas: el modelo escribe un script (`await tools.<servidor>.<tool>(…)`) en la herramienta `execute`. Un script que llama a una sola herramienta se dibuja como esa herramienta (`mcp__sessions__list_sessions`), con su input si está escrito como JSON; otro, como `execute`. Captura `host`: el modelo encontró y llamó `tools.sessions.list_sessions()` del servidor de Newen. Pendiente posible: pasar los servidores de Newen con `codemode: false` por `OPENCODE_CONFIG_CONTENT` para que sean herramientas sueltas.

**Subagentes.** Newen pide la extensión de OpenCode `_meta["opencode/child-session-updates"]`: el trabajo de un subagente va en notificaciones `opencode/session/child_update`, que se callan, en vez de mezclarse en la conversación.

**Permisos.** Opciones `once` / `always` / `reject`. Negar corta el paso del modelo: el turno termina `cancelled` (Newen lo muestra detenido, captura `deny`).

## Settings

| Opción | Por defecto | Apagada pone |
|---|---|---|
| Free models | sí | `OPENCODE_DISABLE_FREE_MODELS=1` |
| Claude Code skills (`~/.claude/skills`, `.claude/skills`) | sí | `OPENCODE_DISABLE_CLAUDE_CODE=1` |
| Project settings (`opencode.json`, `.opencode/`, sus servidores MCP) | sí | `OPENCODE_DISABLE_PROJECT_CONFIG=1` |

**Cuenta:** no se declara. OpenCode funciona sin cuenta (modelos gratuitos), y una pantalla de "Sign in" taparía la ventana; agregar una clave pide terminal (`opencode auth login`). Una sesión cuyo proveedor rechaza las credenciales responde `-32000` y Newen muestra `AuthRequired`.

## Customize

- **Conectores:** la lista sale de `opencode debug config --standalone`, las capas de su config con su archivo (fixture `debug-config.json`): un servidor es el de la última capa que lo define; la del directorio global es "user", el resto "project"; `disabled: true` es Off. No conecta a nadie (`checked: false`). `mcp list` y `api mcp.list` con `--standalone` responden vacío porque no esperan a los plugins (observado).
- **Agregar:** `opencode mcp add <nombre> [--global] (--url U [--header K=V]… | [--env K=V]… -- comando args…)`, con alcance "All my projects" (`--global`) o "This project". **Quitar:** OpenCode no tiene comando; Newen dice que no puede (se edita su `opencode.json`).
- **Skills y plugins de OpenCode:** no se listan. Sus listas (`api skill.list`, `plugin list`) también responden vacío en frío, y la única forma fiable es manejar un servidor de OpenCode por HTTP (ver la investigación). Los skills globales de Newen sí le llegan.

## Las herramientas de Newen

Como en Hermes: dos servidores MCP stdio en `session/new` (`sessions`, `browser`) que corren `newen --mcp-host` ([acp.md](acp.md)). Los servidores stdio de OpenCode heredan su entorno más el suyo.
