# Actualizaciones

quill se actualiza solo desde las releases de `Niiihuel/quill`, un repositorio
**público** desde el 27-10-2026: se leen sin token (ver [Dónde se
lee](#dónde-se-lee)). Cada tag `v*` publica una release con un manifiesto
firmado; una
versión es **obligatoria** si el mensaje de su tag anotado tiene la palabra
«mandatory», y entonces quill se actualiza antes de seguir. Si no, la
actualización es **opcional**: se descarga sola (si Ajustes lo permite), queda
preparada y se aplica sola en el próximo arranque, o antes si la persona pide
reiniciar.

Código: `crates/quill-update` (manifiesto, firma, descarga, instalación y el
servidor de prueba), `crates/quill-engine/src/update.rs` (búsquedas, descargas
y estados, fuera del hilo de la interfaz), la interfaz en `crates/quill-ui`
(`updates.rs`, con los componentes de limn), `apps/quill/src/main.rs` (reinicio
y `quill --install`), `tools/quill-release` (las herramientas del dueño:
`quill-release` firma las releases y `quill-test-feed` es el feed local; no van
en la app) y `.github/workflows/release.yml` (CI).

## Cómo funciona

Cada release publica, además de sus artefactos:

- `quill-update.json`: versión, fecha, notas (el mensaje del tag),
  `minimum_version` y, por plataforma (`linux-x86_64`, `windows-x86_64`,
  `macos-aarch64`), la URL, el tamaño y el SHA-256 del artefacto. Desde 0.1.9
  (manifiesto v2) también:
  - `product` (`"quill"`; `"newen"` también se acepta, ver [El paso a
    Newen](#el-paso-a-newen)) y `channel` (`"stable"`): uno de otro producto o
    canal se rechaza;
  - `sequence`: un número que sólo crece (ver [Seguridad](#seguridad));
  - `expires` (opcional, RFC 3339): pasada esa fecha el manifiesto se rechaza;
  - `next_keys` (opcional): el conjunto completo de claves en que confiar desde
    ahora, para rotar la clave (ver [La clave](#la-clave));
  - `files`: por plataforma, URL, tamaño y SHA-256 de `files-<plataforma>.json`,
    la lista de archivos del bundle (ver [Archivo por
    archivo](#archivo-por-archivo)).
- `quill-update.json.sig`: la firma Ed25519 de esos bytes exactos, en base64.
- `quill-update.json.sigs` (opcional): más firmas, una por línea; sólo en una
  rotación de clave. `.sig` sigue teniendo una sola línea porque 0.1.8 y
  anteriores leen sólo ese archivo.
- `whats-new.json` y sus medios, si la release trae novedades (ver
  [Novedades](#novedades-whats-new)); el manifiesto los nombra y la misma firma los cubre.
- `files-<plataforma>.json` y los blobs `f-<sha256>.gz` (ver [Archivo por
  archivo](#archivo-por-archivo)).

Las copias hasta 0.1.8 leen un manifiesto v2 sin problema: serde ignora los
campos que no conoce (ninguna estructura del manifiesto tiene
`deny_unknown_fields`, ni en 0.1.8), y el manifiesto sigue pesando unos pocos KB
(0.1.8 rechaza uno de más de 64 KiB): por eso las listas de archivos van aparte,
nombradas por digest, y no dentro del manifiesto.

`minimum_version` es la versión obligatoria más nueva que sea menor o igual a
la publicada: el CI la calcula leyendo los mensajes de **todos** los tags
anotados (un tag liviano no cuenta). Una copia instalada es obligada a
actualizarse si su versión es menor que `minimum_version` de la última
release; así una obligatoria vieja sigue mandando aunque después haya salido
una opcional.

### Una búsqueda

1. Se busca la última release (ver [Dónde se lee](#dónde-se-lee)) y
   se bajan `newen-update.json` o, si no está, `quill-update.json`, su firma y
   `.sigs` si existe, con tope de tamaño (ver [El paso a
   Newen](#el-paso-a-newen)).
2. **La firma se verifica antes de leer el JSON**, con `verify_strict` y las
   claves en que se confía (las compiladas, o las que dejó una rotación). Una
   firma que no verifica no instala nada. Después se rechaza un manifiesto de
   otro producto o canal, vencido, o con `sequence` menor que la más alta vista;
   la más alta (`update_sequence`) y las claves de una rotación (`update_keys`)
   se guardan en `quill-store` antes de que la interfaz se entere.
3. Se comparan versiones con SemVer. Si hay una más nueva, el estado pasa a
   «versión disponible» con sus notas; si es obligatoria, o si Ajustes tiene
   **Actualizar solo** encendido, se descarga sin preguntar. Una versión que la
   persona saltó o que se volvió atrás en esta computadora no se ofrece (ver
   abajo).
4. **Cuándo se busca**, siempre con el reloj de pared (no con un temporizador
   que se detiene mientras la computadora duerme):
   - al arrancar, pasados 20 s, sólo si la última búsqueda tiene más de una
     hora; si no, cuando se cumple su intervalo;
   - cada **4 horas ± 15 %** (al azar, para que no busquen todas a la vez);
   - cuando la ventana vuelve a tener el foco o la computadora se despierta
     (el reloj de pared saltó más que el del proceso) y la última búsqueda
     tiene más de una hora;
   - **Buscar actualizaciones**, en el momento.

   El fin de la última búsqueda se guarda en `quill-store` (`update_last_check`)
   antes de mostrarse, y vale entre arranques.
5. **Se sigue buscando con una versión bajando o lista.** Si aparece una más
   nueva, la reemplaza: la descarga anterior se corta y la nueva queda
   preparada en su lugar. Una obligatoria toma el control igual.

Los estados que ve la persona (`quill_model::UpdateState`):

| Estado | Qué se muestra |
| --- | --- |
| Sin buscar / Buscando | «Buscando actualizaciones…» |
| Al día | «quill está al día · Revisado hace 5 min» |
| Disponible | En Ajustes, «La versión 0.3.0 está disponible», con las notas, **Download** y **Skip this version**. Con **Actualizar solo** apagado, además la tira «Update available · v0.3.0 · Download» (pulsarla descarga) |
| Descargando | En Ajustes, «Downloading 0.3.0 · 42%»; con **Actualizar solo** apagado, también en la tira («Downloading update · 42%»); si es obligatoria, una ventana sin cerrar |
| Lista | En la barra lateral, sobre el botón de la cuenta, una tira «Relaunch to update · v0.3.0» (con la barra oculta, un punto en el botón de mostrarla); «Restart to update» en el menú de la cuenta y en Ajustes; y un aviso del sistema (ver abajo) |
| Error | el motivo, en una línea discreta, con **Reintentar** |
| Compilado desde el código | «Compilado desde el código: actualiza con git pull» |
| Sin instalar | «Ejecuta quill --install para mantenerlo actualizado» |

En Ajustes la línea dice además cuándo terminó la última búsqueda («… ·
Checked 3 minutes ago»), así que **Buscar actualizaciones** siempre responde:
«Up to date · Checked just now», el progreso de lo que baja, o el error. Si una
búsqueda falla con una versión bajando o lista, ésta sigue a la vista y la línea
suma «Last check failed: …».

**El aviso del sistema** (el notificador de la computadora, el mismo de las
sesiones; pulsarlo trae quill al frente) sale una sola vez por versión: cuando
queda lista con la ventana sin foco, o cuando lleva 24 horas lista sin que quill
se reinicie. La versión avisada se guarda en `quill-store` (`update_notified`)
antes de mostrar el aviso, así que nunca se repite.

**Saltar una versión.** **Skip this version** guarda esa versión en
`quill-store` (`update_skipped`) antes de que la interfaz se entere; no se
vuelve a ofrecer hasta que salga una más nueva. Una obligatoria no se puede
saltar.

Pulsar la tira, la entrada del menú o el botón de Ajustes reinicia al instante
si ninguna sesión trabaja. Si alguna está en marcha o esperando a la persona, abre «Agents are
still working» con **Cancel**, **Update anyway** y **Wait for agents**: esta
última deja la tira en «Restarting when agents finish» (con una x que lo
cancela) e instala la primera vez que no queda ninguna sesión ocupada. Con
**Actualizar solo** encendido, la búsqueda y la descarga no muestran nada en la
barra lateral: su progreso está sólo en Ajustes › Sistema. La tira, el punto y
el diálogo son como en Claude Desktop; la entrada del menú y el aviso del
sistema son de quill (Desktop no los muestra). Las palabras del diálogo dicen
«agents» porque quill no nombra a ningún proveedor.

Una copia compilada desde el código (bajo una carpeta `target`, o con
`QUILL_NO_UPDATE` puesto) no busca, no descarga y no se instala sola. Una copia
descargada que nunca pasó por `quill --install` tampoco se toca.

### Descarga e instalación

- El artefacto se baja a `app/downloads/` (en Windows y macOS, a
  `quill-updates` en la carpeta temporal) con el digest en el nombre
  (`quill-{sha256}.part` mientras baja). Si la conexión se corta, el siguiente
  intento sigue desde donde quedó (`Range`); si el servidor no admite rangos,
  empieza de nuevo.
- Al terminar, se comprueban **tamaño y SHA-256** contra lo firmado. Si no
  coinciden, se borra y no se instala nada. Una descarga que está completa y
  verificada se reutiliza si la instalación falló.
- **Preparar** (*stage*): verificada, la versión se desempaqueta en su lugar
  definitivo pero sin ser la que arranca (Linux `versions/<v>`, Windows
  `versions\<v>` y `quill.exe.<v>.new`, macOS `.quill.app.<v>`), se guarda
  `staged.json` (versión, SHA-256, ruta y hora) en la instalación, se borra la
  descarga y **recién entonces** la interfaz se entera de que está lista.
  `staged.json` va en la instalación y no en `quill.db` porque lo lee `main()`
  antes de abrir nada (ni la ventana ni la base), vale sólo para esa
  instalación (una de prueba con `QUILL_UPDATE_ROOT` no toca la de la persona)
  y está al lado de lo que nombra.
- **Activar**: el único cambio atómico (el enlace `current`, el `quill.exe`, o
  el `quill.app`) y la limpieza de versiones viejas. Pasa en dos momentos:
  - **en el próximo arranque, solo**: `main()`, antes de la ventana (junto a
    `wait_for_restarter`), ve una versión preparada más nueva que la que corre,
    la activa a prueba (ver [Volver atrás](#volver-atrás)) y arranca el
    programa instalado con los mismos argumentos; queda en el registro;
  - **«Restart to update»**: lo mismo en el momento; la ventana vieja se cierra
    después de que la nueva empezó (`QUILL_RESTART_AFTER` y
    `NEWEN_RESTART_AFTER`: la nueva espera a
    que la vieja salga, porque el socket de quill-ide sólo lo atiende una
    ventana a la vez).
- Los datos de la persona (sesiones, ajustes, `~/.config/quill`) no los toca la
  actualización.

### Archivo por archivo

Entre 0.1.7 y 0.1.8 cambió un solo archivo, el programa (75,5 MB sin
comprimir, 26,8 MB en gzip), pero el bundle son 233 archivos y 638,8 MB, y el
`.tar.gz` entero 198 MB. Por eso, si la release trae la lista de archivos de la
plataforma, se intenta primero archivo por archivo:

- `files-<plataforma>.json` (que el manifiesto nombra por tamaño y SHA-256, así
  que la firma la cubre): `{version, platform, files: [{path, size, sha256,
  mode, url, packed}], links: [{path, target}]}`. `path` es relativo, con `/`,
  tal como lo tiene el archivo de la release (macOS: `quill.app/…`); `url` es el
  blob del archivo, `f-<sha256 del archivo>.gz`, en gzip, y `packed` su tamaño;
  `links` son los enlaces simbólicos (los del framework de macOS). Se rechaza la
  lista entera si un camino sale del bundle (`..`, absoluto, `\`, `:`), se
  repite, o un enlace apunta afuera.
- Un blob que ya publicó una release anterior se nombra con la URL de esa
  release: cada release sube sólo los archivos que cambiaron. Por eso **las
  releases viejas de `Niiihuel/quill` no se borran** (ni sus assets `f-*.gz`):
  una release posterior puede seguir nombrándolos.
- La copia arma la carpeta de la versión nueva con los archivos de la versión
  que corre (Linux `current/`, Windows `quill.exe` y `versions\<la que corre>\`,
  macOS `quill.app/`) cuyo SHA-256 coincide (en Linux un enlace duro si el modo
  es el mismo; si no, una copia; en macOS la copia de APFS es un clon), y baja
  sólo los blobs que faltan a `downloads/blobs/` (reanudables con `Range`, con
  tope en `packed`), los descomprime comprobando tamaño y SHA-256, pone los
  modos y crea los enlaces. El `.zip` de macOS se arma con `ditto --sequesterRsrc`, así
  los atributos van en `__MACOSX/`, que la lista deja afuera. En macOS además `codesign --verify --deep --strict`
  sobre el `quill.app` armado: tiene que ser exactamente el bundle firmado. Lo
  arma `Install::stage_with`, así que la verificación de que es un programa de
  este sistema, el `sync` y el renombre son los mismos que con el archivo
  entero.
- El progreso cuenta los bytes que de verdad se bajan. Con sólo el programa
  cambiado, una actualización baja unos 27 MB en vez de 198 MB (más la lista,
  unos 50 KB).
- **Cualquier falla** (lista o blob que no coincide, red, disco, firma de macOS)
  deja lo que se bajó para reintentar, lo anota en el registro («file by file
  failed … the full archive instead») y sigue con el archivo entero, como antes.

### Volver atrás

Antes de activar, quill escribe en la instalación (no en `quill.db`)
`trial.json`: `{version, previous, attempts}`. Cada arranque de esa versión
suma un intento antes de abrir la ventana. Si llega a 2 sin que se haya
borrado, el tercer arranque vuelve a la versión anterior (el enlace `current`
a `versions/<anterior>`; en Windows `quill.exe.old` vuelve a ser `quill.exe`;
en macOS `.quill.app.old` vuelve a ser `quill.app`), anota la versión en
`bad-version` (no se ofrece nunca más, salvo una más nueva; Ajustes dice «Version
X did not start on this computer and was rolled back.»), lo deja en el registro
y arranca la anterior. `trial.json` se borra cuando la versión corrió 60 s con
la ventana abierta y el motor andando.

En macOS esos archivos van en `.quill.app.update/`, al lado del bundle, para no
dejarlos sueltos en Aplicaciones.

Lo que no cubre: una versión que ni siquiera llega a `main()` (por ejemplo, le
falta una biblioteca del sistema) no puede contar sus intentos ni volver atrás.

**Las migraciones de la base deben ser compatibles hacia atrás por una
versión**: si una versión nueva cambia `quill.db` y se vuelve atrás, la
anterior tiene que poder abrirla.

### El registro

`<datos>/quill/updates.log` (`~/.local/share/quill/updates.log` en Linux;
`<datos>/newen/` si Newen movió la carpeta), las
últimas 200 líneas, cada una con la hora en UTC:

```text
2026-11-02T14:03:11Z check: 0.1.9 offered (0.1.8 runs)
2026-11-02T14:03:11Z download: 0.1.9 started, 104857600 bytes
2026-11-02T14:04:02Z download: 0.1.9 done, 104857600 bytes in 51.2 s; size and SHA-256 match
2026-11-02T14:04:05Z stage: 0.1.9 at /home/ana/.local/share/quill/app/versions/0.1.9
2026-11-02T14:04:05Z notify: 0.1.9 is ready (the window is away)
2026-11-02T18:10:40Z activate: 0.1.9 in place of 0.1.8, at the start
2026-11-02T18:10:41Z start: 0.1.9 on trial after 0.1.8, start 1
2026-11-02T18:11:41Z start: 0.1.9 ran a minute, its trial is over
```

También: `check: up to date`, `check failed: <motivo>`, `download: … failed`,
`stage: … failed`, `skip: …`, `rollback: …`, `schedule: the computer woke`. Una
línea nombra versiones, tamaños, tiempos y el motivo de un fallo; **nunca una
dirección**.

## Novedades («What's new»)

Lo que la persona ve, como lo muestra Claude Desktop y con las palabras de quill; lo marcado
«(inferido)» no se midió en pantalla.

### Lo que se ve

- **Antes de reiniciar.** Con una versión lista, pulsar la tira «Relaunch to update» abre la
  tarjeta de esa versión, si trae anuncio y Ajustes lo permite: el medio arriba, a todo el
  ancho, en 16:9 y recortado por las esquinas; el título (24, negrita); el texto y, si hay,
  una lista de puntos; abajo a la derecha **Later** y **Restart and update** (el principal,
  con el foco). La ✕ va sobre el medio, arriba a la derecha, en un círculo translúcido.
  *Later*, la ✕ o Escape cierran la tarjeta y la tira queda. *Restart and update* hace lo
  mismo que la tira: reinicia si ninguna sesión trabaja y, si alguna trabaja o espera a la
  persona, pregunta «Agents are still working» (Cancel / Update anyway / Wait for agents).
  Sin anuncio, la tira reinicia como antes.
- **Después de actualizar.** La primera vez que arranca una versión que llegó por una
  actualización y trae anuncio, la misma tarjeta sale sola, una vez, con **Got it** y, si el
  anuncio nombra adónde ir, **Try it** (el principal), que lleva ahí. Lo «visto» se guarda en
  `quill-store` antes de mostrarla: un cierre a la fuerza no la repite.
- **El botón «What's new».** En el lugar de la tira (cuando no hay una versión lista), con un
  punto de acento y la versión, hasta que la persona abre las notas de esa versión una vez.
  También se llega a las notas siempre desde el menú de la cuenta, «What's new».
- **Las notas.** Un diálogo de 480 de ancho y `min(720, 85 %)` de alto: una sección por
  versión, de la más nueva a la más vieja, con la fecha en negrita y la versión en una
  pastilla; los grupos NEW, IMPROVED y FIXED, en mayúsculas chicas y atenuadas, con sus
  viñetas en texto secundario; un filo entre versiones. Sin ninguna: «No release notes yet.».
  Muestra lo que quill guardó: las últimas 10 versiones instaladas que trajeron notas.
- **Ajustes › Sistema › Updates › «Show what's new»**, encendido de fábrica. Apagado, no
  hay tarjeta antes ni después; el botón y las notas siguen.
- **Una instalación nueva no muestra nada** hasta su primera actualización: sólo cuenta como
  «actualizado» una versión cuyas notas bajó la versión anterior. Por eso la primera versión
  con novedades (la que sigue a 0.1.6) tampoco las muestra: 0.1.6 no sabe bajarlas.
- **Ventanas chicas.** Si el medio no deja lugar para el texto y los botones (unos 300 px),
  el medio no se muestra; el texto se desplaza dentro de la tarjeta y los botones quedan
  siempre visibles.

### Videos y el códec de Cisco

Un anuncio puede traer un clip MP4 (H.264 Constrained Baseline, 1280 × 720 y 30 fps como
mucho; ver la guía de `limn-video`, «Clips») con su `poster`. Se reproduce con `limn::Video`,
sin sonido y en bucle, con el botón ▶/❚❚ de limn; con «reducir movimiento» queda en pausa
sobre el `poster`. Sin códec, o si el clip no se puede leer, se ve el `poster` solo, sin
botón.

El códec es **OpenH264 de Cisco**, el binario que Cisco compila y distribuye: su licencia de
patentes H.264 lo cubre sólo si quill cumple sus condiciones, que están completas en
`licenses/openh264/BINARY_LICENSE.txt`:

- **quill nunca lo trae ni lo baja solo.** La primera vez que una tarjeta tiene un clip, abajo
  del medio pregunta «Play videos in What's new — downloads the OpenH264 video codec from
  Cisco.», con el aviso de Cisco y **Download** / **Not now**. *Not now* no vuelve a
  preguntar en la tarjeta. La respuesta se guarda en `settings.json` (`video_codec`) antes de
  que el motor baje nada (test `the_video_codec_is_fetched_only_after_a_yes_that_is_kept_first`).
- **La persona lo prende, apaga y vuelve a prender** en Ajustes › Sistema › Updates › «Play
  videos in What's new», con el aviso «OpenH264 Video Codec provided by Cisco Systems, Inc.»
  al lado y el estado (bajando, o por qué falló). **Apagado, el archivo se borra**; prenderlo
  de nuevo lo vuelve a bajar.
- **La licencia de Cisco** está en Ajustes › Sistema › quill › Licenses, entera (bajada de
  https://www.openh264.org/BINARY_LICENSE.txt, ver `licenses/openh264/SOURCE.txt`).

Con el sí, el motor (`crates/quill-engine/src/codec.rs`, fuera del hilo de la interfaz) baja
el `.bz2` de esta plataforma directo de `ciscobinary.openh264.org` por HTTPS (la versión, la
dirección, el nombre y el SHA-256 los da `limn_video::cisco::current()`, que el programa le
pasa al motor), lo descomprime (`bzip2`, en Rust puro), compara el SHA-256 con el de la release
y lo guarda, escrito al lado y renombrado, en
`~/.local/share/quill/codecs/openh264/2.6.0/<archivo>`. Lo que no coincide no se escribe. Al
arrancar con el sí ya dado, sólo comprueba el archivo que está. El programa lo carga con
`Codec::load` en otro hilo, que vuelve a comprobar el SHA-256; si no carga, lo dice en el log y
el clip queda en su `poster`. El reproductor se crea al abrir la tarjeta y se suelta al
cerrarla; sólo pide repintar mientras el clip se reproduce a la vista.

**libstdc++.** En Linux, la biblioteca de Cisco necesita `libstdc++.so.6` al cargarse; las
distribuciones de escritorio la traen. En NixOS no está en una ruta estándar: `shell.nix` la
suma a `LD_LIBRARY_PATH` (`stdenv.cc.cc.lib`), y quien arranque quill de otra forma tiene que
darla (por ejemplo con `nix-ld`). Sin ella, los clips muestran el `poster`.

### El formato

`quill-update.json` suma un campo opcional (las copias viejas lo ignoran: serde salta los
campos que no conoce, y `Manifest` no los rechaza):

```json
"whats_new": { "url": ".../whats-new.json", "size": 1234, "sha256": "…" }
```

`whats-new.json`, subido con la release:

```json
{
  "version": "0.2.0",
  "date": "2026-11-10",
  "notes": { "new": ["…"], "improved": ["…"], "fixed": ["…"] },
  "announcement": {
    "title": "A browser beside the chat",
    "body": "Open any page next to the session.",
    "bullets": ["…"],
    "media": [
      { "kind": "video", "url": ".../whats-new-browser.mp4", "size": 812345, "sha256": "…",
        "poster": { "url": ".../whats-new-browser.png", "size": 183402, "sha256": "…" } },
      { "kind": "image", "url": ".../whats-new-browser.png", "size": 183402, "sha256": "…" }
    ],
    "try_it": "browser"
  }
}
```

- `notes` y `announcement` pueden faltar. `media` va en orden de preferencia: quill muestra
  el primero que baja y coincide. Un `kind` desconocido se salta.
- `try_it` es una lista cerrada que la interfaz conoce: `browser`, `terminal`, `files`,
  `diff` (un panel de la sesión a la vista), `tour`, y `settings:<página>` (`general`,
  `system`, `accounts`, `skills`, `plugins`, `connectors`). Otro valor no muestra *Try it*.
- Los textos son texto plano o un Markdown chico (negrita, código, links), que dibuja
  `limn::Markdown`. Límites: título 60 caracteres, cuerpo 280, cada línea 160, cinco puntos;
  `quill-release` rechaza lo que se pasa y quill lo recorta con «…» (y lo dice en el log).

### Descarga y verificación

Con la actualización, después de verificar el artefacto y antes de mostrar la tira, el motor
(`crates/quill-engine/src/update.rs`, fuera del hilo de la interfaz) baja `whats-new.json`
(como mucho 256 KiB), comprueba su tamaño y SHA-256 contra el manifiesto firmado y que nombre
la misma versión, y después cada medio en orden hasta el primero que coincide (como mucho
16 MiB, sólo HTTPS como los artefactos; un video necesita su `poster`). Lo que no coincide se
borra y no se muestra nunca; si ningún medio coincide, el anuncio sale sin medio. Todo queda
en `~/.local/share/quill/whats-new/<versión>/` (`notes.json`, ya en los términos de la
interfaz, y los medios por su digest). Un fallo de las novedades no frena la actualización.

Al arrancar, el motor lee esas carpetas, borra las que pasan de las 10 instaladas más nuevas,
y le cuenta a la interfaz (`Event::WhatsNew`) las notas, la de la versión lista si hay, si
anuncia esta versión (marcada antes en `quill-store`, `whats_new_seen`) y si el botón sigue
(`whats_new_opened`, que se guarda cuando la persona abre las notas y recién entonces el botón
se va).

### Escribir las notas de una release

Junto al cambio de versión, en el repositorio:

```text
release/notes/0.2.0.toml
release/media/0.2.0/browser.png
release/media/0.2.0/browser.mp4
```

```toml
# date = "2026-11-10"            # si falta, el día del tag
new = ["A browser beside the chat"]
improved = ["Faster `git` status in large folders"]
fixed = ["The update strip no longer flickers"]

[announcement]                   # opcional
title = "A browser beside the chat"
body = "Open any page next to the session and point the agent at it."
bullets = ["Pages keep their sign-ins"]
try_it = "browser"

[[announcement.media]]           # un video: file y su poster
file = "browser.mp4"
poster = "browser.png"

[[announcement.media]]           # una imagen: file solo
file = "browser.png"
```

El workflow **Release**, si existe `release/notes/<versión>.toml`, corre
`quill-release whats-new <tag> <base> dist`: arma `dist/whats-new.json`, copia cada medio como
`dist/whats-new-<archivo>` (los assets de una release no tienen carpetas) con su digest, y
rechaza textos o medios que se pasan de los límites: **cada línea de `new`, `improved` y
`fixed`, y cada punto, 160 caracteres como mucho** (el título 60, el cuerpo 280, cinco
puntos); una línea más larga corta la release en ese paso. `quill-release manifest … --whats-new
dist/whats-new.json` nombra ese archivo en el manifiesto antes de firmarlo, y `quill-release
verify` comprueba, además de la firma, que `whats-new.json` y cada medio en `dist/` sean lo que
el manifiesto nombra. Después se suben con el resto. Sin el archivo de notas, la release sale
como antes, sin novedades. Para probarlo a mano:
`cargo run -p quill-release -- whats-new v0.2.0 https://example.org /tmp/wn` (con el tag
anotado creado).

## Dónde se lee

Una búsqueda lee la última release de `Niiihuel/newen` y, si no tiene
manifiesto, la de `Niiihuel/quill` (`REPOSITORIES` en
`crates/quill-update/src/lib.rs`; ver [El paso a Newen](#el-paso-a-newen)) en
sus direcciones públicas,
`https://github.com/Niiihuel/quill/releases/latest/download/<archivo>`, y baja
los artefactos, las listas y los blobs de las direcciones que nombra el
manifiesto firmado. No usa la API de GitHub, así que no necesita token ni cuenta
contra su límite de 60 pedidos por hora sin autenticar: hasta 0.1.8 el
repositorio era privado y quill pedía un token (`QUILL_UPDATE_TOKEN` o `gh auth
token`); desde 0.1.9 ese camino ya no existe, tampoco el estado «Sin acceso».
Las redirecciones de GitHub (al servidor donde guarda los archivos) se siguen a
mano, cinco como mucho, y cada respuesta tiene tope de tamaño.

Las copias hasta 0.1.8 siguen leyendo el mismo repositorio por la API con su
token: encuentran cada archivo de la release por su nombre, así que no hace
falta publicar en otro lado.

Lo que no cubre: quien controle la red puede ocultar una versión nueva (la
copia sigue como está, y una obligatoria se descubre al volver la red), salvo
que el manifiesto tenga `expires`.

## El paso a Newen

quill pasa a llamarse **Newen**. Nadie pierde datos ni actualizaciones si se
sigue este orden:

1. **0.1.9 es la última versión de quill, el puente.** Se publica desde
   `Niiihuel/quill`, que todavía se llama así. Las copias 0.1.4–0.1.8 leen la
   API de GitHub con un token y no siguen redirecciones: sólo se actualizan
   mientras el repositorio se llame `Niiihuel/quill`, así que tienen que llegar
   a 0.1.9 **antes** del cambio de nombre (las 0.1.0–0.1.3 ya no se actualizan
   solas, ver [Linux](#linux)).
2. El dueño renombra el repositorio `Niiihuel/quill` → `Niiihuel/newen`
   (GitHub redirige las direcciones viejas, inferido de su documentación y
   comprobado en el paso 3 de abajo; 0.1.9 sigue redirecciones) y vuelve
   a cargar los secretos y variables del CI con sus nombres nuevos.
3. **Newen 0.2.0** se publica desde `Niiihuel/newen`, con el manifiesto y los
   artefactos con nombre newen. 0.1.9 la instala con su preparación y su prueba:
   si Newen falla sus primeros arranques, vuelve a 0.1.9, y 0.1.9 encuentra sus
   datos aunque Newen ya los haya movido.

### Lo que hace 0.1.9

- **Busca** en cada base `newen-update.json` (con `.sig` y `.sigs` al lado)
  y, si la respuesta es 404, `quill-update.json`, con la misma verificación y
  las mismas reglas de admisión; un `product` `"newen"` vale como `"quill"`.
  Lee primero `Niiihuel/newen` y, si ahí no hay manifiesto (404),
  `Niiihuel/quill`: no depende de la redirección del cambio de nombre. Sólo un
  404 pasa al siguiente; cualquier otra falla es la de la búsqueda. Mientras
  `Niiihuel/newen` no existe, cada búsqueda hace tres pedidos más que antes
  (los tres responden 404: es lo que GitHub responde por un repositorio que no
  existe, inferido y no probado). Un feed de prueba (`QUILL_UPDATE_FEED`) sigue siendo
  una sola base.
- **Instala** una versión cuyo programa es `newen` (Linux), `newen.exe`
  (Windows) o `Newen.app` (macOS), además de las de quill, por archivo entero o
  archivo por archivo:
  - Linux: `versions/<v>/newen`; si la versión no trae `quill`, se le agrega un
    enlace `quill -> newen`, así un lanzador que arranca `current/quill`
    arranca Newen. El reinicio arranca `current/newen`, y la
    activación y la vuelta atrás aceptan cualquiera de los dos programas.
  - Windows: `newen.exe` queda como `quill.exe` en `Programs\quill`, el nombre
    que arrancan los accesos directos y la clave Run.
  - macOS: `Newen.app` queda como `quill.app`.
- **Encuentra los datos** que Newen movió, con o sin los enlaces `quill ->
  newen` que Newen deja en Linux: una sola función (`quill_files::own_dir`) usa
  `<datos>/newen` y `<config>/newen` si existen y `quill/` si no; la base es
  `newen.db` si existe y `quill.db` si no; la instalación de Linux es
  `<datos>/newen/app` si existe. Todo lo que arma rutas pasa por ahí
  (`Config::default_database`, los ajustes que se leen antes de la ventana, las
  imágenes pegadas, el perfil de Chromium, el registro de actualizaciones).
- **Llavero**: lee el servicio `quill` y, si no tiene el secreto, `newen`
  (donde Newen guarda los suyos); escribe sólo en `quill`, y borrar borra de los
  dos, para que un secreto borrado no vuelva desde el de Newen. Escribir también
  borra la copia de `newen`: Newen lee primero la suya, y una copia vieja (de
  una prueba de Newen que volvió a 0.1.9) taparía el valor que la persona
  cambió acá; sin ella, Newen lee el de `quill` y lo copia.
- **Reinicio**: pone `QUILL_RESTART_AFTER` y `NEWEN_RESTART_AFTER`, y espera
  al proceso que nombre cualquiera de las dos.

Lo que 0.1.8 lee de la release 0.1.9 no cambia: `quill-update.json`, su
`.sig` y el programa `quill` en la raíz del archivo. `tools/quill-release`,
`scripts/package.sh` y los workflows publican 0.1.9 como antes.

### Lo que Newen 0.2.0 tiene que cumplir

- Las mismas claves (`release-key.pub`, `backup-key.pub`), los mismos nombres
  de campos del manifiesto y de plataformas, un tag anotado sin `-` y una
  versión y una `sequence` mayores que las de 0.1.9. El paso «the release
  before» de `release.yml` tiene que encontrar el manifiesto de 0.1.9, que se
  llama `quill-update.json` (y sus `files-*.json`), aunque el de Newen se llame
  `newen-update.json`.
- Archivos planos: Linux con `newen` en la raíz (el enlace `quill` es
  opcional), Windows con `newen.exe` (o `quill.exe`), macOS con `Newen.app` (o
  `quill.app`).
- Su propia vuelta atrás a 0.1.9 tiene que aceptar `versions/0.1.9/quill` y, en
  Windows, una instalación vieja cuyo programa es `quill.exe`; y tiene que poner
  `NEWEN_RESTART_AFTER` al reiniciar.
- Su migración deja los datos donde 0.1.9 los busca (`<base>/newen`,
  `newen.db`) y sus secretos en el servicio `newen`.
- **Una compilación de desarrollo de Newen nunca crea `<datos>/newen` ni
  `<config>/newen`** en una máquina cuyos datos de quill no se migraron: 0.1.9
  tomaría esa carpeta vacía por los datos movidos y la persona vería quill sin
  sesiones (siguen en `quill/`, pero 0.1.9 no las mira).

### Los pasos del dueño

1. **Publicar 0.1.9** como cualquier release (ver [Publicar una
   release](#publicar-una-release)) desde `Niiihuel/quill`. Si conviene que las
   copias se actualicen ya, «mandatory» en el tag (ver [Marcar una versión
   obligatoria](#marcar-una-versión-obligatoria)): una copia 0.1.4–0.1.8 que la
   ve la baja enseguida y pide reiniciar; una 0.1.0–0.1.3 que la vea muestra la
   ventana obligatoria sin poder instalarla.
2. **Esperar a que cada copia llegue a 0.1.9.** En cada máquina: Ajustes ›
   Sistema dice 0.1.9 o, en Linux, `current` apunta a ella (0.1.8 no lleva
   registro de actualizaciones):

   ```sh
   readlink ~/.local/share/quill/app/current   # versions/0.1.9
   ```

   Las copias 0.1.4–0.1.8 necesitan un token para leer la API (`QUILL_UPDATE_TOKEN`
   o `gh auth login`); sin él, se reinstalan a mano (ver [Linux](#linux)).
3. **Renombrar el repositorio** y apuntar el clon local al nombre nuevo:

   ```sh
   gh repo rename newen --repo Niiihuel/quill --yes
   git remote set-url origin https://github.com/Niiihuel/newen.git
   # la dirección vieja tiene que responder con una redirección (3xx) a Niiihuel/newen:
   curl -sI https://github.com/Niiihuel/quill/releases/latest/download/quill-update.json | head -n 3
   ```

   Hasta que salga 0.2.0, 0.1.9 encuentra su propia release en
   `Niiihuel/newen` (como `quill-update.json`) y sigue al día.
4. **Volver a cargar los secretos y variables con sus nombres nuevos**, antes
   del tag de 0.2.0 (un secreto no se puede leer de vuelta: sale del archivo de
   la clave, que **no** se mueve ni se borra):

   ```sh
   gh secret set NEWEN_UPDATE_SIGNING_KEY --env release --repo Niiihuel/newen \
     < ~/.config/quill-release/release-key
   gh variable set NEWEN_WINDOWS_RELEASES --body true --repo Niiihuel/newen
   # sólo si macOS entra en las releases (hoy QUILL_MACOS_RELEASES no está puesta):
   gh variable set NEWEN_MACOS_RELEASES --body true --repo Niiihuel/newen
   # comprobar:
   gh secret list --env release --repo Niiihuel/newen
   gh variable list --repo Niiihuel/newen
   ```

   `LIMN_READ_TOKEN`, `MACOS_*`, `APPLE_*` y el entorno `release` no cambian de
   nombre y siguen ahí después del cambio de nombre. Con 0.2.0 publicada y
   funcionando, borrar los viejos:

   ```sh
   gh secret delete QUILL_UPDATE_SIGNING_KEY --env release --repo Niiihuel/newen
   gh variable delete QUILL_WINDOWS_RELEASES --repo Niiihuel/newen
   gh variable delete QUILL_MACOS_RELEASES --repo Niiihuel/newen   # si estaba puesta
   ```

5. **Publicar Newen 0.2.0** desde `Niiihuel/newen` (rama `feat/newen`). En una
   copia 0.1.9, el registro muestra `check: 0.2.0 offered`, `stage: 0.2.0 at …`
   y, al arrancar, `activate: 0.2.0`.

## Publicar una release

1. Subir la versión del workspace en `Cargo.toml` (`[workspace.package]
   version`), `cargo build` para que cambie `Cargo.lock`, commit y push a
   `main`.
2. Crear el tag **anotado** con las notas como mensaje, y subirlo:

   ```sh
   git tag -a v0.2.0 -m "Fixes the start from the cache and adds …"
   git push origin v0.2.0
   ```

3. El workflow **Release** (`.github/workflows/release.yml`):
   - verifica que el tag sea anotado y que nombre la versión del workspace
     (`quill-release tag`);
   - crea una release borrador con el mensaje del tag como notas (un tag con
     `-`, como `v0.2.0-rc.1`, queda como pre-release y las copias instaladas
     no la ven: `releases/latest` la saltea);
   - llama a **Package** (`.github/workflows/package.yml`), que compila el
     artefacto de Linux en Ubuntu 24.04 (`quill-x86_64-linux.tar.gz`), el de
     Windows (`quill-x86_64-windows.zip`) y el de macOS
     (`quill-aarch64-macos.zip`, firmado, notarizado y con el ticket
     engrapado), cada uno con CEF adentro (`scripts/package.sh`; ver cada
     sistema abajo);
   - arma las novedades si hay `release/notes/<versión>.toml` (`quill-release
     whats-new`, ver [Novedades](#novedades-whats-new));
   - baja la última release publicada (`quill-update.json`, sus firmas y
     `files-*.json`; el borrador de ésta no cuenta); `quill-release` la
     verifica con las claves compiladas;
   - desempaqueta cada artefacto y arma su lista (`quill-release files … --previous
     previous`): un archivo cuyo blob ya publicó una release anterior (o otra
     plataforma de ésta) se nombra con esa URL; para los demás escribe
     `f-<sha256>.gz`, lo único que se sube de blobs;
   - arma el manifiesto (`quill-release manifest`, con `minimum_version`, las
     novedades, las listas y la `sequence`: la de la release anterior más uno,
     la misma si es la misma versión, y error si la anterior es más nueva), lo
     firma con el secreto (`quill-release sign`, con el programa ya compilado
     para que ningún build script vea el secreto), lo **verifica con las claves
     compiladas en quill** (`quill-release verify`: un secreto que no coincide
     con `release-key.pub` o `backup-key.pub` corta acá, y también una lista o
     un blob nuevo que no sea lo que nombra), y recién entonces sube los
     artefactos, las novedades, las listas y los blobs nuevos (de a 50), y al
     final `quill-update.json` y sus firmas, y publica la release. El
     manifiesto se arma sobre los mismos archivos que se suben.

   El job que firma y publica corre en el entorno `release` de GitHub: sólo un
   tag `v*` llega a sus secretos y el dueño tiene que aprobar cada corrida
   (*Review deployments* en la corrida).

**Package** también corre solo, en un PR que toca el empaquetado o a mano
(*Run workflow*): deja los tres bundles como artefactos del run para
probarlos. Ahí macOS se firma si están los secretos (si no, firma ad hoc) y
nunca se notariza.

Hasta el último paso ninguna copia instalada ve la release. Si un paso falla,
el borrador queda en GitHub para borrarlo o volver a correr el workflow.

### Marcar una versión obligatoria

Poner la palabra «mandatory» (en cualquier mayúscula, como palabra suelta) en
el mensaje del tag anotado:

```sh
git tag -a v0.3.0 -m "mandatory: the service changed the message format"
```

«mandatories» o «mandatorily» no cuentan. Desde esa release, toda copia con una
versión menor que `0.3.0` se actualiza antes de seguir, aunque después salgan
`0.3.1`, `0.4.0`… opcionales. Un tag publicado no se cambia: para corregir una
obligatoria por error, publicar otra versión.

### Lo que el CI necesita del repositorio

- **limn en su rama.** quill toma `limn`, `limn-egui` y `limn-editor` como
  dependencias por ruta desde `../limn-quill-integration` (el `Cargo.toml` de
  la raíz). Ese checkout es la rama `quill/integration` de `Niiihuel/limn`;
  el workflow la clona dentro del workspace y enlaza
  `../limn-quill-integration` hacia ella.
- **Secreto `LIMN_READ_TOKEN`**: un token de lectura de `Niiihuel/limn` (un
  token de grano fino con solo lectura de ese repositorio). Sin él, el checkout
  usa el `GITHUB_TOKEN` del repositorio de quill, que no alcanza a leer otro
  repositorio privado.
- **Entorno `release`** (*Settings › Environments*): limitado a tags `v*`, con
  el dueño como revisor obligatorio. Su secreto
  **`QUILL_UPDATE_SIGNING_KEY`** es la clave privada (ver [La clave](#la-clave)).
  Ya está cargado en el entorno; el secreto del mismo nombre a nivel de
  repositorio sobra y conviene borrarlo, porque lo ve cualquier workflow de
  cualquier rama: `gh secret delete QUILL_UPDATE_SIGNING_KEY --repo
  Niiihuel/quill`.
- **Variables `QUILL_WINDOWS_RELEASES` y `QUILL_MACOS_RELEASES`**: en `true`
  para que una release incluya Windows o macOS; apagada, la release sale sin
  ese sistema (Linux siempre va).
- **Secretos de macOS** (una release de macOS no sale sin ellos):
  - `MACOS_CERTIFICATE`: el certificado *Developer ID Application* con su
    clave privada, exportado como `.p12` desde Acceso a Llaveros y en base64
    (`base64 -i quill.p12 | pbcopy`).
  - `MACOS_CERTIFICATE_PASSWORD`: la contraseña con que se exportó el `.p12`.
  - `MACOS_SIGNING_IDENTITY`: el nombre del certificado tal como lo muestra
    `security find-identity -v -p codesigning`, p. ej.
    `Developer ID Application: Nombre Apellido (ABCDE12345)`.
  - `APPLE_API_KEY`: una clave de la API de App Store Connect (*Users and
    Access › Integrations › Keys*, rol *Developer*), el `.p8` en base64.
  - `APPLE_API_KEY_ID` y `APPLE_API_ISSUER`: su *Key ID* y el *Issuer ID* de
    esa misma página.

  `notarytool` usa la clave de la API, no el Apple ID ni una contraseña de
  aplicación. Los entitlements con que se firma (JIT de V8, memoria
  ejecutable, cámara y micrófono para páginas) están en
  `apps/quill/assets/quill.entitlements`.

## La clave

La firma usa una clave Ed25519 sólo para esto. La mitad pública va compilada en
quill (`crates/quill-update/release-key.pub`, una línea en base64); la privada
**nunca** entra al repositorio ni a un log.

Se generó una vez, en la máquina del dueño:

```sh
mkdir -m 700 -p ~/.config/quill-release
cargo run -p quill-release -- keygen ~/.config/quill-release/release-key
# imprime la pública: va a crates/quill-update/release-key.pub
```

`keygen` escribe la privada (32 bytes en base64) con modo 0600, en un archivo
que no puede existir todavía, y sólo imprime la pública. El secreto del CI se
carga desde ese archivo, sin imprimirlo:

```sh
gh secret set QUILL_UPDATE_SIGNING_KEY --repo Niiihuel/quill < ~/.config/quill-release/release-key
```

**Guardar una copia de la privada** fuera de la máquina.

### La clave de respaldo

Desde 0.1.9 quill compila también una segunda clave pública,
`crates/quill-update/backup-key.pub`, y acepta un manifiesto firmado por
cualquiera de las dos. Su privada se generó con `quill-release keygen
~/.config/quill-release/backup-signing-key` (carpeta 0700, archivo 0600) y
**no debe quedar en la máquina**: el dueño la copia a un medio sin red (un
pendrive cifrado, papel) y la borra del disco:

```sh
cp ~/.config/quill-release/backup-signing-key /media/<medio>/   # o imprimirla
shred -u ~/.config/quill-release/backup-signing-key
```

Sirve si la clave de release se pierde o se filtra: con ella se firma una
release que rota las claves (abajo), sin reinstalar nada.

### Rotar las claves

`next_keys` en el manifiesto es el conjunto **completo** de claves en que
confiar desde ahora (incluir la de respaldo si debe seguir). Una copia lo
acepta sólo si el manifiesto lo firmó una clave en la que ya confía **y** cada
clave nueva de `next_keys`; entonces guarda ese conjunto (`update_keys` en
`quill-store`) y desde ahí confía sólo en él: una clave que quedó afuera ya no
firma nada para esa copia. Pasos:

1. `quill-release keygen <archivo nuevo>`; la nueva va a `release-key.pub` en una
   release posterior (las copias nuevas la traen compilada).
2. En la release que rota: agregar `next_keys` al manifiesto antes de firmar
   (hoy a mano: `quill-release manifest` no tiene opción para eso), firmar
   `.sig` con una clave en la que confían (la de release, o la de respaldo si
   la de release se perdió) y `quill-release cosign quill-update.json <clave
   nueva>` para cada clave nueva (agrega una línea a `.sigs`).
3. Mientras haya copias que no vieron esa release, las siguientes repiten
   `next_keys` y la firma con la clave vieja en `.sigs`: una copia que se saltó
   la rotación la hace con cualquiera de ellas.

Lo que no cubre: quien tenga una clave en la que se confía puede rotar a una
suya; sin umbral de firmas, la clave de release es tan poderosa como la de
respaldo.

## Probar con el feed local

El updater se prueba sin GitHub, contra un feed local. Todo se puede apuntar a
otro lado con variables de entorno:

| Variable | Qué hace | Dónde vale |
| --- | --- | --- |
| `QUILL_UPDATE_FEED` | URL base con `quill-update.json` y `.sig` (en vez de las releases del repositorio); también hace que una copia sin instalar busque | siempre (igual se verifica con la clave compilada) |
| `QUILL_UPDATE_KEY` | clave pública en base64 en lugar de la compilada | sólo compilaciones de depuración |
| `QUILL_UPDATE_ROOT` | carpeta de la instalación en vez de `~/.local/share/quill/app` | siempre |
| `QUILL_NO_UPDATE` | esta copia nunca busca ni se instala sola | siempre |

El feed de prueba firma una release inventada con una clave descartable y la
sirve en `127.0.0.1`:

```sh
cargo run -p quill-release --bin quill-test-feed -- --mandatory --slow
# imprime QUILL_UPDATE_FEED=… y QUILL_UPDATE_KEY=…; en otra terminal:
QUILL_UPDATE_FEED=http://127.0.0.1:8765 QUILL_UPDATE_KEY=… QUILL_UPDATE_ROOT=/tmp/quill-app \
  cargo run -p quill
```

Opciones: `--version 9.9.9`, `--mandatory`, `--slow` (descarga lenta, para ver el
progreso), `--corrupt` (SHA-256 que no coincide), `--port`, `--artifact <archivo>`
(lo que se instala, armado como lo arma el CI: el `.tar.gz` o el `.zip` de una
compilación con esa versión; sin él es un relleno que no es un archivo
comprimido y la instalación lo rechaza), `--platform windows-x86_64`, `--tree
<carpeta>` (sirve además el bundle desempaquetado en esa carpeta archivo por
archivo, con su lista y sus blobs, como lo publica el CI; macOS: la carpeta que
tiene `quill.app`; una copia lo prueba antes que el artefacto).

`cargo test -p quill-update` cubre lo mismo sin ventana ni red: firmas,
manifiestos alterados, obligatoria contra opcional, descargas que no coinciden,
reanudación y corte,
instalación atómica en Linux con un `.tar.gz` real (programa, `libcef.so` y
`locales/`), preparar sin cambiar lo que arranca, activar en el próximo
arranque, y volver atrás tras dos arranques fallidos; el registro y su
rotación; y el puente a Newen (su manifiesto y su repositorio primero, su
programa con el enlace `quill`, su prueba que vuelve a quill, y dónde quedó la
instalación). Lo de Windows y macOS sólo se compila (la decisión de cada arranque,
las rutas de lo preparado, la comprobación de los primeros bytes del programa y
la ruta del bundle sí se prueban en cualquier sistema).

`cargo test -p quill-engine update::` prueba el motor contra el feed de
`serve`: lo preparado se guarda antes de avisar «lista» y una versión más nueva
lo reemplaza; una release con lista de archivos se prepara bajando sólo el blob
que cambió, y con un blob malo vuelve al archivo entero; saltar una versión; los avisos del sistema (sin foco, a las 24 h,
nunca dos veces); el calendario con su azar y la búsqueda al volver el foco o
despertar; y que el registro no tenga direcciones.

## Linux

**Artefacto**: `quill-x86_64-linux.tar.gz`, un tar comprimido con gzip con todo
en su raíz, sin carpeta: el programa `quill`, los archivos de CEF
(`libcef.so`, `libEGL.so`, `libGLESv2.so`, `libvk_swiftshader.so`, los `.pak`,
`icudtl.dat`, `v8_context_snapshot.bin`…) y la carpeta `locales/`. Se compila
en Ubuntu 24.04 con las bibliotecas de la distribución (no las de Nix) y con
`CARGO_PROFILE_RELEASE_STRIP=debuginfo`. El programa busca `libcef.so` a su
lado (rpath `$ORIGIN`) y los recursos de CEF en su misma carpeta.

**Instalación por usuario**, sin permisos de administrador:

```text
~/.local/share/quill/app/
  versions/0.1.0/           una carpeta por versión, con el bundle entero
    quill
    libcef.so  *.pak  icudtl.dat  …
    locales/
  versions/0.2.0/
  current -> versions/0.2.0
  downloads/                la descarga en curso o la lista para instalar
```

La primera vez, en una carpeta propia (el tar no trae carpeta y desparramaría
sus archivos donde se abra):

```sh
mkdir quill && tar -xzf quill-x86_64-linux.tar.gz -C quill && ./quill/quill --install
```

`quill --install` copia el programa como la versión que es y deja `current`
apuntando a ella. Si la carpeta del programa tiene `libcef.so`, copia también
sus otros archivos y `locales/` (no otras carpetas); si no (una compilación
propia), copia sólo el programa. Todavía **no** crea el lanzador ni la entrada
del menú (rune sí): hasta entonces, se arranca con
`~/.local/share/quill/app/current/quill`.

Cada actualización:

1. **preparar**: extrae el tar con el `tar` del sistema en
   `versions/.{versión}.{pid}` (conserva los permisos), comprueba que `quill`
   sea un programa ELF, sincroniza todo al disco y lo renombra a
   `versions/{versión}`; `staged.json` lo nombra;
2. **activar** (al arrancar o al reiniciar): crea un enlace `current.new` y lo
   renombra sobre `current`: un inicio encuentra la versión vieja o la nueva,
   nunca ninguna;
3. borra las versiones que no son la actual ni la anterior (la anterior es la
   que todavía corre, y a la que se vuelve atrás).

Un fallo en cualquier paso deja `current` como estaba.

**Copias instaladas hasta la 0.1.3.** Esas versiones esperan el programa solo
comprimido con gzip: bajan el `.tar.gz` (la firma y el SHA-256 coinciden), lo
descomprimen, ven que no empieza como un programa ELF y no instalan nada, con
el error en Ajustes. No se rompe nada, pero tampoco se actualizan: hay que
reinstalar una vez a mano con los pasos de arriba, y desde ahí siguen solas.

**Qué necesita en tiempo de ejecución.** Además de lo que trae el bundle: glibc, Wayland,
libxkbcommon, libgbm y la pila gráfica (Vulkan o GL). El paso `ldd` del workflow
deja la lista exacta en el log de cada release.

**NixOS**: un binario genérico probablemente no arranque (no hay
`/lib64/ld-linux-x86-64.so.2` ni bibliotecas en rutas estándar). Esto es
**inferido** de cómo rune lo documenta, no probado con quill: la salida es
compilar desde el código en `nix-shell` (y entonces esa copia no se actualiza
sola) o usar [`nix-ld`](https://github.com/nix-community/nix-ld) con las mismas
bibliotecas. Para los videos de las novedades hace falta además `libstdc++.so.6`
(`stdenv.cc.cc.lib`), que `shell.nix` ya suma (ver [Videos y el códec de
Cisco](#videos-y-el-códec-de-cisco)).

## Portabilidad y CI

`.github/workflows/check.yml` corre `cargo check --workspace --locked` en Windows y
macOS en cada push a `main` y en cada PR, y falla si no compila. `package.yml` arma
los tres bundles (ver [Publicar una release](#publicar-una-release)); Windows y
macOS entran en una release cuando el dueño enciende sus variables. El estado real
de cada sistema está en [portabilidad.md](portabilidad.md).

## Windows

**Artefacto**: `quill-x86_64-windows.zip`, con todo en su raíz: `quill.exe`,
los archivos de CEF (`libcef.dll`, `chrome_elf.dll`, `libEGL.dll`,
`libGLESv2.dll`, los `.pak`, `icudtl.dat`, `v8_context_snapshot.bin`…) y la
carpeta `locales\`. En el manifiesto es `windows-x86_64`. Compilado con MSVC y
`+crt-static`, así que no hace falta el Visual C++ Redistributable.

**Instalación por usuario**, sin permisos de administrador:

```text
%LOCALAPPDATA%\Programs\quill\
  quill.exe                 el programa, siempre en el mismo lugar
  quill.exe.old             el anterior, hasta la siguiente actualización
  quill.exe.0.3.0.new       una versión preparada, hasta activarla
  versions\0.2.0\           el resto del bundle de cada versión
    libcef.dll  *.pak  icudtl.dat  …
    locales\
```

`quill.exe` busca CEF primero a su lado y si no en `versions\{su versión}\`.
quill se considera instalado sólo si corre desde esa carpeta.

La primera vez: descomprimir el zip en una carpeta (cualquiera) y correr
`quill.exe --install` desde ahí. Copia `quill.exe` a la raíz y el resto (si
la carpeta tiene `libcef.dll`; si no, sólo el programa) a
`versions\{versión}\`.

**Cada actualización**:

1. (preparar) extrae el zip con el `tar.exe` de Windows
   (`%SystemRoot%\System32\tar.exe`, desde Windows 10 1803; sin ventana de
   consola) en `versions\.{versión}.{pid}`;
2. (preparar) saca de ahí `quill.exe` como `quill.exe.{versión}.new` al lado
   del que corre, comprueba que empiece por `MZ` y sincroniza todo al disco;
3. (preparar) renombra la carpeta a `versions\{versión}`;
4. (activar) renombra el `quill.exe` que corre a `quill.exe.old` (Windows
   permite renombrar un ejecutable en uso, no sobrescribirlo) y el nuevo toma
   su nombre: ése es el punto de no retorno, y quill arranca;
5. borra las versiones que no son la nueva ni la que todavía corre, y los
   `quill.exe.*.new` y `quill.exe.*.bad` que dejaron preparaciones y vueltas
   atrás. Una carpeta se renombra antes de borrarse: si un quill la está
   usando, Windows no deja y queda entera.

Volver atrás renombra el `quill.exe` a prueba a `quill.exe.{versión}.bad` y
`quill.exe.old` vuelve a ser `quill.exe`.

Un fallo antes del paso 4 deja la instalación anterior funcionando.

**Sin probar en una máquina Windows todavía**: el camino de reemplazo es el
único del updater que no corrió en ningún lado. Antes de publicar para Windows
hay que hacer una actualización real de punta a punta con `quill-test-feed
--platform windows-x86_64 --artifact quill-x86_64-windows.zip`.

**Sin firma de código (Authenticode).** El `.exe` no está firmado: SmartScreen
avisa en la primera descarga por navegador. Las actualizaciones no pasan por
ahí: su integridad la dan la firma Ed25519 del manifiesto y el SHA-256.

## macOS

**Artefacto**: `quill-aarch64-macos.zip`, hecho con
`ditto -c -k --keepParent quill.app`: en su raíz está `quill.app/`, firmado y
notarizado, con CEF dentro (el framework trae enlaces simbólicos que `ditto`
conserva). En el manifiesto es `macos-aarch64`; sólo Apple Silicon.

**Instalación**: es el bundle mismo. Si quill corre desde
`<carpeta>/quill.app/Contents/MacOS/quill`, la instalación es esa carpeta
(normalmente `/Applications` o `~/Applications`); si no, `~/Applications`. Un
bundle con otro nombre no cuenta como instalado. Si macOS lo corre desde una
copia de sólo lectura (*App Translocation*, al abrirlo donde se descomprimió
sin moverlo), tampoco.

La primera vez: descomprimir el zip y arrastrar `quill.app` a Aplicaciones.
No hace falta `--install`: un `quill.app` ya es una instalación. Si se corre
igual, desde un `quill.app` que no es el instalado copia ese bundle con `ditto`
a la instalación (`~/Applications/quill.app`, salvo `QUILL_UPDATE_ROOT`).

**Cada actualización**:

1. (preparar) extrae el zip con `ditto -x -k` en `.quill.app.{versión}.{pid}`,
   al lado del bundle;
2. (preparar) corre `codesign --verify --deep --strict` sobre el `quill.app`
   nuevo y lo rechaza si no verifica; comprueba que `Contents/MacOS/quill` sea
   un programa Mach-O (64 bits o universal), sincroniza todo al disco y lo
   mueve a `.quill.app.{versión}`;
3. (activar) borra el `.quill.app.old` de la actualización anterior, renombra
   el `quill.app` que corre a `.quill.app.old` y el preparado toma su nombre;
   borra otros `.quill.app.*` que quedaron (no `.old` ni `.update`).

Volver atrás renombra el `quill.app` a prueba a `.quill.app.{versión}.bad` y
`.quill.app.old` vuelve a ser `quill.app`.

Un fallo antes del último paso deja el bundle como estaba. La descarga espera
en `quill-updates` dentro de la carpeta temporal del usuario.

**Sin probar en un Mac**: sólo lo compila el CI (no hay SDK de Apple en la
máquina del dueño); usa sólo la biblioteca estándar y `ditto` y `codesign` del
sistema.

**El bundle** (`scripts/package.sh macos`): `Contents/MacOS/quill`, el
framework de CEF en `Contents/Frameworks/`, y a su lado las cinco apps
auxiliares que Chromium usa para sus procesos (`quill Helper.app`,
`quill Helper (GPU).app`, `(Renderer)`, `(Plugin)`, `(Alerts)`), cada una con
el programa `quill-helper`. quill carga el framework al abrir el navegador (no
está enlazado), y el bucle de CEF corre en el hilo principal, bombeado por la
ventana (en macOS CEF no tiene hilos propios para eso). Se firma de adentro
hacia afuera con *hardened runtime* (bibliotecas del framework, framework,
auxiliares, quill), se notariza con `notarytool` y se engrapa el ticket.

## Seguridad

- La firma se verifica antes de interpretar el JSON, con `verify_strict` y
  rechazando claves débiles; el artefacto se instala sólo si tamaño y SHA-256
  coinciden con lo firmado. Un artefacto sin HTTPS (salvo `http://127.0.0.1` de
  los feeds de prueba), sin digest válido o de más de 1 GiB no se ofrece.
- Nunca baja de versión: una release se ofrece sólo si es más nueva que la
  instalada. Además cada copia guarda la `sequence` más alta que vio y rechaza
  una menor, así que repetir un manifiesto viejo firmado no sirve ni para
  ofrecer una versión vieja pero más nueva que la instalada. La `sequence` la
  pone el CI: la del manifiesto publicado anterior más uno.
- Se rechaza un manifiesto de otro `product` o `channel`, o pasado su `expires`
  (si no se puede leer la fecha, también). El CI hoy no pone `expires`:
  `quill-release manifest --expires <RFC 3339>` lo agrega; si se usa, cada
  release tiene que salir antes de que venza la anterior, o las copias dejan de
  encontrar actualizaciones hasta la próxima.
- Dos claves compiladas (release y respaldo) y rotación con `next_keys` (ver
  [La clave](#la-clave)).
- La lista de archivos y cada blob se verifican contra lo firmado antes de
  usarse; los caminos no pueden salir del bundle.
- Lo que no cubre: quien tenga la clave privada puede publicar una versión (por
  eso nunca entra al repositorio ni a un log, el CI la usa sólo en el paso de
  firma, y sólo dentro del entorno `release`, con aprobación).
- `QUILL_UPDATE_KEY` sólo existe en compilaciones de depuración.

## Pendiente antes de la primera release

Estas cosas no están resueltas y bloquean o condicionan la primera release.

1. **CEF en el bundle.** Cada artefacto lleva CEF junto al programa (ver
   cada sistema); el programa lo busca a su lado (y en Windows en
   `versions\{versión}\`). Falta probar el arranque de un bundle real en una
   máquina sin CEF instalado, en cada sistema.
2. **CEF en el CI.** `cef-dll-sys` lo descarga durante el build en
   `$RUNNER_TEMP/cef` (`CEF_PATH`), que se guarda en la caché de Actions por
   versión; la versión es la que fija `Cargo.lock` y el archivo lo verifica
   `cef-dll-sys` contra su índice.
3. **Lanzador y menú** de `quill --install` (rune los crea; quill no).
4. **Windows** sin probar en una máquina y **sin firma Authenticode**;
   **macOS** sin probar en una máquina.
5. **Avisos de terceros** (`THIRD_PARTY_NOTICES.md` con cargo-about, que rune sí
   publica): quill todavía no los genera.
