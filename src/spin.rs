//! The spin logo: the builtin logo (or a text file in logo syntax) drawn
//! as an extruded 3D slab. It has no timer and reads no input of its own;
//! the `--watch` loop calls [`SpinState::tick`] once per refresh, which is
//! what makes it turn. Port of src/logo/spin/render.c + spin.c.

use crate::instance::Instance;
use crate::logo::PositionOpt;

// Palette slots 1..9 hold the logo's $1..$9 colors; raw SGR sequences found
// in custom logo files get the slots after that. Slot 0 is the terminal
// default.
const FF_SPIN_PALETTE_MAX: usize = 32;
const FF_SPIN_PALETTE_LOGO_SLOTS: usize = 10;

// Terminal cells are about twice as tall as wide. World units are cell
// heights, so one column is half a unit wide.
const FF_SPIN_CELL_ASPECT: f32 = 0.5;
// Camera distance in bounding-sphere radii. Smaller means stronger
// perspective.
const FF_SPIN_CAMERA_RADII: f32 = 4.0;

// Shading for the slab's edges, dark to bright. The faces keep the logo's
// glyphs.
const FF_SPIN_WALL_RAMP: [u32; 4] = [0x2591, 0x2592, 0x2593, 0x2588]; // ░▒▓█
const FF_SPIN_LIGHT: [f32; 3] = [-0.48, 0.6, 0.64]; // unit length, camera space

// Degrees per frame when neither speed option is set.
const SPIN_DEFAULT_DEGREES_PER_FRAME: f32 = 10.0;

// M_PI as float: C computes `(float) M_PI` from the double literal, which
// rounds to the same nearest-f32 value as the constant.
const PI_F32: f32 = std::f32::consts::PI;

// Rust's f32::sin/cos need not match glibc's sinf/cosf bit-for-bit, and the
// rendered cells change with one ulp — call libm directly.
unsafe extern "C" {
    fn sinf(x: f32) -> f32;
    fn cosf(x: f32) -> f32;
}

/// A logo's ASCII art as a grid of cells. The renderer extrudes it into a
/// slab.
pub(crate) struct SpinArt {
    width: u32,
    height: u32,
    glyphs: Vec<u32>, // Unicode code point per cell, 0 = empty
    colors: Vec<u8>,  // palette index per cell
    palette: Vec<String>, // SGR parameters, e.g. "1;34"
}

pub(crate) struct SpinFrame {
    width: u32,
    height: u32,
    glyphs: Vec<u32>,
    colors: Vec<u8>,
}

/// ffTimeGetTick: CLOCK_MONOTONIC in milliseconds.
pub(crate) fn time_get_tick() -> f64 {
    let mut now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: plain clock read.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
    now.tv_sec as f64 * 1000.0 + now.tv_nsec as f64 / 1000000.0
}

fn utf8_decode(text: &[u8], cursor: &mut usize) -> u32 {
    let s = &text[*cursor..];
    let b0 = s[0];
    if b0 < 0x80 {
        *cursor += 1;
        return b0 as u32;
    }
    let (length, glyph) = if b0 & 0xE0 == 0xC0 {
        (2usize, (b0 & 0x1F) as u32)
    } else if b0 & 0xF0 == 0xE0 {
        (3, (b0 & 0x0F) as u32)
    } else if b0 & 0xF8 == 0xF0 {
        (4, (b0 & 0x07) as u32)
    } else {
        *cursor += 1;
        return b'?' as u32;
    };
    let mut glyph = glyph;
    for i in 1..length {
        // A NUL terminator in the C original reads as a non-continuation
        // byte; past-the-end behaves the same here.
        let byte = s.get(i).copied().unwrap_or(0);
        if byte & 0xC0 != 0x80 {
            *cursor += i;
            return b'?' as u32;
        }
        glyph = (glyph << 6) | (byte & 0x3F) as u32;
    }
    *cursor += length;
    glyph
}

fn glyph_is_blank(glyph: u32) -> bool {
    glyph <= ' ' as u32 || glyph == 0x7F || glyph == 0xA0 || glyph == 0x2800
}

fn palette_add(palette: &mut Vec<String>, params: &[u8]) -> u8 {
    if params.is_empty() || (params.len() == 1 && params[0] == b'0') {
        return 0;
    }
    for (i, entry) in palette.iter().enumerate().skip(FF_SPIN_PALETTE_LOGO_SLOTS) {
        if entry.as_bytes() == params {
            return i as u8;
        }
    }
    if palette.len() >= FF_SPIN_PALETTE_MAX {
        return 0;
    }
    // The scanned params are digits and semicolons only.
    palette.push(String::from_utf8_lossy(params).into_owned());
    (palette.len() - 1) as u8
}

