# quill · Especificación — UI con limn (borrador)

> **Nombres (2026-10-25):** quill es multi-agente. Lo que este borrador llama `quill-protocol` es el adaptador `quill-agent-claude`; lo genérico va en `quill-agent` (trait `AgentAdapter` + `AgentCapabilities`) y en `quill-engine`, sin nombres de proveedor. Ver spec.md D11.

> Borrador. Resumen y decisiones finales: [`spec.md`](spec.md).

**Estado:** borrador 0.1 · **Fecha:** 2026-10-25 · **Alcance:** MVP = surface "Code"
(shell, sidebar de sesiones, transcript, composer, tarjeta de permisos, panel
derecho, panel de diff, terminal; Settings mínimo).

## 0. Reglas y fuentes

| Regla | Detalle |
|---|---|
| Sin activos de Anthropic | No se usan fuentes Anthropic Sans/Serif/Mono, íconos, mascota (el cangrejo pixelado de las capturas), CSS ni textos de marca. Se reconstruyen layout, jerarquía, estados y medidas. |
| Íconos | Los de limn: Bootstrap Icons (MIT) mapeados por `Icon::*`; un pack propio vía `limn_egui::set_icon_pack` si hace falta. |
| Toda la UI es limn | Como en RUNE: sólo `apps/quill` toca eframe/egui, y sólo para la ventana. |
| Si limn no tiene algo | **Todo lo que haya que agregar, mejorar o refactorizar en limn es un PR a limn** (`/home/user/projects/personal/lienzo`), un worktree por cambio en `../limn-<tema>`, con tests, docs, changelog y galería (`docs/contributing.md` de limn). Nunca un parche, una copia ni un widget genérico reimplementado dentro de quill. quill sólo **compone** componentes de limn que ya existen. Mientras el PR no se mezcla, quill apunta su dependencia a la rama del PR (§5). |
| Referencias | **surco** (`/home/user/projects/personal/surco`, reproductor con limn): referencia principal de shell (sidebar + contenido + panel derecho), de organización de estado y vistas, y del host. **RUNE** (`/home/user/projects/personal/rune`): referencia de estructura de workspace y de contratos con el motor. |
| Medidas | **Todas las medidas en px son de captura** (1408×844, tema claro, escala 1, fuente de sistema DejaVu-like), con error ±2 px. No son tokens del producto original. |
| Tema oscuro | **No hay capturas oscuras**: la paleta oscura de §3 es propuesta, no medida. |

---

## 1. Arquitectura de la UI

### 1.1 Modelo (limn)

| Pieza | En quill | Dueño |
|---|---|---|
| Estado de dominio | `quill_state::Store`: sesiones, turnos, ítems del transcript, pedidos de permiso, git, terminales. | host |
| Estado de interfaz | `quill_ui::Ui`: sesión abierta, paneles visibles y sus tamaños, borradores, menús abiertos, colapsados, ajustes. | host |
| Vista | `quill_ui::view(&Ui, &Store, &Look, &ViewContext) -> Element`. Función pura, reconstruida cada cuadro. | — |
| `UiTree` | Uno por ventana. Guarda foco, hover, scroll, texto de inputs y transiciones **por ID**. | `apps/quill` |
| Acciones | `UiAction` (`Clicked`, `InputChanged`, `Submitted`, `Drag*`, `CopyRequested`…) → `quill_ui::update` → `Vec<Intent>`. | — |
| Efectos | `Intent`: todo lo que la UI quiere hacer fuera de sí misma. Lo ejecuta el host. | `apps/quill` |
| Motor | `quill-engine`: runtime Tokio propio (2 hilos), procesos del agente, git, PTYs. Emite `Event` por un `EventSink` que despierta la UI. | `quill-engine` |

Componentes **controlados**: el valor llega en cada cuadro (`expanded`, `open`,
texto del composer) y el componente sólo reporta la intención. Si `update` la
ignora, nada cambia en pantalla.

**Cómo lo organiza surco (modelo a seguir para estado y vistas):**

| surco (`crates/app/src`) | Qué hace | En quill |
|---|---|---|
| `state.rs`: `State::update(&UiAction) -> Vec<Command>` y `State::apply(Event) -> Vec<Command>` | Un solo lugar muta el estado; ambas entradas devuelven órdenes para el worker. | `quill_ui::update` / `on_applied` devuelven `Vec<Intent>` (mismo patrón; `Store` y `Ui` separados como en RUNE porque el dominio de quill es más grande). |
| `view.rs`: `view(&State, Size) -> Element`; `window_view` arma el shell y luego **envuelve** el árbol con cada diálogo (`confirm_dialog(state, update_offer(state, …))`) y `Toast::layer` | Los overlays son funciones `fn(state, tree, width) -> Element`, no estado del árbol. | Igual: `settings_modal(ui, tree)`, `trust_dialog(ui, tree)`, `Toast::layer(tree, toasts, offset)`. Un archivo por zona: `view/sidebar.rs`, `view/transcript.rs`, `view/composer.rs`, `view/panel.rs`, `view/diff.rs`, `view/terminal.rs`. |
| `backend.rs::spawn(ctx, …) -> (UnboundedSender<Command>, Receiver<Event>, watch::Receiver<PlayerView>)` | Runtime Tokio de 2 hilos en su propio hilo; eventos por `mpsc` + `wake(ctx)`; lo que cambia continuamente (posición del reproductor) es un `watch`, no un flujo. | `quill-engine::spawn` con el mismo trío: `Intent` hacia el motor, `Envelope<Event>` hacia la UI, y `watch` para valores continuos (estado "Thinking… Ns", % de contexto, snapshot de la terminal activa). |
| `lib.rs`: `fn ui` hace layout → eventos → `state.update` → `send(commands)` → `set_root` → layout → paint; `request_repaint_after` sólo si algo cambia | Ciclo de cuadro. | §1.3. |
| `ScalarTransition` en el estado (`library_slide`, `panel_slide`) | Las animaciones de apertura de paneles son estado del host, leído en `view` con `value(now)`. | `Ui.sidebar_slide`, `Ui.panel_slide`, `Ui.diff_slide`. |
| Tests en `lib.rs`: `UiTree::new(view(&state, size))`, `set_reduced_motion(true)`, clic por rect, `state.update(&action)` | Pruebas de pantalla sin ventana. | §6. |
| `--screenshot <png> --after <ms>`, `--search`, `--play` | Conducir la ventana desde la línea de comandos. | `--fixture <nombre> --screenshot <png>` (§6.2). |

### 1.2 Capas (sólo hacia abajo, como RUNE)

```text
apps/quill ──► quill-ui ──► quill-components ──► limn
     │            │                └──► quill-model
     │            └──► quill-state ──► quill-model
     ├──► quill-engine ──► quill-model, quill-git, quill-pty
     └──► limn-egui (sólo el host)
```

- `quill-ui` no conoce `quill-engine` ni ningún adaptador: pide cosas con `Intent`.
- `quill-components` son funciones puras props → `Element`, probadas sin ventana.
- Nada bloqueante en el hilo de UI: procesos, git, disco y PTY viven en `quill-engine`.

### 1.3 Ciclo de un cuadro (`apps/quill/src/main.rs`, calcado de surco `crates/app/src/lib.rs::ui` y RUNE `App::ui`)

| # | Paso | Detalle |
|---|---|---|
| 0 | Fuentes | Una vez: `limn::set_fonts(...)` (§3.3) y `limn_egui::install_fonts(&ctx)`; luego `attach_fonts(&mut tree, &ctx)`. |
| 1 | Drenar | Hasta N=256 `Envelope` del motor → `Store::apply(event) -> Applied`; luego los "últimos valores" (ver 1.4). Una ráfaga nunca bloquea un cuadro. |
| 2 | Reacciones | `quill_ui::on_applied(&mut Ui, &Store, &Applied) -> Vec<Intent>` (p. ej. abrir el panel de diff al terminar un turno si el usuario lo pidió, notificar). |
| 3 | Entrada | En orden: atajos del host (ventana) → `quill_ui::intercept(&mut Ui, &Store, &UiEvent)` (teclas hacia la terminal enfocada, keybinds globales) → `tree.handle_event` → `quill_ui::update` por cada `UiAction`. |
| 4 | Vista y layout | `tree.set_root(view(..)); tree.layout(size)`; aplicar foco pedido (`tree.focus(id)`) después de un layout que contenga el control. |
| 5 | after_layout | `quill_ui::after_layout(&mut Ui, &Store, &UiTree)`: marcar leída la sesión si se ve el final, pedir más historial, medir anclas. |
| 6 | Pintar | `limn_egui::paint_at`; `limn_egui::set_cursor(ctx, tree.cursor())`; repintar sólo si `tree.animation_repaint_after()` o si hay eventos pendientes. |
| 7 | Ejecutar | El host despacha los `Intent` acumulados a `quill-engine` (canal acotado) o a la plataforma (portapapeles, diálogo de carpeta, abrir URL). |

### 1.4 Motor asíncrono → UI

Contrato tomado de RUNE (`rune-provider::EventSink`) y de surco (`backend.rs::spawn`: `mpsc` + `wake` + `watch` para valores continuos), con tipos propios:

| Entrega | Uso en quill | Semántica |
|---|---|---|
| `send(Event)` | Ítems del transcript, herramientas, permisos, fin de turno, errores. | Nunca se pierde; cola acotada (1024): el productor espera si está llena. |
| `send_latest(key, Event)` | Delta de texto en streaming por turno, estado de "Thinking… Ns", % de contexto, git stat, tamaño del PTY. | Se guarda sólo el más nuevo por clave. |
| `send_lossy(Event)` | Latidos ("Running a command…"), progreso de herramienta. | Se descarta si la cola está llena. |
| `wake()` | `ctx.request_repaint()` | Cada evento despierta la UI. |
| `Envelope { generation, event }` | Al reiniciar/cambiar el proceso de una sesión se sube `generation`; el `Store` descarta lo viejo. | — |

`Event` mínimo del MVP (en `quill-model`):

| Evento | Efecto en `Store` | Zona |
|---|---|---|
| `SessionsListed(Vec<SessionSummary>)` | Lista de sidebar. | Sidebar |
| `SessionStatus { id, status }` (`Idle`, `Running`, `Awaiting`, `Ready`, `Error`) | Punto de estado. | Sidebar, título |
| `TurnStarted { session, turn }` | Línea de estado "Thinking…". | Transcript |
| `TextDelta { session, turn, block, text }` | Agrega al bloque; markdown en `streaming(true)`. | Transcript |
| `ThinkingDelta { … }` | Bloque de pensamiento colapsado. | Transcript |
| `ToolStarted { id, kind, title, target }` / `ToolOutput { id, chunk }` / `ToolFinished { id, outcome, diff_stat }` | Fila de herramienta y su cuerpo. | Transcript |
| `TodosUpdated { session, items }` | Tarjeta de tareas. | Transcript |
| `PermissionRequested { request_id, session, tool, title, detail, command, options }` | Tarjeta de permiso; sesión → `Awaiting`. | Transcript, sidebar |
| `PermissionResolved { request_id, decision }` | Retira la tarjeta. | Transcript |
| `TurnFinished { turn, summary, stats, changed_files }` | Resumen colapsado del turno, `+N −M`. | Transcript, panel derecho |
| `TurnFailed { turn, reason, retryable }` | `Notice` de error en el turno. | Transcript |
| `GitStatus { session, base, head, files, stat }` / `FilePatch { file, lines }` | Panel derecho "Changes", panel de diff. | Diff |
| `ContextUsage { session, used, capacity }` | Medidor de contexto. | Panel derecho |
| `TerminalOutput { term, bytes }` → parseado en `quill-pty` a `TerminalScreen` (snapshot) con `send_latest` | Grilla visible. | Terminal |

`Intent` mínimo (en `quill-ui`):

| Intent | Origen |
|---|---|
| `NewSession { folder, worktree, mode, model, effort, prompt }` | Composer de "New session" |
| `Send { session, text, attachments }` / `Stop { session }` / `Queue { session, text }` | Composer |
| `SetMode` / `SetModel` / `SetEffort { session, … }` | Menús del composer |
| `Decide { request_id, decision: Once \| Always \| Deny }` | Tarjeta de permiso |
| `OpenFolderDialog` / `TrustWorkspace(path)` | Chip de carpeta |
| `LoadPatch { session, file }` / `RefreshGit { session }` | Panel de diff |
| `TermSpawn { session, cwd }` / `TermInput { term, bytes }` / `TermResize { term, cols, rows }` / `TermClose` | Terminal |
| `Copy(String)` | `UiAction::CopyRequested`, botón copiar |
| `SetSetting { key, value }` | Settings |

### 1.5 Streaming

| Problema | Solución |
|---|---|
| Muchos deltas por segundo | `send_latest` por `(turn, block)`: el `Store` concatena, la UI ve como mucho un cambio por cuadro. |
| Markdown a medio escribir | `Markdown::new(..).streaming(true)` (limn: estiliza delimitadores abiertos, retiene un fence a medio escribir). Al cerrar el bloque, `streaming(false)`. |
| Scroll al final | `ScrollArea::measured_list(id, n, est, row_key, build_row).follow_end(true)`; la clave de fila es el id del ítem, que ancla la vista si el usuario subió. |
| Muchos ítems | `measured_list` sólo construye lo visible; `Arc<[Item]>` en el `Store` para clonar punteros. |
| "Thinking…" escalonado | Texto derivado en `view` desde `turn.started_at` y `now` (15/30/45/60 s); `TextShimmer`. Repintado cada 1 s sólo mientras hay turno activo. |

### 1.6 IDs (memoria de limn)

Derivados de ids del dominio; nunca índices.

| Control | ID |
|---|---|
| Fila de sesión | `session:{sid}` · menú `session:{sid}:menu` |
| Grupo de sidebar | `group:{key}` |
| Scroll del transcript | `transcript:{sid}` |
| Fila de herramienta | `tool:{tool_id}` · cuerpo `tool:{tool_id}:out` |
| Resumen del turno | `turn:{turn_id}:summary` |
| Tarjeta de permiso | `perm:{req}:deny`, `perm:{req}:always`, `perm:{req}:more`, `perm:{req}:once` |
| Composer | `composer:{sid \| new}`; chips `composer:{sid}:mode`, `:model`, `:folder`, `:worktree`, `:env` |
| Panel derecho | `env:{sid}:*`, `ctx:{sid}`, `changes:{sid}:commit` |
| Diff | `diff:{sid}` · archivo `diff:{sid}:file:{path}` · `diff:{sid}:more:{hunk}` |
| Terminal | `term:{tid}` · pestaña `term:{tid}:tab` |
| Divisores | `split:main`, `split:right` |

Regla del gallery de limn: un test falla si dos controles comparten ID (§6).

---

## 2. Pantallas y zonas del MVP

### 2.0 Mapa general

```text
┌──────────── 212 ────────────┬───────────────────────── 1196 ─────────────────────────┐
│ ≡ ▣ ‹ › [⧉|</>]   (27 h)    │ Título sesión ▾                     ▤ >_ ± ⊕ ▢ ⋯  – ▢ ✕ │ 27
├─────────────────────────────┼──────────────────────────────────────────┬─────────────┤
│ + New · Search · …  (nav)   │      columna transcript 541 (centrada)   │ panel der.  │
│ grupo "work ▾"              │                                          │ 206 flotante│
│ ▸ fila sesión ●             │                                          │             │
│                             │      composer 566 × 34 (crece ↑)          │             │
├──── pie cuenta (35 h) ──────┤      fila de chips (16 h)                │             │
└─────────────────────────────┴──────────────────────────────────────────┴─────────────┘
```

| Zona | x / ancho | y / alto | Notas |
|---|---|---|---|
| Ventana | 1408 | 844 | Sin marco nativo; mínimo 600×400. |
| Barra superior | todo el ancho | 0–27 (hairline en y=26) | Parte sobre la sidebar + parte sobre el área principal. |
| Sidebar | 0–212 (hairline x=211) | 27–844 | Ficha: 288 por defecto, 200–420. En captura 212 → **usar default 240, rango 200–420** (decisión de quill). |
| Área principal | 212–1408 (1196) | 27–844 | Contiene transcript + composer, y opcionalmente diff/terminal a la derecha. |
| Columna transcript | 540–1081 (541) | 36–770 | Centrada en el área principal (centro x≈810), no en "área − panel derecho". |
| Composer | 528–1094 (566) | 782–815 (34; 49 a 2 líneas) | 12 px más ancho que la columna a cada lado. Margen inferior 29. |
| Fila de chips | 528–1094 | 821–837 | Bajo el composer. |
| Panel derecho | 1198–1404 (206) | 36–320 | Tarjetas apiladas, a 4 px del borde. Desaparece al abrir el diff. |
| Con diff abierto | chat 212–691 (479) · diff 692–1408 (716) | — | ≈ 2:3. Columna del chat 243–660 (417): padding lateral 31. |
| Con terminal | diff 27–436 · terminal 436–844 | — | 50/50 vertical dentro del panel derecho. |

**Reglas del shell (tomadas de surco `view.rs::window_view`, adaptadas a las medidas de Code):**

| Regla | surco | quill |
|---|---|---|
| Columnas | `row[gap, library, handle, pane.flex(1), handle, panel, gap]` sobre `column[top_bar, row.flex(1), player]` | `column[title_bar 27, row[sidebar, handle, main.flex(1), handle?, side_pane?]]` (sin barra inferior; el composer vive dentro de `main`). |
| Separación | `GAP = 8`: superficies sueltas con radio 4 y el handle **es** el hueco (`ResizeHandle::thickness(GAP)`) | Code no tiene huecos: superficies a ras con hairline de 1 px. `ResizeHandle::vertical(id).thickness(6)` superpuesto a la hairline (área de agarre sin hueco visible). |
| Mínimo del contenido | `CONTENT_MIN_WIDTH = 360`; "los paneles laterales ceden antes que el contenido" (`right_whole = right_width.min(window − … − CONTENT_MIN_WIDTH)`) | `MAIN_MIN = 420`; la sidebar y el panel de diff/terminal se recortan antes que la columna del chat. |
| Panel derecho por ancho | `PANEL_FROM = 1024`: por debajo, la cola reemplaza al contenido; cerrado, queda un `PeekPanel` de 40 | `PANEL_FROM = 1100`: por debajo, el panel Environment/Changes no flota y el diff reemplaza al chat (con "volver"); cerrado, `PeekPanel` opcional. |
| Sidebar plegada | `RAIL_BELOW = 860` → `SideRail` de 72 | `RAIL_BELOW = 760` → sidebar oculta (`b`), sin rail (Code no tiene rail). |
| Animación | `ScalarTransition` por panel; el panel que se abre se dibuja a ancho parcial con `clip_children()` sobre su contenido a ancho final ("sliding") | Igual para sidebar y panel de diff; respeta `set_reduced_motion`. |
| Tamaños | `state.left_width`, `state.right_width`, persistidos | `Ui.sidebar_width` (200–420), `Ui.side_pane_fraction` (0.3–0.75), `Ui.terminal_fraction` (0.2–0.8), persistidos. |

