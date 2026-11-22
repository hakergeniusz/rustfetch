//! rustfetch-only HyFetch-style pride-flag logo coloring.
//!
//! This is NOT a fastfetch feature: there is no upstream byte-parity target
//! for it, only the guarantee that leaving it unset changes nothing (see
//! `scripts/hyfetchtest.sh`). Raw logo types ignore the palette, as upstream
//! does for `--logo-color-N` too. When enabled, the RGB values and the 8-bit
//! ANSI indices are byte-exact against installed HyFetch 2.1.1.
//!
//! Config surface (`logo.hyfetch`, off by default):
//!
//! ```jsonc
//! "logo": { "hyfetch": {
//!   "preset": "transgender",
//!   "mode": "24bit",
//!   "lightness": 0.65,
//!   "customPresets": { "myflag": ["#FF0000", "#00FF00", "#0000FF"] }
//! } }
//! ```
//!
//! `preset` also accepts an array or a comma-separated string; several
//! names pick one at random (seeded from the clock and pid, no dependency),
//! and `random` picks from vendored ∪ custom presets. Custom names shadow
//! vendored ones. `mode` accepts `24bit`/`rgb`/`truecolor` and
//! `8bit`/`ansi256`/`256color`, case-insensitively.
//!
//! `lightness` is a number in 0..=1 (or `null`/absent) and controls the
//! OKHSL lightness *floor*: after decoding each hex to linear sRGB → OKLab →
//! OKHSL (palette 0.7.6's `toe` + gamut-aware saturation), the OKHSL L is
//! clamped to at least that value and converted back. The default is 0.65,
//! HyFetch's `Config::default_lightness(Dark)`; a light terminal would use
//! 0.4 (`ClampMax`), which rustfetch does not detect, so 0.65 is always the
//! fallback. Explicit `lightness: 0` is a no-op floor and therefore yields
//! the raw vendored hex bytes — use it to opt out of the adjustment. The
//! conversion is a dependency-free f32 port of palette's math (including its
//! `mul_add` transfer functions), so it matches HyFetch's byte output.
//!
//! Attribution: the OKHSL conversion is derived from the `palette` crate
//! (https://github.com/Ogeon/palette, dual-licensed MIT OR Apache-2.0) and
//! the ANSI-256 mapping from `ansi_colours`
//! (https://github.com/mina86/ansi_colours, dual-licensed MIT OR Apache-2.0).
//! Both are reimplemented here without adding dependencies; their algorithms
//! and constant tables are reproduced with credit to their authors.
//!
//! New flags (they are intentionally absent from the verbatim fastfetch
//! help captures), parsed in `cli.rs` and merged per-field after the config:
//!
//! - `--hyfetch-preset <name[,name...]>`
//! - `--hyfetch-mode <24bit|8bit>`
//! - `--hyfetch-lightness <0.0..1.0>`

use std::sync::OnceLock;

use crate::hyfetch_presets;

/// HyFetch's `Config::default_lightness(TerminalTheme::Dark)`: the OKHSL
/// lightness floor applied to every preset when the user does not override it.
/// (A light terminal would use 0.4, but rustfetch never auto-detects the theme.)
pub const DEFAULT_LIGHTNESS: f64 = 0.65;

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
    /// OKHSL ClampMin floor; `None`/absent means [`DEFAULT_LIGHTNESS`],
    /// `Some(0.0)` keeps the vendored RGB bytes verbatim.
    pub lightness: Option<f64>,
    /// User presets, checked before the vendored table by name.
    pub custom: Vec<(String, Vec<u32>)>,
    /// `resolve()` and `resolve_full()` are called with the same `&self` in
    /// main.rs; memoize the chosen name so a multi-name/`random` selection
    /// cannot yield a nine-slot palette from one flag and the full color
    /// list from another.
    picked: OnceLock<String>,
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

/// HyFetch's `ColorProfile::with_length` distribution for `len` colors and
/// `n` output slots: every color gets `n / len` copies plus a share of the
/// remainder — one for the center when it is odd, then one for each
/// outermost pair. Returns the source *indices* in output order, which is
/// what the logo gradient needs to spread a flag over its line count; the
/// returned vector has exactly `n` entries when `len` is non-zero.
pub fn with_length_indices(len: usize, n: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let center = len / 2;
    let repeats = n / len;
    let mut extras = n % len;
    let mut weights = vec![repeats; len];
    if extras % 2 == 1 {
        if let Some(slot) = weights.get_mut(center) {
            *slot = slot.saturating_add(1);
        }
        extras -= 1;
    }
    for i in 0..(extras / 2) {
        if let Some(slot) = weights.get_mut(i) {
            *slot = slot.saturating_add(1);
        }
        let opposite = len.saturating_sub(1).saturating_sub(i);
        if let Some(slot) = weights.get_mut(opposite) {
            *slot = slot.saturating_add(1);
        }
    }
    let mut out = Vec::with_capacity(n);
    for (index, times) in weights.iter().enumerate() {
        for _ in 0..*times {
            out.push(index);
        }
    }
    out
}

