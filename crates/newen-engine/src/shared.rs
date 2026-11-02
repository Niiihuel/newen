//! One process an agent's sessions share, when its adapter splits it among them
//! (`AgentAdapter::mux`, docs/remote-control.md §3). It starts with the first session that
//! needs it, lives while quill runs, and starts again after it ends. Each session reads and
//! writes through it as if the process were its own: its lines carry the session's launch
//! (`generation`), as an own process's do.

use crate::{Input, Running};
use newen_agent::{AgentAdapter, SessionSpec};
use newen_model::SessionId;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

/// A session's way to the shared process.
#[derive(Clone)]
pub(crate) struct Shared {
    tx: UnboundedSender<Op>,
}

enum Op {
    /// The session joins under its launch's `generation`; its lines go to `inbox`.
    Attach {
        session: SessionId,
        generation: u64,
        inbox: UnboundedSender<Input>,
    },
    Line {
        session: SessionId,
        line: String,
    },
    Detach {
        session: SessionId,
    },
}

impl Shared {
    /// The agent's shared process: `spec` gives its launch and its mux, `env` what its
    /// environment adds (the agent's variables and its servers' secrets, every session's).
    pub(crate) fn start(
        adapter: Box<dyn AgentAdapter>,
        spec: SessionSpec,
        program: Option<String>,
        env: Vec<(String, String)>,
    ) -> Self {
        let (tx, rx) = unbounded_channel();
        tokio::spawn(run(adapter, spec, program, env, rx));
        Self { tx }
    }

    pub(crate) fn attach(
        &self,
        session: SessionId,
        generation: u64,
        inbox: UnboundedSender<Input>,
    ) {
        let _ = self.tx.send(Op::Attach {
            session,
            generation,
            inbox,
        });
    }

    pub(crate) fn line(&self, session: SessionId, line: String) {
        let _ = self.tx.send(Op::Line { session, line });
    }

    pub(crate) fn detach(&self, session: SessionId) {
        let _ = self.tx.send(Op::Detach { session });
    }
}

async fn run(
    adapter: Box<dyn AgentAdapter>,
    spec: SessionSpec,
    program: Option<String>,
    env: Vec<(String, String)>,
    mut ops: UnboundedReceiver<Op>,
) {
    let mut sessions: HashMap<SessionId, (u64, UnboundedSender<Input>)> = HashMap::new();
    let mut process: Option<(Running, ChildStdin)> = None;
    let mut mux = adapter.mux(&spec);
    // What the process says, a line at a time; `None` once it ended (by its launch).
    let (said, mut hear) = unbounded_channel::<(u64, Option<String>)>();
    let mut launch = 0u64;
    loop {
        tokio::select! {
            op = ops.recv() => {
                let Some(op) = op else { return };
                let session = match &op {
                    Op::Attach { session, .. } | Op::Line { session, .. } | Op::Detach { session } => *session,
                };
                if let Op::Detach { .. } = op {
                    sessions.remove(&session);
                    if let Some(mux) = mux.as_mut() {
                        mux.closed(session);
                    }
                    continue;
                }
                if process.is_none() {
                    launch += 1;
                    mux = adapter.mux(&spec);
                    match spawn(adapter.as_ref(), &spec, program.as_deref(), &env, launch, said.clone()) {
                        Ok(started) => process = Some(started),
                        Err(error) => {
                            eprintln!("quill: {error}");
                            if let Op::Attach { generation, inbox, .. } = op {
                                let _ = inbox.send(Input::Exited { generation, code: Some(127) });
                            }
                            continue;
                        }
                    }
                }
                match op {
                    Op::Attach { generation, inbox, .. } => {
                        sessions.insert(session, (generation, inbox));
                    }
                    Op::Line { line, .. } => {
                        let Some(mux) = mux.as_mut() else { continue };
                        let (out, back) = mux.outbound(session, &line);
                        if let Some((generation, inbox)) = sessions.get(&session) {
                            for line in back {
                                let _ = inbox.send(Input::Line { generation: *generation, line });
                            }
                        }
                        if let Some((_, stdin)) = process.as_mut() {
                            for line in out {
                                if stdin.write_all(format!("{line}\n").as_bytes()).await.is_err() {
                                    break;
                                }
                            }
                            let _ = stdin.flush().await;
                        }
                    }
                    Op::Detach { .. } => {}
                }
            }
            heard = hear.recv() => {
                let Some((from, line)) = heard else { return };
                if from != launch {
                    continue;
                }
                match line {
                    Some(line) => {
                        let Some(mux) = mux.as_mut() else { continue };
                        for (session, line) in mux.inbound(&line) {
                            if let Some((generation, inbox)) = sessions.get(&session) {
                                let _ = inbox.send(Input::Line { generation: *generation, line });
                            }
                        }
                    }
                    // It ended: every session's agent did, and the next one to ask starts it again.
                    None => {
                        process = None;
                        for (_, (generation, inbox)) in sessions.drain() {
                            let _ = inbox.send(Input::Exited { generation, code: None });
                        }
                    }
                }
            }
        }
    }
}

/// Start the process, its stdout told on `said` under `launch`.
fn spawn(
    adapter: &dyn AgentAdapter,
    spec: &SessionSpec,
    program: Option<&str>,
    env: &[(String, String)],
    launch: u64,
    said: UnboundedSender<(u64, Option<String>)>,
) -> Result<(Running, ChildStdin), String> {
    let plan = adapter.launch(spec);
    let mut command = Command::from(newen_files::command(program.unwrap_or(&plan.program)));
    command
        .args(&plan.args)
        .current_dir(&plan.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command.envs(plan.env.iter().map(|(k, v)| (k, v)));
    for key in &plan.env_remove {
        command.env_remove(key);
    }
    command.envs(env.iter().map(|(k, v)| (k, v)));
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", plan.program))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stdin = child.stdin.take().expect("piped stdin");
    tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if said.send((launch, Some(line))).is_err() {
                return;
            }
        }
        let _ = said.send((launch, None));
    });
    // stderr is drained so the process never blocks on a full pipe.
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(_)) = lines.next_line().await {}
    });
    Ok((Running(child), stdin))
}
