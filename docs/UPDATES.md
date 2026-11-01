# Actualizaciones

quill se actualiza solo desde las releases de `Niiihuel/quill`, un repositorio
**privado**. Cada tag `v*` publica una release con un manifiesto firmado; una
versión es **obligatoria** si el mensaje de su tag anotado tiene la palabra
«mandatory», y entonces quill se actualiza antes de seguir. Si no, la
actualización es **opcional**: se descarga sola (si Ajustes lo permite) y se
instala cuando la persona lo pide.

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
  `macos-aarch64`), la URL, el tamaño y el SHA-256 del artefacto.
- `quill-update.json.sig`: la firma Ed25519 de esos bytes exactos, en base64.

`minimum_version` es la versión obligatoria más nueva que sea menor o igual a
la publicada: el CI la calcula leyendo los mensajes de **todos** los tags
anotados (un tag liviano no cuenta). Una copia instalada es obligada a
actualizarse si su versión es menor que `minimum_version` de la última
release; así una obligatoria vieja sigue mandando aunque después haya salido
una opcional.

### Una búsqueda

1. Se busca la última release (ver [Token](#token-y-el-repositorio-privado)) y
   se bajan `quill-update.json` y su firma, con tope de tamaño.
2. **La firma se verifica antes de leer el JSON**, con `verify_strict` y la
   clave compilada. Una firma que no verifica no instala nada.
3. Se comparan versiones con SemVer. Si hay una más nueva, el estado pasa a
   «versión disponible» con sus notas; si es obligatoria, o si Ajustes tiene
   **Actualizar solo** encendido, se descarga sin preguntar.
4. Una búsqueda empieza a los 20 s de abrir quill y se repite cada 6 horas;
   **Buscar actualizaciones** la hace en el momento.

Los estados que ve la persona (`quill_model::UpdateState`):

| Estado | Qué se muestra |
| --- | --- |
| Sin buscar / Buscando | «Buscando actualizaciones…» |
| Al día | «quill está al día · Revisado hace 5 min» |
| Disponible | «La versión 0.3.0 está disponible», con las notas |
| Descargando | progreso; si es obligatoria, una ventana sin cerrar |
| Lista | En la barra lateral, sobre el botón de la cuenta, una tira «Relaunch to update · v0.3.0» (con la barra oculta, un punto en el botón de mostrarla); en Ajustes, «Restart to update». Sin aviso emergente ni entrada en el menú de la cuenta |
| Error | el motivo, en una línea discreta, con **Reintentar** |
| Compilado desde el código | «Compilado desde el código: actualiza con git pull» |
| Sin instalar | «Ejecuta quill --install para mantenerlo actualizado» |
| Sin acceso | «Las actualizaciones necesitan acceso al repositorio privado…» |

Pulsar la tira (o el botón de Ajustes) reinicia al instante si ninguna sesión
trabaja. Si alguna está en marcha o esperando a la persona, abre «Agents are
still working» con **Cancel**, **Update anyway** y **Wait for agents**: esta
última deja la tira en «Restarting when agents finish» (con una x que lo
cancela) e instala la primera vez que no queda ninguna sesión ocupada. La
descarga y la búsqueda no muestran nada en la barra lateral: su progreso está
sólo en Ajustes › Sistema. Es el comportamiento de Claude Desktop
(la tira, el punto y el diálogo; no tiene aviso emergente ni entrada de menú);
las palabras del diálogo dicen «agents» porque quill no nombra a ningún
proveedor.

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
- **Instalar siempre espera a la persona** («Reiniciar para actualizar»). La
  instalación deja la versión nueva en su lugar y quill la arranca con los
  mismos argumentos; la ventana vieja se cierra después de que la nueva empezó
  (`QUILL_RESTART_AFTER`: la nueva espera a que la vieja salga, porque el socket
  de quill-ide sólo lo atiende una ventana a la vez).
- Los datos de la persona (sesiones, ajustes, `~/.config/quill`) no los toca la
  actualización.

## Token y el repositorio privado

Una búsqueda usa la API de GitHub (`releases/latest`) y baja los archivos por
su API, con `Authorization: Bearer`. El token sale, en orden, de:

1. la variable `QUILL_UPDATE_TOKEN`;
2. la salida de `gh auth token`, si `gh` está en el `PATH` (espera cinco
   segundos como mucho, fuera del hilo de la interfaz).

Sin token, o si la API responde 401, 403 o 404 con el token, el estado es
**Sin acceso** y el mensaje lo dice. Un repositorio público se leería sin
token: `PRIVATE = false` en `crates/quill-update/src/lib.rs` cambia la búsqueda
a `releases/latest/download` (sin API).

El token **no se escribe en logs ni en el estado** (`Source` no implementa
`Debug`), y solo viaja a `api.github.com`: las redirecciones se siguen a mano y
sin el token, porque la API manda la descarga a otro servidor. El token no se
guarda en disco.

Lo que no cubre: quien tenga el token puede leer el repositorio privado; quien
controle la red puede ocultar una versión nueva (la copia sigue como está, y una
obligatoria se descubre al volver la red).

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
   - compila el artefacto de Linux en Ubuntu 24.04 (`quill-x86_64-linux.tar.gz`),
     el de Windows (`quill-x86_64-windows.zip`) y el de macOS
     (`quill-aarch64-macos.zip`), cada uno con CEF adentro (ver cada sistema
     abajo), y los sube al borrador;
   - arma el manifiesto (`quill-release manifest`, con `minimum_version`), lo
     firma con el secreto (`quill-release sign`, con el programa ya compilado
     para que ningún build script vea el secreto), lo **verifica con la clave
     compilada en quill** (`quill-release verify`: un secreto que no coincide
     con `release-key.pub` corta acá), y recién entonces sube
     `quill-update.json` y `quill-update.json.sig` y publica la release.

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
  la raíz). Ese checkout es la rama `quill/integration` de `Niiihuel/limn`,
  que **todavía no está subida**: el dueño tiene que hacer push de esa rama
  antes del primer build de CI. El workflow la clona dentro del workspace y
  enlaza `../limn-quill-integration` hacia ella.
- **Secreto `LIMN_READ_TOKEN`**: un token de lectura de `Niiihuel/limn` (un
  token de grano fino con solo lectura de ese repositorio). Sin él, el checkout
  usa el `GITHUB_TOKEN` del repositorio de quill, que no alcanza a leer otro
  repositorio privado.
- **Secreto `QUILL_UPDATE_SIGNING_KEY`**: la clave privada (ver [La clave](#la-clave)).
- **Variable `QUILL_WINDOWS_RELEASES`**: en `true` para incluir el job de
  Windows; apagada, la release sale solo para Linux.

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

**Guardar una copia de la privada** fuera de la máquina. Las copias instaladas
sólo confían en la clave con la que se compilaron: perderla obliga a reinstalar a
mano con una clave nueva. Para cambiarla a propósito: vaciar `release-key.pub`,
generar otra, commitear y publicar; las copias viejas no aceptarán esa release y
hay que reinstalarlas.

## Probar con el feed local

El updater se prueba sin GitHub, contra un feed local. Todo se puede apuntar a
otro lado con variables de entorno:

| Variable | Qué hace | Dónde vale |
| --- | --- | --- |
| `QUILL_UPDATE_FEED` | URL base con `quill-update.json` y `.sig` (en vez de la API del repositorio); también hace que una copia sin instalar busque | siempre (igual se verifica con la clave compilada) |
| `QUILL_UPDATE_KEY` | clave pública en base64 en lugar de la compilada | sólo compilaciones de depuración |
| `QUILL_UPDATE_ROOT` | carpeta de la instalación en vez de `~/.local/share/quill/app` | siempre |
| `QUILL_UPDATE_TOKEN` | token de la API (ver [Token](#token-y-el-repositorio-privado)) | siempre |
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
comprimido y la instalación lo rechaza), `--platform windows-x86_64`.

`cargo test -p quill-update` cubre lo mismo sin ventana ni red: firmas,
manifiestos alterados, obligatoria contra opcional, descargas que no coinciden,
reanudación, la búsqueda por la API con un token que no alcanza, e instalación
atómica en Linux con un `.tar.gz` real (programa, `libcef.so` y `locales/`).
Lo de Windows y macOS sólo se compila (la comprobación de los primeros bytes
del programa y la ruta del bundle sí se prueban en cualquier sistema).

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

1. extrae el tar con el `tar` del sistema en `versions/.{versión}.{pid}`
   (conserva los permisos), comprueba que `quill` sea un programa ELF y
   sincroniza todo al disco;
2. lo renombra a `versions/{versión}`;
3. crea un enlace `current.new` y lo renombra sobre `current`: un inicio
   encuentra la versión vieja o la nueva, nunca ninguna;
4. borra las versiones que no son la actual ni la anterior (la anterior es la
   que todavía corre).

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
bibliotecas.

## Portabilidad y CI

`.github/workflows/check.yml` corre `cargo check --workspace --locked` en Windows y
macOS en cada push a `main` y en cada PR, con `continue-on-error` hasta que ambos
den verde una vez (después se saca). El job de release de Windows sigue apagado
(`QUILL_WINDOWS_RELEASES`) y no hay job de macOS: ambas decisiones quedan para el
dueño; el estado real de cada sistema está en [portabilidad.md](portabilidad.md).

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

1. extrae el zip con el `tar.exe` de Windows
   (`%SystemRoot%\System32\tar.exe`, desde Windows 10 1803; sin ventana de
   consola) en `versions\.{versión}.{pid}`;
2. saca de ahí `quill.exe` como `quill.exe.{versión}.new` al lado del que
   corre, comprueba que empiece por `MZ` y sincroniza todo al disco;
3. renombra la carpeta a `versions\{versión}`;
4. renombra el `quill.exe` que corre a `quill.exe.old` (Windows permite
   renombrar un ejecutable en uso, no sobrescribirlo) y el nuevo toma su
   nombre: ése es el punto de no retorno, y quill arranca;
5. borra las versiones que no son la nueva ni la que todavía corre. Una
   carpeta se renombra antes de borrarse: si un quill la está usando, Windows
   no deja y queda entera.

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

1. extrae el zip con `ditto -x -k` en `.quill.app.{versión}.{pid}`, al lado
   del bundle;
2. corre `codesign --verify --deep --strict` sobre el `quill.app` nuevo y lo
   rechaza si no verifica; comprueba que `Contents/MacOS/quill` sea un
   programa Mach-O (64 bits o universal) y sincroniza todo al disco;
3. borra el `.quill.app.old` de la actualización anterior, renombra el
   `quill.app` que corre a `.quill.app.old` y el nuevo toma su nombre.

Un fallo antes del último paso deja el bundle como estaba. La descarga espera
en `quill-updates` dentro de la carpeta temporal del usuario.

**Sin probar en un Mac**: este camino ni siquiera se compiló (no hay SDK de
Apple en la máquina del dueño); usa sólo la biblioteca estándar y `ditto` y
`codesign` del sistema.

## Seguridad

- La firma se verifica antes de interpretar el JSON, con `verify_strict` y
  rechazando claves débiles; el artefacto se instala sólo si tamaño y SHA-256
  coinciden con lo firmado. Un artefacto sin HTTPS (salvo `http://127.0.0.1` de
  los feeds de prueba), sin digest válido o de más de 1 GiB no se ofrece.
- Nunca baja de versión: una release se ofrece sólo si es más nueva que la
  instalada, así que repetir un manifiesto viejo firmado no sirve.
- Lo que no cubre: quien tenga la clave privada puede publicar una versión (por
  eso nunca entra al repositorio ni a un log, y el CI la usa sólo en el paso de
  firma).
- `QUILL_UPDATE_KEY` sólo existe en compilaciones de depuración.

## Pendiente antes de la primera release

Estas cosas no están resueltas y bloquean o condicionan la primera release.

1. **CEF en el bundle.** Cada artefacto lleva CEF junto al programa (ver
   cada sistema); el programa lo busca a su lado (y en Windows en
   `versions\{versión}\`). Falta probar el arranque de un bundle real en una
   máquina sin CEF instalado.
2. **CEF en el CI.** `cef-dll-sys` descarga CEF durante el build si no hay
   `CEF_PATH`. El workflow no fija la versión ni la verifica: confirmar que la
   descarga funciona en `ubuntu-24.04` y en `windows-latest`.
3. **limn `quill/integration`** sin subir (ver arriba).
4. **Lanzador y menú** de `quill --install` (rune los crea; quill no).
5. **Windows** sin probar (ver arriba) y sin firma; **macOS** sin compilar ni
   probar.
6. **Avisos de terceros** (`THIRD_PARTY_NOTICES.md` con cargo-about, que rune sí
   publica): quill todavía no los genera.