/// Exact port of hyfetch's `ColorProfile::with_length`: weights are expanded
/// first, then the requested `n` slots are filled by [`with_length_indices`].
/// The expanded list is never truncated when it exceeds `n` entries — the
/// port used to ask only for nine slots because the logo had nine palette
/// markers, but the per-line gradient asks for the logo's line count.
pub fn with_length(colors: &[(u32, u32)], n: usize) -> Vec<u32> {
    let mut weighted = Vec::new();
    for (color, weight) in colors {
        for _ in 0..*weight {
            weighted.push(*color);
        }
    }
    let indices = with_length_indices(weighted.len(), n);
    indices
        .into_iter()
        .filter_map(|index| weighted.get(index).copied())
        .collect()
}

/// The original nine-slot palette form of [`with_length`], kept for the
/// fastfetch-compatible `$1`-`$9` path.
pub fn with_length_9(colors: &[(u32, u32)]) -> Vec<u32> {
    with_length(colors, 9)
}

// ── Faithful f32 port of palette 0.7.6's Okhsl ClampMin ──────────────────
// HyFetch's `ColorProfile::with_lightness_adaptive` converts LinSrgb →
// Oklab → Okhsl with palette's `into_color_mut()`, clamps `okhsl.lightness`
// and converts back. The port below mirrors palette's own `f32` arithmetic,
// including `mul_add` in the transfer functions and the degrees round-trip
// of `OklabHue`, so the final `Srgb<u8>` bytes are identical to HyFetch's.

/// `f64` literal → `f32`, matching palette's `T::from_f64(constant)` calls.
#[inline]
fn c(value: f64) -> f32 {
    value as f32
}

/// `ok_utils::toe`: OKLab L → sRGB-white-referenced lightness.
fn toe(l: f32) -> f32 {
    let k_1 = c(0.206);
    let k_2 = c(0.03);
    let k_3 = (1.0 + k_1) / (1.0 + k_2);
    0.5 * (k_3 * l - k_1 + ((k_3 * l - k_1).powi(2) + c(4.0) * k_2 * k_3 * l).sqrt())
}

/// `ok_utils::toe_inv`: the inverse of [`toe`].
fn toe_inv(l_r: f32) -> f32 {
    let k_1 = c(0.206);
    let k_2 = c(0.03);
    let k_3 = (1.0 + k_1) / (1.0 + k_2);
    (l_r.powi(2) + k_1 * l_r) / (k_3 * (l_r + k_2))
}

/// `Oklab` → linear sRGB (Ottosson's matrices), palette's
/// `linear_srgb_to_oklab` inverse used by `oklab_to_linear_srgb`.
fn oklab_to_rgb(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let l_ = l + c(0.3963377774) * a + c(0.2158037573) * b;
    let m_ = l - c(0.1055613458) * a - c(0.0638541728) * b;
    let s_ = l - c(0.0894841775) * a - c(1.2914855480) * b;
    let (l_, m_, s_) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    (
        c(4.0767416621) * l_ - c(3.3077115913) * m_ + c(0.2309699292) * s_,
        c(-1.2684380046) * l_ + c(2.6097574011) * m_ - c(0.3413193965) * s_,
        c(-0.0041960863) * l_ - c(0.7034186147) * m_ + c(1.7076147010) * s_,
    )
}

/// Linear sRGB → Oklab, palette's `linear_srgb_to_oklab`.
fn rgb_to_oklab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let l = c(0.4122214708) * r + c(0.5363325363) * g + c(0.0514459929) * b;
    let m = c(0.2119034982) * r + c(0.6806995451) * g + c(0.1073969566) * b;
    let s = c(0.0883024619) * r + c(0.2817188376) * g + c(0.6299787005) * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    (
        c(0.2104542553) * l + c(0.7936177850) * m - c(0.0040720468) * s,
        c(1.9779984951) * l - c(2.4285922050) * m + c(0.4505937099) * s,
        c(0.0259040371) * l + c(0.7827717662) * m - c(0.8086757660) * s,
    )
}

/// `ok_utils::LC`: a lightness/chroma point.
#[derive(Clone, Copy)]
struct Lc {
    lightness: f32,
    chroma: f32,
}