### 2.1 Marco y barra superior

**Layout**

| Elemento | Medida | Componente |
|---|---|---|
| Barra | alto 27, fondo = canvas, hairline inferior | `Element::row` + `Separator::horizontal` |
| Hamburguesa, toggle sidebar, ‹ › | íconos 16 en botones ~22, x=18/42/66/88 | `IconButton` `ButtonSize::Mini` |
| Toggle Chat/Code | píldora 2×24 en x 155–205, segmento activo con fondo | `SegmentedControl` (quill sólo tiene Code: dejar sólo el ícono de Code o quitar) |
| Título de sesión | x=229, 13 px, chevron ▾ (menú de sesión) | `Button` ghost + `DropdownMenu` |
| Toggles de paneles | 5 íconos a la derecha (panel derecho, terminal, diff, preview, archivos) + ⋯, pitch 22; activo con fondo `selected`  | `Toggle` (ghost, icon-only) o `ToolbarItem` en `Toolbar` |
| Controles de ventana | 3 círculos ~16 px gris claro, x=1338/1365/1392 | `WindowControls` (**falta** estilo Linux redondo, ver §4) |
| Arrastre / resize | toda la barra salvo controles | host: `ViewportCommand::StartDrag` / `BeginResize` como RUNE `apps/desktop/src/frame.rs` |

**Estados:** maximizada (botón medio restaura), sin sesión ("New session" en gris, sin toggles de diff).
**Atajos:** Ctrl+K paleta (fuera del MVP), `b` alterna sidebar fuera de campos editables, Ctrl+F buscar.

| Usar de limn | Crear |
|---|---|
| `IconButton`, `Toggle`, `Toolbar`/`ToolbarItem` (`toolbar.rs`), `SegmentedControl` (`components.rs`), `DropdownMenu` (`dropdown_menu.rs`), `Separator` (`separator.rs`), `WindowControls` (`window_controls.rs`) | limn: variante Linux de `WindowControls`. quill (`apps/quill`): región de arrastre/resize (patrón RUNE `frame.rs`). |

### 2.2 Sidebar de sesiones

**Layout**

| Elemento | Medida | Componente |
|---|---|---|
| Fondo | `#FAFAFA` (panel), hairline derecha | `SidebarPanel` (`panel_header.rs`) o `Element::panel` |
| Navegación (New, Search, …) | filas 24 alto, pitch 25, desde y=32; ícono x=19, texto x=32, 13 px; inset 6 | `ListRow` con `ListRowSizing` compacto |
| Fila activa / hover | fondo `#E0E0E0`/`#E6E6E6`, radio 6, x 6–204 | `ListRow::filled()` + `RadiusRole::Control` |
| Encabezado de grupo | "work ▾", 11 px muted, y=182, x=13 | `SectionHeader` (`.disclosure`) |
| Fila de sesión | 24 alto, título truncado 13 px x=13, punto de estado 6 px en x=192 | `ListRow` + `.trailing(StatusDot)` |
| Vacío | "No sessions match the current filters" centrado 11 px muted + link "Show all sessions" | `Empty` (sin ícono) o `Text` + `Link` |
| Pie de cuenta | hairline y=809, alto 35; avatar 14 + nombre + "· plan" muted + chevron; ícono a la derecha | `Avatar`, `Button` ghost, `IconButton` |
| Borde redimensionable | 200–420 | `ResizeHandle::vertical("split:sidebar")` |

**Jerarquía:** nav fija → grupos (por carpeta en el MVP; fecha/estado después) → filas → pie.

**Desktop (captura del 2026-10-27):**
- **Navegación:** New, Search, Artifacts, Customize y More. quill tiene New, Search y Customize, que abre su página propia en el lugar de la conversación (`extensiones.md` §12). Artifacts no tiene equivalente en el CLI. No pudimos ver qué contiene More, porque en la captura el clic no abrió nada.
- **Agrupado:** por defecto agrupa por carpeta, y Desktop deja "Reset to defaults" deshabilitado con Folder elegido. El menú ofrece Group by (Date, Status, Folder, Environment, None), Sort by, Show (Metadata, Side chats, Empty groups), New section, Reset to defaults y View all. quill ofrece Folder, Status y None.
- **Encabezado de grupo:** el nombre en gris chico con un triángulo relleno (▾ abierto, ▸ cerrado); quill usa los chevrons de las tarjetas del panel derecho, por elección del usuario. Al pasar el puntero aparecen el filtro y el "+". Cerrado, muestra un ⚠ al lado del nombre si alguna sesión espera respuesta.
- **Filas:** agrupando por carpeta no llevan ícono de lugar. quill solo lo muestra en los otros agrupados (inferido).
- **Pie:** está bajo una línea y es un botón con la inicial en círculo, el nombre, "· plan" y ˅. Su menú tiene el email, Settings (Ctrl+,) y páginas de claude.ai. quill solo pone Settings.
- **Remote Control:** las sesiones de Remote Control llevan su propio grupo, con puntos de estado. quill no las tiene.

**Estados de fila:**

| Estado | Señal | Color quill (§3) |
|---|---|---|
| `Running` | quill: el título en gris con un brillo que pasa (sin punto) | `muted` |
| `Awaiting` (pide permiso, pregunta o aprobar un plan) | punto fijo de 6 px al final de la fila | `awaiting` (ámbar) |
| `Ready` (terminó y no se vio) | punto fijo de 6 px al final de la fila; el título conserva su peso, no va en negrita | `accent` (azul de quill) |
| `Error` | punto fijo | `removed` (rojo) |
| `Idle` | sin punto | — |
| Cargando lista | 3–5 filas `SkeletonGroup::rows` | — |

El punto azul aparece cuando un turno termina mientras la persona mira otra sesión, y se va al abrirla (en la lista o desde el banner). "Mark as unread" (menú "···", tecla U) lo vuelve a poner; "Mark as read" lo quita. Cada punto dice qué es a un lector de pantalla ("Unread response", "Needs your input", "Failed"). Desktop muestra en la fila un punto azul para la respuesta sin leer, ámbar (o una mano) para lo que espera a la persona, gris que late mientras trabaja y un aro gris tenue para lo terminado; lo que quill no copia: que el punto mismo se pueda pulsar para marcar leída o no leída. Que el punto vaya al final de la fila, y su tamaño, son inferidos.

**Interacciones:** clic abre `/session/{sid}`; clic derecho → `ContextMenu` (renombrar, fijar, archivar); `b` oculta; arrastre del borde.

| Usar de limn | Crear |
|---|---|
| `ListRow`, `ListRowSizing` (`list_row.rs`), `SectionHeader` (`page.rs`), `ResizeHandle` (`resize_handle.rs`), `ContextMenu` (`context_menu.rs`), `SkeletonGroup` (`skeleton.rs`), `Empty` (`empty.rs`), `Avatar` (`avatar.rs`), `Badge::dot()` (`badge.rs`) | limn: `StatusDot` con latido (o `Badge::dot().pulse(true)`). quill: derivación de estado de fila, agrupado. |

### 2.3 Página "New session" (vacío)

| Elemento | Medida | Componente |
|---|---|---|
| Bloque centrado | el saludo (§2.13c) y la tarjeta del composer como un solo bloque, centrado a lo ancho de la columna; la línea del saludo queda a ~42 % del alto (nunca a menos de 24 px del borde superior: en una ventana baja se achica primero el espacio de arriba) y la tarjeta 24 px debajo. Nada debajo del bloque. Modelado sobre lo que se ve en la página de chat nuevo de claude.ai; las medidas son de quill | `view::home` |
| Composer | una tarjeta tan ancha como la columna del transcript (hasta 920), radio 18: arriba el texto (placeholder "Describe a task or ask a question", dos líneas de alto vacío, crece hasta 184) y, dentro de la tarjeta, abajo, "+" a la izquierda y enviar a la derecha. Debajo de la tarjeta una fila: a la izquierda los chips "Local", agente y carpeta (se parten en líneas si la columna es angosta); a la derecha modo, modelo, esfuerzo y uso, los mismos menús que el pie de §2.6 | `Composer::pill_controls` + `context` (`view::home_footer`) |
| Al enviar el primer mensaje | la sesión existe y vuelve el diseño de siempre: transcript arriba, composer abajo. El composer baja deslizándose (`animate_move`, el mismo id en las dos posiciones); con movimiento reducido salta. El foco sigue en el composer (mismo id) | `Element::animate_move` |
| Chip de agente | entre "Local" y la carpeta, **solo con 2 o más agentes**: el logo del agente (`AgentInfo.icon`) y su nombre; abre un `DropdownMenu` "Agent" con los disponibles. Elegir uno pone `ui.agent`, vuelve modo, modelo y esfuerzo a los de ese agente (`AgentInfo.default_mode` y los de Settings) y guarda `Settings.default_agent` (se persiste antes de seguir). Sin elección guardada se usa el primero. Inferido: Desktop no tiene el equivalente | `chip` + `DropdownMenu` (`view::agent_menu`) |

**Estados:** sin carpeta (chip "Choose…") → menú "No folder / Open folder…" → diálogo nativo → **"Trust this workspace?"** (`AlertDialog`) → carpeta elegida, aparece chip Worktree; Worktree on = chip con tinta accent.

### 2.4 Transcript

**Layout**

| Elemento | Medida | Componente |
|---|---|---|
| Línea de origen | "Started on this computer · work · Save as environment", 11 px muted, y=54; link subrayado | `Element::formatted_text` + `Link` |
| Mensaje del usuario | alineado a la derecha; ancho máx ≈460 (85 % de la columna); padding 12/8; radio 10; fondo `#F3F3F3`; 13 px, interlínea 15 | `Element::panel` + `Text` (componente quill `UserBubble`) |
| Acciones del mensaje del usuario | debajo de la burbuja, a la derecha, al pasar el puntero: hora ("2 days ago"), copiar ("Copy message"), "Rewind to here" y "Fork from here". Rewind recorta desde el mensaje, devuelve el texto al composer y retoma desde la respuesta anterior; al final de la conversación queda una línea "Rewound to an earlier message." con "Undo rewind" (ubicación inferida), que devuelve los mensajes hasta que la persona envía uno nuevo; mientras el agente trabaja se muestra apagado ("Rewind is unavailable while the agent is working"). Fork abre una sesión desde la respuesta anterior con el texto en el composer; el primer mensaje no tiene fork | `user_actions`, `rewound_line`, `Intent::RewindTo`, `Intent::UndoRewind`, `Intent::Fork` |
| Flecha "↓" al final | 36 px sobre el relleno de la página, con borde y sin halo (se lo quitamos a pedido del usuario) | `transcript` |
| Separación entre bloques | 22–35 px tras el mensaje del usuario; 8–12 entre párrafos | `column.gap` |
| Resumen del turno (colapsado) | "Read and edited saludo.py, ran a command" muted 13 + `+1 −1` + ▸, x=544 | `Collapsible` (título) + `DiffStat` (`list_row.rs`) |
| Grupo de herramientas (expandido) | tarjeta 541 ancho, filas 27, hairlines entre filas, radio 8, padding izq. 9; "verbo" muted + objetivo 13 px + `+1 −1` + ▸ | `FramedCard::rows` + `Collapsible` por fila |
| Texto del asistente | 13 px, sin burbuja, x=544 | `Markdown` (`.font_size(13)`, `.streaming`) |
| Código en línea | fondo `#F2F2F2`, mono 11, radio 3 | `Markdown` (code_fill) |
| Bloque de código | tarjeta al ancho del contenido (541–806), alto 31, radio 6, botón copiar a la derecha | `CodeBlock` (`code_block.rs`) |
| Acciones del mensaje | y=367: copiar, bifurcar, fijar, leer en voz alta (íconos 14, pitch 18) + "Just now" 10 muted; visibles al hover del último | `MessageActions` (`message_actions.rs`) |
| Tarjeta de tareas | 541 ancho, filas 20, ícono ✓ (hecho) / ◌ (en curso), 12 px | `FramedCard` + `ListRow` |
| Estado "Needs your input" | punto naranja anillado + chip warning (fondo `#EFE9E0`, 10 px) | `Chip::tone(AlertTone::Warning)` + `StatusDot` |

**Jerarquía de un turno:** mensaje usuario → [pensamiento] → línea de actividad / resumen colapsable → texto del asistente intercalado con grupos de herramientas → [tarjeta de permiso] → [tareas] → [estado] → acciones.

**Estados del turno**

| Estado | Qué se ve | Componente |
|---|---|---|
| Vacío (sesión nueva) | sólo línea de origen | — |
| Cargando sesión | skeleton de 3 bloques; texto "Loading session" para lector de pantalla | `Skeleton` |
| Enviado, esperando | "Thinking…" con shimmer; escala a "Still thinking…" (15 s)… | `TextShimmer` (`text_shimmer.rs`) |
| Corriendo herramienta | "Running saludo.py to verify output ▸" muted; fila con `Collapsible::loading(true)` (spinner) tras 650 ms | `Collapsible`, `Ticker` para la línea que cambia |
| Streaming de texto | markdown creciendo, `follow_end` | `Markdown::streaming(true)` |
| Esperando permiso | tarjeta §2.7 + chip "Needs your input" | ver §2.7 |
| Parando | "Stopping…" con shimmer | `TextShimmer` |
| Error de turno | aviso inline "Turn failed" / rate limit, con "Retry" | `Notice` + `.ink(danger)` o `Alert` |
| Terminado | resumen colapsado "Read and edited …" + `+N −M`; tarjeta de cambios | `Collapsible`, `DiffStat` |
| Herramienta fallida/denegada | fila con `Collapsible::ink(danger)`, detalle "Failed"/"Denied" | `Collapsible::detail` |

**Tarjeta de herramienta (cuerpo expandido)**

| Tipo | Cabecera (verbo · objetivo · meta) | Cuerpo |
|---|---|---|
| Bash | "Ran" / "Running" · descripción | comando (mono) + salida, máx 400 alto → `OutputBlock::max_height(400)` |
| Read | "Read" · ruta | rango de líneas → `CodeBlock` |
| Edit/Write | "Edited"/"Created" · ruta · `+a −d` | diff unificado → `CodeDiff::single_gutter(true)` (máx 400) |
| Grep/Glob | "Searched" · patrón | `OutputBlock` |
| TodoWrite | "Updated todos" | checklist |
| Otras / MCP | "Used {label}" | `DescriptionList` + `OutputBlock` |

**Interacciones:** Enter/Espacio sobre una fila la abre/cierra (estado por `tool_id` en `Ui`); clic en `+1 −1` o en la ruta abre el panel de diff en ese archivo; seleccionar texto + Ctrl+C → `CopyRequested`.

