//! `swap` module: ported from detection/swap/swap_linux.c. /proc/swaps is
//! the primary source (one item per device, names may carry octal escapes);
//! the meminfo fallback reports a single "Total" item. The default text
//! sums all items into one row — "Unused" when devices exist but report no
//! space, "Disabled" when none are configured.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    modules::colorize_percent,
    util,
};

pub struct SwapResult {
    pub name: String,
    pub used: u64,
    pub total: u64,
}

/// detectByProcSwaps: one entry per line after the header. Returns None
/// when the file is unreadable or a line is malformed (falling back to
/// meminfo), Some when the file itself parsed.
fn detect_by_proc_swaps() -> Option<Result<Vec<SwapResult>, ()>> {
    let content = util::read_file("/proc/swaps")?;
    let mut items = Vec::new();
    for line in content.lines().skip(1) {
        if line.is_empty() {
            continue;
        }
        // "%255s %*[^\t]%lu%lu": name, then the total and used columns.
        let mut tokens = line.split_whitespace();
        let (Some(name), Some(kind), Some(total), Some(used)) =
            (tokens.next(), tokens.next(), tokens.next(), tokens.next())
        else {
            return Some(Err(()));
        };
        let _ = kind;
        let Ok(total) = total.parse::<u64>() else {
            return Some(Err(()));
        };
        let Ok(used) = used.parse::<u64>() else {
            return Some(Err(()));
        };
        items.push(SwapResult {
            name: decode_octal_escapes(name),
            used: used * 1024,
            total: total * 1024,
        });
    }
    Some(Ok(items))
}

/// The kernel escapes backslashes in device names as \NNN octal triples.
fn decode_octal_escapes(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let octal = std::str::from_utf8(&bytes[index + 1..index + 4]).unwrap_or("");
            if let Ok(value) = u8::from_str_radix(octal, 8) {
                out.push(value);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// detectByProcMeminfo: one aggregate "Total" item, wrapping arithmetic.
fn detect_by_proc_meminfo() -> Result<Vec<SwapResult>, String> {
    let Some(content) = util::read_file("/proc/meminfo") else {
        return Err("ffReadFileData(\"/proc/meminfo\", ARRAY_SIZE(buf)-1, buf)".to_string());
    };
    let total_kb = content
        .lines()
        .find_map(|line| {
            let (name, value) = util::parse_colon_line(line)?;
            (name == "SwapTotal").then(|| value.split_whitespace().next()?.parse::<u64>().ok())?
        })
        .unwrap_or(0);
    let free_kb = content
        .lines()
        .find_map(|line| {
            let (name, value) = util::parse_colon_line(line)?;
            (name == "SwapFree").then(|| value.split_whitespace().next()?.parse::<u64>().ok())?
        })
        .unwrap_or(0);
    Ok(vec![SwapResult {
        name: "Total".to_string(),
        total: total_kb * 1024,
        used: total_kb.wrapping_sub(free_kb) * 1024,
    }])
}

pub fn detect() -> Result<Vec<SwapResult>, String> {
    match detect_by_proc_swaps() {
        Some(Ok(items)) => Ok(items),
        Some(Err(())) => detect_by_proc_meminfo(),
        None => detect_by_proc_meminfo(),
    }
}

/// The default text row for one aggregate: "{used} / {total} ({pct})" or
/// the Unused/Disabled markers when there is no space to report.
fn aggregate_value(storage: &SwapResult, total_count: usize, color: bool) -> String {
    if storage.total == 0 {
        return if total_count > 0 { "Unused".to_string() } else { "Disabled".to_string() };
    }
    let pct = storage.used as f64 / storage.total as f64 * 100.0;
    format!(
        "{} / {} ({})",
        util::fmt_bytes(storage.used),
        util::fmt_bytes(storage.total),
        colorize_percent(pct, 50, 80, color)
    )
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(items) = detect() else {
        // Detection errors are hidden rows by default.
        return Vec::new();
    };

    // Without the `separate` option all devices are summed into one row.
    let mut total = SwapResult { name: String::new(), used: 0, total: 0 };
    for item in &items {
        total.used = total.used.saturating_add(item.used);
        total.total = total.total.saturating_add(item.total);
    }
    vec![Row::new("Swap", aggregate_value(&total, items.len(), instance.color))]
}

/// ffGenerateSwapJsonResult: one object per device, empty array when none.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error),
        Ok(items) => {
            let mut result = Json::Array(Vec::new());
            for item in items {
                let mut object = Json::obj();
                object.push("name", item.name);
                object.push("used", item.used);
                object.push("total", item.total);
                result.push_value(object);
            }
            JsonOutcome::Result(result)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_octal_escapes() {
        assert_eq!(decode_octal_escapes("/dev/sda1"), "/dev/sda1");
        assert_eq!(decode_octal_escapes("/dev/\\040dir"), "/dev/ dir");
        assert_eq!(decode_octal_escapes("bad\\9escape"), "bad\\9escape");
    }

    #[test]
    fn aggregate_marks_unused_and_disabled() {
        assert_eq!(aggregate_value(&SwapResult { name: String::new(), used: 0, total: 0 }, 0, false), "Disabled");
        assert_eq!(aggregate_value(&SwapResult { name: String::new(), used: 0, total: 0 }, 2, false), "Unused");
    }
}