/// Walks fastfetch logo text ($1..$9 colors, `$$`, tabs, SGR escapes). With
/// `fill` false it only measures the grid; with `fill` true, `cells` holds
/// the pre-allocated grid sized by the measure pass.
fn art_walk(
    text: &[u8],
    mut fill: Option<(&mut Vec<u32>, &mut Vec<u8>, u32)>,
    palette: &mut Vec<String>,
) -> (u32, u32) {
    let (mut row, mut col, mut width) = (0u32, 0u32, 0u32);
    let mut color: u8 = 1; // fastfetch starts every logo in $1
    let mut p = 0usize;
    while p < text.len() {
        match text[p] {
            b'\r' => {
                p += 1;
            },
            b'\n' => {
                if col > width {
                    width = col;
                }
                row += 1;
                col = 0;
                p += 1;
            },
            b'\t' => {
                col += 4;
                p += 1;
            },
            0x1b => {
                if p + 1 < text.len() && text[p + 1] == b'[' {
                    let params = p + 2;
                    let mut end = params;
                    while end < text.len() && (text[end].is_ascii_digit() || text[end] == b';') {
                        end += 1;
                    }
                    let at_end = end >= text.len();
                    if !at_end && text[end] == b'm' && fill.is_some() {
                        color = palette_add(palette, &text[params..end]);
                    }
                    // p = *end ? end + 1 : end
                    p = if at_end { end } else { end + 1 };
                } else {
                    p += 1;
                }
            },
            b'$' => {
                if p + 1 < text.len() && (b'1'..=b'9').contains(&text[p + 1]) {
                    color = text[p + 1] - b'0';
                    p += 2;
                    continue;
                }
                if p + 1 < text.len() && text[p + 1] == b'$' {
                    p += 1; // `$$` is a literal dollar sign
                }
                let glyph = utf8_decode(text, &mut p);
                if let Some((glyphs, colors, art_width)) = fill.as_mut()
                    && !glyph_is_blank(glyph)
                {
                    let index = (row * *art_width + col) as usize;
                    glyphs[index] = glyph;
                    colors[index] = color;
                }
                col += 1;
            },
            _ => {
                let glyph = utf8_decode(text, &mut p);
                if let Some((glyphs, colors, art_width)) = fill.as_mut()
                    && !glyph_is_blank(glyph)
                {
                    let index = (row * *art_width + col) as usize;
                    glyphs[index] = glyph;
                    colors[index] = color;
                }
                col += 1;
            },
        }
    }
    if col > width {
        width = col;
    }
    if col > 0 {
        row += 1;
    }
    (width, row)
}

/// Drops blank rows and columns around the art, so it spins around its
/// center.
fn art_crop(art: &mut SpinArt) -> bool {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for y in 0..art.height {
        for x in 0..art.width {
            if art.glyphs[(y * art.width + x) as usize] == 0 {
                continue;
            }
            if x < min_x {
                min_x = x;
            }
            if x > max_x {
                max_x = x;
            }
            if y < min_y {
                min_y = y;
            }
            if y > max_y {
                max_y = y;
            }
        }
    }
    if min_x == u32::MAX {
        return false;
    }
    let width = max_x - min_x + 1;
    let height = max_y - min_y + 1;
    if width == art.width && height == art.height {
        return true;
    }
    let mut glyphs = vec![0u32; (width * height) as usize];
    let mut colors = vec![0u8; (width * height) as usize];
    for y in 0..height {
        let from = ((min_y + y) * art.width + min_x) as usize;
        let to = (y * width) as usize;
        glyphs[to..to + width as usize].copy_from_slice(&art.glyphs[from..from + width as usize]);
        colors[to..to + width as usize].copy_from_slice(&art.colors[from..from + width as usize]);
    }
    art.glyphs = glyphs;
    art.colors = colors;
    art.width = width;
    art.height = height;
    true
}

/// ffSpinArtParse: `colors` holds the logo's $1..$9 colors. Returns None
/// for empty art.
pub(crate) fn spin_art_parse(text: &[u8], colors: &[&str]) -> Option<SpinArt> {
    let mut palette: Vec<String> = Vec::with_capacity(FF_SPIN_PALETTE_MAX);
    palette.push(String::new()); // slot 0: the terminal default
    let color_slots = colors.len().min(FF_SPIN_PALETTE_LOGO_SLOTS - 1);
    for color in &colors[..color_slots] {
        palette.push((*color).to_string());
    }
    palette.resize(FF_SPIN_PALETTE_LOGO_SLOTS, String::new());

    let mut scratch: Vec<String> = Vec::new();
    let (width, height) = art_walk(text, None, &mut scratch);
    if width == 0 || height == 0 {
        return None;
    }
    let mut glyphs = vec![0u32; (width * height) as usize];
    let mut cell_colors = vec![0u8; (width * height) as usize];
    let _ = art_walk(text, Some((&mut glyphs, &mut cell_colors, width)), &mut palette);
    let mut art = SpinArt { width, height, glyphs, colors: cell_colors, palette };
    if !art_crop(&mut art) {
        return None;
    }
    Some(art)
}

