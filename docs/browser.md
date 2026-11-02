# Navegador integrado

Un Chromium que el agente maneja y que la persona ve y usa en un pane, como el "Browser" de Claude Desktop. Este documento fija cómo se integra en Newen y en qué orden se construye.

Fuentes:
- Mediciones propias del 2026-10-26 con Google Chrome 150.0.7871.46 y el CLI 2.1.310 en esta máquina (scripts en el scratchpad de la sesión; se resumen abajo).
- El comportamiento visible del "Browser" de Claude Desktop, descrito por lo que la persona ve; sus detalles internos no se documentan acá.

Lo que se marca *(inferido)* no está confirmado.

## 1. Qué hace el navegador

- **Motor:** cada pestaña es un Chromium y el agente lo maneja por **CDP**. Sin Playwright.
- **Herramientas** para el agente: `navigate`, `read_page`, `find`, `get_page_text`, `form_input`, `computer`, `javascript_tool`, `read_console_messages`, `read_network_requests`, `resize_window`, `tabs_context`, `tabs_create`, `tabs_select`, `tabs_close`, `browser_batch`, más `preview_start/stop/list/logs` para servidores de desarrollo. Los esquemas y las descripciones son de Newen (§4).
- **`read_page`** inyecta un script que escribe una línea por elemento, `role "name" [ref_N] href=… type=… placeholder=…`, con refs estables por nodo, profundidad 15, tope de 50 000 caracteres y valores sensibles redactados. Así un sitio grande no devuelve el árbol completo.
- **`computer`** hace capturas JPEG (calidad 75, ≤800 px de ancho); los clicks van en coordenadas de esa imagen y se escalan a la página.
- **Pie de cada resultado:** "Tab Context:" con la pestaña y la lista de pestañas.
- **Tiempos:** límite de 30 s por herramienta (45 s para JS). Un lote corre hasta 25 acciones, corta en el primer error y tiene 180 s en total.
- **UI:**
  - Filas del transcript con verbo por herramienta ("Opening {host}" → "Opened {host}", "Reading page", "Filling in a field", "Clicking", "Capturing page", "Batch — N actions").
  - Una tarjeta "{host} · Opened in Browser" con miniatura y "Open".
  - El pane: pestañas, atrás/adelante, recargar, URL, "Select element", viewport (Responsive, Mobile, Tablet, Custom) y un popover de ajustes.
- **Seguridad:**
  - localhost siempre; otros sitios con aprobación por sitio ("Allow once", "Always allow for this website", "Don't allow");
  - cámara y micrófono negados, diálogos JS suprimidos para el agente;
  - popups que el agente no maneja, credenciales en la URL con confirmación, tope de 8 pestañas.
- **Ciclo de vida:**
  - Un pane por sesión, creado al primer uso.
  - Cookies en memoria por defecto ("Until quit"), con "Shared" y "Per session" como alternativas.
  - Un pane oculto se destruye a los 5 min.
- **Shim `$BROWSER`** para que `gh`, vite y otros abran sus URLs locales en el pane.

## 2. Decisión: Chromium del sistema, CDP propio, por pipe