**Selección:** la columna del transcript es un `Element::selection_scope()` de limn, así que se selecciona como un documento: arrastrar pasa de un párrafo a los siguientes (mensajes de la persona, respuestas con títulos, listas, código y links, y el texto de los pasos), Shift+clic extiende la selección, y Ctrl+A, con un clic previo en un texto del transcript y ningún campo enfocado, selecciona el transcript entero (no sólo el mensaje: cada mensaje ya tiene su botón de copiar). Ctrl+C copia texto plano con forma de markdown: una línea en blanco entre párrafos, mensajes y bloques; los ítems de una lista uno por línea con su marca (`• uno`, `1. dos`); el código con sus líneas tal cual, sin cerca de ```; el texto de un link, no su dirección. Un clic en un link o un botón sigue siendo un clic.

| Usar de limn | Crear |
|---|---|
| `ScrollArea::measured_list` + `follow_end` (`scroll_area.rs`), `Markdown` (`markdown.rs`), `Collapsible` (`collapsible.rs`), `FramedCard` (`framed_card.rs`), `OutputBlock` (`output_block.rs`), `CodeBlock` (`code_block.rs`), `CodeDiff` (`code_diff.rs`), `DiffStat` (`list_row.rs`), `TextShimmer`, `Ticker` (`ticker.rs`), `MessageActions`, `Chip` (`chip.rs`), `Notice` (`notice.rs`), `Skeleton` | quill (`quill-components`): `UserBubble`, `TurnSummary`, `ToolRow` (catálogo tipo→verbo/cuerpo), `TodoCard`. limn: nada (`FramedCard::rows` + `Collapsible` alcanzan). |

### 2.3b Links de la conversación

- **Clic:** abre la página en el navegador de la sesión; sin pestañas abre una, y si ya hay, suma una pestaña. Si el link es una ruta de archivo, abre el archivo en Files. Otros esquemas (`mailto:`, links de otras apps) se abren en la computadora.
- **Clic derecho sobre un link:** un menú en ese punto con "Open in Browser" (el de quill), "Open in default browser" (el del sistema) y "Copy link". El host detecta qué link hay bajo el puntero con `Frame::hit_test` porque limn no tiene menús por fragmento de texto.
- **Abrir afuera** (`quill_files::open_in_system`, el único lugar que lo hace): `xdg-open` en Linux, `rundll32 url.dll,FileProtocolHandler` en Windows (no `cmd /C start`, que cortaría una dirección en sus `&`) y `open` en macOS.

### 2.3c Customize › Connectors y el "+" del composer

**Connectors** (página nueva de Customize). Para quill, un conector es un servidor MCP del agente. Desktop muestra los conectores de claude.ai, que un programa local no puede alcanzar.

- **Por CLI:** cada agente de `AgentInfo` tiene su sección con su logo (18 px, en negrita), su nombre y la cuenta, sobre un filo. Una sección vacía dice "No connectors in {agente} yet." Un elemento que está activo en **varios** agentes lleva al final el logo de cada uno (con su nombre en el tooltip); si está en uno solo, su sección ya lo dice. Skills y Plugins se ordenan igual. **"All agents"** es la primera sección (icono de globo, 18 px, en Connectors y en Skills): lo que quill guarda y agrega a cada sesión (`agents.md` "La capa All agents"). Cada fila dice de qué agentes se usa con el logo de cada agente al final, entintado por su estado: tinta normal si se usa, apagado si la persona lo sacó de ese agente, ámbar si el agente tiene uno propio que gana ("Replaced in {agente}"), rojo si no lo puede usar ("Not in {agente}", con el motivo en el tooltip). La línea de debajo repite esos casos con palabras. El "⋯" tiene "Use in {agente}" (uno por agente) y "Remove". Para cambiar un servidor global se lo agrega de nuevo con el mismo nombre.
- **Yours:** los servidores que el agente tiene configurados (`Intent::Servers`; con Claude Code, `claude mcp list`; con Codex, `codex mcp list --json`, que no los arranca y por eso dice "On" y no "Connected").
  - Cada uno con su estado, su origen ("from a plugin", "from your account") y su dirección o comando.
  - Los propios se pueden quitar ("⋯" › Remove). Los de un plugin se quitan desinstalando el plugin, y los de la cuenta desde claude.ai.
  - El listado arranca cada servidor para revisarlo, así que se pide aparte, solo al abrir esta página.
- **Discover:** el registro público de MCP (`registry.modelcontextprotocol.io`), que sirve para cualquier agente con MCP. El motor lo consulta con `curl` (`Intent::FindServers`, `quill-engine/src/registry.rs`) y la búsqueda pregunta con cada palabra. "Add" abre "Add custom connector" ya completo: la dirección, o `npx`/`uvx`/`docker` con el paquete, más los argumentos y variables que pide, para completar.
- **Add custom connector:** (el primer campo es "Add to": **All agents**, o uno o varios CLIs; con All agents aparecen "Secret variables", "Secret headers" y, para una dirección, "Older SSE transport", y no hay "Where")
  - Campos: nombre; dirección `https://` o comando con sus argumentos; variables `KEY=value`; encabezados `Name: value` (solo para direcciones); y dónde va (los alcances del agente, `server_scopes`).
  - "Add to": un botón por agente, uno o varios (empieza con el agente de una sesión nueva, no con el primero). Cada uno lo agrega con su propio programa (`ExtensionOp::AddServer`; Claude Code `claude mcp add`, Codex `codex mcp add`), así que queda en la configuración de cada CLI y se ve en su terminal. Un intento por agente: si uno falla (Codex no admite encabezados), el error sale en la sección de ese agente y los demás siguen.
  - "Where": los alcances de cada agente marcado (`server_scopes`), con "Where in {agente}" cuando hay varios; un agente con un solo alcance (Codex) no lo muestra.
  - Quitar y cambiar un estado actúan sobre el CLI de la fila.

**"+" del composer** (como en Desktop):
- **Connectors:**
  - "Browse connectors" abre Connectors en Discover, y "Manage connectors" abre el diálogo `/mcp`.
  - Debajo, cada servidor MCP de la sesión con su interruptor (`mcp_toggle`, solo para la sesión). El agente informa sus servidores solo mientras corre. Con un agente que no puede apagarlos por sesión (Codex) no hay interruptor: la fila dice "On" u "Off in its config". Lo mismo en `/mcp` (§2.11b): sin "Turn on" ni "Clear authentication" si el agente no los tiene (`capabilities.mcp_toggle`, `mcp_clear_auth`).
  - Con Codex, `/` lista también sus skills (`AgentEvent::Skills`, §2.11).
- **Plugins:** cada plugin prendido que tiene skills: su único skill, o un submenú con todos ("N skills"). Elegir uno escribe `/plugin:skill` en el mensaje. Después vienen "Manage plugins" y "Browse plugins". Sin interruptores, como en Desktop.

### 2.4a Archivo cambiado en el diff

- **Plegar:** el chevron y el nombre de cada archivo pliegan sus líneas y las vuelven a mostrar (`diff:fold:{path}`).
- **Abrir:** un botón doble, igual al de "Create PR".
  - La parte principal ("Open in Files") abre el archivo en el panel Files de quill, con el cursor en la primera línea cambiada.
  - La flecha despliega "Open in", con los mismos lugares que el menú de una conversación (quill-ide, los editores encontrados, la app por defecto y el gestor de archivos), apuntando a ese archivo y esa línea.
- Reemplaza al botón "Open in editor", que abría solo el último editor elegido. Es decisión del usuario; Desktop solo tiene ese botón.

### 2.4b Background tasks

Es el panel "Background tasks" de Desktop, hecho sobre las tareas que quill ya seguía (`Session::tasks`, con los agentes ayudantes y su panel Agent). No duplica nada.

| Parte | Qué muestra | Dónde |
|---|---|---|
| Apertura | Un botón en la barra superior, solo si la sesión tiene tareas (Desktop lo pone en su menú "⋯", con la cantidad), y el texto "N running tasks" de la línea de estado | `TASKS_TOGGLE`, `TASKS_OPEN` |
| Cabecera | "Background tasks"; la papelera "Clear finished tasks" mientras haya terminadas; expandir y cerrar | `tasks_pane` |
| Grupos | "Running N" y "Finished N", plegables, abiertos al empezar | `tasks:running`, `tasks:finished` |
| Fila | La descripción. Debajo, el tipo (Agent, Bash, Workflow, Monitor, Task), el estado (Running, Completed, Failed, Stopped) y el tiempo, en vivo mientras corre. Después los tokens y las herramientas usadas, y "View transcript" para un agente (abre su panel Agent). Las que corren llevan "Stop" (`stop_task` del CLI). | `tasks_pane`, `Intent::StopTask` |
| Vacío | "Background tasks appear here" | `tasks_pane` |

Lo que la papelera limpia se guarda con la sesión, así que sigue limpio tras reiniciar quill (`Intent::ClearTasks`, `Event::TasksCleared`).

El adaptador traduce `task_type` del CLI a un tipo genérico y `usage` a `TaskUsage` (`TaskProgress`, `TaskEnded`).

**Terminals** (panel derecho): los comandos que el agente deja corriendo (tareas de tipo `command`), cada uno con su estado y "Stop" mientras corre. Un clic abre su salida en el panel Terminal, en una pestaña con su nombre que sigue el archivo donde el CLI la escribe (`tail -F`; "Output is being written to: …" en el resultado del Bash de Claude Code). Sin ese archivo, abre Background tasks.

**Falta:**
- El modelo de cada agente: Desktop lo muestra y el CLI no lo informa en las tareas.
- Los comandos en segundo plano de Codex.

### 2.4c Vistas del transcript

Desktop tiene tres vistas del transcript, Normal, Thinking y Verbose, cada una con todo lo de la anterior.

- **Normal:** como antes. El pensamiento no se dibuja; los grupos de herramientas y sus cuerpos se abren y cierran a mano.
- **Thinking:** además, el pensamiento del modelo en su lugar dentro del turno: un bloque con "Thought process" en muted, el texto en el tamaño del transcript, y un botón Copy que copia el texto como cita (cada línea con `> `). Un bloque vacío no se dibuja.
- **Verbose:** además, todo colapsable abierto y sin toggle: los grupos de herramientas, las filas con su cuerpo (diff, salida), el "N more" de la tarjeta de cambios y la tarjeta de un mensaje recibido de otra sesión. Las flechas quedan, abiertas, sin acción. Una fila de helper (Agent, Task) no tiene cuerpo en línea: abre su panel, que Verbose no cambia.
- **Menú de la sesión:** "Transcript view ›" en el menú del título y en el de la fila del sidebar, con los radios Normal, Thinking y Verbose. Thinking sólo aparece si el agente puede dar el pensamiento (`AgentCapabilities.thinking_text`) y la sesión tiene pensamiento o aún no respondió. Si la vista de la sesión no es la predeterminada aparece "Make {vista} the default", que guarda la vista en Settings y devuelve la sesión a seguir el predeterminado.
- **Ctrl+O** cicla Normal → Thinking → Verbose → Normal, saltando Thinking cuando no se ofrece.
- **Predeterminado:** Settings › General › Appearance › "Default transcript view" (Normal). Una sesión sin elección sigue el predeterminado; una elección se guarda en su log (`Event::ViewChosen`) y vuelve al abrir la sesión.
- **Inferido:** que la vista se pueda guardar por sesión y en Settings es decisión de quill; Desktop la guarda en la app. Que el pensamiento llega por `--thinking-display summarized`, que el adaptador de Claude ya pide. Que Desktop lleva el ancla del scroll al cambiar de vista y avisa "Make {vista} your default view?" no está; tampoco la proyección de los canales de Slack.

### 2.4d Búsqueda web

Los demás pasos usan los verbos de Desktop: "Started planning", "Read task output", "Stopped task", "Listed resources", "Read resource", "Entered/Created a worktree", "Scheduled check-in", etc. Lo que sigue al verbo (el servidor, la URI, el comando detenido) es de quill; Desktop no muestra nada ahí. `BashOutput` y `KillShell` son los nombres viejos de `TaskOutput` y `TaskStop` (la CLI 2.1.312 lista `KillShell`/`KillBash` como alias). `TaskStop` ya no se oculta como herramienta de tareas: detiene un comando en segundo plano, no toca la lista. EnterPlanMode no se despliega, porque su salida es la instrucción de la CLI al modelo. Al desplegarse, los recursos MCP se ven como una lista (nombre sobre URI) y la lectura de un recurso muestra su texto, no el JSON. La salida de una tarea muestra el estado y lo que imprimió, sin las etiquetas de la CLI. Esas etiquetas son inferidas, porque la 2.1.312 ya no trae la herramienta.

Una herramienta `WebSearch` del agente no es un paso: es un bloque propio (`web_search_block` en `view.rs`). Desktop la dibuja como un paso plano, "Searched web" con la consulta; este bloque es diseño de quill, tomado de rareui (`WebSearch`), sin copiar su código ni sus colores. La licencia de rareui no está revisada.

- **Pastilla** de 40 px, redonda: un globo en un círculo de 28 px, la consulta mientras busca ("Searching “…”", con `TextShimmer`) o "Searched N sites" al terminar, hasta cuatro letras de los sitios (`Avatar`, 20 px) y un chevron. Clic para plegar o desplegar el panel; en Verbose queda abierto.
- **Mientras busca:** un anillo de progreso fijo en 0.08 sobre el globo y cuatro esqueletos en el panel. El anillo desaparece al terminar.
- **Panel:** tarjetas de fuente en 4 columnas (2 si el bloque mide menos de 400 px), hasta 560 px de ancho. Cada tarjeta muestra el título en dos líneas, la letra del sitio, su nombre (`mozilla` para `developer.mozilla.org`) y el número. Desde 5 fuentes se ven tres y una tarjeta "N more sources"; "Show less" vuelve a plegarlas. Un clic en una tarjeta abre la página como un link de la conversación, con el mismo menú al clic derecho.
- **Color de la letra:** `hsl(h, 50 %, 48 %)` con `h` = FNV-1a del nombre del sitio mod 360. Es una decisión de quill: limn no tiene relleno con degradado.
- **Datos que lee:** de cada resultado de la herramienta, `tool_use_result.results[*].content[*]` (título y URL, sin repetir URL); si falta, el JSON de `Links: [...]` del texto. La consulta sale del `input.query`. El resumen del modelo y el bloque `REMINDER` no se muestran. Un `WebSearch` con error o detenido queda como paso.
- **Pendiente:** chips de cita en el texto de la respuesta, movimiento (rellenos, entradas escalonadas, la pastilla que se despliega con animación), favicons reales (con una opción de Settings, sin servicios de terceros) . `TextShimmer` de limn tiene 13 px fijos y no trunca, por lo que la pastilla usa 13 px y puede partir una consulta larga en dos líneas.

### 2.5 Resumen / tarjeta de cambios del turno

| Parte | Medida / texto | Componente |
|---|---|---|
| Un archivo | una fila de 36 px: ícono, nombre, "Undo", `+N −M` en verde y rojo y chevron | `changes_card` |
| Varios | encabezado con ±, "Edited N files", "Undo", totales y chevron; debajo, los primeros 3 archivos con su carpeta en gris y "N more" / "Show less" | `changes_card` |
| Marco | borde de tarjeta con radio 12; cada fila se ilumina bajo el puntero y abre el diff del pedido | — |
| Lugar | entre la respuesta y sus acciones (copiar, fork, leer, hora), como en Desktop | `entries` |

Es el diseño de Desktop. En Desktop, "Undo" le pide al agente que deshaga ("Ask Claude to undo this turn's file changes"). En quill, "Undo" restaura de verdad la instantánea del pedido (engine.md §6) y después muestra "Undone".

### 2.6 Composer

**Layout**

| Elemento | Medida | Componente |
|---|---|---|
| Tarjeta | 566×34 (una línea), crece hacia arriba (49 a 2 líneas); radio ~10; borde 1 px `#E6E6E6`; fondo canvas | `Composer::new(id, text, placeholder, 566, palette).auto_grow(18, 200).inline_controls(true).card_radius(10).border_color(..)` |
| `+` (adjuntar) | x=545, ícono 14 | `IconButton` en `pill_controls(leading, …)` |
| Texto | x=561, 13 px; placeholder muted: "Type / for commands" (sesión) / "Describe a task or ask a question" (nueva) | editor del `Composer` |
| Enviar / Parar | x=1050: ↵ (con texto), ■ (corriendo), oculto si vacío y quieto | `IconButton` en `pill_controls(…, trailing)` |
| Micrófono | x=1076 | `DictationButton` (fuera del MVP: ocultar) |
| Fila de chips (izq.) | y=829, 11–12 px: "This computer" (ícono laptop), carpeta ("Choose…" / "work"), "Worktree" (ícono rama), modo ("Auto"/"Manual"/…) | `Composer::footer(row)` con `Button` ghost `Mini` + ícono |
| Fila de chips (der.) | "Haiku 5.5 Low" (modelo normal + esfuerzo muted), indicador circular de contexto x=1076 | `Button` ghost + `Progress` circular (**no existe**: ver §4) |
| Modo con color | "Manual" en ocre, "Auto" neutro | `Button` con `palette.text = warning_ink` |
| Chip abierto | fondo `selected` mientras su menú está abierto | `Button::active(true)` |

**Los modos de permiso** salen del adaptador del agente elegido (`AgentAdapter::modes`, que llega en `AgentInfo.modes`): cada uno trae `id`, nombre, descripción y un `tone` (`Ask`, `Edits`, `Plan`, `Auto`, `Read`, `Unsafe`) que elige el ícono y la tinta (`Ask` ocre, `Unsafe` rojo). La interfaz no conoce las palabras de ningún agente; los modos sin palabras no se listan. `listed` dice cuáles se ofrecen antes de que el agente cuente sus capacidades, y los `Unsafe` esperan a que Settings los permita. Los textos de las tarjetas ("Allow Codex to…") usan la primera palabra del nombre del agente.

**Menús anclados a los chips** (todos `DropdownMenu` con `MenuHeader::title`)

| Menú | Ancho | Filas | Contenido | Captura |
|---|---|---|---|---|
| Modelo | 155 | 20 alto | encabezado "Model · this session"; 4 modelos con atajo 1–4 a la derecha y ✓ accent; chip "Preset"; separador; "Effort  High ›" (submenú); "More models ›"; sección "Fast mode" con `Switch` | |
| Modelo y esfuerzo por sesión | Son de cada conversación, como el "this session" de Desktop. Al elegirlos, el motor los guarda (`Event::ModelChosen`, `Event::EffortChosen`), se los dice al CLI y los usa al relanzar el agente, al retomar tras un reinicio y al hacer fork. Al volver a una conversación, el composer muestra los suyos. Las sesiones de antes de este cambio conservan lo que muestre la UI | `Session::model`, `Session::effort` |
| Esfuerzo (submenú) | 234 | 20 | texto explicativo muted 3 líneas; Low / Medium ("Recommended" chip) / High / Extra / Max ⓘ / opción destacada en violeta | |
| Permisos | 340 | 34 (2 líneas) | encabezado "Mode" y los modos en el orden y con los textos de Desktop:<br>• Manual, mano, "Always ask before making changes";<br>• Accept edits, `<>`, "Automatically accept all file edits";<br>• Plan, pergamino, "Create a plan before making changes";<br>• Auto, rayo, "The agent handles permission decisions", solo si el modelo elegido lo ofrece (`supportsAutoMode` del CLI, en `ModelInfo::modes`);<br>• Bypass permissions, advertencia, "Accepts all permissions", solo si Settings › agente › "Allow bypass permissions mode" está activo.<br>El chip va en ámbar con Manual y en rojo con Bypass | `mode_menu` |
| Carpeta | — | 20 | "No folder" / "Open folder…" | |

Popover: radio ~10, sombra suave, padding 4, fila con hover `#E6E6E6` radio 6 inset 4, encabezado 10–11 px muted. Se abre hacia arriba (anclado abajo).

**Estados del composer**

| Estado | Comportamiento |
|---|---|
| Vacío | placeholder; botón enviar oculto |
| Con texto | ↵ visible; Enter envía, Shift+Enter salto (`enter_submits(true)`) |
| Turno corriendo | ■ Parar; Enter dirige el mensaje al turno (steer, `priority:"next"`); Ctrl+Enter lo encola para el fin del turno; el placeholder dice "Enter to send, Ctrl+Enter to queue for later". El menú "Queue for later" llega después |
| Esperando permiso | igual que corriendo; el foco salta a la tarjeta de permiso |
| Deshabilitado (sin carpeta/no confiada) | `Composer::disabled()` + chip de carpeta resaltado |
| Error al enviar/parar | `Toast` con el motivo |

