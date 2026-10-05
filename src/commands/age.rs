use camino::Utf8Path;

use anyhow::Result;

use crate::commands::count::open_repo;

/// The age of the last commit, as git's `%cr` renders it ("3 days ago").
/// Read through libgit2, so no process per repo. An empty repo (unborn
/// branch) has no last commit and reports nothing.
pub fn do_age(project: &Utf8Path) -> Result<Option<String>> {
    let repo = open_repo(project)?;
    let Ok(head) = repo.head() else {
        return Ok(None);
    };
    let commit = head.peel_to_commit()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    Ok(Some(relative_date(now - commit.time().seconds())))
}

/// `n unit` / `n units`.
fn plural(n: i64, unit: &str) -> String {
    if n == 1 {
        format!("1 {unit}")
    } else {
        format!("{n} {unit}s")
    }
}

/// git's `show_date_relative` (date.c), thresholds and rounding included, so
/// the output is what `git log -1 --format=%cr` printed before.
fn relative_date(diff: i64) -> String {
    if diff < 0 {
        return "in the future".to_string();
    }
    if diff < 90 {
        return format!("{} ago", plural(diff, "second"));
    }
    let minutes = (diff + 30) / 60;
    if minutes < 90 {
        return format!("{} ago", plural(minutes, "minute"));
    }
    let hours = (minutes + 30) / 60;
    if hours < 36 {
        return format!("{} ago", plural(hours, "hour"));
    }
    let days = (hours + 12) / 24;
    if days < 14 {
        return format!("{} ago", plural(days, "day"));
    }
    if days < 70 {
        return format!("{} ago", plural((days + 3) / 7, "week"));
    }
    if days < 365 {
        return format!("{} ago", plural((days + 15) / 30, "month"));
    }
    if days < 1825 {
        let total_months = (days * 12 * 2 + 365) / (365 * 2);
        let (years, months) = (total_months / 12, total_months % 12);
        return if months == 0 {
            format!("{} ago", plural(years, "year"))
        } else {
            format!("{}, {} ago", plural(years, "year"), plural(months, "month"))
        };
    }
    format!("{} ago", plural((days + 183) / 365, "year"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_gits_relative_dates() {
        const H: i64 = 3600;
        const D: i64 = 24 * H;
        for (diff, want) in [
            (-5, "in the future"),
            (1, "1 second ago"),
            (89, "89 seconds ago"),
            (90, "2 minutes ago"),
            (60 * 60, "60 minutes ago"),
            (2 * H, "2 hours ago"),
            (35 * H, "35 hours ago"),
            (2 * D, "2 days ago"),
            (13 * D, "13 days ago"),
            (14 * D, "2 weeks ago"),
            (69 * D, "10 weeks ago"),
            (70 * D, "2 months ago"),
            (364 * D, "12 months ago"),
            (365 * D, "1 year ago"),
            (400 * D, "1 year, 1 month ago"),
            (1000 * D, "2 years, 9 months ago"),
            (1825 * D, "5 years ago"),
        ] {
            assert_eq!(relative_date(diff), want, "{diff}");
        }
    }
}
