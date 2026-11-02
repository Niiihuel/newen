//! GitHub as the person's identity, through the `gh` program: who is signed in, the browser
//! sign-in, the sign-out and the contribution calendar. `gh` keeps the credentials in the
//! system's keychain; quill holds none and needs no HTTP client for the API. Only the avatar
//! is fetched, from avatars.githubusercontent.com, the way logos.rs fetches images. Everything
//! runs off the UI's thread. The calendar's validation is ported from openADE
//! (`github_contributions.rs`, the user's own code, MIT OR Apache-2.0).

use crate::{logos, Sink};
use newen_model::{ContributionCalendar, ContributionDay, Event, GithubState};
use serde_json::Value;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

const HOST: &str = "github.com";
/// Where the one-time code is typed (`gh` prints the same address).
const DEVICE_URL: &str = "https://github.com/login/device";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const INVALID_CALENDAR: &str = "GitHub returned an invalid contribution calendar";
/// An avatar is fetched again after this long.
const AVATAR_FRESH: Duration = Duration::from_secs(24 * 60 * 60);
const AVATAR_SIDE: u32 = 128;
// GitHub defaults this collection to one year ago through the current time.
// https://docs.github.com/en/graphql/reference/users#user
const CALENDAR_QUERY: &str = "query newenContributions { viewer { login contributionsCollection { \
contributionCalendar { totalContributions weeks { contributionDays { date weekday \
contributionCount contributionLevel } } } } } }";

/// How to get `gh` on this system, shown where it is missing.
pub(crate) fn install_hint() -> String {
    if cfg!(windows) {
        "GitHub CLI (gh) is not installed. Install it with: winget install GitHub.cli".into()
    } else if cfg!(target_os = "macos") {
        "GitHub CLI (gh) is not installed. Install it with: brew install gh".into()
    } else {
        "GitHub CLI (gh) is not installed. Install it with your package manager \
         (for example apt install gh, dnf install gh, pacman -S github-cli), \
         or see cli.github.com."
            .into()
    }
}

/// What a finished `gh` run printed. `None` from [`run`] means `gh` is not installed.
struct Ran {
    exit: i32,
    stdout: String,
    stderr: String,
}

impl Ran {
    /// The last line `gh` said on error, or on stdout when stderr is empty.
    fn said(&self) -> String {
        let words = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        words
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_owned()
    }
}

fn gh(args: &[&str]) -> Command {
    let mut command = Command::from(newen_files::command("gh"));
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        // Never prompt, color or check for updates: the output is read by quill.
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN");
    command
}

async fn run(mut command: Command, seconds: u64) -> Result<Option<Ran>, String> {
    let child = match command.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not start gh: {e}")),
    };
    let out = tokio::time::timeout(Duration::from_secs(seconds), child.wait_with_output())
        .await
        .map_err(|_| format!("gh took longer than {seconds}s"))?
        .map_err(|e| e.to_string())?;
    Ok(Some(Ran {
        exit: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }))
}

/// A login as GitHub allows one: letters, digits and single hyphens. It names the avatar's
/// file, so nothing else gets through.
fn valid_login(login: &str) -> bool {
    !login.is_empty()
        && login.len() <= 39
        && login
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// `gh api user`'s answer: login, name and the avatar's address.
fn parse_user(body: &str) -> Result<(String, Option<String>, Option<String>), String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "gh answered something else")?;
    let login = v["login"]
        .as_str()
        .filter(|l| valid_login(l))
        .ok_or("gh gave no login")?;
    let text = |key: &str| {
        v[key]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    Ok((login.to_owned(), text("name"), text("avatar_url")))
}

/// The avatar's address at 128 px, only when it is on avatars.githubusercontent.com over https.
fn avatar_address(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://avatars.githubusercontent.com/")?;
    let path = rest.split(['?', '#']).next()?;
    let plain = !path.is_empty()
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.'));
    plain.then(|| format!("https://avatars.githubusercontent.com/{path}?s={AVATAR_SIDE}"))
}