**Cola de mensajes** (`view::queue_bar`, `lib::queue_key`). Los mensajes enviados con Ctrl+Enter mientras el turno corre esperan en la cola de la sesión, y se muestran en una banda sobre el composer, encima de la barra de la rama. Con un mensaje, la banda dice "1 message queued", su texto y tres botones: enviar ya (`Intent::QueueSendNow`), editar en el composer (`QueueTake`) y quitar (`QueueRemove`). Con dos o más, se pliega en "N messages queued · Next: …" y al abrirla lista cada mensaje (dos líneas como máximo) con un menú "⋮", y "Clear all" (`QueueClear`); la lista se desplaza pasado 352 px. Con el composer vacío, la flecha arriba recupera el último mensaje de la cola, como Desktop (`handleDequeueLastQueued`). Una cola restaurada tras reiniciar se ve igual; sus mensajes no se envían hasta que la persona los envíe. Un mensaje llevado al composer (`QueueTaken`) no se guarda ni se vuelve a contar al arrancar: si la persona borró ese texto, no reaparece tras reiniciar. Lo que no está: arrastrar para reordenar, "Send now" que interrumpe el paso en curso (hoy lo manda como steer), y el recibo "unread" de un steer.

**Sugerencia del siguiente prompt** (`Session::suggestion`, `lib::suggestion_key`). Cuando un turno termina y el agente sugiere qué pedir después (`AgentEvent::PromptSuggestion`; hoy solo Claude Code, con Settings › Claude Code › "Prompt suggestions", prendido por defecto y aplicado a las sesiones nuevas, con la etiqueta y la descripción de Desktop), el composer vacío la muestra en el lugar del placeholder, como Desktop. Con el composer vacío y enfocado, Tab o → la ponen en él sin enviarla, para mandarla o cambiarla. Se oculta mientras hay texto y vuelve si se borra; se va al enviar un mensaje, al empezar un turno o al parar el agente, y no se guarda. Una que llega con el turno siguiente ya en marcha se descarta. El placeholder, y con él la sugerencia, sigue a la vista con el composer enfocado y vacío, como en Desktop, lo normal justo después de enviar (`Composer::persistent_placeholder`, de limn).

**Atajos:** Enter enviar; Shift+Enter nueva línea; Esc parar (si corre) o cerrar menú; 1–4 eligen modelo con el menú abierto (`DropdownMenuItem::shortcut`); `@` menciones (`SuggestionList`, fase 2). El `/` abre la paleta de abajo.

**Paleta `/`** (`crates/quill-ui/src/slash.rs`).

- **Disparador:** un `/` al inicio del texto o tras un espacio; la consulta llega hasta el final, sin espacios y con 50 caracteres como máximo. El caret se toma al final porque limn no lo reporta. Esc la cierra para ese mismo `/` hasta que se vuelva a escribir.
- **Filas:** primero los comandos propios de quill que ya hacen algo: `model`, `effort`, `permissions` (alias `allowed-tools`), `plan` si el agente ofrece ese modo, `mcp`, `plugin`, `skills`, `config`, `usage`, y con sesión elegida `tasks`, `diff`, `fork`, `rename`. Luego los del agente, por nombre. Un comando del agente con el nombre de uno propio se muestra como "Custom command" y va al agente. Sin capacidades, un título: "More commands available once the session is running".
- **Ranking** (estable): nombre exacto, alias exacto, prefijo del nombre, prefijo de alias, subcadena del nombre; el resto se descarta. El trozo que casa va en negrita.
- **Teclas** (antes que limn, con el composer enfocado): ↑/↓ mueven la fila sin dar la vuelta; Enter o Tab eligen; Esc cierra. Con cero filas, Enter envía como siempre. Un clic elige la fila.
- **Al elegir:** un comando del agente deja `/nombre ` en lugar de `/consulta`; uno de quill quita el token y hace lo que su botón o menú (`update` con el mismo id). Un menú sin nada que elegir (`/usage`) se abre, pero el composer no toma el foco hasta que el menú se cierra y entonces recupera el texto sin el token.
- **Argumentos:** `/model`, `/effort` y `/permissions` no abren su menú: dejan `/comando ` y la paleta lista sus opciones (los modelos, los niveles o los modos que ofrece su menú, con la descripción de cada uno), filtradas por lo que se escriba después. Enter o Tab aplican la fila con el mismo id que su clic, quitan el token y cierran la paleta. Es toma propia de quill, con teclado primero: en Desktop las filas abren selectores.
- **Presentación:** `SuggestionList` compacta, hasta 512 px, la descripción de la fila elegida en un globo al lado.
- **Falta:** el texto fantasma "Type to filter" tras el caret y los chips `/nombre` como átomos. limn no los dibuja todavía, así que el texto es plano.

**Paleta `@`** (`crates/quill-ui/src/mention.rs`).

- **Disparador:** un `@` al inicio del texto o tras un espacio; la consulta llega hasta el final, sin espacios y con 50 caracteres como máximo. Esc la cierra para ese mismo `@consulta`; al escribir más, vuelve.
- **Filas:** primero las sesiones (no archivadas, sin la que está en pantalla, con título, la más reciente primero; 3 sin consulta y 8 con ella). Coincide si la consulta está en el título, en el título sin espacios o en la rama. Icono de globo, título, y a la derecha `carpeta · rama`. Luego hasta 50 archivos de la carpeta de la sesión, buscados en el motor (`FindFiles`, sin contenido; `find::best`); la respuesta se guarda solo si es de la consulta en pantalla. Sin consulta, la lista muestra el primer nivel de la carpeta si el panel de archivos ya lo tiene listado; no se lee el disco en el hilo de la UI. En un composer sin sesión no hay archivos (la búsqueda necesita una sesión).
- **Teclas:** ↑/↓ sin dar la vuelta; Enter elige (Tab no, en Code no hay aceptar con Tab); Esc cierra; un clic elige.
- **Al elegir:** un archivo deja `@ruta/relativa ` sin comillas; una sesión deja `@[título] ` y se anota en `mentioned_sessions` para la tarea siguiente.
- **Al enviar:** cada sesión cuyo `@[título]` sigue en el texto llega al agente con una nota `<session-mentions>` que dice su id, numeradas por orden de aparición; el texto guardado es el que escribió la persona. La nota y la herramienta `read_session` (título, carpeta y últimos 40 mensajes, con tope de 20 000 caracteres, solo lectura) son redacción propia de quill.
- **Pendiente:** los chips de mención como átomos.
- Las paletas `/` y `@` no se abren a la vez: cada una mira solo su propio token.

**Imágenes** (`crates/quill-ui/src/lib.rs`: `is_image`, `add_images`; `apps/quill/src/images.rs`).

- **Fuentes:** Ctrl+V (Cmd+V en macOS) con el composer enfocado pega el texto del portapapeles del sistema o, si no tiene texto, su imagen (la lee egui-winit, con la función `clipboard` de eframe). En la terminal el texto va entre las marcas de bracketed paste cuando el programa las pidió. Arrastrar a la ventana y "+" › "Add files or photos". Son imágenes PNG, JPEG, GIF y WebP por extensión; los demás archivos siguen como @-menciones.
- **Límite:** 20 imágenes por mensaje. Las que sobran no entran y el composer dice "Maximum of 20 images allowed.". El motor revisa formato y tamaño al enviar.
- **Miniatura:** cuadrado de 85 px, esquinas redondeadas, recorte "cover". Se decodifica en un hilo, a 256 px como máximo, una sola vez por imagen; mientras tanto, o si no decodifica, muestra el icono de archivo. En la transcripción, las imágenes de un mensaje van encima del globo, alineadas a la derecha (`image:msg:{request}:{índice}`).
- **Pegadas:** se guardan como PNG en `attachments/`, junto a la base de datos de quill (`…/quill/attachments/pasted-<milisegundos>.png`), antes de mostrarse.
- **Visor:** un clic en una miniatura del composer (`image:open:{índice}`) de un mensaje (`image:msg:{request}:{índice}`) o de un paso (una imagen generada o una que leyó el agente, `image:tool:{call}`; las de las filas, `image:step:…`, abajo) abre la imagen sobre la ventana, con fondo oscuro (`ui.viewer`, `view::image_viewer`, `ImageViewer` de limn). Se cierra con ✕ (`image:close`), con Escape o con un clic en el fondo, que reportan el mismo control. Solo la imagen del composer lleva "Annotate image" (`image:annotate`); una imagen enviada no se anota. Anotar cierra el visor al instante y deja la imagen en `ui.annotating`, que abre el editor [cd5136de8 cierre instantáneo].
- **Origen y tamaño:** la imagen crece desde su miniatura en 350 ms y vuelve a ella en 250 ms (`cubic-bezier(.32,.72,0,1)`). El host toma el rectángulo de la miniatura del frame (`Viewer::origin`). A tamaño completo, la imagen se decodifica en un hilo, una sola vez al abrirse, a 2048 px como máximo (`apps/quill/src/images.rs` `full`): PNG, o JPEG a calidad 90 si el PNG pasaría de 8 MiB, el tope de limn. Mientras, muestra la miniatura.
- **Imágenes de los pasos** (`view::step_pictures`, `view::run_pictures`, `view::picture_strip`). Las de un paso son las que trajo su resultado (§3.6 de engine.md) o, si no trajo, el archivo que abrió un `Read` de una imagen; un paso fallido no muestra ninguna, como Desktop. Se cuentan por tanda: las llamadas a herramientas entre dos mensajes, que es lo que Desktop recorre y numera.
  - **Miniaturas al final de la fila.** La línea plegada de un grupo de pasos lleva a su derecha las imágenes del grupo: las tres últimas y, antes, "+N" (gris) por las demás. Cada miniatura mide 24 px de alto y lo que pida su forma hasta 44 px de ancho (16:9 hasta que el host lee el tamaño del archivo), recortada para llenar, esquinas de 4 px y borde fino; 4 px entre ellas y 8 px hasta el "+N". Con el grupo abierto la línea no las lleva: cada fila de paso lleva las suyas al final. Un clic en una miniatura no pliega ni despliega la fila.
  - **Tarjeta al pasar el puntero.** Sobre una miniatura se abre una tarjeta de 320 px encima de ella, con su borde derecho alineado al de la miniatura (debajo si arriba no hay sitio): la imagen a todo el ancho, de alto según su forma hasta 240 px (si no cabe, ajustada sobre el fondo neutro), y debajo el nombre del paso terminado ("Captured page", "Read logo.png") y, si la tanda tiene más de una, su lugar ("1 of 2"). Se cierra al salir de la miniatura y no se abre con el visor encima. Diferencias con Desktop: aparece al instante, donde la de Desktop espera un momento (el popover al pasar de limn no tiene demora), y muestra la miniatura de 256 px, algo blanda al ampliarse.
  - **Visor.** Un clic (`image:step:{call}:{índice}`, el índice entre las imágenes de ese paso) abre el visor en esa imagen, creciendo desde la miniatura, con toda la tanda como galería (`Viewer::gallery`, `Viewer::index`): cada imagen se nombra por su paso, debajo va "i of N", y las flechas (`image:previous`, `image:next`, o ← y → del teclado) la recorren y se detienen en los extremos. Al cambiar de imagen ya no vuelve a la miniatura de la que creció: se cierra con un fundido. Las miniaturas se decodifican como las de un mensaje (`image_paths` incluye las de los pasos, también las de los ayudantes), y el host lee a la vez el tamaño de cada archivo.
- **Pendiente con limn:** el ancho máximo de 40 rem y el alto de la ventana menos 64 px (hoy la imagen ocupa toda la zona libre), el nombre bajo la imagen (hoy es el título de la cabecera) y que las acciones vayan fuera de la esquina solo si la caja de la imagen deja sitio: limn no ajusta la caja al tamaño de la imagen.
- **Editor de anotaciones** (`crates/quill-ui/src/annotate.rs`, `view::annotator`). Mismo fondo oscuro que el visor. La imagen va centrada, ajustada a su proporción dentro de `min(640, ventana − 32)` de ancho y `ventana − 160` de alto, con un `Sketch` exacto encima (`annotate:sketch`).
  - **Barra flotante**, 24 px sobre el borde inferior: Pen; Shapes (Line, Arrow, Rectangle, Ellipse; el botón muestra la última forma, Rectangle por defecto, y su ▾ abre el menú); Ink color ▾ (Red `#E03131` por defecto, Blue, Green, Black, White); Line width ▾ (Thin 2, Medium 4 por defecto, Thick 8, Extra thick 12); Undo, Redo y Clear all, que aparecen cuando hay historial; Save, que espera a tener una marca; y ✕. La herramienta armada se ve pulsada.
  - **Teclas:** Ctrl+Z deshace, Ctrl+Shift+Z rehace, Escape pide cerrar. Tinta y grosor se recuerdan durante la ejecución; la herramienta vuelve a Pen al cerrar.
  - **Cerrar** (✕, Escape o el fondo) sin marcas cierra al instante; con marcas pregunta "Discard your annotations?" (Discard, destructivo, o Keep editing). Clear all mueve las marcas a rehacer, en orden, así que Redo las recupera (un Ctrl+Z después no).
  - **Guardar:** el host (`apps/quill/src/images.rs` `annotate`) decodifica el original a tamaño completo y pinta las marcas con `rasterize_sketch`, escalando el grosor por el ancho de la imagen dividido por el ancho en que se mostró. Lo guarda como PNG en `attachments/` con el nombre `<nombre>-annotated.png`, o `-annotated-2`, `-annotated-3`… si está ocupado, y lo pone en el lugar de la imagen del composer. Si no se puede escribir, el editor queda abierto y dice "Couldn't save your annotations. Try again.".
  - **Pendiente:** la herramienta de texto (necesita texto pintado en los píxeles), seleccionar y mover marcas, y las formas Step y Redact de Desktop.
- **Pendiente:** el anillo azul de selección y la ✕ que solo aparece con el puntero encima: el `Attachment` de limn la muestra siempre y no tiene anillo de foco.

| Usar de limn | Crear |
|---|---|
| `Composer` (`composer.rs`), `IconButton`/`Button` (`components.rs`), `DropdownMenu`, `DropdownMenuItem::{shortcut, description, submenu, radio, disabled}`, `MenuHeader` (`menu_header.rs`), `Switch`, `Chip`, `SuggestionList` (`suggestion_list.rs`), `AlertDialog` ("Trust this workspace?", "Change effort?"), `Toast` | limn: indicador circular de progreso (`Progress::ring`) para el contexto. quill: `ComposerChips` (fila de entorno), catálogo de modos/modelos/esfuerzos. |

### 2.7 Tarjeta de permisos

**Layout**

| Elemento | Medida | Componente |
|---|---|---|
| Tarjeta | 540–893 (≈353, ancho de contenido, máx = columna), 165 alto, padding 12, radio 8, hairline `#F1F1F1`, fondo canvas | `FramedCard` o `Card` (`card.rs`) |
| Título | "Allow Claude to run {acción}?" negrita 12 | `Text` `FontFamily::SansBold` |
| Descripción | 2 líneas muted 11, pitch 23 | `Text::muted` |
| Comando | caja 329×30, fondo `#F3F3F3`, radio 4, mono 10, padding 12 | `CodeBlock` sin etiqueta, o `Element::panel` + `Text::mono` |
| Botones | fila alineada a la derecha, alto 20, gap 10: **Deny** (secundario, fondo `#F2F2F2`) · **Always allow** primario `#191919` con keycaps `Ctrl` `Shift` `⏎` y chevron separado (botón dividido) | `ButtonGroup::align_end()` + `Button` + **SplitButton (falta)** + `Kbd` |
| Menú del chevron | "Allow once   Ctrl+Enter" | `DropdownMenu` |
| ≤ 420 px de contenedor | columna, botones a ancho completo | `ButtonGroup::stacked(true)` |

**Estados**

| Estado | Qué se ve |
|---|---|
| Pendiente | tarjeta + chip "Needs your input" + punto `warning` en la fila de sidebar |
| Resolviendo | botones deshabilitados, spinner en el pulsado (`Button::loading(true)`) |
| Error de entrega | vuelve a pendiente + `Notice` danger bajo los botones |
| Resuelto | la tarjeta sale (`Collapse::new(id, None)` cierra el hueco) y queda una línea en el transcript ("Allowed once" / "Denied") |
| Varias en cola | contador "1 of N" (`Pager`, fase 2) |
| "Always" deshabilitado | botón deshabilitado + `TooltipSpec` con el motivo |

**Atajos:** Esc = Deny · Ctrl+Enter = Allow once · Ctrl+Shift+Enter = Always allow. Se resuelven en `quill_ui::intercept` mientras haya un pedido visible en la sesión enfocada (no dependen del foco del botón).

| Usar de limn | Crear |
|---|---|
| `FramedCard`/`Card`, `ButtonGroup` (`button_group.rs`), `Button` (`loading`, `disabled`, `tooltip_with`), `Kbd`, `DropdownMenu`, `Collapse` (`collapse.rs`), `Notice`, `Chip` | limn: `SplitButton` (P1). quill: `PermissionCard` (variantes genérica / plan / pregunta), mapeo herramienta→título. |

**Plan propuesto** (`plan_card`). Hay dos casos:
- **El agente espera la respuesta** (Claude Code, `ExitPlanMode` como permiso): "Claude proposed a plan", Open plan, Reject, Revise…, Accept y el botón dividido de los modos.
- **El agente termina el turno con el plan** (Codex en modo Plan, `docs/codex.md` "Modo Plan"): no espera nada. Si la sesión sigue en un modo de tono `Plan` y lo último después del mensaje de la persona es un `ExitPlanMode` sin permiso propio, la misma tarjeta queda bajo el turno: "{Agente} proposed a plan", Open plan, Revise… e "Implement in {modo}".
  - "Implement in {modo}" es un botón dividido; su menú tiene los demás modos ofrecidos que no planifican. Implementar cambia al modo que la sesión tenía antes de planificar (o al del agente por defecto) y manda "Implement the plan." como mensaje de la persona.
  - Revise… manda lo escrito como el mensaje siguiente, sin salir del plan.
  - La tarjeta se va cuando la persona escribe o cambia de modo. No tiene atajo de teclado.