struct SpinGeometry {
    hx: f32,
    hy: f32,
    hz: f32, // half extents of the slab
    camera: f32, // camera distance from the slab center
    focal: f32,  // maps the front face 1:1 onto cells at yaw 0
}

fn spin_geometry(art: &SpinArt) -> SpinGeometry {
    let hx = art.width as f32 * FF_SPIN_CELL_ASPECT * 0.5;
    let hy = art.height as f32 * 0.5;
    let hz = 0.3f32.max(hx.max(hy) * 0.055);
    let radius = (hx * hx + hy * hy + hz * hz).sqrt();
    let camera = radius * FF_SPIN_CAMERA_RADII;
    let focal = camera - hz;
    SpinGeometry { hx, hy, hz, camera, focal }
}

/// Rx(tilt) * Ry(yaw): spin around the logo's vertical axis, then lean it.
/// Returns (matrix, inverse) with the inverse stored transposed.
fn rotation_matrix(yaw: f32, tilt: f32) -> ([[f32; 3]; 3], [[f32; 3]; 3]) {
    let (cy, sy) = (unsafe { cosf(yaw) }, unsafe { sinf(yaw) });
    let (cx, sx) = (unsafe { cosf(tilt) }, unsafe { sinf(tilt) });
    let m = [
        [cy, 0.0, sy],
        [sx * sy, cx, -sx * cy],
        [-cx * sy, sx, cx * cy],
    ];
    let mut out = [[0.0f32; 3]; 3];
    let mut inverse = [[0.0f32; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = m[i][j];
            inverse[j][i] = m[i][j];
        }
    }
    (out, inverse)
}

/// ffSpinMirrorGlyph: the glyph mirrored horizontally.
fn mirror_glyph(glyph: u32) -> u32 {
    const PAIRS: [(u32, u32); 23] = [
        (b'/' as u32, b'\\' as u32),
        (b'(' as u32, b')' as u32),
        (b'[' as u32, b']' as u32),
        (b'{' as u32, b'}' as u32),
        (b'<' as u32, b'>' as u32),
        (b'b' as u32, b'd' as u32),
        (b'p' as u32, b'q' as u32),
        (0x258C, 0x2590),
        (0x2598, 0x259D),
        (0x2596, 0x2597),
        (0x259B, 0x259C),
        (0x2599, 0x259F),
        (0x259A, 0x259E),
        (0x25E2, 0x25E3),
        (0x25E4, 0x25E5),
        (0x2571, 0x2572),
        (0x250C, 0x2510),
        (0x2514, 0x2518),
        (0x251C, 0x2524),
        (0x256D, 0x256E),
        (0x2570, 0x256F),
        (0x2554, 0x2557),
        (0x255A, 0x255D),
    ];
    for (left, right) in PAIRS {
        if glyph == left {
            return right;
        }
        if glyph == right {
            return left;
        }
    }
    if (0x2800..=0x28FF).contains(&glyph) {
        // Braille: swap the left dot column (1,2,3,7) with the right one
        // (4,5,6,8).
        let dots = glyph - 0x2800;
        return 0x2800 + (((dots & 0x07) << 3) | ((dots & 0x38) >> 3) | ((dots & 0x40) << 1) | ((dots & 0x80) >> 1));
    }
    glyph
}

/// Frame size that keeps the art inside the frame at every yaw for this
/// tilt. The art is centered; the extra space is the same on both sides.
fn frame_size(art: &SpinArt, tilt: f32) -> (u32, u32) {
    let g = spin_geometry(art);
    let mut max_x = art.width as f32 * 0.5;
    let mut max_y = art.height as f32 * 0.5;
    for step in 0u32..96 {
        let (rot, _) = rotation_matrix(step as f32 * 2.0 * PI_F32 / 96.0, tilt);
        for corner in 0u32..8 {
            let p = [
                if corner & 1 != 0 { g.hx } else { -g.hx },
                if corner & 2 != 0 { g.hy } else { -g.hy },
                if corner & 4 != 0 { g.hz } else { -g.hz },
            ];
            let mut c = [0.0f32; 3];
            for i in 0..3 {
                c[i] = rot[i][0] * p[0] + rot[i][1] * p[1] + rot[i][2] * p[2];
            }
            let scale = g.focal / (g.camera - c[2]);
            max_x = max_x.max(f32::abs(c[0]) * scale / FF_SPIN_CELL_ASPECT);
            max_y = max_y.max(f32::abs(c[1]) * scale);
        }
    }
    // A cell is drawn when the ray through its center hits, and the
    // outermost cell centers sit half a cell inside the frame edge.
    let margin_x = (max_x - art.width as f32 * 0.5 - 0.5).ceil();
    let margin_y = (max_y - art.height as f32 * 0.5 - 0.5).ceil();
    (
        art.width + 2 * u32::from(margin_x > 0.0) * margin_x as u32,
        art.height + 2 * u32::from(margin_y > 0.0) * margin_y as u32,
    )
}

