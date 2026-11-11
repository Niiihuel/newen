# Portabilidad: Linux, Windows y macOS

Auditoría del 2026-11-01 (rama `feat/windows-macos`). Leyenda: **anda** (código con
camino real), **degradado** (compila y no rompe, pero hace menos), **falta** (no
existe).

Decisiones del dueño para esta pasada: el navegador es **CEF embebido en los tres
sistemas** (cada release lleva CEF), **macOS firmado y notarizado** (Developer ID), y
se publican **macOS arm64 y Windows x64** además de Linux x86_64.

## Qué se verificó y qué sólo se leyó

| Verificación | Resultado |
|---|---|
| Linux | `scripts/check.sh` verde (fmt, clippy `-D warnings`, tests) |
| Windows, desde Linux | `cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -D warnings` limpio, con CEF compilado con su feature `dox` (sin el wrapper de C++). El MSVC real lo compila el CI |
| Windows y macOS, desde Linux | `scripts/check-others.sh`: `cargo check --workspace` para `x86_64-pc-windows-msvc` y `aarch64-apple-darwin`, lo mismo que `check.yml`, sin sus toolchains: `newen-browser/dox` deja a cef-dll-sys sin compilar nada y un compilador de C de mentira crea los archivos que piden los scripts de build. Solo tipos: no enlaza ningún programa |
| Windows y macOS en CI | `check.yml` (`cargo check`) sin `continue-on-error`; `package.yml` arma los tres bundles |
| En una máquina Windows o Mac | **nada se corrió todavía**: todo lo de esos sistemas está escrito leyendo la documentación de CEF, de Chromium y de cada sistema |

## Lo que la auditoría encontró roto

1. **`check.yml` fallaba en Windows y macOS desde el principio**, escondido por
   `continue-on-error`: la vista nativa importaba dmabufs con Vulkan
   (`newen-browser/src/native.rs`), cosa sólo de Linux, y el tipo de ventana que
   CEF recibe cambia por sistema. Por fallar ahí, `newen-engine` y la app nunca se
   habían compilado fuera de Linux (tenían un error más: `reg` con el `Command` de
   tokio).
2. **La release de Linux publicada no arranca en una máquina limpia**: el `ldd`
   del workflow decía `libcef.so => not found`. El artefacto era el programa solo
   y CEF no se empaquetaba; sólo andaba donde CEF ya estaba (Nix).
3. **Windows**: `newen.exe` abría con una consola, y **cada proceso hijo**
   (git, gh, reg, PowerShell, los CLIs de los agentes, servidores de vista
   previa…) abría la suya: faltaban `windows_subsystem` y `CREATE_NO_WINDOW`.
   `Command::new("claude")` no encontraba los `claude.cmd`/`codex.cmd` de npm.
4. **CEF en Windows**: `libcef.dll` era dependencia de carga; sin ella al lado,
   `newen.exe` no llega ni a `main`.
5. **CEF en macOS**: no existía nada de lo que CEF exige ahí: cargar el framework
   en tiempo de ejecución, las apps auxiliares para sus procesos, que el
   `NSApplication` responda `CrAppProtocol`, y su bucle en el hilo principal
   (macOS no admite `multi_threaded_message_loop`).
6. **Ubuntu 23.10+** restringe los user namespaces sin privilegios (AppArmor): el
   sandbox de Chromium no arranca sin un `chrome-sandbox` setuid, imposible en una
   instalación por usuario.
7. **El instalador en macOS** usaba la ruta XDG de Linux y comprobaba un ELF.

## Cómo quedó