**Pregunta** (`question_card`). Una pregunta por página, con sus opciones y "Other" con un campo para las palabras de la persona. La entrada puede traer:
- `other: false`: sin "Other".
- `secret: true`: el campo se enmascara.
- Ninguna opción: solo el campo, "Type your answer"; Next / Submit se habilita al escribir.
- `blocking: false` (una pregunta sin la que el agente sigue): tras un minuto sin que la persona la toque, el pie muestra "Skipped in 42s" junto a Skip; al llegar a cero, `Ui::due` la contesta sin respuestas (`Decision::Answer({})`). Tocarla apaga la cuenta. El reloj vive en `Ui::unanswered` y lo pone `Ui::tick`.

### 2.8 Panel derecho (Environment / Context / Connectors / Changes)

| Tarjeta | y | Contenido | Componente |
|---|---|---|---|
| Environment ▾ | 36–170 | filas 25: "This computer" + link "Save"; carpeta "work"; rama "master"; "Remote Control" + `Switch` | `Card` + `ListRow` (`.icon`, `.trailing`) + `Switch` |
| Context ▸ | 177–212 | "7%" + mini barra/indicador azul | `Collapsible` + `Progress` (ring falta) ; al abrir → `ContextWindow` (`context_window.rs`) |
| Connectors ▸ | 219–255 | "1 connected" muted | `Collapsible` (fuera del MVP: puede ocultarse) |
| Changes ▾ | 262–320 | ícono ± + `+1 −1` + botón "Commit changes" (secundario, deshabilitado mientras corre) | `Card` + `DiffStat` + `Button` `Mini` |

**Cuentas en vivo.** Los `+N −M` de la barra de la rama (sobre el composer, junto a Create PR) y de la tarjeta Changes son lo sin commitear, y cambian mientras el agente escribe código, archivos nuevos incluidos: el motor lo vuelve a leer tras cada paso que tocó archivos (engine.md §8.2) y cada cifra cuenta hacia arriba o hacia abajo hasta la nueva, como en Desktop (`Element::rolling_number`: curva ease-out, un número entero en cada cuadro; con movimiento reducido salta). Un lector de pantalla oye las cifras a las que se llega ("+26 −6", el nombre del control de la barra y del de la tarjeta), no las de paso. Las dos escriben las cifras con separador de miles. Sin repositorio, la tarjeta suma lo que cambiaron los pedidos de la sesión.

Medidas: ancho 206, a 4 px del borde derecho, tarjetas radio 8, gap 7, padding horizontal 14, títulos 11 px semibold con chevron. **Flota** sobre el área principal (no desplaza la columna); a anchos chicos debe colapsar a `PeekPanel` (`peek_panel.rs`) para no tapar la columna (regla quill: si `área − 2·206 < 541`, se colapsa).

### 2.9 Panel de diff

| Elemento | Medida | Componente |
|---|---|---|
| Panel | 692–1408 (716) con chat 479 a la izquierda; divisor 1 px | `Element::row` + `ResizeHandle::vertical("split:main")` |
| Cabecera | alto 29: "master → working tree ▾" (rama base → destino, menú de base) ; a la derecha buscar, árbol, ⋯, expandir, cerrar (pitch 23) | `PanelHeader` (`PanelHeaderSizing::PANE`) con `.leading(..)` y `.trailing(..)`, `.dismiss(id, tooltip)` |
| Cabecera de archivo | alto 24, fondo `#F2F2F2`; ▾ + `<>` + ruta 11 px; a la derecha `+1−1`, "visto" ✓, abrir en editor ↗, ⋯. Un archivo sin líneas que mostrar dice por qué junto a su nombre: "Binary file", o "Too large to show" si tiene cuentas y ningún hunk (un archivo nuevo de más de 2000 líneas) | **DiffFileHeader (falta)** — hoy `Collapsible` + `DiffStat` + `IconButton` |
| Filas | alto 14, mono ~11; columnas nº viejo x=720, nº nuevo x=744, código x=759 | `CodeDiff::new(id, lines).font_size(11).language(..)` |
| Fondo de fila | agregado `#E7F4E7`, quitado `#FEEEEF` | `CodeDiff::diff_backgrounds(true)` con colores quill |
| Resaltado por palabra | agregado `#C8E7CD`, quitado `#F7CFD8` | **falta en `CodeDiff`** |
| Barra de margen | 2–3 px en x=692: verde `#208635` / rojo | **falta** (o tinte de gutter) |
| "Show more lines" | ▾ + 11 px muted, expande contexto oculto | **falta** (fila de expansión de hunk) |
| Layout | unified en la captura; side-by-side en Desktop (por defecto split) | MVP: unified. Side-by-side → fase 2 en limn |

**Estados:** sin cambios (`Empty` "No changes"); cargando patch (`Skeleton` por archivo); archivo enorme (colapsado con "Load diff"); binario ("Binary file"); error de git (`Notice` "Git isn't working").
**Interacciones:** clic en cabecera colapsa; ↗ abre en editor externo (`Intent::OpenInEditor`); expandir maximiza el panel (oculta el chat); ✕ cierra; tamaño persistido en `Ui`.
**Muchos archivos (hecho):** la lista es virtual (`Element::measured_list` de limn, `diff:scroll`): solo se arman las filas a la vista, cada una la banda del archivo y, abierta, su diff. El menú "Open in" se arma solo para la fila que lo tiene abierto. Un diff de más de 300 archivos o 20 000 líneas cambiadas empieza con todo plegado y el aviso "Files are collapsed for large diffs. Select a file to expand it." (umbrales inferidos); uno menor empieza con todo abierto. Un clic en la banda invierte el estado inicial de ese archivo. Un archivo sin líneas que mostrar (binario: "Binary file" junto al nombre) conserva su banda, la flecha atenuada y "+0 −0", y no se abre. Con 557 archivos, un cuadro (vista y layout, release) bajó de ~17 ms a ~2 ms.
**Atajos:** toggle del diff (combinación desconocida) → quill propone Ctrl+Shift+D; Esc cierra si tiene foco.

| Usar de limn | Crear |
|---|---|
| `CodeDiff` (`code_diff.rs`), `PanelHeader` (`panel_header.rs`), `ResizeHandle`, `TreeRow`/`TreeView` (`tree_view.rs`, árbol de archivos fase 2), `SearchField`/`FindBar` (`find_bar.rs`) | limn (P1): extensiones de `CodeDiff` (palabras, hunks expandibles, barra de margen, cabecera de archivo). quill: parser de unified diff → `CodeDiffLine` (en `quill-git`), diff por palabras (crate `similar`). |

### 2.10 Terminal

| Elemento | Medida | Componente |
|---|---|---|
| Panel | 436–844 bajo el diff (50/50) o solo en el panel derecho; divisor horizontal | `ResizeHandle::horizontal("split:right")` |
| Cabecera | alto 31: "Terminal" + cwd muted ("work") + `+` ; a la derecha ⋯, expandir, cerrar | `PanelHeader` `PANE`; con varias pestañas → `TabStrip` (`tab_strip.rs`) en `.leading` |
| Cuerpo | padding 8, mono ~11, interlínea ~14–16; aviso de reconexión muted ("[shell reconnected — replaying buffered output]"); prompt; cursor bloque accent | **TerminalView (falta)** |
| Paleta ANSI | 16 colores + cursor + selección (§3.1) | tokens quill |

**Estados:** iniciando (cursor de reposo al 50 % de opacidad, parpadeo sólo sin movimiento reducido); vivo; proceso terminado ("[process exited with code N]" muted + botón "Restart"); reconectado (línea muted); error de spawn (`Notice`); cerrando con comando activo → `AlertDialog` "A command is still running in {name}…".
**Interacciones:** foco por clic; con foco, `intercept` manda `UiEvent::Key`/`Text`/`Paste` a `Intent::TermInput` en vez de a limn (salvo atajos reservados de quill); selección con arrastre → `Copy`; rueda = scrollback; redimensionar → `TermResize` (cols/rows desde el tamaño del layout y el ancho de celda medido); Ctrl+Shift+←/→ mueve pestañas; Delete/Backspace sobre la pestaña la cierra.

| Usar de limn | Crear |
|---|---|
| `PanelHeader`, `TabStrip`/`TabItem`, `ContextMenu`, `AlertDialog`, `caret_blink.rs`/`motion.rs` (parpadeo), `Element::drawing` (base para pintar celdas), `UiEvent::Key`/`Text` + `tree.focused_id()` | limn (P0): `TerminalView` genérico que pinta un `TerminalScreen` del host. quill (`quill-pty`): PTY (`portable-pty`) + parser VT (`alacritty_terminal` o `vt100`) → snapshot. |

### 2.10b Archivos (Files)

Es el tile de archivos de Desktop: árbol, pestañas y archivo en un solo pane. Las medidas salen de capturas. quill pone adentro el editor de quill-ide (`quill_components::code`).

| Elemento | Medida | Componente |
|---|---|---|
| Botón | ícono de lista en la barra de arriba, antes del globo; "Files", Ctrl+Shift+E (el de un editor; inferido) | `toggle` |
| Panel | la mitad del espacio al abrir, como el browser (inferido) | `Pane::Files` |
| Cabecera | las pestañas de los archivos abiertos y, a la derecha, el botón del árbol ("Show file tree"/"Hide file tree"), expandir y cerrar, como Desktop. Las pestañas son el sistema compartido con el navegador y las terminales (ver más abajo) | `tabs_header`, `pane_tab` |
| Árbol | columna a la **derecha** del archivo, de 240 px, como Desktop (Desktop permite entre 160 y 640). Si el pane mide menos de 400, ocupa todo el pane mientras está abierto. Filas de 24 px con líneas guía por nivel. Las carpetas llevan solo un chevron; los archivos de código y configuración llevan `<>` y el resto una hoja (la lista de extensiones es inferida); sin colores de git | `tree::view(.., Look::Desktop, ..)` |
| Ruta del archivo | `Breadcrumb::fade_overflow()` de limn: los segmentos no se colapsan en "⋯"; lo que no entra se desvanece en el borde y se ve más al ensanchar el pane | `file_view` |
| Vacío | "Open files appear here" y el texto de Desktop en dos líneas centradas y cortadas al ancho | `file_view` |
| Barra del archivo | ruta como breadcrumb: las carpetas muted la muestran en el árbol y el nombre copia la ruta. Herramientas: Reload file, Preview/View source (Markdown), Find in file, Copy file contents; con cambios, Cancel y Save | `limn::Breadcrumb`, `tool_surface` |
| Cuerpo | el editor de quill-ide (sintaxis, búsqueda) o Markdown renderizado; "This file can't be shown as text." si no es texto | `code::editor`, `code::markdown_preview`, `find::bar` |
| Vacío | "Open files appear here" / "Pick a file in the tree, or click a file path in the conversation." | — |

**Interacciones:**
- **Click:** en una carpeta, la abre o la cierra en el lugar (se lista la primera vez). En un archivo, o en una ruta de la conversación, lo abre en una pestaña (o va a la suya, si ya está abierta) y lo muestra en el árbol.
- **Cerrar una pestaña con cambios:** pregunta lo mismo que Desktop: "Discard unsaved changes?" / "Your edits to {name} have not been saved. Leaving this tab discards them.", con Discard o Keep editing. Desktop no marca las pestañas con cambios y quill tampoco.
- **Menú contextual del árbol:** algo nuevo (con `/` al final, una carpeta), renombrar, copiar, cortar, pegar, copiar la ruta relativa, mandar a la papelera del escritorio (con confirmación).
- **Teclado:** con foco en una fila, flechas y las teclas de nvim-tree (`hjkl`, `a`, `r`, `d`, `y`, `x`, `p`). En el editor, Ctrl+F busca y Ctrl+S guarda. Escape cierra la búsqueda.
- **Cambios sin guardar:** un archivo con cambios sin guardar no se mueve ni va a la papelera. Si se renombra o se mueve un archivo abierto, su pestaña lo sigue; si va a la papelera, la pestaña se cierra. Lo que escribe el agente se vuelve a leer en las pestañas sin cambios.
- **Volver a listar:** al abrir el pane y al terminar cada turno, porque el agente puede haber cambiado la carpeta.

**Código compartido con quill-ide:**
- `quill_components::tree`: estado, teclas, menús, diálogos y vista. No toca el disco: devuelve qué listar o hacer, y avisa qué se abrió, movió o mandó a la papelera.
- `quill-files`: listado (respeta `.gitignore`), operaciones y búsqueda (`find`: los archivos que ve git, la coincidencia por letras en orden y las líneas que contienen unas palabras; antes vivía en quill-ide, que ahora lo usa de acá). Lo corre el engine (`Intent::ListDir`, `Intent::FileOp`, `Intent::FindFiles`) o el hilo de disco de quill-ide.
- **Filtro del árbol** (como Desktop): arriba del árbol, el campo "Filter, or ? to search in files", que se puede vaciar ("Clear filter").
  - **Por nombre:** muestra los archivos cuyo camino contiene las letras en orden, con la carpeta en gris; clic abre el archivo.
  - **Con "?" adelante:** busca en el contenido, con mayúsculas que cuentan solo si las palabras tienen alguna. Agrupa por archivo y muestra hasta 5 líneas por archivo y "More matches in this file.". Clic en una línea abre el archivo con el cursor en esa línea y a la vista.
  - **Mensajes:** "Type after ? to search in files", "No matching files", "No matches in file contents" y los avisos de tope ("Only the first 200 results are shown…", "Only 200 matches are shown…").
  - **Diferencia con Desktop:** su búsqueda en contenido acepta expresiones regulares; la de quill busca texto literal.
  - El motor recorre la carpeta en cada búsqueda (atajo: con carpetas muy grandes convendría un índice), y la UI descarta respuestas a palabras viejas.

**Pestañas compartidas (Files, navegador, terminales):** las tres usan `pane_tab` y `tabs_header` de `quill-components/pieces.rs`. Cada pestaña es una pastilla de 28 px, radio 8, con su nombre de hasta 200 px (el de una terminal sin nombre propio sigue con su carpeta en gris) y su X. La activa va sobre el relleno activo. La tira mide lo que sus pestañas hasta el espacio libre (`max_width`); si no entran, se desplaza en horizontal con la rueda (`Element::horizontal_scroll`), desvanece el borde que esconde pestañas (`horizontal_edge_fade`) y mantiene la activa a la vista (`reveal_horizontal_item`). El "+" de una pestaña nueva queda fuera del scroll, siempre visible justo después. Una sola terminal sigue mostrándose como en Desktop, como "Terminal" y la carpeta, sin pastilla.

**Falta:** cambiar el ancho del árbol arrastrando, pestañas de vista previa en cursiva, y ver los cambios en disco entre turnos mientras el pane está abierto (quill-ide los vigila cada segundo).

### 2.11 Settings

Medido a 1× sobre Desktop (capturas del 2026-10-26: General, System, Claude Code, la columna).

| Elemento | Medida | Componente |
|---|---|---|
| Modal | 1024×772 (alto: ventana − 72, entre 320 y 772); cabecera de 44 con "Settings" y ✕, hairline debajo; Escape cierra | `limn::Modal` con `flush_body` |
| Columna | 211 de ancho, borde de 1 px; buscador compacto arriba; títulos de grupo muted (18 arriba, 8 abajo); filas de 32 con radio 8: ícono de 16 a 9 px del borde y nombre de 14 px a 15 px del ícono; la página abierta sobre `#DADADA` | `SearchField`, filas propias |
| Sección | título de 15 px; 53 px del título al texto de la primera fila; 40 entre secciones | — |
| Fila | título y descripción de 14 px, las líneas de la descripción a 20 px, 24 px del título a la descripción; alto mínimo 52, o 68 con descripción; control a la derecha; hairline `#F3F3F3` que va de la copia al control (no de borde a borde) | `ActionRow` (`subtitle_line_height`) |
| Select | valor de 14 px y chevron, sin caja; abre el menú de quill | `settings::select` |
| Interruptor | 36×20; encendido azul `#257DE0`, apagado gris claro `#CECECE`, siempre con el pulgar blanco | `Switch` Accent con `ink` y `off_ink` |
| Tema | tres íconos de 26 px en un riel `#F3F3F3`; el elegido es una ficha blanca con borde `#E7E7E7` | `SegmentedControl` con `selection_border` |
| Botón | 28 de alto, relleno `#F2F2F2`, radio 8 ("Open", "Choose…") | `setting_button` |
| Valor | en la tinta del texto, no muted (la versión) | — |
| Error al guardar | "Couldn't save the setting. Try again." (texto de Desktop) arriba de la página, en rojo, con el motivo muted; se va con el siguiente cambio o al cerrar | `Event::SettingsNotSaved` |

