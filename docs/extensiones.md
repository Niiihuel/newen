# Extensiones por agente y capa global (`Customize`)

Diseño para que Skills, Plugins y Connectors (servidores MCP) de Newen muestren lo de cada CLI por separado, más una capa **Global (all agents)** que Newen guarda en su propia configuración y aplica a todos los agentes. Es solo diseño: no se tocó código ni la configuración de ningún CLI.

**Fuentes y marcas.**
- Newen: rama `feat/codex` de `~/projects/personal/quill-codex`.
- Claude Code: `claude --help` y `claude mcp|plugin … --help` de la versión 2.1.312, y las claves (sin valores) de `~/.claude.json` y `~/.claude/settings.json`.
- Codex: `codex … --help` y `codex mcp list --json` (sin valores) de `codex-cli 0.163.0`, el esquema `app-server` v2 (`scratchpad/codex/schema/v2/`) y el código de `openai/codex@main` leído con `gh api` el 2026-10-28.
- Gemini CLI y OpenCode: su documentación y código en `google-gemini/gemini-cli@main` y `anomalyco/opencode@dev`, leídos con `gh api`. No se instalaron ni se probaron, así que todo lo suyo es **(doc)**, y lo que ni siquiera está escrito ahí es **(inferido)**.
- Claude Desktop: lo que la persona ve en sus páginas de Skills, Plugins y Connectors.

---

## 1. Newen hoy

### 1.1 Contrato (`crates/newen-agent/src/lib.rs`)

| Pieza | Qué es | Claude | Codex |
|---|---|---|---|
| `servers_query` / `servers` / `server_scopes` | Lista de servidores configurados, leída de un programa que corre una vez | `claude mcp list` (texto que se parsea, sin JSON), con los alcances local, project y user | `None`: Connectors dice "No connectors yet", aunque `codex mcp list --json` lista 4 servidores |
| `plugins_query` / `plugins`, `marketplaces_query` | Plugins instalados y ofrecidos, y marketplaces | `plugin list --json --available`, `plugin marketplace list --json` | `None` |
| `extension_words` | Alcances de instalación y estados de un skill | user, project, local / on, name-only, user-invocable-only, off | `(vec![], vec![])` |
| `extension_op(ExtensionOp)` → `ProgramRun` | Cambios: SwitchPlugin, Install, Uninstall, Update, marketplaces, SkillState, AddServer, RemoveServer | Todos (`claude plugin …`, `claude mcp add/remove`, `edit-skill-overrides --json`) | `None` |
| `AgentCommand::McpStatus/McpReconnect/McpToggle/McpAuthenticate/McpClearAuth/Skills/ReloadPlugins/ReloadSkills` | Control de la sesión en curso | Pedidos de control: `mcp_status`, `mcp_toggle`, `mcp_authenticate`, `get_skills_dialog`, `reload_*` | `_ => vec![]` (codex lib.rs:892) |
| `AgentCapabilities::mcp` | La sesión informa y controla sus servidores MCP | sí | no |
| `McpSource` | User, Project, Local, Managed, Plugin, Account, App, Other | Sale del nombre: `plugin:` es Plugin, `claude.ai ` es Account, `sdk` es App, y todo lo demás es Other | — |

### 1.2 Motor (`crates/newen-engine/src/lib.rs`)

- `server_op` (lib.rs ~655): corre `extension_op` y después `servers_query`, y contesta `Event::Servers{agent, folder, servers, scopes, error}`.
- `extensions` (~698): corre la operación, después `plugins_query`, `marketplaces_query` y `skills_of`, y contesta `Event::Extensions`. Si el cambio salió bien, las sesiones del mismo agente reciben `ReloadPlugins` o `ReloadSkills`.
- `skills_of` (~3070): arranca el CLI como para una sesión, le manda `Start` y `Skills` y lee hasta que llega `AgentEvent::Skills`. Es un **runner de sesión de un solo uso** que ya sirve para preguntarle al CLI por su propio protocolo.
- `registry.rs`: Discover consulta el registro público de MCP, que no depende de ningún agente.
- `servers.rs` no tiene que ver con MCP: son los dev servers de `preview_start`.

### 1.3 UI (`crates/newen-ui/src/view.rs` 9513–10500, `lib.rs` 1983–2020 y 3937–4330)

- `customize_pages`: Skills, Plugins y Connectors en la columna de Settings. Cada página recorre `ui.agents` y arma secciones por agente. Los Skills se titulan "{agent} › {source}" cuando hay más de un agente.
- **El botón Add del header solo actúa para el primer agente** (`connectors_page`: `ui.agents.first()`, y en Plugins el primero que ya tiene extensiones), como dice el atajo de view.rs:9541.
- **Problema probable** (visto leyendo el código, no ejecutado): `skills_page` descarta a un agente sin alcances (`.filter(|k| !k.list.scopes.is_empty())`), y Codex devuelve alcances vacíos. Con Codex, "Looking for skills…" quedaría girando para siempre.
- `/mcp` (ui.md §2.11b) muestra los servidores de la sesión en curso. Lo ofrece solo un agente con `capabilities.mcp`.

### 1.4 Lo que Newen guarda hoy

Nada sobre extensiones: todo se lee en vivo de los CLIs. `settings.json` (`config_dir()/newen/settings.json`, engine lib.rs:115) guarda `agents.<id>.{program, model, effort, mode, allow_unsafe, options, env}`. Los logos se piden con las claves `mcp:<label>` y `plugin:<name>`. **No hay datos que migrar**: ver §6.

---

## 2. Matriz de capacidades

Leyenda: ✔ verificado (help del CLI o esquema o código), **(doc)** solo leído en la documentación o el código de main, **(inferido)** sin fuente directa.

### 2.1 Servidores MCP

