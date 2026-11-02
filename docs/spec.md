# Newen: especificación

**Estado:** borrador para revisión · **Fecha:** 2026-10-25

Newen es una app de escritorio nativa, en Rust con [limn](https://github.com/Niiihuel/limn), para trabajar con **agentes de código de cualquier proveedor**: la persona elige el CLI (Claude Code, Codex, Antigravity, OpenCode, Grok…) y Newen lo maneja mediante un adaptador. El primer adaptador, y el del MVP, es **Claude Code**. La experiencia se inspira en **Code** de Claude Desktop: toma comportamientos, protocolos observables de los CLI y estructura visual, descritos por lo que la persona ve. No usa código, fuentes, íconos ni marca de Anthropic ni de otros proveedores.

Documentos de detalle:
- [`engine.md`](engine.md): motor de agente (proceso del CLI, protocolo, permisos, atribución, persistencia, git, PTY).
- [`ui.md`](ui.md): UI con limn (arquitectura, zonas medidas, tokens, PRs a limn, workspace, pruebas).
- [`plan.md`](plan.md): tareas en orden.

Ambos documentos son borradores. Si algo choca con este documento, manda este.

## 1. Objetivo y no objetivos

**Objetivo del producto:** un solo entorno para cualquier agente de código por CLI, con la misma UI, el mismo registro por petición y el mismo rewind, sin importar el proveedor.

**Objetivo del MVP:** con el adaptador de Claude Code, abrir una carpeta local, conversar con el agente en una sesión y ver, de cada petición, qué herramientas usó, qué comandos corrió y qué archivos creó, modificó o borró, con aprobación de permisos, diff y terminal. Las sesiones se reanudan después de reiniciar la app.

**No objetivos del MVP:** adaptadores distintos de Claude Code (la arquitectura ya los contempla; ver D11), Chat contra la API, Cowork y VM, Browser/preview, artefactos, MCP propio, SSH/WSL/teleport, tareas programadas, i18n (MVP en inglés, textos centralizados), macOS y Windows (MVP en Linux) y distribución pública.

## 2. Decisiones

| # | Decisión | Por qué |
|---|---|---|
| D1 | Newen habla **directamente con cada CLI** por su protocolo nativo, sin SDKs de otros lenguajes. Para Claude Code, el adaptador `newen-agent-claude` usa stream-json (stdin/stdout) y el protocolo de control, y reimplementa la capa delgada del Agent SDK de TypeScript. | Evita Node en el proceso y mantiene todo en Rust. |
| D2 | Newen **no se presenta como otro cliente**: con Claude Code no usa `CLAUDE_CODE_ENTRYPOINT=claude-desktop` ni `client_platform: desktop_app`, y vale lo mismo para cada proveedor. La autenticación es el login propio de cada CLI. | Honestidad del cliente; engine.md §11.1. |
| D3 | **Rango de versiones probado por adaptador** (piso y techo), fixtures por versión y tipos con fallback `Unknown`. | Ninguno de estos protocolos es una API pública estable. |
| D4 | **Atribución por petición independiente del agente:** un `request_id` propio, más **snapshots en un repo git sombra** (`$XDG_DATA_HOME/newen/snapshots`) al abrir cada turno y, si el adaptador expone hooks de herramienta (Claude Code: `PostToolUse`), tras cada herramienta que escribe. | Ni el CLI ni Bash dicen qué archivos cambió cada petición (engine.md §6). Newen registra creados, modificados, borrados y renombrados por petición con cualquier CLI, y ofrece un rewind real. |
| D11 | **Adaptadores de agente.** `newen-agent` define el trait `AgentAdapter`, las `AgentCapabilities` (modelos, niveles de esfuerzo, modos de permiso, hooks por herramienta, resume, fork, steering, interrupt) y los eventos normalizados (texto, pensamiento, herramienta y resultado, permiso, uso, fin de turno). Cada CLI tiene su crate `newen-agent-<id>` (`claude` en el MVP; después `codex`, `opencode`, `grok`, `antigravity`…). El motor, el store, el git sombra y la UI solo conocen `newen-agent`. La UI arma los selectores desde las capacidades. | Agregar un proveedor no toca la UI ni el motor; los nombres genéricos no llevan "claude". |
| D5 | **SQLite como fuente de la UI** (`newen-store`). El JSONL del CLI se usa solo a través de `--resume`, y el fork con `--fork-session`. | El formato del JSONL es interno del CLI. |
| D6 | **Un actor Tokio por sesión**. Un gobernador limita los procesos a `max(6, RAM/3 GiB)`; un guard detiene la sesión si se cae en bucle; para detenerla, `interrupt` → SIGTERM → SIGKILL al grupo. | Ver engine.md §1, §5. |
| D7 | **Capas solo hacia abajo** (modelo RUNE): la UI no depende del motor; se comunican por `newen-model` (`Event` hacia la UI, `Intent` hacia el motor). El shell y el bucle de cuadros siguen a surco. | Testeable sin ventana ni CLI, y reemplazable por un `MockEngine` que reproduce fixtures. |
| D8 | Todo lo genérico que falte en limn va **como PR a limn**, un worktree por tema (`../limn-<tema>`). Newen depende de la rama `quill/integration` de limn hasta que se mezcle. | Política del proyecto (ui.md §4–5). |
| D9 | Git a través del binario `git` (sin git2). `gix` queda como opción a futuro. | Simple y con el mismo comportamiento que el git del usuario. |
| D10 | Tokens visuales propios: Inter y JetBrains Mono, acento propio. Las medidas salen de las capturas, a 1408×844. | Fuentes e íconos de Anthropic son propietarios (ui.md §3). |

## 3. Arquitectura

```text
apps/newen ──► newen-ui ──► newen-components ──► limn
     │            │   └──► newen-state ──────────┐
     │            └──────────────────────────────┴──► newen-model
     ├──► newen-engine ──► newen-agent, newen-store, newen-git, newen-pty ──► newen-model
     │         └──► newen-agent-claude (y futuros newen-agent-<id>) ──► newen-agent
     └──► limn-egui (solo el host)
```

| Crate | Responsabilidad |
|---|---|
| `newen-model` | Tipos de dominio e IDs (`AgentId`, `SessionId`, `RequestId`…); contratos `Event`, `Intent`, `Envelope`, `EventSink`. Sin IO. |
| `newen-agent` | Trait `AgentAdapter` (lanzar, enviar, interrumpir, decidir permiso, reanudar, fork), `AgentCapabilities` y eventos normalizados. Registro de adaptadores disponibles. Sin IO de procesos. |
| `newen-agent-claude` | Adaptador de Claude Code: mensajes stream-json y de control (serde), codec por línea, `LaunchSpec` (flags y entorno), mapeo a eventos normalizados, hooks `PreToolUse`/`PostToolUse`. |
| `newen-engine` | `Engine` y actores de sesión, independientes del agente: proceso, `PermissionBroker`, cola y steering, `TurnTracker`, gobernador. Llama al adaptador de la sesión. También `MockEngine` (reproduce fixtures). |
| `newen-store` | SQLite (rusqlite bundled y migraciones): sesiones, turnos, eventos, efectos por petición y blobs. |
| `newen-git` | Wrapper de `git`, estado, diff y parser unified → líneas, repo sombra (snapshot/restore/diff) y worktrees. |
| `newen-pty` | PTY por pestaña (`portable-pty`), parser VT → `TerminalScreen`, OSC 133 y PTY oculto para `!`. |
| `newen-state` | `Store` de la UI: aplica `Event` y deriva el estado de filas, sesiones y turnos. |
| `newen-components` | `Look` (tokens) y piezas puras props → `Element` (UserBubble, ToolRow, TurnSummary, PermissionCard…). |
| `newen-ui` | `Ui`, `view`, `update`, atajos; tests en los que el test hace de host. |
| `apps/newen` | Ventana (eframe + wgpu vía `limn-egui`), marco sin borde, bucle de cuadros, `--fixture` y `--screenshot`. |

## 4. Alcance del MVP y criterios de aceptación

Cada criterio se verifica con un test (CLI falso o fixture) y, donde hay captura, con métricas de geometría a 1408×844 contra ella.

| # | Funcionalidad | Criterio de aceptación | Referencia |
|---|---|---|---|
| A1 | Nueva sesión | Elegir carpeta, confirmar la confianza del workspace, elegir modelo, esfuerzo y modo de permisos, y enviar. El CLI arranca en esa carpeta y aparece la sesión en la sidebar con su estado. | |
| A2 | Transcript en streaming | El texto llega incremental. Las herramientas se ven como filas colapsables (Read, Edited +N −M, Ran) y el turno terminado como resumen colapsado. | |
| A3 | Permisos | Tarjeta "Allow Claude to …?" con Deny, Allow once (Ctrl+Enter) y Always allow (Ctrl+Shift+Enter). La sidebar marca la sesión que espera input. Modos Manual, Accept edits y Plan. | |
| A4 | Interrupción y cola | Stop interrumpe el turno. Lo enviado durante un turno queda en cola, o se inyecta como steer según la prioridad. | engine.md §5 |
| A5 | Atribución por petición | Por cada mensaje del usuario se ven y se persisten los archivos creados, modificados, borrados y renombrados (también los hechos por Bash), los comandos con su código de salida y las herramientas con su resultado. | engine.md §6 |
| A6 | Rewind | Volver los archivos al estado anterior a una petición, con vista previa de lo que cambia. | engine.md §6.4 |
| A7 | Diff | Panel "rama → working tree", unified, con encabezado de archivo y +N −M. Resaltado por palabra cuando se mezcle el PR de `CodeDiff`. | |
| A8 | Terminal | Panel de terminal con pestañas en la carpeta de la sesión; reconexión con reproducción del buffer. Depende del PR `TerminalView`. | |
| A9 | Persistencia y reanudación | Al reiniciar la app se ven las sesiones y sus transcripts desde SQLite, y la sesión se continúa con `--resume`. | engine.md §7 |
| A10 | Panel derecho | Environment (carpeta, rama), Context (% de uso) y Changes (+N −M, Commit changes). | |

## 5. Riesgos principales

1. El protocolo de un CLI cambia sin aviso. Mitigación: D3, tests de contrato contra el CLI instalado, adaptador por proveedor.
1b. Los CLIs no exponen lo mismo (permisos interactivos, hooks, fork). Mitigación: `AgentCapabilities` y degradación explícita en la UI. Por ejemplo, sin hooks la atribución se hace por turno y no por herramienta.
2. Snapshots lentos en repos grandes. Mitigación: hook con timeout, caché de stat y snapshot solo por turno como respaldo (engine.md §11.1).
3. limn no tiene terminal ni split view. Mitigación: PRs P0/P1 en paralelo; mientras tanto, la salida de comandos se muestra con `OutputBlock`.
4. IME y acentos en el composer sin probar en limn. Mitigación: probarlo temprano y reportarlo a limn.
5. 18 puntos del protocolo sin verificar (engine.md §11.2, V1–V18). Mitigación: la tarea de captura de fixtures es la primera del plan.

## 6. Decisiones de revisión (2026-10-25)

| Tema | Decisión |
|---|---|
| Sidebar | Como en Claude Desktop: redimensionable entre 200 y 420 px y colapsable; ancho inicial el observado en Code (≈212 px a 1408×844). |
| Panel derecho | En flujo; colapsa por debajo de ~1200 px. |
| Tarjeta de permiso | Inline en el transcript, como en la captura. |
| Acento | Índigo `#3B5BDB`. |
| Modo "Auto" (clasificador de Desktop) | Fuera del MVP; solo Manual, Accept edits y Plan. |
| Idioma | Inglés en el MVP, textos en `newen-ui/src/text.rs`. |
| Proveedores | Multi-CLI por adaptadores (D11); el MVP implementa solo Claude Code. |