/// A picture of any common kind, scaled into a square at most 128 px, as PNG.
fn avatar_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let format = image::guess_format(bytes).ok()?;
    if !matches!(
        format,
        image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::WebP
            | image::ImageFormat::Gif
    ) {
        return None;
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    // A small file can decode to a huge picture: the sizes are capped before decoding.
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(64 << 20);
    reader.limits(limits);
    let picture = reader.decode().ok()?.thumbnail(AVATAR_SIDE, AVATAR_SIDE);
    let mut png = Cursor::new(Vec::new());
    picture.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(png.into_inner())
}

/// The avatar kept in `dir`, downloaded when absent or a day old. Blocking.
fn avatar(dir: &Path, login: &str, url: &str) -> Option<PathBuf> {
    let file = dir.join(format!("{login}.png"));
    let fresh = std::fs::metadata(&file)
        .and_then(|m| m.modified())
        .is_ok_and(|t| {
            SystemTime::now()
                .duration_since(t)
                .map_or(true, |a| a < AVATAR_FRESH)
        });
    if fresh {
        return Some(file);
    }
    let fetched = avatar_address(url)
        .and_then(|url| logos::get(&logos::agent(), &url, None))
        .and_then(|bytes| avatar_png(&bytes));
    match fetched {
        Some(png) => {
            std::fs::create_dir_all(dir).ok()?;
            std::fs::write(&file, png).ok()?;
            Some(file)
        }
        // A stale picture beats none.
        None => file.is_file().then_some(file),
    }
}

/// Ask `gh` who is signed in and tell the UI; the avatar follows once downloaded.
pub(crate) async fn check(avatars: Option<PathBuf>, ui: &Sink) {
    let status = run(gh(&["auth", "status", "--hostname", HOST]), 30).await;
    let status = match status {
        Ok(Some(ran)) => ran,
        Ok(None) => return ui(Event::Github(GithubState::Missing(install_hint()))),
        Err(e) => return ui(Event::Github(GithubState::Unknown(e))),
    };
    if status.exit != 0 {
        let said = format!("{}{}", status.stdout, status.stderr).to_lowercase();
        // No network is not "signed out": the person is, `gh` just cannot confirm it.
        let state = if said.contains("error connecting") || said.contains("timeout") {
            GithubState::Unknown("Could not reach GitHub".into())
        } else {
            GithubState::SignedOut
        };
        return ui(Event::Github(state));
    }
    let user = match run(gh(&["api", "user"]), 30).await {
        Ok(Some(ran)) if ran.exit == 0 => parse_user(&ran.stdout),
        Ok(Some(ran)) => Err(ran.said()),
        Ok(None) => return ui(Event::Github(GithubState::Missing(install_hint()))),
        Err(e) => Err(e),
    };
    let (login, name, url) = match user {
        Ok(user) => user,
        Err(e) => return ui(Event::Github(GithubState::Unknown(e))),
    };
    let shown = avatars
        .as_ref()
        .map(|d| d.join(format!("{login}.png")))
        .filter(|f| f.is_file());
    let signed = |avatar| {
        Event::Github(GithubState::SignedIn {
            login: login.clone(),
            name: name.clone(),
            avatar,
        })
    };
    ui(signed(shown.clone()));
    let (Some(dir), Some(url)) = (avatars, url) else {
        return;
    };
    let who = login.clone();
    let got = tokio::task::spawn_blocking(move || avatar(&dir, &who, &url)).await;
    if let Ok(Some(file)) = got {
        if shown.as_ref() != Some(&file) {
            ui(signed(Some(file)));
        }
    }
}

/// The code in a line of `gh auth login`'s output ("! First copy your one-time code: ABCD-1234").
fn parse_code(line: &str) -> Option<String> {
    let after = line.split("one-time code:").nth(1)?.trim();
    let code = after.split_whitespace().next()?;
    let ok = code.len() == 9
        && code.as_bytes()[4] == b'-'
        && code
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || b.is_ascii_alphanumeric());
    ok.then(|| code.to_owned())
}

