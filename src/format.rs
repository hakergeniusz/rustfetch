//! The display format engine: ffStrbufAppendDouble and the shared
//! size/duration/frequency formatters, driven by the FFOptionsDisplay
//! format fields (defaults from ffOptionsInitDisplay). The percent and
//! temperature formatters join in the module-rewire commit.

/// FF_SPACE_BEFORE_UNIT_* (options/display.h). DEFAULT means "unit gets a
/// space unless the formatter says otherwise" — every current caller
/// treats it like ALWAYS except kelvin/percent, which check it directly.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum SpaceBeforeUnit {
    #[default]
    Default,
    Always,
    Never,
}

/// FF_SIZE_BINARY_PREFIX_TYPE_*: the divisor and prefix table ffSizeAppendNum
/// uses. The upstream default branch (impossible from the CLI/config enums)
/// divides by 1024 and only ever prints "B".
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum SizeBinaryPrefix {
    #[default]
    Iec,
    Si,
    Jedec,
}

/// FF_TEMPERATURE_UNIT_*: the unit ffTempsAppendNum converts to. DEFAULT
/// and CELSIUS print the same.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum TemperatureUnit {
    #[default]
    Default,
    Celsius,
    Fahrenheit,
    Kelvin,
}

/// FF_COLOR_FG_* (common/color.h): the SGR parameters the default percent/
/// temp/bar colors resolve to.
pub const FG_GREEN: &str = "32";
pub const FG_YELLOW: &str = "33";
pub const FG_RED: &str = "31";
pub const FG_LIGHT_YELLOW: &str = "93";
pub const FG_LIGHT_RED: &str = "91";
pub const FG_LIGHT_WHITE: &str = "97";
pub const FG_LIGHT_BLACK: &str = "90";

/// FF_PERCENTAGE_TYPE_* bits (common/percent.h): what a percent module
/// prints. A module config of 0 inherits display.percentType.
pub mod percent_type {
    pub const NUM_BIT: u8 = 1 << 0;
    pub const BAR_BIT: u8 = 1 << 1;
    pub const HIDE_OTHERS_BIT: u8 = 1 << 2;
    pub const NUM_COLOR_BIT: u8 = 1 << 3;
    pub const BAR_MONOCHROME_BIT: u8 = 1 << 4;
}

/// FFPercentageModuleConfig: the module's own green/yellow thresholds and
/// type override; every percent module inits one with its own defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PercentModuleConfig {
    pub green: u8,
    pub yellow: u8,
    /// 0 inherits display.percentType.
    pub type_flags: u8,
}

/// FFColorRangeConfig: the green/yellow thresholds ffTempsAppendNum uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorRangeConfig {
    pub green: u8,
    pub yellow: u8,
}

/// The per-print context the percent/temps formatters read from the
/// caller: the pipe gate (upstream's instance.config.display.pipe) and
/// appendOutputColor's resolved tail color (the module entry's outputColor
/// when set, else display.colorOutput; empty prints no escape). Built by
/// crate::modules::print_ctx at each value-build site.
#[derive(Clone, Debug, Default)]
pub struct PrintCtx {
    pub pipe: bool,
    pub output_color: String,
}

/// The FFOptionsDisplay format fields the formatters read, in
/// ffOptionsInitDisplay's defaults. The theme-dependent percent/temp/bar
/// colors detect once here (upstream's forceEnv probe reads COLORFGBG
/// before the config parse; unset stays dark → the light variants).
#[derive(Clone, Debug)]
pub struct DisplayFormat {
    pub duration_abbreviation: bool,
    pub duration_space_before_unit: SpaceBeforeUnit,
    pub size_binary_prefix: SizeBinaryPrefix,
    pub size_ndigits: u8,
    pub size_max_prefix: u8,
    pub size_space_before_unit: SpaceBeforeUnit,
    pub freq_ndigits: i8,
    pub freq_space_before_unit: SpaceBeforeUnit,
    pub temp_unit: TemperatureUnit,
    pub temp_ndigits: u8,
    pub temp_color_green: String,
    pub temp_color_yellow: String,
    pub temp_color_red: String,
    pub temp_space_before_unit: SpaceBeforeUnit,
    pub bar_char_elapsed: String,
    pub bar_char_total: String,
    pub bar_border_left: String,
    pub bar_border_right: String,
    pub bar_border_left_elapsed: String,
    pub bar_border_right_elapsed: String,
    /// "auto" selects the percent bands per block; a parsed color or the
    /// empty string (no color) disable that.
    pub bar_color_elapsed: String,
    pub bar_color_total: String,
    pub bar_color_border: String,
    pub bar_width: u8,
    pub percent_type: u8,
    pub percent_ndigits: u8,
    pub percent_color_green: String,
    pub percent_color_yellow: String,
    pub percent_color_red: String,
    pub percent_space_before_unit: SpaceBeforeUnit,
    pub percent_width: u8,
}