| Pieza | Linux | Windows | macOS |
|---|---|---|---|
| Artefacto | `newen-x86_64-linux.tar.gz` | `newen-x86_64-windows.zip` | `newen-aarch64-macos.zip` (`Newen.app`) |
| Dónde está CEF | al lado del programa; `libcef.so` por rpath `$ORIGIN` (`apps/newen/build.rs`) | al lado de `newen.exe` o en `versions\{versión}\`; `libcef.dll` con `/DELAYLOAD` + `SetDllDirectoryW` | framework en `Contents/Frameworks`, cargado con `LibraryLoader` al abrir el navegador |
| Procesos de Chromium | Newen mismo (`--type=…`) | Newen mismo | `newen Helper*.app` con `newen-helper` (`crates/newen-browser/src/bin`) |
| Bucle de CEF | hilos propios | hilos propios | hilo principal: `Native::pump` en cada cuadro y cuando CEF pide (`on_schedule_message_pump_work`) |
| Cuadros de una pestaña | GPU (dmabuf) con X11 + Vulkan, si no CPU | CPU | CPU |
| Sandbox de Chromium | sí, salvo user namespaces restringidos o `NEWEN_NO_SANDBOX` | **no** (falta el `bootstrap.exe` de CEF) | **no** (falta el sandbox en las auxiliares) |
| Instalación y updates | `versions/` + enlace `current` | `newen.exe` en la raíz, CEF en `versions\` | el `.app` mismo, reemplazado entero tras `codesign --verify` |
| Firma | — | **sin Authenticode** | Developer ID + notarización (secretos en `docs/UPDATES.md`) |
| Navegador por defecto | lo que diga Ajustes (headless o nativo) | nativo siempre (no hay headless) | nativo siempre |

## Subsistemas

| Subsistema | Linux | Windows | macOS | Notas |
|---|---|---|---|---|
| Procesos hijos (`newen_files::command`) | anda | anda: sin consola; busca por `PATH`×`PATHEXT` | anda | Matar un agente que es `.cmd` mata `cmd.exe`, no el `node` de abajo: quedaría huérfano (falta un *Job Object*) |
| Enlace Newen ↔ newen-ide (`newen-link`) | socket Unix | TCP en 127.0.0.1 con secreto en archivo | socket Unix | Windows sin probar en tiempo de ejecución |
| Terminal (`newen-pty`) | anda | degradado: `%COMSPEC%`; `busy()` siempre `false` | anda | ConPTY vía `portable-pty`, sin probar |
| Archivos: copiar, mover, papelera | anda | degradado: copiar un enlace simbólico da error; mover sin reemplazar no es atómico | anda salvo lo atómico | `trash` es multiplataforma |
| Abrir en el sistema / mostrar en carpeta | `xdg-open`, D-Bus | `rundll32`, `explorer /select,` | `open`, `open -R` | |
| Carpetas de datos y config | XDG | `%LOCALAPPDATA%`, `%APPDATA%` | `~/Library/Application Support` | `newen_files::{home_dir, data_dir, config_dir}` |
| Iniciar al abrir sesión | autostart `.desktop` | clave `Run` (`reg`) | LaunchAgent | Win/mac sin probar; el plist toma efecto en el próximo login |
| No dejar dormir | `systemd-inhibit` | **falta** (`SetThreadExecutionState` necesita FFI) | `caffeinate` | |
| Batería | `/sys/class/power_supply` | vacío | vacío | "no mantener despierto a batería" no distingue fuera de Linux |
| Notificaciones | `notify-send`/`busctl` | PowerShell | `osascript` | Un clic abre la sesión: Linux con `notify-send --action --wait` (no por `busctl`); Windows mientras su PowerShell espera, 10 min, sin probar; macOS **falta** (pide una app firmada) |
| Sonido propio de Newen | `pw-play`, `paplay`, `aplay` | PowerShell `SoundPlayer` | `afplay` | Windows y macOS sin probar |
| Leer en voz alta | `spd-say` | **falta** | **falta** (sería `say`) | |
| Servidores de `preview_start` | grupo + `PDEATHSIG` | `taskkill /T /F` | grupo, sin `PDEATHSIG` | En Windows no mueren si Newen muere de golpe |
| `$BROWSER` de los agentes (bridge) | socket Unix | **falta**: abren el navegador del sistema | anda en teoría | |
| Navegador headless (Chromium por pipe) | anda | **falta** (`--remote-debugging-io-pipes` con handles) | anda | Sin importancia mientras CEF esté en el bundle |
| Página bajo la ventana (`direct.rs`) | Wayland | no aplica | no aplica | Ventana transparente sólo en Linux |
| Reinicio tras actualizar | espera por `/proc/<pid>` | espera 1 s fija | espera 1 s fija | |
| Zona horaria / nombre | `date`, `getent` | UTC; `USERNAME` | `date`; usuario | cosmético |
| Fuentes del sistema (`fc-match`) | anda | degradado: fuentes de limn | igual | |
| Salida por consola | sí | **se pierde**: `newen --install` no imprime nada (el código de salida sí) | sí | consecuencia de `windows_subsystem` |
| Ajuste "navegador nativo" | decide | no hace nada (siempre nativo) | no hace nada | habría que ocultarlo ahí |

## Qué falta, en el orden recomendado

1. **Correr los bundles en máquinas reales.** Bajar los artefactos del run de
   `package.yml` y, en cada sistema: arranca, abre una sesión de Claude y de
   Codex, el navegador muestra una página y responde al teclado, la terminal anda,
   `newen --install` instala, y una actualización con `newen-test-feed` (ver
   `docs/UPDATES.md`) reemplaza la versión. En macOS además: la ventana minimizada
   no congela el navegador (el bucle depende de los cuadros de la ventana), y
   Gatekeeper abre el `.app` notarizado sin avisos.
2. **Encender las releases**: cargar los secretos de macOS y poner
   `NEWEN_WINDOWS_RELEASES` y `NEWEN_MACOS_RELEASES` en `true`.
3. **Copias de Linux ≤ 0.1.3**: rechazan el artefacto nuevo (no es un ELF) y se
   reinstalan una vez a mano.
4. **Windows**: firma Authenticode (sin ella SmartScreen avisa en la primera
   descarga), *Job Object* para que matar un agente `.cmd` mate su árbol, el
   sandbox de CEF, mantener despierto, voz, batería y el bridge de `$BROWSER`.
5. **macOS**: sandbox de CEF en las auxiliares, `say` para la voz, batería
   (`pmset`), y vigilar el pid sin `/proc`.
6. **Todos**: cuadros por GPU fuera de Linux (handles compartidos de D3D11,
   IOSurface en macOS) si la CPU no alcanza en pantallas grandes.
