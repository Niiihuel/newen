//! Newen's own shell integration for bash and zsh, as engine.md §9.2 has it: an rcfile (bash)
//! or a `ZDOTDIR` (zsh) that loads the person's configuration first and then marks where each
//! command starts (OSC 133 C) and ends, with its exit code (D), and each prompt (A). fish 4
//! marks its commands itself, so it is run as it is.

use std::path::{Path, PathBuf};

/// How a shell is started with the marks.
pub(crate) enum Launch {
    Args(Vec<String>),
    Env(Vec<(String, String)>),
}

const BASH: &str = r#"# Newen's marks for an agent's terminal (docs/terminal.md). The person's own first.
[ -f "$HOME/.bashrc" ] && . "$HOME/.bashrc"
__newen_ran=
__newen_prompting=
__newen_preexec() {
  [ -n "$__newen_prompting" ] && return
  case "$BASH_COMMAND" in __newen_*) return ;; esac
  [ -n "$__newen_ran" ] && return
  __newen_ran=1
  printf '\033]133;C\007'
}
__newen_precmd() {
  local status=$?
  __newen_prompting=1
  [ -n "$__newen_ran" ] && printf '\033]133;D;%s\007' "$status"
  __newen_ran=
  printf '\033]133;A\007'
}
__newen_ready() { __newen_prompting=; }
trap '__newen_preexec' DEBUG
PROMPT_COMMAND="__newen_precmd;${PROMPT_COMMAND:+$PROMPT_COMMAND;}__newen_ready"
"#;

const ZSHENV: &str = r#"# Newen's marks for an agent's terminal (docs/terminal.md): the person's own files first.
ZDOTDIR="${NEWEN_ZDOTDIR:-$HOME}"
[ -f "$ZDOTDIR/.zshenv" ] && . "$ZDOTDIR/.zshenv"
NEWEN_ZDOTDIR="$ZDOTDIR"
ZDOTDIR="@NEWEN_DIR@"
"#;

const ZSHRC: &str = r#"ZDOTDIR="$NEWEN_ZDOTDIR"
[ -f "$ZDOTDIR/.zshrc" ] && . "$ZDOTDIR/.zshrc"
__newen_ran=
__newen_preexec() { __newen_ran=1; print -n '\e]133;C\a' }
__newen_precmd() {
  local exit_status=$?
  [[ -n $__newen_ran ]] && print -n "\e]133;D;$exit_status\a"
  __newen_ran=
  print -n '\e]133;A\a'
}
preexec_functions=(__newen_preexec $preexec_functions)
precmd_functions=(__newen_precmd $precmd_functions)
"#;

/// The marks for `shell`, written where it reads them; `None` for a shell that marks its
/// commands itself or that Newen has no script for.
pub(crate) fn prepare(shell: &str) -> Option<Launch> {
    let name = Path::new(shell).file_name()?.to_str()?;
    let dir = scripts_dir();
    match name {
        "bash" => {
            let rc = dir.join("bashrc");
            write(&rc, BASH)?;
            Some(Launch::Args(vec![
                "--rcfile".into(),
                rc.display().to_string(),
                "-i".into(),
            ]))
        }
        "zsh" => {
            let zdot = dir.join("zsh");
            std::fs::create_dir_all(&zdot).ok()?;
            // Back to Newen's folder, for zsh to read its .zshrc next.
            let zshenv = ZSHENV.replace("@NEWEN_DIR@", &zdot.display().to_string());
            write(&zdot.join(".zshenv"), &zshenv)?;
            write(&zdot.join(".zshrc"), ZSHRC)?;
            let theirs = std::env::var("ZDOTDIR")
                .ok()
                .or_else(|| std::env::var("HOME").ok())
                .unwrap_or_default();
            Some(Launch::Env(vec![
                ("ZDOTDIR".into(), zdot.display().to_string()),
                ("NEWEN_ZDOTDIR".into(), theirs),
            ]))
        }
        _ => None,
    }
}

/// Where the scripts are kept while Newen runs: its own folder in the temporary one.
fn scripts_dir() -> PathBuf {
    std::env::temp_dir().join(format!("newen-shell-{}", std::process::id()))
}

fn write(path: &Path, text: &str) -> Option<()> {
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(path, text).ok()
}
