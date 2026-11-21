//! `display` module: monitors. Primary source is GNOME's mutter display
//! config over D-Bus (read in-process like fastfetch's displayserver
//! backend); sizes and the no-D-Bus fallback come from DRM EDID blobs,
//! a port of fastfetch's `edidHelper.c` (MIT).

use std::path::Path;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
};

/// EDID detailed timing descriptor scan range and stride.
const DTD_START: usize = 0x36;
const DTD_END: usize = 0x7E;
const DTD_STRIDE: usize = 0x12;

pub(crate) struct Edid {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) refresh: f64,
    pub(crate) name: Option<String>,
    pub(crate) physical_mm: (u32, u32),
    /// ffEdidGetHdrCompatible: a CTA-861 extension advertises HDR metadata.
    pub(crate) hdr: bool,
    /// Basic Display Parameters (bytes 21/22): the kernel exposes these as
    /// the connector's mm sizes, which is what wl_output reports.
    pub(crate) bdp_mm: (u32, u32),
    /// The 0xFF descriptor serial, else the packed 32-bit id as 0x%08X.
    pub(crate) serial: Option<String>,
    /// (year, week) from bytes 17/16.
    pub(crate) manufacture: Option<(u16, u16)>,
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
        hdr: hdr_compatible(data),
        bdp_mm: (u32::from(data[21]) * 10, u32::from(data[22]) * 10),
        serial: descriptor_serial(data),
        manufacture: manufacture_date(data),
    })
}

/// ffEdidGetSerial: the 0xFF descriptor's 13 ASCII bytes, else the packed
/// serial word as 0x%08X when it isn't 0/0xFFFFFFFF.
fn descriptor_serial(data: &[u8]) -> Option<String> {
    for offset in [54, 72, 90, 108] {
        if data[offset] == 0 && data[offset + 1] == 0 && data[offset + 3] == 0xFF {
            let serial: String = data[offset + 5..offset + 18]
                .iter()
                .take_while(|&&ch| ch != 0x0A && ch != 0x00)
                .map(|&ch| ch as char)
                .collect();
            let serial = serial.trim_matches(' ').to_string();
            if !serial.is_empty() {
                return Some(serial);
            }
        }
    }
    let int_serial = u32::from(data[12])
        | u32::from(data[13]) << 8
        | u32::from(data[14]) << 16
        | u32::from(data[15]) << 24;
    if int_serial != 0 && int_serial != 0xFFFF_FFFF {
        return Some(format!("0x{int_serial:08X}"));
    }
    None
}

/// ffEdidGetManufactureDate: (year, week) from bytes 17/16, gated on a sane
/// year byte; a 0xFF week means "not reported".
fn manufacture_date(data: &[u8]) -> Option<(u16, u16)> {
    if data[17] > 0 && data[17] < 0xFF {
        let mut week = u16::from(data[16]);
        if week == 0xFF {
            week = 0;
        }
        Some((u16::from(data[17]) + 1990, week))
    } else {
        None
    }
}