/// `LC::max_saturation` — one Halley iteration, with palette's branch order.
fn max_saturation(a: f32, b: f32) -> f32 {
    let (k_0, k_1, k_2, k_3, k_4, w_l, w_m, w_s) = if c(-1.88170328) * a - c(0.80936493) * b > 1.0 {
        (
            c(1.19086277), c(1.76576728), c(0.59662641), c(0.75515197), c(0.56771245),
            c(4.0767416621), c(-3.3077115913), c(0.2309699292),
        )
    } else if c(1.81444104) * a - c(1.19445276) * b > 1.0 {
        (
            c(0.73956515), c(-0.45954404), c(0.08285427), c(0.12541070), c(0.14503204),
            c(-1.2684380046), c(2.6097574011), c(-0.3413193965),
        )
    } else {
        (
            c(1.35733652), c(-0.00915799), c(-1.15130210), c(-0.50559606), c(0.00692167),
            c(-0.0041960863), c(-0.7034186147), c(1.7076147010),
        )
    };
    let saturation = k_0 + k_1 * a + k_2 * b + k_3 * a.powi(2) + k_4 * a * b;
    let k_l = c(0.3963377774) * a + c(0.2158037573) * b;
    let k_m = c(-0.1055613458) * a - c(0.0638541728) * b;
    let k_s = c(-0.0894841775) * a - c(1.2914855480) * b;
    // MAX_SRGB_SATURATION_SEARCH_MAX_ITER == 1
    let l_ = 1.0 + saturation * k_l;
    let m_ = 1.0 + saturation * k_m;
    let s_ = 1.0 + saturation * k_s;
    let l = l_.powi(3);
    let m = m_.powi(3);
    let s = s_.powi(3);
    let l_ds = c(3.0) * k_l * l_.powi(2);
    let m_ds = c(3.0) * k_m * m_.powi(2);
    let s_ds = c(3.0) * k_s * s_.powi(2);
    let l_ds2 = c(6.0) * k_l.powi(2) * l_;
    let m_ds2 = c(6.0) * k_m.powi(2) * m_;
    let s_ds2 = c(6.0) * k_s.powi(2) * s_;
    let f = w_l * l + w_m * m + w_s * s;
    let f1 = w_l * l_ds + w_m * m_ds + w_s * s_ds;
    let f2 = w_l * l_ds2 + w_m * m_ds2 + w_s * s_ds2;
    saturation - f * f1 / (f1.powi(2) - c(0.5) * f * f2)
}

/// `LC::find_cusp` — cusp of the sRGB gamut for a normalized hue.
fn find_cusp(a: f32, b: f32) -> Lc {
    let max_saturation = max_saturation(a, b);
    let rgb = oklab_to_rgb(1.0, max_saturation * a, max_saturation * b);
    let max_lightness = (1.0 / rgb.0.max(rgb.1).max(rgb.2)).cbrt();
    Lc { lightness: max_lightness, chroma: max_lightness * max_saturation }
}

/// `ok_utils::find_gamut_intersection` — one Halley step after the triangle
/// intersection, with the `flt_max = 10e5` guards.
fn find_gamut_intersection(a: f32, b: f32, l1: f32, c1: f32, l0: f32, cusp: Lc) -> f32 {
    if (l1 - l0) * cusp.chroma - (cusp.lightness - l0) * c1 <= 0.0 {
        cusp.chroma * l0 / (c1 * cusp.lightness + cusp.chroma * (l0 - l1))
    } else {
        let t = cusp.chroma * (l0 - 1.0)
            / (c1 * (cusp.lightness - 1.0) + cusp.chroma * (l0 - l1));
        let dl = l1 - l0;
        let dc = c1;
        let k_l = c(0.3963377774) * a + c(0.2158037573) * b;
        let k_m = c(-0.1055613458) * a - c(0.0638541728) * b;
        let k_s = c(-0.0894841775) * a - c(1.2914855480) * b;
        let l_dt = dl + dc * k_l;
        let m_dt = dl + dc * k_m;
        let s_dt = dl + dc * k_s;
        let lightness = l0 * (1.0 - t) + t * l1;
        let chroma = t * c1;
        let l_ = lightness + chroma * k_l;
        let m_ = lightness + chroma * k_m;
        let s_ = lightness + chroma * k_s;
        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;
        let ldt = c(3.0) * l_dt * l_ * l_;
        let mdt = c(3.0) * m_dt * m_ * m_;
        let sdt = c(3.0) * s_dt * s_ * s_;
        let ldt2 = c(6.0) * l_dt * l_dt * l_;
        let mdt2 = c(6.0) * m_dt * m_dt * m_;
        let sdt2 = c(6.0) * s_dt * s_dt * s_;
        let r = c(4.0767416621) * l - c(3.3077115913) * m + c(0.2309699292) * s - 1.0;
        let r1 = c(4.0767416621) * ldt - c(3.3077115913) * mdt + c(0.2309699292) * sdt;
        let r2 = c(4.0767416621) * ldt2 - c(3.3077115913) * mdt2 + c(0.2309699292) * sdt2;
        let u_r = r1 / (r1 * r1 - c(0.5) * r * r2);
        let mut t_r = -r * u_r;
        let g = c(-1.2684380046) * l + c(2.6097574011) * m - c(0.3413193965) * s - 1.0;
        let g1 = c(-1.2684380046) * ldt + c(2.6097574011) * mdt - c(0.3413193965) * sdt;
        let g2 = c(-1.2684380046) * ldt2 + c(2.6097574011) * mdt2 - c(0.3413193965) * sdt2;
        let u_g = g1 / (g1 * g1 - c(0.5) * g * g2);
        let mut t_g = -g * u_g;
        let b = c(-0.0041960863) * l - c(0.7034186147) * m + c(1.7076147010) * s - 1.0;
        let b1 = c(-0.0041960863) * ldt - c(0.7034186147) * mdt + c(1.7076147010) * sdt;
        let b2 = c(-0.0041960863) * ldt2 - c(0.7034186147) * mdt2 + c(1.7076147010) * sdt2;
        let u_b = b1 / (b1 * b1 - c(0.5) * b * b2);
        let mut t_b = -b * u_b;
        let flt_max = c(10e5);
        if u_r < 0.0 { t_r = flt_max; }
        if u_g < 0.0 { t_g = flt_max; }
        if u_b < 0.0 { t_b = flt_max; }
        t + t_r.min(t_g.min(t_b))
    }
}