fn frame_init(width: u32, height: u32) -> Option<SpinFrame> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(SpinFrame {
        width,
        height,
        glyphs: vec![0; (width * height) as usize],
        colors: vec![0; (width * height) as usize],
    })
}

fn frame_clear(frame: &mut SpinFrame) {
    frame.glyphs.fill(0);
    frame.colors.fill(0);
}

/// Copies the art face-on into the center of the frame, exactly as
/// authored.
fn frame_put_art(frame: &mut SpinFrame, art: &SpinArt) {
    frame_clear(frame);
    if frame.width < art.width || frame.height < art.height {
        return;
    }
    let left = (frame.width - art.width) / 2;
    let top = (frame.height - art.height) / 2;
    for y in 0..art.height {
        let to = ((top + y) * frame.width + left) as usize;
        let from = (y * art.width) as usize;
        frame.glyphs[to..to + art.width as usize]
            .copy_from_slice(&art.glyphs[from..from + art.width as usize]);
        frame.colors[to..to + art.width as usize]
            .copy_from_slice(&art.colors[from..from + art.width as usize]);
    }
}

#[derive(PartialEq)]
enum SpinHit {
    None,
    Front,
    Back,
    Wall,
}

/// Casts one ray (object space) through the extruded art. On a hit, `cell`
/// is the art cell and `normal` the object-space normal of the surface.
fn cast_ray(
    art: &SpinArt,
    g: &SpinGeometry,
    o: &[f32; 3],
    d: &[f32; 3],
    cell: &mut u32,
    normal: &mut [f32; 3],
) -> SpinHit {
    let lo = [-g.hx, -g.hy, -g.hz];
    let hi = [g.hx, g.hy, g.hz];
    let (mut t_near, mut t_far) = (0.0f32, f32::INFINITY);
    let mut axis: i32 = -1;
    for a in 0..3 {
        if f32::abs(d[a]) < 1e-12 {
            if o[a] < lo[a] || o[a] > hi[a] {
                return SpinHit::None;
            }
            continue;
        }
        let (mut t1, mut t2) = ((lo[a] - o[a]) / d[a], (hi[a] - o[a]) / d[a]);
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        if t1 > t_near {
            t_near = t1;
            axis = a as i32;
        }
        if t2 < t_far {
            t_far = t2;
        }
        if t_near > t_far {
            return SpinHit::None;
        }
    }
    if axis < 0 {
        return SpinHit::None;
    }

    // Grid coordinates: x in columns from the left, y in rows from the top.
    let (w, h) = (art.width as i32, art.height as i32);
    let gx = (o[0] + d[0] * t_near + g.hx) / FF_SPIN_CELL_ASPECT;
    let gy = g.hy - (o[1] + d[1] * t_near);
    let dgx = d[0] / FF_SPIN_CELL_ASPECT;
    let dgy = -d[1];
    let mut cx = f32::floor(gx) as i32;
    let mut cy = f32::floor(gy) as i32;
    if cx < 0 {
        cx = 0;
    }
    if cx >= w {
        cx = w - 1;
    }
    if cy < 0 {
        cy = 0;
    }
    if cy >= h {
        cy = h - 1;
    }

    *normal = [0.0; 3];
    if art.glyphs[(cy as u32 * art.width + cx as u32) as usize] != 0 {
        *cell = cy as u32 * art.width + cx as u32;
        normal[axis as usize] = if d[axis as usize] > 0.0 { -1.0 } else { 1.0 };
        if axis == 2 {
            return if d[2] < 0.0 { SpinHit::Front } else { SpinHit::Back };
        }
        return SpinHit::Wall;
    }

    // Walk the grid cell by cell (Amanatides-Woo) until the ray leaves the
    // slab.
    let step_x: i32 = if dgx > 0.0 { 1 } else { -1 };
    let step_y: i32 = if dgy > 0.0 { 1 } else { -1 };
    let mut t_max_x = if dgx != 0.0 {
        t_near + ((if dgx > 0.0 { cx + 1 } else { cx }) as f32 - gx) / dgx
    } else {
        f32::INFINITY
    };
    let mut t_max_y = if dgy != 0.0 {
        t_near + ((if dgy > 0.0 { cy + 1 } else { cy }) as f32 - gy) / dgy
    } else {
        f32::INFINITY
    };
    let t_delta_x = if dgx != 0.0 { (1.0 / dgx).abs() } else { f32::INFINITY };
    let t_delta_y = if dgy != 0.0 { (1.0 / dgy).abs() } else { f32::INFINITY };
    loop {
        let along_x = t_max_x < t_max_y;
        if (if along_x { t_max_x } else { t_max_y }) > t_far {
            return SpinHit::None;
        }
        if along_x {
            cx += step_x;
            t_max_x += t_delta_x;
            if cx < 0 || cx >= w {
                return SpinHit::None;
            }
        } else {
            cy += step_y;
            t_max_y += t_delta_y;
            if cy < 0 || cy >= h {
                return SpinHit::None;
            }
        }
        if art.glyphs[(cy as u32 * art.width + cx as u32) as usize] != 0 {
            *cell = cy as u32 * art.width + cx as u32;
            if along_x {
                normal[0] = -(step_x as f32);
            } else {
                normal[1] = step_y as f32; // grid rows grow downward
            }
            return SpinHit::Wall;
        }
    }
}

