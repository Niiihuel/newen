//! The new-session page's greeting: quill's own words, by the local hour and day. One is
//! picked from the day's date and the part of the day, so it holds still through a frame, a
//! session and a day's part, and changes with the next. Inferred, not Desktop's.

/// A greeting's stem and what ends it: "Good morning" + "" or "Weekend project" + "?".
type Line = (&'static str, &'static str);

const MORNING: [Line; 3] = [("Good morning", ""), ("Morning", ""), ("Fresh start", "")];
const AFTERNOON: [Line; 3] = [
    ("Good afternoon", ""),
    ("Afternoon", ""),
    ("Back at it", "?"),
];
const EVENING: [Line; 3] = [
    ("Good evening", ""),
    ("Evening", ""),
    ("Evening session", "?"),
];
/// Midnight to five.
const NIGHT: [Line; 3] = [
    ("Burning the midnight oil", "?"),
    ("Still up", "?"),
    ("Night owl", "?"),
];

/// The greeting at `now` (seconds since 1970) in a zone `offset` seconds from UTC.
pub fn at(now: u64, offset: i64, name: Option<&str>) -> String {
    let local = now as i64 + offset;
    let day = local.div_euclid(86_400);
    let hour = local.rem_euclid(86_400) / 3_600;
    // 1 January 1970 was a Thursday; 0 is Sunday.
    let weekday = (day + 4).rem_euclid(7);
    let (part, mut pool): (u64, Vec<Line>) = match hour {
        0..=4 => (0, NIGHT.to_vec()),
        5..=11 => (1, MORNING.to_vec()),
        12..=16 => (2, AFTERNOON.to_vec()),
        _ => (3, EVENING.to_vec()),
    };
    // A day's touch joins the pool, but a small hour keeps its own words.
    if part != 0 {
        match weekday {
            1 => pool.push(("Fresh week", "")),
            5 => pool.push(("Happy Friday", "")),
            0 | 6 => pool.push(("Weekend project", "?")),
            _ => {}
        }
    }
    let seed = (day as u64).wrapping_mul(0x9E37_79B1).wrapping_add(part);
    let (stem, end) = pool[(seed >> 5) as usize % pool.len()];
    match name {
        Some(name) => format!("{stem}, {name}{end}"),
        None => format!("{stem}{end}"),
    }
}

/// The greeting now, by the local clock.
pub fn now(offset: i64, name: Option<&str>) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    at(secs, offset, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Midnight UTC of Friday 2026-10-30, plus `hour`.
    fn friday(hour: u64) -> u64 {
        1_793_318_400 + hour * 3_600
    }

    fn words(greeting: &str) -> &str {
        greeting.split(',').next().unwrap()
    }

    #[test]
    fn the_part_of_the_day_sets_the_pool_and_its_edges() {
        let in_pool = |hour, pool: &[Line]| {
            let said = at(friday(hour), 0, Some("Ana"));
            pool.iter().any(|(stem, _)| words(&said) == *stem) || words(&said) == "Happy Friday"
        };
        // The night is 0 to 5 and never takes a day's touch.
        for hour in [0, 4] {
            assert!(NIGHT
                .iter()
                .any(|(s, _)| words(&at(friday(hour), 0, None)).starts_with(s)));
        }
        assert!(in_pool(5, &MORNING) && in_pool(11, &MORNING));
        assert!(in_pool(12, &AFTERNOON) && in_pool(16, &AFTERNOON));
        assert!(in_pool(17, &EVENING) && in_pool(23, &EVENING));
        assert!(!NIGHT.iter().any(|(s, _)| *s == "Happy Friday"));
    }

    #[test]
    fn it_holds_still_within_a_part_of_the_day_and_follows_the_zone() {
        let a = at(friday(13), 0, Some("Ana"));
        assert_eq!(a, at(friday(15) + 59, 0, Some("Ana")));
        // 02:00 UTC is still Thursday night three hours west: the local clock rules.
        assert!(at(friday(2), -3 * 3_600, None).ends_with('?'));
        assert_ne!(
            at(friday(2), -3 * 3_600, None),
            at(friday(2), 12 * 3_600, None)
        );
    }

    #[test]
    fn the_days_touches_come_up_and_a_missing_name_leaves_no_comma() {
        let week = |from: u64, hour: u64| {
            (0..140).map(move |d| at(from + d * 86_400 + hour * 3_600, 0, None))
        };
        assert!(week(friday(0), 9).any(|g| g == "Happy Friday"));
        assert!(week(friday(0), 9).any(|g| g == "Fresh week"));
        assert!(week(friday(0), 9).any(|g| g == "Weekend project?"));
        assert!(week(friday(0), 9).all(|g| !g.contains(',')));
        assert!(at(friday(0) + 9 * 3_600, 0, Some("Ana")).contains(", Ana"));
    }
}
