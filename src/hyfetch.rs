//! rustfetch-only HyFetch-style pride-flag logo coloring.
//!
//! This is NOT a fastfetch feature: there is no upstream byte-parity target
//! for it, only the guarantee that leaving it unset changes nothing (see
//! `scripts/hyfetchtest.sh`). Raw logo types ignore the palette, as upstream
//! does for `--logo-color-N` too.
//!
//! Config surface (`logo.hyfetch`, off by default):
//!
//! ```jsonc
//! "logo": { "hyfetch": {
//!   "preset": "transgender",
//!   "mode": "24bit",
//!   "lightness": null,
//!   "customPresets": { "myflag": ["#FF0000", "#00FF00", "#0000FF"] }
//! } }
//! ```
//!
//! `preset` also accepts an array or a comma-separated string; several
//! names pick one at random (seeded from the clock and pid, no dependency),
//! and `random` picks from vendored ∪ custom presets. Custom names shadow
//! vendored ones. `mode` accepts `24bit`/`rgb`/`truecolor` and
//! `8bit`/`ansi256`/`256color`, case-insensitively. `lightness` is null
//! (vendored hex, byte-exact) or a number in 0..=1: a number applies
//! OKHSL ClampMin with the Ottosson matrices. The conversion is necessarily
//! approximate — matching hyfetch's `palette` crate byte-for-byte would
//! require a dependency this port does not have.
//!
//! New flags (they are intentionally absent from the verbatim fastfetch
//! help captures), parsed in `cli.rs` and merged per-field after the config:
//!
//! - `--hyfetch-preset <name[,name...]>`
//! - `--hyfetch-mode <24bit|8bit>`
//! - `--hyfetch-lightness <0.0..1.0>`

use crate::hyfetch_presets;

/// The two foreground SGR families hyfetch supports; 16-color is not used.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AnsiMode {
    /// `\e[38;2;R;G;Bm`, spelled `24bit`/`rgb`/`truecolor`.
    #[default]
    Rgb,
    /// `\e[38;5;Nm`, spelled `8bit`/`ansi256`/`256color`.
    Ansi256,
}

/// A resolved `logo.hyfetch` object before preset selection.
#[derive(Clone, Debug, Default)]
pub struct Config {
    /// Explicit preset name(s), or one `random` to pick from all names.
    /// Empty means the feature is off.
    pub presets: Vec<String>,
    /// Color encoding, `24bit` when the key is absent.
    pub mode: AnsiMode,
    /// OKHSL ClampMin floor; `None` keeps the vendored RGB bytes verbatim.
    pub lightness: Option<f64>,
    /// User presets, checked before the vendored table by name.
    pub custom: Vec<(String, Vec<u32>)>,
}

/// `parse_hex` on an RGB integer: `#` optional, exactly 3 or 6 hex digits.
pub fn parse_hex(input: &str) -> Option<u32> {
    let digits = input.strip_prefix('#').unwrap_or(input);
    match digits.len() {
        3 => {
            let mut value = 0u32;
            for digit in digits.chars() {
                let nibble = digit.to_digit(16)?;
                value = value.checked_mul(256)?.checked_add(nibble.checked_mul(17)?)?;
            }
            Some(value)
        },
        6 => u32::from_str_radix(digits, 16).ok(),
        _ => None,
    }
}

/// Case-insensitive `logo.hyfetch.mode` names.
pub fn parse_mode(text: &str) -> Option<AnsiMode> {
    match text.to_ascii_lowercase().as_str() {
        "24bit" | "rgb" | "truecolor" => Some(AnsiMode::Rgb),
        "8bit" | "ansi256" | "256color" => Some(AnsiMode::Ansi256),
        _ => None,
    }
}