impl Default for DisplayFormat {
    fn default() -> Self {
        // ffOptionsInitDisplay: terminalLightTheme only when COLORFGBG
        // names a light background (fgbgToColor's dark flag); unset stays
        // dark and the light color variants init.
        let light = crate::modules::terminaltheme::terminal_is_light_by_env();
        let (yellow, red, white) = if light { (FG_YELLOW, FG_RED, "37") } else { (FG_LIGHT_YELLOW, FG_LIGHT_RED, FG_LIGHT_WHITE) };
        Self {
            duration_abbreviation: false,
            duration_space_before_unit: SpaceBeforeUnit::Default,
            size_binary_prefix: SizeBinaryPrefix::Iec,
            size_ndigits: 2,
            size_max_prefix: 8, // YB
            size_space_before_unit: SpaceBeforeUnit::Default,
            freq_ndigits: 2,
            freq_space_before_unit: SpaceBeforeUnit::Default,
            temp_unit: TemperatureUnit::Default,
            temp_ndigits: 1,
            temp_color_green: FG_GREEN.to_string(),
            temp_color_yellow: yellow.to_string(),
            temp_color_red: red.to_string(),
            temp_space_before_unit: SpaceBeforeUnit::Default,
            bar_char_elapsed: "■".to_string(),
            bar_char_total: "-".to_string(),
            bar_border_left: "[ ".to_string(),
            bar_border_right: " ]".to_string(),
            bar_border_left_elapsed: String::new(),
            bar_border_right_elapsed: String::new(),
            bar_color_elapsed: "auto".to_string(),
            bar_color_total: white.to_string(),
            bar_color_border: white.to_string(),
            bar_width: 10,
            percent_type: percent_type::NUM_BIT | percent_type::NUM_COLOR_BIT,
            percent_ndigits: 0,
            percent_color_green: FG_GREEN.to_string(),
            percent_color_yellow: yellow.to_string(),
            percent_color_red: red.to_string(),
            percent_space_before_unit: SpaceBeforeUnit::Default,
            percent_width: 0,
        }
    }
}

/// ffStrbufAppendDouble: yyjson's double writing with fastfetch's
/// trailing-zero handling on top. `precision == 0` rounds to the nearest
/// integer first; `precision > 0` writes at most that many decimals, which
/// the `trailing_zeros` flag pads back up to. `precision < 0` writes the
/// shortest representation (the fraction formatter's mode). Negative
/// values and -0.0 keep their sign; |value| >= 1e21 writes e-notation and
/// skips the padding (unreachable through the size/duration/frequency
/// formatters — their values are bounded — but ported for the fraction
/// formatter).
pub fn append_double(out: &mut String, value: f64, precision: i8, trailing_zeros: bool) {
    let mut value = value;
    if precision == 0 {
        // C round(): half away from zero, keeping the sign of -0.0.
        value = value.round();
    }
    let bits = value.to_bits();
    let negative = bits >> 63 == 1;
    let magnitude = f64::from_bits(bits & !(1u64 << 63));

    // yyjson_write_number: fixed notation for precision > 0, shortest
    // otherwise; |value| >= 1e21 is scientific and returns before the
    // padding walk (which would otherwise run off the buffer start).
    if magnitude > 1e21 {
        push_shortest(out, negative, magnitude);
        return;
    }

    let mut text = if negative { "-".to_string() } else { String::new() };
    text.push_str(&if precision > 0 {
        fixed_digits(magnitude, precision as u32)
    } else {
        shortest_digits(magnitude)
    });

    if trailing_zeros {
        if precision > 1 {
            // The C walk counts the written fraction digits one by one,
            // so an already-trimmed "1.5" pads one zero for precision 2.
            let decimals = text.rfind('.').map_or(0, |dot| text.len() - dot - 1);
            let pad = precision as i32 - decimals as i32;
            if pad > 0 {
                for _ in 0..pad {
                    text.push('0');
                }
            }
        } else if precision == 0 || text.ends_with('0') {
            strip_dot_zero(&mut text);
        }
    } else if text.ends_with('0') {
        strip_dot_zero(&mut text);
    }
    out.push_str(&text);
}

/// yyjson's fixed write: round to `p` decimals half away from zero on the
/// exact binary expansion (yyjson rounds its 17-digit intermediate the
/// same way; the two differ only past 17 significant digits), trim the
/// trailing zeros, and keep one zero after a stripped dot.
fn fixed_digits(magnitude: f64, p: u32) -> String {
    let text = if p <= 15 {
        // value·10^p fits a u128 for every formatter-reachable magnitude
        // and p ≤ 15, so the rounding is exact.
        let Some(scaled) = round_scaled(magnitude, p) else {
            return fallback_fixed(magnitude, p);
        };
        let digits = scaled.to_string();
        if digits.len() > p as usize {
            let split = digits.len() - p as usize;
            format!("{}.{}", &digits[..split], &digits[split..])
        } else {
            format!("0.{:0>width$}", digits, width = p as usize)
        }
    } else {
        // precision > 15 leaves the u128 range; Rust's formatter rounds
        // the exact expansion correctly but ties to even — a divergence
        // only exact half-way values beyond 16 decimals can observe.
        fallback_fixed(magnitude, p)
    };
    // Trim trailing fraction zeros; "1.00" reads back as "1.0" (yyjson
    // keeps one zero after the dot to mark the number as floating point).
    let trimmed = text.trim_end_matches('0');
    if trimmed.ends_with('.') {
        format!("{trimmed}0")
    } else {
        trimmed.to_string()
    }
}

/// The p > 15 fallback: Rust's correctly rounded fixed form, re-trimmed
/// into yyjson's shape.
fn fallback_fixed(magnitude: f64, p: u32) -> String {
    format!("{magnitude:.0$}", p as usize)
}

/// value·10^p rounded half away from zero on the exact expansion; None
/// when the exact product leaves the u128 range.
fn round_scaled(magnitude: f64, p: u32) -> Option<u128> {
    let bits = magnitude.to_bits();
    let exp_field = (bits >> 52) & 0x7ff;
    let frac = bits & ((1u64 << 52) - 1);
    if exp_field == 0 {
        // Subnormals are below 1e-308: every p ≤ 15 rounds them to zero.
        return Some(0);
    }
    let significand = (frac | 1u64 << 52) as u128;
    let exponent = exp_field as i32 - 1075;
    if exponent >= 0 {
        // An integral exact expansion needs no rounding.
        significand
            .checked_mul(10u128.pow(p))?
            .checked_shl(exponent as u32)
    } else {
        let scaled = significand.checked_mul(10u128.pow(p))?;
        let shift = (-exponent) as u32;
        if shift >= 128 {
            // Scaled < 2^68 sits far below half of the last digit.
            return Some(0);
        }
        let divisor = 1u128 << shift;
        let quotient = scaled >> shift;
        let remainder = scaled & (divisor - 1);
        Some(if remainder * 2 >= divisor { quotient + 1 } else { quotient })
    }
}

