//! `display` module: monitors. Primary source is GNOME's mutter display
//! config over D-Bus (read in-process like fastfetch's displayserver
//! backend); sizes and the no-D-Bus fallback come from DRM EDID blobs,
//! a port of fastfetch's `edidHelper.c` (MIT).

use std::path::Path;

use crate::{instance::Instance, modules::Row};

/// EDID detailed timing descriptor scan range and stride.
const DTD_START: usize = 0x36;
const DTD_END: usize = 0x7E;
const DTD_STRIDE: usize = 0x12;

struct Edid {
    width: u32,
    height: u32,
    refresh: f64,
    name: Option<String>,
    physical_mm: (u32, u32),
}

fn parse_edid(data: &[u8]) -> Option<Edid> {
    const HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
    if data.len() < 128 || data[..8] != HEADER {
        return None;
    }

    let (width, height, refresh) = preferred_timing(data);
    Some(Edid {
        width: width?,
        height: height?,
        refresh: refresh.unwrap_or(0.0),
        name: monitor_name(data),
        physical_mm: physical_size(data),
    })
}

/// Preferred detailed timing descriptor: active pixels and refresh rate.
fn preferred_timing(data: &[u8]) -> (Option<u32>, Option<u32>, Option<f64>) {
    for i in (DTD_START..DTD_END).step_by(DTD_STRIDE) {
        // pixel clock of 0 marks a monitor descriptor, not a timing one
        if i + 1 >= data.len() || (data[i] == 0 && data[i + 1] == 0) {
            continue;
        }
        let hactive = u32::from(data[i + 2]) + (u32::from(data[i + 4] & 0xF0) << 4);
        let hblank = u32::from(data[i + 3]) + (u32::from(data[i + 4] & 0x0F) << 8);
        let vactive = u32::from(data[i + 5]) + (u32::from(data[i + 7] & 0xF0) << 4);
        let vblank = u32::from(data[i + 6]) + (u32::from(data[i + 7] & 0x0F) << 8);
        let pixclk = (u32::from(data[i + 1]) << 8) | u32::from(data[i]);
        if hactive + hblank == 0 || vactive + vblank == 0 {
            continue;
        }
        let refresh = f64::from(pixclk) * 10_000.0
            / f64::from(hactive + hblank)
            / f64::from(vactive + vblank);
        return (Some(hactive), Some(vactive), Some(refresh));
    }
    (None, None, None)
}

/// Monitor name from the 0xFC descriptor, falling back to the PnP id
/// (three letters plus the hex product code), like fastfetch.
fn monitor_name(data: &[u8]) -> Option<String> {
    for offset in [0x36, 0x48, 0x5A, 0x6C] {
        if data[offset] == 0 && data[offset + 1] == 0 && data[offset + 3] == 0xFC {
            let raw = &data[offset + 5..offset + 5 + 13];
            let end = raw.iter().position(|&b| b == 0x0A).unwrap_or(raw.len());
            let name: String = raw[..end]
                .iter()
                .filter(|b| b.is_ascii_graphic() || **b == b' ')
                .map(|b| *b as char)
                .collect();
            let name = name.trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    Some(vendor_and_model(data))
}

/// "ACR0E43"-style fallback id: three PNP letters + product code.
fn vendor_and_model(data: &[u8]) -> String {
    let letter = |bits: u32| char::from_u32(bits + u32::from(b'A') - 1).unwrap_or('?');
    let l1 = letter((u32::from(data[8]) >> 2) & 0x1F);
    let l2 = letter(((u32::from(data[8]) & 0x3) << 3) | ((u32::from(data[9]) & 0xE0) >> 5));
    let l3 = letter(u32::from(data[9]) & 0x1F);
    let code = u16::from(data[10]) | (u16::from(data[11]) << 8);
    format!("{l1}{l2}{l3}{code:04X}")
}

/// Physical panel size in millimeters, preferring the detailed timing
/// descriptor when it agrees with the basic display parameters (see
/// fastfetch issue #1406).
fn physical_size(data: &[u8]) -> (u32, u32) {
    let dw = ((u32::from(data[68]) & 0xF0) << 4) + u32::from(data[66]);
    let dh = ((u32::from(data[68]) & 0x0F) << 8) + u32::from(data[67]);
    let bw = u32::from(data[21]) * 10;
    let bh = u32::from(data[22]) * 10;

    if dw.abs_diff(bw) < 10 && dh.abs_diff(bh) < 10 && dw > 0 && dh > 0 {
        (dw, dh)
    } else {
        (bw, bh)
    }
}

/// Connector paths under /sys/class/drm that carry an EDID.
fn connectors() -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut connectors: Vec<std::path::PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("card") && name.contains('-')
        })
        .map(|entry| entry.path())
        .collect();
    connectors.sort();
    connectors
}

/// Reads the EDID of a connector that is physically connected.
fn read_edid(connector: &Path) -> Option<Edid> {
    let status = crate::util::read_trimmed_in(connector, "status")?;
    if status != "connected" {
        return None;
    }
    let data = std::fs::read(connector.join("edid")).ok()?;
    parse_edid(&data)
}

