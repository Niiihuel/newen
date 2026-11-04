//! A provider's sign-in (docs/opencode.md "Proveedores"): its first step, then the page opened
//! in the browser and the attempt followed until it is done, fails, takes too long or the
//! person cancels it. Each step is a run of the agent's program, which its adapter writes and
//! reads; `step` runs it.

use newen_agent::{AgentAdapter, ProgramRun, SignInState, SignInStep};
use newen_model::ProviderSignIn;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The attempt under way for each agent's provider: a cancel takes it away, and the follow-up
/// stops once it is not its own.
pub(crate) type Attempts = Arc<Mutex<HashMap<(String, String), String>>>;

/// How often an attempt is asked how it stands, and for how long at most.
pub(crate) const EVERY: Duration = Duration::from_secs(2);
pub(crate) const WITHIN: Duration = Duration::from_secs(15 * 60);

/// A sign-in to `provider` by `method`: what it tells the person as it goes (`tell`), the page
/// it opens (`open`, https only); whether it ended connected. The adapter is `Send`, not
/// `Sync`: it is held behind a lock, taken for a moment and never across an await.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn follow<R, F>(
    adapter: &Mutex<Box<dyn AgentAdapter>>,
    key: (String, String),
    method: &str,
    answers: &[(String, String)],
    attempts: &Attempts,
    (every, within): (Duration, Duration),
    step: R,
    open: impl Fn(&str),
    tell: impl Fn(ProviderSignIn),
) -> bool
where
    R: Fn(ProgramRun) -> F,
    F: Future<Output = Result<String, String>>,
{
    let provider = key.1.as_str();
    let read = |out: Result<String, String>| match out {
        Ok(printed) => adapter.lock().unwrap().provider_sign_in_read(&printed),
        Err(why) => SignInState::Failed(why),
    };
    let run = |step: SignInStep| adapter.lock().unwrap().provider_sign_in_run(provider, step);
    let failed = |why: String| {
        tell(ProviderSignIn::Failed(why));
        false
    };
    let Some(begin) = run(SignInStep::Begin { method, answers }) else {
        return failed("This way of signing in is not offered.".into());
    };
    let (attempt, url, instructions, code) = match read(step(begin).await) {
        SignInState::Failed(why) => return failed(why),
        // Credentials this computer has: connected at once.
        SignInState::Done | SignInState::Pending => {
            tell(ProviderSignIn::Done);
            return true;
        }
        SignInState::Waiting {
            attempt,
            url,
            instructions,
            code,
        } => (attempt, url, instructions, code),
    };
    if let Some(page) = url.as_deref().filter(|u| u.starts_with("https://")) {
        open(page);
    }
    attempts
        .lock()
        .unwrap()
        .insert(key.clone(), attempt.clone());
    tell(ProviderSignIn::Waiting {
        url,
        instructions,
        code,
    });
    let ours = || attempts.lock().unwrap().get(&key) == Some(&attempt);
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        tokio::time::sleep(every).await;
        // Cancelled, or a new attempt took its place.
        if !ours() {
            return false;
        }
        let Some(check) = run(SignInStep::Check { attempt: &attempt }) else {
            break;
        };
        let state = read(step(check).await);
        if !ours() {
            return false;
        }
        match state {
            SignInState::Pending | SignInState::Waiting { .. } => continue,
            SignInState::Done => {
                attempts.lock().unwrap().remove(&key);
                tell(ProviderSignIn::Done);
                return true;
            }
            SignInState::Failed(why) => {
                attempts.lock().unwrap().remove(&key);
                return failed(why);
            }
        }
    }
    attempts.lock().unwrap().remove(&key);
    failed("The sign-in took too long. Try again.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// What OpenCode's service answered on 2026-11-04, in order.
    const STARTED: &str = r#"{"data":{"attemptID":"con_1","url":"https://github.com/login/device","instructions":"Enter code: 8787-8C73","mode":"auto"}}"#;
    const PENDING: &str = r#"{"data":{"status":"pending"}}"#;
    const COMPLETE: &str = r#"{"data":{"status":"complete"}}"#;

    struct Seen {
        told: Mutex<Vec<ProviderSignIn>>,
        opened: Mutex<Vec<String>>,
        asked: Mutex<Vec<Vec<String>>>,
    }

    async fn sign_in(answers: Vec<&str>, attempts: &Attempts, seen: &Seen) -> bool {
        let opencode: Box<dyn AgentAdapter> = Box::new(newen_agent_opencode::OpenCode::new());
        let opencode = Mutex::new(opencode);
        let answers = Mutex::new(
            answers
                .into_iter()
                .map(str::to_owned)
                .collect::<VecDeque<_>>(),
        );
        let key = ("opencode".to_owned(), "github-copilot".to_owned());
        let form = [("deploymentType".to_owned(), "github.com".to_owned())];
        let quick = (Duration::from_millis(1), Duration::from_secs(5));
        follow(
            &opencode,
            key,
            "oauth:device",
            &form,
            attempts,
            quick,
            |run: ProgramRun| {
                seen.asked.lock().unwrap().push(run.args);
                let answer = answers.lock().unwrap().pop_front();
                async move { answer.ok_or_else(|| "no answer".to_owned()) }
            },
            |page| seen.opened.lock().unwrap().push(page.to_owned()),
            |state| seen.told.lock().unwrap().push(state),
        )
        .await
    }

    fn seen() -> Seen {
        Seen {
            told: Mutex::new(vec![]),
            opened: Mutex::new(vec![]),
            asked: Mutex::new(vec![]),
        }
    }

    #[tokio::test]
    async fn a_sign_in_opens_its_page_and_is_done_once_its_attempt_completes() {
        let (attempts, seen) = (Attempts::default(), seen());
        let connected = sign_in(vec![STARTED, PENDING, PENDING, COMPLETE], &attempts, &seen).await;
        assert!(connected);
        assert_eq!(
            *seen.opened.lock().unwrap(),
            ["https://github.com/login/device"]
        );
        assert_eq!(
            *seen.told.lock().unwrap(),
            [
                ProviderSignIn::Waiting {
                    url: Some("https://github.com/login/device".into()),
                    instructions: Some("Enter code: 8787-8C73".into()),
                    code: false,
                },
                ProviderSignIn::Done
            ]
        );
        assert_eq!(
            seen.asked.lock().unwrap().len(),
            4,
            "begun, then asked three times"
        );
        assert!(attempts.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_cancelled_sign_in_stops_following_and_says_nothing_more() {
        let (attempts, seen) = (Attempts::default(), seen());
        let cancel = {
            let attempts = attempts.clone();
            async move {
                // Once it waits, the person cancels: the attempt is taken away.
                while attempts.lock().unwrap().is_empty() {
                    tokio::task::yield_now().await;
                }
                attempts.lock().unwrap().clear();
            }
        };
        let many = vec![
            STARTED, PENDING, PENDING, PENDING, PENDING, PENDING, PENDING,
        ];
        let (connected, ()) = tokio::join!(sign_in(many, &attempts, &seen), cancel);
        assert!(!connected);
        assert_eq!(seen.told.lock().unwrap().len(), 1, "only that it waited");
    }

    #[tokio::test]
    async fn a_refused_sign_in_says_why() {
        let (attempts, seen) = (Attempts::default(), seen());
        let refused = r#"{"_tag":"InvalidRequestError","message":"Missing required form field: deploymentType"}"#;
        assert!(!sign_in(vec![refused], &attempts, &seen).await);
        assert_eq!(
            *seen.told.lock().unwrap(),
            [ProviderSignIn::Failed(
                "Missing required form field: deploymentType".into()
            )]
        );
        assert!(seen.opened.lock().unwrap().is_empty());
    }
}