/// yyjson's shortest write (write_f64_raw): plain decimal for
/// 1e-6 ≤ |value| < 1e21 (the JSON.stringify thresholds), e-notation
/// outside; integral values keep a ".0".
fn shortest_digits(magnitude: f64) -> String {
    if magnitude == 0.0 {
        return "0.0".to_string();
    }
    if magnitude < 1e-6 || magnitude >= 1e21 {
        return format!("{magnitude:e}");
    }
    // Rust's Display is the shortest round-trip form in plain notation;
    // integral values need the float marker appended.
    let plain = format!("{magnitude}");
    if plain.contains('.') {
        plain
    } else {
        format!("{plain}.0")
    }
}

/// The |value| >= 1e21 branch: e-notation without a '+' or exponent
/// padding, which matches Rust's LowerExp.
fn push_shortest(out: &mut String, negative: bool, magnitude: f64) {
    if negative {
        out.push('-');
    }
    out.push_str(&format!("{magnitude:e}"));
}

/// The `.0` removal both trailing-zero paths share: yyjson always appends
/// ".0" to integral floats, and fastfetch strips those two characters.
fn strip_dot_zero(text: &mut String) {
    text.truncate(text.len().saturating_sub(2));
}

/// ffSizeAppendNum: divide until below the base, then format via
/// ffStrbufAppendDouble (trailing zeros kept) plus the prefix.
pub fn size_append_num(out: &mut String, bytes: u64, d: &DisplayFormat) {
    const IEC: [&str; 10] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB", "ZiB", "YiB", ""];
    const SI: [&str; 10] = ["B", "kB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB", ""];
    const JEDEC: [&str; 6] = ["B", "KB", "MB", "GB", "TB", ""];

    let (base, prefixes) = match d.size_binary_prefix {
        SizeBinaryPrefix::Iec => (1024u64, &IEC[..]),
        SizeBinaryPrefix::Si => (1000, &SI[..]),
        SizeBinaryPrefix::Jedec => (1024, &JEDEC[..]),
    };

    let mut size = bytes as f64;
    let mut counter = 0usize;
    while size >= base as f64
        && counter < d.size_max_prefix as usize
        && prefixes.get(counter + 1).is_some_and(|prefix| !prefix.is_empty())
    {
        size /= base as f64;
        counter += 1;
    }

    if counter == 0 {
        out.push_str(&bytes.to_string());
    } else {
        append_double(out, size, d.size_ndigits as i8, true);
    }
    if d.size_space_before_unit != SpaceBeforeUnit::Never {
        out.push(' ');
    }
    out.push_str(prefixes[counter]);
}

/// ffDurationAppendNum: seconds fold into minutes with a >= 30 bump,
/// zero tails are omitted, 100+ days carry the "(!)" marker (long form
/// only), and the abbreviated form joins with single spaces.
pub fn duration_append_num(out: &mut String, total_seconds: u64, d: &DisplayFormat) {
    let space_before_unit = d.duration_space_before_unit != SpaceBeforeUnit::Never;
    let abbreviated = d.duration_abbreviation;

    if total_seconds < 60 {
        out.push_str(&total_seconds.to_string());
        if space_before_unit {
            out.push(' ');
        }
        out.push_str(if abbreviated { "sec" } else { "second" });
        if total_seconds != 1 {
            out.push('s');
        }
        return;
    }

    let mut total = total_seconds;
    let seconds = (total % 60) as u32;
    total /= 60;
    if seconds >= 30 {
        total += 1;
    }
    let minutes = (total % 60) as u32;
    total /= 60;
    let hours = (total % 24) as u32;
    total /= 24;
    // upstream's (uint32_t) truncation of the day count.
    let days = total as u32;

    if days > 0 {
        out.push_str(&days.to_string());
        if space_before_unit {
            out.push(' ');
        }
        if abbreviated {
            out.push('d');
            if hours > 0 || minutes > 0 {
                out.push(' ');
            }
        } else {
            out.push_str(if days == 1 { "day" } else { "days" });
            if days >= 100 {
                out.push_str("(!)");
            }
            if hours > 0 || minutes > 0 {
                out.push_str(", ");
            }
        }
    }

    if hours > 0 {
        out.push_str(&hours.to_string());
        if space_before_unit {
            out.push(' ');
        }
        if abbreviated {
            out.push('h');
            if minutes > 0 {
                out.push(' ');
            }
        } else {
            out.push_str(if hours == 1 { "hour" } else { "hours" });
            if minutes > 0 {
                out.push_str(", ");
            }
        }
    }

    if minutes > 0 {
        out.push_str(&minutes.to_string());
        if space_before_unit {
            out.push(' ');
        }
        if abbreviated {
            out.push('m');
        } else {
            out.push_str(if minutes == 1 { "min" } else { "mins" });
        }
    }
}

/// ffFreqAppendNum: mhz/1000 with fixed decimals and trailing zeros
/// ("4.40 GHz"), or the raw MHz count when ndigits is negative; prints
/// nothing (returns false) for a zero frequency.
pub fn freq_append_num(out: &mut String, mhz: u32, d: &DisplayFormat) -> bool {
    if mhz == 0 {
        return false;
    }
    let space_before_unit = d.freq_space_before_unit != SpaceBeforeUnit::Never;
    if d.freq_ndigits >= 0 {
        append_double(out, mhz as f64 / 1000.0, d.freq_ndigits, true);
        if space_before_unit {
            out.push(' ');
        }
        out.push_str("GHz");
    } else {
        out.push_str(&mhz.to_string());
        if space_before_unit {
            out.push(' ');
        }
        out.push_str("MHz");
    }
    true
}