| | Claude Code 2.1.312 | Codex 0.163.0 | Gemini CLI (doc) | OpenCode (doc) |
|---|---|---|---|---|
| **Dónde se guardan** | user: `~/.claude.json` › `mcpServers`; local: `~/.claude.json` › `projects[<ruta>].mcpServers`; project: `.mcp.json`, aprobado o no con `enabledMcpjsonServers` y `disabledMcpjsonServers` (claves vistas en `~/.claude.json`) | user: `~/.codex/config.toml` › `[mcp_servers.<n>]`; project: `.codex/config.toml` (capa `project`, si el proyecto es de confianza); los plugins también aportan servidores (`pluginId`). Esquema ✔ `codex-rs/config/src/mcp_types.rs`: transporte `Stdio{command,args,env,env_vars,cwd}` o `StreamableHttp{url,bearer_token_env_var,http_headers,env_http_headers}`, y además `enabled`, `required`, `startup_timeout_sec`, `tool_timeout_sec` | `mcpServers` en `~/.gemini/settings.json` y `.gemini/settings.json`, en `/etc/gemini-cli/` y en las extensiones | `mcp` en `~/.config/opencode/opencode.json` o en el `opencode.json` del proyecto, más la configuración remota o gestionada |
| **Transportes** | stdio, sse, http (`-t`) ✔, ws (`add-json`) ✔ | stdio y streamable HTTP. **No tiene SSE** ✔ | stdio, sse (`url`), http (`httpUrl`) | `local` y `remote` |
| **Listar** | `claude mcp list` (texto; revisa la salud de cada servidor, y es lento) ✔; `claude mcp get <n>` ✔ | `codex mcp list --json` ✔, que devuelve `name, enabled, disabled_reason, transport{…}, auth_status` sin decir de dónde viene cada uno; `codex mcp get <n> --json` ✔; en la sesión, `mcpServerStatus/list{threadId?}` ✔ con `pluginId`, `authStatus`, `tools` | `gemini mcp list` | `opencode mcp list` |
| **Agregar** | `claude mcp add [-t] [-s local\|user\|project] [-e K=V] [-H] name target [-- args]` ✔; `add-json` ✔ | `codex mcp add <n> (--url U \| -- cmd…) [--env K=V] [--bearer-token-env-var V]` ✔, **solo en user**; `config/value/write{keyPath, value, filePath?}` ✔ puede escribir en otro archivo | `gemini mcp add [-s user\|project]` (por defecto project) | `opencode mcp add` (interactivo) o el `POST /mcp` de `opencode serve` |
| **Quitar** | `claude mcp remove <n> [-s]` ✔ | `codex mcp remove <n>` ✔ | `gemini mcp remove [-s]` | No hay comando: se edita la configuración o se usa la API |
| **Apagar sin quitar** | Para la sesión: el pedido de control `mcp_toggle` ✔ (Newen). Fijo: `disabledMcpjsonServers` (solo `.mcp.json`) | Fijo: `enabled = false` en el TOML. Para el proceso: `-c mcp_servers.<n>.enabled=false` (inferido de `-c`). **No hay apagado de sesión** en el esquema | `gemini mcp enable\|disable [--session]` | `"enabled": false`, y `connect`/`disconnect` en la API |
| **Autenticar (OAuth)** | En la sesión: `mcp_authenticate` y `mcp_clear_auth` ✔; desde la terminal: `claude mcp login\|logout <n>` ✔ | `codex mcp login\|logout <n>` ✔; `mcpServer/oauth/login{name, threadId?}` → `authorizationUrl`, y después `mcpServer/oauthLogin/completed` ✔ | `/mcp auth` (solo dentro de la sesión) | `opencode mcp auth\|logout` |
| **Alcances** | local, project, user, y además managed, plugin y la cuenta (claude.ai) | user y project (este solo editando el archivo), system o gestionado, y plugin | user, project, system y extensión | global, project, remoto y gestionado |
| **Override por sesión** | `--mcp-config <json\|archivo>…` ✔, que **suma** servidores; `--strict-mcp-config` ✔ (descarta los demás, Newen no lo usa) | `-c mcp_servers.<n>.<k>=<toml>` ✔ (capa `sessionFlags`), y `thread/start.config` (objeto libre ✔, que se carga como `request_overrides` en la misma capa, ✔ `app-server/src/config_manager.rs:484`; con `mcp_servers` es **inferido**, Newen ya lo hace con `tools.update_plan`) | `--allowed-mcp-server-names` (solo filtra); ACP `session/new{mcpServers}` (inferido) | `OPENCODE_CONFIG_CONTENT` o `OPENCODE_CONFIG` |
| **Recargar** | `reload_plugins` | `config/mcpServer/reload` ✔ | `/mcp reload` | (inferido) |

### 2.2 Skills

| | Claude Code | Codex | Gemini (doc) | OpenCode (doc) |
|---|---|---|---|---|
| **Formato** | carpeta con `SKILL.md` y frontmatter YAML `name`, `description` (más claves propias de Claude) | **el mismo**: `SKILL.md` con `name`, `description` y `metadata.short-description` ✔ (`ext/skills/src/loader`), más un `agents/openai.yaml` opcional ✔. Nombre de hasta 64 caracteres ✔. El mismo skill (`graphify`) está instalado en los dos con el mismo frontmatter | SKILL.md (agentskills.io) | SKILL.md; el `name` debe cumplir `^[a-z0-9]+(-[a-z0-9]+)*$` y ser igual al nombre de la carpeta |
| **Dónde** | `~/.claude/skills`, `.claude/skills` del proyecto, y los skills de plugins (`plugin:skill`) | `$CODEX_HOME/skills` (obsoleto), `~/.agents/skills`, `.codex/skills` del proyecto, `.agents/skills` en cada carpeta desde la raíz hasta el cwd, `/etc/codex/skills` (admin) y el caché del sistema ✔ (`ext/skills/src/host_roots.rs`) | `~/.gemini/skills`, `~/.agents/skills`, `.gemini/skills`, `.agents/skills` | `.opencode/skills`, `~/.config/opencode/skills`, **`~/.claude/skills`**, `.claude/skills`, `~/.agents/skills` |
| **Listar** | `get_skills_dialog` en la sesión ✔ | `skills/list{cwds, forceReload}` ✔, con `scope ∈ user\|repo\|system\|admin`, `enabled` y `pluginId` | `gemini skills list` | — |
| **Estado** | `edit-skill-overrides --json` ✔ (`skillOverrides` en los ajustes locales), con 4 estados | `skills/config/write{path\|name, enabled}` ✔: solo on u off | `gemini skills enable\|disable` | `permission.skill` |
| **Override por sesión** | `--plugin-dir <dir>` ✔ (un plugin con `skills/`, que quedan con el prefijo del plugin, inferido); `--add-dir` (inferido: no está claro si carga los `.claude/skills` de esa carpeta) | `skills/extraRoots/set{extraRoots}` ✔, que vale para el proceso app-server (Newen usa uno por sesión) | extensión (inferido) | `OPENCODE_CONFIG_CONTENT` (inferido) |

**¿Son compatibles?** El núcleo sí: carpeta, `SKILL.md`, `name` y `description`. Las claves que solo entiende Claude (por ejemplo `allowed-tools`, `model`) Codex las ignora sin fallar (inferido: su parser solo pide `name` y `description`). OpenCode es más estricto con el nombre. `~/.agents/skills` lo leen Codex, Gemini y OpenCode, pero **Claude no**.

### 2.3 Plugins y marketplaces

| | Claude Code | Codex | Gemini (doc) | OpenCode (doc) |
|---|---|---|---|---|
| **Plugin** | `.claude-plugin/plugin.json` con skills, commands, agents, hooks y `.mcp.json`. Se guarda en `~/.claude/plugins/` (`installed_plugins.json`) y se activa con `enabledPlugins` en settings.json | `.codex-plugin/plugin.json`, y **también lee `.claude-plugin/plugin.json` y `.cursor-plugin/plugin.json`** ✔ (`exec-server-protocol/src/protocol.rs:55`). El caché local ya tiene `claude-plugins-official/vercel` instalado en Codex. Se activa con `[plugins."id@mkt"] enabled` en config.toml | extensión `gemini-extension.json` en `~/.gemini/extensions` | módulo JS/TS en `.opencode/plugins` o paquete npm (`"plugin": [...]`) |
| **Comandos** | `claude plugin install\|uninstall\|enable\|disable\|update\|list --json` ✔ | `codex plugin add\|remove\|list [--json --available]` ✔; `plugin/list\|install\|uninstall\|read\|installed` ✔ (sin enable/disable por CLI, inferido; se cambia `enabled` con `config/value/write`) | `gemini extensions install\|uninstall\|enable\|disable` | ninguno |
| **Marketplace** | `claude plugin marketplace add\|list\|update\|remove` ✔ (`known_marketplaces.json`) | `codex plugin marketplace add\|list\|upgrade\|remove` ✔; `marketplace/add{source, refName?, sparsePaths?}` ✔. **Lee `.claude-plugin/marketplace.json`** además de `.agents/plugins/marketplace.json` ✔ (`core-plugins/src/marketplace.rs:21`) | galería en geminicli.com (tópico de GitHub) | no tiene |
| **Override por sesión** | `--plugin-dir`, `--plugin-url` ✔; `--settings '{"enabledPlugins":…}'` (inferido) | `-c plugins."id".enabled=false` (inferido) | `-e/--extensions` | `--pure` |

