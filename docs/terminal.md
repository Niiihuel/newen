# Terminal compartida con el agente

El agente corre comandos en una pestaña de la terminal de Newen que la persona ve y usa. Cuando un comando pide algo (la contraseña de `sudo`, un `y/n`), la persona lo escribe ahí. El agente lee la pantalla, sabe si el comando terminó o espera a alguien, y puede contestar lo que no es secreto.

Estado: **hecho el 2026-11-05**, fases T0 a T6 (§8). Falta la prueba a mano con cada CLI real (T7). Lo que se marca *(inferido)* no está confirmado.

Fuentes:
- Mediciones propias del 2026-11-05 en esta máquina (Linux x86_64, fish 4.9.3, bash y zsh): scripts en el scratchpad de la sesión, resumidos en §2.
- Codex CLI, que tiene el par más parecido (`exec_command` y `write_stdin`), leído en su código: [`unified_exec/process_manager.rs`](https://github.com/openai/codex/blob/main/codex-rs/core/src/unified_exec/process_manager.rs), [`process.rs`](https://github.com/openai/codex/blob/main/codex-rs/core/src/unified_exec/process.rs), [`tools/handlers/unified_exec.rs`](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/handlers/unified_exec.rs) y [`tools/context.rs`](https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/context.rs).
- [OSC 133 en Zellij](https://zellij.dev/documentation/shell-integration.html), [en Windows Terminal](https://learn.microsoft.com/windows/terminal/tutorials/shell-integration) y [el OSC 633 de VS Code](https://code.visualstudio.com/docs/terminal/shell-integration).
- [`proc_pid_stat(5)`](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html), [`portable-pty`](https://docs.rs/portable-pty/latest/portable_pty/trait.MasterPty.html) y [`vt100::Callbacks`](https://docs.rs/vt100/0.16.2/vt100/trait.Callbacks.html).
- macOS: XNU [`bsd/kern/tty.c`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/tty.c) y [`bsd/sys/proc_info.h`](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info.h); iTerm2 [`PTYTask.m`](https://github.com/gnachman/iTerm2/blob/master/sources/Tasks/PTYTask.m); Ghostty [`termio/Exec.zig`](https://github.com/ghostty-org/ghostty/blob/main/src/termio/Exec.zig).
- Windows: conhost en [`readDataCooked.cpp`](https://github.com/microsoft/terminal/blob/main/src/host/readDataCooked.cpp) y [`getset.cpp`](https://github.com/microsoft/terminal/blob/main/src/host/getset.cpp).
- MCP: [progreso](https://modelcontextprotocol.io/specification/2025-06-18/basic/utilities/progress) y [tiempos](https://modelcontextprotocol.io/specification/2025-06-18/basic/lifecycle).

## 1. Por qué

- **Claude Code** corre cada comando con `< /dev/null`, medido en `/proc/<pid>/fd/0`. Un `sudo` o un `y/n` reciben fin de archivo en el acto, y nadie puede contestarles. La pestaña que sigue la salida de un comando suyo en segundo plano dice "This shows a command's output; it takes no typing."
- **Codex** tiene su propia terminal (`tty: true` en `exec_command`), pero la persona no la ve.
- **Newen** ya tenía pestañas de terminal (`newen-pty`) que la persona usa. Desde el 2026-11-05 contestan la pregunta DA1 con la que fish 4 arranca (sin respuesta, fish nunca mostraba el prompt). Ahora también el agente las usa.

## 2. Lo medido

| Señal | Cómo se lee | Medido |
|---|---|---|
| Terminó el comando | Marca OSC 133 D, con el código de salida. Sin marcas: el grupo en primer plano vuelve a ser el de la shell (`process_group_leader()`, es decir `tcgetpgrp`). | Sí |
| Espera que alguien escriba | El proceso en primer plano está en `read` o `readv` sobre un `/dev/pts` o `/dev/tty`. `/proc/<fg>/syscall` empieza con `0` (x86_64; `63` en aarch64) y su fd apunta a la terminal. | Sí, con `read -p` y `read -s` de bash |
| Pide un secreto | ECHO apagado con ICANON prendido, en el termios del lado maestro (`MasterPty::get_termios`). Es la misma regla de iTerm2 (`passwordInput`) y Ghostty (`password_input`). | Sí: con `read -s` ECHO se apaga, y al terminar vuelve |
| Marcas de fish | fish 4.9 emite `133;C;cmdline_url=<comando>` al empezar, `133;D;<código>` al terminar y `133;A` / `133;B` en el prompt, sin configurar nada. | Sí: `false` da `D;1` y `sh -c "exit 3"` da `D;3` |
| Marcas de bash y zsh | No las emiten. Newen las agrega en la pestaña del agente (§4). | Sí: tests con `read`, `exit 3` y `(exit 4)` |
| Preguntas de la terminal | fish espera la respuesta a DA1 antes de su primer prompt. Se contestan DA1 y DSR 5/6. | Sí |

Límites:
- `/proc/<pid>/syscall` pide el mismo usuario (ptrace). Un `sudo` ya elevado no se ve leyendo; sí se ve su ECHO apagado, que es lo que importa ahí.
- Un programa que espera con `epoll` (Node: `npm init`) no está en `read`. Para eso existe la vuelta por silencio (§3.3): el agente recibe `running` con `idle_ms` y la última línea, donde se ve la pregunta.
- **macOS:** el kernel duerme a un `read` de la terminal con el mensaje `"ttyin"` (`tty.c`), pero lo guarda por hilo (`uu_wmesg`), y libproc no tiene ningún campo que lo diga. Tampoco se confirmó que `ps -o wchan` lo muestre. Así que en macOS se ve el secreto (ECHO, por termios) y el fin del comando (grupo en primer plano y marcas), y la espera común solo por silencio.
- **Windows:** ConPTY no tiene grupo en primer plano ni avisa al host el modo de eco: conhost dibuja el eco él mismo (`readDataCooked.cpp`) y no hay mensaje que lo reporte *(inferido de lo que no está en el código leído)*. En Windows un comando termina cuando la pestaña lleva 1 s callada (`shortcut:` en `Terminal::state`), y el secreto no se distingue. El puente MCP de OpenCode y Hermes tampoco existe ahí (es un socket Unix, [acp.md](acp.md)).

## 3. Herramientas

Un tercer servidor MCP de Newen, `terminal` (`mcp__terminal__*`), junto a `sessions` y `browser`. Se define en `newen-agent` como `TERMINAL_TOOLS` y `TERMINAL_SERVER`, con el mismo formato que `HOST_TOOLS` y sin nombres de proveedor. Las descripciones son de Newen y dicen cuándo usarlas.

### 3.1 Herramientas

- **`terminal_run{command, yield_ms?, max_chars?}`**
  - Abre la pestaña de la sesión si hace falta, espera a que la shell esté lista y escribe el comando como un solo pegado más Enter. Queda en el historial y la persona lo ve.
  - Se niega si ya corre un comando en la pestaña: una cosa a la vez.
- **`terminal_write{text, enter?, yield_ms?, max_chars?}`**
  - Manda texto al comando que corre (`\u0003` es Ctrl+C) y vuelve igual que `run`.
  - Con `text` vacío solo espera y lee: así el agente sigue un comando largo o espera mientras la persona escribe.
  - **Se niega si ECHO está apagado:** el secreto lo escribe la persona, nunca el agente.
  - **Se niega si no corre nada** (la shell está en su prompt): un comando nuevo va por `terminal_run` y su permiso.
- **`terminal_read{lines?}`**: la pantalla como se ve, más `lines` líneas de arriba, con el estado.

### 3.2 Resultado

Las tres devuelven una línea JSON y el texto:

```json
{"type":"terminal","state":"waiting","exit":null,"secret":true,"idle_ms":120,"prompt":"[sudo] password for nihuel:"}
```

- **`state`** puede ser:
  - `done`, con `exit` cuando la shell lo marca;
  - `waiting`, con `secret` y la última línea con texto como `prompt`;
  - `running`;
  - `idle`: solo en `terminal_read`, la shell en su prompt.
- **El texto** es lo que salió desde que empezó el comando: desde la marca C hasta la D cuando existen, si no desde la línea siguiente a la del comando. Se corta a `max_chars` (20 000 por omisión) y conserva el final, con "[N characters cut]" adelante.
- **Lo escrito con ECHO apagado** no aparece en pantalla, así que no llega al modelo ni al store (está en el test del motor).

### 3.3 Cuándo vuelve

- **Termina:** el comando terminó.
- **Espera:** el comando espera a alguien, y sigue así un sondeo después (100 ms).
- **Silencio:** corre sin imprimir nada hace 2 s (y la llamada lleva al menos 500 ms).
- **Tiempo:** pasó `yield_ms`. Para `run` es de 10 000 ms por omisión; para `write`, 2 000 con texto y 10 000 vacío. Siempre queda entre 250 y 30 000, como en Codex.

Codex vuelve solo por tiempo o por fin del proceso: no detecta una espera ni vuelve por silencio (`collect_output_until_deadline`; tras el fin espera hasta 50 ms a que se cierre la salida). Su respuesta dice `Process exited with code N` o `Process running with session ID N`, y recorta la salida conservando principio y fin. Newen vuelve también por espera y por silencio, porque acá la pregunta es para la persona y el agente tiene que saber pronto que debe avisarle.

Una llamada MCP devuelve un solo resultado y no tiene resultados parciales. Por eso nada bloquea más de 30 s y el agente vuelve a mirar. El progreso de MCP no hace falta.

## 4. Arquitectura

- **Registro compartido.** `newen_pty::Terminals` guarda las pestañas de cada carpeta. Lo crea la app, lo recibe el motor en `Config::terminals`, y la ventana lo usa para dibujar y para mandar las teclas sin pasar por eventos: escribir en un PTY no bloquea.
  - El motor depende de `newen-pty` sin su parte de limn: la feature `view` tiene las celdas y las teclas.
  - `newen-ui` sigue sin depender del motor.
- **Una pestaña por sesión**, en su carpeta, con el título de la sesión (24 caracteres como mucho, si no "Agent"). No cambia la pestaña activa de la persona, y el agente solo escribe en la suya. Se cierra cuando la sesión se archiva.
- **`newen-pty`:**
  - `Terminal::state()`: `Prompt`, `Running` o `Waiting{secret}`, con las señales de §2;
  - `run_command` y `progress`: el fin del comando, el código de salida y la salida;
  - `text` y `lines`: la pantalla como texto;
  - `ready`: la shell dibujó su prompt y lleva 150 ms quieta, o no marca y lleva 1 s;
  - las marcas OSC 133, leídas en `Callbacks::unhandled_osc`;
  - las respuestas DA1 y DSR, en `unhandled_csi`.
- **Marcas propias** (`marks.rs`), solo en la pestaña del agente. Las de la persona quedan como estaban.
  - bash corre con `--rcfile`: carga `~/.bashrc` y marca C con una trampa `DEBUG`, y D y A en `PROMPT_COMMAND`.
  - zsh corre con un `ZDOTDIR` de Newen: carga los archivos de la persona y agrega `preexec` y `precmd` al frente de sus listas.
  - fish se corre como está.
  - Las marcas de fish no traen nonce, así que no se usa ninguno. Un programa que imprime una marca falsa solo hace volver antes la llamada (`shortcut:` en `unhandled_osc`).
- **Motor** (`newen-engine/src/terminal.rs`).
  - Un `Internal::HostTool` de `terminal_*` corre en una tarea aparte, como el navegador. Sondea cada 50 ms hasta que se cumple §3.3 y contesta por `host_answer`.
  - Antes de `terminal_run` pide permiso con el mismo `ask` que un servidor de desarrollo (`kind: "command"`). Un modo que no pregunta nada (Bypass) no pregunta esto. "Always allow in this session" deja correr todos los comandos de la sesión.
- **Persistir antes de avisar.** El resultado de cada paso va al store como cualquier `ToolResult`. Las pestañas no sobreviven a un reinicio; si una se cierra, la herramienta lo dice.

## 5. Adaptadores

| Adaptador | Cómo | Test |
|---|---|---|
| Claude | `terminal` en `initialize.sdkMcpServers` y en cada `mcp_set_servers`, ruteado por `mcp_message`. `can_use_tool` lo permite sin preguntar, como `sessions` y `browser`: el permiso lo pide el motor, en el modo de la sesión. `TERMINAL_GUIDE` va en `appendSystemPrompt`. | `the_terminal_server_lists_its_tools_and_its_calls_reach_the_engine` |
| Codex | En `dynamicTools` de `thread/start`, junto a `HOST_TOOLS`; los ítems se leen como `terminal__<tool>`. Un hilo creado antes no las tiene (docs/codex.md). | `newen_s_tools_are_declared_to_a_new_thread_and_answered_through_the_engine` cuenta `HOST_TOOLS + TERMINAL_TOOLS` |
| ACP (OpenCode, Hermes) | `Bridge::servers()` agrega `newen --mcp-host terminal`, el puente sirve `TERMINAL_TOOLS` y `ours()` reconoce `mcp__terminal__*`. | bridge: `a_session_is_given_all_three_of_newens_servers` y `the_terminal_server_offers_the_terminal_tools`; OpenCode y Hermes: sus nombres se leen como `mcp__terminal__<tool>` |

Codex, OpenCode y Hermes no reciben `TERMINAL_GUIDE`: lo que hace falta saber está en las descripciones de las herramientas.

## 6. Interfaz

- **Paso en el transcript:** "Ran in Terminal `cmd`". Debajo, una tarjeta con:
  - el comando;
  - cómo estaba cuando volvió la llamada: "Waiting for your password", "Waiting for your answer", "Still running", "Done" o "Exited with code N";
  - sus últimas 6 líneas;
  - "Open", que muestra el pane en la pestaña del agente (`TerminalRequest::Agent`).
- **Pasos de las otras herramientas:** "Waiting for you in Terminal", "Answered the command `y`", "Followed the command" y "Read the terminal". La terminal no cuenta como conector en "Used in this session".
- **Aviso:** una respuesta con `state: waiting` es el aviso de una pregunta ("A command is waiting for you in the Terminal."), con el nivel de Settings para preguntas, si la sesión no está a la vista.
- **Pane:**
  - una pestaña cuyo comando espera lleva un punto (●);
  - sobre la terminal, una franja dice "Type the password here; the agent never sees it." o "The agent's command is waiting for your answer.";
  - la pestaña de solo lectura dice que no toma teclas;
  - la grilla resta el alto de la franja.
- **Permiso:** "Allow the agent to run this command in the Terminal?", con la carpeta y el comando, y "Always allow in this session" como alternativa.
- **Sin hacer:** la sesión no pasa a "Needs you" mientras el comando espera; el aviso y la franja lo dicen.

## 7. Seguridad

- **El agente nunca escribe con ECHO apagado.** Lo que la persona escribe ahí no está en pantalla, así que no llega al modelo ni al store.
- **`terminal_read`** devuelve solo la pantalla de la pestaña de la sesión, no las otras pestañas de la persona.
- **Un comando nuevo solo entra por `terminal_run`, con el permiso del modo.** `terminal_write` contesta a un comando que ya corre. Si ese comando es una shell (`ssh`, un REPL), lo que se le escribe corre sin preguntar: se aprobó al correrlo.
- **Ctrl-C** de la persona interrumpe como siempre.

## 8. Fases

| ID | Tarea | Estado |
|---|---|---|
| T0 | Contestar DA1 y DSR 5/6 para que fish arranque. | Hecho: `it_answers_what_programs_ask_of_a_terminal`, `a_shell_that_asks_gets_its_prompt_up` |
| T1 | `newen-pty`: `state`, `progress`, `text`, OSC 133 y los scripts de bash y zsh. | Hecho: `a_command_runs_waits_for_someone_and_ends_with_its_code`, `zsh_gets_newens_marks`, `fish_marks_its_own_commands`, `marks_are_counted_with_the_exit_code` |
| T2 | El motor recibe `Terminals`; la app dibuja y escribe por el mismo registro. | Hecho; sin cambios visibles |
| T3 | `TERMINAL_TOOLS` y su despacho en el motor. | Hecho: `a_command_is_run_answered_and_followed_to_its_end` (permiso, espera, secreto, fin, lectura y cierre) |
| T4 | Adaptadores con su test de oferta. | Hecho (§5) |
| T5 | Interfaz. | Hecho: `a_command_in_the_shared_terminal_is_a_card_a_notice_and_a_band` |
| T6 | Docs: este archivo, agents.md, acp.md, codex.md y engine.md §9. | Hecho |
| T7 | A mano con Claude, Codex y OpenCode: `sudo -k; sudo true` (la persona escribe), `rm -i` (el agente contesta `y`) y `npm init` (preguntas encadenadas). | **Falta** |

## 9. Abierto

- **"Needs you":** que la sesión pase a ese estado mientras su comando espera a la persona.
- **`!` desde el compositor** (engine.md §9.2) podría usar esta misma pestaña en lugar de un PTY oculto.
- **macOS:** probar si `ps -o wchan` (o `kinfo_proc` por sysctl) muestra `ttyin` para un proceso en `read` de la terminal. Si es así, la espera se vería como en Linux.
