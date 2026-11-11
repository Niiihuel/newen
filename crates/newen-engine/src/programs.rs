//! The agents' programs kept at their latest version. Each one's version is read as Newen
//! runs, after its own updater when Settings ask for it. A version that changed, by that
//! updater or by anything else (the program updating itself in a terminal), is told to the
//! UI, which tells the person; the agent's models are asked again, and its sessions start
//! again on it when Settings ask.

use crate::{adapter, run_command, Input, Internal, Router, AGENTS};
use newen_agent::{ProgramRun, SessionSpec};
use newen_model::Event;
use std::time::{Duration, Instant};

/// The first look, once Newen has started.
pub(crate) const FIRST: Duration = Duration::from_secs(20);
/// How often each version is read: an update made outside Newen is seen within this.
pub(crate) const EVERY: Duration = Duration::from_secs(30 * 60);
/// How often an agent's own updater runs.
const UPDATE_EVERY: Duration = Duration::from_secs(4 * 60 * 60);

impl Router {
    /// Read the version of every agent's program that is installed, and again after its
    /// updater when Settings ask and it is due: each is told as `Internal::AgentVersion`.
    pub(crate) fn check_agents(&mut self) {
        // The app does; the tests never run a person's programs.
        if !self.config.updates {
            return;
        }
        for agent in AGENTS {
            let Some(adapter) = adapter(agent) else {
                continue;
            };
            let program = (self.program(agent))
                .unwrap_or_else(|| adapter.launch(&SessionSpec::default()).program);
            if !newen_files::installed(&program) {
                continue;
            }
            // Never under a turn: an updater may replace files its program still reads.
            let working = (self.busy.iter())
                .any(|s| self.started.get(s).is_some_and(|new| new.agent == agent));
            let due = (self.agent_updates.get(agent)).is_none_or(|at| at.elapsed() >= UPDATE_EVERY);
            let update = (adapter.update_args())
                .filter(|_| self.settings.agents_auto_update && due && !working);
            if update.is_some() {
                self.agent_updates.insert(agent.to_owned(), Instant::now());
            }
            let run = |args: Vec<String>, timeout| ProgramRun {
                args,
                stdin: None,
                timeout,
                until: None,
            };
            let version = run(vec!["--version".into()], 20);
            let update = update.map(|args| run(args, 600));
            let read = || self.account_command(agent, &version.args);
            let (Some(before), Some(after)) = (read(), read()) else {
                continue;
            };
            let updater = (update.as_ref()).and_then(|run| self.account_command(agent, &run.args));
            let internal = self.internal.clone();
            tokio::spawn(async move {
                let tell = |said: Result<String, String>| {
                    if let Some(version) = said.ok().as_deref().and_then(version_of) {
                        let _ = internal.send(Internal::AgentVersion {
                            agent: agent.to_owned(),
                            version,
                        });
                    }
                };
                // Before the updater too: the first reading is what later ones are held against.
                tell(run_command(before, &version, agent).await);
                if let (Some(command), Some(run)) = (updater, update) {
                    // What it failed with is its own to say; the version tells what it did.
                    let _ = run_command(command, &run, agent).await;
                    tell(run_command(after, &version, agent).await);
                }
            });
        }
    }

    /// `agent`'s program is at `version`: the UI hears each one that is new. One that differs
    /// from the last reading was updated while Newen runs: its models are asked again and,
    /// when Settings ask, its running sessions start again on it (a working one after its
    /// turn).
    pub(crate) fn agent_version(&mut self, agent: String, version: String) {
        let before = self.versions.insert(agent.clone(), version.clone());
        if before.as_ref() == Some(&version) {
            return;
        }
        (self.ui)(Event::AgentVersion {
            agent: agent.clone(),
            version: version.clone(),
        });
        if before.is_none() {
            return;
        }
        self.ask_models(agent.clone());
        if self.settings.agents_auto_refresh {
            for (session, tx) in &self.sessions {
                if (self.started.get(session)).is_some_and(|new| new.agent == agent) {
                    let _ = tx.send(Input::Refresh);
                }
            }
        }
    }
}

/// The version in what a program printed for `--version`: its first word that starts with a
/// digit, without a leading `v` ("codex-cli 0.166.0", "opencode v2.2.5", "1.5.3").
pub(crate) fn version_of(stdout: &str) -> Option<String> {
    stdout
        .split_whitespace()
        .map(|word| word.strip_prefix('v').unwrap_or(word))
        .find(|word| word.starts_with(|c: char| c.is_ascii_digit()))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_read_from_what_each_program_prints() {
        for (said, version) in [
            ("2.1.322 (Claude Code)\n", "2.1.322"),
            ("codex-cli 0.166.0", "0.166.0"),
            ("opencode v2.2.5", "2.2.5"),
            ("grok 1.0.77 (50ca579fa996) [stable]", "1.0.77"),
            ("1.5.3", "1.5.3"),
        ] {
            assert_eq!(version_of(said).as_deref(), Some(version));
        }
        assert_eq!(version_of("no version here"), None);
    }

    #[test]
    fn every_agent_says_how_its_program_updates() {
        for agent in AGENTS {
            assert!(adapter(agent).unwrap().update_args().is_some(), "{agent}");
        }
    }
}