fn render_art(art: &SpinArt, yaw: f32, tilt: f32, frame: &mut SpinFrame) {
    frame_clear(frame);
    let g = spin_geometry(art);
    let (rot, inv) = rotation_matrix(yaw, tilt);
    let origin = [inv[0][2] * g.camera, inv[1][2] * g.camera, inv[2][2] * g.camera];
    let ramp_last = (FF_SPIN_WALL_RAMP.len() - 1) as f32;

    for y in 0..frame.height {
        let v = frame.height as f32 * 0.5 - (y as f32 + 0.5);
        for x in 0..frame.width {
            let u = (x as f32 + 0.5 - frame.width as f32 * 0.5) * FF_SPIN_CELL_ASPECT;
            let mut d = [0.0f32; 3];
            for i in 0..3 {
                d[i] = inv[i][0] * u + inv[i][1] * v - inv[i][2] * g.focal;
            }
            let mut cell = 0u32;
            let mut n = [0.0f32; 3];
            let hit = cast_ray(art, &g, &origin, &d, &mut cell, &mut n);
            if hit == SpinHit::None {
                continue;
            }

            let index = (y * frame.width + x) as usize;
            frame.colors[index] = art.colors[cell as usize];
            match hit {
                SpinHit::Front => frame.glyphs[index] = art.glyphs[cell as usize],
                SpinHit::Back => frame.glyphs[index] = mirror_glyph(art.glyphs[cell as usize]),
                _ => {
                    let mut diffuse = 0.0f32;
                    for i in 0..3 {
                        diffuse += (rot[i][0] * n[0] + rot[i][1] * n[1] + rot[i][2] * n[2]) * FF_SPIN_LIGHT[i];
                    }
                    let luminance = 0.15f32 + 0.85f32 * f32::max(diffuse, 0.0);
                    frame.glyphs[index] = FF_SPIN_WALL_RAMP[(luminance * ramp_last + 0.5) as usize];
                },
            }
        }
    }
}

fn append_sgr(out: &mut String, art: &SpinArt, color: u8, bold: bool) {
    out.push_str(if bold { "\x1b[0;1" } else { "\x1b[0" });
    if (color as usize) < art.palette.len() && !art.palette[color as usize].is_empty() {
        out.push(';');
        out.push_str(&art.palette[color as usize]);
    }
    out.push('m');
}

fn push_code_point(out: &mut String, glyph: u32) {
    // ffStrbufAppendUtf32CodePoint encodes anything up to U+10FFFF and
    // emits U+FFFD above; surrogate code points would come out as CESU-8
    // bytes upstream, which Rust strings cannot carry, so they take the
    // replacement character here too.
    match char::from_u32(glyph) {
        Some(ch) => out.push(ch),
        None => out.push('\u{FFFD}'),
    }
}

/// Appends `count` cells starting at `index` with colors, ending in a
/// reset.
fn append_cells(out: &mut String, frame: &SpinFrame, art: &SpinArt, index: u32, count: u32, color: bool, bold: bool) {
    let mut active: i32 = -1;
    for i in index..index + count {
        let glyph = frame.glyphs[i as usize];
        if glyph == 0 {
            out.push(' ');
            continue;
        }
        if color && frame.colors[i as usize] as i32 != active {
            active = frame.colors[i as usize] as i32;
            append_sgr(out, art, frame.colors[i as usize], bold);
        }
        push_code_point(out, glyph);
    }
    if active >= 0 {
        out.push_str("\x1b[0m");
    }
}

