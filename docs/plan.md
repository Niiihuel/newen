# Newen: plan de implementación

Derivado de [`spec.md`](spec.md). Cada tarea es un incremento chico que se puede verificar y termina con `cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test --workspace` en verde. Hay dos carriles: **Q** (Newen) y **L** (PRs a limn, cada uno desde un worktree `../limn-<tema>`). Las dependencias van entre corchetes.

## Fase 0: arranque

| ID | Tarea | Verificación |
|---|---|---|
| Q0.1 | Workspace con los crates vacíos de spec §3, `AGENTS.md`, `shell.nix`, perfiles de RUNE (`opt-level = 2` para dependencias) y limn por git (rama `quill/integration`, por ahora igual a `main`). | `cargo build --workspace` |
| Q0.2 | `apps/newen` abre una ventana limn sin borde, con barra propia, sidebar vacía y área central; arrastre y cambio de tamaño por el host (patrón `frame.rs` de RUNE). | Abre a 1408×844; `--screenshot` produce un PNG |
| Q0.3 | CI local: script `scripts/check.sh` (fmt, clippy, test). | El script pasa |

## Fase 1: verificar el protocolo con el CLI real

| ID | Tarea | Verificación |
|---|---|---|
| Q1.1 | `tools/capture`: cliente mínimo que lanza el CLI instalado con la línea de engine.md §2.4 en un repo temporal y guarda cada línea JSON in/out. | Corre un turno simple y deja un JSONL |
| Q1.2 | Capturar los escenarios de engine.md §11.3: turno simple, Edit + Bash con `rm`, allow/deny/always, AskUserQuestion, plan, steer, interrupt, `--resume`, `--fork-session`, `rewind_files`. Anonimizar y guardar en `crates/newen-protocol/fixtures/<versión-cli>/`. | Fixtures versionados sin datos personales |
| Q1.3 | Cerrar V1–V13 de engine.md §11.2 con esas capturas y actualizar engine.md. **Hecho 2026-10-25 (engine.md §11.4);** quedan V2, V3 (id de fork), V8 y el steer entre herramientas. | Tabla V con estado "verificado" o "difiere" |

## Fase 2: protocolo y motor (sin UI)

| ID | Tarea | Verificación |
|---|---|---|
| Q2.0 | `newen-agent`: trait `AgentAdapter`, `AgentCapabilities`, eventos normalizados y registro de adaptadores. El diseño se revisa contra Claude Code y contra un segundo CLI (Codex u OpenCode) solo en papel, para que nada sea específico de Claude. | Doc de mapeo de eventos de 2 CLIs + tests de tipos |
| Q2.1 | `newen-agent-claude`: tipos de mensajes y control con fallback `Unknown`, codec por línea, `LaunchSpec` y mapeo a eventos normalizados. [Q1.2, Q2.0] | Ida y vuelta de todos los fixtures |
| Q2.2 | CLI falso (binario de test que reproduce un guion stream-json). | Test que lo lanza y lee el guion |
| Q2.3 | `newen-engine` (independiente del agente) + `newen-agent-claude`: transporte del proceso, `ControlMux` (request_id ↔ respuesta), `initialize` y un turno simple → `Event`s. [Q2.1, Q2.2] | Test: turno simple con el CLI falso |
| Q2.4 | Permisos: `can_use_tool` → `Event::PermissionRequested` → `Intent::Decide` (allow once/always/deny). [Q2.3] | Tests de allow, deny y abort |
| Q2.5 | Cola, steering (`next`/`later`), `interrupt` y escalado SIGTERM → SIGKILL; crash antes de `init`; stream sin `result`; guard de crash-loop. [Q2.3] | Tests con guiones del CLI falso |
| Q2.6 | `newen-store`: esquema y migraciones, escritor por lotes, sesiones/turnos/eventos; persistir antes de notificar. [Q2.3] | Test: reabrir la base y reconstruir el timeline |
| Q2.7 | Reanudación con `--resume` y fork con `--fork-session`. [Q2.6, Q1.3] | Test con el CLI falso y verificación manual con el CLI real |

## Fase 3: UI con datos simulados

