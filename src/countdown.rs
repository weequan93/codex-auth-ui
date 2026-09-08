use chrono::{DateTime, Local, TimeZone};

#[derive(Debug, Clone, PartialEq)]
pub enum ResetDisplay {
    Countdown(String),
    Reset,
    Unknown,
}

pub fn reset_display(now: i64, resets_at: Option<i64>, weekly: bool) -> ResetDisplay {
    let Some(reset) = resets_at else {
        return ResetDisplay::Unknown;
    };
    if now >= reset {
        return ResetDisplay::Reset;
    }

    let minutes = reset.saturating_sub(now).saturating_add(59) / 60;
    if weekly && minutes >= 24 * 60 {
        let days = minutes / (24 * 60);
        let hours = (minutes % (24 * 60)) / 60;
        ResetDisplay::Countdown(format!("Resets in ~{days}d {hours}h"))
    } else {
        let hours = minutes / 60;
        let remaining_minutes = minutes % 60;
        ResetDisplay::Countdown(format!("Resets in ~{hours}h {remaining_minutes}m"))
    }
}

pub fn last_checked_label(timestamp: Option<i64>) -> String {
    let Some(timestamp) = timestamp else {
        return "Never checked".into();
    };
    let Some(value): Option<DateTime<Local>> = Local.timestamp_opt(timestamp, 0).single() else {
        return "Last checked: unknown".into();
    };
    format!("Last checked {}", value.format("%b %-d, %-I:%M %p"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_hour_scale_countdown() {
        assert_eq!(
            reset_display(1_000, Some(1_000 + 2 * 3600 + 15 * 60), false),
            ResetDisplay::Countdown("Resets in ~2h 15m".into())
        );
    }

    #[test]
    fn formats_week_scale_countdown() {
        assert_eq!(
            reset_display(1_000, Some(1_000 + 3 * 86400 + 4 * 3600), true),
            ResetDisplay::Countdown("Resets in ~3d 4h".into())
        );
    }

    #[test]
    fn never_advances_an_expired_window() {
        assert_eq!(
            reset_display(2_000, Some(2_000), false),
            ResetDisplay::Reset
        );
        assert_eq!(reset_display(2_000, Some(1_000), true), ResetDisplay::Reset);
    }

    #[test]
    fn extreme_persisted_timestamps_do_not_overflow() {
        assert!(matches!(
            reset_display(i64::MIN, Some(i64::MAX), true),
            ResetDisplay::Countdown(_)
        ));
    }
}