**Conclusión:** un mismo marketplace de Claude se puede agregar a Codex (verificado en el código), y los plugins con skills y `.mcp.json` andan en los dos. Los hooks y commands de Claude, no (inferido). Los plugins **no son portables en general**, así que en la capa Global no hay plugins (§3.3).

### 2.4 Conectores de cuenta

| | Claude Code | Codex | Gemini / OpenCode |
|---|---|---|---|
| Qué son | Los conectores de claude.ai, que aparecen como `claude.ai <Nombre>` en `mcp list` (`McpSource::Account`) | Las "apps" de la cuenta de ChatGPT: `app/list\|read\|installed` ✔ en el esquema (no capturado) | No existen (doc) |
| Se administran en | claude.ai | chatgpt.com (inferido) | — |

En Newen solo se **leen**. La capa Global no los incluye, porque pertenecen a la cuenta de cada proveedor.

---

## 3. Propuesta de UX para Customize

### 3.1 Principios

1. **Tres páginas, como hoy y como Desktop:** Skills, Plugins y Connectors (ui.md §2.11, captura). No se agregan páginas por agente: la columna de Settings sigue corta.
2. **Un selector de agente en el header:** el chip `[◉ All agents ▾]`, al lado del de la carpeta, con el mismo estilo de `customize::folder_chip`. Sus entradas son "All agents", "Global only" y una por agente con el logo que trae su adaptador (`AgentAdapter::icon`).
3. **Secciones por agente:** con "All agents", la página muestra primero **Global · all agents** y después una sección por agente ("Claude Code", "Codex"), con su logo de 16 px en el título. Dentro de cada sección siguen los grupos por origen que ya existen ("User", "Project", "from a plugin").
4. **Con un agente elegido**, la página muestra lo que *ese* agente va a usar: lo suyo más lo Global que le aplica, marcado con la pastilla "Global". Es la vista "efectiva".
5. **Desktop no tiene varios agentes,** así que el selector y las marcas por agente son diseño de Newen (inferido). Lo demás sigue las medidas de ui.md §2.11: filas de 63 px, recuadro de 36, menú "⋯" y secciones de 16 px.

### 3.2 Connectors, con "All agents"

