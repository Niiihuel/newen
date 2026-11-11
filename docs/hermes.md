# Hermes Agent en Newen (`newen-agent-hermes`)

Hermes Agent, de Nous Research, es el cuarto agente de Newen. Habla ACP con `hermes acp`; el protocolo común está en [acp.md](acp.md). Todo lo de Hermes vive en `crates/newen-agent-hermes`.

## Fuente de verdad

- **Código abierto:** `NousResearch/hermes-agent`, MIT, en Python 3.14. Leído y corrido en el commit `74b2ed6` de `main` (2026-11-03), posterior a la release `v2026.9.24` (`__release_date__`).
- **Versión:** `agentInfo.version` es el `baseVersion` del sello de instalación; un checkout sin sello dice `0.0.0`. `tested()` = `2026.9.24` y `0.0.0`. `hermes acp --version` imprime `git.74b2ed6`.
- **Instalación de prueba:** las dependencias de su `uv.lock` (extras `acp` y `mcp`) en un venv aparte; su instalador oficial también es desde el código.
- **Actualización** ([agents.md](agents.md), «El programa al día»): `hermes update` (`update_args`), que es lo que dice su documentación; sin probar, porque en esta máquina no está instalado *(inferido)*. Tampoco se vio qué imprime `hermes --version`: si no es el `baseVersion` de `agentInfo.version`, sus sesiones cuentan siempre como de otra versión, lo que solo cambia qué lista de modelos va primero. Sus modelos los dice solo una sesión.
- **Capturas** (2026-11-03): `crates/newen-agent-hermes/fixtures/2026.9.24/`. **El modelo es el de la captura** (`--fake-model`, un servidor compatible con OpenAI con un guion por escenario, como los tests de cable del propio Hermes): el protocolo ACP de Hermes es el real, lo que "decide el modelo" es conocido. No hay proveedor gratuito usable: los modelos gratuitos de OpenCode Zen responden "Free models work in OpenCode" fuera de OpenCode, y no se usaron credenciales de la persona. Escenarios: `simple`, `edit-bash`, `deny`, `always`, `todos`, `interrupt`, `resume`, `fork`, `plan`, `risky`, `host`.
- **Sin tocar la configuración de la persona:** `HOME` y `HERMES_HOME` en una carpeta de prueba, con `model: {provider: custom, base_url: http://127.0.0.1:<puerto>/v1}`, `approvals.mode: manual` y `model_catalog.enabled: false`.

## Proceso

`hermes acp` es un visor: el modelo, las herramientas, los MCP y los permisos corren en un **daemon** por `HERMES_HOME` (`gateway run --idle-exit`), que el primer `hermes acp` arranca y que se apaga solo tras 10 minutos sin uso. Un `hermes acp` por sesión de Newen; todos comparten ese daemon. Consecuencias:
- Una sesión solo se puede usar en el proceso que la abrió o la cargó: tras reiniciar, Newen la reanuda con `session/resume` (que repite la historia antes de responder: no se muestra otra vez, captura `resume`).
- Las variables que Newen pone al proceso no llegan al daemon salvo los interruptores de lanzamiento de Hermes (Settings), que pasan como política de cada sesión nueva.

## Modos

Los de su `modes`, cambiados con `session/set_mode` (solo entre turnos; Newen espera a que termine el que corre):

| Modo | Tono | Qué hace |
|---|---|---|
| Ask (`default`, por defecto) | Ask | Pregunta antes de cada edición, con el diff (captura `edit-bash`) |
| Accept edits (`accept_edits`) | Edits | Edita esta carpeta solo; pregunta por archivos sensibles (captura `plan`: no hubo tarjeta) |
| Don't ask (`dont_ask`) | Auto | Toda edición sola salvo archivos sensibles |

Los comandos riesgosos piden permiso en todos los modos según `approvals.mode` de la persona (`risky`: `rm -rf build` pidió, con "Allow for session"/"Allow always" como sugerencia). Modelos `proveedor:modelo` desde `models`/la opción `model`; esfuerzos desde `reasoning_effort` (`none` … `ultra`).

## Herramientas

Hermes nombra la herramienta solo en el título, "`<tool>: <qué hace>`", y no da input para las suyas: se lee de su ubicación y su texto.

| Hermes | Newen |
|---|---|
| `terminal` | `Bash`; el comando entero sale de su texto `$ …` (el título lo acorta, "mv notes.txt moved.txt + 1 command") |
| `read_file`, `write_file`, `patch` | `Read`, `Write`, `Edit` del archivo; el diff viene en la tarjeta de permiso, no en el resultado |
| `search_files`, `web_search`, `web_extract` | `Grep`, `WebSearch`, `WebFetch` |
| `delegate_task` | `Agent` |
| `skill_view` | `Skill` |
| `todo_list` | no se dibuja: su lista llega como `plan` y se dibuja como `TodoWrite` (captura `todos`) |
| `mcp__editor_<servidor>_<8 hex>__<tool>` (un servidor de la sesión) / `mcp__<servidor>__<tool>` | `mcp__<servidor>__<tool>` |

Las herramientas MCP y `todo_list` están detrás de su puente de búsqueda (`tool_search`, `tool_call`); el evento de la herramienta trae igual el nombre real. Sus tarjetas de permiso tienen ids propios (`perm-check-N`, `edit-approval-N`) que después se cierran con un `tool_call_update`: no se dibujan como herramientas.

**Rareza:** cuando agrega una nota a la respuesta (su verificador de ediciones fallidas), Hermes reenvía el mensaje entero con el mismo `messageId`; `Flavor::resends_words` toma solo lo nuevo (captura `deny`).

## Settings

| Opción | Por defecto | Pone |
|---|---|---|
| Project rules (AGENTS.md, SOUL.md, memoria, skills precargados) | sí | apagada: `HERMES_IGNORE_RULES=1` |
| Run hooks without asking | no | prendida: `HERMES_ACCEPT_HOOKS=1` |

No se ofrece `HERMES_YOLO_MODE`: es por sesión al crearla y no se aplica al reanudar. **Cuenta:** no se declara: el proveedor se elige con `hermes model` en una terminal (pide TTY), y un turno sin proveedor falla con el texto de Hermes que lo dice.

## Customize

- **Conectores:** `hermes mcp list` (lee `config.yaml`, no conecta; `checked: false`). Su tabla no tiene JSON: se lee por columnas separadas por espacios. Alcance único, "All my projects" (los servidores son del perfil).
- **Agregar:** `hermes config set mcp_servers.<nombre> '<json>'` con la entrada entera (`command`/`args`/`env`, o `url`/`headers`); su `mcp add` pregunta en la terminal y no toma encabezados. **Quitar:** `hermes config unset mcp_servers.<nombre>`. Verificados contra el CLI real en un home de prueba.
- **Skills y plugins de Hermes:** no se listan (sus listas no tienen JSON; `plugins list` descarga su catálogo cada vez).
- **Skills globales de Newen: no.** Hermes no tiene forma de sumar una carpeta de skills por proceso: `skills.external_dirs` es de su config y se lee en el daemon compartido. La fila del skill dice "Hermes cannot take skills from Newen".

## Las herramientas de Newen

Dos servidores MCP stdio en `session/new` (`sessions`, `browser`) que corren `newen --mcp-host` ([acp.md](acp.md)). Los arranca el daemon, con **solo** el entorno de la entrada (más PATH, HOME y pocos más): por eso la clave de la sesión y los secretos de los servidores globales van como valores (`inline_secrets`). Captura `host`: `list_sessions` respondió.