/// Port of ffEdidGetHdrCompatible: any CTA-861 extension block (>= 256
/// bytes of EDID) with an HDR Static or Dynamic Metadata Data Block.
pub(crate) fn hdr_compatible(data: &[u8]) -> bool {
    if data.len() < 256 {
        return false;
    }
    let mut base = 128;
    while base + 128 <= data.len() {
        let cta = &data[base..base + 128];
        if cta[0] == 0x02 && cta[1] >= 0x03 && cta[2] > 4 {
            let end = (cta[2] as usize).min(127);
            let mut i = 4;
            while i + 1 < end {
                let blk_len = (cta[i] & 0x1f) as usize;
                if blk_len > 0 && (cta[i] & 0xe0) >> 5 == 0x07 && matches!(cta[i + 1], 6 | 7) {
                    return true;
                }
                i += blk_len + 1;
            }
        }
        base += 128;
    }
    false
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
pub(crate) fn edids_by_connector() -> Vec<(String, Edid)> {
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
pub(crate) struct MutterDisplay {
    pub(crate) connector: String,
    pub(crate) model: String,
    /// The monitor specs' serial ('' when absent or "unknown").
    pub(crate) serial: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) refresh: f64,
    /// The monitor properties dict: 'is-builtin' (None when absent).
    pub(crate) is_builtin: Option<bool>,
    /// 'color-mode' == 1 (BT2100) means HDR is active.
    pub(crate) color_mode: Option<u32>,
    /// 'supported-color-modes' lists 1 (BT2100).
    pub(crate) hdr_supported: bool,
    /// The logical monitor's scale, when this connector is listed there.
    pub(crate) scale: f64,
    /// The logical monitor's transform (0 = normal).
    pub(crate) transform: u32,
    /// The logical monitor's primary flag, when listed there.
    pub(crate) primary: Option<bool>,
}

pub(crate) fn mutter_displays() -> Option<Vec<MutterDisplay>> {
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
            // header: (connector, vendor, product, serial)
            let serial = header
                .split("', '")
                .nth(3)
                .map(|serial| serial.trim_matches('\'').trim_end_matches(')').trim_matches('\'').to_string())
                .filter(|serial| !serial.is_empty() && serial != "unknown")
                .unwrap_or_default();
            let logical = logical_monitor_for(&text, connector);
            displays.push(MutterDisplay {
                connector: connector.to_string(),
                model: model.to_string(),
                serial,
                width,
                height,
                refresh,
                is_builtin: region.contains("'is-builtin': <true>").then_some(true).or_else(|| {
                    region.contains("'is-builtin': <false>").then_some(false)
                }),
                color_mode: property_u32(region, "'color-mode': <"),
                hdr_supported: supported_bt2100(region),
                scale: logical.as_ref().map_or(1.0, |logical| logical.2),
                transform: logical.as_ref().map_or(0, |logical| logical.3),
                primary: logical.as_ref().map(|logical| logical.4),
            });
        }

        match next_header {
            Some(end) => cursor = end,
            None => break,
        }
    }

    (!displays.is_empty()).then_some(displays)
}

/// The uint32 payload of a `'key': <N>` property in the rendered dict.
fn property_u32(region: &str, key: &str) -> Option<u32> {
    let start = region.find(key)? + key.len();
    let end = region[start..].find('>')? + start;
    region[start..end].parse().ok()
}

/// Whether 'supported-color-modes' lists 1 (FF_MUTTER_COLOR_MODE_BT2100).
fn supported_bt2100(region: &str) -> bool {
    const KEY: &str = "'supported-color-modes': <[";
    region
        .find(KEY)
        .and_then(|start| {
            let start = start + KEY.len();
            region[start..]
                .find("]>")
                .map(|end| region[start..start + end].split(", ").any(|mode| mode == "1"))
        })
        .unwrap_or(false)
}

/// (x, y, scale, transform, primary) of the logical monitor listing
/// `connector`; the tuple opens with two integers, unlike mode tuples
/// (which open with a quoted mode name).
fn logical_monitor_for(text: &str, connector: &str) -> Option<(i32, i32, f64, u32, bool)> {
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find(", [('") {
        let monitors_start = cursor + rel;
        let Some(tuple_open) = text[..monitors_start].rfind("[(") else {
            cursor = monitors_start + 1;
            continue;
        };
        let tuple = &text[tuple_open + 2..];
        let mut parts = tuple.splitn(6, ", ");
        let parsed = (|| {
            Some((
                parts.next()?.trim_start_matches('(').parse::<i32>().ok()?,
                parts.next()?.parse::<i32>().ok()?,
                parts.next()?.parse::<f64>().ok()?,
                parts.next()?.parse::<u32>().ok()?,
                parts.next()?.parse::<bool>().ok()?,
            ))
        })();
        let monitors_list = parts.next().unwrap_or_default();
        match parsed {
            // The quoted connector must appear inside the monitor list.
            Some((x, y, scale, transform, primary))
                if monitors_list.contains(&format!("'{connector}'")) =>
            {
                return Some((x, y, scale, transform, primary));
            },
            _ => cursor = monitors_start + 1,
        }
    }
    None
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
fn rows_from_mutter(instance: &Instance, displays: &[MutterDisplay], edids: &[(String, Edid)]) -> Vec<Row> {
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
            Row::new(&format!("{} ({name})", instance.display_name("display")), value)
        })
        .collect()
}