/// appendOutputColor (common/impl/percent.c): the reset tail ends with the
/// module's output color, or display.colorOutput when the module set none.
fn append_output_color(out: &mut String, ctx: &PrintCtx) {
    if !ctx.output_color.is_empty() {
        out.push_str(&format!("\x1b[{}m", ctx.output_color));
    }
}

/// The bar's total/elapsed/border character for block i: with both elapsed
/// borders set the border characters themselves stand in for the value
/// (borderAsValue), so the first and last blocks print them.
fn bar_block(d: &DisplayFormat, border_as_value: bool, i: u8, elapsed: bool) -> &str {
    if border_as_value && i == 0 {
        if elapsed {
            return &d.bar_border_left_elapsed;
        }
        return &d.bar_border_left;
    }
    if border_as_value && i == d.bar_width - 1 {
        if elapsed {
            return &d.bar_border_right_elapsed;
        }
        return &d.bar_border_right;
    }
    if elapsed {
        &d.bar_char_elapsed
    } else {
        &d.bar_char_total
    }
}

/// ffPercentAppendBar (common/impl/percent.c): the `[ ■■■■------ ]` bar.
/// The percent bands (green/yellow/red) are per block unless the elapsed
/// color is a fixed one or the monochrome bit is set, in which case the
/// whole elapsed run takes the band color of the percent itself.
pub fn percent_append_bar(out: &mut String, percent: f64, config: PercentModuleConfig, ctx: &PrintCtx, d: &DisplayFormat) {
    let green = config.green;
    let yellow = config.yellow;

    let border_as_value = !d.bar_border_left_elapsed.is_empty() && !d.bar_border_right_elapsed.is_empty();

    if !border_as_value && !d.bar_border_left.is_empty() {
        if !ctx.pipe && !d.bar_color_border.is_empty() {
            out.push_str(&format!("\x1b[{}m", d.bar_color_border));
        }
        out.push_str(&d.bar_border_left);
    }

    if percent == f64::MIN {
        // -DBL_MAX: the unset sentinel draws a full bar of total
        // characters, light-black when a total color is configured.
        if !ctx.pipe && !d.bar_color_total.is_empty() {
            out.push_str(&format!("\x1b[{FG_LIGHT_BLACK}m"));
        }
        for i in 0..d.bar_width {
            out.push_str(bar_block(d, border_as_value, i, false));
        }
    } else {
        let percent_type_flags = if config.type_flags == 0 { d.percent_type } else { config.type_flags };

        let blocks_percent = if percent > 0.0 {
            let blocks = percent / 100.0 * f64::from(d.bar_width) + 0.5;
            if blocks >= f64::from(d.bar_width) {
                d.bar_width
            } else {
                blocks as u8
            }
        } else {
            0
        };

        let auto_color_elapsed = d.bar_color_elapsed.eq_ignore_ascii_case("auto");
        let monochrome = percent_type_flags & percent_type::BAR_MONOCHROME_BIT != 0 || !auto_color_elapsed;

        if !ctx.pipe && !d.bar_color_elapsed.is_empty() && monochrome {
            let color = if !auto_color_elapsed {
                d.bar_color_elapsed.as_str()
            } else if green <= yellow {
                if percent < f64::from(green) {
                    d.percent_color_green.as_str()
                } else if percent < f64::from(yellow) {
                    d.percent_color_yellow.as_str()
                } else {
                    d.percent_color_red.as_str()
                }
            } else if percent < f64::from(yellow) {
                d.percent_color_red.as_str()
            } else if percent < f64::from(green) {
                d.percent_color_yellow.as_str()
            } else {
                d.percent_color_green.as_str()
            };
            out.push_str(&format!("\x1b[{color}m"));
        }

        for i in 0..blocks_percent {
            if !ctx.pipe && !d.bar_color_elapsed.is_empty() && !monochrome {
                // Per-block bands: the section boundaries are the
                // thresholds scaled onto the bar width (C's truncating
                // uint32_t cast), so long bars fade through all three
                // colors at their own positions.
                let section1_begin = (f64::from(if green <= yellow { green } else { yellow }) / 100.0 * f64::from(d.bar_width) + 0.5) as u32;
                let section2_begin = (f64::from(if green > yellow { green } else { yellow }) / 100.0 * f64::from(d.bar_width) + 0.5) as u32;
                if i as u32 == section2_begin {
                    out.push_str(&format!("\x1b[{}m", if green > yellow { d.percent_color_green.as_str() } else { d.percent_color_red.as_str() }));
                } else if i as u32 == section1_begin {
                    out.push_str(&format!("\x1b[{}m", d.percent_color_yellow));
                } else if i == 0 {
                    out.push_str(&format!("\x1b[{}m", if green <= yellow { d.percent_color_green.as_str() } else { d.percent_color_red.as_str() }));
                }
            }
            out.push_str(bar_block(d, border_as_value, i, true));
        }

        if blocks_percent < d.bar_width {
            if !ctx.pipe && !d.bar_color_total.is_empty() {
                out.push_str(&format!("\x1b[{}m", d.bar_color_total));
            }
            for i in blocks_percent..d.bar_width {
                out.push_str(bar_block(d, border_as_value, i, false));
            }
        }
    }

    if !border_as_value && !d.bar_border_right.is_empty() {
        if !ctx.pipe && !d.bar_color_border.is_empty() {
            out.push_str(&format!("\x1b[{}m", d.bar_color_border));
        }
        out.push_str(&d.bar_border_right);
    }

    if !ctx.pipe
        && (!d.bar_color_elapsed.is_empty() || !d.bar_color_total.is_empty() || !d.bar_color_border.is_empty())
    {
        out.push_str("\x1b[m");
        append_output_color(out, ctx);
    }
}