| ID | Tarea | Verificación |
|---|---|---|
| Q3.1 | `newen-components::Look` (tokens de ui.md §3), claro y oscuro. | Test de tokens |
| Q3.2 | `newen-state` + `MockEngine` que reproduce fixtures de eventos; `--fixture <nombre>`. | `cargo run -p newen -- --fixture permission` |
| Q3.3 | Shell: sidebar, columna central y panel derecho, con las reglas de cesión de ancho de surco. | Métricas de geometría (±4 px) |
| Q3.4 | Sidebar de sesiones: grupos por carpeta, estados (running, awaiting, ready, error) con punto, y vacío "No sessions…". [L3 o un punto provisional] | Fixture + métricas de geometría |
| Q3.5 | Transcript: burbuja de usuario, texto en streaming, `ToolRow` colapsable y `TurnSummary` (+N −M). | Fixtures `turn-done`/`turn-expanded` |
| Q3.6 | Composer y chips: carpeta, worktree, modo, modelo y esfuerzo con sus menús. | Fixtures `model-menu`/`effort-menu`/`mode-menu` |
| Q3.7 | `PermissionCard` con Deny, Allow once y Always allow, atajos Ctrl+Enter y Ctrl+Shift+Enter. [L2] | Fixture `permission`/`permission-menu` + test de teclado |
| Q3.8 | Panel derecho: Environment, Context y Changes. [L3 para `Progress::ring`] | Métricas de geometría |
| Q3.9 | Prueba temprana de IME y acentos en el composer (Wayland y X11); si falla, issue o PR a limn. | Resultado documentado |

## Fase 4: integración motor ↔ UI

| ID | Tarea | Verificación |
|---|---|---|
| Q4.1 | `apps/newen` conecta `Engine` real: `Intent` → motor, `Envelope<Event>` → UI con despertar de la ventana. [Q2.*, Q3.*] | Sesión real de punta a punta en un repo de prueba |
| Q4.2 | Nueva sesión: diálogo de carpeta, confianza del workspace, modelo, esfuerzo y modo. | Criterio A1 |
| Q4.3 | Reanudar sesiones al iniciar desde SQLite. | Criterio A9 |

## Fase 5: archivos, atribución y diff

| ID | Tarea | Verificación |
|---|---|---|
| Q5.1 | `newen-git`: wrapper de `git`, estado, `numstat`/`name-status` y parser unified. | Tests sobre repos temporales |
| Q5.2 | Repo sombra: snapshot al abrir cada turno y tras cada herramienta que escribe (hook `PostToolUse` vía `hook_callback`); efectos por petición en `newen-store`. [Q2.6] | Test: Edit, `rm` y `mv` por Bash quedan atribuidos a su petición (A5) |
| Q5.3 | Vista "qué hizo esta petición": archivos, comandos y herramientas. | Criterio A5 |
| Q5.4 | Rewind por petición con vista previa. | Test sobre un repo temporal (A6) |
| Q5.5 | Panel de diff "rama → working tree". [L4 para palabra/expansión] | Criterio A7, métricas de geometría |

## Fase 6: terminal

| ID | Tarea | Verificación |
|---|---|---|
| Q6.1 | `newen-pty`: PTY por pestaña, buffer en anillo, parser VT → `TerminalScreen`, reconexión con reproducción. | Tests: `echo` y salida de código |
| Q6.2 | `TerminalPane` con pestañas y cierre con confirmación si hay un comando corriendo. [L1] | Criterio A8, métricas de geometría |
| Q6.3 | SplitView chat \| diff / terminal. [L5] | Métricas de geometría |

## Carril L: PRs a limn (en paralelo; prioridad de ui.md §4.1)

| ID | PR | Lo necesita |
|---|---|---|
| L1 | `feat(terminal): TerminalView` (P0) | Q6.2 |
| L2 | `feat(button): SplitButton` | Q3.7 |
| L3 | `feat(status): StatusDot with pulse, and Progress::ring` | Q3.4, Q3.8 |
| L4 | `feat(code-diff): word highlights, expandable context, margin bar and file header` | Q5.5 |
| L5 | `feat(layout): SplitView` | Q3.3 (opcional), Q6.3 |
| L6 | `feat(window-controls): round Linux style` (P2) | Q0.2 (opcional) |
| L7 | `feat(button-group): stack below a width` (P2) | Q3.7 (ventanas angostas) |

Cada PR lleva tests sin ventana, documentación en `docs/components/`, una entrada en el changelog y una página en la galería. Cuando se abre, se suma a la rama `quill/integration`; cuando se mezcla en `main`, Newen vuelve a `main`.

## Orden sugerido

Q0 → Q1 → (Q2 ∥ Q3 ∥ L2, L3) → Q4 → (Q5 ∥ L4) → (L1, L5) → Q6.

Después del MVP: un adaptador por proveedor (`newen-agent-codex`, `-opencode`, `-grok`, `-antigravity`…), cada uno con su fase de captura del protocolo (como Q1), fixtures, CLI falso y tests de contrato.

Primer hito demostrable: Q4.1. Una sesión real con transcript, permisos y sidebar, sin diff ni terminal.
