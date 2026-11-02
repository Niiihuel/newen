# Remote Control: seguir una sesión desde el celular

Una sesión de Newen se puede seguir y manejar desde otro dispositivo (el celular, otra computadora) a través del servicio del propio agente. El agente sigue corriendo en esta computadora; el otro dispositivo solo muestra la sesión y le manda mensajes. Newen no abre puertos ni maneja credenciales: prende y apaga lo que el CLI ya trae, y muestra lo que llega del otro lado.

El código genérico solo ve una capacidad (`AgentCapabilities::remote`), un comando (`AgentCommand::Remote { on, name }`) y tres eventos (`AgentEvent::Remote`, `Withdrawn` y `Prompt`). Cómo lo hace cada CLI vive en su adaptador.

## 1. Lo que ve la persona

- **Environment:** en la tarjeta del panel derecho, la fila "Remote Control" con su switch, solo si el agente de la sesión lo ofrece. Mientras espera la respuesta dice "Connecting…", y si el agente no pudo, "Failed".
- **Diálogo:** con el switch prendido, la fila abre un diálogo con el link de la sesión y su código QR para la cámara del celular, más "Turn off", "Copy link" y "Done". Si no se pudo prender, el diálogo dice por qué y ofrece "Try again".
- **`/remote-control` y `/rc`:** prenden Remote Control si está apagado y abren el diálogo. Newen los resuelve solo, sin mandarlos al CLI como texto.
- **Settings › Sessions:** "Connect new sessions to Remote Control" prende cada sesión nueva apenas su agente está listo. Viene apagado.
- **Mensajes del otro dispositivo:** se muestran como mensajes de la persona y quedan guardados. Lo que cambia su turno se atribuye a ese mensaje, como con los escritos en Newen.
- **Permisos:** un permiso contestado en el celular saca la tarjeta de Newen.
- **Fin:** cuando el proceso del agente termina (Newen se cierra, la sesión se archiva), Remote Control queda apagado. El estado no se guarda: al volver a abrir la sesión empieza apagado.

## 2. Claude Code (verificado con el CLI 2.1.316)

Tres capturas con el CLI real y un celular, la última guardada como `crates/newen-agent-claude/fixtures/2.1.316/remote.jsonl`. Ahí se mandaron dos mensajes desde el celular, se aprobó un permiso allá y se apagó desde Newen. `tools/capture` tiene el escenario `remote` para repetirlas.

- **Disponible:** la respuesta de `initialize` trae `remote_control_available: true`, que el adaptador pasa a `AgentCapabilities::remote`. Los otros tres campos (`remote_control_auto_enable`, `remote_control_auto_connect_default`, `remote_control_auto_on_by_default`) vinieron en `false`. Newen no los usa: conectar sesiones solas lo decide el ajuste de Newen.
- **Prender:** `{"subtype":"remote_control","enabled":true,"name":"<título>"}`. Responde `{session_url, connect_url, environment_id, bridge_epoch, bridge_session_id}`. `session_url` es el link de la sesión en claude.ai/code, el que muestra el diálogo. Antes de la respuesta llega `system/bridge_state {state:"ready"}`, y medio segundo después `{state:"connected"}`.
- **Apagar:** `{"subtype":"remote_control","enabled":false}` responde un success sin cuerpo.
- **Mensajes del celular:** solo llegan con `--replay-user-messages`, que Newen pasa siempre. Sin ese flag llega solo el id del mensaje (en `command_lifecycle` y `system/status`) y su turno. Con el flag, el mensaje del celular llega como `user` con `isReplay: true`, `origin: {kind: "human"}`, el texto como string y un `uuid` que Newen no mandó.
  - Los mensajes propios de Newen también vuelven con `isReplay`, pero con el `uuid` que Newen les dio y sin `origin`.
  - El adaptador descarta los suyos por `uuid` y convierte los otros en `AgentEvent::Prompt`. El motor los guarda como `Event::Queued`.
- **Permiso contestado en el celular:** el CLI le manda a Newen el `can_use_tool` de siempre. Cuando el celular lo aprueba, llega `control_cancel_request` con ese `request_id`, y después corre la herramienta. El `tool_result` trae `tool_result_meta[].permission_decision {decision: "accept", source: "user_temporary"}`. El adaptador convierte la cancelación de un permiso pendiente en `AgentEvent::Withdrawn` y la UI saca la tarjeta. Antes de este cambio Newen la dejaba colgada, también con un turno interrumpido.
- **Nada que delegar:** con Remote Control prendido desde Newen no llegó `remote_control_work_secret` ni ningún pedido de tokens. El CLI se registra con su propio login.
- **Requisitos** (de la documentación pública de Claude Code): login con suscripción de claude.ai; con API key no anda.
- **Sin verificar:**
  - frenar un turno desde el celular: el turno terminó antes de poder frenarlo; se supone que llega como cualquier turno interrumpido;
  - cambiar modelo o modo desde el celular;
  - fotos desde el celular: hoy solo se muestra el texto;
  - que una sesión reanudada (`--resume`) vuelva a conectarse sola, como dice la documentación del CLI. Si pasa, el adaptador lo toma de `bridge_state` y la sesión queda prendida sin link;
  - qué estados trae `bridge_state` al cortarse la red.