/// ffPercentAppendNum: the `(87%)` number. Percent width/ndigits come from
/// display (printf's `%*.*f%%`); the color follows the module's bands with
/// the strict `>` comparisons (a percent sitting exactly on a threshold
/// takes the higher band's color, unlike the bar's `<` walk).
pub fn percent_append_num(out: &mut String, percent: f64, config: PercentModuleConfig, parentheses: bool, ctx: &PrintCtx, d: &DisplayFormat) {
    let green = config.green;
    let yellow = config.yellow;
    let percent_type_flags = if config.type_flags == 0 { d.percent_type } else { config.type_flags };
    let colored = percent_type_flags & percent_type::NUM_COLOR_BIT != 0;

    if parentheses {
        out.push('(');
    }

    if colored && !ctx.pipe {
        let code = if percent == f64::MIN {
            FG_LIGHT_BLACK
        } else if green <= yellow {
            if percent > f64::from(yellow) {
                d.percent_color_red.as_str()
            } else if percent > f64::from(green) {
                d.percent_color_yellow.as_str()
            } else {
                d.percent_color_green.as_str()
            }
        } else if percent < f64::from(yellow) {
            d.percent_color_red.as_str()
        } else if percent < f64::from(green) {
            d.percent_color_yellow.as_str()
        } else {
            d.percent_color_green.as_str()
        };
        out.push_str(&format!("\x1b[{code}m"));
    }

    if percent == f64::MIN {
        out.push('-');
    } else {
        let space = if d.percent_space_before_unit == SpaceBeforeUnit::Always { " " } else { "" };
        out.push_str(&format!("{percent:w$.p$}{space}%", w = usize::from(d.percent_width), p = d.percent_ndigits as usize));
    }

    if colored && !ctx.pipe {
        out.push_str("\x1b[m");
        append_output_color(out, ctx);
    }

    if parentheses {
        out.push(')');
    }
}