```text
┌──────────────────────────────────────────────────────────────────────────────────────┐
│ Connectors   [Yours] Discover          [Search…]  [◉ All agents ▾]  [▣ Newen ▾]  [+ Add ▾] │
│                                                                                      │
│ Global · all agents  (2)                                              [Manage…]      │
│ ┌──┐ github                                                         ◐C ◐X    ⋯      │
│ │▣ │ from Newen · npx -y @modelcontextprotocol/server-github                          │
│ └──┘─────────────────────────────────────────────────────────────────────────────────│
│ ┌──┐ linear                                                         ◐C ⚠X    ⋯      │
│ │▣ │ from Newen · https://mcp.linear.app/mcp · Codex uses its own "linear"            │
│ └──┘─────────────────────────────────────────────────────────────────────────────────│
│ ┌──┐ legacy-sse                                                     ◐C ⊘X    ⋯      │
│ │▣ │ from Newen · https://old.example/sse · Not available in Codex (SSE)              │
│ └──┘─────────────────────────────────────────────────────────────────────────────────│
│                                                                                      │
│ (C) Claude Code  (4)                                                                 │
│ ┌──┐ sentry                       Connected · All my projects · https://mcp.sentry…  ⋯│
│ ┌──┐ figma                        Needs authentication · from your account            │
│ ┌──┐ vercel                       Connected · from a plugin                           │
│                                                                                      │
│ (X) Codex  (5)                                                                       │
│ ┌──┐ linear     [Overrides Global]   On · All my projects · https://mcp.linear.app…  ⋯│
│ ┌──┐ node_repl                    On · from a plugin · node …                         │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

- **Marcas por agente** (componente de Newen `customize::agent_marks`; ícono de 14 px por agente al final de la fila):
  - lleno: se aplica en ese agente;
  - atenuado: la persona lo sacó de ese agente;
  - ⚠: ese agente tiene uno propio con el mismo nombre, y gana el suyo;
  - ⊘: el agente no lo puede usar.

  Cada marca tiene un tooltip que dice el motivo con las palabras del adaptador.
- **El "⋯" de una fila Global:**
  - "Edit…";
  - "Use in ▸", con un checkbox por agente;
  - "Copy into Claude Code's config…", que es el `AddServer` de hoy y hace una copia, no un vínculo;
  - "Remove", en rojo y con confirmación.
- **El "⋯" de una fila de un agente:** el de hoy (Remove), más "Make global…", que crea la entrada en Global con la definición que da `mcp get` (§6) y pregunta si se la quita del agente.
- **"+ Add ▾":** "Add custom connector…" abre el diálogo de §3.5. Ya no actúa solo para el primer agente.

### 3.3 Skills y Plugins

```text
│ Skills          [Search…]  [◉ All agents ▾]  [▣ Newen ▾]  [+ Add ▾]                    │
│ Global · all agents  (1)                                                             │
│ ┌──┐ release-notes                                               ◐C ◐X ◐G ◐O   ⋯    │
│ │▤ │ from Newen · Writes release notes from the git log                               │
│ (C) Claude Code › User  (3)        …las filas de hoy, con su estado al final           │
│ (C) Claude Code › Plugin  (12)                                                        │
│ (X) Codex › User  (9)              On / Off                                          │
│ (X) Codex › System  (4)            [locked: "Built into Codex"]                       │
```

- **Global de skills:** son las carpetas `SKILL.md` que Newen guarda (§4.1). "+ Add ▾" ofrece "New skill…", "Import folder…" y "Make global" desde la fila de un agente, que copia la carpeta.
- **Avisos por agente** en la marca: "Name must be lowercase-with-dashes for OpenCode" o "Uses `allowed-tools`, which Codex ignores". Son avisos, no bloquean: el skill se aplica igual. Lo que lo hace inusable (un nombre que el agente rechaza) sale como ⊘.
- **Plugins no tiene sección Global** (§2.3). En cambio:
  - el "+" de una tarjeta de Discover abre "Install for ▸", con un submenú por agente y sus alcances;
  - "Add marketplace" pregunta para qué agentes (`[x] Claude Code [x] Codex`) y corre el `AddMarketplace` de cada uno;
  - si un agente no acepta el formato, su checkbox aparece desactivado y explica por qué.

### 3.4 Con un agente elegido (vista efectiva)

```text
│ Connectors   [Yours] Discover          [(X) Codex ▾]  [+ Add ▾]                          │
│ Codex will use  (6)                                                                  │
│ ┌──┐ github      [Global]           from Newen · npx -y …                         ⋯  │
│ ┌──┐ linear                         On · All my projects ·                         ⋯  │
│ │  │             Global "linear" is not used: Codex has its own                       │
│ ┌──┐ node_repl                      On · from a plugin                                │
│ Not available in Codex  (1)                                                          │
│ ┌──┐ legacy-sse  [Global]           SSE servers aren't supported by Codex             │
```

### 3.5 Diálogo "Add custom connector"

```text
┌ Add custom connector ─────────────────────────────────────┐
│ Name        [github                      ]                 │  [a-z0-9_-], válido en todos
│ Runs        (•) Command   ( ) Address                      │
│ Command     [npx -y @modelcontextprotocol/server-github]   │
│ Variables   GITHUB_TOKEN = [••••••••••]  [🔒 Secret ✓]     │
│ Where       [All agents (Newen)                       ▾]   │
│                All agents (Newen)                          │
│                ── Claude Code ──                           │
│                All my projects                             │
│                This project, shared                        │
│                This project, for me                        │
│                ── Codex ──                                 │
│                All my projects                             │
│ Use in      [x] Claude Code  [x] Codex   (solo con All agents)
│                                          [Cancel]  [Add]   │
└────────────────────────────────────────────────────────────┘
```

- "Where" es la lista de hoy (`server_scopes`), agrupada por agente, con "All agents (Newen)" primero. Si `DropdownMenu` de limn no tiene títulos de grupo, se pide un PR a limn (AGENTS.md).
- Con "All agents", "Address" ofrece http y sse. Un agente sin sse sale desmarcado y con su motivo.
- Una variable o un header marcados "Secret" no se escriben en ningún archivo (§4.3).

### 3.6 Diálogo `/mcp` de la sesión

La columna Type suma "Global", como pastilla y con el logo de Newen no, con la palabra. El motor la pone cuando el nombre del servidor está entre los que inyectó (§5.2). El resto no cambia.

---

## 4. Modelo de la configuración global

### 4.1 Dónde vive

- **Archivo propio:** `config_dir()/newen/extensions.json`, al lado de `settings.json`. Lo escribe solo el motor (`newen-engine`, fuera del hilo de la UI), y guarda antes de avisar (AGENTS.md).
- **Skills globales:** `config_dir()/newen/skills/<name>/SKILL.md`. Así un repositorio de dotfiles se los lleva con el resto de la configuración.

```json
{
  "version": 1,
  "servers": {
    "github": {
      "transport": "stdio",
      "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": { "secret": "mcp/github/GITHUB_TOKEN" }, "LOG": "warn" },
      "enabled": true,
      "except": ["codex"]
    },
    "linear": { "transport": "http", "url": "https://mcp.linear.app/mcp",
                "headers": { "Authorization": { "secret": "mcp/linear/Authorization" } } }
  },
  "skills": { "except": { "release-notes": ["opencode"] } }
}
```

- `except` es una lista negra: **un agente nuevo recibe todo** lo que es compatible con él (P6).
- Un valor es texto plano, o `{ "secret": "<clave>" }` en el llavero, o `{ "env": "NAME" }`, que toma la variable del entorno de Newen.

### 4.2 Cómo se aplica: inyección por sesión (recomendada)

| Agente | Servidores | Skills |
|---|---|---|
| Claude | `--mcp-config '<json>'` ✔, sin `--strict-mcp-config`, así que suma a los de la persona. Los secretos van como `${NEWEN_MCP_…}` (inferido: Claude expande `${VAR}` en `.mcp.json`; hay que verificar que haga lo mismo en `--mcp-config`, V1) | `--plugin-dir <config>/newen/claude-plugin` ✔: un plugin que Newen genera con `.claude-plugin/plugin.json {"name":"newen"}` y `skills/` apuntando a la carpeta global. Los skills quedan como `newen:<name>` (inferido, V4) |
| Codex | `thread/start.config.mcp_servers.<n>` (inferido, V2), que viaja por stdin y no por argv. Los secretos se pasan con `env_vars`, `env_http_headers` y `bearer_token_env_var` ✔, que solo nombran variables | `skills/extraRoots/set{extraRoots:[<config>/newen/skills]}` ✔, después de `initialize` |
| Gemini | ACP `session/new{mcpServers}` (inferido) | una extensión que Newen genere, o `~/.agents/skills` (inferido) |
| OpenCode | `OPENCODE_CONFIG_CONTENT='{"mcp":…}'` (doc) | `OPENCODE_CONFIG_CONTENT` (inferido) |

**Inyectar o escribir en los archivos de cada CLI:**

| | Inyección por sesión | Escribir con `claude mcp add -s user` o `codex mcp add` |
|---|---|---|
| Los archivos de la persona | No se tocan | Se modifican, y hay que saber cuáles creó Newen y cuáles no |
| Fuente de verdad | Una: `extensions.json` | Dos, que se desincronizan si la persona edita a mano |
| Secretos | En el llavero; solo llegan al proceso | En texto plano dentro de `~/.claude.json` o `config.toml` |
| Fuera de Newen (la terminal) | No se aplican | Se aplican |
| Estado y OAuth | Los da la sesión (`/mcp`). `mcp list` desde la terminal no los ve | Los ve el CLI como cualquier otro |
| Quitar | Se borra la entrada y listo | Hay que adivinar en qué alcance quedó |

**Recomendación:** inyección por sesión: se arma en cada sesión la unión de sus servidores y se entrega al agente sin escribir en `~/.claude.json`. Para quien quiera un servidor también en la terminal está "Copy into <agent>'s config…", que es una copia explícita y una sola vez, con el `AddServer` de hoy (§3.2).

### 4.3 Secretos

- Se guardan en el llavero del sistema: el crate `keyring`, servicio `newen`, cuenta `mcp/<server>/<KEY>`. engine.md §7 ya preveía esto, con la política de no seguir si no hay Secret Service. Agregar `keyring` necesita su comentario en `Cargo.toml` (AGENTS.md).
- **Al arrancar una sesión**, el motor resuelve cada secreto y lo pone en el **entorno del proceso** del agente, como `NEWEN_MCP_<SERVER>_<KEY>`. La configuración inyectada solo lleva el *nombre* de la variable. Ningún secreto queda en argv (`/proc/<pid>/cmdline` lo puede leer cualquier usuario), en un archivo ni en el log.
- **Límite conocido:** las herramientas de shell del agente heredan su entorno, así que el modelo podría leer la variable. Codex filtra por defecto las variables con `KEY`, `SECRET` o `TOKEN` en el nombre al pasárselas al shell (inferido de `shell_environment_policy`), y por eso el nombre termina en `_TOKEN`. De Claude no se sabe que filtre nada (inferido). Si esto no alcanza: un archivo `0600` en `$XDG_RUNTIME_DIR/newen/<session>.mcp.json`, que se borra al terminar (P3).
- **Sin llavero:** solo se aceptan `{ "env": "NAME" }` y texto que no es secreto, y el diálogo lo dice.
- **OAuth:** Newen no guarda tokens (engine.md §2, auth delegada). Cada CLI guarda la sesión de cada servidor inyectado, así que un servidor Global con OAuth se conecta una vez *por agente*. La fila Global lo muestra por agente: "Claude Code: signed in · Codex: Connect".

### 4.4 Precedencia

De mayor a menor:

1. **Lo gestionado** (managed o enterprise del CLI), que el CLI impone y Newen no puede cambiar.
2. **El interruptor de la sesión** (`/mcp`, `mcp_toggle`), que dura solo esa sesión.
3. **El proyecto según el CLI** (`.mcp.json`, `projects[...]`, `.codex/config.toml`, `.claude/skills`, `.agents/skills`).
4. **La persona según el CLI** (user o local).
5. **Global de Newen.**

**Regla de nombres:** si el agente ya tiene un servidor (o un skill) con el mismo nombre, **gana el del agente y Newen no inyecta el Global en ese agente**. La fila muestra ⚠, salvo que la definición sea la misma, y entonces dice "Same as Global" sin aviso. Esto hace falta porque Codex pone `sessionFlags` por encima de todo: si Newen no lo filtrara, el Global taparía el servidor de la persona. El motor decide con la última lista que leyó de ese agente (`Event::Servers` y `Event::Extensions`, que quedan guardadas en el motor). Si todavía no tiene ninguna, inyecta igual y deja la decisión al CLI (V3).

---

## 5. Cambios

### 5.1 Contrato (`newen-agent`, sin nombres de proveedor)

```rust
pub enum Transport { Stdio, Http, Sse }

