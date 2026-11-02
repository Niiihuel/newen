//! quill's "All agents" layer: MCP servers and skills that quill keeps in its own folder and
//! adds to each session, without writing the agents' files (docs/agents.md "All agents").
//!
//! - `extensions.json` holds the servers; secrets are in the system keychain, the file only
//!   says a value is one. A secret reaches the agent only as a variable of its process.
//! - `skills/<name>/SKILL.md` are the skills; a session gets `injected/<agent>/skills`, a
//!   folder of links to the ones that agent uses.
//! - When an agent has a server or skill of its own with the name of a global one, the
//!   agent's wins and the global one is not added (the person's decision, spec §9.4).

use newen_agent::{InjectedServer, SessionExtras, ToolRule};
use newen_model::{
    AppliedTo, Applies, GlobalOp, GlobalServer, GlobalSkill, GlobalState, ToolChoice,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Where secrets are kept: the system keychain, or memory in tests.
pub trait Secrets: Send + Sync + std::fmt::Debug {
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, account: &str);
}

/// The system keychain (Secret Service on Linux), service `newen`; a secret it does not have is
/// read from `quill`'s, where quill, Newen's name before, kept them, and copied forward
/// (docs/UPDATES.md, «Venir de quill»).
#[derive(Debug)]
pub struct Keychain;

impl Keychain {
    fn services() -> Carried<Service, Service> {
        Carried(Service("newen"), Service("quill"))
    }
}

impl Secrets for Keychain {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Self::services().get(account)
    }
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        Self::services().set(account, value)
    }
    fn delete(&self, account: &str) {
        Self::services().delete(account)
    }
}

/// One service of the system keychain.
#[derive(Debug)]
struct Service(&'static str);

impl Service {
    fn entry(&self, account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(self.0, account).map_err(|e| e.to_string())
    }
}

impl Secrets for Service {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        match self.entry(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.entry(account)?
            .set_password(value)
            .map_err(|e| e.to_string())
    }
    fn delete(&self, account: &str) {
        if let Ok(entry) = self.entry(account) {
            let _ = entry.delete_credential();
        }
    }
}

/// Secrets written to the first; read from the second when the first has none, and then
/// copied to the first (a copy that cannot be written is no error: the second still has it). A
/// write removes the second's: quill 0.1.9, which a rollback returns to, reads its own first,
/// and an older value there would hide the one written here. A delete removes both, or the
/// second would give back a secret deleted here.
#[derive(Debug)]
struct Carried<A, B>(A, B);

impl<A: Secrets, B: Secrets> Secrets for Carried<A, B> {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        if let Some(value) = self.0.get(account)? {
            return Ok(Some(value));
        }
        let found = self.1.get(account)?;
        if let Some(value) = &found {
            let _ = self.0.set(account, value);
        }
        Ok(found)
    }
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.0.set(account, value)?;
        self.1.delete(account);
        Ok(())
    }
    fn delete(&self, account: &str) {
        self.0.delete(account);
        self.1.delete(account);
    }
}

/// Secrets held in memory: for tests, and for the screenshots' fake keychain.
#[derive(Debug, Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl Secrets for MemorySecrets {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(account.into(), value.into());
        Ok(())
    }
    fn delete(&self, account: &str) {
        self.0.lock().unwrap().remove(account);
    }
}

/// What an agent already has of its own, as the engine last read it; `None` is not read yet,
/// and then the global one is added and the agent left to decide.
#[derive(Clone, Debug, Default)]
pub struct Own {
    pub servers: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
}

/// One agent as the layer sees it.
pub struct AgentView<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub extras: SessionExtras,
}

