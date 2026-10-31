# Remote Control: seguir una sesión desde el celular

Una sesión de quill se puede seguir y manejar desde otro dispositivo (el celular, otra computadora) a través del servicio del propio agente. El agente sigue corriendo en esta computadora; el otro dispositivo solo muestra la sesión y le manda mensajes. quill no abre puertos ni maneja credenciales: prende y apaga lo que el CLI ya trae, y muestra lo que llega del otro lado.

El código genérico solo ve una capacidad (`AgentCapabilities::remote`), un comando (`AgentCommand::Remote { on, name }`) y tres eventos (`AgentEvent::Remote`, `Withdrawn` y `Prompt`). Cómo lo hace cada CLI vive en su adaptador.

## 1. Lo que ve la persona

- **Environment:** en la tarjeta del panel derecho, la fila "Remote Control" con su switch, solo si el agente de la sesión lo ofrece. Mientras espera la respuesta dice "Connecting…", y si el agente no pudo, "Failed".
- **Diálogo:** con el switch prendido, la fila abre un diálogo con el link de la sesión y su código QR para la cámara del celular, más "Turn off", "Copy link" y "Done". Si no se pudo prender, el diálogo dice por qué y ofrece "Try again".
- **`/remote-control` y `/rc`:** prenden Remote Control si está apagado y abren el diálogo. quill los resuelve solo, sin mandarlos al CLI como texto.
- **Settings › Sessions:** "Connect new sessions to Remote Control" prende cada sesión nueva apenas su agente está listo. Viene apagado.
- **Mensajes del otro dispositivo:** se muestran como mensajes de la persona y quedan guardados. Lo que cambia su turno se atribuye a ese mensaje, como con los escritos en quill.
- **Permisos:** un permiso contestado en el celular saca la tarjeta de quill.
- **Fin:** cuando el proceso del agente termina (quill se cierra, la sesión se archiva), Remote Control queda apagado. El estado no se guarda: al volver a abrir la sesión empieza apagado.

## 2. Claude Code (verificado con el CLI 2.1.316)

Tres capturas con el CLI real y un celular, la última guardada como `crates/quill-agent-claude/fixtures/2.1.316/remote.jsonl`. Ahí se mandaron dos mensajes desde el celular, se aprobó un permiso allá y se apagó desde quill. `tools/capture` tiene el escenario `remote` para repetirlas.

- **Disponible:** la respuesta de `initialize` trae `remote_control_available: true`, que el adaptador pasa a `AgentCapabilities::remote`. Los otros tres campos (`remote_control_auto_enable`, `remote_control_auto_connect_default`, `remote_control_auto_on_by_default`) vinieron en `false`. quill no los usa: conectar sesiones solas lo decide el ajuste de quill.
- **Prender:** `{"subtype":"remote_control","enabled":true,"name":"<título>"}`. Responde `{session_url, connect_url, environment_id, bridge_epoch, bridge_session_id}`. `session_url` es el link de la sesión en claude.ai/code, el que muestra el diálogo. Antes de la respuesta llega `system/bridge_state {state:"ready"}`, y medio segundo después `{state:"connected"}`.
- **Apagar:** `{"subtype":"remote_control","enabled":false}` responde un success sin cuerpo.
- **Mensajes del celular:** solo llegan con `--replay-user-messages`, que quill pasa siempre. Sin ese flag llega solo el id del mensaje (en `command_lifecycle` y `system/status`) y su turno. Con el flag, el mensaje del celular llega como `user` con `isReplay: true`, `origin: {kind: "human"}`, el texto como string y un `uuid` que quill no mandó.
  - Los mensajes propios de quill también vuelven con `isReplay`, pero con el `uuid` que quill les dio y sin `origin`.
  - El adaptador descarta los suyos por `uuid` y convierte los otros en `AgentEvent::Prompt`. El motor los guarda como `Event::Queued`.
- **Permiso contestado en el celular:** el CLI le manda a quill el `can_use_tool` de siempre. Cuando el celular lo aprueba, llega `control_cancel_request` con ese `request_id`, y después corre la herramienta. El `tool_result` trae `tool_result_meta[].permission_decision {decision: "accept", source: "user_temporary"}`. El adaptador convierte la cancelación de un permiso pendiente en `AgentEvent::Withdrawn` y la UI saca la tarjeta. Antes de este cambio quill la dejaba colgada, también con un turno interrumpido.
- **Nada que delegar:** con Remote Control prendido desde quill no llegó `remote_control_work_secret` ni ningún pedido de tokens. El CLI se registra con su propio login.
- **Requisitos** (de la documentación pública de Claude Code): login con suscripción de claude.ai; con API key no anda.
- **Sin verificar:**
  - frenar un turno desde el celular: el turno terminó antes de poder frenarlo; se supone que llega como cualquier turno interrumpido;
  - cambiar modelo o modo desde el celular;
  - fotos desde el celular: hoy solo se muestra el texto;
  - que una sesión reanudada (`--resume`) vuelva a conectarse sola, como dice la documentación del CLI. Si pasa, el adaptador lo toma de `bridge_state` y la sesión queda prendida sin link;
  - qué estados trae `bridge_state` al cortarse la red.

## 3. Codex (pendiente: un app-server compartido)

Codex trae Remote Control en su `app-server` (protocolo abierto, Apache-2.0):

- métodos experimentales `remoteControl/enable|disable {ephemeral?}`, `remoteControl/status/read`, `remoteControl/pairing/start {manualCode?}` → `{pairingCode, manualPairingCode, environmentId, expiresAt}`, `remoteControl/pairing/status` → `{claimed}`, `remoteControl/client/list|revoke`;
- la notificación `remoteControl/status/changed {status: disabled|connecting|connected|errored, serverName, installationId, environmentId}`.

Se empareja con la app de ChatGPT del celular, con el código, y solo con login de ChatGPT. Funciona por proceso: dos procesos con la misma inscripción chocan, y un hilo cargado en un proceso no lo puede tomar otro. Por eso, como decidió el dueño, quill va a correr **un app-server compartido** para todas sus sesiones de Codex:

- `codex app-server --listen unix://<ruta de quill>`, y cada sesión entra con `codex app-server proxy --sock <ruta>`;
- `experimentalApi: true` en `initialize`;
- `remoteControl/enable {ephemeral: true}`, con la decisión guardada por quill;
- emparejar desde Settings con el código manual. El CLI abierto solo imprime ese código y no un QR;
- lista de dispositivos y revocarlos;
- `serverRequest/resolved` para sacar la tarjeta que el celular contestó;
- los turnos que arranca el celular.

A resolver al hacerlo: las variables y secretos por sesión, que hoy van en el entorno de cada proceso, y qué pasa con todas las sesiones si el proceso compartido se cae.

## 4. Pendiente

- Codex (§3).
- Mantener la computadora despierta mientras una sesión tiene Remote Control prendido (Desktop tiene un ajuste para eso).
- Un grupo propio en la barra lateral para las sesiones con Remote Control.