pub struct NewServer {            // ya existe; `remote: bool` pasa a `transport`
    pub name: String, pub target: String, pub args: Vec<String>,
    pub transport: Transport, pub scope: Option<String>,
    pub env: Vec<String>, pub headers: Vec<String>,
    /// Variables y encabezados cuyo valor está en una variable del proceso del agente
    /// (un secreto): (clave del servidor, variable del proceso).
    pub env_from: Vec<(String, String)>,
    pub headers_from: Vec<(String, String)>,
}

pub struct SessionSpec { /* … */
    /// Los servidores Global que la sesión agrega, ya filtrados por el motor.
    pub servers: Vec<NewServer>,
    /// Carpetas de skills que la sesión agrega.
    pub skill_roots: Vec<PathBuf>,
}

/// Lo que el adaptador puede sumar a una sesión sin tocar los archivos del agente.
pub struct SessionExtras { pub transports: Vec<Transport>, pub skills: bool }

trait AgentAdapter {
    fn session_extras(&self) -> SessionExtras { SessionExtras::default() } // nada
    /// Por qué el agente no puede usar este servidor o skill; `None` si puede.
    fn refuses_server(&self, _s: &NewServer) -> Option<String> { None }
    fn refuses_skill(&self, _name: &str, _frontmatter: &serde_json::Value) -> Option<String> { None }
}

