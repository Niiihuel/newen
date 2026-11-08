# Antigravity CLI en Newen (`newen-agent-antigravity`)

Antigravity CLI, de Google (binario `agy`), es el sexto agente de Newen. Habla ACP con `agy acp`; el protocolo común está en [acp.md](acp.md). Todo lo de Antigravity vive en `crates/newen-agent-antigravity`.

## Fuente de verdad

- **No es código abierto:** `google-antigravity/antigravity-cli` publica solo README, CHANGELOG y ejemplos, sin licencia; el binario es Go. Lo que dice este documento sale de su `--help`, su CHANGELOG, su documentación (antigravity.google/docs) y las capturas.
- **Versión:** `agentInfo.version`. `tested()` = `1.5.3`.
- **Instalación:** `curl -fsSL https://antigravity.google/cli/install.sh | bash` (verifica el sha512 del manifiesto) deja `agy` en `~/.local/bin`, que Newen ya suma al `PATH`.
- **Capturas** (2026-11-08, `crates/newen-agent-antigravity/fixtures/1.5.3/`): Gemini 3.8 Flash con la cuenta de Google de la persona. Escenarios: `simple`, `edit-bash`, `deny`, `always`, `ask`, `interrupt`, `resume`, `plan`, `unsafe`, `host`. `todos` no se guarda: Antigravity no tiene herramienta de tareas (lleva su lista en un archivo propio) y el modelo terminó explorando la máquina. En `plan` se reemplazaron el usuario y la carpeta de la captura en los textos.
- **Sin tocar la configuración de la persona:** `HOME` en una carpeta de prueba; la cuenta está en el llavero del sistema, que sigue disponible.

## Proceso y cuenta

Un `agy acp` por sesión. Sin cuenta, `initialize` responde y `session/new` da `-32000`: Newen lo muestra como cuenta requerida. La cuenta es la de `agy` (Google en el navegador, Gemini Enterprise, clave de la API de Gemini o Vertex AI), o `agy acp --login` en una terminal; Newen no la maneja.

## Modos

Los de sus `modes` (también en `configOptions`), con `session/set_mode`:

| Modo | Tono | Qué hace |
|---|---|---|
| Default (`default`, por defecto) | Ask | Pregunta antes de los comandos según la configuración de permisos de la persona; edita sin preguntar (capturas `edit-bash`, `deny`) |
| Accept edits (`accept-edits`) | Edits | Edita el espacio de trabajo sin preguntar |
| Plan (`plan`) | Plan | Escribe un plan para revisar y termina el turno |
| Skip permissions (`skip-permissions`) | Unsafe | `agy acp --dangerously-skip-permissions`. Nunca pregunta (captura `unsafe`: un comando sin tarjeta). Solo con "Allow bypass permissions mode" en Settings |

**Skip permissions es un interruptor de lanzamiento** (`AgentAdapter::launch_mode`): entrar o salir de él vuelve a lanzar `agy acp` sobre la misma conversación (`session/resume`), después del turno en curso. Lanzado así, ACP sigue diciendo `default`, y el adaptador lo informa como `skip-permissions`. Modelos (Gemini, Claude, GPT-OSS) y esfuerzos (`low` … `high`) de `configOptions`.

## Herramientas

Su input tiene las claves en mayúscula, y lo que no nombra por tipo es "Run `<tool>`":

| Antigravity | Newen |
|---|---|
| `execute` `{CommandLine, toolSummary}` | `Bash` |
| `read` `{Path, StartLine, EndLine}` | `Read` |
| `edit` `{Path, Content}` | `Write` |
| `edit` `{Path, Target, Replacement}` | `Edit` |
| `edit` con `ArtifactMetadata.RequestFeedback: true` | `ExitPlanMode` con su `Content`: el plan para revisar, que la tarjeta lleva a "Implement in …" (captura `plan`) |
| "Run mcp_`<servidor>`_`<tool>`" | `mcp__<servidor>__<tool>` (el servidor más largo de la sesión con ese prefijo) |
| "Run ask_question" | nada: es la pregunta de abajo |
| "Run `<tool>`" | `<tool>` |

## Preguntas

`elicitation/create` en modo `form`: cada propiedad es una pregunta, con `oneOf` de opciones y un campo `<clave>_other` para palabras propias (el núcleo lo toma como el de `<clave>_custom`). Se contesta `{action: "accept", content: {q1: "1"}}` (captura `ask`).

## Herramientas de Newen

`session/new` toma servidores MCP stdio (`SessionExtras::host`; no declara HTTP ni SSE): los tres de Newen. El modelo los ve como `mcp_<servidor>_<tool>` (captura `host`: `list_sessions`, sin tarjeta).

## Opciones (Settings)

Para las sesiones nuevas, como interruptores de `agy acp` que van solo cuando dejan su valor por defecto: **Sandbox** (`--sandbox`, terminal restringida; por defecto no) y **Usage data** (`--disable-telemetry` al apagarla; por defecto sí). Verificado con el 1.5.3: arranca y abre sesión con las dos.

## Logo

La forma de su marca de svgl.app (`antigravity.svg`, la silueta de su máscara, sin el degradé) en `currentColor` (`assets/antigravity.svg`): solo donde Newen nombra a sus agentes (AGENTS.md).

## Customize

- **Conectores:** `agy mcp list` (tabla `NAME TYPE STATUS COMMAND/URL`, sin JSON). Agregar: `agy mcp add [-e K=V]… [-H "N: v"]… [-t http] <nombre> -- <comando o url> [args…]` (sus opciones van antes del nombre). Quitar: `agy mcp remove <nombre>`. Un solo alcance, "All my projects" (`~/.gemini/config/mcp_config.json`).
- **Skills y plugins:** no se listan.

## Lo que no incluye

- **Cuenta en Newen:** se inicia con `agy` o `agy acp --login` en la terminal.
- **Fork:** no tiene `session/fork`; un fork de Newen abre una sesión que la reanuda.
- **Lista de tareas:** la lleva en un archivo propio (`todo.md` en su carpeta `brain`), que se ve como ediciones de ese archivo.