/// What one session gets.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub servers: Vec<InjectedServer>,
    pub skill_roots: Vec<PathBuf>,
    /// Secrets, as variables of the agent's process.
    pub env: Vec<(String, String)>,
    /// The names of the servers added, which the session's server list marks as global.
    pub names: Vec<String>,
    pub applied: Vec<AppliedTo>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct File {
    version: u32,
    servers: Vec<GlobalServer>,
    skills: SkillsFile,
    /// The tool permissions the person chose on connectors' pages, every agent's.
    #[serde(default)]
    tools: Vec<ToolChoice>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct SkillsFile {
    /// Skill name to the agents it is turned off for.
    except: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct Layer {
    file: Option<PathBuf>,
    secrets: Arc<dyn Secrets>,
    keychain: Arc<OnceLock<bool>>,
}

/// A server's or skill's name: lowercase words with dashes or underscores, valid for every
/// agent (OpenCode is the strictest about it, spec §2.2).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && !name.starts_with(['-', '_'])
}

fn account(server: &str, header: bool, key: &str) -> String {
    format!(
        "mcp/{server}/{}{key}",
        if header { "header:" } else { "env:" }
    )
}

/// `NEWEN_MCP_<SERVER>_<KEY>`: the variable of the agent's process that holds a secret.
fn variable(server: &str, header: bool, key: &str) -> String {
    let up = |s: &str| {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect::<String>()
    };
    format!(
        "NEWEN_MCP_{}_{}{}",
        up(server),
        if header { "H_" } else { "" },
        up(key)
    )
}

impl Layer {
    pub fn new(file: Option<PathBuf>, secrets: Arc<dyn Secrets>) -> Self {
        Self {
            file,
            secrets,
            keychain: Arc::default(),
        }
    }

    fn root(&self) -> Option<&Path> {
        self.file.as_deref().and_then(Path::parent)
    }

    fn read(&self) -> Result<File, String> {
        let Some(path) = &self.file else {
            return Ok(File::default());
        };
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("extensions.json could not be read: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Whole or not at all: written beside the file, then moved over it.
    fn write(&self, file: &File) -> Result<(), String> {
        let path = self.file.as_ref().ok_or("quill has no folder for this")?;
        let mut file = file.clone();
        file.version = 1;
        file.servers.sort_by(|a, b| a.name.cmp(&b.name));
        std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
            .map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_vec_pretty(&file).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    fn skills_dir(&self) -> Option<PathBuf> {
        self.root().map(|r| r.join("skills"))
    }

    /// The skills in quill's folder, by name.
    fn skills(&self, file: &File) -> Vec<GlobalSkill> {
        let Some(dir) = self.skills_dir() else {
            return vec![];
        };
        let mut found: Vec<GlobalSkill> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                // A skill being uploaded waits beside them under a dot.
                if name.starts_with('.') {
                    return None;
                }
                let text = std::fs::read_to_string(entry.path().join("SKILL.md")).ok()?;
                Some(GlobalSkill {
                    description: frontmatter(&text, "description").unwrap_or_default(),
                    except: file.skills.except.get(&name).cloned().unwrap_or_default(),
                    name,
                })
            })
            .collect();
        found.sort_by(|a, b| a.name.cmp(&b.name));
        found
    }

    /// Make a change. The secrets go to the keychain first, then the file is written; the
    /// caller announces the new state after this returns.
    pub fn op(&self, op: GlobalOp) -> Result<(), String> {
        let mut file = self.read()?;
        match op {
            GlobalOp::Load => {}
            GlobalOp::SetServer(server) => self.set_server(&mut file, server)?,
            GlobalOp::RemoveServer(name) => {
                let at = file.servers.iter().position(|s| s.name == name);
                if let Some(server) = at.map(|i| file.servers.remove(i)) {
                    self.write(&file)?;
                    self.forget(&server, &[]);
                }
            }
            GlobalOp::Use {
                skill,
                name,
                agent,
                on,
            } => {
                let except = if skill {
                    file.skills.except.entry(name).or_default()
                } else {
                    match file.servers.iter_mut().find(|s| s.name == name) {
                        Some(server) => &mut server.except,
                        None => return Err(format!("There is no server named {name}")),
                    }
                };
                except.retain(|a| *a != agent);
                if !on {
                    except.push(agent);
                }
                file.skills.except.retain(|_, agents| !agents.is_empty());
                self.write(&file)?;
            }
            GlobalOp::ImportSkill(from) => return self.import_skill(&from),
            GlobalOp::NewSkill {
                name,
                description,
                instructions,
            } => {
                let dir = self.new_skill_dir(&name)?;
                let instructions = match instructions.trim() {
                    "" => "What the agent should do when this skill applies.",
                    words => words,
                };
                let body = format!(
                    "---\nname: {name}\ndescription: {}\n---\n\n# {name}\n\n{instructions}\n",
                    description.replace('\n', " ")
                );
                std::fs::write(dir.join("SKILL.md"), body).map_err(|e| e.to_string())?;
            }
            GlobalOp::UploadSkill { path, replace } => {
                return self.upload_skill(&path, replace.as_deref())
            }
            GlobalOp::ExportSkill { name, to } => {
                let dir = self.skill_dir(&name)?;
                return crate::skills::zip_folder(&dir, &name, &to);
            }
            GlobalOp::Tools(choices) => {
                for choice in choices {
                    file.tools.retain(|t| {
                        (&t.agent, &t.server, &t.tool)
                            != (&choice.agent, &choice.server, &choice.tool)
                    });
                    if choice.policy.is_some() {
                        file.tools.push(choice);
                    }
                }
                self.write(&file)?;
            }
            GlobalOp::RemoveSkill(name) => {
                let dir = self.skill_dir(&name)?;
                std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
                file.skills.except.remove(&name);
                self.write(&file)?;
            }
        }
        Ok(())
    }

    fn set_server(&self, file: &mut File, mut server: GlobalServer) -> Result<(), String> {
        if !valid_name(&server.name) {
            return Err("A server's name is lowercase letters, digits, - and _".into());
        }
        if server.target.trim().is_empty() {
            return Err(if server.remote {
                "The address is empty"
            } else {
                "The command is empty"
            }
            .into());
        }
        let old = file
            .servers
            .iter()
            .position(|s| s.name == server.name)
            .map(|i| file.servers.remove(i));
        server.sse &= server.remote;
        if let Some(old) = &old {
            server.except = old.except.clone();
        }
        let unused = if server.remote {
            &mut server.env
        } else {
            &mut server.headers
        };
        unused.clear();
        let mut kept = vec![];
        for (header, vars) in [(false, &mut server.env), (true, &mut server.headers)] {
            vars.retain(|v| !v.key.trim().is_empty());
            for var in vars.iter_mut() {
                var.key = var.key.trim().to_owned();
                if !var.secret {
                    continue;
                }
                let acc = account(&server.name, header, &var.key);
                match var.value.take() {
                    Some(value) => self
                        .secrets
                        .set(&acc, &value)
                        .map_err(|e| format!("The keychain could not keep {}: {e}", var.key))?,
                    // Edited without typing the secret again: the one kept stays.
                    None if self.secrets.get(&acc).ok().flatten().is_none() => {
                        return Err(format!("{} is a secret and has no value", var.key))
                    }
                    None => {}
                }
                kept.push(acc);
            }
        }
        file.servers.push(server);
        self.write(file)?;
        if let Some(old) = old {
            self.forget(&old, &kept);
        }
        Ok(())
    }

    /// Delete `server`'s secrets, except the accounts in `keep`.
    fn forget(&self, server: &GlobalServer, keep: &[String]) {
        for (header, vars) in [(false, &server.env), (true, &server.headers)] {
            for var in vars.iter().filter(|v| v.secret) {
                let acc = account(&server.name, header, &var.key);
                if !keep.contains(&acc) {
                    self.secrets.delete(&acc);
                }
            }
        }
    }

    /// The folder of the skill `name`, which has a SKILL.md.
    fn skill_dir(&self, name: &str) -> Result<PathBuf, String> {
        let dir = self
            .skills_dir()
            .ok_or("quill has no folder for this")?
            .join(name);
        if !valid_name(name) || !dir.join("SKILL.md").is_file() {
            return Err(format!("There is no skill named {name}"));
        }
        Ok(dir)
    }

    /// A skill from a file (crate::skills): unpacked beside quill's skills, then moved in,
    /// over the one it replaces.
    fn upload_skill(&self, from: &Path, replace: Option<&str>) -> Result<(), String> {
        let root = self.skills_dir().ok_or("quill has no folder for this")?;
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let staged = root.join(".upload");
        let _ = std::fs::remove_dir_all(&staged);
        let unpacked = crate::skills::unpack(from, &staged);
        let name = match unpacked {
            Ok(found) => replace.map(str::to_owned).unwrap_or(found),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staged);
                return Err(e);
            }
        };
        let done = (|| {
            let dir = match replace {
                Some(old) => {
                    let dir = self.skill_dir(old)?;
                    std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
                    dir
                }
                None => {
                    let dir = self.new_skill_dir(&name)?;
                    std::fs::remove_dir(&dir).map_err(|e| e.to_string())?;
                    dir
                }
            };
            std::fs::rename(&staged, &dir).map_err(|e| e.to_string())
        })();
        let _ = std::fs::remove_dir_all(&staged);
        done
    }

    /// What `agent`'s session is told of its tools: the person's choices for its own servers,
    /// and for the "All agents" servers it gets (`injected`).
    pub fn tool_rules(&self, agent: &str, injected: &[String]) -> Vec<ToolRule> {
        let file = self.read().unwrap_or_default();
        (file.tools.into_iter())
            .filter(|t| t.agent == agent || t.agent.is_empty() && injected.contains(&t.server))
            .filter_map(|t| {
                Some(ToolRule {
                    policy: t.policy?,
                    server: t.server,
                    tool: t.tool,
                })
            })
            .collect()
    }

    /// Forget the choices for a server `agent` no longer has: a rule for a server that is not
    /// there would name a server to the agent that it does not know.
    pub fn forget_tools(&self, agent: &str, server: &str) {
        if let Ok(mut file) = self.read() {
            let before = file.tools.len();
            file.tools
                .retain(|t| !(t.agent == agent && t.server == server));
            if file.tools.len() != before {
                let _ = self.write(&file);
            }
        }
    }

    fn new_skill_dir(&self, name: &str) -> Result<PathBuf, String> {
        if !valid_name(name) || name.contains('_') {
            return Err("A skill's name is lowercase words joined by -".into());
        }
        let dir = self
            .skills_dir()
            .ok_or("quill has no folder for this")?
            .join(name);
        if dir.exists() {
            return Err(format!("There is already a skill named {name}"));
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir)
    }

    fn import_skill(&self, from: &Path) -> Result<(), String> {
        let text = std::fs::read_to_string(from.join("SKILL.md"))
            .map_err(|_| format!("{} has no SKILL.md", from.display()))?;
        let folder = from
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = frontmatter(&text, "name").unwrap_or(folder);
        let dir = self.new_skill_dir(&name)?;
        copy_dir(from, &dir).map_err(|e| {
            let _ = std::fs::remove_dir_all(&dir);
            e.to_string()
        })
    }

    /// How each agent treats each global item, and what its session gets. With `resolve`, the
    /// secrets are read from the keychain and put in the plan's `env`; without it nothing
    /// touches the keychain and a secret counts as there.
    pub fn plan(&self, agent: &AgentView, own: &Own, resolve: bool) -> Plan {
        let file = self.read().unwrap_or_default();
        let mut plan = Plan::default();
        let applied = |plan: &mut Plan, name: &str, skill, applies| {
            plan.applied.push(AppliedTo {
                agent: agent.id.into(),
                name: name.into(),
                skill,
                applies,
            })
        };
        for server in &file.servers {
            let refusal = if server.except.iter().any(|a| a == agent.id) {
                Some(Applies::Excluded)
            } else if !agent.extras.servers {
                Some(Applies::Refused(format!(
                    "{} cannot take servers from quill",
                    agent.name
                )))
            } else if server.sse && !agent.extras.sse {
                Some(Applies::Refused(format!(
                    "{} does not support SSE servers",
                    agent.name
                )))
            } else if own
                .servers
                .as_ref()
                .is_some_and(|s| s.contains(&server.name))
            {
                Some(Applies::Replaced)
            } else {
                None
            };
            if let Some(why) = refusal {
                applied(&mut plan, &server.name, false, why);
                continue;
            }
            match self.inject(server, agent, resolve, &plan.env) {
                Ok((injected, env)) => {
                    plan.names.push(server.name.clone());
                    plan.servers.push(injected);
                    plan.env.extend(env);
                    applied(&mut plan, &server.name, false, Applies::Active);
                }
                Err(why) => applied(&mut plan, &server.name, false, Applies::Refused(why)),
            }
        }
        let mut links = vec![];
        for skill in self.skills(&file) {
            let applies = if skill.except.iter().any(|a| a == agent.id) {
                Applies::Excluded
            } else if !agent.extras.skills {
                Applies::Refused(format!("{} cannot take skills from quill", agent.name))
            } else if own.skills.as_ref().is_some_and(|s| s.contains(&skill.name)) {
                Applies::Replaced
            } else {
                links.push(skill.name.clone());
                Applies::Active
            };
            applied(&mut plan, &skill.name, true, applies);
        }
        if resolve && !links.is_empty() {
            if let Some(dir) = self.link_skills(agent.id, &links) {
                plan.skill_roots.push(dir);
            }
        }
        plan
    }

    fn inject(
        &self,
        server: &GlobalServer,
        agent: &AgentView,
        resolve: bool,
        taken: &[(String, String)],
    ) -> Result<(InjectedServer, Vec<(String, String)>), String> {
        let mut out = InjectedServer {
            name: server.name.clone(),
            target: server.target.clone(),
            args: server.args.clone(),
            remote: server.remote,
            sse: server.sse,
            ..Default::default()
        };
        let mut env = vec![];
        for (header, vars) in [(false, &server.env), (true, &server.headers)] {
            for var in vars {
                if !var.secret {
                    let pair = (var.key.clone(), var.value.clone().unwrap_or_default());
                    if header {
                        out.headers.push(pair)
                    } else {
                        out.env.push(pair)
                    }
                    continue;
                }
                // Some agents give a server's environment a variable only under its own name.
                let name = if agent.extras.env_by_name && !header {
                    var.key.clone()
                } else {
                    variable(&server.name, header, &var.key)
                };
                if resolve {
                    let value = self
                        .secrets
                        .get(&account(&server.name, header, &var.key))?
                        .ok_or_else(|| format!("{} is not in the keychain", var.key))?;
                    if let Some((_, other)) = taken.iter().chain(&env).find(|(n, _)| *n == name) {
                        if *other != value {
                            return Err(format!("{name} is already set by another global server"));
                        }
                    }
                    env.push((name.clone(), value));
                }
                let pair = (var.key.clone(), name);
                if header {
                    out.header_vars.push(pair)
                } else {
                    out.env_vars.push(pair)
                }
            }
        }
        Ok((out, env))
    }

    /// `injected/<agent>/skills`: links to the skills the agent uses, and only those.
    fn link_skills(&self, agent: &str, names: &[String]) -> Option<PathBuf> {
        let root = self.root()?;
        let dir = root.join("injected").join(agent).join("skills");
        std::fs::create_dir_all(&dir).ok()?;
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            if !names.contains(&entry.file_name().to_string_lossy().into_owned()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        for name in names {
            let link = dir.join(name);
            if !link.exists() {
                #[cfg(unix)]
                std::os::unix::fs::symlink(root.join("skills").join(name), &link).ok()?;
                #[cfg(not(unix))]
                copy_dir(&root.join("skills").join(name), &link).ok()?;
            }
        }
        Some(dir)
    }

    /// The layer and how every agent in `agents` treats it. Never reads a secret.
    pub fn state(
        &self,
        agents: &[AgentView],
        own: &HashMap<String, Own>,
        error: Option<String>,
    ) -> GlobalState {
        let (file, read_error) = match self.read() {
            Ok(file) => (file, None),
            Err(e) => (File::default(), Some(e)),
        };
        let applied = agents
            .iter()
            .flat_map(|a| {
                self.plan(a, &own.get(a.id).cloned().unwrap_or_default(), false)
                    .applied
            })
            .collect();
        let keychain = *self
            .keychain
            .get_or_init(|| self.secrets.get("probe").is_ok());
        GlobalState {
            skills: self.skills(&file),
            tools: file.tools,
            servers: file.servers,
            applied,
            error: error.or(read_error),
            keychain,
        }
    }
}

/// A `key: value` line of a SKILL.md's frontmatter (the part between the first two `---`).
pub(crate) fn frontmatter(text: &str, key: &str) -> Option<String> {
    let head = text.strip_prefix("---")?.split("\n---").next()?;
    head.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix(':')?.trim();
        let value = value.trim_matches(|c| c == '"' || c == '\'');
        (!value.is_empty()).then(|| value.to_owned())
    })
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// The names an agent's own lists hold, which decide what the global layer leaves to it.
pub fn remember(own: &Mutex<HashMap<String, Own>>, agent: &str, update: impl FnOnce(&mut Own)) {
    update(own.lock().unwrap().entry(agent.to_owned()).or_default());
}