**Páginas.**
- Preferences:
  - General: tema, tamaño del texto (S 13, M 14, L 15, como Desktop) y ancho del transcript (Narrow 770, Medium 960, Wide 1280), vista del transcript por defecto (§2.4c), movimiento, fuente de la interfaz (System, la del escritorio, o Ubuntu, la que trae limn; OpenDyslexic de Desktop no, porque habría que incluirla) y fuente de código (un nombre que `fc-match` resuelve, para el código y la terminal, que se escribe y se guarda con Enter; sin `fc-match`, como en Windows, quedan las que trae limn). El viejo `"wide": true` se lee como Medium.
  - Sessions: prefijo de rama (por defecto `quill`; hoy lo usa el pedido de Create PR cuando la sesión está en la rama principal, y lo usarán las worktrees), mantener la computadora despierta (por ahora solo en Linux, con `systemd-inhibit`) y también con batería (`/sys/class/power_supply`, leído cada minuto mientras importa), tipo de pull request, abrir los pull requests en el navegador del sistema (los links con `/pull/N` o `/merge_requests/N`) y archivar al mergear su pull request (la dirección sale de lo último que dijo el agente en un turno; con `gh pr view` cada 5 minutos) y "Archive inactive sessions" (Never, 1, 7 o 30 días). Cada 10 minutos la app archiva las sesiones quietas ese tiempo, salvo las fijadas y las que están trabajando; quietas se mide desde el último mensaje.
  - Sessions, grupo "Notifications" (como Desktop): "Permission requests" (un pedido de permiso) y "Questions" (una pregunta con opciones) pueden ser Off, Badge only o Banners; "Task complete" (el turno termina) puede ser Off o Banners, y un Badge guardado se lee como Banners. "Notification sound" elige "quill" o None y vale para los banners: con "quill", el banner sale callado y suena un sonido propio de quill, uno cuando una tarea termina (dos tonos suaves que bajan una cuarta, sol–re) y otro cuando una sesión necesita a la persona (los mismos tonos subiendo, re–sol), de unos 380 ms cada uno. Desktop toca el sonido del sistema; quill no usa sonidos ajenos: el motor los sintetiza (`quill-engine/src/chime.rs`, senos con un poco de segundo y tercer armónico, ataque de 6 ms y caída de 90 ms, un tercio de la escala) y los toca fuera del hilo de la UI con lo que trae el sistema: `pw-play`, si no `paplay`, si no `aplay` en Linux (el WAV va por stdin); `afplay` en macOS (desde un archivo en la carpeta temporal); en Windows, `System.Media.SoundPlayer` de PowerShell, sin ventana de consola. Sin ninguno, no suena nada. No suena cuando la persona mira esa misma sesión con la ventana al frente, porque entonces no hay banner. El textual de cada uno es el de la descripción del ajuste, con "An agent" en lugar de "Claude". Lo decide `quill_ui::notice`: un evento así de una sesión que no está en pantalla, o con la ventana sin foco, da un banner con el título de la sesión; una sesión en pantalla con la ventana al frente no da nada. El turno de un chat lateral (`/btw`) no cuenta como "Task complete". El banner lo muestra el notificador del sistema, sin dependencias, y pulsarlo lleva a esa sesión: la ventana de quill se restaura, pide el foco y abre la sesión como un clic en su fila (`Event::NotificationClicked`, `quill_ui::banner_clicked`). En Linux, `notify-send --app-name=quill --hint=boolean:suppress-sound:true --action=default=Open --wait` espera al banner en una tarea del motor e imprime `default` cuando se lo pulsa; si `notify-send` no está, o es anterior a `--action` (libnotify 0.7.9), va el mismo pedido `Notify` por D-Bus con `busctl --user`, que se ve pero no abre la sesión. Bajo Wayland el compositor puede limitarse a marcar la ventana en lugar de traerla al frente (inferido). En Windows, un toast de PowerShell con su AppUserModelID (inferido: queda con el nombre de PowerShell hasta que quill registre el suyo), callado, cuyo `Activated` escucha ese mismo PowerShell hasta diez minutos: un clic en ese tiempo, en el toast o en el centro de notificaciones, abre la sesión; después ya no (inferido, sin probar en Windows; hacerlo siempre pediría registrar quill como app con su propio activador). En macOS, `osascript` con `display notification`: el banner se ve, pero un clic no llega a quill; para eso hace falta una app firmada con su propio identificador, que quill hoy no tiene. Badge son las marcas propias de quill, en la barra lateral y la ventana, que ya muestran una sesión que espera o no leída (inferido: el badge de Desktop es el contador del dock, que quill no tiene). "Draw attention" (llamar la atención a la ventana) va en el mismo grupo, al final, y se pide con cada aviso que no sea Off, banner o badge, cuando la sesión no está en pantalla.
  - Browser: Chromium, vista nativa, sitios permitidos y "Open links in built-in browser". Apagado, los links del chat abren el navegador del sistema (con `quill_files::open_in_system`). Clic derecho en un link ofrece las dos cosas y copiar.
- Agents: una página por agente, con el logo que trae su adaptador (Claude Code: el de svgl.app, monocromo como los demás íconos), con su programa y versión y con lo que arranca una sesión nueva (modelo, esfuerzo, modo, modos que se saltean aprobaciones), según lo que dice `AgentCapabilities`. Debajo, el grupo "Sessions" con las opciones que declara el adaptador (`Adapter::options`: un interruptor si no tiene opciones, un select si las tiene) y las variables de entorno (`KEY=value` por línea). Se guardan en `agents.<id>.options` y `agents.<id>.env` y valen para las sesiones nuevas. La UI no sabe qué significan: el adaptador de Claude Code las traduce a `--settings` (output style, sandbox local, sandbox estricto, como las opciones de Desktop en `c7357c32b`); otro agente declara las suyas.
- This computer: System (versión, abrir al iniciar sesión, carpeta de datos).

- **Logos.** Cada conector y cada plugin muestra su logo en el recuadro de 36 px cuando quill ya lo tiene; si no, sigue el ícono de siempre, y el recuadro conserva su fondo para que un logo oscuro se vea en el tema oscuro. quill no trae ningún logo: al abrir Connectors, Plugins o el diálogo `/mcp` pide los que faltan, una vez por cada clave (`mcp:<nombre>`, `plugin:<nombre>`), y el motor los busca en la red (ver `engine.md` §12). Se apaga en Settings › General con "Logos of connectors and plugins"; apagado, no se pide ninguno y quedan los íconos.
- Customize ya no es una columna de Settings: es una página propia (`extensiones.md` §12), y Settings › General conserva «Customize…». Lo que sigue describe las secciones y filas que la página conserva. Skills y Plugins tienen diseño propio, como las de Desktop (capturadas en oscuro a 1× el 2026-10-26; los valores en claro son inferidos). Todo se lee y se cambia con los comandos del CLI. Cada página se lee de nuevo al abrirse y al cambiar de carpeta.
  - **Header:** título en negrita de 22 px; la búsqueda propia de la página (256×28, sobre `#2A2A2A`); y botones de 28 de alto con radio 6. El botón principal, "+ Add ▾", usa la tinta del texto como relleno, igual que Desktop.
  - **Por CLI:** Skills, Plugins y Connectors tienen una sección por agente (§2.3c). Los skills van por origen debajo ("User", "Project", "System", "Plugin"), sin prefijo del agente. Una página no se queda en "Looking for skills…" por un agente sin alcances de instalación: lo que cuenta es que su lista haya llegado (`Extensions.loaded`). Lo que el programa no puede hacer no se ofrece: el menú de un plugin de Codex solo trae "Remove" (sin Update ni Turn off, `ExtensionKinds`), y con Codex el estado de un skill no pide una carpeta de proyecto porque es de la persona.
  - **Skills globales:** "+ Add ▾" en el header de Skills ofrece "New skill for all agents" (nombre y descripción; crea `quill/skills/<nombre>/SKILL.md`) e "Import a folder for all agents" (se escribe la ruta de una carpeta con `SKILL.md`; quill guarda una copia). El nombre son palabras en minúscula unidas con guiones. Aparecen en el menú `/` de cada sesión (`/quill:<nombre>` en Claude Code, `$<nombre>` en Codex).
  - **Discover:** la ventana trae un botón por agente arriba (Claude Code, Codex): se instala en el agente elegido, que empieza siendo el de una sesión nueva. "Add marketplace" pregunta "Add to" con un botón por agente, y "Manage marketplaces" los lista por agente.
  - **Tarjeta:** "Add your first plugins" sobre `#2A2A2A`, radio 12, con 16 px de margen. Aparece mientras no haya ningún plugin instalado y lleva el botón "Discover plugins". No usa la ilustración de Desktop.
  - **Sección:** título de 16 px, el contador en una pastilla y, al final, el botón de la carpeta (contorno `#4B4B4B`, 34 de alto; 28 en el header).
  - **Fila (63 px):** ícono de 16 sobre un recuadro de 36 (`#0B0B0B`, radio 8); el nombre en 15 px a 16 px del recuadro; debajo, en 13 px atenuado, "from {origen} · {descripción}" en una sola línea; al final, el estado y el menú "⋯" (28×28); y una línea `#272727` debajo.
  - **Cargando:** spinner con "Looking for skills…" o "Looking for plugins…".
  - **Plugins:**
    - "In this project" con el botón de la carpeta, o "No plugins installed for this project yet.". Debajo, "Your plugins".
    - El "⋯" de cada plugin tiene Update, Turn off/on y Remove (en rojo; pregunta "Remove plugin?").
    - "+ Add ▾" tiene "Add marketplace" (abre un diálogo con el aviso de confiar, el campo URL y "A GitHub owner/repo or a Git repository URL.") y "Manage marketplaces" (la lista, y el "⋯" de cada uno con "Check for updates" y "Remove").
    - **Discover** abre "Directory", un modal aparte como el de Desktop: "Plugins" en la columna (items de 200), la búsqueda a lo ancho, "Filter by ▾" por marketplace y "+" para agregar uno. Debajo van tarjetas de a dos (los 60 más instalados, o lo que encuentre la búsqueda), cada una con su marketplace, sus instalaciones y "+". En un proyecto, ese "+" abre el menú de dónde instalar; si ya está instalado dice "Added".
  - **Skills:**
    - El header lleva la carpeta.
    - Los skills van agrupados por origen.
    - Si el estado no es On, se muestra al final de la fila.
    - El "⋯" ofrece los cuatro estados como opciones excluyentes. Si no se pueden cambiar, aparecen desactivados con el motivo: el bloqueo que informa el CLI, o "Choose a project folder…".

Las páginas de cuenta y nube, y las que son solo de Anthropic en Desktop, no se incluyen. Los servidores MCP se ven por sesión con `/mcp` (§2.11b).

**Buscador.** Filtra las filas de todas las páginas por título o descripción y las agrupa como "Página › Sección". Sin resultados muestra "No settings match your search.".

**Tema oscuro.** Los valores son inferidos: no hay captura oscura de Settings.

**Falta:** recorrer la columna con el teclado y resaltar lo encontrado por el buscador.

### 2.11b Servidores MCP de la sesión (`/mcp`)

Es el diálogo de Desktop "MCP servers in this session". Se abre al escribir `/mcp` en el composer con una sesión elegida. Los datos son del agente: el adaptador los traduce de `mcp_status` del CLI (`AgentCommand::McpStatus`) y la UI no sabe de qué CLI vienen. Solo aparece si el agente lo ofrece (`AgentCapabilities::mcp`).

| Elemento | Medida | Componente |
|---|---|---|
| Modal | 720 de ancho; título "MCP servers in this session"; "Done" abajo a la derecha; Escape y clic afuera cierran | `limn::Modal` |
| Tabla | columnas Server (flexible: la inicial del servidor en un avatar de 24, porque quill no muestra logos de terceros, y el nombre), Type (160: el origen en una pastilla con contorno) y Status (160); filas de 44 | `limn::Table` |
| Server | el nombre sin el prefijo del agente; debajo, "Provided by the {plugin} plugin" si viene de un plugin | — |
| Type | User, Project, Local, Managed, Plugin, Account (conectores de la cuenta, inferido: Desktop los lista aparte) y App (las herramientas de quill) | `McpSource` |
| Status (160) | lo mismo que la tabla de Connectors de Desktop: ✓ si está conectado, spinner mientras conecta, "Connect" si se puede iniciar sesión desde quill, "Needs authentication" con un tooltip si no, "Reconnect" (con el error en el tooltip) si falló, y "Turn on" si está apagado. "Connected · N tools" queda para el detalle | `McpStatus` |

**Estados.**
- Sin respuesta (el agente no está corriendo): "Loading this session's MCP servers… If they don't appear, send a message in the session to start it." (texto de Desktop).
- Sin servidores: "This session has no MCP servers."
- Durante un turno, los botones se desactivan con el tooltip "Available when the current turn finishes".

**Refresco.** Pide el estado al abrir, después de cada acción y cada 2 s mientras está abierto (`Ui::due`). El intervalo es inferido: Desktop también consulta periódicamente, pero no se encontró a qué ritmo.

**Iniciar sesión (OAuth)**, como en Desktop:
- **Connect / Re-authenticate.** Mandan `mcp_authenticate` sin dirección de retorno. El CLI escucha en localhost la respuesta de la página y reconecta el servidor solo. La URL que devuelve la abre el engine en el navegador, y solo si es https.
- **Mientras dura**, la fila dice "Opening sign-in…" y después "Complete sign-in in your browser…". El detalle ofrece Reconnect con "Finished signing in from your browser? Reconnect to pick up the new credentials.". Si el servidor no necesitaba iniciar sesión, se reconecta en el acto. El inicio de sesión termina cuando cambia el estado del servidor.
- **Clear authentication** pregunta primero, con el texto de Desktop: "Clear authentication for {name}?" / "The stored credentials for this server are revoked and deleted. You can authenticate again afterwards.". Después muestra "Authentication cleared for {name}.".
- **Botones según el estado:** needs-auth → Connect; failed → Connect y Reconnect; connected → Re-authenticate y Clear authentication (solo en el detalle); apagado → Turn on.
- **Quién puede iniciar sesión desde quill:** solo los servidores http o sse cuya sesión no está guardada en otro lado. Los conectores de la cuenta y los stdio dicen dónde iniciar sesión.

**Detalle de un servidor.** Se abre con un clic en su fila y se vuelve con "All servers". Muestra el nombre, el plugin, el tipo, el estado y sus botones. Debajo van sus herramientas, agrupadas por lo que su servidor dice que hacen: "Read-only tools", "Write/delete tools" y "Other tools".

Esta lista es de quill. En una sesión de Code, Desktop solo muestra cuántas herramientas hay. El CLI no manda las descripciones de las herramientas en `mcp_status`.

**Falta:**
- Apagar un servidor o una herramienta. El CLI no tiene un pedido por herramienta: solo reglas `permissions.deny`, que modificarían la configuración de la persona.
- Pegar a mano la URL de retorno (`mcp_oauth_callback_url`).

### 2.12 Atajos del MVP (tabla única, en `quill_ui::keybinds`, editable como en RUNE)