/// One line per frame row, each ending in '\n'. `trim_right` drops the
/// empty cells at the end of a row, which is what the other text logo types
/// print; a frame that is redrawn as it turns must keep its full width
/// instead, or the information column would move with the angle.
fn frame_to_ansi(frame: &SpinFrame, art: &SpinArt, color: bool, bold: bool, trim_right: bool) -> String {
    let mut out = String::with_capacity((frame.width * frame.height * 4) as usize);
    for y in 0..frame.height {
        let mut length = frame.width;
        if trim_right {
            // Empty cells at the end of a line are not part of the art.
            // Printing them would pad every line to the frame width and
            // make the output differ from the other text logo types.
            while length > 0 && frame.glyphs[(y * frame.width + length - 1) as usize] == 0 {
                length -= 1;
            }
        }
        append_cells(&mut out, frame, art, y * frame.width, length, color, bold);
        out.push('\n');
    }
    out
}

/// The per-process spin state (instance.state.logoSpin): the parsed art and
/// everything the watch loop's redraw needs.
pub(crate) struct SpinState {
    art: SpinArt,
    configured: PositionOpt,
    speed_per_frame: f32,
    speed_per_round: f32,
    tilt: f32,
    angle: f32,
    animated: bool,
    pipe: bool,
    degrees: f32,
    started: bool,
    printed: bool,
    printed_width: u32,
    printed_height: u32,
    last_draw_ms: f64,
}

/// One ffSpinLogoPrint's frame: the ANSI text plus the clear flag the
/// redraw bookkeeping needs.
pub(crate) struct SpinDraw {
    /// `\e[2J\e[H` when the frame size changed under an already-printed
    /// logo (a resize would otherwise keep cells of the previous frame).
    pub clear: bool,
    pub ansi: String,
}

impl SpinState {
    pub(crate) fn new(
        art: SpinArt,
        configured: PositionOpt,
        opts: &crate::logo::LogoOptions,
        animated: bool,
        pipe: bool,
    ) -> Self {
        Self {
            art,
            configured,
            speed_per_frame: opts.spin_speed_per_frame,
            speed_per_round: opts.spin_speed_per_round,
            tilt: opts.spin_tilt,
            angle: opts.spin_angle,
            animated,
            pipe,
            degrees: 0.0,
            started: false,
            printed: false,
            printed_width: 0,
            printed_height: 0,
            last_draw_ms: 0.0,
        }
    }

    /// One step of the turn. `speedPerRound` wins over `speedPerFrame` when
    /// both are set; 0 means unset for either.
    fn step_degrees(&mut self) -> f32 {
        if self.speed_per_round != 0.0 {
            // Seconds per revolution: follow the clock instead of counting
            // frames, so the turn keeps its pace even when a refresh takes
            // longer than the interval asked for.
            let now = time_get_tick();
            let elapsed_ms = now - self.last_draw_ms;
            self.last_draw_ms = now;
            return (360.0 / self.speed_per_round as f64 * elapsed_ms / 1000.0) as f32;
        }
        if self.speed_per_frame != 0.0 {
            return self.speed_per_frame;
        }
        SPIN_DEFAULT_DEGREES_PER_FRAME
    }

    /// Advances the rotation by one `--watch` refresh and returns true when
    /// the logo has to be drawn again.
    pub(crate) fn tick(&mut self) -> bool {
        if !self.printed {
            return false;
        }
        let step = self.step_degrees();
        if step == 0.0 {
            return false; // a logo that does not turn does not have to be redrawn
        }
        // The angle is wrapped instead of growing, or a long `--watch`
        // session would lose precision in the sine and cosine.
        let degrees = self.degrees + step;
        self.degrees = degrees - f32::floor(degrees / 360.0) * 360.0;
        true
    }

    /// Draws the current frame. Called from the logo load and then once per
    /// tick.
    pub(crate) fn draw(&mut self) -> Option<SpinDraw> {
        if !self.started {
            // The first frame starts where `logo.spin.angle` says, and
            // every `--watch` refresh turns it by one step.
            self.started = true;
            let mut degrees = self.angle % 360.0;
            if degrees < 0.0 {
                degrees += 360.0;
            }
            self.degrees = degrees;
        }
        let spin_angle = self.degrees * PI_F32 / 180.0;

        let tilt = self.tilt * PI_F32 / 180.0;
        let (mut width, mut height) = (self.art.width, self.art.height);
        // With `--watch` the frame has to stay the same size from the first
        // draw on, or the information column would move when the logo
        // starts turning.
        if self.animated || spin_angle != 0.0 || tilt != 0.0 {
            // A turned slab sticks out of the art's box, so the frame has
            // to make room for its widest angle or the logo would be
            // clipped.
            let (size_width, size_height) = frame_size(&self.art, tilt);
            width = size_width;
            height = size_height;
        }

        let mut frame = frame_init(width, height)?;

        if spin_angle == 0.0 && tilt == 0.0 {
            frame_put_art(&mut frame, &self.art); // face on: exactly the original art
        } else {
            render_art(&self.art, spin_angle, tilt, &mut frame);
        }

        // The frame already carries its own SGR sequences; `bold` mirrors
        // display.brightColor, whose default (dark terminal theme) this
        // port assumes like the cache line's bolt does.
        let mut ansi = frame_to_ansi(&frame, &self.art, !self.pipe, true, !self.animated);
        // The serialiser ends every row with a newline; the last one would
        // add an empty line to the logo and change the output from the
        // other logo types.
        if ansi.ends_with('\n') {
            ansi.pop();
        }

        let clear = self.printed && (width != self.printed_width || height != self.printed_height);
        self.printed = true;
        self.printed_width = width;
        self.printed_height = height;
        self.last_draw_ms = time_get_tick(); // start measuring the next step from now
        Some(SpinDraw { clear, ansi })
    }
}