/// `ok_utils::ST::mid` — smooth approximation of the cusp position.
fn st_mid(a: f32, b: f32) -> (f32, f32) {
    let s = c(0.11516993) + 1.0 / (
        c(7.44778970) + c(4.15901240) * b
            + a * (c(-2.19557347) + c(1.75198401) * b
                + a * (c(-2.13704948) - c(10.02301043) * b
                    + a * (c(-4.24894561) + c(5.38770819) * b + c(4.69891013) * a)))
    );
    let t = c(0.11239642) + 1.0 / (
        c(1.61320320) - c(0.68124379) * b
            + a * (c(0.40370612) + c(0.90148123) * b
                + a * (c(-0.27087943) + c(0.61223990) * b
                    + a * (c(0.00299215) - c(0.45399568) * b - c(0.14661872) * a)))
    );
    (s, t)
}

/// `ok_utils::ChromaValues` — the three interpolated chroma bounds.
#[derive(Clone, Copy)]
struct ChromaValues {
    zero: f32,
    mid: f32,
    max: f32,
}

/// `ChromaValues::from_normalized` (`get_Cs` in the reference implementation).
fn chroma_values(lightness: f32, a: f32, b: f32) -> ChromaValues {
    let cusp = find_cusp(a, b);
    let max_chroma = find_gamut_intersection(a, b, lightness, 1.0, lightness, cusp);
    let s = cusp.chroma / cusp.lightness;
    let t = cusp.chroma / (1.0 - cusp.lightness);
    let k = max_chroma / (lightness * s).min((1.0 - lightness) * t);
    let (s_mid, t_mid) = st_mid(a, b);
    let c_a = lightness * s_mid;
    let c_b = (1.0 - lightness) * t_mid;
    let c_mid = c(0.9) * k
        * (1.0 / (1.0 / (c_a * c_a * c_a * c_a) + 1.0 / (c_b * c_b * c_b * c_b)))
            .sqrt()
            .sqrt();
    let c_a = lightness * c(0.4);
    let c_b = (1.0 - lightness) * c(0.8);
    let c_zero = (1.0 / (1.0 / (c_a * c_a) + 1.0 / (c_b * c_b))).sqrt();
    ChromaValues { zero: c_zero, mid: c_mid, max: max_chroma }
}

/// `OklabHue::from_cartesian`: palette stores hues as normalized *degrees*
/// (`PI + atan2(-b, -a)`); `into_cartesian` later converts back with `sin_cos`.
fn hue_from_cartesian(a: f32, b: f32) -> f32 {
    (std::f32::consts::PI + (-b).atan2(-a)).to_degrees()
}

/// palette's sRGB decode for `u8`: `mul_add(x, 1/1.055, 0.055/1.055).powf(2.4)`.
fn into_linear(byte: u8) -> f32 {
    let value = byte as f32 / 255.0;
    if value <= c(0.04045) {
        c(1.0 / 12.92) * value
    } else {
        value.mul_add(c(1.0 / 1.055), c(0.055 / 1.055)).powf(c(2.4))
    }
}

