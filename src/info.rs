//! The palette's info panel. Identity comes from CI's build stamp and the running
//! process; a value that is missing or unreadable says so on screen.
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEVELOPMENT: &str = "development build, not from CI";
const NO_CHECKOUT: &str = "no checkout on this PC";
const REPOS: &str = r"D:\repos";

#[derive(Clone, Copy)]
struct Stamp<'a> {
    repo: Option<&'a str>,
    server: Option<&'a str>,
    commit: Option<&'a str>,
    built: Option<&'a str>,
}

const BUILD: Stamp = Stamp {
    repo: option_env!("GITHUB_REPOSITORY"),
    server: option_env!("GITHUB_SERVER_URL"),
    commit: option_env!("SHOWS_BUILD_SHA"),
    built: option_env!("SHOWS_BUILD_TIME"),
};

fn present(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn civil_days(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146097 + day_of_era - 719468
}

/// (year, month, day) for a count of days since 1970-01-01.
fn civil_date(days: i64) -> (i64, i64, i64) {
    let days = days + 719468;
    let era = days.div_euclid(146097);
    let day_of_era = days.rem_euclid(146097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let month = (shifted_month + 2) % 12 + 1;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day_of_year - (153 * shifted_month + 2) / 5 + 1)
}

fn sunday_on_or_after(year: i64, month: i64, day: i64) -> i64 {
    let days = civil_days(year, month, day);
    // 1970-01-01 was a Thursday.
    days + (7 - (days + 4).rem_euclid(7)) % 7
}

/// Daylight time runs from 2 AM on March's second Sunday to 2 AM on November's first.
fn pacific_offset(unix: i64) -> i64 {
    const STANDARD: i64 = -8 * 3600;
    const DAYLIGHT: i64 = -7 * 3600;
    let (year, ..) = civil_date((unix + STANDARD).div_euclid(86400));
    let start = sunday_on_or_after(year, 3, 8) * 86400 + 2 * 3600 - STANDARD;
    let end = sunday_on_or_after(year, 11, 1) * 86400 + 2 * 3600 - DAYLIGHT;
    if (start..end).contains(&unix) {
        DAYLIGHT
    } else {
        STANDARD
    }
}

fn pacific(unix: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let local = unix + pacific_offset(unix);
    let (_, month, day) = civil_date(local.div_euclid(86400));
    let seconds = local.rem_euclid(86400);
    let (hour, minute) = (seconds / 3600, seconds % 3600 / 60);
    let hour12 = if hour % 12 == 0 { 12 } else { hour % 12 };
    let meridiem = if hour < 12 { "AM" } else { "PM" };
    format!(
        "{} {day}, {hour12}:{minute:02} {meridiem} Pacific",
        MONTHS[month as usize - 1]
    )
}

fn elapsed(seconds: i64) -> String {
    let ago =
        |count: i64, unit: &str| format!("{count} {unit}{} ago", if count == 1 { "" } else { "s" });
    match seconds {
        i64::MIN..-300 => "this PC's clock is behind the build".into(),
        -300..60 => "just now".into(),
        60..3600 => ago(seconds / 60, "minute"),
        3600..86400 => ago(seconds / 3600, "hour"),
        _ => ago(seconds / 86400, "day"),
    }
}

fn built(stamp: Option<&str>, now: i64) -> String {
    let Some(stamp) = present(stamp) else {
        return DEVELOPMENT.into();
    };
    match stamp.parse::<i64>() {
        Ok(unix) if (0..=253_402_300_799).contains(&unix) => {
            format!("{} ({})", pacific(unix), elapsed(now - unix))
        }
        _ => format!("unreadable build time stamp: {stamp}"),
    }
}

fn installed(path: io::Result<PathBuf>) -> String {
    match path {
        Ok(path) => path.display().to_string(),
        Err(error) => format!("could not read the running executable's path: {error}"),
    }
}

fn checkout(root: &Path, repo: Option<&str>) -> String {
    let name = present(repo)
        .and_then(|repo| repo.rsplit('/').next())
        .unwrap_or(env!("CARGO_PKG_NAME"));
    let path = root.join(name);
    match path.join(".git").try_exists() {
        Ok(true) => path.display().to_string(),
        Ok(false) => NO_CHECKOUT.into(),
        Err(error) => format!("could not check {}: {error}", path.display()),
    }
}

fn panel(stamp: Stamp, now: i64, exe: io::Result<PathBuf>, repos: &Path) -> String {
    let repo = present(stamp.repo);
    let link = repo
        .zip(present(stamp.server))
        .map(|(repo, server)| format!("{}/{repo}", server.trim_end_matches('/')));
    let version =
        present(stamp.commit).map(|commit| format!("remote-{}", commit.get(..7).unwrap_or(commit)));
    let rows = [
        ("Repo", repo.unwrap_or(DEVELOPMENT).to_owned()),
        ("Link", link.unwrap_or_else(|| DEVELOPMENT.into())),
        ("Version", version.unwrap_or_else(|| DEVELOPMENT.into())),
        ("Built", built(stamp.built, now)),
        ("Installed", installed(exe)),
        ("Checkout", checkout(repos, repo)),
    ];
    let mut text = String::from("Shows remote info\n\n");
    for (label, value) in rows {
        text.push_str(&format!("{label:<11}{value}\n"));
    }
    text.push_str("\nAny key closes this panel.");
    text
}

/// Built once when the panel opens: unchanging text never redraws over a selection.
pub fn text() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    panel(BUILD, now, std::env::current_exe(), Path::new(REPOS))
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAMPED: Stamp = Stamp {
        repo: Some("nelsong6/shows-remote"),
        server: Some("https://forgejo.romaine.life/"),
        commit: Some("4daba4f0123456789abcdef0123456789abcdef0"),
        built: Some("1793661240"),
    };
    const UNSTAMPED: Stamp = Stamp {
        repo: None,
        server: None,
        commit: None,
        built: None,
    };
    const NOW: i64 = 1793661240 + 2 * 86400;

    fn line<'a>(text: &'a str, label: &str) -> &'a str {
        text.lines()
            .find_map(|line| line.strip_prefix(label))
            .unwrap_or_else(|| panic!("no {label} line in {text}"))
            .trim_start()
    }

    fn exe() -> io::Result<PathBuf> {
        Ok(PathBuf::from(r"C:\apps\shows-remote.exe"))
    }

    #[test]
    fn a_ci_build_names_its_repo_link_release_and_age() {
        let text = panel(STAMPED, NOW, exe(), Path::new("missing"));
        assert_eq!(line(&text, "Repo"), "nelsong6/shows-remote");
        assert_eq!(
            line(&text, "Link"),
            "https://forgejo.romaine.life/nelsong6/shows-remote"
        );
        assert_eq!(line(&text, "Version"), "remote-4daba4f");
        assert_eq!(line(&text, "Built"), "Nov 2, 3:14 PM Pacific (2 days ago)");
        assert_eq!(line(&text, "Installed"), r"C:\apps\shows-remote.exe");
    }

    #[test]
    fn an_unstamped_build_says_so_on_every_stamped_line() {
        for stamp in [
            UNSTAMPED,
            Stamp {
                repo: Some(""),
                server: Some("  "),
                commit: Some(""),
                built: Some(""),
            },
        ] {
            let text = panel(stamp, NOW, exe(), Path::new("missing"));
            for label in ["Repo", "Link", "Version", "Built"] {
                assert_eq!(line(&text, label), DEVELOPMENT, "{label}");
            }
            assert_eq!(line(&text, "Checkout"), NO_CHECKOUT);
        }
    }

    #[test]
    fn a_partial_stamp_never_invents_the_missing_value() {
        let text = panel(
            Stamp {
                server: None,
                ..STAMPED
            },
            NOW,
            exe(),
            Path::new("missing"),
        );
        assert_eq!(line(&text, "Repo"), "nelsong6/shows-remote");
        assert_eq!(line(&text, "Link"), DEVELOPMENT);
    }

    #[test]
    fn checkout_requires_git_in_the_repo_named_by_the_stamp() {
        let repos = tempfile::tempdir().unwrap();
        let folder = repos.path().join("shows-remote");
        std::fs::create_dir(&folder).unwrap();
        assert_eq!(
            checkout(repos.path(), STAMPED.repo),
            NO_CHECKOUT,
            "a folder without .git is not a checkout"
        );
        std::fs::create_dir(folder.join(".git")).unwrap();
        for repo in [STAMPED.repo, None] {
            assert_eq!(checkout(repos.path(), repo), folder.display().to_string());
        }
        assert_eq!(
            checkout(repos.path(), Some("nelsong6/other-app")),
            NO_CHECKOUT
        );
    }

    #[test]
    fn failures_are_stated_instead_of_left_blank() {
        let denied = io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let text = panel(
            Stamp {
                built: Some("yesterday"),
                ..STAMPED
            },
            NOW,
            Err(denied),
            Path::new("missing"),
        );
        assert_eq!(
            line(&text, "Built"),
            "unreadable build time stamp: yesterday"
        );
        assert_eq!(
            line(&text, "Installed"),
            "could not read the running executable's path: access denied"
        );
    }

    #[test]
    fn pacific_time_follows_daylight_saving_boundaries() {
        for (unix, expected) in [
            (1772963999, "Mar 8, 1:59 AM Pacific"),
            (1772964000, "Mar 8, 3:00 AM Pacific"),
            (1793523599, "Nov 1, 1:59 AM Pacific"),
            (1793523600, "Nov 1, 1:00 AM Pacific"),
            (1798790340, "Dec 31, 11:59 PM Pacific"),
            (1798747500, "Dec 31, 12:05 PM Pacific"),
            (1767254700, "Jan 1, 12:05 AM Pacific"),
            (1835481600, "Feb 29, 4:00 PM Pacific"),
        ] {
            assert_eq!(pacific(unix), expected);
        }
    }

    #[test]
    fn elapsed_time_reads_as_plain_language() {
        for (seconds, expected) in [
            (0, "just now"),
            (-30, "just now"),
            (59, "just now"),
            (60, "1 minute ago"),
            (3599, "59 minutes ago"),
            (3600, "1 hour ago"),
            (86399, "23 hours ago"),
            (86400, "1 day ago"),
            (86400 * 30, "30 days ago"),
            (-301, "this PC's clock is behind the build"),
        ] {
            assert_eq!(elapsed(seconds), expected);
        }
    }
}