/// The watch loop's `if (ffSpinTick()) ffLogoPrint()`: one rotation step
/// per refresh, then a fresh redraw through the chars pipeline. None when
/// the logo stays still (or is absent).
pub(crate) fn watch_redraw(instance: &Instance) -> Option<(bool, crate::logo::Logo)> {
    let mut guard = instance.spin.borrow_mut();
    let state = guard.as_mut()?;
    if !state.tick() {
        return None;
    }
    let draw = state.draw()?;
    let configured = state.configured;
    drop(guard);
    let logo =
        crate::logo::rebuild_spin_chars(&draw.ansi, configured, &instance.logo_options, instance.pipe);
    Some((draw.clear, logo))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(colors: &[&str], text: &str) -> SpinArt {
        let Some(art) = spin_art_parse(text.as_bytes(), colors) else {
            panic!("parse failed")
        };
        art
    }

    #[test]
    fn utf8_decode_walks_sequences_and_substitutes() {
        let mut cursor = 0;
        assert_eq!(utf8_decode("Aé█".as_bytes(), &mut cursor), b'A' as u32);
        assert_eq!(cursor, 1);
        assert_eq!(utf8_decode("Aé█".as_bytes(), &mut cursor), 0xE9);
        assert_eq!(cursor, 3);
        assert_eq!(utf8_decode("Aé█".as_bytes(), &mut cursor), 0x2588);
        assert_eq!(cursor, 6);
        // A lone continuation byte substitutes '?' and advances one.
        let mut cursor = 0;
        assert_eq!(utf8_decode(&[0x80], &mut cursor), b'?' as u32);
        assert_eq!(cursor, 1);
        // A truncated sequence at the end substitutes too; the cursor
        // stops on the missing byte like upstream's `*cursor += i`.
        let mut cursor = 0;
        assert_eq!(utf8_decode(&[0xE2, 0x94], &mut cursor), b'?' as u32);
        assert_eq!(cursor, 2);
    }

    #[test]
    fn mirror_glyph_swaps_pairs_and_braille_dots() {
        assert_eq!(mirror_glyph(b'/' as u32), b'\\' as u32);
        assert_eq!(mirror_glyph(b'\\' as u32), b'/' as u32);
        assert_eq!(mirror_glyph(b'x' as u32), b'x' as u32);
        // U+258C left half block ↔ right half block
        assert_eq!(mirror_glyph(0x258C), 0x2590);
        // Braille U+2813 (dots 1,2,5,7?) — swap the left/right dot columns.
        assert_eq!(mirror_glyph(0x2801), 0x2808); // dot 1 ↔ dot 4
        assert_eq!(mirror_glyph(0x28FF), 0x28FF); // full block mirrors to itself
    }

    #[test]
    fn art_walk_measures_and_fills_the_grid() {
        // Two rows. Tabs widen the column count by 4 and stay blank.
        let art = parse(&["31", "34"], "$1ab\tc\n$2d\n");
        assert_eq!((art.width, art.height), (7, 2));
        // (0,0) 'a' in slot 1, (1,0) 'b' in slot 1, (6,0) 'c' — the tab
        // cells stay empty — and (0,1) 'd' in slot 2.
        assert_eq!(art.glyphs[0], b'a' as u32);
        assert_eq!(art.glyphs[1], b'b' as u32);
        assert_eq!(art.glyphs[2], 0);
        assert_eq!(art.glyphs[3], 0);
        assert_eq!(art.glyphs[6], b'c' as u32);
        assert_eq!(art.glyphs[7], b'd' as u32);
        assert_eq!(art.colors[0], 1);
        assert_eq!(art.colors[6], 1);
        assert_eq!(art.colors[7], 2);
        assert_eq!(art.palette[1], "31");
        assert_eq!(art.palette[2], "34");
    }

    #[test]
    fn art_walk_crops_blank_borders_and_dedups_raw_sgr() {
        let art = parse(&[], "\x1b[1;34mab\x1b[0m\n\x1b[1;34mcd\x1b[0m\n\n");
        assert_eq!((art.width, art.height), (2, 2));
        // Both rows share the raw SGR: one slot after the 10 logo slots.
        assert_eq!(art.palette.len(), 11);
        assert_eq!(art.palette[10], "1;34");
        // A "0" or empty reset never occupies a slot.
        assert_eq!(art.colors[0], 10);
    }

    #[test]
    fn palette_add_caps_at_32() {
        let mut palette: Vec<String> = vec![String::new(); FF_SPIN_PALETTE_LOGO_SLOTS];
        for i in 0..(FF_SPIN_PALETTE_MAX - FF_SPIN_PALETTE_LOGO_SLOTS) {
            let params = format!("3{i}");
            let slot = palette_add(&mut palette, params.as_bytes());
            assert_eq!(slot as usize, FF_SPIN_PALETTE_LOGO_SLOTS + i);
        }
        // The 32nd distinct color falls back to the default slot.
        assert_eq!(palette_add(&mut palette, b"99"), 0);
        assert_eq!(palette.len(), FF_SPIN_PALETTE_MAX);
    }

    #[test]
    fn face_on_draw_matches_the_art_and_the_trailing_newline_strips() {
        let mut state = SpinState::new(
            parse(&["31"], "ab\ncd\n"),
            PositionOpt::Left,
            &crate::logo::LogoOptions::default(),
            false,
            false,
        );
        let Some(draw) = state.draw() else { panic!("draw failed") };
        // trimRight drops trailing blanks; each row starts in its palette
        // color with the bold prefix and ends in a reset; the serialiser's
        // final newline is stripped.
        assert_eq!(draw.ansi, "\x1b[0;1;31mab\x1b[0m\n\x1b[0;1;31mcd\x1b[0m");
        assert!(!draw.clear);
        assert_eq!((state.printed_width, state.printed_height), (2, 2));
    }

    #[test]
    fn tick_turns_and_wraps() {
        let opts = crate::logo::LogoOptions {
            spin_speed_per_frame: 100.0,
            ..crate::logo::LogoOptions::default()
        };
        let mut state = SpinState::new(parse(&[], "ab\n"), PositionOpt::Left, &opts, false, false);
        assert!(state.draw().is_some());
        assert!(state.tick());
        assert!((state.degrees - 100.0).abs() < f32::EPSILON);
        assert!(state.tick());
        assert!((state.degrees - 200.0).abs() < f32::EPSILON);
        // speedPerRound wins over speedPerFrame and follows the clock.
        let opts = crate::logo::LogoOptions {
            spin_speed_per_frame: 100.0,
            spin_speed_per_round: 360.0, // one revolution per second
            ..crate::logo::LogoOptions::default()
        };
        let mut state = SpinState::new(parse(&[], "ab\n"), PositionOpt::Left, &opts, false, false);
        assert!(state.draw().is_some());
        let step = state.step_degrees();
        // The elapsed time since the draw is tiny; the step is proportional.
        assert!((0.0..1.0).contains(&step), "{step}");
    }

    #[test]
    fn angle_starts_the_first_frame() {
        let opts = crate::logo::LogoOptions {
            spin_angle: 400.0, // wraps to 40
            spin_tilt: 80.0,   // a steep lean, so the frame grows
            ..crate::logo::LogoOptions::default()
        };
        let mut state = SpinState::new(parse(&[], "abcdefghij\nklmnopqrst\n0123456789\n"), PositionOpt::Left, &opts, false, false);
        let Some(draw) = state.draw() else { panic!("draw failed") };
        // The turned slab sticks out of the art's box: the 96-step sweep
        // grew the 10x3 frame to five rows.
        assert_eq!(state.printed_height, 5);
        assert!(draw.ansi.lines().count() >= 2);
    }

    #[test]
    fn pipe_drops_every_escape() {
        let mut state = SpinState::new(
            parse(&["31"], "ab\ncd\n"),
            PositionOpt::Left,
            &crate::logo::LogoOptions::default(),
            false,
            true,
        );
        let Some(draw) = state.draw() else { panic!("draw failed") };
        assert_eq!(draw.ansi, "ab\ncd");
    }

    #[test]
    fn animated_keeps_the_full_frame_width() {
        let mut state = SpinState::new(
            parse(&["31"], "ab\n"),
            PositionOpt::Left,
            &crate::logo::LogoOptions::default(),
            true,
            true,
        );
        let Some(draw) = state.draw() else { panic!("draw failed") };
        // No trimRight even in pipe mode: every row is padded to the frame
        // width, which the 96-step sweep grew beyond the art's box.
        let widths: Vec<usize> = draw.ansi.lines().map(|line| line.chars().count()).collect();
        assert!(widths.iter().all(|width| *width == widths[0]), "{widths:?}");
        assert!(widths[0] >= 2);
    }
}