/// ffTempsAppendNum: the temperature with its unit, colored by the module's
/// bands whenever escapes are allowed (unlike the percent number, there is
/// no bit to disable that). Kelvin keeps a space before the unit unless
/// NEVER — the inverted polarity of the other units.
pub fn temps_append_num(out: &mut String, celsius: f64, config: ColorRangeConfig, ctx: &PrintCtx, d: &DisplayFormat) {
    if celsius == f64::MIN {
        return;
    }
    let green = config.green;
    let yellow = config.yellow;

    if !ctx.pipe {
        let code = if green <= yellow {
            if celsius > f64::from(yellow) {
                d.temp_color_red.as_str()
            } else if celsius > f64::from(green) {
                d.temp_color_yellow.as_str()
            } else {
                d.temp_color_green.as_str()
            }
        } else if celsius < f64::from(yellow) {
            d.temp_color_red.as_str()
        } else if celsius < f64::from(green) {
            d.temp_color_yellow.as_str()
        } else {
            d.temp_color_green.as_str()
        };
        out.push_str(&format!("\x1b[{code}m"));
    }

    match d.temp_unit {
        TemperatureUnit::Default | TemperatureUnit::Celsius => {
            let space = if d.temp_space_before_unit == SpaceBeforeUnit::Always { " " } else { "" };
            out.push_str(&format!("{celsius:.p$}{space}°C", p = d.temp_ndigits as usize));
        },
        TemperatureUnit::Fahrenheit => {
            let space = if d.temp_space_before_unit == SpaceBeforeUnit::Always { " " } else { "" };
            out.push_str(&format!("{:.p$}{space}°F", celsius * 1.8 + 32.0, p = d.temp_ndigits as usize));
        },
        TemperatureUnit::Kelvin => {
            let space = if d.temp_space_before_unit == SpaceBeforeUnit::Never { "" } else { " " };
            out.push_str(&format!("{:.p$}{space}K", celsius + 273.15, p = d.temp_ndigits as usize));
        },
    }

    if !ctx.pipe {
        out.push_str("\x1b[m");
        append_output_color(out, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn double(value: f64, precision: i8, trailing_zeros: bool) -> String {
        let mut out = String::new();
        append_double(&mut out, value, precision, trailing_zeros);
        out
    }

    #[test]
    fn append_double_fixed_matches_the_installed_binary() {
        // The reference cases behind fmt_bytes' old half-up emulation.
        assert_eq!(double(14.625, 2, true), "14.63");
        assert_eq!(double(1.125, 2, true), "1.13");
        assert_eq!(double(14.875, 2, true), "14.88");
        assert_eq!(double(999.9951171875, 2, true), "1000.00");
        // Trailing zeros kept through the pad walk.
        assert_eq!(double(8.0, 2, true), "8.00");
        assert_eq!(double(1.5, 2, true), "1.50");
        assert_eq!(double(1.05, 2, true), "1.05");
        // At most precision digits, then padded back up.
        assert_eq!(double(1.234, 2, true), "1.23");
        assert_eq!(double(0.04, 2, true), "0.04");
        assert_eq!(double(1e-16, 2, true), "0.00");
        // Exactly 2^-30 (1 byte in GiB units): the fixed path must not
        // round it up at 9 digits.
        assert_eq!(double(9.313225746154785e-10, 9, true), "0.000000001");
        // precision 0 rounds to an integer and strips the ".0".
        assert_eq!(double(2.5, 0, true), "3");
        assert_eq!(double(2.4, 0, true), "2");
        assert_eq!(double(-2.5, 0, true), "-3");
        // Negative precision keeps the shortest form, ".0" stripped.
        assert_eq!(double(8.0, -1, true), "8");
        assert_eq!(double(1234.5, -1, true), "1234.5");
        assert_eq!(double(0.5, -1, false), "0.5");
        // trailing_zeros=false strips the integral ".0" too.
        assert_eq!(double(8.0, 2, false), "8");
        assert_eq!(double(1.5, 2, false), "1.5");
        // Signs and -0.0.
        assert_eq!(double(-1.25, 2, true), "-1.25");
        assert_eq!(double(-0.4, 0, false), "-0");
        assert_eq!(double(-0.0, 2, true), "-0.00");
        // Zero and subnormals.
        assert_eq!(double(0.0, 2, true), "0.00");
        assert_eq!(double(f64::MIN_POSITIVE * 0.5, 2, true), "0.00");
    }

    #[test]
    fn append_double_shortest_thresholds_match_json_stringify() {
        assert_eq!(double(1e-5, -1, true), "0.00001");
        assert_eq!(double(9.99e-6, -1, true), "0.00000999");
        assert_eq!(double(1e-6, -1, true), "0.000001");
        assert_eq!(double(9.9e-7, -1, true), "9.9e-7");
        assert_eq!(double(1.2345678901234567e19, -1, true), "12345678901234567000");
        assert_eq!(double(12345678901234567000.0, -1, false), "12345678901234567000");
    }

    #[test]
    fn append_double_scientific_past_1e21() {
        assert_eq!(double(2e34, 2, true), "2e34");
        assert_eq!(double(1.5e300, 0, true), "1.5e300");
    }

    fn size(bytes: u64, d: &DisplayFormat) -> String {
        let mut out = String::new();
        size_append_num(&mut out, bytes, d);
        out
    }

    #[test]
    fn size_append_num_defaults() {
        let d = DisplayFormat::default();
        assert_eq!(size(999, &d), "999 B");
        assert_eq!(size(1024, &d), "1.00 KiB");
        assert_eq!(size(1024 * 1024 * 1024 * 58, &d), "58.00 GiB");
        assert_eq!(size(117 * (1 << 27), &d), "14.63 GiB");
        // SI prefixes and base 1000.
        let si = DisplayFormat { size_binary_prefix: SizeBinaryPrefix::Si, ..d.clone() };
        assert_eq!(size(1000, &si), "1.00 kB");
        assert_eq!(size(1000 * 1000, &si), "1.00 MB");
        assert_eq!(size(999, &si), "999 B");
        // JEDEC caps at TB: the prefix table ends after TB, so a PiB
        // still divides four times and prints 1024 TB (upstream's exact
        // behavior — the null terminator stops the loop, not the value).
        let jedec = DisplayFormat { size_binary_prefix: SizeBinaryPrefix::Jedec, ..d.clone() };
        assert_eq!(size(1024, &jedec), "1.00 KB");
        assert_eq!(size(1024u64.pow(5), &jedec), "1024.00 TB");
        // maxPrefix stops the division (kB at index 1).
        let capped = DisplayFormat { size_max_prefix: 1, ..d.clone() };
        assert_eq!(size(1024 * 1024, &capped), "1024.00 KiB");
        // More digits.
        let digits = DisplayFormat { size_ndigits: 0, ..d.clone() };
        assert_eq!(size(1024, &digits), "1 KiB");
        let digits5 = DisplayFormat { size_ndigits: 5, ..d.clone() };
        assert_eq!(size(1024 * 3 + 700, &digits5), "3.68359 KiB");
        // space-before-unit never.
        let tight = DisplayFormat { size_space_before_unit: SpaceBeforeUnit::Never, ..d.clone() };
        assert_eq!(size(1024, &tight), "1.00KiB");
    }

    fn duration(seconds: u64, d: &DisplayFormat) -> String {
        let mut out = String::new();
        duration_append_num(&mut out, seconds, d);
        out
    }

    #[test]
    fn duration_append_num_defaults() {
        let d = DisplayFormat::default();
        assert_eq!(duration(45, &d), "45 seconds");
        assert_eq!(duration(1, &d), "1 second");
        assert_eq!(duration(3299, &d), "55 mins");
        assert_eq!(duration(3269, &d), "54 mins");
        assert_eq!(duration(90_000, &d), "1 day, 1 hour");
        assert_eq!(duration(95_400, &d), "1 day, 2 hours, 30 mins");
        assert_eq!(duration(8_640_000, &d), "100 days(!)");
        assert_eq!(duration(8_639_940, &d), "99 days, 23 hours, 59 mins");
    }

    #[test]
    fn duration_append_num_abbreviated() {
        let d = DisplayFormat { duration_abbreviation: true, ..Default::default() };
        // Every unit carries its own space in the default space mode, so
        // "7 h 33 m" — the join spaces only follow longer units.
        assert_eq!(duration(45, &d), "45 secs");
        assert_eq!(duration(1, &d), "1 sec");
        assert_eq!(duration(3299, &d), "55 m");
        assert_eq!(duration(95_400, &d), "1 d 2 h 30 m");
        assert_eq!(duration(90_000, &d), "1 d 1 h");
        assert_eq!(duration(8_640_000, &d), "100 d");
        assert_eq!(duration(3_600, &d), "1 h");
        assert_eq!(duration(3_600 + 60, &d), "1 h 1 m");
        assert_eq!(duration(86_400, &d), "1 d");
    }

    #[test]
    fn duration_append_num_space_never() {
        let d = DisplayFormat { duration_space_before_unit: SpaceBeforeUnit::Never, ..Default::default() };
        assert_eq!(duration(45, &d), "45seconds");
        assert_eq!(duration(95_400, &d), "1day, 2hours, 30mins");
    }

    fn freq(mhz: u32, d: &DisplayFormat) -> String {
        let mut out = String::new();
        let appended = freq_append_num(&mut out, mhz, d);
        assert!(appended || out.is_empty());
        out
    }

    #[test]
    fn freq_append_num_defaults() {
        let d = DisplayFormat::default();
        assert_eq!(freq(0, &d), "");
        assert_eq!(freq(4400, &d), "4.40 GHz");
        assert_eq!(freq(2100, &d), "2.10 GHz");
        let raw = DisplayFormat { freq_ndigits: -1, ..d.clone() };
        assert_eq!(freq(4400, &raw), "4400 MHz");
        let none = DisplayFormat { freq_space_before_unit: SpaceBeforeUnit::Never, ..d.clone() };
        assert_eq!(freq(4400, &none), "4.40GHz");
    }

    /// A dark-theme display (the COLORFGBG-less default), pinned so the
    /// assertions don't depend on the test environment's env.
    fn dark_display() -> DisplayFormat {
        let d = DisplayFormat::default();
        DisplayFormat {
            temp_color_yellow: FG_LIGHT_YELLOW.to_string(),
            temp_color_red: FG_LIGHT_RED.to_string(),
            percent_color_yellow: FG_LIGHT_YELLOW.to_string(),
            percent_color_red: FG_LIGHT_RED.to_string(),
            bar_color_total: FG_LIGHT_WHITE.to_string(),
            bar_color_border: FG_LIGHT_WHITE.to_string(),
            ..d
        }
    }

    fn percent_num(percent: f64, config: PercentModuleConfig, parentheses: bool, ctx: &PrintCtx, d: &DisplayFormat) -> String {
        let mut out = String::new();
        percent_append_num(&mut out, percent, config, parentheses, ctx, d);
        out
    }

    fn percent_bar(percent: f64, config: PercentModuleConfig, ctx: &PrintCtx, d: &DisplayFormat) -> String {
        let mut out = String::new();
        percent_append_bar(&mut out, percent, config, ctx, d);
        out
    }

    fn temps_num(celsius: f64, config: ColorRangeConfig, ctx: &PrintCtx, d: &DisplayFormat) -> String {
        let mut out = String::new();
        temps_append_num(&mut out, celsius, config, ctx, d);
        out
    }

    #[test]
    fn percent_append_num_band_colors() {
        let d = dark_display();
        let plain = PrintCtx { pipe: true, output_color: String::new() };
        let tty = PrintCtx { pipe: false, output_color: String::new() };
        // The default type (NUM|NUM_COLOR) colors whenever escapes go out;
        // ascending bands color red past yellow (memory's {50, 80}).
        assert_eq!(percent_num(87.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, false, &plain, &d), "87%");
        assert_eq!(percent_num(87.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, false, &tty, &d), "\x1b[91m87%\x1b[m");
        assert_eq!(percent_num(60.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, false, &tty, &d), "\x1b[93m60%\x1b[m");
        assert_eq!(percent_num(50.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, false, &tty, &d), "\x1b[32m50%\x1b[m");
        // Descending bands (battery's {50, 20}): red below yellow, green
        // from green up.
        assert_eq!(percent_num(10.0, PercentModuleConfig { green: 50, yellow: 20, type_flags: 0 }, false, &tty, &d), "\x1b[91m10%\x1b[m");
        assert_eq!(percent_num(30.0, PercentModuleConfig { green: 50, yellow: 20, type_flags: 0 }, false, &tty, &d), "\x1b[93m30%\x1b[m");
        assert_eq!(percent_num(60.0, PercentModuleConfig { green: 50, yellow: 20, type_flags: 0 }, false, &tty, &d), "\x1b[32m60%\x1b[m");
        // A percent sitting exactly on a threshold stays in the lower
        // band (strict >): 80 does not exceed yellow.
        assert_eq!(percent_num(20.0, PercentModuleConfig { green: 50, yellow: 20, type_flags: 0 }, false, &tty, &d), "\x1b[93m20%\x1b[m");
        // The module's own type override can silence the color bit.
        let uncolored = PercentModuleConfig { green: 50, yellow: 80, type_flags: percent_type::NUM_BIT };
        assert_eq!(percent_num(87.0, uncolored, false, &tty, &d), "87%");
        // The unset sentinel prints "-" in the light-black no-value color.
        assert_eq!(percent_num(f64::MIN, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, false, &tty, &d), "\x1b[90m-\x1b[m");
        assert_eq!(percent_num(f64::MIN, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, false, &plain, &d), "-");
    }

    #[test]
    fn percent_append_num_parens_width_and_tail() {
        let d = dark_display();
        let tty = PrintCtx { pipe: false, output_color: String::new() };
        assert_eq!(percent_num(87.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, true, &tty, &d), "(\x1b[91m87%\x1b[m)");
        // The module's output color rides the reset tail; pipe mode drops
        // the whole wrap.
        let styled = PrintCtx { pipe: false, output_color: "1".to_string() };
        assert_eq!(percent_num(87.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, false, &styled, &d), "\x1b[91m87%\x1b[m\x1b[1m");
        let styled_pipe = PrintCtx { pipe: true, output_color: "1".to_string() };
        assert_eq!(percent_num(87.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, false, &styled_pipe, &d), "87%");
        // printf's %*.*f: width pads, ndigits keeps decimals, the space
        // goes between number and %.
        let wide = DisplayFormat { percent_width: 6, percent_ndigits: 1, percent_space_before_unit: SpaceBeforeUnit::Always, ..dark_display() };
        assert_eq!(percent_num(87.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, false, &tty, &wide), "\x1b[91m  87.0 %\x1b[m");
    }

    #[test]
    fn percent_append_bar_pipe_and_defaults() {
        let d = dark_display();
        let plain = PrintCtx { pipe: true, output_color: String::new() };
        // Pipe mode: plain characters, borders, no colors anywhere.
        assert_eq!(percent_bar(87.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, &plain, &d), "[ ■■■■■■■■■- ]");
        assert_eq!(percent_bar(0.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, &plain, &d), "[ ---------- ]");
        assert_eq!(percent_bar(100.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, &plain, &d), "[ ■■■■■■■■■■ ]");
        // The rounding is blocks = pct/100*width + 0.5, truncated.
        assert_eq!(percent_bar(95.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, &plain, &d), "[ ■■■■■■■■■■ ]");
        assert_eq!(percent_bar(94.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, &plain, &d), "[ ■■■■■■■■■- ]");
        // The unset sentinel draws an all-total bar.
        assert_eq!(percent_bar(f64::MIN, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, &plain, &d), "[ ---------- ]");
    }

    #[test]
    fn percent_append_bar_colors() {
        let d = dark_display();
        let tty = PrintCtx { pipe: false, output_color: String::new() };
        let cfg = PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 };
        // Per-block bands: the section boundaries sit at green and yellow
        // scaled onto the bar width (5 and 8 for 50/80 at width 10).
        assert_eq!(
            percent_bar(87.0, cfg, &tty, &d),
            "\x1b[97m[ \x1b[32m■■■■■\x1b[93m■■■\x1b[91m■\x1b[97m-\x1b[97m ]\x1b[m"
        );
        // 25%: two elapsed blocks (2.5 + 0.5 → 3? 25/100*10+0.5 = 3), all
        // under the first section, then total.
        assert_eq!(percent_bar(25.0, cfg, &tty, &d), "\x1b[97m[ \x1b[32m■■■\x1b[97m-------\x1b[97m ]\x1b[m");
        // The sentinel uses the light-black value color for the body.
        assert_eq!(percent_bar(f64::MIN, cfg, &tty, &d), "\x1b[97m[ \x1b[90m----------\x1b[97m ]\x1b[m");
        // The monochrome bit colors the whole elapsed run by band instead.
        let mono = DisplayFormat { percent_type: percent_type::BAR_BIT | percent_type::BAR_MONOCHROME_BIT, ..dark_display() };
        assert_eq!(
            percent_bar(87.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, &tty, &mono),
            "\x1b[97m[ \x1b[91m■■■■■■■■■\x1b[97m-\x1b[97m ]\x1b[m"
        );
        // A fixed elapsed color replaces the auto bands; the total keeps
        // its own color. An empty elapsed color disables all elapsed
        // color but the total's remains.
        let fixed = DisplayFormat { bar_color_elapsed: "35".to_string(), ..dark_display() };
        assert_eq!(
            percent_bar(87.0, cfg, &tty, &fixed),
            "\x1b[97m[ \x1b[35m■■■■■■■■■\x1b[97m-\x1b[97m ]\x1b[m"
        );
        let clear = DisplayFormat { bar_color_elapsed: String::new(), ..dark_display() };
        assert_eq!(percent_bar(87.0, cfg, &tty, &clear), "\x1b[97m[ ■■■■■■■■■\x1b[97m-\x1b[97m ]\x1b[m");
    }

    #[test]
    fn percent_append_bar_border_as_value() {
        let d = DisplayFormat {
            bar_border_left_elapsed: "<<".to_string(),
            bar_border_right_elapsed: ">>".to_string(),
            ..dark_display()
        };
        let plain = PrintCtx { pipe: true, output_color: String::new() };
        // The elapsed borders replace the value characters at the ends
        // and the outer borders disappear — but the total run keeps the
        // OUTER border characters at its own ends (upstream's exact
        // asymmetric wiring).
        assert_eq!(
            percent_bar(30.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, &plain, &d),
            "<<■■------ ]"
        );
        // A full bar is all elapsed: the elapsed borders cap both ends,
        // eight ■ between them.
        assert_eq!(
            percent_bar(100.0, PercentModuleConfig { green: 50, yellow: 80, type_flags: 0 }, &plain, &d),
            "<<■■■■■■■■>>"
        );
    }

    #[test]
    fn temps_append_num_defaults() {
        let d = dark_display();
        let plain = PrintCtx { pipe: true, output_color: String::new() };
        let tty = PrintCtx { pipe: false, output_color: String::new() };
        let range = ColorRangeConfig { green: 60, yellow: 80 };
        assert_eq!(temps_num(45.0, range, &plain, &d), "45.0°C");
        assert_eq!(temps_num(45.0, range, &tty, &d), "\x1b[32m45.0°C\x1b[m");
        assert_eq!(temps_num(70.0, range, &tty, &d), "\x1b[93m70.0°C\x1b[m");
        assert_eq!(temps_num(95.5, range, &tty, &d), "\x1b[91m95.5°C\x1b[m");
        // A temperature on the threshold stays in the cooler band.
        assert_eq!(temps_num(80.0, range, &tty, &d), "\x1b[93m80.0°C\x1b[m");
        // The output color rides the reset tail.
        let styled = PrintCtx { pipe: false, output_color: "1".to_string() };
        assert_eq!(temps_num(45.0, range, &styled, &d), "\x1b[32m45.0°C\x1b[m\x1b[1m");
        // The unset sentinel appends nothing at all.
        let mut empty = String::from("keep");
        temps_append_num(&mut empty, f64::MIN, range, &tty, &d);
        assert_eq!(empty, "keep");
    }

    #[test]
    fn temps_append_num_units_and_spaces() {
        let plain = PrintCtx { pipe: true, output_color: String::new() };
        let range = ColorRangeConfig { green: 60, yellow: 80 };
        let fahrenheit = DisplayFormat { temp_unit: TemperatureUnit::Fahrenheit, ..dark_display() };
        assert_eq!(temps_num(100.0, range, &plain, &fahrenheit), "212.0°F");
        // Kelvin keeps its space unless NEVER — the inverted polarity.
        let kelvin = DisplayFormat { temp_unit: TemperatureUnit::Kelvin, ..dark_display() };
        assert_eq!(temps_num(0.0, range, &plain, &kelvin), "273.1 K");
        let kelvin_tight = DisplayFormat { temp_unit: TemperatureUnit::Kelvin, temp_space_before_unit: SpaceBeforeUnit::Never, ..dark_display() };
        assert_eq!(temps_num(0.0, range, &plain, &kelvin_tight), "273.1K");
        let spaced = DisplayFormat { temp_space_before_unit: SpaceBeforeUnit::Always, ..dark_display() };
        assert_eq!(temps_num(45.0, range, &plain, &spaced), "45.0 °C");
        let more_digits = DisplayFormat { temp_ndigits: 2, ..dark_display() };
        assert_eq!(temps_num(45.0, range, &plain, &more_digits), "45.00°C");
    }
}