pub enum AgentCommand { /* … */ Extension(ExtensionOp) } // un cambio que va por el protocolo del agente
pub enum McpSource { /* … */ Global }                     // lo pone el motor, no el adaptador
pub struct AgentCapabilities { /* … */ pub mcp_toggle: bool } // Codex informa pero no apaga por sesión
```

- **Claude:** `launch` agrega `--mcp-config` con `spec.servers` (los secretos como `${VAR}`) y `--plugin-dir` si hay `skill_roots`. `session_extras` devuelve stdio, http y sse, con skills.
- **Codex:**
  - `thread_params` mete `spec.servers` en `config.mcp_servers`.
  - `Start` agrega `skills/extraRoots/set`.
  - `servers_query` es `["mcp","list","--json"]`, con su parser.
  - `AddServer` y `RemoveServer` usan `codex mcp add/remove`, y `server_scopes` es `[("user","All my projects")]`.
  - `plugins_query`, `marketplaces_query` y `extension_op` usan `codex plugin …` (§2.3).
  - `Skills` es `skills/list`, y `Extension(SkillState)` es `skills/config/write`.
  - `McpStatus` es `mcpServerStatus/list{threadId}`, `McpAuthenticate` es `mcpServer/oauth/login`, y `McpReconnect` es `config/mcpServer/reload`.
  - `refuses_server` rechaza sse ("Codex doesn't support SSE servers").

### 5.2 Motor

- **Módulo nuevo `global.rs`:** carga y guarda `extensions.json` (escribe a un archivo temporal y lo renombra), usa el llavero, y tiene `plan(agent, adapter, last_lists) → (servers, skill_roots, env, skipped)`.
- **Intents y eventos:** `Intent::Global(GlobalOp)` con Add, Edit, Remove, UseIn, MakeGlobal y CopyTo; y `Event::Global{servers, skills, per_agent: Map<agent, Vec<(name, Applies)>>}`, donde `Applies` es Active, Excluded, Overridden(why) o Refused(why).
- **Al lanzar una sesión:** completa `SessionSpec.servers` y `skill_roots`, y agrega el `env` de los secretos después del `env` del agente que viene de Settings.
- **`AgentEvent::McpServers`:** marca `McpSource::Global` en los servidores cuyo nombre se inyectó.
- **`skills_of` se generaliza** a `ask_session(adapter, cwd, commands, until)`, que sirve para `AgentCommand::Extension`, para la lista de skills de Codex y para el estado de MCP fuera de una sesión.
- **Un cambio en Global** se aplica a las sesiones *nuevas*. Las que ya corren muestran "Applies to new sessions", el mismo texto que las opciones del agente (ui.md §2.11). Recargar una sesión en curso queda para después.

### 5.3 UI

- **view.rs:**
  - `customize_header` suma el chip de agente;
  - `connectors_page`, `skills_page` y `plugins_page` arman la sección Global y la vista efectiva;
  - se quita el atajo "primer agente" (view.rs:9541, 9585) y se corrige el filtro de alcances vacíos en `skills_page`.
- **newen-components:** `customize::agent_marks`, que es composición de Newen y no un componente genérico.
- **Diálogo Add:** "Where" agrupado por agente y "Secret" en cada variable o header.
- **El estado de la UI** suma `ui.global: Option<GlobalExtensions>` y `ui.customize_agent: Option<String>`. La elección se guarda en `settings.json` (`customize_agent`), como el resto de las preferencias de página.

---

## 6. Migración

- **Datos:** no hay nada que migrar. Newen no guarda extensiones: `ui.extensions` y `connectors.listed` ya están separados por agente. `extensions.json` se crea cuando se agrega el primer Global.
- **`NewServer.remote`** viaja solo en un Intent y no se guarda. Pasa a `transport` sin compatibilidad hacia atrás.
- **"Make global"** es opcional y lo hace la persona, fila por fila. Toma la definición completa de `codex mcp get <n> --json` ✔ o de `claude mcp get <n>` (texto: el parser se arma con una captura, V5). Si la definición trae valores de variables, se guardan **directo en el llavero**: no se muestran ni van al log. Después pregunta "Remove it from Claude Code's config?" (`RemoveServer`).
- **Textos:** "from your account" y "from a plugin" siguen viniendo de `McpSource`. Lo que solo dice "Claude" pasa a nombrar al agente (`AgentInfo.name`).

---

## 7. Plan por fases (cada una se cierra con `scripts/check.sh`)

| Fase | Contenido | Pruebas |
|---|---|---|
| **0. Codex al nivel de Claude, por agente** | Lo de Codex de §5.1 (servidores, plugins, marketplaces, skills, MCP de la sesión), `mcp_toggle`, la corrección de `skills_page` y el Add por agente | Captura nueva `fixtures/0.163.0/mcp-list.json` (sin valores); parseo de `mcp list --json`, de `plugin list --json` y de `skills/list`; los `ProgramRun` de cada `ExtensionOp`, como los de Claude (`crates/newen-agent-codex/src/tests.rs`); en la UI, que una página con dos agentes muestre las dos secciones y que el Add de cada sección mande su `agent` |
| **1. Secciones y selector de agente** | Chip de agente, secciones con logo, vista efectiva sin Global, "Where" agrupado | Pruebas de la UI en `tests.rs`: el chip filtra y una sección vacía dice "No connectors yet" con el nombre del agente |
| **2. Servidores Global** | `global.rs`, el llavero, `SessionSpec.servers`, cómo aplica cada adaptador, la regla de nombres, `McpSource::Global`, la sección Global y las marcas | Ida y vuelta de `extensions.json`; `plan()` con conflicto, servidor excluido, transporte rechazado y lista desconocida; en `launch` de Claude, `--mcp-config` con `${NEWEN_MCP_…}` **y ningún valor secreto en args**; en `thread/start` de Codex, `config.mcp_servers` con `env_vars` y sin valores; un `LaunchPlan` con el secreto solo en `env`; persistir antes de notificar (el archivo existe cuando sale `Event::Global`) |
| **3. Skills Global** | La carpeta global, el plugin generado para Claude, `skills/extraRoots/set`, las validaciones `refuses_skill`, New, Import y Make global | El frontmatter que acepta y el que rechaza cada adaptador; el plugin generado tiene `plugin.json` válido; `Start` de Codex incluye `skills/extraRoots/set` |
| **4. Copiar entre capas, y marketplaces para varios agentes** | "Copy into…", "Make global…", "Add marketplace" e "Install for" para varios agentes | Que `extension_op` se llame para cada agente marcado; que el error de uno no corte los demás |
| **5. Gemini y OpenCode** | Cada uno con su adaptador y su fase de captura, como Codex (agents.md) | Sus propias capturas |

**Verificaciones con el CLI real** (las corre la persona, o se autorizan antes, porque inician servidores o sesiones):

- V1: Claude expande `${VAR}` dentro de `--mcp-config`.
- V2: el `thread/start.config.mcp_servers` de Codex aparece en `mcpServerStatus/list` y conecta.
- V3: en Claude, quién gana entre `--mcp-config` y un servidor de la persona con el mismo nombre, y qué `scope` informa `mcp_status` para un servidor dinámico.
- V4: qué nombre reciben los skills que llegan por `--plugin-dir`, y si `--add-dir` carga `.claude/skills`.
- V5: el formato de salida de `claude mcp get`.
- V6: `mcp_authenticate` funciona con un servidor que llegó por `--mcp-config`.
- V7: el `app-server` de Codex termina cuando se cierra stdin, para `ask_session`.

---

## 8. Preguntas para el dueño

1. **¿Global solo dentro de Newen (inyección) o también en la terminal?** La recomendación es solo en Newen, más "Copy into…" cuando haga falta.
2. **¿La carpeta global de skills es la de Newen o `~/.agents/skills`?** `~/.agents/skills` ya la leen Codex, Gemini y OpenCode también fuera de Newen, pero es una carpeta compartida que Newen pasaría a escribir.
3. **Secretos:** ¿alcanza con la variable de entorno del proceso (que el shell del agente puede leer), o se prefiere el archivo `0600` temporal para Claude?
4. **Conflicto de nombres:** ¿gana el del agente (propuesto) o el Global?
5. **Agente nuevo:** ¿recibe todo lo Global compatible (lista negra, propuesto) o hay que habilitarlo uno por uno?
6. **¿Hace falta un Global por proyecto** (por ejemplo `.newen/extensions.json`)? Hoy no está en el diseño.
7. **Plugins:** ¿alcanza con "Install for" y "Add marketplace" para varios agentes, o se quiere una capa Global de plugins?
8. **¿Se acepta la dependencia `keyring`?** Sin Secret Service: ¿no se guardan secretos, o se avisa y se permite el texto plano?
9. **¿Se ofrece el importador propio de Codex** (`externalAgentConfig/detect|import` ✔: MCP_SERVER_CONFIG, SKILLS, PLUGINS…)? Escribe en la configuración de Codex, no en la de Newen.
10. **Conectores de cuenta** (claude.ai, apps de ChatGPT): ¿solo se leen por agente, o se abren las páginas del proveedor para administrarlos?

## 9. Decisiones del dueño (2026-10-28)

1. **Alcance.** Desde Newen se agregan, quitan y activan skills, MCPs y conectores **en cada CLI** (Claude, Codex y los que vengan), con los comandos propios de cada uno (`claude mcp add|remove`, `codex mcp add|remove`, carpetas de skills de cada CLI). Eso queda también en la terminal. Además existe la capa **"All agents"**, que Newen inyecta por sesión (no escribe en los archivos de los CLIs). El diálogo de agregar ofrece el destino: uno o varios CLIs, o "All agents".
2. **Skills globales:** carpeta propia de Newen (`config_dir/newen/skills/`), inyectada por sesión. Las skills de un CLI van a la carpeta de ese CLI.
3. **Secretos:** llavero del sistema (dependencia `keyring`), llegan al agente como variable de entorno.
4. **Conflicto de nombres:** gana el del CLI; el global queda marcado como reemplazado.
5. Para el resto de las preguntas (5–10) rige la recomendación de este documento.

## 10. Estado (2026-10-28, rama `wip/ext` de `quill-wt-ext`)

- **Fase 0, hecha.** Codex al nivel de Claude: servidores (`codex mcp list --json`, add, remove), plugins y marketplaces (`codex plugin …`), skills (`skills/list` y `skills/config/write` del `app-server`), MCP de la sesión (`mcpServerStatus/list`, `config/mcpServer/reload`, `mcpServer/oauth/login`) y sus skills en el menú `/`. Los tres fallos: el "Add" del header ya no actúa solo para el primer agente, "Looking for skills…" no se queda girando (`Extensions.loaded`) y los conectores de Codex se listan. Distinto de lo propuesto: `McpToggle` y borrar credenciales no existen en Codex (`mcp_toggle` y `mcp_clear_auth` en false, la UI los oculta); y apagar o actualizar un plugin tampoco (`ExtensionKinds`).
- **Fase 1, hecha, sin la capa Global.** Secciones por agente con logo en las tres páginas, marcas con el logo de los agentes donde un elemento está activo, "Add to" con uno o varios CLIs (cada uno con su propio comando), "Where" por agente, Discover con un botón por agente y "Add marketplace" para varios. La sección "All agents" quedó fuera; entra como primera sección. No se hizo el chip de agente del header ni la vista efectiva (§3.1 y §3.4): con las secciones separadas no hacían falta todavía.
- **Pendiente:** fases 2 a 5 (Global, skills globales, copiar entre capas, Gemini y OpenCode), `keyring`, y las verificaciones V1 a V7 de §7 que necesitan sesiones reales.

## 11. Estado, fases 2 y 3 (2026-10-28, rama `wip/global` de `quill-wt-global`)

- **Fase 2 (servidores globales), hecha.** `newen-engine/src/global.rs`: `extensions.json` (escritura atómica, guarda antes de avisar con `Event::Global`), llavero con `keyring` (`Secrets`: `Keychain` real y `MemorySecrets` para pruebas), `plan()` con la regla de nombres, la exclusión por agente y lo que cada adaptador rechaza. Inyección: Claude con `--mcp-config` (secretos como `${NEWEN_MCP_…}`), Codex con `thread/start.config.mcp_servers`. `McpSource::Global`. UI: "All agents" como primera sección de Connectors, "Add to" con "All agents", marcas por agente con su estado.
- **Fase 3 (skills globales), hecha.** `config_dir/newen/skills/`, "New skill" e "Import a folder", quitar, apagar por agente. Claude: `--plugin-dir` con un plugin que el adaptador genera (`newen:<nombre>`); Codex: `skills/extraRoots/set`.
- **Distinto de lo propuesto.** `servers` es una lista y no un mapa en `extensions.json`; `NewServer` conserva `remote` (la capa global tiene su propio `GlobalServer { remote, sse }`, así que no hizo falta el enum `Transport`); no hay `{ "env": NAME }` como valor ni "Edit…" (se agrega de nuevo con el mismo nombre); quitar no pide confirmación; "Use in" es un menú de opciones, no de casillas; la ruta de "Import" se escribe (no hay selector de carpetas); el secreto de un servidor stdio de Codex va con el nombre de la clave (`env_vars` no renombra), con choque entre servidores.
- **Verificaciones.** V1: sí (también en línea, no solo con archivo). V2: sí. V3: en Claude gana el de `--mcp-config` (por eso filtra Newen). V4: `newen:<nombre>`, y `--add-dir` sí carga `.claude/skills`. V7: sí. V5 y V6 sin verificar (no bloquean). Detalle en `docs/agents.md`.
- **Sin verificar:** el llavero real (Secret Service); una sesión real de cada CLI con un servidor y un skill globales de punta a punta (se probó cada pieza contra el CLI real, y el motor con el agente falso).
- **Pendiente:** fase 4 (copiar entre capas, "Make global", marketplaces para varios agentes) y fase 5 (Gemini, OpenCode); recargar una sesión en curso al cambiar Global; el chip de agente y la vista efectiva (§3.1, §3.4).

## 12. Customize como página propia (2026-11-02, rama `feat/customize-page`)

Customize deja de ser una columna de Ajustes. Lo que sigue es lo que la persona ve.

### 12.1 Cómo se llega y qué hay arriba

- **Entrada:** el «Customize» de la barra lateral abre la página en el lugar de la conversación; la barra lateral queda. Elegir una sesión o «New» la cierra. Ajustes › General conserva «Customize…». El «+» del composer («Browse connectors», «Manage plugins», «Browse plugins») y `/skills` y `/plugin` también abren la página.
- **Encabezado:** «Customize» en negrita de 22 y el chip de la capa, que abre su menú (All agents y cada agente). No hay engranaje: Ajustes se abre desde la barra lateral o con Ctrl+,.
- **Pestañas:** Skills, Connectors y Plugins. Una pestaña en la que ningún agente mostrado participa se ve atenuada, no responde y dice por qué en su tooltip («Codex has no plugins Newen can manage.»). El motivo sale de lo que declara cada adaptador (`AgentInfo.customize`: si tiene `plugins_query` y `servers_query`), no del nombre del agente.
- **Migas:** dentro de un elemento, `Customize › Connectors › github`. Cada parte vuelve atrás. Son el `Breadcrumb` de limn.
- **Listas largas:** el cuerpo de la página es una lista virtual de limn (`measured_list`): cada fila guarda lo que muestra y se arma y se mide solo al entrar en vista, así que con cientos de skills o conectores un cuadro arma unas 15 filas. Las marcas de «en qué otros agentes está» y la descripción de cada plugin se buscan una vez por cuadro, no por fila, y el logo de un conector se pide por su propia etiqueta.
- **Barra de cada pestaña, una sola fila:** búsqueda; en Connectors, el filtro de estado como menú compacto (All / Connected / Not connected, con el elegido como rótulo); en Skills y Plugins, el chip de carpeta (y en Skills, «Sort»); «Discover»; y «+ Add ▾». El botón relleno es el que más sirve: Discover mientras la lista está vacía y Add después. En una ventana angosta la barra baja de línea. Arriba quedan solo el título con el chip de la capa y las pestañas.

### 12.2 Las capas («All agents» y cada agente)

El chip del encabezado elige qué se ve:

- **All agents** (por defecto): primero la sección «All agents», que es la capa propia de Newen (§4, se agrega a cada sesión), y después una sección por agente, como antes. Las marcas por agente de cada fila global siguen igual (§3.2).
- **Un agente:** solo la sección de ese agente. La capa global no se muestra como sección; en el detalle de un skill global aparece un interruptor para ese agente.

Elegir un agente que no tiene la pestaña abierta vuelve a Skills.

### 12.3 Connectors

- Sin pestañas internas: «Discover» es el botón de la barra. Dentro de Discover, la barra empieza con «← Your connectors», que vuelve a la lista, y la búsqueda pasa a ser la del registro. El filtro de estado es el menú de la barra.
- **Lista:** Connector (logo de 24 o el ícono del enchufe, y el nombre, que abre el detalle), Type («Local» o «Remote») y Status de 160 («Connected», «On», «Needs sign-in», «Pending approval», «Not connected», «Failed to connect»), y el «⋯» con Remove para los propios. Por debajo de 560 de ancho la columna Type no se muestra.
- **Franja:** si algún conector necesita a la persona (sesión vencida o falla), «N of your connectors need attention» con «Reconnect all», toda en ámbar: fondo ámbar translúcido, borde ámbar, y el ícono, el texto y el botón en la tinta ámbar de «Manual» (`look::manual`), legible en claro y oscuro. Reconectar todo pide `McpReconnect` en cada sesión que cargó ese servidor y vuelve a listar los de cada agente. «Hide for N days» no está.
- **Vacía:** «Add your first connectors», en tarjeta y sin ilustración, con «Discover connectors». **Sin resultados:** la lupa de 32, «No connectors match your search» y «Try a different term, or browse Discover.».
- **Discover:** el registro público de MCP en tarjetas de dos o tres por fila (mínimo 280). Cada tarjeta: radio `tile` 16 (token nuevo de limn), recuadro de 40, nombre de hasta dos líneas, «autor • Remote • v1.0.0», descripción de dos líneas y un botón fantasma «+». El directorio de plugins usa la misma tarjeta: «marketplace • 12k installs», un punto si la versión instalada no es la que se ofrece, y un engranaje que abre la página del plugin si ya está instalado.

### 12.4 El detalle de un conector

- Encabezado: logo de 36, nombre en 18 negrita, estado, un interruptor y «⋯» (Read its tools again, Remove). El interruptor está apagado con su motivo: un CLI prende y apaga un servidor solo dentro de la sesión (`/mcp`), y la capa global se prende por agente desde su fila.
- Datos con recuadro de 20: tipo, el comando o la dirección, «Added by» (You, for all your projects / This project's settings / Your organization / A plugin / Your account / Newen, for all agents) y «Used by».
- **What it can do:** «Can run code without asking» (un comando: corre un programa en esta computadora al empezar la sesión), «Can send data to other services» (una dirección), y según lo que dicen sus herramientas de sí mismas, «Only reads» o «Can change or delete things». Es **inferido**: la (i) lo dice, porque un servidor puede hacer más de lo que declara.
- **Tool permissions:** grupos Read-only, Write/delete y Other (por `read_only` y `destructive` del servidor), cada uno con su contador y un menú que fija todo el grupo (Always allow, Needs approval, Blocked, «As the agent decides»). Cada herramienta es una fila de 56 con sangría de 24 y tres íconos con tooltip: círculo con tilde, mano y prohibido. Una bloqueada se ve atenuada. Sin elegir, decide el agente con su propia configuración. Los grupos no se pliegan y no hay «Custom».
- **De dónde salen las herramientas:** de la última sesión del agente que cargó el servidor; si ninguna lo hizo, el motor arranca el programa del agente sin sesión y le pide el estado de sus servidores (`Intent::ServerTools`), repitiendo cada segundo mientras alguno conecta, 30 s como máximo.

### 12.5 Cómo se guardan y cómo llegan a cada agente los permisos

- La UI junta los cambios y los manda 500 ms después del último clic (`GlobalOp::Tools`). El motor los escribe en `extensions.json` (`tools`) **antes** de avisar con `Event::Global`.
- Se guardan por agente y servidor (`agent` vacío para un servidor de «All agents»). Quitar un servidor de un agente borra sus elecciones.
- Al lanzar una sesión, el motor le da al adaptador las elecciones de ese agente y las de los servidores globales que esa sesión recibe (`SessionSpec.tool_rules`), solo con las políticas que el adaptador declara (`AgentAdapter::tool_policies`). Se aplican a sesiones nuevas.
- **Claude Code:** reglas de permiso en el `--settings` de la sesión: Always allow → `permissions.allow`, Needs approval → `ask`, Blocked → `deny`, con el nombre `mcp__<servidor>__<herramienta>` (lo que no es letra, dígito, `_` o `-` en el nombre del servidor pasa a `_`). Se suman a las de la persona; en el CLI un `deny` gana a un `ask` y un `ask` a un `allow`, así que una regla propia más estricta de la persona sigue mandando.
- **Codex:** en el `config` del `thread/start`, que Codex pone sobre la configuración de la persona solo para ese hilo: Always allow → `mcp_servers.<servidor>.tools.<herramienta>.approval_mode = "approve"`, Needs approval → `"prompt"`, Blocked → la herramienta en `disabled_tools` del servidor. **Inferido:** salió de los tipos de configuración de `openai/codex@main`, más nuevos que la 0.163.0 probada; que la capa del hilo se mezcle con el servidor de `config.toml` sin redefinirlo no se verificó con el CLI.
- Un adaptador que no declara una política deja ese ícono atenuado con «{agente} can't say this for one tool»; sin ninguna, la sección dice que el agente decide solo.

### 12.6 Skills

- Cada fila tiene un interruptor (el primer estado del agente o el último; para Claude Code, «on» u «off») además del «⋯» con todos los estados. Bloqueado o sin carpeta de proyecto cuando el agente lo pide, el interruptor está apagado.
- «Sort» ordena por origen (con secciones) o por nombre (una lista) y filtra por origen. «Last edited» no está: los CLIs no dicen la fecha.
- «+ Add ▾»: **Create with an agent** (cierra la página y deja en el composer de una sesión nueva un pedido para escribir un skill), **Write instructions…** (nombre, descripción e instrucciones; crea el skill global), **Upload a file…** (`.md`, `.zip` o `.skill`, con el diálogo del sistema) e **Import a folder…**.
- **Subir:** un `.md` es el `SKILL.md` (si no trae frontmatter, se le agrega con el nombre del archivo); un `.zip` o `.skill` se abre en una carpeta aparte y se toma la carpeta que tiene el `SKILL.md` más arriba. Se saltean las entradas que salen de la carpeta, y el total no pasa de 32 MB. Dependencia nueva: `zip` (solo deflate).
- **Detalle:** nombre, interruptor, descripción, de dónde viene y «Only adds instructions» (un skill son palabras que el agente lee; sus scripts corren solo si el agente los corre, con sus permisos). Para un skill global, el «⋯» tiene Download (guarda un `.zip`), Replace… y Delete en rojo. Un skill de un agente vive en la carpeta de ese CLI: se descarga, reemplaza o borra allí, y la página lo dice.

### 12.7 Plugins

- La lista de antes, con una flecha que abre el detalle: descripción del marketplace, «From the {marketplace} marketplace», qué puede hacer y «Connectors & tools» (los servidores de origen plugin con su nombre y los skills `plugin:…`).
- Las líneas de confianza se deducen de lo que Newen ve que trae: solo skills → «Only adds instructions»; un comando → «Can run code without asking»; una dirección → «Can send data to other services». Es **inferido**: hooks y commands no se listan, así que puede hacer más.

### 12.8 Add custom connector

- «Name» («Shown in the connectors list.») y «MCP server URL, or command», con el texto de ayuda de la URL.
- Avisos, en ámbar, debajo del campo: caracteres ocultos (de ancho cero, de dirección o de control), un usuario o contraseña antes del host, letras fuera de ASCII en el host, y un host que no contiene ninguna palabra del nombre («evil.example doesn't look like github's site»). No bloquean.
- `http://` solo para esta computadora (`localhost`, `127.0.0.1`, `[::1]`); cualquier otro pide `https://` y «Add» queda apagado.
- **Advanced settings:** OAuth Client ID y Client Secret. Claude Code los toma (`mcp add --client-id` y `--client-secret`; el secreto va por stdin, nunca como argumento: **inferido** que el CLI lee ahí su pregunta, sin verificar). Codex no tiene cliente propio: con Codex elegido, o con «All agents», los campos no se muestran y se dice por qué.
- No hay «Step 1 of 2»: es un solo paso.

### 12.9 Íconos

Se quedan los de Newen, sin biblioteca de terceros. El trazo baja de 1,7 a **1,25** sobre la grilla de 20 (`icons::STROKE`), y una prueba lo controla en todos. Íconos nuevos, en el mismo estilo y descritos por lo que muestran: `SKILL` (un libro abierto), `PLUGIN` (una pieza de rompecabezas), `TOOL` (una llave), `LOCK`, `BLOCKED` (un aro con una barra), `UPLOAD`, `DOWNLOAD`, `INFO`, `DISCOVER` (una brújula), `STOREFRONT` (una tienda con toldo) y `SORT`.

### 12.10 Sin verificar

- La página no se abrió en la app real (solo pruebas de layout sin superposición a 700, 1000, 1600 y 2400 de ancho).
- `Intent::ServerTools` con los CLIs reales: que Claude Code conteste `mcp_status` sin un mensaje enviado y que Codex lo haga sin hilo.
- El mapeo de Codex en la 0.163.0 y la lectura del secreto OAuth por stdin en Claude Code.