/// Run `gh auth login --web`: report the code (and open the page), then ask who is signed in,
/// or say why not. The returned sender cancels (a message or a drop kills the process).
pub(crate) fn connect(avatars: Option<PathBuf>, ui: Sink, open_page: bool) -> UnboundedSender<()> {
    let (cancel, mut cancelled) = unbounded_channel::<()>();
    let mut command = gh(&[
        "auth",
        "login",
        "--web",
        "--hostname",
        HOST,
        "--git-protocol",
        "https",
        "--scopes",
        "read:user",
    ]);
    // quill opens the page itself.
    command.env("GH_BROWSER", "true").env("BROWSER", "true");
    tokio::spawn(async move {
        let failed = |message: String| ui(Event::GithubFailed { message });
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return failed(install_hint());
            }
            Err(e) => return failed(format!("could not start gh: {e}")),
        };
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return failed("could not read gh".into());
        };
        // Both streams into one: the code may come on either.
        let (tx, mut lines) = unbounded_channel::<String>();
        fn forward(stream: impl AsyncRead + Unpin + Send + 'static, tx: UnboundedSender<String>) {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        forward(stdout, tx.clone());
        forward(stderr, tx);
        let mut told = false;
        let mut last = String::new();
        let flow = async {
            loop {
                tokio::select! {
                    line = lines.recv() => {
                        let Some(line) = line else { break true };
                        if !told {
                            if let Some(code) = parse_code(&line) {
                                told = true;
                                if open_page {
                                    let _ = newen_files::open_in_system(DEVICE_URL);
                                }
                                ui(Event::GithubCode { code, url: DEVICE_URL.into() });
                            }
                        }
                        if !line.trim().is_empty() {
                            last = line;
                        }
                    }
                    // A cancel, or the sender dropped: the person abandoned it.
                    _ = cancelled.recv() => break false,
                }
            }
        };
        // The whole sign-in has ten minutes, the time a device code lives.
        match tokio::time::timeout(Duration::from_secs(600), flow).await {
            Ok(true) => {}
            Ok(false) => return drop(child.kill().await),
            Err(_) => {
                let _ = child.kill().await;
                return failed("Sign-in took too long".into());
            }
        }
        if child.wait().await.is_ok_and(|s| s.success()) {
            return check(avatars, &ui).await;
        }
        failed(if last.trim().is_empty() {
            "Sign-in failed".into()
        } else {
            last.trim().to_owned()
        });
    });
    cancel
}

/// Sign `login` out of `gh`, then ask who is signed in (another account may take its place).
pub(crate) async fn disconnect(login: String, avatars: Option<PathBuf>, ui: &Sink) {
    if !valid_login(&login) {
        return;
    }
    let ran = run(
        gh(&["auth", "logout", "--hostname", HOST, "--user", &login]),
        30,
    )
    .await;
    if let Ok(Some(ran)) = &ran {
        if ran.exit != 0 {
            ui(Event::GithubFailed {
                message: ran.said(),
            });
        }
    }
    check(avatars, ui).await;
}

/// Fetch and validate the calendar of `login`.
pub(crate) async fn contributions(login: String, ui: &Sink) {
    let result = load(&login).await;
    ui(Event::GithubContributions(result));
}

async fn load(login: &str) -> Result<ContributionCalendar, String> {
    let query = format!("query={CALENDAR_QUERY}");
    let ran = run(gh(&["api", "graphql", "-f", &query]), 30)
        .await?
        .ok_or_else(install_hint)?;
    if ran.exit != 0 {
        let said = ran.said().to_lowercase();
        return Err(
            if said.contains("401") || said.contains("auth") || said.contains("token") {
                "GitHub's authorization expired. Reconnect your account.".into()
            } else if said.contains("403") || said.contains("429") || said.contains("rate limit") {
                "GitHub did not allow the request, or the rate limit was reached.".into()
            } else {
                "Could not ask GitHub for the contributions".into()
            },
        );
    }
    if ran.stdout.len() > MAX_RESPONSE_BYTES {
        return Err("GitHub's calendar exceeds the size limit".into());
    }
    parse_calendar(&ran.stdout, login)
}

/// Days since 1970-01-01 of a proleptic Gregorian date; `None` for a day that does not exist.
fn days_from_civil(y: i64, m: i64, d: i64) -> Option<i64> {
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&m) || d < 1 || d > month_days[(m - 1) as usize] {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// The day number of a `YYYY-MM-DD` date.
fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || d.len() != 2 {
        return None;
    }
    days_from_civil(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)
}