#[cfg(test)]
mod tests {
    use super::*;
    use newen_model::GlobalVar;

    /// What the keychain does with its two services, Newen's and quill's: a secret is written
    /// to Newen's, read from it first and from quill's when Newen's has none, copied forward
    /// then. A write removes quill's copy, so quill 0.1.9 after a rollback, which reads its own
    /// first, finds the new value in Newen's; a delete removes both, so a deleted one never
    /// comes back from quill's.
    #[test]
    fn a_secret_is_read_from_quill_s_service_when_newen_s_has_none_and_copied() {
        let both = Carried(MemorySecrets::default(), MemorySecrets::default());
        let account = "mcp/github/env:TOKEN";
        assert_eq!(both.get(account), Ok(None));
        // Written by quill: found, and copied forward; quill's stays.
        both.1.set(account, "from quill").unwrap();
        assert_eq!(both.get(account), Ok(Some("from quill".into())));
        assert_eq!(both.0.get(account), Ok(Some("from quill".into())));
        assert_eq!(both.1.get(account), Ok(Some("from quill".into())));
        // Written here: Newen's; quill's older copy goes.
        both.set(account, "from newen").unwrap();
        assert_eq!(both.0.get(account), Ok(Some("from newen".into())));
        assert_eq!(both.1.get(account), Ok(None));
        assert_eq!(both.get(account), Ok(Some("from newen".into())));
        // Deleted: from both.
        both.1.set(account, "from quill again").unwrap();
        both.delete(account);
        assert_eq!(both.get(account), Ok(None));
        assert_eq!(both.1.get(account), Ok(None));
    }

