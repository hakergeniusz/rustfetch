//! `uptime` module: time since boot, fastfetch style ("2 days, 3 hours, 4 mins").

use crate::{instance::Instance, modules::Row, util};

fn plural(count: u64, unit: &str) -> String {
    if count == 1 {
        format!("{count} {unit}")
    } else {
        format!("{count} {unit}s")
    }
}

/// ffDurationAppendNum: seconds are rounded into minutes (>= 30 bumps),
/// and the module rounds the /proc/uptime milliseconds into seconds
/// ((ms + 500) / 1000) before that.
fn duration_parts(total_seconds: u64) -> Vec<String> {
    if total_seconds < 60 {
        return vec![plural(total_seconds, "second")];
    }

    let mut minutes_total = total_seconds / 60;
    if total_seconds % 60 >= 30 {
        minutes_total += 1;
    }
    let days = minutes_total / (60 * 24);
    let hours = (minutes_total / 60) % 24;
    let minutes = minutes_total % 60;

    let mut parts = Vec::new();
    if days > 0 {
        // ffDurationAppendNum marks 100+ days with "(!)".
        let mut text = plural(days, "day");
        if days >= 100 {
            text.push_str("(!)");
        }
        parts.push(text);
    }
    if hours > 0 {
        parts.push(plural(hours, "hour"));
    }
    if minutes > 0 {
        parts.push(plural(minutes, "min"));
    }
    parts
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(content) = util::read_file("/proc/uptime") else {
        return Vec::new();
    };
    let Some(total_seconds) = util::first_line(&content)
        .split_whitespace()
        .next()
        .and_then(|token| token.parse::<f64>().ok())
        .map(|value| {
            // ffDetectUptime: uptime = (uint64)(sec * 1000), and the module
            // prints ffDurationAppendNum((uptime + 500) / 1000).
            let milliseconds = (value * 1000.0) as u64;
            (milliseconds + 500) / 1000
        })
    else {
        return Vec::new();
    };

    vec![Row::new("Uptime", duration_parts(total_seconds).join(", "))]
}

#[cfg(test)]
mod tests {
    use super::duration_parts;

    #[test]
    fn rounds_seconds_like_ffdurationappendnum() {
        // 54 min 59 s still bumps to "55 mins"; 54 min 29 s does not.
        assert_eq!(duration_parts(3299).join(", "), "55 mins");
        assert_eq!(duration_parts(3269).join(", "), "54 mins");
        assert_eq!(duration_parts(3300).join(", "), "55 mins");
        // 3330 s: 55 min 30 s bumps into "56 mins".
        assert_eq!(duration_parts(3330).join(", "), "56 mins");
        // Under a minute prints seconds, never "0 mins".
        assert_eq!(duration_parts(45).join(", "), "45 seconds");
        assert_eq!(duration_parts(1).join(", "), "1 second");
        // Zero-minute tails are omitted; 100+ days get the "(!)" marker.
        assert_eq!(duration_parts(90_000).join(", "), "1 day, 1 hour");
        assert_eq!(duration_parts(95_400).join(", "), "1 day, 2 hours, 30 mins");
        assert_eq!(duration_parts(8_640_000).join(", "), "100 days(!)");
        assert_eq!(duration_parts(60).join(", "), "1 min");
    }
}