/// The calendar `gh api graphql` printed, validated: the account is `login`'s, there are no
/// GraphQL errors, and weeks and days are consecutive, Sunday-aligned and within a year.
pub(crate) fn parse_calendar(body: &str, login: &str) -> Result<ContributionCalendar, String> {
    let invalid = || INVALID_CALENDAR.to_owned();
    let v: Value = serde_json::from_str(body).map_err(|_| invalid())?;
    if v["errors"].as_array().is_some_and(|e| !e.is_empty()) {
        // GraphQL can answer with errors: no partial data, and no server words repeated.
        return Err("GitHub could not deliver the calendar. Check your account's access.".into());
    }
    let viewer = &v["data"]["viewer"];
    let viewer_login = viewer["login"].as_str().ok_or_else(invalid)?;
    if !viewer_login.eq_ignore_ascii_case(login) {
        return Err(
            "The credential in use belongs to another account. Reconnect the selected account."
                .into(),
        );
    }
    let calendar = &viewer["contributionsCollection"]["contributionCalendar"];
    let total = calendar["totalContributions"]
        .as_u64()
        .ok_or_else(invalid)? as u32;
    let raw_weeks = calendar["weeks"].as_array().ok_or_else(invalid)?;
    if raw_weeks.is_empty() || raw_weeks.len() > 54 {
        return Err(invalid());
    }
    let mut weeks = Vec::with_capacity(raw_weeks.len());
    let (mut previous_day, mut previous_week, mut first_day) =
        (None::<i64>, None::<i64>, None::<i64>);
    for week in raw_weeks {
        let days = week["contributionDays"].as_array().ok_or_else(invalid)?;
        if days.is_empty() || days.len() > 7 {
            return Err(invalid());
        }
        let mut row: [Option<ContributionDay>; 7] = Default::default();
        let mut week_start = None;
        for day in days {
            let date = day["date"].as_str().ok_or_else(invalid)?;
            let number = day_number(date).ok_or_else(invalid)?;
            let weekday = day["weekday"].as_u64().ok_or_else(invalid)? as usize;
            let count = u32::try_from(day["contributionCount"].as_u64().ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
            let level = match day["contributionLevel"].as_str().ok_or_else(invalid)? {
                "NONE" => 0,
                "FIRST_QUARTILE" => 1,
                "SECOND_QUARTILE" => 2,
                "THIRD_QUARTILE" => 3,
                "FOURTH_QUARTILE" => 4,
                _ => return Err(invalid()),
            };
            // 1970-01-01 was a Thursday; weeks run from Sunday.
            let computed = (number + 4).rem_euclid(7) as usize;
            if weekday >= 7 || computed != weekday || previous_day.is_some_and(|p| p + 1 != number)
            {
                return Err(invalid());
            }
            let sunday = number - weekday as i64;
            if week_start.is_some_and(|s| s != sunday) {
                return Err(invalid());
            }
            week_start = Some(sunday);
            first_day.get_or_insert(number);
            previous_day = Some(number);
            row[weekday] = Some(ContributionDay {
                date: date.to_owned(),
                count,
                level,
            });
        }
        if previous_week.is_some_and(|p| Some(p + 7) != week_start) {
            return Err(invalid());
        }
        previous_week = week_start;
        weeks.push(row);
    }
    // GitHub aligns the first week to Sunday, which can add up to six days to the one-year
    // (possibly leap) period.
    match first_day.zip(previous_day) {
        Some((first, last)) if last - first <= 366 + 6 => {}
        _ => return Err(invalid()),
    }
    Ok(ContributionCalendar {
        login: viewer_login.to_owned(),
        total,
        weeks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({ "data": { "viewer": {
            "login": "octocat",
            "contributionsCollection": { "contributionCalendar": {
                "totalContributions": 8,
                "weeks": [
                    { "contributionDays": [{ "date": "2026-09-05", "weekday": 6,
                        "contributionCount": 0, "contributionLevel": "NONE" }] },
                    { "contributionDays": [
                        { "date": "2026-09-06", "weekday": 0, "contributionCount": 3,
                          "contributionLevel": "SECOND_QUARTILE" },
                        { "date": "2026-09-07", "weekday": 1, "contributionCount": 5,
                          "contributionLevel": "FOURTH_QUARTILE" }
                    ] }
                ]
            } }
        } } })
    }

    fn parse(v: &Value, login: &str) -> Result<ContributionCalendar, String> {
        parse_calendar(&v.to_string(), login)
    }

    #[test]
    fn calendar_keeps_partial_weeks_zero_days_and_api_intensities() {
        let calendar = parse(&fixture(), "Octocat").unwrap();
        assert_eq!(calendar.total, 8);
        assert!(calendar.weeks[0][0].is_none());
        assert_eq!(calendar.weeks[0][6].as_ref().unwrap().level, 0);
        assert_eq!(calendar.weeks[1][0].as_ref().unwrap().level, 2);
        assert_eq!(calendar.weeks[1][1].as_ref().unwrap().level, 4);
        assert!(calendar.weeks[1][2].is_none());
    }

    #[test]
    fn calendar_rejects_other_accounts_errors_and_invalid_days() {
        assert!(parse(&fixture(), "other-account").is_err());
        let mut value = fixture();
        value["errors"] = json!([{ "message": "private server detail" }]);
        let error = parse(&value, "octocat").unwrap_err();
        assert!(!error.contains("private server detail"));
        for (field, invalid) in [
            ("date", json!("2026-09-99")),
            ("weekday", json!(5)),
            ("contributionLevel", json!("UNKNOWN")),
            ("contributionCount", json!(-1)),
        ] {
            let mut value = fixture();
            value["data"]["viewer"]["contributionsCollection"]["contributionCalendar"]["weeks"]
                [0]["contributionDays"][0][field] = invalid;
            assert!(parse(&value, "octocat").is_err(), "{field}");
        }
        let mut value = fixture();
        value["data"]["viewer"]["contributionsCollection"]["contributionCalendar"]["weeks"][1]
            ["contributionDays"][0]["date"] = json!("2026-09-13");
        assert!(parse(&value, "octocat").is_err());
        assert!(parse_calendar("not json", "octocat").is_err());
    }

    #[test]
    fn days_are_counted_from_the_epoch() {
        assert_eq!(day_number("1970-01-01"), Some(0));
        assert_eq!(
            day_number("2024-02-29"),
            day_number("2024-02-28").map(|d| d + 1)
        );
        assert_eq!(day_number("2025-02-29"), None);
        assert_eq!(day_number("2026-9-05"), None);
    }

    #[test]
    fn user_comes_from_gh_api_user() {
        let body = r#"{"avatar_url":"https://avatars.githubusercontent.com/u/1?v=4","login":"Niiihuel","name":"Nihuel Prieto"}"#;
        let (login, name, url) = parse_user(body).unwrap();
        assert_eq!(
            (login.as_str(), name.as_deref()),
            ("Niiihuel", Some("Nihuel Prieto"))
        );
        assert_eq!(
            avatar_address(&url.unwrap()).as_deref(),
            Some("https://avatars.githubusercontent.com/u/1?s=128")
        );
        // No name is none; a login that could escape the data folder is refused.
        assert_eq!(parse_user(r#"{"login":"a","name":null}"#).unwrap().1, None);
        assert!(parse_user(r#"{"login":"../x"}"#).is_err());
        assert!(parse_user("[]").is_err());
    }

    #[test]
    fn only_github_avatars_over_https_are_fetched() {
        assert!(avatar_address("http://avatars.githubusercontent.com/u/1").is_none());
        assert!(avatar_address("https://evil.com/avatars.githubusercontent.com/u/1").is_none());
        assert!(avatar_address("https://avatars.githubusercontent.com.evil.com/u/1").is_none());
        assert!(avatar_address("https://avatars.githubusercontent.com/u/1 2").is_none());
    }

    #[test]
    fn the_login_code_is_read_from_gh_words() {
        assert_eq!(
            parse_code("! First copy your one-time code: 1A2B-3C4D").as_deref(),
            Some("1A2B-3C4D")
        );
        assert_eq!(
            parse_code("- Press Enter to open github.com in your browser..."),
            None
        );
        assert_eq!(parse_code("one-time code: nope"), None);
    }

    #[test]
    fn a_missing_gh_says_how_to_install_it() {
        assert!(install_hint().contains("gh"));
    }
}