| Atajo | Acción | Origen |
|---|---|---|
| Ctrl+N | Nueva sesión | quill |
| `b` (fuera de campos) | Alternar sidebar | |
| Ctrl+K | Paleta (fase 2) | |
| Enter / Shift+Enter | Enviar / nueva línea | composer |
| Esc | Parar turno / cerrar menú / Deny si hay permiso | |
| Ctrl+Enter | Allow once | |
| Ctrl+Shift+Enter | Always allow | |
| 1–4 (menú de modelo abierto) | Elegir modelo | |
| Ctrl+O | Vista del transcript: Normal → Thinking → Verbose (sin Thinking si el agente no lo da) | Desktop (`cycleTranscriptMode`) |
| Ctrl+` | Alternar terminal | quill (original sin combinación conocida) |
| Ctrl+Shift+D | Alternar diff | quill |
| Ctrl+, | Settings | quill |

### 2.13 Bienvenida y recorrido (`quill_ui::tour`)

Tras la bienvenida (la pluma y Get started) y el paso de apariencia, un recorrido de seis burbujas señala controles reales de la ventana, en orden: el composer, el modelo y su esfuerzo, el modo de permisos, New, los botones del panel derecho, el terminal y los cambios, y el menú de cuenta. Cada burbuja dice qué hace el control en pocas palabras, con "1 of 6", Back, Next (en el último, Done) y "Skip tour", en inglés como el resto de la interfaz; Esc o la X lo cierran como Skip. Mientras corre, el resto de la ventana no recibe entradas. Si un control de un paso no está en pantalla (el panel cerrado, la sidebar oculta con Ctrl+B, la ventana angosta), el host pasa al siguiente en el mismo cuadro. Al terminar o saltarlo, Done o Skip guardan `toured` en los settings; una persona que ya tenía `welcomed` y no `toured` lo ve una vez al arrancar, y "Take the tour" en el menú de cuenta lo vuelve a abrir. Un arranque con agente de prueba no lo muestra, como tampoco la bienvenida, salvo que `QUILL_WELCOME` la pida.

### 2.13b Paso de apariencia (`quill_ui::appearance`)

**Diseño propio de quill, sin precedente en Desktop**: Desktop no tiene paso de tema en su onboarding; el tema solo se elige en Settings › General › Appearance (control de iconos System / Light / Dark, por defecto System).

Va justo después de "Get started" y antes de todo lo demás, en una sola secuencia: bienvenida → apariencia → inicio de sesión (si el agente está desconectado) → recorrido. A ventana completa y con el lenguaje de la bienvenida: la pluma de quill (56 px), "Choose your look", dos líneas muted y tres tarjetas de 196×124 (System, Light, Dark), cada una una miniatura de la ventana de quill (tira de la sidebar, un mensaje y una respuesta, la barra del composer) dibujada con elementos de limn en los colores fijos de ese tema; System la parte por la mitad, clara a la izquierda y oscura a la derecha. La elegida lleva un aro de 2 px; elegir aplica el tema en vivo (`settings.theme`, guardado con `SaveSettings`, el host lo aplica en el cuadro siguiente, igual que Settings) y el botón "Continue" (320×44, relleno) cierra el paso. Se guarda `looked` en los settings, como `welcomed` y `toured`, para que salga una sola vez. Quien ya tenía `welcomed` antes de que existiera el paso no lo ve (se lee en los primeros settings de la ejecución). Un arranque con agente de prueba no lo muestra, salvo `QUILL_WELCOME`. "Take the tour" sigue en el menú de cuenta.

No usa nada que limn no tenga.

### 2.13c Saludo de la página nueva (`quill_ui::greeting`)

**Diseño propio de quill**: centrado sobre la tarjeta del composer en el bloque de §2.3, a 40 px y sin icono (Desktop lo pone arriba de la columna, a 20 px). La letra es la de la interfaz (Ubuntu) en su peso normal: limn no trae una serif y no se agrega una fuente de un proveedor. Ni la chispa naranja ni "What's up next" son de quill. La frase cambia con la hora y el día locales (`ui.utc_offset`, sin dependencias nuevas): 5–11 mañana ("Good morning, {name}", "Morning", "Fresh start"), 12–16 tarde, 17–23 noche ("Good evening", "Evening", "Evening session?"), 0–4 madrugada ("Burning the midnight oil, {name}?", "Still up?", "Night owl?"). Lunes ("Fresh week"), viernes ("Happy Friday") y fin de semana ("Weekend project?") suman su frase al conjunto, salvo en la madrugada. La elección es determinista: se siembra con la fecha local y la parte del día, así que no parpadea entre cuadros ni entre sesiones del mismo tramo. Sin nombre, la frase pierde la coma y el nombre ("Good morning", "Weekend project?").

### 2.14 Cuenta e inicio de sesión (`quill_ui::account`)

La cuenta es del programa del agente: quill no guarda tokens ni usa el cliente OAuth de Desktop, maneja los `claude auth …` del CLI (ver `agents.md`). Al arrancar el motor pregunta quién está conectado (`Intent::CheckSignIn`) con el mismo entorno que las sesiones, y la respuesta (`Event::Auth`) no se persiste: el CLI es la fuente.

- **Botón de cuenta** (pie de la sidebar): es la identidad de GitHub, no la del agente (§2.14b). Con GitHub conectado muestra el avatar de la persona (o las iniciales de su nombre) y su nombre, `@login` si no tiene; sin conectar, el nombre del usuario de quill con sus iniciales. Ya no muestra el plan del agente: el plan vive en Settings › Accounts. En Desktop se ve el pie con la inicial y el nombre; usar GitHub como identidad es de quill (inferido).
- **Menú**, en este orden: cabecera (nombre y `@login` de GitHub, o "GitHub not connected" / "GitHub CLI not installed", apagado y sin acción), Settings (Ctrl+,), Take the tour, y al final "Disconnect GitHub" ("Connect GitHub" si nadie está conectado; sin `gh`, abre Settings › Accounts, que dice cómo instalarlo). El "Sign out" del agente salió de este menú: está en Settings › Accounts, en la sección de cada agente.
- **Sign out** (del agente, desde Settings › Accounts) pregunta antes (agregado de quill: Desktop no confirma, pero el logout del CLI también cierra la sesión de `claude` en la terminal y revoca los tokens): "Sign out of {agente}?" / "This also signs you out of {programa} in your terminal." / Cancel · Sign out.
- **Pantalla de inicio de sesión**, a ventana completa como la bienvenida (diseño propio, inferido: la de Desktop es una página web). Sale cuando el agente por defecto dice `SignedOut` (y tras el Get started de un primer arranque), o cuando el botón "Sign in" de una sesión la pide. Columna de 360: la pluma de quill, "Sign in to {agente}", una línea muted, y un botón de 320×44 por método que declara el adaptador (el primero relleno). Tras elegir: "Continue in your browser", "Copy link", el campo "Paste the code here" con Continue, y Cancel. Si el programa falla, el mensaje de su stderr en rojo y "Try again". Al llegar `Auth { SignedIn }` desaparece.
- **En una sesión**: un turno que termina porque el agente no está conectado (`AgentEvent::AuthRequired`) muestra "Not signed in" con el botón "Sign in", que abre la pantalla.
- El recorrido no arranca (ni avanza pasos) mientras la pantalla está arriba; sigue donde estaba al conectarse.
- Pruebas y capturas: el agente de prueba contesta `auth` según `QUILL_FAKE_AUTH` (`signed-in`/`signed-out`); nunca se corre el CLI real.

El aspecto es el propio de quill, con la paleta de la ventana y la burbuja invertida (tinta como fondo), que queda igual de legible en oscuro y claro. Es inferido: el onboarding de Desktop no se capturó, así que ni los pasos ni su forma salen de él. El título va en el cuerpo normal porque limn no ofrece peso para el título de `CoachMark`.

---

## 3. Tokens de quill


### 2.14b GitHub y Settings › Accounts (`quill_ui::github`, `quill_ui::accounts`)

GitHub es la identidad de la persona, no un agente. El motor lo maneja con `gh` (`quill-engine/src/github.rs`); `gh` guarda el token en el llavero del sistema y quill no tiene cliente HTTP ni credenciales. Todo corre fuera del hilo de la UI; ver `agents.md` § Cuenta de GitHub.

- **Página "Accounts"** (la cuarta de Preferences). Una sección **GitHub**: fila conectada "Signed in as @{login}" (el nombre debajo) con View profile (abre `https://github.com/{login}` en el navegador del sistema), Reconnect y Disconnect; sin conectar, "Connect GitHub"; sin `gh`, "GitHub CLI not found" con la instrucción de instalación por sistema (winget, brew, gestor de paquetes) y "Check again". Debajo, la tarjeta de contribuciones: "N contributions in the last year", Refresh, el mapa de calor (53 semanas, domingo a sábado, etiquetas de mes y Mon/Wed/Fri), la leyenda Less → More y tooltips "N contributions on Mon DD". Mientras carga, un esqueleto; si falla, una alerta con Retry y Reconnect GitHub. Se pide al abrir la página y con Refresh, no antes.
- **Origen del mapa de calor:** es el de openADE (`settings_ui/activity_charts.rs`: `contribution_scale`, `calendar_chart`, `github_card`; código propio del usuario, MIT OR Apache-2.0), con `HeatmapChart`, `ChartLegend`, `ChartAxisLabel`, `Skeleton` y `Alert` de limn, y los textos en inglés. La validación del calendario (semanas consecutivas y alineadas al domingo, fechas reales, no más de 54 semanas ni 372 días, cuenta igual a la de `gh`, GraphQL con `errors` descartado entero sin repetir sus palabras) también es de openADE.
- **Una sección por agente** (Claude Code, Codex, de `AgentInfo`): su estado ("Signed in" con email, organización y plan; "Not signed in"), Sign in / Sign out (los flujos de §2.14, ahora desde aquí) y su **uso**: "5-hour limit" y "Weekly" con el porcentaje, una barra de 4 px (azul; ámbar desde 75 %, rojo desde 90 %) y "Resets in …" (`view::resets_in`). Los límites son los que informó la última sesión de ese agente (`rate_limit_event` de Claude, `account/rateLimits/updated` de Codex), guardados por agente en `Store::limits_by_agent`; sin sesión que los haya informado, la fila dice que se verán al informarlos. Un `rateLimits` de Codex sin sesión (como `models_query`) no se pregunta todavía. En Desktop se ven las filas "5-hour limit" y "Weekly · all models", con barras de 4 px que avisan a 75 % y marcan peligro a 90 %; en Desktop GitHub aparece solo como un conector, "Signed in as @{login}", con Disconnect y Reconnect.
- **Conectar:** corre `gh auth login --web --hostname github.com --git-protocol https --scopes read:user`; `gh` imprime un código de un solo uso y quill lo muestra grande en un diálogo con Copy code y Cancel, y abre `https://github.com/login/device` con `quill_files::open_in_system`. Cancelar (o 600 s) mata el proceso por su PID. **Desconectar** pregunta antes ("This also signs you out of gh in your terminal.") y corre `gh auth logout --hostname github.com --user {login}`; luego se pregunta de nuevo quién queda (otra cuenta de `gh` puede tomar su lugar).
- **Avatar:** se baja `avatar_url` solo si es `https://avatars.githubusercontent.com/…`, con `?s=128`, con el cliente de `logos.rs`; se guarda como PNG en `quill/avatars/{login}.png` (login validado) y se renueva a las 24 h.
- Capturas: un `gh` falso primero en el PATH contesta `auth status`, `api user` y `api graphql` con JSON enlatado; nunca se corre el `gh` real con login ni logout.

### 3.1 Paleta

Fuente: muestreo de píxeles (ImageMagick) en y para el claro; propuesta propia para el oscuro. El acento de quill es **propio** (índigo "tinta"), no el `clay` de la marca.

| Rol (`quill_components::Look`) | Claro (medido → quill) | Oscuro (propuesto) | Uso |
|---|---|---|---|
| `canvas` | `#FFFFFF` → `#FFFFFF` | `#161616` | área principal, composer |
| `sidebar` | `#FAFAFA` → `#FAFAFA` | `#1B1B1B` | sidebar, barra |
| `raised` | `#F3F3F3` → `#F4F4F5` | `#232325` | burbuja del usuario, caja de comando |
| `code_fill` | `#F2F2F2` → `#F2F2F3` | `#262628` | código en línea, cabecera de archivo del diff |
| `menu` | `#FFFFFF` | `#1F1F21` | popovers |
| `hover` | ≈`#E6E6E6` → `rgba(0,0,0,0.06)` | `rgba(255,255,255,0.06)` | velos |
| `selected` | `#E0E0E0` → `rgba(0,0,0,0.10)` | `rgba(255,255,255,0.10)` | fila activa, chip abierto |
| `border` | `#E5E5E5`/`#E0E0E0` → `#E4E4E7` | `#2E2E31` | hairlines |
| `border_soft` | `#F1F1F1` → `#EFEFF1` | `#262629` | contorno de tarjetas |
| `text` | ≈`#1A1A1A` → `#18181B` | `#EDEDED` | texto |
| `muted` | ≈`#808080` → `#71717A` | `#A1A1AA` | secundario |
| `faint` | ≈`#969696` → `#A1A1AA` | `#71717A` | pistas, deshabilitado |
| `primary` / `on_primary` | `#191919` / blanco → `#18181B` / `#FFFFFF` | `#EDEDED` / `#161616` | botón primario (Always allow) |
| `accent` | `#257DE0` (punto, switch) → **`#3B5BDB`** | **`#7B93F5`** | acento quill: punto "ready", ✓, foco, switch |
| `warning` | `#F0AC55` → `#E8A33D` | `#F2B654` | punto awaiting, "Blocked" |
| `warning_fill` | `#EFE9E0` → `#FBF1E1` | `#3A2E1A` | chip "Needs your input" |
| `warning_ink` | ocre ("Manual") → `#9A5B00` | `#F2B654` | modo Manual |
| `danger` | — → `#D33A4A` | `#FF6B78` | errores |
| `git_added` | `#1E9E3C` → `#1F8A3B` | `#43C463` | `+N` |
| `git_removed` | ≈`#E07797`/`#CD2054` → `#C8344F` | `#FF6680` | `−N` |
| `diff_add_row` / `_word` | `#E7F4E7` / `#C8E7CD` → `#E8F5EA` / `#C5E6CC` | `#15291B` / `#1F4A2B` | filas del diff |
| `diff_del_row` / `_word` | `#FEEEEF` / `#F7CFD8` → `#FDEEF0` / `#F5CCD5` | `#2E171B` / `#5A2430` | filas del diff |
| `shadow` | — → `rgba(0,0,0,0.10)` | `rgba(0,0,0,0.45)` | popovers, modal |
| `scrim` | gris del modal → `rgba(0,0,0,0.32)` | `rgba(0,0,0,0.55)` | fondo del modal |

Paleta ANSI propia de la terminal (16 + cursor + selección; valores propios):

| Clave | Claro | Oscuro | Clave | Claro | Oscuro |
|---|---|---|---|---|---|
| black | `#1F1F23` | `#2A2A2E` | bright black | `#6B6B73` | `#5C5C64` |
| red | `#C8344F` | `#FF6680` | bright red | `#E0475F` | `#FF8095` |
| green | `#1F8A3B` | `#43C463` | bright green | `#26A047` | `#62D981` |
| yellow | `#9A6B00` | `#F2C14E` | bright yellow | `#B07D00` | `#FFD36B` |
| blue | `#3B5BDB` | `#7B93F5` | bright blue | `#4C6EF5` | `#9AAEFF` |
| magenta | `#A43EB0` | `#D88BE3` | bright magenta | `#BC52C8` | `#E7A6EE` |
| cyan | `#0F7C8C` | `#4FC6D6` | bright cyan | `#1193A6` | `#7DDCE8` |
| white | `#A1A1AA` | `#C8C8CC` | bright white | `#D4D4D8` | `#F2F2F2` |
| fg / bg | `#18181B` / `#FFFFFF` | `#EDEDED` / `#161616` | cursor / selección | `#3B5BDB` / `#3B5BDB40` | `#7B93F5` / `#7B93F54D` |

Contraste mínimo 3:1 forzado al pintar (como `minimumContrastRatio` del original).

### 3.2 Correspondencia con limn

| limn | quill |
|---|---|
| `Scheme` (const, `scheme.rs`) | `Look::LIGHT.scheme()` / `Look::DARK.scheme()`: `canvas`, `panel`=`sidebar`, `raised`, `hover`, `selected`, `border`, `shadow`, `text`, `muted`, `muted_on_raised`, `menu`, `menu_hover`, `menu_muted`, `control`. |
| `ComponentPalette` | `scheme.palette()` (canvas), `scheme.card()` (sobre tarjetas), `scheme.menu()` (popovers), `scheme.on(Surface::Solid(primary), on_primary)` (botón primario). |
| Colores fuera del `Scheme` | git, diff, warning, accent, ANSI: los guarda `Look` (como RUNE `rune-components/src/look.rs`) y se pasan con `.ink`, `DiffStat::inks`, `Chip::tone`. |
| `Tokens.radius` | `control: 6`, `surface: 10`, `card: 8`, `panel: 12` (`set_tokens` una vez al arrancar). |
| `Tokens.motion` | `Motion::DEFAULT` (hover 120, enter 140, exit 110, move 170); la entrada de la tarjeta de permiso 160 ms ≈ `enter_ms`. Movimiento reducido: `tree.set_reduced_motion(ajuste o limn_egui::prefers_reduced_motion())`. |
| `Tokens.tooltip` | `None` (cada palette deriva el suyo). |
| Selección de tema | `Ui.theme: System \| Light \| Dark`; `System` lee el tema del SO en el host. |

### 3.3 Tipografía

| Rol | Fuente propuesta (licencia) | Alternativa | Tamaño / interlínea | Medido |
|---|---|---|---|---|
| UI (Sans) | **Inter** 4.x (OFL-1.1): Regular, Medium, SemiBold→`SansMedium`, Bold | IBM Plex Sans (OFL) | 13 / 18 | body 13 px |
| Pie / metadatos | Inter | — | 12 / 16 | footnote 12 px |
| Caption / encabezados de grupo | Inter Medium | — | 11 / 14 | caption 11 px |
| Micro (chips "Preset", "Needs your input") | Inter Medium | — | 10 / 12 | captura |
| Título de sesión / modal | Inter Medium | — | 13–14 | captura |
| Respuesta del asistente | Inter (MVP); **Source Serif 4** (OFL) opcional en Settings | Newsreader (OFL) | 13–15 según "Transcript text size" | el original usa serif de marca para respuestas |
| Código, diff | **JetBrains Mono** (OFL-1.1) | IBM Plex Mono (OFL) | 12 / 17 (comfortable 13 / 19) | código 12 px, interlínea 17 px |
| Terminal | JetBrains Mono | — | 13 / 1.2 | xterm 13 |

Instalación: `limn::set_fonts(Fonts::new().face(FontFamily::Sans, inter_regular).face(FontFamily::SansMedium, inter_medium).face(FontFamily::SansBold, inter_bold).face(FontFamily::Monospace, jetbrains_mono)...)` y después `limn_egui::install_fonts(&ctx)` (que usa `limn::fonts()`). Los `.ttf` y sus `OFL.txt` en `apps/quill/assets/fonts/` con un `NOTICE.md` (como `crates/limn/assets/fonts/NOTICE.md`).

Escala de texto de "Transcript text size": S=13, M=14, L=15, como Desktop (interlineado 1.43, y 1.47 en L). Aplica a `Markdown::font_size`, a la burbuja y a las filas de herramientas.

### 3.4 Espaciado y radios

| Token quill | px | Uso medido |
|---|---|---|
| `space.2xs` | 2 | gap entre íconos de acción |
| `space.xs` | 4 | inset de filas de menú, margen del panel derecho |
| `space.sm` | 6 | inset de filas de sidebar, gap entre tarjetas |
| `space.md` | 8 | padding vertical de burbuja, padding de terminal |
| `space.lg` | 12 | padding de tarjetas, de burbuja (horizontal), del composer |
| `space.xl` | 20 | separación entre turnos |
| `space.2xl` | 32 | padding lateral de la columna con diff (31) |
| `radius.control` | 6 | filas, botones, chips |
| `radius.card` | 8 | tarjetas de herramienta, permiso, panel derecho |
| `radius.surface` | 10 | menús, burbuja del usuario |
| `radius.panel` | 12 | composer, modal |
| `size.row` | 24 | sidebar, nav de settings |
| `size.header` | 27–32 | barra superior 28, cabeceras de panel 30 |
| `size.sidebar` | 240 (200–420) | captura 212 |
| `size.column` | 540 (máx) | columna del transcript |
| `size.composer` | columna + 24 | 566 |

---

## 4. Lo que falta: PRs candidatos a limn vs. piezas de quill

Política: cada fila de 4.1 es un PR a `Niiihuel/limn` desde un worktree propio
(`git -C /home/user/projects/personal/lienzo worktree add ../limn-<tema> -b feat/<tema> main`),
con tests sin ventana, doc en `docs/components/`, entrada en el changelog y
página o caso en la galería. quill nunca lleva una copia ni un parche del
componente; mientras tanto apunta a la rama del PR (§5). Varios agregados
chicos que tocan los mismos archivos (`element.rs`, `tree.rs`, changelog,
galería) van en un solo PR, como recomienda RUNE.

### 4.1 PRs candidatos a limn (orden = impacto en el MVP)