/// palette's sRGB encode for `u8` (its `SliceTable` lookup rounds to nearest).
fn from_linear(value: f32) -> u8 {
    let encoded = if value <= c(0.0031308) {
        c(12.92) * value
    } else {
        value.powf(c(1.0 / 2.4)).mul_add(c(1.055), c(-0.055))
    };
    (encoded * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Faithful port of HyFetch's OKHSL `ClampMin`: raise the sRGB-referenced
/// lightness to `requested` while preserving the OKHSL hue and saturation.
/// `requested = 0.0` is a no-op and therefore preserves the input bytes.
pub fn clamp_min_lightness(color: u32, requested: f64) -> u32 {
    let r = into_linear((color >> 16 & 0xff) as u8);
    let g = into_linear((color >> 8 & 0xff) as u8);
    let b = into_linear((color & 0xff) as u8);
    let (l, a, b) = rgb_to_oklab(r, g, b);

    // Oklab → Okhsl (`FromColorUnclamped<Oklab> for Okhsl`).
    let lr = toe(l);
    let chroma = a.hypot(b);
    let (hue, saturation) = if chroma == 0.0 || l == 1.0 || l == 0.0 {
        (0.0, 0.0)
    } else {
        let hue = hue_from_cartesian(a, b);
        let cs = chroma_values(l, a / chroma, b / chroma);
        let mid = c(0.8);
        let mid_inv = c(1.25);
        let saturation = if chroma < cs.mid {
            let k_1 = mid * cs.zero;
            let k_2 = 1.0 - k_1 / cs.mid;
            let t = chroma / (k_1 + k_2 * chroma);
            t * mid
        } else {
            let k_0 = cs.mid;
            let k_1 = (1.0 - mid) * (cs.mid * mid_inv).powi(2) / cs.zero;
            let k_2 = 1.0 - k_1 / (cs.max - cs.mid);
            let t = (chroma - k_0) / (k_1 + k_2 * (chroma - k_0));
            mid + (1.0 - mid) * t
        };
        (hue, saturation)
    };
    // `into_color_mut()` in HyFetch uses palette's *clamped* FromColor path:
    // Okhsl's saturation and lightness are constrained to [0, 1]. Skipping
    // this changes saturated hues such as #6F0071 by two bytes.
    let saturation = saturation.clamp(0.0, 1.0);
    let lr = lr.clamp(0.0, 1.0);

    // Clamp the Okhsl lightness (palette's `clamp_min_assign`).
    let clamped = lr.max(c(requested));

    // Okhsl → Oklab (`FromColorUnclamped<Okhsl> for Oklab`).
    let (l, a, b) = if clamped == 1.0 {
        (1.0, 0.0, 0.0)
    } else if clamped == 0.0 {
        (0.0, 0.0, 0.0)
    } else {
        // `hue` is already in degrees; `hues.rs::into_cartesian` uses the
        // raw degrees → radians conversion and returns (cos, sin).
        let (sin_hue, cos_hue) = hue.to_radians().sin_cos();
        let a_ = cos_hue;
        let b_ = sin_hue;
        let okl = toe_inv(clamped);
        let cs = chroma_values(okl, a_, b_);
        let mid = c(0.8);
        let mid_inv = c(1.25);
        let chroma = if saturation < mid {
            let t = mid_inv * saturation;
            let k_1 = mid * cs.zero;
            let k_2 = 1.0 - k_1 / cs.mid;
            t * k_1 / (1.0 - k_2 * t)
        } else {
            let t = (saturation - mid) / (1.0 - mid);
            let k_0 = cs.mid;
            let k_1 = (1.0 - mid) * cs.mid * cs.mid * mid_inv * mid_inv / cs.zero;
            let k_2 = 1.0 - k_1 / (cs.max - cs.mid);
            k_0 + t * k_1 / (1.0 - k_2 * t)
        };
        (okl, chroma * a_, chroma * b_)
    };

    let (r, g, b) = oklab_to_rgb(l, a, b);
    (from_linear(r) as u32) << 16 | (from_linear(g) as u32) << 8 | from_linear(b) as u32
}

// ── Faithful port of ansi_colours' rgb → ANSI-256 conversion ─────────────
// HyFetch's `Srgb<u8>::to_ansi_string` calls `ansi_colours::AsRGB::to_ansi256`.
// That crate uses a weighted distance metric and a precomputed grey ramp, not
// a plain nearest-cube search (a plain search maps #699aff to 69; HyFetch
// maps it to 75). Both tables and the arithmetic below are ported verbatim.

/// The 256-entry xterm palette: 16 system colors, a 6×6×6 cube, 24 grays.
const ANSI_COLORS: [u32; 256] = ansi_colors();

const fn ansi_colors() -> [u32; 256] {
    let mut out = [0u32; 256];
    let system = [
        0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5,
        0x7f7f7f, 0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
    ];
    let mut i = 0;
    while i < 16 {
        out[i] = system[i];
        i += 1;
    }
    let cube = [0u32, 95, 135, 175, 215, 255];
    while i < 232 {
        let j = i - 16;
        out[i] = cube[j / 36] << 16 | cube[j / 6 % 6] << 8 | cube[j % 6];
        i += 1;
    }
    while i < 256 {
        let gray = ((i - 232) * 10 + 8) as u32;
        out[i] = gray << 16 | gray << 8 | gray;
        i += 1;
    }
    out
}

/// ansi_colours' `ANSI256_FROM_GREY`: best ΔE*₀₀ grey for each luminance byte.
const ANSI256_FROM_GREY: [u8; 256] = [
    16, 16, 16, 16, 16, 232, 232, 232, 232, 232, 232, 232, 232, 232, 233, 233,
    233, 233, 233, 233, 233, 233, 233, 233, 234, 234, 234, 234, 234, 234, 234, 234,
    234, 234, 235, 235, 235, 235, 235, 235, 235, 235, 235, 235, 236, 236, 236, 236,
    236, 236, 236, 236, 236, 236, 237, 237, 237, 237, 237, 237, 237, 237, 237, 237,
    238, 238, 238, 238, 238, 238, 238, 238, 238, 238, 239, 239, 239, 239, 239, 239,
    239, 239, 239, 239, 240, 240, 240, 240, 240, 240, 240, 240, 59, 59, 59, 59,
    59, 241, 241, 241, 241, 241, 241, 241, 242, 242, 242, 242, 242, 242, 242, 242,
    242, 242, 243, 243, 243, 243, 243, 243, 243, 243, 243, 244, 244, 244, 244, 244,
    244, 244, 244, 244, 102, 102, 102, 102, 102, 245, 245, 245, 245, 245, 245, 246,
    246, 246, 246, 246, 246, 246, 246, 246, 246, 247, 247, 247, 247, 247, 247, 247,
    247, 247, 247, 248, 248, 248, 248, 248, 248, 248, 248, 248, 145, 145, 145, 145,
    145, 249, 249, 249, 249, 249, 249, 250, 250, 250, 250, 250, 250, 250, 250, 250,
    250, 251, 251, 251, 251, 251, 251, 251, 251, 251, 251, 252, 252, 252, 252, 252,
    252, 252, 252, 252, 188, 188, 188, 188, 188, 253, 253, 253, 253, 253, 253, 254,
    254, 254, 254, 254, 254, 254, 254, 254, 254, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 231, 231, 231, 231, 231, 231, 231, 231, 231,
];

/// ansi_colours' weighted (compuphase) distance between an RGB triple and a
/// packed ANSI palette entry.
fn ansi_distance(rgb: (u8, u8, u8), entry: u32) -> u32 {
    let y = ((entry >> 16 & 0xff) as i32, (entry >> 8 & 0xff) as i32, (entry & 0xff) as i32);
    let r_sum = rgb.0 as i32 + y.0;
    let r = rgb.0 as i32 - y.0;
    let g = rgb.1 as i32 - y.1;
    let b = rgb.2 as i32 - y.2;
    ((1024 + r_sum) * r * r + 2048 * g * g + (1534 - r_sum) * b * b) as u32
}

/// ansi_colours' cheap luminance (the Y row of the sRGB → XYZ matrix scaled
/// to 2^24), rounded to nearest.
fn ansi_luminance(r: u8, g: u8, b: u8) -> usize {
    let value = 3_567_664u32 * r as u32
        + 11_998_547u32 * g as u32
        + 1_211_005u32 * b as u32;
    ((value + (1u32 << 23)) >> 24) as usize
}

/// ansi_colours' per-axis 6×6×6 cube thresholds.
fn cube_threshold(value: u8, a: u8, b: u8, c: u8, d: u8, e: u8) -> (u8, u32) {
    if value < a { (0, 0) }
    else if value < b { (1, 95) }
    else if value < c { (2, 135) }
    else if value < d { (3, 175) }
    else if value < e { (4, 215) }
    else { (5, 255) }
}

/// The best cube entry for an RGB triple, as `(index, packed_rgb)`.
fn ansi_cube(rgb: (u8, u8, u8)) -> (u8, u32) {
    let (ri, r) = cube_threshold(rgb.0, 38, 115, 155, 196, 235);
    let (gi, g) = cube_threshold(rgb.1, 36, 116, 154, 195, 235);
    let (bi, b) = cube_threshold(rgb.2, 35, 115, 155, 195, 235);
    (ri * 36 + gi * 6 + bi + 16, r << 16 | g << 8 | b)
}

/// HyFetch's ANSI-256 index for a packed RGB color (the `ansi_colours`
/// algorithm, byte-identical to `hyfetch -m 8bit`).
pub fn ansi256_index(color: u32) -> u8 {
    let rgb = ((color >> 16 & 0xff) as u8, (color >> 8 & 0xff) as u8, (color & 0xff) as u8);
    let gray_index = ANSI256_FROM_GREY[ansi_luminance(rgb.0, rgb.1, rgb.2)];
    let gray_distance = ansi_distance(rgb, ANSI_COLORS[gray_index as usize]);
    let (cube_index, cube_rgb) = ansi_cube(rgb);
    if ansi_distance(rgb, cube_rgb) < gray_distance {
        cube_index
    } else {
        gray_index
    }
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
        if let Some(name) = self.picked.get() {
            return Ok(name.clone());
        }
        let name = if self.presets.iter().any(|name| name == "random") {
            let names = self.available_names();
            names
                .get(random_index(names.len()))
                .cloned()
                .ok_or_else(|| "unknown hyfetch preset: random".to_owned())?
        } else {
            self.presets
                .get(random_index(self.presets.len()))
                .cloned()
                .ok_or_else(|| "no hyfetch preset selected".to_owned())?
        };
        let _ = self.picked.set(name);
        self.picked
            .get()
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
        // Absent/null lightness means HyFetch's dark-terminal default; an
        // explicit 0 is a no-op floor that keeps the raw vendored hex bytes.
        let requested = self.lightness.unwrap_or(DEFAULT_LIGHTNESS);
        let colors: Vec<_> = colors
            .into_iter()
            .map(|(color, weight)| (clamp_min_lightness(color, requested), weight))
            .collect();
        let palette = with_length_9(&colors);
        Ok(palette
            .into_iter()
            .map(|color| sgr(color, self.mode))
            .collect())
    }

    /// Every color of the selected preset after the lightness floor, in
    /// upstream order — unlike [`Config::resolve`], not folded into nine
    /// palette slots. Presets such as `progress` have eleven colors, so the
    /// logo gradient needs one raw SGR escape per color instead of `$N`.
    /// Weights are expanded (as in `with_length`), so upstream repetitions
    /// such as transgender's five entries are preserved.
    pub fn resolve_full(&self) -> Result<Vec<String>, String> {
        // `pick_name` memoizes the choice, so this is the very same preset
        // `resolve()` used even when the config named several of them.
        let name = self.pick_name()?;
        let colors = self
            .preset_colors(&name)
            .ok_or_else(|| format!("unknown hyfetch preset: {name}"))?;
        let requested = self.lightness.unwrap_or(DEFAULT_LIGHTNESS);
        let mut out = Vec::new();
        for (color, weight) in colors {
            let adjusted = clamp_min_lightness(color, requested);
            let escape = sgr(adjusted, self.mode);
            for _ in 0..weight {
                out.push(escape.clone());
            }
        }
        Ok(out)
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
    fn with_length_indices_spreads_each_flag_once_over_n_lines() {
        // 19-line Arch logo, six rainbow colors: 3,3,3,4,3,3.
        assert_eq!(
            with_length_indices(6, 19),
            vec![0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 5, 5, 5]
        );
        // Five transgender entries: 4,4,3,4,4 (center gets the odd extra).
        assert_eq!(
            with_length_indices(5, 19),
            vec![0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4]
        );
        // Eleven progress colors: 2,2,2,2,1,1,1,2,2,2,2 (four outer pairs).
        let progress = with_length_indices(11, 19);
        assert_eq!(progress.len(), 19);
        assert_eq!(
            progress,
            vec![0, 0, 1, 1, 2, 2, 3, 3, 4, 5, 6, 7, 7, 8, 8, 9, 9, 10, 10]
        );
        assert!(with_length_indices(0, 19).is_empty());
        // Fewer lines than colors spread from the outside in, like hyfetch.
        assert_eq!(with_length_indices(11, 4), vec![0, 1, 9, 10]);
    }

    #[test]
    fn resolve_full_keeps_every_preset_color_in_upstream_order() {
        let progress = Config {
            presets: split_preset_names("progress"),
            ..Config::default()
        };
        let full = progress.resolve_full();
        let Ok(full) = full else { panic!("progress resolves") };
        assert_eq!(full.len(), 11);
        assert_eq!(
            full,
            vec![
                "38;2;255;255;255", "38;2;255;175;200", "38;2;116;215;238",
                "38;2;214;138;73", "38;2;158;158;158", "38;2;255;100;83",
                "38;2;255;141;0", "38;2;255;238;0", "38;2;4;191;53",
                "38;2;105;154;255", "38;2;229;77;255",
            ]
        );
        // The nine-slot palette still drops gray and orange.
        let Ok(palette) = progress.resolve() else { panic!("progress palette") };
        assert_eq!(palette.len(), 9);
        assert!(!palette.iter().any(|slot| slot == "38;2;158;158;158"));
        assert!(!palette.iter().any(|slot| slot == "38;2;255;141;0"));

        // Duplicate flag colors keep their upstream multiplicity.
        let transgender = Config {
            presets: split_preset_names("transgender"),
            ..Config::default()
        };
        assert_eq!(
            transgender.resolve_full(),
            Ok(vec![
                "38;2;85;205;253".to_owned(),
                "38;2;246;170;183".to_owned(),
                "38;2;255;255;255".to_owned(),
                "38;2;246;170;183".to_owned(),
                "38;2;85;205;253".to_owned(),
            ])
        );
    }

    #[test]
    fn resolve_and_resolve_full_share_one_random_pick() {
        let config = Config {
            presets: split_preset_names("rainbow,transgender"),
            ..Config::default()
        };
        let Ok(palette) = config.resolve() else { panic!("palette") };
        let Ok(full) = config.resolve_full() else { panic!("full") };
        assert_eq!(palette.len(), 9);
        assert!(full.len() == 5 || full.len() == 6);
        // Every full color must be present in the nine-slot palette of the
        // same flag (or be one of the colors folded away by with_length_9).
        let Some(first) = full.first() else { panic!("at least one color") };
        assert!(palette.iter().any(|slot| slot == first));
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
        // Oracle: hyfetch -m 8bit maps these post-lightness colors to 75/203.
        assert_eq!(ansi256_index(0x0069_9aff), 75);
        assert_eq!(ansi256_index(0x00ff_6453), 203);
    }

    /// Every index below comes from `hyfetch -p <raw hexes> -m 8bit` (the
    /// adjusted RGB triples are those asserted in the color test above).
    #[test]
    fn ansi256_matches_hyfetch_8bit_codes() {
        let cases: &[(u32, u8)] = &[
            // rainbow
            (0x00ff_6453, 203), (0x00ff_8d00, 208), (0x00ff_ee00, 226),
            (0x0004_bf35, 35), (0x0069_9aff, 75), (0x00e5_4dff, 171),
            // transgender
            (0x0055_cdfd, 81), (0x00f6_aab7, 217), (0x00ff_ffff, 231),
            // nonbinary
            (0x00fc_f431, 227), (0x00b8_82e7, 140), (0x009e_9e9e, 247),
            // bisexual
            (0x00ff_5798, 204), (0x00ca_7fc4, 176),
            // demisexual
            (0x00fb_14ff, 201), (0x00d3_d3d3, 252),
        ];
        for &(color, expected) in cases {
            assert_eq!(ansi256_index(color), expected, "color #{color:06x}");
        }
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
    fn default_lightness_palette_for_transgender() {
        // Transgender's colors are already above the 0.65 floor, so the
        // default adjustment is a no-op — exactly as hyfetch 2.1.1 prints.
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

    /// Every triple below was extracted from `hyfetch -p <hexes> -m rgb`
    /// (installed oracle 2.1.1); the port must reproduce them byte-for-byte.
    #[test]
    fn okhsl_clamp_min_matches_hyfetch_bytes() {
        let cases: &[(u32, &str)] = &[
            // rainbow
            (0x00e5_0000, "38;2;255;100;83"),
            (0x00ff_8d00, "38;2;255;141;0"),
            (0x00ff_ee00, "38;2;255;238;0"),
            (0x0002_8121, "38;2;4;191;53"),
            (0x0000_4cff, "38;2;105;154;255"),
            (0x0077_0088, "38;2;229;77;255"),
            // transgender
            (0x0055_cdfd, "38;2;85;205;253"),
            (0x00f6_aab7, "38;2;246;170;183"),
            (0x00ff_ffff, "38;2;255;255;255"),
            // demisexual / nonbinary
            (0x0000_0000, "38;2;158;158;158"),
            (0x006f_0071, "38;2;251;20;255"),
            (0x00d3_d3d3, "38;2;211;211;211"),
            (0x00fc_f431, "38;2;252;244;49"),
            (0x009d_59d2, "38;2;184;130;231"),
            (0x0028_2828, "38;2;158;158;158"),
            // bisexual / progress
            (0x00d6_0270, "38;2;255;87;152"),
            (0x009b_4f96, "38;2;202;127;196"),
            (0x0000_38a8, "38;2;103;155;255"),
            (0x00ff_afc8, "38;2;255;175;200"),
            (0x0074_d7ee, "38;2;116;215;238"),
            (0x0061_3915, "38;2;214;138;73"),
            // agender / pansexual / queer
            (0x00ba_baba, "38;2;186;186;186"),
            (0x00ba_f484, "38;2;186;244;132"),
            (0x00ff_1c8d, "38;2;255;86;155"),
            (0x00b5_7fdd, "38;2;184;132;223"),
            (0x0049_821e, "38;2;103;182;42"),
        ];
        for &(color, expected) in cases {
            let actual = clamp_min_lightness(color, DEFAULT_LIGHTNESS);
            let r = actual >> 16 & 0xff;
            let g = actual >> 8 & 0xff;
            let b = actual & 0xff;
            assert_eq!(format!("38;2;{r};{g};{b}"), expected, "input #{color:06x}");
        }
    }

    #[test]
    fn default_lightness_applies_and_zero_keeps_raw_hex() {
        let expected = vec![
            "38;2;255;100;83".to_owned(),
            "38;2;255;100;83".to_owned(),
            "38;2;255;141;0".to_owned(),
            "38;2;255;238;0".to_owned(),
            "38;2;4;191;53".to_owned(),
            "38;2;4;191;53".to_owned(),
            "38;2;105;154;255".to_owned(),
            "38;2;229;77;255".to_owned(),
            "38;2;229;77;255".to_owned(),
        ];
        let config = Config {
            presets: split_preset_names("rainbow"),
            ..Config::default()
        };
        assert_eq!(config.resolve(), Ok(expected.clone()));
        let raw = Config {
            presets: split_preset_names("rainbow"),
            lightness: Some(0.0),
            ..Config::default()
        };
        assert_eq!(raw.resolve(), Ok(vec![
            "38;2;229;0;0".to_owned(),
            "38;2;229;0;0".to_owned(),
            "38;2;255;141;0".to_owned(),
            "38;2;255;238;0".to_owned(),
            "38;2;2;129;33".to_owned(),
            "38;2;2;129;33".to_owned(),
            "38;2;0;76;255".to_owned(),
            "38;2;119;0;136".to_owned(),
            "38;2;119;0;136".to_owned(),
        ]));
    }

    #[test]
    fn lightness_clamp_min_only_raises_dark_colors() {
        let dark = 0x0000_0040;
        let bright = 0x00ff_ffff;
        let lifted = clamp_min_lightness(dark, 0.5);
        assert_ne!(lifted, dark);
        assert!(lifted > dark);
        assert_eq!(clamp_min_lightness(bright, 0.5), bright);
        // A zero floor reproduces the vendored hexes exactly (the raw-hex
        // escape hatch); extremely dark non-preset colors can shift by a
        // byte through palette's lossy round-trip, so use a real preset hex.
        assert_eq!(clamp_min_lightness(0x006f_0071, 0.0), 0x006f_0071);
    }

    #[test]
    fn custom_names_shadow_vendored_and_random_picks_one() {
        let config = Config {
            presets: split_preset_names("customflag"),
            custom: vec![("customflag".to_owned(), vec![0x00ff_0000])],
            ..Config::default()
        };
        // Custom presets use the same 0.65 floor: #FF0000 → (255,100,83),
        // confirmed against `hyfetch -p '#FF0000' -m rgb`.
        assert_eq!(
            config.resolve(),
            Ok(vec!["38;2;255;100;83".to_owned(); 9])
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
