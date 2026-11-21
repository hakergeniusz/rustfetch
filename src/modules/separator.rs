//! `separator` module: a line of the configured string at the key column,
//! ported from separator.c. Defaults: one dash per title column (1 +
//! username width + hostname up to the first dot, or the whole hostname
//! when the title module ran with `fqdn`). The JSON config object sets
//! `string`, `outputColor` and `times`; its parse errors are
//! showErrors-gated rows printed ahead of the separator, like upstream's
//! ffPrintError calls during ffParseSeparatorJsonObject.

use crate::{
    instance::Instance,
    modules::Row,
    wcwidth::utf8_str_width,
};


struct SeparatorOptions {
    string: String,
    output_color: String,
    times: u32,
}

impl Default for SeparatorOptions {
    fn default() -> Self {
        // ffInitSeparatorOptions.
        Self { string: "-".to_string(), output_color: String::new(), times: 0 }
    }
}

impl SeparatorOptions {
    /// ffParseSeparatorJsonObject: parse errors become rows (ffPrintError
    /// is showErrors-gated) and parsing continues.
    fn parse(&mut self, entry: &crate::jsonc::Value, errors: &mut Vec<Row>, state: &crate::cli::ColorState) {
        let error = |errors: &mut Vec<Row>, message: String| {
            errors.push(Row::error("Separator", message));
        };
        let crate::jsonc::Value::Object(entries) = entry else {
            return;
        };
        for (key, value) in entries {
            match key.as_str() {
                "type" | "condition" => {},
                "string" => match value.as_str() {
                    Some(string) => {
                        if string.is_empty() {
                            error(errors, "Property 'string' must not be empty".to_string());
                        } else {
                            self.string = string.to_string();
                        }
                    },
                    None => error(errors, "Property 'string' must be a string".to_string()),
                },
                "outputColor" => {
                    // ffOptionParseColor(nullptr) is a no-op; the parse
                    // resolves PROP words against the final display colors.
                    if let Some(color) = value.as_str() {
                        self.output_color = state.parse_color(color);
                    }
                },
                "times" => match value {
                    crate::jsonc::Value::Int(int) if *int >= 0 && *int <= u32::MAX as i64 => {
                        self.times = *int as u32;
                    },
                    crate::jsonc::Value::UInt(uint) if *uint <= u32::MAX as u64 => {
                        self.times = *uint as u32;
                    },
                    _ => {
                        error(
                            errors,
                            "Property 'times' must be a non-negative integer no greater than 4294967295".to_string(),
                        );
                    },
                },
                "length" => error(errors, "The option length has been renamed to times.".to_string()),
                _ => error(errors, format!("Unknown JSON key {key}")),
            }
        }
    }
}

/// ffPrintSeparator: the logo line is the row emission's job; this builds
/// the separator text.
pub fn run(instance: &Instance) -> Vec<Row> {
    let mut options = SeparatorOptions::default();
    let mut rows = Vec::new();
    if let Some(entry) = instance.entry {
        let state = crate::cli::ColorState::from_instance(instance);
        options.parse(entry, &mut rows, &state);
    }

    let text = if options.times > 0 {
        if options.string.len() == 1 {
            // ffPrintCharTimes prints the first byte N times; a one-byte
            // string is always ASCII.
            let byte = options.string.as_bytes()[0] as char;
            std::iter::repeat(byte).take(options.times as usize).collect::<String>()
        } else {
            options.string.repeat(options.times as usize)
        }
    } else {
        // No times: size the separator to the title's user@host length.
        // The fqdn branch keeps upstream's byte length (not width).
        let platform = crate::platform::platform();
        let host = &platform.host_name;
        let host_length = if instance.title_fqdn.get() {
            host.len()
        } else {
            host.find('.').unwrap_or(host.len())
        };
        let title_length = (1 + utf8_str_width(&platform.user_name) + host_length as u32) as usize;

        if options.string.len() == 1 {
            let byte = options.string.as_bytes()[0] as char;
            std::iter::repeat(byte).take(title_length).collect::<String>()
        } else {
            let string_width = utf8_str_width(&options.string);
            let mut remaining = title_length as i32;

            let mut out = String::new();
            while remaining >= string_width as i32 {
                out.push_str(&options.string);
                remaining -= string_width as i32;
                // A zero-width separator would spin forever here (upstream
                // hangs); the port breaks out instead.
                if string_width == 0 {
                    break;
                }
            }
            if remaining > 0 {
                if string_width != options.string.len() as u32 {
                    // Unicode: consume whole characters until the width
                    // runs out, writing every consumed byte — the last one
                    // may overshoot, like upstream's walk.
                    let mut taken = 0usize;
                    for c in options.string.chars() {
                        if remaining <= 0 {
                            break;
                        }
                        remaining -= crate::wcwidth::wcwidth(c as u32) as i32;
                        taken += c.len_utf8();
                    }
                    out.push_str(&options.string[..taken]);
                } else {
                    out.push_str(&options.string[..remaining as usize]);
                }
            }
            out
        }
    };

    // The output color wraps only the separator text, outside pipe mode.
    let value = if instance.color && !options.output_color.is_empty() {
        format!("\x1b[{}m{}\x1b[m", options.output_color, text)
    } else {
        text
    };
    rows.push(Row::raw(value));
    rows
}