| P | PR (título) | Worktree | Alcance | API sugerida | Por qué |
|---|---|---|---|---|---|
| **P0** | `feat(terminal): TerminalView, a grid of cells painted from a host snapshot` | `../limn-terminal` | Componente nuevo `terminal_view.rs`: grilla de celdas con atributos (16/256/truecolor, negrita, subrayado, inverso), cursor (bloque/barra, parpadeo con `caret_blink`, quieto con movimiento reducido), selección por arrastre que reporta texto (`CopyRequested`), scrollback virtualizado por filas, contraste mínimo configurable, métricas de celda medidas con `TextLayout`. Sin PTY ni parser: el host da el snapshot. Galería: página con una pantalla fija de ejemplo. | `TerminalView::new(id, screen: Arc<TerminalScreen>, palette).colors(AnsiColors).min_contrast(3.0).render()`; `TerminalScreen { cols, rows, cells, scrollback, cursor }`; `TerminalView::grid_size(size, font_size) -> (cols, rows)` para `TermResize`. Teclas: el host las toma con `tree.focused_id() == id` antes de `handle_event`. | No hay emulador ni grilla en limn (ficha terminal). Bloquea la terminal. |
| **P1** | `feat(code-diff): word highlights, expandable context, margin bar and file header` | `../limn-code-diff` | Extiende `code_diff.rs`: rangos intra-línea, fila de contexto plegado ("Show more lines") que reporta acción, barra de margen add/del, colores del diff configurables, `DiffFileHeader` (ruta, `DiffStat`, plegado, visto, acciones). Side-by-side queda para otro PR. | `CodeDiffLine.word_ranges: Vec<Range<usize>>`; `CodeDiffLineKind::Folded { hidden }` → `Clicked {id}:expand:{row}`; `.margin_bar(true)`; `.colors(DiffColors { add_row, add_word, del_row, del_word, add_bar, del_bar })`; `DiffFileHeader::new(id, path, palette).stat(a, d).collapsed(b).viewed(b).actions(vec)`. |: resalte por palabra, expansión, barra; hoy sólo tinte de fila. |
| **P1** | `feat(button): SplitButton, a primary action joined to its menu` | `../limn-split-button` | Botón con acción principal + chevron que abre `DropdownMenu`, keycaps opcionales dentro del botón, variantes del `Button`, foco y Tab independientes por mitad. | `SplitButton::new(id, label, menu_id, palette).variant(ButtonVariant::Primary).keys(&["Ctrl","Shift","⏎"]).menu(entries, open, dismiss_id).render()` → `Clicked {id}` / `Clicked {menu_id}`. | "Always allow ▾"; "Send ▾" (More send options). |
| **P1** | `feat(layout): SplitView for panes divided by resize handles` | `../limn-split-view` | Fila o columna de paneles con `ResizeHandle` entre ellos, mínimos por panel, fracciones controladas por el host, "ceder antes que el contenido" (regla de surco) y `clip_children` por panel; opcionalmente el deslizamiento de apertura. | `SplitView::row(id, vec![Pane::new(el).min(420.0).flex(), Pane::new(el).fraction(0.6).min(320.0)]).handle_thickness(6.0).render()`; reporta `Drag*` en `{id}:handle:{i}`; `SplitView::resized(fractions, i, delta, total)` como helper puro. | Sidebar \| chat \| diff y diff / terminal; hoy surco lo resuelve a mano en `window_view` y quill lo repetiría 3 veces. |
| **P1** | `feat(status): StatusDot with an optional pulse, and Progress::ring` | `../limn-status-marks` | `StatusDot` (punto suelto con color, tamaño, latido 0.3↔1 en 1.2 s, quieto con movimiento reducido, descripción accesible) o `Badge::dot().pulse(bool)` usable sin hijo; `Progress::ring(diameter)` (anillo determinado). Ambos chicos y en los mismos archivos → un PR. | `StatusDot::new(color).pulse(true).size(6.0).description("Working")`; `Progress::new(fraction, palette).ring(14.0)`. | Estados de sesión de la sidebar, "Needs your input", contexto del composer y del panel derecho. |
| P2 | `feat(window-controls): round Linux style` | `../limn-window-controls` | `WindowControlsStyle::{Windows, Round}`: círculos de 16, glifos finos, mismo orden a la derecha. | `WindowControls::new(id, palette).style(WindowControlsStyle::Round)` | Barra propia en Linux. |
| P2 | `feat(button-group): stack below a width` | `../limn-button-group-stack` | Apilar y estirar los botones cuando el contenedor es más angosto que un umbral. | `ButtonGroup::new(b).align_end().stack_below(420.0)` | Tarjeta de permiso ≤ 420 px (permisos). |
| P3 | `feat(command-palette): a searchable command list over the window` | `../limn-command-palette` | `Modal` + `SearchField` + lista agrupada con atajos y placeholder por modo. | `CommandPalette::new(id, query, groups, selected, open, palette).placeholder(..)` | Ctrl+K (fase 2). |
| P3 | `feat(fonts): a serif family` | `../limn-serif` | `FontFamily::Serif` (+ `SerifItalic`) en `Fonts::face`, `TextSpan`, `Markdown::font_family`. | `Fonts::new().face(FontFamily::Serif, font)` | "Response font" serif (Settings). |
| P3 | `feat(theme): per-role font sizes` | `../limn-type-scale` | Que la escala de `Text` roles (Body 14, Label 13…) sea un token, para hosts con body 13. | `Tokens { type_scale: TypeScale { body: 13.0, .. }, .. }` | Body de Code es 13 px; varios componentes fijan 14. |
| P3 | `feat(code-diff): side-by-side layout` | `../limn-code-diff-split` | Dos columnas con scroll sincronizado. | `CodeDiff::layout(DiffLayout::Split)` | Panel Changes del original (por defecto split). |
| P3 | `feat(settings-layout): navigation column beside content` | `../limn-settings-layout` | Columna de secciones agrupadas con buscador + contenido. | `SettingsLayout::new(nav_groups, active, content, palette).search(query)` | Modal de Settings. |

### 4.2 Piezas específicas de quill (`quill-components` / `quill-ui`, sólo composición)

| Pieza | Compuesta con (limn existente) | Zona |
|---|---|---|
| `Look` (tokens §3) + `Scheme` | `Scheme`, `ComponentPalette`, `set_tokens` | todo |
| `UserBubble` | `Element::panel` + `Text` | transcript |
| `TurnSummary` (resumen colapsable con `+N −M`) | `Collapsible`, `DiffStat` | transcript |
| `ToolRow` + catálogo herramienta → (verbo, objetivo, cuerpo) | `Collapsible`, `FramedCard::rows`, `OutputBlock`, `CodeBlock`, `CodeDiff` | transcript |
| `TodoCard` | `FramedCard`, `ListRow` | transcript |
| `TurnStatusLine` ("Thinking…" escalonado, "Stopping…") | `TextShimmer`, `Ticker` | transcript |
| `PermissionCard` (genérica; plan y pregunta en fase 2) | `FramedCard`, `ButtonGroup`, `SplitButton`*, `Kbd`, `Collapse` | transcript |
| `ComposerChips` (entorno, carpeta, worktree, modo, modelo/esfuerzo) | `Composer::footer`, `Button` ghost `Mini`, `DropdownMenu`, `MenuHeader`, `Switch` | composer |
| `SessionRow` + derivación de estado de fila | `ListRow`, `StatusDot`* | sidebar |
| `EnvironmentPanel` (Environment / Context / Changes) | `Card`, `ListRow`, `Switch`, `Progress::ring`*, `ContextWindow`, `DiffStat` | panel derecho |
| `DiffPane` (cabecera rama → working tree, archivos) | `PanelHeader`, `CodeDiff`*, `DiffFileHeader`* | diff |
| `TerminalPane` (pestañas, cabecera, cierre con confirmación) | `PanelHeader`, `TabStrip`, `AlertDialog`, `TerminalView`* | terminal |
| Shell (`window_view`) | `SplitView`* (o `ResizeHandle` mientras), `PeekPanel`, `WindowControls` | marco |
| Región de arrastre/resize de ventana | host: `ViewportCommand::StartDrag`/`BeginResize` (patrón RUNE `apps/desktop/src/frame.rs`) | `apps/quill` |
| Parser unified diff → `CodeDiffLine`, diff por palabras | crate `similar` | `quill-git` |
| PTY + parser VT → `TerminalScreen` | `portable-pty`, `alacritty_terminal` o `vt100` | `quill-pty` |

\* = depende de un PR de 4.1.

## 5. Workspace de quill (modelo: RUNE; host y vistas como surco)

```text
quill/
  Cargo.toml            # [workspace] resolver 2; deps compartidas; limn por git (rama quill/integration)
  AGENTS.md             # reglas (UI sólo limn, lo que falte en limn = PR a limn, nada bloqueante en UI, sin activos de Anthropic)
  shell.nix / flake.nix # como limn y RUNE
  docs/
    ARCHITECTURE.md     # capas, ciclo de cuadro, contratos (este doc §1)
    LIMN_INTEGRATION.md # rama de integración y lista de PRs a limn
    UI.md               # esta sección, mantenida
  apps/
    quill/              # ventana eframe+wgpu, bucle de cuadros, marco sin borde (drag/resize),
      src/main.rs       #   wake, atajos de host, --screenshot/--fixture, assets/fonts
      fixtures/         # estados de captura (JSON de eventos) para screenshots y tests
  crates/
    quill-model/        # tipos y contratos: SessionId, Session, Turn, Item, ToolCall, PermissionRequest, FileChange, TermId; Event, Intent/Command, EventSink, Envelope
    quill-engine/       # runtime Tokio, procesos del CLI, sesiones, permisos, git, PTYs; recibe Intent, emite Event. Incluye MockEngine que reproduce fixtures
    quill-protocol/     # tipos serde de stream-json y del protocolo de control, codec por línea, LaunchSpec (puro, sin IO)
    quill-store/        # SQLite: sesiones, turnos, eventos, efectos por petición, blobs
    quill-git/          # status, numstat, patch por archivo, parser unified → CodeDiffLine
    quill-pty/     # PTY + parser VT → TerminalScreen (snapshot)
    quill-state/        # Store: aplica Event → Applied
    quill-components/   # Look (tokens §3), piezas puras props → Element (UserBubble, ToolRow, PermissionCard…)
    quill-ui/           # Ui, view, update, intercept, on_applied, after_layout, keybinds; tests como host
  tools/                # (opcional) capturas comparadas, generación de fixtures
```

| Convención (de RUNE) | En quill |
|---|---|
| Capas sólo hacia abajo | `quill-ui` no depende de `quill-engine`. |
| UI controlada, estado en `Store` + `Ui` | ídem. |
| IDs derivados del dominio | §1.6. |
| Dependencia nueva = comentario en `Cargo.toml` con el porqué | ídem. |
| `[profile.dev.package."*"] opt-level = 2` | el renderer sin optimizar pierde cuadros. |
| `cargo fmt --all` seguro (limn por git) | ídem. |
| Dependencia de limn | `limn`/`limn-egui` por git a la rama `quill/integration` de `Niiihuel/limn`, que une los PRs de §4.1 aún no mezclados (RUNE hace lo mismo con `rune/integration-*`). Para desarrollar un PR: `[patch."https://github.com/Niiihuel/limn"] limn = { path = "../limn-<tema>/crates/limn" }` local, nunca commiteado; al mezclar, `cargo update -p limn -p limn-egui`. surco usa ruta `../lienzo`, más simple pero `cargo fmt --all` reformatearía limn. |
| Lo que falta en limn | lista viva en `docs/LIMN_INTEGRATION.md` con PR, rama y estado; ningún componente genérico vive en quill. |
| Simulado se etiqueta como simulado | el backend mock muestra "Simulated" en el título. |
| i18n | MVP en inglés con textos centralizados en `quill-ui/src/text.rs`; decidir si se agrega `quill-i18n` (es/en) como `rune-i18n`. |

Comandos: `cargo run -p quill`, `cargo run -p quill -- --fixture permission --screenshot out.png`, `cargo test --workspace`.

---

## 6. Estrategia de pruebas de UI

### 6.1 Niveles

| Nivel | Dónde | Qué prueba | Cómo (limn `docs/testing.md`) |
|---|---|---|---|
| Componente | `quill-components` | Cada pieza: regiones, textos, colores, que lo pasivo no tenga región. | `UiTree::new(row![comp, spacer.flex(1)])`, `tree.layout(size)`, `frame.regions`, `frame.commands` (`DrawCommand::Text`/`FillRect`), `frame.hit_test`. `tree.set_text_measurer(|t,_,_| t.chars().count() as f32 * 7.0)` para anchos deterministas. |
| Pantalla | `quill-ui/src/tests/` | Flujos: evento del motor → `Store` → vista → clic → `Intent`. El test hace de host (como `rune-ui/src/tests`). | Secuencia de `Event` de un fixture → `apply` → `view` → `layout` → `handle_event(PointerDown/Up)` → `update` → assert de `Vec<Intent>`. |
| Teclado | `quill-ui` | Esc/Ctrl+Enter/Ctrl+Shift+Enter en permiso; Enter/Shift+Enter; `b`; teclas a la terminal enfocada. | `UiEvent::Key { .. }` por `intercept` + `handle_event`. |
| Animación | `quill-components` | Latido de `StatusDot`, entrada de la tarjeta, shimmer quieto con movimiento reducido. | `tree.layout_at(size, t0)`, `layout_at(size, t0+200ms)`; en el resto `set_reduced_motion(true)`. |
| Tokens | `quill-components/tests/tokens.rs` | `set_tokens` con radios de quill. | Un único test de integración (los tokens son de proceso). |
| Anchos extremos | todos | Nada se sale de la ventana a 600, 1024, 1408, 1920; panel derecho colapsa. | Bucle de anchos; assert `rect.right() <= width`. |
| IDs únicos | `quill-ui` | Ningún par de controles comparte ID en ningún fixture, ambos temas, 3 anchos. | Recorrer `frame.regions`, igual que los tests del gallery. |
| Motor | `quill-state` | Orden, generación descartada, `send_latest` coalescido, emparejamiento tool_use/tool_result. | Tests puros sin UI. |

### 6.2 Contra las capturas

1. **Fixtures por captura.** Cada captura relevante se transcribe a un fixture de eventos (`apps/quill/fixtures/*.json`) con datos neutros (sin datos personales):

| Fixture | Captura | Estado |
|---|---|---|
| `new-session` | 040802 | página vacía, composer, chips |
| `model-menu` / `effort-menu` / `mode-menu` | 040802 / 040903 / 041032 | menús abiertos (`--set` como el gallery) |
| `permission` | 041143 | tarjeta pendiente, tareas, panel derecho |
| `permission-menu` | 041155 | menú del split button |
| `turn-done` / `turn-expanded` | 041215 / 041226 | resumen colapsado / expandido |
| `diff` / `diff-terminal` | 041229 / 041232 | paneles |
| `settings` | 035021 | modal Appearance |

2. **Métricas de oro (test sin ventana).** Para cada fixture, a 1408×844, el test busca regiones/IDs o anclas (`element.anchor(id)`, `tree.anchor_offset`) y compara con la tabla de §2 con tolerancia ±4 px (o ±6 % en anchos de texto, porque la fuente difiere): ancho de sidebar, alto de barra, rect de la columna, rect del composer, rect de la tarjeta de permiso, filas de 24/27, divisor chat/diff 2:3, divisor diff/terminal 50 %. Estas son las aserciones que fallan si el layout se desvía.
3. **Captura visual (manual/CI opcional).** `cargo run -p quill -- --fixture permission --screenshot out.png --size 1408x844` con movimiento reducido (como `--screenshot` de RUNE y del gallery). Comparación **estructural**, no de píxeles (otra fuente, otros íconos, otro acento):
   - superposición al 50 %: `magick out.png ref.png -compose blend -define compose:args=50 -composite overlay.png`;
   - bordes: `magick X.png -edge 1 -negate edges.png` sobre ambas y `compare -metric AE -fuzz 15%` restringido a cajas de zona (`-crop`) para ver desalineaciones de bloques;
   - revisión humana con una hoja de zonas (rects de §2 dibujados sobre ambas).
4. **Regresión propia.** Las capturas de quill aprobadas se guardan como golden propias (`apps/quill/fixtures/golden/*.png`) y se comparan pixel a pixel con umbral (`compare -metric AE -fuzz 2%`); las de Desktop nunca se versionan dentro de quill.
5. **Lo que falta capturar** (para cerrar medidas): tema oscuro, sidebar con varios estados, error de turno, streaming de texto, tarjeta de cambios de varios archivos, menú del título, terminal con varias pestañas. Mientras tanto, las medidas de esas zonas son propuestas y se marcan así en los tests.

---

## 7. Limitaciones de limn que afectan el MVP

| Limitación | Impacto | Salida |
|---|---|---|
| **No hay emulador de terminal** (ni grilla de celdas) | Bloquea la terminal del MVP. | P0: `TerminalView` en limn + `quill-pty` (PTY/VT). Mientras el PR no se mezcla: la terminal se pospone o se muestra sólo la salida con `OutputBlock` (componente existente), sin entrada. |
| IME y RTL **sin probar** (ROADMAP) | Riesgo en el composer con teclas muertas/compose en Linux (acentos en español). | Probar temprano en Wayland y X11; reportar a limn. |
| Accesibilidad incompleta (ROADMAP: árbol, caret, preferencias del SO) | Lectores de pantalla limitados; no bloquea el MVP visual. | Seguir el roadmap; etiquetar con `description`/tooltips. |
| Tamaños de fuente no se tematizan por componente; escala fija (Body 14, Label 13…) | Algunos componentes quedan a 14 donde la captura mide 13. | Usar `Text` roles/`font_size` donde se puede; anotar diferencias; PR para tamaños por rol si molesta. |
| Sin `FontFamily::Serif` | "Response font" serif no disponible. | MVP sans; P3. |
| Sin región de arrastre de ventana en limn | Marco sin borde. | Host, como RUNE `frame.rs` (`StartDrag`/`BeginResize`). |
| `WindowControls` sólo estilo Windows | Diferencia visual en Linux. | P2 o aceptar el estilo. |
| `CodeDiff` sin palabra/expansión/split | Diff menos rico que la captura. | P1; MVP acepta filas tintadas. |
| Tokens globales de proceso | Dos ventanas no pueden tener radios distintos. | Irrelevante para el MVP. |
| Plataformas: sólo Linux en CI (ROADMAP) | macOS/Windows sin garantía. | MVP Linux. |
| `font_definitions_with` (fuente elegida por la persona) sólo en ramas de RUNE | Cambiar la fuente de interfaz en caliente. | En `main` alcanza `limn::set_fonts` + `install_fonts` al arrancar; cambio en caliente requiere ese PR. |

## 8. Preguntas abiertas

- Ancho por defecto de la sidebar: 212 (captura) vs 288 (fuente estática). Propuesta 240.
- ¿Panel derecho flotante (como el original) o en flujo? Flotante tapa la columna en ventanas < ~1200 px.
- ¿i18n es/en desde el inicio (como RUNE) o inglés en el MVP?
- ¿Inline approvals (tarjeta dentro del transcript, como se ve) o tarjeta fija sobre el composer? Las capturas la muestran inline.
- Acento final de quill (propuesto índigo `#3B5BDB`).