## 3. Codex (un app-server compartido)

Codex trae Remote Control en su `app-server`, con protocolo abierto (Apache-2.0):

- métodos experimentales `remoteControl/enable|disable {ephemeral?}`, `remoteControl/status/read`, `remoteControl/pairing/start {manualCode?}` → `{pairingCode, manualPairingCode, environmentId, expiresAt}`, `remoteControl/pairing/status` → `{claimed}` y `remoteControl/client/list|revoke`;
- la notificación `remoteControl/status/changed {status: disabled|connecting|connected|errored, serverName, installationId, environmentId}`, que todo app-server manda al arrancar.

El celular se empareja con la app de ChatGPT y queda como un cliente más del proceso. Ve los hilos cargados en ese proceso y los maneja. Remote Control funciona por proceso: dos procesos con la misma inscripción chocan (HTTP 409), y un hilo cargado en un proceso no lo puede tomar otro. Por eso, como decidió el dueño, Newen corre **un solo `codex app-server` para todas sus sesiones de Codex** cuando la persona lo pide.

### 3.1 Cómo lo hace Newen

- **La opción:** Settings › Codex › "Remote Control" (opción del adaptador, `remote`). Con ella prendida, las sesiones de Codex que arrancan entran al proceso compartido; las ya abiertas siguen en el suyo hasta reiniciarse.
- **Un proceso por stdio, no `--listen`:** `codex app-server --listen unix://` habla WebSocket por el socket, no líneas JSON, y en ese modo Remote Control se inscribe con una clave vacía, la misma del daemon de Codex y de `codex remote-control`. Las dos cosas chocarían. Por stdio, la inscripción es la de Newen (`clientInfo.name = "newen"`), separada de la de la app de escritorio, y el proceso muere con Newen.
- **El reparto (`newen_agent::Mux`, `CodexMux`):** el motor corre el proceso y pasa cada línea por el multiplexor del adaptador (`crates/newen-agent-codex/src/mux.rs`), así cada sesión habla como si el proceso fuera suyo:
  - cada pedido sale con un id de la conexión y su respuesta vuelve con el de la sesión;
  - el primer `initialize` contesta a todas las sesiones y solo un `initialized` llega al proceso;
  - lo de un hilo va a su sesión, y lo de un subagente a la sesión del padre (`thread/started` trae `parentThreadId`);
  - lo que no es de ningún hilo (`remoteControl/status/changed`, la cuenta) va a todas;
  - lo de un hilo ajeno (uno que arrancó el celular) no va a nadie.
- **El entorno:** las variables del agente y los secretos de los servidores "All agents" van en el entorno del proceso, como iban en el de cada sesión. Las del puente de `$BROWSER`, que son de cada sesión, van en la config del hilo (`shell_environment_policy.set`).
- **Prender y emparejar:** el switch de la sesión manda `remoteControl/enable {ephemeral: true}` (o `disable`), que vale para todo el proceso. "Pair a phone" pide `remoteControl/pairing/start {manualCode: true}` y el diálogo muestra el código y cuántos minutos le quedan. `remoteControl/status/changed` mueve el switch de todas las sesiones.
- **Lo que hace el celular:** un turno que Newen no inició (llega `turn/started` sin un pedido de Newen en camino) es del celular, y su ítem `userMessage` se muestra como mensaje de la persona. Una aprobación que contesta el celular llega como `serverRequest/resolved` y saca la tarjeta.

### 3.2 Verificado

- En vivo (codex-cli 0.163.0, cuenta Plus, por stdio con `experimentalApi`): `remoteControl/status/read` responde `disabled`, y `remoteControl/enable {ephemeral: true}` responde `connecting` al instante.
- Si la inscripción falla, el estado pasa a `errored` y `pairing/start` devuelve el error. Con esta cuenta falló con HTTP 403 `{"detail":"Multi-factor authentication required"}`: la documentación oficial pide MFA en la cuenta de ChatGPT para Remote Control. El adaptador lo dice en palabras.
- Con un app-server falso (`NEWEN_FAKE_APP_SERVER`): dos sesiones en un proceso, cada una con su hilo y sus respuestas, y Remote Control prendido desde una llega a las dos (prueba del motor).

### 3.3 Sin verificar

- El emparejamiento con el celular, el formato y la duración del código, y lo que manda el celular (mensajes, aprobaciones). Las formas vienen del esquema y del código fuente.
- Si la app del celular lista un host Linux.

## 4. Pendiente

- Codex: verificar con el celular (§3.3), la lista de dispositivos y revocarlos, los hilos que arranca el celular (hoy no se muestran) y un mensaje del celular metido en un turno de Newen.
- Codex: el proceso compartido guarda las variables y secretos con que arrancó; un cambio en Settings le llega en su próximo arranque.
- Mantener la computadora despierta mientras una sesión tiene Remote Control prendido (Desktop tiene un ajuste para eso).
- Un grupo propio en la barra lateral para las sesiones con Remote Control.
