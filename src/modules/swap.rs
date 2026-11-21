//! `swap` module: ported from detection/swap/swap_linux.c. /proc/swaps is
//! the primary source (one item per device, names may carry octal escapes);
//! the meminfo fallback reports a single "Total" item. The default text
//! sums all items into one row — "Unused" when devices exist but report no
//! space, "Disabled" when none are configured.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
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
            name: util::decode_octal_escapes(name),
            used: used * 1024,
            total: total * 1024,
        });
    }
    Some(Ok(items))
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

/// The default text row for one aggregate: bar + "used / total (pct)" per
/// the percent type, or the Unused/Disabled markers when there is no space
/// to report (the bar still draws at 0%).
fn aggregate_value(storage: &SwapResult, total_count: usize, percent: crate::format::PercentModuleConfig, ctx: &crate::format::PrintCtx, df: &crate::format::DisplayFormat) -> String {
    let percent_type_flags = if percent.type_flags == 0 { df.percent_type } else { percent.type_flags };
    let mut value = String::new();

    if storage.total == 0 {
        if percent_type_flags & crate::format::percent_type::BAR_BIT != 0 {
            crate::format::percent_append_bar(&mut value, 0.0, percent, ctx, df);
            value.push(' ');
        }
        if percent_type_flags & crate::format::percent_type::HIDE_OTHERS_BIT == 0 {
            value.push_str(if total_count > 0 { "Unused" } else { "Disabled" });
        } else {
            let parentheses = !value.is_empty();
            crate::format::percent_append_num(&mut value, 0.0, percent, parentheses, ctx, df);
        }
    } else {
        let percentage = storage.used as f64 / storage.total as f64 * 100.0;
        let mut used_pretty = String::new();
        crate::format::size_append_num(&mut used_pretty, storage.used, df);
        let mut total_pretty = String::new();
        crate::format::size_append_num(&mut total_pretty, storage.total, df);

        if percent_type_flags & crate::format::percent_type::BAR_BIT != 0 {
            crate::format::percent_append_bar(&mut value, percentage, percent, ctx, df);
            value.push(' ');
        }
        if percent_type_flags & crate::format::percent_type::HIDE_OTHERS_BIT == 0 {
            value.push_str(&used_pretty);
            value.push_str(" / ");
            value.push_str(&total_pretty);
            value.push(' ');
        }
        if percent_type_flags & crate::format::percent_type::NUM_BIT != 0 {
            let parentheses = !value.is_empty();
            crate::format::percent_append_num(&mut value, percentage, percent, parentheses, ctx, df);
        }
    }

    // ffStrbufTrimRight(&str, ' ').
    while value.ends_with(' ') {
        value.pop();
    }
    value
}

/// Module options: separate off (ffInitSwapOptions).
#[derive(Default)]
pub(crate) struct SwapOptions {
    separate: bool,
}

impl SwapOptions {
    fn from_instance(instance: &Instance) -> Self {
        let Some(entry) = instance.entry else {
            return Self::default();
        };
        Self {
            separate: entry.get("separate").and_then(crate::jsonc::Value::as_bool).unwrap_or(false),
        }
    }
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let options = SwapOptions::from_instance(instance);
    let percent = super::percent_config(instance.entry, crate::format::PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 });
    let ctx = super::print_ctx(instance);
    let Ok(items) = detect() else {
        // Detection errors are hidden rows by default.
        return Vec::new();
    };

    if options.separate && !items.is_empty() {
        // One "Swap (device)" row per swap entry.
        return items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let key = if item.name.is_empty() {
                    "Swap".to_string()
                } else {
                    format!("{} ({})", instance.display_name("swap"), item.name)
                };
                Row::new(&key, aggregate_value(item, items.len(), percent, &ctx, &instance.display_format))
                    .with_key_args(vec![
                        // printSwap's separate path pre-increments the index.
                        crate::modules::RowKeyArg::uint("index", i as u32 + 1),
                        crate::modules::RowKeyArg::text("name", item.name.clone()),
                    ])
            })
            .collect();
    }

    // Without the `separate` option all devices are summed into one row.
    let mut total = SwapResult { name: String::new(), used: 0, total: 0 };
    for item in &items {
        total.used = total.used.saturating_add(item.used);
        total.total = total.total.saturating_add(item.total);
    }
    vec![Row::new(
        instance.display_name("swap"),
        aggregate_value(&total, items.len(), percent, &ctx, &instance.display_format),
    )
    .with_key_args(vec![
        // The aggregate path calls printSwap with index 0 and an empty name.
        crate::modules::RowKeyArg::uint("index", 0),
        crate::modules::RowKeyArg::text("name", ""),
    ])]
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
        assert_eq!(util::decode_octal_escapes("/dev/sda1"), "/dev/sda1");
        assert_eq!(util::decode_octal_escapes("/dev/\\040dir"), "/dev/ dir");
        assert_eq!(util::decode_octal_escapes("bad\\9escape"), "bad\\9escape");
    }

    #[test]
    fn aggregate_marks_unused_and_disabled() {
        let percent = crate::format::PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 };
        let ctx = crate::format::PrintCtx { pipe: true, output_color: String::new() };
        assert_eq!(aggregate_value(&SwapResult { name: String::new(), used: 0, total: 0 }, 0, percent, &ctx, &Default::default()), "Disabled");
        assert_eq!(aggregate_value(&SwapResult { name: String::new(), used: 0, total: 0 }, 2, percent, &ctx, &Default::default()), "Unused");
    }
}