/// EDIDs keyed by connector name without the `cardN-` prefix (e.g. "DP-1").
fn edids_by_connector() -> Vec<(String, Edid)> {
    connectors()
        .into_iter()
        .filter_map(|connector| {
            let name = connector.file_name()?.to_string_lossy().into_owned();
            let suffix = name.split_once('-')?.1.to_string();
            let edid = read_edid(&connector)?;
            Some((suffix, edid))
        })
        .collect()
}

/// One active display from mutter: `('DP-1', 'ACR', 'XV271U M3', 'serial')`
/// header plus the mode carrying `{'is-current': <true>}`, whose name is
/// "WxH@refresh".
struct MutterDisplay {
    connector: String,
    model: String,
    width: u32,
    height: u32,
    refresh: f64,
}

fn mutter_displays() -> Option<Vec<MutterDisplay>> {
    let text = std::borrow::Cow::from(crate::dbus::call_method_printed(
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
        "GetCurrentState",
    )?);

    let mut displays = Vec::new();
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find("(('") {
        let header_start = cursor + rel;
        let Some(header_end) = text[header_start..].find(')') else {
            break;
        };
        let header_end = header_end + header_start;

        // header: (('<connector>', '<vendor>', '<model>', '<serial>')
        let header = &text[header_start + 2..header_end];
        let mut fields = header.split("', '").map(|f| f.trim_start_matches('(').trim_matches('\''));
        let (Some(connector), Some(model)) = (fields.next(), fields.nth(1)) else {
            cursor = header_end;
            continue;
        };

        // modes region runs until the next monitor header (or end of output)
        let next_header = text[header_end..].find("(('").map(|rel| header_end + rel);
        let region = match next_header {
            Some(end) => &text[header_end..end],
            None => &text[header_end..],
        };

        if let Some(mode_name) = current_mode_name(region)
            && let Some((width, height, refresh)) = parse_mode_name(&mode_name)
        {
            displays.push(MutterDisplay {
                connector: connector.to_string(),
                model: model.to_string(),
                width,
                height,
                refresh,
            });
        }

        match next_header {
            Some(end) => cursor = end,
            None => break,
        }
    }

    (!displays.is_empty()).then_some(displays)
}

/// The mode name ("2560x1440@179.877") of the tuple flagged is-current.
fn current_mode_name(region: &str) -> Option<String> {
    let marker = region.find("'is-current': <true>")?;
    let tuple_start = region[..marker].rfind("('")? + 1;
    let name_start = region[tuple_start..].find('\'')? + tuple_start + 1;
    let name_end = region[name_start..].find('\'')? + name_start;
    Some(region[name_start..name_end].to_string())
}

fn parse_mode_name(name: &str) -> Option<(u32, u32, f64)> {
    let (size, refresh) = name.split_once('@')?;
    let (width, height) = size.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?, refresh.parse().ok()?))
}

fn format_value(
    width: u32,
    height: u32,
    refresh: f64,
    physical_mm: (u32, u32),
    built_in: bool,
) -> String {
    let mut value = format!("{width}x{height}");
    let inch = f64::hypot(f64::from(physical_mm.0), f64::from(physical_mm.1)) / 25.4;
    if inch > 1.0 {
        value.push_str(&format!(" in {}\"", (inch + 0.5) as u32));
    }
    if refresh > 0.0 {
        value.push_str(&format!(", {} Hz", (refresh + 0.5) as u64));
    }
    value.push_str(if built_in {
        " [Built-in]"
    } else {
        " [External]"
    });
    value
}

/// GNOME session path: mutter tells us the active modes; EDID adds sizes.
fn rows_from_mutter(displays: &[MutterDisplay], edids: &[(String, Edid)]) -> Vec<Row> {
    displays
        .iter()
        .map(|display| {
            let edid = edids.iter().find(|(name, _)| *name == display.connector);
            let physical_mm = edid.map_or((0, 0), |(_, edid)| edid.physical_mm);
            let built_in = display.connector.contains("eDP") || display.connector.contains("LVDS");
            let value =
                format_value(display.width, display.height, display.refresh, physical_mm, built_in);
            let name = edid
                .and_then(|(_, edid)| edid.name.clone())
                .unwrap_or_else(|| display.model.clone());
            Row::new(&format!("Display ({name})"), value)
        })
        .collect()
}

/// Fallback without D-Bus: every connected connector's preferred timing.
fn rows_from_edid(edids: &[(String, Edid)]) -> Vec<Row> {
    let mut rows = Vec::new();
    for (connector, edid) in edids {
        let built_in = connector.contains("eDP") || connector.contains("LVDS");
        let value = format_value(edid.width, edid.height, edid.refresh, edid.physical_mm, built_in);
        let key = match &edid.name {
            Some(name) => format!("Display ({name})"),
            None => "Display".to_string(),
        };
        rows.push(Row::new(&key, value));
    }
    rows
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let edids = edids_by_connector();

    match mutter_displays() {
        Some(displays) => rows_from_mutter(&displays, &edids),
        None => rows_from_edid(&edids),
    }
}