/// Fallback without D-Bus: every connected connector's preferred timing.
fn rows_from_edid(instance: &Instance, edids: &[(String, Edid)]) -> Vec<Row> {
    let mut rows = Vec::new();
    for (connector, edid) in edids {
        let built_in = connector.contains("eDP") || connector.contains("LVDS");
        let value = format_value(edid.width, edid.height, edid.refresh, edid.physical_mm, built_in);
        let key = match &edid.name {
            Some(name) => format!("{} ({name})", instance.display_name("display")),
            None => "Display".to_string(),
        };
        rows.push(Row::new(&key, value));
    }
    rows
}

/// The number of displays the display server reports; the brightness module
/// uses it to decide whether DDC/CI probing is worth it.
pub(crate) fn display_count() -> usize {
    mutter_displays().map_or(0, |displays| displays.len())
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let edids = edids_by_connector();

    match mutter_displays() {
        Some(displays) => rows_from_mutter(instance, &displays, &edids),
        None => rows_from_edid(instance, &edids),
    }
}

// ---- JSON (ffGenerateDisplayJsonResult over the mutter-enriched results) ----

const HDR_UNKNOWN: u8 = 0;
const HDR_UNSUPPORTED: u8 = 1;
const HDR_SUPPORTED: u8 = 2;
const HDR_ENABLED: u8 = 3;

const TYPE_UNKNOWN: u8 = 0;
const TYPE_BUILTIN: u8 = 1;
const TYPE_EXTERNAL: u8 = 2;

/// ffWaylandGenerateIdFromName: the last eight name bytes as a little
/// endian u64, or the whole (shorter) name zero padded.
fn generate_id_from_name(name: &str) -> u64 {
    let bytes = name.as_bytes();
    let mut id = [0u8; 8];
    if bytes.len() > 8 {
        id.copy_from_slice(&bytes[bytes.len() - 8..]);
    } else if !bytes.is_empty() {
        id[..bytes.len()].copy_from_slice(bytes);
    }
    u64::from_le_bytes(id)
}

pub struct DisplayResult {
    pub id: u64,
    pub name: String,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
    pub refresh: f64,
    pub dpi: u32,
    pub preferred_width: u32,
    pub preferred_height: u32,
    pub preferred_refresh: f64,
    pub physical_width: u32,
    pub physical_height: u32,
    pub rotation: u32,
    pub bit_depth: u8,
    pub hdr_status: u8,
    pub display_type: u8,
    pub manufacture_year: u16,
    pub manufacture_week: u16,
    pub serial: String,
}

/// The display list, mutter-enriched like ffdsConnectMutter + the EDID
/// fallbacks from mutterApplyEdid. dpi comes from the logical monitor's
/// scale (96 * scale) like the wl_output scale event.
fn detect() -> Option<Vec<DisplayResult>> {
    let displays = mutter_displays()?;
    let edids = edids_by_connector();

    Some(
        displays
            .iter()
            .map(|display| {
                let edid = edids.iter().find(|(name, _)| *name == display.connector);
                // mutterApplyEdid: the EDID fills what mutter left unknown.
                let mut hdr_status = if display.color_mode == Some(1) {
                    HDR_ENABLED
                } else if display.hdr_supported {
                    HDR_SUPPORTED
                } else {
                    HDR_UNKNOWN
                };
                if hdr_status == HDR_UNKNOWN {
                    hdr_status = if edid.is_some_and(|(_, edid)| edid.hdr) {
                        HDR_SUPPORTED
                    } else {
                        HDR_UNSUPPORTED
                    };
                }

                DisplayResult {
                    id: generate_id_from_name(&display.connector),
                    name: edid
                        .and_then(|(_, edid)| edid.name.clone())
                        .unwrap_or_else(|| display.model.clone()),
                    primary: display.primary.unwrap_or(false),
                    width: display.width,
                    height: display.height,
                    refresh: display.refresh,
                    dpi: (96.0 * display.scale) as u32,
                    preferred_width: edid.map_or(0, |(_, edid)| edid.width),
                    preferred_height: edid.map_or(0, |(_, edid)| edid.height),
                    preferred_refresh: edid.map_or(0.0, |(_, edid)| edid.refresh),
                    physical_width: edid.map_or(0, |(_, edid)| edid.bdp_mm.0),
                    physical_height: edid.map_or(0, |(_, edid)| edid.bdp_mm.1),
                    // wl_output transforms 0-3 map to whole rotations.
                    rotation: match display.transform {
                        1 => 90,
                        2 => 180,
                        3 => 270,
                        _ => 0,
                    },
                    bit_depth: 0,
                    hdr_status,
                    display_type: match display.is_builtin {
                        Some(true) => TYPE_BUILTIN,
                        Some(false) => TYPE_EXTERNAL,
                        None => {
                            // No is-builtin property: the connector-name guess.
                            if display.connector.contains("eDP") || display.connector.contains("LVDS") {
                                TYPE_BUILTIN
                            } else {
                                TYPE_UNKNOWN
                            }
                        },
                    },
                    manufacture_year: edid.and_then(|(_, edid)| edid.manufacture).map_or(0, |(year, _)| year),
                    manufacture_week: edid.and_then(|(_, edid)| edid.manufacture).map_or(0, |(_, week)| week),
                    serial: if display.serial.is_empty() {
                        edid.and_then(|(_, edid)| edid.serial.clone()).unwrap_or_default()
                    } else {
                        display.serial.clone()
                    },
                }
            })
            .collect(),
    )
}

/// ffGenerateDisplayJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let Some(displays) = detect() else {
        return JsonOutcome::Error("Couldn't detect display".to_string());
    };

    let mut arr = Json::Array(Vec::new());
    for display in &displays {
        let mut obj = Json::obj();
        obj.push("id", display.id);
        obj.push("name", display.name.clone());
        obj.push("primary", display.primary);

        let mut output = Json::obj();
        output.push("width", display.width as u64);
        output.push("height", display.height as u64);
        output.push("refreshRate", display.refresh);
        match display.hdr_status {
            HDR_UNKNOWN => output.push("drrStatus", Json::Null),
            _ => {
                // The DRR status stays unknown on wayland; the other enum
                // values are unreachable here.
                output.push("drrStatus", Json::Null)
            },
        }
        output.push("dpi", display.dpi as u64);
        obj.push("output", output);

        let mut scaled = Json::obj();
        scaled.push("width", ((display.width * 96 + display.dpi / 2) / display.dpi) as u64);
        scaled.push("height", ((display.height * 96 + display.dpi / 2) / display.dpi) as u64);
        obj.push("scaled", scaled);

        let mut preferred = Json::obj();
        preferred.push("width", display.preferred_width as u64);
        preferred.push("height", display.preferred_height as u64);
        preferred.push("refreshRate", display.preferred_refresh);
        obj.push("preferred", preferred);

        let mut physical = Json::obj();
        physical.push("width", display.physical_width as u64);
        physical.push("height", display.physical_height as u64);
        obj.push("physical", physical);

        obj.push("rotation", display.rotation as u64);
        obj.push("bitDepth", display.bit_depth as u64);
        match display.hdr_status {
            HDR_UNKNOWN => obj.push("hdrStatus", Json::Null),
            HDR_UNSUPPORTED => obj.push("hdrStatus", "Unsupported"),
            HDR_SUPPORTED => obj.push("hdrStatus", "Supported"),
            _ => obj.push("hdrStatus", "Enabled"),
        }
        match display.display_type {
            TYPE_BUILTIN => obj.push("type", "Builtin"),
            TYPE_EXTERNAL => obj.push("type", "External"),
            _ => obj.push("type", "Unknown"),
        }
        if display.manufacture_year != 0 {
            let mut manufacture_date = Json::obj();
            manufacture_date.push("year", display.manufacture_year as u64);
            manufacture_date.push("week", display.manufacture_week as u64);
            obj.push("manufactureDate", manufacture_date);
        } else {
            obj.push("manufactureDate", Json::Null);
        }
        if display.serial.is_empty() {
            obj.push("serial", Json::Null);
        } else {
            obj.push("serial", display.serial.clone());
        }
        obj.push("platformApi", "wayland-mutter");
        arr.push_value(obj);
    }
    JsonOutcome::Result(arr)
}