| Opción | Por qué no |
|---|---|
| Servo | Sin CDP; compatibilidad web incompleta, y el agente tiene que verificar la app en el motor de los usuarios; compila SpiderMonkey (C++) |
| Zephium | Es una aplicación, no un motor, y no corre en Linux |
| Playwright | Es un cliente de CDP que agrega Node y sus propios navegadores |
| WebKitGTK (wry) | Sin CDP; WebDriver limitado |
| Crates de CDP (chromiumoxide, headless_chrome, chromey) | Ninguno soporta el pipe. chromiumoxide genera tipos de una versión vieja del protocolo que fallan cuando Chrome cambia (#266, #335) y trae reqwest y 4,9 MB de código. headless_chrome bloquea hilos dentro de un engine tokio |

**Lo elegido:**
- **Motor:** el Chromium del sistema. Primero `NEWEN_CHROME` o el ajuste de Settings; si no, `chromium`, `google-chrome-stable` o `google-chrome` del PATH. En nixpkgs hay 150.0.7871.46. Descargar Chrome for Testing queda para más adelante (en NixOS necesita nix-ld).
- **Transporte:** `--remote-debugging-pipe`: Chrome lee del fd 3 y escribe en el fd 4, JSON terminado en NUL. Un puerto TCP lo puede manejar cualquier proceso local, cookies incluidas; el pipe solo Newen.
- **CDP:** una capa propia y chica. `serde_json::Value` de ida y vuelta, respuestas por `id`, eventos por `sessionId` y structs solo para los ~30 métodos que se usan. No agrega dependencias: `std::io::pipe` y un `dup2` antes del `exec`.
- **Frames:** screencast JPEG decodificado con `zune-jpeg`, que ya está en `Cargo.lock` vía `image`. Tarda 2,9 ms para 1600×1000 fuera del hilo de la UI. Se muestran en una superficie de limn.

### Mediciones (Chrome 150, esta máquina)

| Qué | Resultado |
|---|---|
| Arranque por pipe | 0,34 s hasta `Browser.getVersion` |
| Memoria (PSS, todo el árbol) | 259 MiB en reposo; +20 a 40 MiB por sesión con una página chica |
| Contexto por sesión (`Target.createBrowserContext`) | Cookies aisladas: la sesión 2 no ve la cookie de la 1 |
| Screencast | 60 fps animando, ~33 fps al scrollear; un click se ve en el frame siguiente; sin cambios en la página no hay frames |
| HiDPI | `deviceScaleFactor` agranda las capturas pero **no** el screencast; hace falta `--force-device-scale-factor` al lanzar (vale para todo el proceso) |
| Texto | `Input.insertText` e `imeSetComposition` (acentos, CJK, emoji) funcionan |
| Selector de archivos, diálogos, descargas, popups | Interceptables: `Page.setInterceptFileChooserDialog`, `javascriptDialogOpening`, ruta de descarga por contexto, `targetCreated` con `openerId` |
| Cursor | CDP no lo informa; aproximación con `getComputedStyle(elementFromPoint(x, y)).cursor` |
| Imagen en el resultado de una herramienta MCP propia | El CLI 2.1.310 se la pasa al modelo: leyó "ZQ 742" y "Blue" de la captura |

## 3. Arquitectura

```
newen-ui ──Intent::Browser*──▶ newen-engine ──▶ newen-browser ──pipe/CDP──▶ Chromium
   ▲  Event::Browser* (no se guardan)  │               │
   │                                   │      frames RGB decodificados
apps/newen ◀───────── Frames (canal aparte) ──────────┘
   └─ limn_egui::present_surface("browser:<sesión>:<pestaña>")
```

Las capas se respetan:
- `newen-ui` no conoce al engine ni a Chromium; pinta una superficie y emite intenciones.
- `newen-browser` no sabe nada del agente ni de la UI.
- `apps/newen` es la raíz que une los frames con limn.

### 3.1 `newen-browser` (crate nuevo, lado engine)
- **`Chromium`:** encuentra el binario y lanza `--headless --remote-debugging-pipe --user-data-dir=<datos de Newen>/browser --force-device-scale-factor=<escala de la ventana>` con los flags de Playwright: `--no-first-run --no-default-browser-check --disable-background-networking --disable-component-update --disable-default-apps --disable-breakpad --disable-renderer-backgrounding --disable-background-timer-throttling --disable-backgrounding-occluded-windows`, y `about:blank` como primera página. El sandbox queda prendido.
  - Un solo proceso por Newen, lanzado al primer uso.
  - Se cierra 5 min después de que ninguna sesión tenga pestañas, como Desktop.
  - Si se cae, las sesiones se enteran ("The browser stopped.") y se relanza al siguiente uso; las pestañas no se reabren solas.
- **`Cdp`:** la capa de transporte. Pedidos por `id` con timeout, eventos por `sessionId` y `Target.attachToTarget{flatten:true}`. Las claves opcionales se omiten: un `null` en `startScreencast` deja a Chrome sin frames.
- **`Context` por sesión de Newen:** `Target.createBrowserContext` con cookies "Until quit" por defecto, como Desktop. Hasta 8 pestañas por contexto.
  - Cada pestaña lleva su URL, título, `canGoBack/Forward`, carga y viewport.
  - Popups con `openerId` se abren como pestaña nueva.
  - Diálogos JS: se le muestran a la persona y se suprimen para el agente.
  - El selector de archivos de una página abre el diálogo del escritorio (`Page.setInterceptFileChooserDialog`, `DOM.setFileInputFiles`); las descargas van a `~/Downloads` con aviso.
- **Screencast:** solo la pestaña visible de un pane visible. JPEG a la medida del área, con el ack después de decodificar, así que Chromium no manda más rápido de lo que se dibuja. El frame RGB se publica en `Frames`, donde el último pisa al anterior. Un pane oculto detiene el screencast.
- **Entrada:** `Input.dispatchMouseEvent` (move, press, release, wheel con `clickCount` y modificadores), `dispatchKeyEvent` (down/up con `code`, `key`, `text`) e `insertText`/`imeSetComposition`. El cursor se pide con la aproximación de arriba, a lo sumo cada 120 ms mientras el mouse se mueve *(inferido)*.
- **Herramientas del agente** (§4) sobre la misma pestaña; scripts inyectados para `read_page`, `find` y `get_page_text`, con la misma forma de salida que Desktop.
- **Política** (§5) antes de cada navegación y cada acción.

### 3.2 `newen-model`
- **Intención:** `Intent::Browser{session, op}`, con `BrowserOp` = `Open{url?}`, `Navigate{tab, url}`, `NewTab{url?}`, `CloseTab`, `SelectTab`, `Back`, `Forward`, `Reload`, `Stop`, `Show{view: Option<BrowserView{tab, width, height}>}`, `Input{tab, input: BrowserInput}`, `Copy{tab}` y `Select{tab, on}`.
- **Evento:** `Event::Browser{session, update}`, con `BrowserUpdate` = `Tabs{tabs, active}`, `Cursor`, `Copied`, `Notice`, `Failed`, `Selecting` y `Selected`. **No se guarda** (`Store::keeps`).
- `SITE_PERMISSION`: el nombre de la herramienta en la tarjeta de permiso de un sitio (§5).

### 3.3 `newen-engine`
- Es dueño del `Browser`, que lanza Chromium al primer `Open` o a la primera herramienta.
- **La entrada se aplica en el acto, en orden.** El resto de las operaciones de la persona va a una cola; las que cargan páginas corren aparte para no frenarla.
- **Las herramientas del navegador corren en su propia tarea** y responden por `Internal::HostAnswered`. El router no espera páginas.
- Cierra el contexto cuando la sesión se archiva o se borra.
- **Cierre por inactividad:** Chromium se cierra 5 min después de que ninguna sesión tenga pestañas.
- **El puente de `$BROWSER`** (`bridge.rs`, §5).

### 3.4 `apps/newen`
- **Frames:** cada frame toma de `Frames` lo nuevo y llama a `present_surface` (`SurfaceFrame::from_rgb`). Un frame nuevo despierta la ventana.
- **Vista:** después de pintar le dice al engine qué pestaña está a la vista y su tamaño (`Show`), o que ninguna.
- **Entrada:** `page.rs` lee la entrada cruda de egui antes que limn.
  - El mouse, la rueda y la cuenta de clicks, donde la página está arriba de todo o mientras dure un arrastre.
  - Las teclas, el texto, el IME y el portapapeles, mientras la página tiene el foco.
  - Los atajos del navegador (Ctrl+L, Ctrl+T, Ctrl+W, Ctrl+R, Ctrl+Tab, F6, Ctrl+Shift+B).
- **Cursor:** el de la página, sobre ella.
- **Copiar y elegir:** lo copiado va al portapapeles; un elemento elegido se suma al mensaje.
- **Barra de URL:** muestra la dirección de la pestaña, salvo mientras la persona escribe.
- `newen --open-url URL` es el lado del script de `$BROWSER`.

### 3.5 limn: `Element::focus_area` público
Alcanzó con hacer público `Element::focus_area`, que ya existía para `TerminalView` (rama `feat/focus-area`, integrada en `quill/integration`):
- toma el foco con un click o con Tab y no informa nada, así que el host lee la entrada;
- alrededor de un `Element::surface` forma una superficie que recibe entrada.

La entrada no se tradujo a acciones de limn porque la página quiere más de lo que limn traduce: Alt, los otros botones, soltar teclas. El cursor lo pone el host con egui.

### 3.6 `newen-ui`: `Pane::Browser`
- **Encabezado:** pestañas con "New tab" y "Close tab"; atrás/adelante, recargar/detener, la URL ("Enter a URL") y "Select element" / "Exit selection mode". Los textos son los de Desktop.
- **Botón:** el globo de la barra superior, "Browser", Ctrl+Shift+B.
- **Cuerpo:** la superficie de la pestaña, en un `focus_area`.
- **Estados:**
  - una pestaña en blanco (`about:blank`, sin cargar) o ninguna pestaña muestran el estado vacío de Desktop: el globo, "Browse the web" y "Type a URL or ask the agent to open a site. It can read, click, and type.". El original nombra a Claude y ofrece "Detect dev server", que Newen todavía no tiene. Mientras se ve el estado vacío, la superficie no se dibuja y el motor deja de mostrar la página;
  - el error de arranque ("No Chromium found…", "The browser stopped.");
  - debajo de la página, el último aviso, hasta que se cierra (ver 3.7).
- **Atajos:** los del navegador de Desktop, con Ctrl en Linux.
- **El pane se abre solo** cuando el agente abre una página, como Desktop.

### 3.7 El tamaño de la página y los avisos
**El tamaño** lo dice el botón de viewport de la barra de herramientas, no una barra aparte:
- **Encendido** (con el relleno de los toggles activos de Newen) mientras la pestaña tiene un tamaño fijado, lo haya elegido la persona o el agente con `resize_window`.
- **Ícono:** un teléfono si el ancho es de 480 px o menos; si no, un monitor.
- **Tooltip:** "Viewport" sin tamaño fijado; "Viewport: 1440 × 900" si lo eligió la persona; "Viewport: 1440 × 900 · set by the agent" si lo eligió el agente.
- **Menú:** "Responsive", "Mobile" y "Tablet", más el tamaño propio del agente si no es ninguno de esos. La opción marcada dice "Set by the agent" cuando la eligió el agente. "Responsive" es la forma de volver al tamaño del pane (`BrowserOp::Viewport { size: None }`).

Quién fijó el tamaño viaja en `BrowserTab::emulated_by_agent`, que pone `newen-browser`: el agente al redimensionar lo activa y el menú de la persona lo borra. `resize_window` ya no deja un aviso: el botón lo muestra.

**Los avisos** (lo que dijo un diálogo de la página, una descarga guardada, un fallo del navegador) tienen una franja de 32 px bajo la página, con un ícono (el globo, o la advertencia en rojo si es un fallo), la frase y la X de "Dismiss". Duran hasta que la persona los cierra o llega otro. Van ahí porque son texto de largo variable que en la barra de herramientas no entra. En el ancho mínimo de la columna, la frase se desvanece antes de la X, y los controles de la barra de herramientas no se pisan ni se salen del pane.

Por qué así: el navegador de Claude Desktop, por lo que se ve, no avisa del tamaño con una barra. Lo muestra en su botón de viewport, resaltado y con el tamaño en el tooltip, y en su menú marca la opción que eligió el agente, con "Responsive" para volver al tamaño del panel. Esto se infiere de lo que la app muestra, no de una especificación.

## 4. El agente

### 4.1 Servidor y herramientas
- **Servidor:** un segundo servidor MCP en proceso, `browser` (`mcp__browser__*`, el nombre del video). Va junto a `sessions`, en `initialize.sdkMcpServers`.
- **Definición:** las herramientas se definen en `newen-agent` como `BROWSER_TOOLS`, con el mismo formato que `HOST_TOOLS` y sin nombres de proveedor. Cualquier adaptador que sepa ofrecer herramientas del host las expone.
- **Set y semántica:** los del navegador de Desktop, con esquemas propios y descripciones redactadas para Newen:
  - `navigate{url | "back" | "forward", tabId?}`;
  - `read_page{filter?: all|interactive, ref_id?, depth?, max_chars?}`;
  - `find{query}`;
  - `get_page_text`;
  - `form_input{ref, value}`;
  - `computer{action: screenshot|left_click|right_click|double_click|triple_click|type|key|scroll|scroll_to|hover|left_click_drag|wait, coordinate?, ref?, text?}` (sin el `zoom` de Desktop);
  - `javascript_tool{text}`;
  - `read_console_messages{pattern?, clear?}`;
  - `read_network_requests{url_pattern?}`;
  - `resize_window{width, height}` (0 devuelve el tamaño del pane);
  - `tabs_context`, `tabs_create{url?}`, `tabs_select{tabId}`, `tabs_close{tabId}`;
  - `browser_batch{actions[≤25]}`.
- **Resultados:** texto más el pie "Tab Context:". `computer screenshot` devuelve una **imagen** (JPEG, ≤800 px), y por eso `AgentCommand::HostToolResult` pasa de `text` a bloques `Text | Image{mime, data}`.

### 4.2 Transcript
- **Verbos:** los de Desktop por herramienta en `view::step_title`, con la misma lógica que hoy tienen `sessions__*`.
- **Resumen del grupo:** hasta tres verbos distintos ("Read page, clicked…"), como Desktop; un solo paso dice también sobre qué actuó ("Opened example.com").
- **Tarjeta:** después de un `tabs_create`, o de un `navigate` que abrió la pestaña: "{host} · Opened in Browser" y "Open", que muestra el pane en esa pestaña.
  - El resultado empieza con una línea JSON `{"type":"browser","tabId","url"}`, como los de Desktop.
  - En lugar de la miniatura va un globo: una superficie no se reescala.
- **Carga:** las herramientas van con `anthropic/alwaysLoad`, así que el agente no las busca con ToolSearch.

### 4.3 Prompt
Un bloque corto, redactado para Newen, que se agrega como `appendSystemPrompt` junto al de sesiones:
- hay un navegador integrado y la persona ve lo mismo;
- conviene verificar los cambios web abriéndolos y mirando la consola;
- localhost está permitido y lo demás pide permiso.

El texto de las descripciones de Desktop no se copia.

## 5. Seguridad
- **Sitios:**
  - Las direcciones locales (localhost, 127.x, `*.localhost`, `[::1]`) y `about:`/`data:` nunca preguntan.
  - Otros sitios se usan si ellos, o un dominio del que cuelgan, están en Settings › Browser › Allowed sites.
  - Si no, una tarjeta de permiso: "Allow the agent to use the browser on {site}?", con "Allow once", "Always allow for this website" (que lo agrega a la lista) y "Don't allow".
  - Se pregunta al abrir (`navigate`, `tabs_create`) y al actuar en una pestaña que quedó en otro sitio. La espera no cuenta contra el tiempo de la herramienta.
- **Esquemas:** el agente abre solo páginas web; `file:` y los demás se rechazan.
- **Popups:** las pestañas que abre una página son de la persona; el agente no actúa en ellas.
- **Diálogos JS:** se cierran solos (alert y beforeunload aceptados; confirm y prompt cancelados) y la persona ve lo que decían.
- **Descargas:** van a `~/Downloads`, con aviso.
- **Pipe en lugar de puerto:** ningún otro proceso local puede manejar este Chromium.
- **`$BROWSER`:**
  - Cada agente corre con `BROWSER` apuntando a un script, en una carpeta 0700 con su socket 0600, y con una clave propia de su sesión.
  - Una página local se abre en la pestaña de esa sesión; lo demás, en el navegador del escritorio.
  - Newen acepta cualquier puerto local, no solo los de procesos de la sesión *(simplificación anotada en el código)*.
  - Si la persona ya tiene un `BROWSER` propio, no se toca.

## 6. Estado
Hecho y probado contra Chrome 150 (pruebas de `newen-browser`) y en la app con un agente real (haiku), headless; la vista nativa está en §8:
1. **Ver y usar:** el pane, las pestañas, la navegación, el mouse, el teclado, la rueda, copiar, el cursor, los campos de archivo (con el diálogo del escritorio), las descargas y los diálogos.
2. **El agente:** las quince herramientas, las capturas como imagen, los verbos, el resumen y la tarjeta.
3. **Completo:**
   - permisos por sitio y la lista en Settings;
   - consola, red, JS, lotes y el viewport del agente (`resize_window`);
   - cierre por inactividad; si Chromium se cae, se relanza al siguiente uso.
4. **Integración con el proyecto:**
   - el puente de `$BROWSER`;
   - "Select element": el modo de inspección de Chromium resalta; el elemento elegido se suma al mensaje con su HTML y una captura PNG que el agente lee por su ruta.

5. **Además:**
   - el menú de viewport de la persona (Responsive, Mobile 375 × 812, Tablet 768 × 1024, y el tamaño que fijó el agente);
   - la ruta de Chromium en Settings › Browser;
   - las pestañas de cada sesión reabren después de reiniciar Newen (`BrowserUpdate::Kept`, la única novedad del navegador que se guarda);
   - **servidores de desarrollo** (`preview_start`, `preview_stop`, `preview_list`, `preview_logs`):
     - Leen el archivo de lanzamiento que da el adaptador (`AgentAdapter::launch_file`; para Claude, `.claude/launch.json` con el formato de Desktop, explicado en su prompt).
     - La persona aprueba cada configuración en una tarjeta con el comando ("Always" vale mientras Newen corre).
     - Corren en su propio grupo de procesos, con `PORT`, y mueren con Newen aunque lo maten (`PR_SET_PDEATHSIG`).
     - Se espera hasta 180 s a que escuchen, y como mucho 5 por carpeta.
     - Al estar listos se abren en una pestaña con su tarjeta; los logs se guardan para `preview_logs`.

Falta:
- "Annotate": dibujar sobre la página.
- Que una aprobación de servidor "Always" dure entre reinicios, y la lista de servidores en el pane.
- Miniaturas en la tarjeta del transcript y centrar en el pane una página emulada más angosta.

## 7. Abierto
- **Archivo de servidores de desarrollo:** lo da el adaptador. Si otro agente no tiene uno propio, se podría acordar un `.newen/launch.json` genérico.
- **Pantallas con otra escala:** al mover la ventana a una pantalla con otra escala hay que relanzar Chromium (la escala es por proceso), o aceptar un frame borroso hasta el próximo uso.
- **Sin probar:** el menú contextual de la página (un click derecho por CDP no disparó `contextmenu`) y la composición por GPU en headless.
- **Si no hay Chromium:** el pane lo explica ("No Chromium found. Install chromium or google-chrome, or set its path in Settings."). Descargar Chrome for Testing queda para después.
- **Imágenes en el mensaje:** el mensaje que se manda al agente es solo texto, así que la captura de "Select element" viaja como una ruta `@`. Mandar la imagen en el mensaje mismo, como Desktop, pide que `AgentCommand::Prompt` lleve bloques.

## 8. Vista nativa (CEF)

El screencast (capturas JPEG por CDP) se nota con demora: cada frame es un JPEG en base64 que Chromium no repite hasta el ack, y el resize espera un frame nuevo. Eso no pasa si la página es una capa dentro del compositor de Chromium, pero Newen no es Chromium, así que lo más parecido es CEF sin ventana: cada pestaña dibuja en un buffer y Newen lo compone en su propia ventana, como cualquier otra cosa suya.

- **CEF:** el `cef-binary` de nixpkgs (147.0.10) y `cef = "=147.1.0"` (cef-rs), que coinciden. `shell.nix` arma el directorio que cef-rs espera (`CEF_PATH`). Chromium corre dentro del proceso de Newen (`multi_threaded_message_loop`), con sandbox, y sus procesos son Newen mismo con `--type=…` (`native::subprocess()`, lo primero de `main`).
- **Sin ventana:** `windowless_rendering_enabled` y estilo Alloy. Con frames por CPU, `--ozone-platform=headless`, así que Chromium no abre ningún display, ni X11 ni Wayland; con frames por GPU, `--ozone-platform=x11` (abajo). La GPU va por EGL sin display (`--use-angle=gl-egl`): el default de ANGLE pide un display X11 y, sin él, el proceso GPU se cae y Chromium dibuja por software.
- **El mismo CDP:** `native.rs` contesta el stream que `Cdp` le habla a un Chromium lanzado. Lo de una página va al DevTools de su browser (`SendDevToolsMessage`, en el hilo UI de CEF); lo de todo el navegador se resuelve con CEF:
  - `Target.createBrowserContext` crea un `RequestContext` en memoria por sesión.
  - `Target.createTarget` crea un browser; su `targetId` es el real de la página (`Target.getTargetInfo`), porque `lib.rs` lo compara con el id del frame principal.
  - Títulos, direcciones, popups, cierres y descargas se sintetizan como los eventos `Target.*` y `Browser.download*` de Chromium.
  - `tools.rs` y `lib.rs` tratan igual una pestaña nativa y una headless: el mouse, el teclado, el cursor y el foco emulado son los mismos. Cambian dos cosas: de dónde salen los frames y cómo se fija el tamaño, que es el de la vista (`GetViewRect`) y no `Emulation.setDeviceMetricsOverride`. Con los dos a la vez, Chromium a veces se queda pintando al tamaño viejo; el override queda solo para emular un teléfono.
- **Los frames:** `OnPaint` entrega BGRA a la escala de la ventana (`GetScreenInfo` con `Frames::scale`) y del tamaño que tiene la pestaña en CSS px (`GetViewRect`). Solo se publica la pestaña en pantalla, en su superficie, como el screencast.
  - Un `<select>` abierto llega como un paint aparte (`PET_POPUP`) y se dibuja encima del de la página, en su lugar.
  - Un paint que todavía tiene el tamaño de antes de un resize no se muestra: si se mostrara, quedaría en pantalla hasta que la página volviera a cambiar. Chromium a veces sigue con el tamaño viejo (pasa con un `<select>` abierto) hasta que vuelve a pintar, así que 100 ms después se le pide un repaint (`Invalidate`). Pedirlo junto con el `WasResized` se traga el frame nuevo.
  - Cada paint se convierte una sola vez, de BGRA al RGBA premultiplicado de egui, en una pasada por palabra (`to_rgba`), y la ventana lo toma sin convertir ni copiar (`SurfaceFrame::from_image`, rama de limn `feat/surface-image`). `newen-browser` se compila optimizado también en dev: sin optimizar, cada paint tardaba ~370 ms.
  - Medido con el ejemplo (pane de 800×1100 a escala 2, Radeon 780M): de la rueda al frame en `Frames` hay ~10 ms de mediana (~5 ms de Chromium, con su copia de la GPU a CPU, y ~2,5 ms de la conversión). Con `windowless_frame_rate` a 120, un scroll continuo pinta ~100 frames por segundo (60 con 60).
  - Así Chromium baja cada frame de la GPU, Newen lo convierte y egui lo vuelve a subir. Lo más parecido acá son los frames por GPU.
- **Frames por GPU** (`OnAcceleratedPaint`, `shared_texture_enabled`): el frame no sale de la GPU.
  - eframe pide su device con `VULKAN_EXTERNAL_MEMORY_DMA_BUF` (`wgpu_setup` en `main.rs`) y se lo pasa a `Native::start`.
  - Cada paint llega como un dmabuf de un plano, BGRA. `Gpu::import` lo envuelve con `texture_from_dmabuf_fd` de wgpu-hal 30 (no hizo falta portar el módulo de cef-rs, que además no libera la memoria) y lo copia a una textura propia de la pestaña. Se espera esa copia antes de volver, porque CEF devuelve el buffer a su pool al volver.
  - Un `<select>` (`PET_POPUP`) se copia aparte y encima de la página, también en la GPU.
  - La ventana registra la textura en el renderer de egui-wgpu y limn la dibuja tal cual (`present_surface_texture`, rama de limn `feat/surface-texture`). Mientras la textura no cambie, no se registra de nuevo.
  - Medido con el ejemplo (pane de 800×1100 a escala 2, Radeon 780M, RADV): importar, copiar y esperar tarda 0,64 ms de mediana (p90 1,2 ms), contra ~2,5 ms de la conversión más la bajada de Chromium. Ya no hay subida de 14 MB en el hilo de la UI. De la rueda al frame quedan ~9,6 ms de mediana (CPU: ~11 ms), casi todo la espera de Chromium a su próximo BeginFrame (120 fps).
  - **Por qué X11:** Chromium reserva estos buffers a través de un servidor de display, aunque no abra ventanas. Con `headless` no puede ("Unable to allocate frame for first frame capture: OOM?"). Con `wayland` sí, pero pinta a 1× sin importar la escala de `GetScreenInfo`, `--force-device-scale-factor` o la de la pantalla (probado con `WaylandPerSurfaceScale` y `WaylandUiScale` apagados). Con `x11` (XWayland en una sesión Wayland) respeta la escala. Sin `DISPLAY`, o sin Vulkan que importe dmabufs, los frames van por CPU como antes y Newen lo dice en stderr.
  - En X11 Chromium no informa el modifier (`DRM_FORMAT_MOD_INVALID`), así que se toma lineal (en Wayland llegaba `0x0`, lineal, con el mismo stride alineado a 256). Si una GPU diera buffers en tiles, la página se vería revuelta; habría que pedirle el modifier a GBM.
  - `shell.nix` pone el directorio de CEF al final de `LD_LIBRARY_PATH`. Su `libvulkan.so.1` no sabe dónde NixOS guarda los drivers, y con él primero wgpu no encontraba Vulkan y caía a GL, donde no hay importación.
- **Presentación directa** (`apps/newen/src/direct.rs`): en Wayland, la página no espera el frame de Newen. Va en una subsuperficie propia de Newen, **debajo** de la ventana, y la ventana la deja ver por un hueco.
  - **El hueco:** limn deja que el host pinte una superficie (`present_surface_paint`, rama de limn `feat/surface-paint`). Newen pinta ahí un "punzón": un callback de egui-wgpu que escribe píxeles transparentes sin mezclar, en el orden de pintado. Lo que Newen dibuja después (menús, tooltips, diálogos) queda encima de la página sin hacer nada especial, y los recortes de Newen la recortan sola.
  - **Los buffers:** tres dmabufs lineales de GBM, en el nodo de render de la GPU de wgpu (por el bus PCI del adapter), importados en wgpu para copiar y entregados al compositor con `zwp_linux_dmabuf_v1`. Cada paint se copia en la GPU, se espera la copia y se hace commit en el hilo de CEF (`Native::present_with`). La subsuperficie está en modo desync y no recibe el puntero (región de entrada vacía): la entrada sigue yendo a la ventana, como antes.
  - **Medidas:** el punzón perfora exactamente lo que la subsuperficie muestra. Ese rectángulo (`wp_viewport.set_source`/`set_destination`) es el paint recortado al lugar de la página y a la ventana. Durante un resize, donde la página todavía no llegó, se ve el fondo de Newen, nunca el escritorio, y la página no se sale de la ventana.
  - **Esquinas:** Hyprland redondea las esquinas de cada superficie, también las de una subsuperficie. Por eso la subsuperficie se extiende 32 px lógicos más allá del hueco, debajo de lo opaco de Newen, hacia los lados que quedan dentro de la ventana: las esquinas redondeadas quedan tapadas.
  - **Pantalla completa:** los compositores tratan una ventana a pantalla completa como opaca (ver Weston #470), y el hueco sale negro. Mientras dura, la ventana dibuja la página como textura (el camino anterior) y la subsuperficie se oculta.
  - **La GPU:** con vista nativa, wgpu pide la de bajo consumo (`PowerPreference::LowPower`), la misma que usa el compositor en un laptop con dos. Con la de alto rendimiento (la NVIDIA, en esta máquina) el compositor rechaza los buffers.
  - **Si algo falta** (no es Wayland, no hay `zwp_linux_dmabuf_v1`, `wp_viewporter` o GBM, o no hay frames por GPU), la ventana dibuja la página como antes, y Newen avisa en stderr.
  - **Probado en Hyprland 0.55** (escala 2) por la persona: scroll, resize de los paneles, menús de Newen sobre la página, `<select>`, pantalla completa y vuelta.
- **Cuándo:** si Settings › Browser › "Native view" está prendido, en cualquier sesión (`NEWEN_BROWSER=native|headless` decide igual). Si no, el Chromium headless de siempre. Newen corre en Wayland nativo en ambos casos.
- **Probado:** `cargo run -p newen-browser --example native [dir]` (en `shell.nix`; frames por GPU en un device propio, que se leen de vuelta para comprobarlos, y `NEWEN_NO_GPU=1` para el camino por CPU): el frame al tamaño y la escala del pane, el texto pintado, el teclado, la lista de un `<select>` sobre la página y un tamaño que pide el agente. Las mismas pruebas que `tests/chromium.rs` hace con la headless; es un ejemplo porque CEF necesita un programa propio.

**Antes, con ventanas X11 hijas** (hasta `6279fa9`): cada pestaña era una ventana X11 hija de la de Newen, y en Wayland eso obligaba a XWayland.
- Nada de Newen podía dibujarse encima de la página: había que ocultarla y mostrar su último frame (`Frame::drawn_over`, rama de limn `feat/drawn-over`), como hace Desktop.
- El teclado necesitaba reenviar teclas con `XSendEvent`.
- En una pantalla con escala, Newen y Chromium se veían borrosos o con cursores de tamaños distintos.
- Embeber CEF en una ventana Wayland ajena tampoco anda todavía (issue #2804; el PR #4233 portado a CEF 154 se probó y aborta con el foco del teclado, no se puede ocultar y winit entra en pánico con los `wl_output` de Chromium).

**Embeber CEF en la ventana Wayland, descartado** (2026-10-26): se probó con el SDK del fork de CEF 154 (`wayland.2` y `pacing.1`) y el PR #4233. La vista embebida (estilo Alloy) funciona con varios ajustes: winit parcheado (los `wl_output` de Chromium y el `xdg_surface` para los popups), `gtk-xft-dpi` en 96 dentro del proceso, `--disable-features=WaylandFractionalScaleV1` y `SetFocus` al enfocar la página. Pero el SDK aborta por un DCHECK cuando el teclado desaparece, y nada de Newen puede dibujarse sobre la página. La presentación directa da la misma ruta al compositor con el CEF oficial.

**Falta:** el menú contextual de la página (sin ventana, CEF no lo muestra); Windows y macOS, que con este modo no necesitan ventanas propias; probarlo con un agente real en un escritorio; y que CEF arranque solo cuando se use el navegador (hoy arranca con Newen en modo nativo).
