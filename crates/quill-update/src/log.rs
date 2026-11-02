//! The update log, `<data dir>/quill/updates.log` (`newen/` once Newen moved the folder;
//! docs/UPDATES.md): one line per check,
//! download, stage, activation, rollback and error, the newest [`LINES`] kept. A line names
//! versions, sizes, times and the reason a step failed; never an address or a token.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// The lines kept; older ones go as new ones come.
pub const LINES: usize = 200;

/// Writers of this process take turns: a line is read, kept and rewritten whole.
static TURN: Mutex<()> = Mutex::new(());

/// This person's update log.
pub fn path() -> Option<PathBuf> {
    Some(quill_files::own_dir(&quill_files::data_dir()?).join("updates.log"))
}

/// Add `line` to the log at `path`, stamped with the time in UTC. A log that cannot be written
/// is left as it is: it never stops an update.
pub fn write(path: &Path, line: &str) {
    let _turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let kept = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<&str> = kept.lines().collect();
    let stamped = format!("{} {}", stamp(now()), line.replace(['\n', '\r'], " "));
    lines.push(&stamped);
    let from = lines.len().saturating_sub(LINES);
    let mut text = lines[from..].join("\n");
    text.push('\n');
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // Written beside it and renamed: a crash leaves the old log or the new one.
    let next = path.with_extension("log.new");
    if std::fs::write(&next, text).is_ok() {
        let _ = std::fs::rename(&next, path);
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// `seconds` since 1970 as `2026-11-02T14:03:11Z`.
pub(crate) fn stamp(seconds: u64) -> String {
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Howard Hinnant's civil_from_days.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_stamped_in_utc_and_only_the_newest_are_kept() {
        assert_eq!(stamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(stamp(1_793_628_191), "2026-11-02T14:03:11Z");
        assert_eq!(stamp(951_782_400), "2000-02-29T00:00:00Z");
        let folder = tempfile::tempdir().unwrap();
        let log = folder.path().join("quill/updates.log");
        for at in 0..LINES + 50 {
            write(&log, &format!("check: line {at}\nsecond"));
        }
        let text = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), LINES);
        assert!(lines[0].ends_with("check: line 50 second"), "{}", lines[0]);
        assert!(lines[LINES - 1].ends_with(&format!("check: line {} second", LINES + 49)));
        assert_eq!(lines[0].as_bytes()[10], b'T');
    }
}