/// A comma-separated `preset` string (and each element of a preset array)
/// becomes a list of candidate names; blank elements are skipped.
pub fn split_preset_names(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `customPresets` values: a non-empty hex list, all parsed with
/// [`parse_hex`] so `#RGB`, six digits and the missing-`#` form all work.
pub fn parse_custom(colors: &[String]) -> Result<Vec<u32>, String> {
    if colors.is_empty() {
        return Err("a custom preset must contain at least one color".to_owned());
    }
    colors
        .iter()
        .map(|hex| parse_hex(hex).ok_or_else(|| format!("invalid hex color: {hex}")))
        .collect()
}

/// `random` is a selector, never a preset name a user may define.
pub fn validate_custom_name(name: &str) -> Result<(), String> {
    if name == "random" {
        return Err("reserved preset name: random".to_owned());
    }
    Ok(())
}

/// Exact port of hyfetch's `ColorProfile::with_length` (8 = the logo has
/// nine palette slots): weights are expanded first, then each of the
/// resulting colors gets `9 / orig` copies plus a share of the remainder —
/// one for the center when it is odd, then one for each outermost pair.
/// The expanded list is never truncated when it exceeds nine entries.
pub fn with_length_9(colors: &[(u32, u32)]) -> Vec<u32> {
    let mut weighted = Vec::new();
    for (color, weight) in colors {
        for _ in 0..*weight {
            weighted.push(*color);
        }
    }
    if weighted.is_empty() {
        return Vec::new();
    }
    let orig = weighted.len() as i64;
    let center = (orig / 2) as usize;
    let repeats = 9_i64.div_euclid(orig);
    let mut extras = 9_i64.rem_euclid(orig);
    let mut weights = vec![repeats as usize; weighted.len()];
    if extras % 2 == 1 {
        if let Some(slot) = weights.get_mut(center) {
            *slot = slot.saturating_add(1);
        }
        extras -= 1;
    }
    for i in 0..(extras / 2) as usize {
        if let Some(slot) = weights.get_mut(i) {
            *slot = slot.saturating_add(1);
        }
        let opposite = (orig as usize).saturating_sub(1).saturating_sub(i);
        if let Some(slot) = weights.get_mut(opposite) {
            *slot = slot.saturating_add(1);
        }
    }
    let mut out = Vec::with_capacity(9);
    for (index, color) in weighted.iter().enumerate() {
        let times = weights.get(index).copied().unwrap_or(0);
        for _ in 0..times {
            out.push(*color);
        }
    }
    out
}

/// sRGB transfer function, decoded to linear light.
fn linearize(channel: f64) -> f64 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear light back to the sRGB transfer function.
fn encode(channel: f64) -> f64 {
    let clamped = channel.clamp(0.0, 1.0);
    if clamped <= 0.0031308 {
        clamped * 12.92
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    }
}

/// Ottosson's linear-sRGB → OKLab matrices (via cube-root LMS).
fn rgb_to_oklab(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    (
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    )
}

/// Ottosson's OKLab → linear-sRGB matrices.
fn oklab_to_rgb(l: f64, a: f64, b: f64) -> (f64, f64, f64) {
    let l_ = l + 0.3963377774 * a + 0.2158037573 * b;
    let m_ = l - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = l - 0.0894841775 * a - 1.2914855480 * b;
    let (l_, m_, s_) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    (
        4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_,
        -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_,
        -0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_,
    )
}

/// Dependency-free OKHSL ClampMin (`L = max(L, requested)`), keeping the
/// original chroma and hue. Values are approximate by design; byte-parity
/// with hyfetch's `palette` crate is not possible without dependencies.
pub fn clamp_min_lightness(color: u32, requested: f64) -> u32 {
    let r = linearize((color >> 16 & 0xff) as f64 / 255.0);
    let g = linearize((color >> 8 & 0xff) as f64 / 255.0);
    let b = linearize((color & 0xff) as f64 / 255.0);
    let (l, a, b) = rgb_to_oklab(r, g, b);
    let chroma = a.hypot(b);
    let hue = b.atan2(a);
    let saturation = if (1.0 - (2.0 * l - 1.0).abs()) == 0.0 {
        0.0
    } else {
        chroma / (1.0 - (2.0 * l - 1.0).abs())
    };
    let new_l = l.max(requested);
    let new_chroma = (1.0 - (2.0 * new_l - 1.0).abs()) * saturation;
    let (r, g, b) = oklab_to_rgb(new_l, new_chroma * hue.cos(), new_chroma * hue.sin());
    let byte = |value: f64| (encode(value) * 255.0).round().clamp(0.0, 255.0) as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

/// Nearest of the xterm-256 color cube (16..231) and the 24 grays
/// (232..255) by squared RGB distance; the first (lowest) index wins ties.
/// Pure red lands on cube slot 196, not the ANSI base red at slot 9.
pub fn ansi256_index(color: u32) -> u8 {
    let target = [
        (color >> 16 & 0xff) as i64,
        (color >> 8 & 0xff) as i64,
        (color & 0xff) as i64,
    ];
    let cube = [0_i64, 95, 135, 175, 215, 255];
    let mut best: (i64, u8) = (i64::MAX, 0);
    for (ri, r) in cube.iter().enumerate() {
        for (gi, g) in cube.iter().enumerate() {
            for (bi, b) in cube.iter().enumerate() {
                let index = 16 + 36 * ri + 6 * gi + bi;
                let distance = distance(&target, &[*r, *g, *b]);
                if distance < best.0 {
                    best = (distance, index as u8);
                }
            }
        }
    }
    for step in 0_i64..24 {
        let gray = 8 + 10 * step;
        let distance = distance(&target, &[gray, gray, gray]);
        if distance < best.0 {
            best = (distance, (232 + step) as u8);
        }
    }
    best.1
}

fn distance(a: &[i64; 3], b: &[i64; 3]) -> i64 {
    let dr = a[0] - b[0];
    let dg = a[1] - b[1];
    let db = a[2] - b[2];
    dr * dr + dg * dg + db * db
}

/// An SGR *parameter string* (no `\e[` / `m`) for one palette slot.
pub fn sgr(color: u32, mode: AnsiMode) -> String {
    let r = color >> 16 & 0xff;
    let g = color >> 8 & 0xff;
    let b = color & 0xff;
    match mode {
        AnsiMode::Rgb => format!("38;2;{r};{g};{b}"),
        AnsiMode::Ansi256 => format!("38;5;{}", ansi256_index(color)),
    }
}

/// Config + CLI merge, per-field, CLI last, like `logo.color.N`. A preset
/// from either source enables the feature; absent preset means off.
pub fn merge(
    base: Option<Config>,
    preset: Option<&str>,
    mode: Option<AnsiMode>,
    lightness: Option<f64>,
) -> Option<Config> {
    let mut config = base.unwrap_or_default();
    if let Some(preset) = preset {
        config.presets = split_preset_names(preset);
    }
    if let Some(mode) = mode {
        config.mode = mode;
    }
    if let Some(lightness) = lightness {
        config.lightness = Some(lightness);
    }
    if config.presets.is_empty() {
        None
    } else {
        Some(config)
    }
}

impl Config {
    fn known(&self, name: &str) -> bool {
        self.custom.iter().any(|(custom, _)| custom == name)
            || hyfetch_presets::PRESETS.iter().any(|(vendored, _)| *vendored == name)
    }

    fn available_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.custom.iter().map(|(name, _)| name.clone()).collect();
        for (name, _) in hyfetch_presets::PRESETS {
            if !names.iter().any(|known| known == name) {
                names.push((*name).to_owned());
            }
        }
        names
    }

    fn pick_name(&self) -> Result<String, String> {
        if self.presets.is_empty() {
            return Err("no hyfetch preset selected".to_owned());
        }
        for name in &self.presets {
            if name != "random" && !self.known(name) {
                return Err(format!("unknown hyfetch preset: {name}"));
            }
        }
        if self.presets.iter().any(|name| name == "random") {
            let names = self.available_names();
            return names
                .get(random_index(names.len()))
                .cloned()
                .ok_or_else(|| "unknown hyfetch preset: random".to_owned());
        }
        self.presets
            .get(random_index(self.presets.len()))
            .cloned()
            .ok_or_else(|| "no hyfetch preset selected".to_owned())
    }

    fn preset_colors(&self, name: &str) -> Option<Vec<(u32, u32)>> {
        if let Some((_, colors)) = self.custom.iter().find(|(custom, _)| custom == name) {
            return Some(colors.iter().map(|color| (*color, 1)).collect());
        }
        hyfetch_presets::PRESETS
            .iter()
            .find(|(vendored, _)| *vendored == name)
            .map(|(_, colors)| {
                colors
                    .iter()
                    .filter_map(|(hex, weight)| parse_hex(hex).map(|color| (color, *weight)))
                    .collect()
            })
    }

    /// Nine palette slot strings, ready for `logo_color_overrides`.
    pub fn resolve(&self) -> Result<Vec<String>, String> {
        let name = self.pick_name()?;
        let colors = self
            .preset_colors(&name)
            .ok_or_else(|| format!("unknown hyfetch preset: {name}"))?;
        let colors = match self.lightness {
            Some(requested) => colors
                .into_iter()
                .map(|(color, weight)| (clamp_min_lightness(color, requested), weight))
                .collect(),
            None => colors,
        };
        let palette = with_length_9(&colors);
        Ok(palette
            .into_iter()
            .map(|color| sgr(color, self.mode))
            .collect())
    }
}

/// A deterministic-enough seed without another dependency: wall-clock
/// nanoseconds mixed with the pid through one xorshift64* step.
fn random_index(len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let mut state = nanos ^ ((std::process::id() as u64) << 32) ^ 0x9e37_79b9_7f4a_7c15;
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    (state as usize) % len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_accepts_optional_hash_three_or_six_digits() {
        assert_eq!(parse_hex("#E50000"), Some(0x00e5_0000));
        assert_eq!(parse_hex("E50000"), Some(0x00e5_0000));
        assert_eq!(parse_hex("#F00"), Some(0x00ff_0000));
        assert_eq!(parse_hex("f00"), Some(0x00ff_0000));
        // The one vendored color without a `#`.
        assert_eq!(parse_hex("51091D"), Some(0x0051_091d));
        for bad in ["", "#", "#FF", "#FFFFF", "#GGGGGG", "##F00", "#FFFFFFF"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn parse_mode_is_case_insensitive() {
        for name in ["24bit", "RGB", "truecolor"] {
            assert_eq!(parse_mode(name), Some(AnsiMode::Rgb), "{name}");
        }
        for name in ["8bit", "Ansi256", "256color"] {
            assert_eq!(parse_mode(name), Some(AnsiMode::Ansi256), "{name}");
        }
        assert_eq!(parse_mode("16bit"), None);
    }

    #[test]
    fn with_length_9_matches_hyfetch_weights() {
        // transgender (5 colors): w = [2, 2, 1, 2, 2].
        let transgender = [(0, 1), (1, 1), (2, 1), (3, 1), (4, 1)];
        assert_eq!(
            with_length_9(&transgender),
            vec![0, 0, 1, 1, 2, 3, 3, 4, 4]
        );
        // nonbinary (4 colors): w = [2, 2, 3, 2].
        let nonbinary = [(0, 1), (1, 1), (2, 1), (3, 1)];
        assert_eq!(with_length_9(&nonbinary), vec![0, 0, 1, 1, 2, 2, 2, 3, 3]);
        // rainbow (6 colors): w = [2, 1, 1, 2, 1, 2].
        let rainbow = [(0, 1), (1, 1), (2, 1), (3, 1), (4, 1), (5, 1)];
        assert_eq!(
            with_length_9(&rainbow),
            vec![0, 0, 1, 2, 3, 3, 4, 5, 5]
        );
        // demisexual expands to exactly nine (weights 2,3,1,3) → all 1.
        let demisexual = [(0, 2), (1, 3), (2, 1), (3, 3)];
        assert_eq!(
            with_length_9(&demisexual),
            vec![0, 0, 1, 1, 1, 2, 3, 3, 3]
        );
    }

    #[test]
    fn with_length_9_does_not_clamp_expanded_weights() {
        // 15 weighted entries: repeats 0, extras 9; the center and four
        // outer pairs are picked, so the result is still exactly nine.
        let long = [(0, 5), (1, 5), (2, 5)];
        let result = with_length_9(&long);
        assert_eq!(result.len(), 9);
        assert_eq!(result.iter().filter(|color| **color == 0).count(), 4);
        assert_eq!(result.iter().filter(|color| **color == 1).count(), 1);
        assert_eq!(result.iter().filter(|color| **color == 2).count(), 4);
        assert!(with_length_9(&[]).is_empty());
    }

    #[test]
    fn custom_presets_validate_hex_and_reserved_name() {
        let colors: Vec<String> = ["#F00", "00FF00"]
            .iter()
            .map(|text| (*text).to_owned())
            .collect();
        assert_eq!(
            parse_custom(&colors),
            Ok(vec![0x00ff_0000, 0x0000_ff00])
        );
        let invalid: Vec<String> = vec!["blue".to_owned()];
        assert_eq!(
            parse_custom(&invalid),
            Err("invalid hex color: blue".to_owned())
        );
        assert!(parse_custom(&[]).is_err());
        assert!(validate_custom_name("myflag").is_ok());
        assert_eq!(
            validate_custom_name("random"),
            Err("reserved preset name: random".to_owned())
        );
    }

    #[test]
    fn sgr_formats_rgb_and_ansi256() {
        let red = parse_hex("#E50000").unwrap_or(0);
        assert_eq!(sgr(red, AnsiMode::Rgb), "38;2;229;0;0");
        let pure = parse_hex("#FF0000").unwrap_or(0);
        assert_eq!(sgr(pure, AnsiMode::Ansi256), "38;5;196");
        assert_eq!(ansi256_index(parse_hex("#000000").unwrap_or(0)), 16);
    }

    #[test]
    fn unknown_preset_is_an_error() {
        let config = Config {
            presets: split_preset_names("no-such-flag"),
            ..Config::default()
        };
        assert_eq!(
            config.resolve(),
            Err("unknown hyfetch preset: no-such-flag".to_owned())
        );
    }

    #[test]
    fn lightness_none_keeps_the_vendored_bytes() {
        let config = Config {
            presets: split_preset_names("transgender"),
            ..Config::default()
        };
        assert_eq!(
            config.resolve(),
            Ok(vec![
                "38;2;85;205;253".to_owned(),
                "38;2;85;205;253".to_owned(),
                "38;2;246;170;183".to_owned(),
                "38;2;246;170;183".to_owned(),
                "38;2;255;255;255".to_owned(),
                "38;2;246;170;183".to_owned(),
                "38;2;246;170;183".to_owned(),
                "38;2;85;205;253".to_owned(),
                "38;2;85;205;253".to_owned(),
            ])
        );
    }

    #[test]
    fn lightness_clamp_min_only_raises_dark_colors() {
        let dark = 0x0000_0040;
        let bright = 0x00ff_ffff;
        let lifted = clamp_min_lightness(dark, 0.5);
        assert_ne!(lifted, dark);
        assert!(lifted > dark);
        assert_eq!(clamp_min_lightness(bright, 0.5), bright);
    }

    #[test]
    fn custom_names_shadow_vendored_and_random_picks_one() {
        let config = Config {
            presets: split_preset_names("customflag"),
            custom: vec![("customflag".to_owned(), vec![0x00ff_0000])],
            ..Config::default()
        };
        assert_eq!(
            config.resolve(),
            Ok(vec!["38;2;255;0;0".to_owned(); 9])
        );
        let config = Config {
            presets: split_preset_names("random"),
            ..Config::default()
        };
        assert!(config.resolve().is_ok_and(|palette| palette.len() == 9));
    }

    #[test]
    fn merge_applies_cli_fields_last_and_requires_a_preset() {
        let base = Config {
            presets: split_preset_names("transgender"),
            mode: AnsiMode::Ansi256,
            lightness: Some(0.4),
            ..Config::default()
        };
        assert!(merge(None, None, None, None).is_none());
        let merged = merge(
            Some(base),
            Some("nonbinary,rainbow"),
            Some(AnsiMode::Rgb),
            Some(0.75),
        );
        let Some(merged) = merged else {
            panic!("merge should enable the feature");
        };
        assert_eq!(merged.presets.len(), 2);
        assert_eq!(merged.mode, AnsiMode::Rgb);
        assert_eq!(merged.lightness, Some(0.75));
    }
}