    fn layer() -> (Layer, tempdir::Dir, Arc<MemorySecrets>) {
        let dir = tempdir::Dir::new();
        let secrets = Arc::new(MemorySecrets::default());
        (
            Layer::new(Some(dir.0.join("quill/extensions.json")), secrets.clone()),
            dir,
            secrets,
        )
    }

    mod tempdir {
        pub struct Dir(pub std::path::PathBuf);
        impl Dir {
            pub fn new() -> Self {
                let n = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                let dir = std::env::temp_dir().join(format!(
                    "quill-global-{n}-{:?}",
                    std::thread::current().id()
                ));
                std::fs::create_dir_all(&dir).unwrap();
                Self(dir)
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    const CLAUDE: AgentView = AgentView {
        id: "claude",
        name: "Claude Code",
        extras: SessionExtras {
            servers: true,
            sse: true,
            env_by_name: false,
            skills: true,
        },
    };
    const CODEX: AgentView = AgentView {
        id: "codex",
        name: "Codex",
        extras: SessionExtras {
            servers: true,
            sse: false,
            env_by_name: true,
            skills: true,
        },
    };

    fn github() -> GlobalServer {
        GlobalServer {
            name: "github".into(),
            target: "npx".into(),
            args: vec!["-y".into(), "server-github".into()],
            env: vec![
                GlobalVar {
                    key: "GITHUB_TOKEN".into(),
                    value: Some("ghp_secret".into()),
                    secret: true,
                },
                GlobalVar {
                    key: "LOG".into(),
                    value: Some("warn".into()),
                    secret: false,
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn a_secret_is_in_the_keychain_and_not_in_the_file() {
        let (layer, dir, secrets) = layer();
        layer.op(GlobalOp::SetServer(github())).unwrap();
        let text = std::fs::read_to_string(dir.0.join("quill/extensions.json")).unwrap();
        assert!(
            !text.contains("ghp_secret") && text.contains("warn"),
            "{text}"
        );
        assert_eq!(
            secrets
                .get("mcp/github/env:GITHUB_TOKEN")
                .unwrap()
                .as_deref(),
            Some("ghp_secret")
        );
        // Edited without typing the secret again: it stays.
        let mut again = github();
        again.env[0].value = None;
        layer.op(GlobalOp::SetServer(again)).unwrap();
        assert!(secrets
            .get("mcp/github/env:GITHUB_TOKEN")
            .unwrap()
            .is_some());
        layer.op(GlobalOp::RemoveServer("github".into())).unwrap();
        assert!(secrets
            .get("mcp/github/env:GITHUB_TOKEN")
            .unwrap()
            .is_none());
    }

    #[test]
    fn the_secret_reaches_the_process_only_as_a_variable() {
        let (layer, _dir, _) = layer();
        layer.op(GlobalOp::SetServer(github())).unwrap();
        let plan = layer.plan(&CLAUDE, &Own::default(), true);
        let json = serde_json::to_string(&plan.servers).unwrap();
        assert!(!json.contains("ghp_secret"), "{json}");
        assert_eq!(
            plan.servers[0].env_vars,
            [(
                "GITHUB_TOKEN".to_owned(),
                "NEWEN_MCP_GITHUB_GITHUB_TOKEN".to_owned()
            )]
        );
        assert_eq!(
            plan.env,
            [(
                "NEWEN_MCP_GITHUB_GITHUB_TOKEN".to_owned(),
                "ghp_secret".to_owned()
            )]
        );
        // Codex names a server's variable like the server reads it.
        let plan = layer.plan(&CODEX, &Own::default(), true);
        assert_eq!(
            plan.env,
            [("GITHUB_TOKEN".to_owned(), "ghp_secret".to_owned())]
        );
        // Without resolving, the keychain is not read.
        assert!(layer.plan(&CLAUDE, &Own::default(), false).env.is_empty());
    }

    #[test]
    fn the_agents_own_wins_and_exclusion_and_sse_are_honored() {
        let (layer, _dir, _) = layer();
        layer.op(GlobalOp::SetServer(github())).unwrap();
        let legacy = GlobalServer {
            name: "legacy".into(),
            remote: true,
            sse: true,
            target: "https://x.test/sse".into(),
            ..Default::default()
        };
        layer.op(GlobalOp::SetServer(legacy)).unwrap();
        let own = Own {
            servers: Some(vec!["github".into()]),
            skills: None,
        };
        let plan = layer.plan(&CLAUDE, &own, true);
        let how = |plan: &Plan, name: &str| {
            plan.applied
                .iter()
                .find(|a| a.name == name)
                .unwrap()
                .applies
                .clone()
        };
        assert_eq!(how(&plan, "github"), Applies::Replaced);
        assert_eq!(how(&plan, "legacy"), Applies::Active);
        assert_eq!(plan.names, ["legacy"]);
        assert!(
            matches!(how(&layer.plan(&CODEX, &Own::default(), true), "legacy"), Applies::Refused(w) if w.contains("SSE"))
        );
        layer
            .op(GlobalOp::Use {
                skill: false,
                name: "github".into(),
                agent: "codex".into(),
                on: false,
            })
            .unwrap();
        assert_eq!(
            how(&layer.plan(&CODEX, &Own::default(), true), "github"),
            Applies::Excluded
        );
        layer
            .op(GlobalOp::Use {
                skill: false,
                name: "github".into(),
                agent: "codex".into(),
                on: true,
            })
            .unwrap();
        assert_eq!(
            how(&layer.plan(&CODEX, &Own::default(), true), "github"),
            Applies::Active
        );
        // A list not read yet adds it and leaves the choice to the agent.
        assert_eq!(
            how(&layer.plan(&CLAUDE, &Own::default(), true), "github"),
            Applies::Active
        );
    }

    #[test]
    fn two_servers_asking_the_same_name_with_different_secrets_clash_in_codex() {
        let (layer, _dir, _) = layer();
        layer.op(GlobalOp::SetServer(github())).unwrap();
        let mut other = github();
        other.name = "other".into();
        other.env[0].value = Some("different".into());
        layer.op(GlobalOp::SetServer(other)).unwrap();
        let plan = layer.plan(&CODEX, &Own::default(), true);
        assert!(plan
            .applied
            .iter()
            .any(|a| matches!(&a.applies, Applies::Refused(w) if w.contains("already set"))));
        assert_eq!(layer.plan(&CLAUDE, &Own::default(), true).names.len(), 2);
    }

    #[test]
    fn a_missing_secret_refuses_the_server() {
        let (layer, _dir, secrets) = layer();
        layer.op(GlobalOp::SetServer(github())).unwrap();
        secrets.delete("mcp/github/env:GITHUB_TOKEN");
        let plan = layer.plan(&CLAUDE, &Own::default(), true);
        assert!(plan.servers.is_empty() && matches!(&plan.applied[0].applies, Applies::Refused(_)));
    }

    #[test]
    fn skills_are_listed_linked_and_replaced() {
        let (layer, dir, _) = layer();
        layer
            .op(GlobalOp::NewSkill {
                name: "release-notes".into(),
                description: "Writes notes".into(),
                instructions: String::new(),
            })
            .unwrap();
        let from = dir.0.join("elsewhere/other-skill");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(
            from.join("SKILL.md"),
            "---\nname: other-skill\ndescription: \"Another\"\n---\nbody",
        )
        .unwrap();
        layer.op(GlobalOp::ImportSkill(from.clone())).unwrap();
        assert!(layer.op(GlobalOp::ImportSkill(from)).is_err());
        assert!(layer
            .op(GlobalOp::NewSkill {
                name: "Bad Name".into(),
                description: String::new(),
                instructions: String::new(),
            })
            .is_err());
        let state = layer.state(&[CLAUDE, CODEX], &HashMap::new(), None);
        assert_eq!(
            state
                .skills
                .iter()
                .map(|s| (s.name.as_str(), s.description.as_str()))
                .collect::<Vec<_>>(),
            [
                ("other-skill", "Another"),
                ("release-notes", "Writes notes")
            ]
        );
        layer
            .op(GlobalOp::Use {
                skill: true,
                name: "other-skill".into(),
                agent: "codex".into(),
                on: false,
            })
            .unwrap();
        let own = Own {
            servers: None,
            skills: Some(vec!["release-notes".into()]),
        };
        let plan = layer.plan(&CODEX, &own, true);
        assert!(
            plan.applied.iter().all(|a| a.applies != Applies::Active),
            "{:?}",
            plan.applied
        );
        assert!(plan.skill_roots.is_empty());
        let plan = layer.plan(&CLAUDE, &Own::default(), true);
        assert!(plan.skill_roots[0].join("release-notes/SKILL.md").is_file());
        layer
            .op(GlobalOp::RemoveSkill("other-skill".into()))
            .unwrap();
        assert!(!layer
            .state(&[CLAUDE], &HashMap::new(), None)
            .skills
            .iter()
            .any(|s| s.name == "other-skill"));
    }

    /// Tool permissions are kept before they are announced, replace the one before for the
    /// same tool, go to the agent they were chosen for (and an "All agents" server's to every
    /// agent it is added to), and go away with their server.
    #[test]
    fn tool_choices_are_kept_and_reach_their_agents_sessions() {
        use newen_agent::ToolPolicy;
        let (layer, _dir, _) = layer();
        let choice = |agent: &str, server: &str, tool: &str, policy| ToolChoice {
            agent: agent.into(),
            server: server.into(),
            tool: tool.into(),
            policy,
        };
        layer
            .op(GlobalOp::Tools(vec![
                choice("claude", "sentry", "search", Some(ToolPolicy::Allow)),
                choice("claude", "sentry", "delete", Some(ToolPolicy::Ask)),
                choice("", "github", "push", Some(ToolPolicy::Block)),
            ]))
            .unwrap();
        layer
            .op(GlobalOp::Tools(vec![
                choice("claude", "sentry", "delete", Some(ToolPolicy::Block)),
                choice("claude", "sentry", "search", None),
            ]))
            .unwrap();
        let state = layer.state(&[CLAUDE], &HashMap::new(), None);
        assert_eq!(state.tools.len(), 2);
        let rules = layer.tool_rules("claude", &["github".into()]);
        let said: Vec<(&str, &str, ToolPolicy)> = (rules.iter())
            .map(|r| (r.server.as_str(), r.tool.as_str(), r.policy))
            .collect();
        assert_eq!(
            said,
            [
                ("github", "push", ToolPolicy::Block),
                ("sentry", "delete", ToolPolicy::Block)
            ]
        );
        // Not added to Codex's sessions: its rule is not given.
        assert!(layer.tool_rules("codex", &[]).is_empty());
        layer.forget_tools("claude", "sentry");
        assert_eq!(layer.tool_rules("claude", &[]).len(), 0);
    }

    /// A skill uploaded as a file is added, replaces another, and saves back as a .zip.
    #[test]
    fn a_skill_is_uploaded_replaced_and_exported() {
        let (layer, dir, _) = layer();
        let md = dir.0.join("Review.md");
        std::fs::write(&md, "---\nname: review\ndescription: Reviews\n---\nLook.").unwrap();
        layer
            .op(GlobalOp::UploadSkill {
                path: md.clone(),
                replace: None,
            })
            .unwrap();
        assert!(layer
            .op(GlobalOp::UploadSkill {
                path: md.clone(),
                replace: None
            })
            .is_err());
        std::fs::write(
            &md,
            "---\nname: other\ndescription: Better\n---\nLook again.",
        )
        .unwrap();
        layer
            .op(GlobalOp::UploadSkill {
                path: md,
                replace: Some("review".into()),
            })
            .unwrap();
        let state = layer.state(&[CLAUDE], &HashMap::new(), None);
        let names: Vec<(&str, &str)> = (state.skills.iter())
            .map(|s| (s.name.as_str(), s.description.as_str()))
            .collect();
        assert_eq!(names, [("review", "Better")]);
        let zip = dir.0.join("review.zip");
        layer
            .op(GlobalOp::ExportSkill {
                name: "review".into(),
                to: zip.clone(),
            })
            .unwrap();
        layer.op(GlobalOp::RemoveSkill("review".into())).unwrap();
        layer
            .op(GlobalOp::UploadSkill {
                path: zip,
                replace: None,
            })
            .unwrap();
        assert_eq!(
            layer.state(&[CLAUDE], &HashMap::new(), None).skills.len(),
            1
        );
    }
}
