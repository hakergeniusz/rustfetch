//! `monitor` module: the same display data as `display`, but with
//! fastfetch's monitor formatting, ported from monitor.c: "2560x1440 px @
//! 179.877 Hz - 600x340 mm (27.15 inches, 108.18 ppi) [HDR Compatible]".

use crate::{
    instance::Instance,
    modules::{display, Row},
};

/// %g of the refresh rate rounded to three decimals (monitor.c).
fn format_refresh(refresh: f64) -> String {
    let rounded = ((refresh * 1000.0 + 0.5) as i64) as f64 / 1000.0;
    let mut text = format!("{rounded:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

fn format_value(width: u32, height: u32, refresh: f64, physical_mm: (u32, u32), hdr: bool) -> String {
    let mut value = format!("{width}x{height} px");
    if refresh > 0.0 {
        value.push_str(&format!(" @ {} Hz", format_refresh(refresh)));
    }
    let inch = f64::hypot(f64::from(physical_mm.0), f64::from(physical_mm.1)) / 25.4;
    if inch > 0.0 {
        let ppi = f64::hypot(f64::from(width), f64::from(height)) / inch;
        value.push_str(&format!(
            " - {}x{} mm ({inch:.2} inches, {ppi:.2} ppi)",
            physical_mm.0, physical_mm.1
        ));
    }
    if hdr {
        value.push_str(" [HDR Compatible]");
    }
    value
}

fn row(name: &str, value: String) -> Row {
    if name.is_empty() {
        Row::new("Monitor", value)
    } else {
        Row::new(&format!("Monitor ({name})"), value)
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    match display::mutter_displays() {
        Some(displays) => {
            let edids = display::edids_by_connector();
            displays
                .iter()
                .map(|display| {
                    let edid = edids
                        .iter()
                        .find(|(connector, _)| *connector == display.connector);
                    // mutterApplyMonitorProperties: color-mode 2 (BT2100) is
                    // HDR enabled and wins over everything; with an EDID the
                    // status is already resolved, only a missing one leaves
                    // the supported list to decide.
                    let hdr = match display.color_mode {
                        Some(2) => true,
                        _ => edid.map_or(display.hdr_supported, |(_, edid)| edid.hdr),
                    };
                    // On GNOME the physical size reaches fastfetch through
                    // wl_output, i.e. the kernel's BDP-based connector mm.
                    let physical_mm = edid.map_or((0, 0), |(_, edid)| edid.bdp_mm);
                    let value =
                        format_value(display.width, display.height, display.refresh, physical_mm, hdr);
                    let name = edid
                        .and_then(|(_, edid)| edid.name.clone())
                        .unwrap_or_else(|| display.model.clone());
                    row(&name, value)
                })
                .collect()
        },
        None => display::edids_by_connector()
            .iter()
            .map(|(_, edid)| {
                let value = format_value(
                    edid.width,
                    edid.height,
                    edid.refresh,
                    edid.physical_mm,
                    edid.hdr,
                );
                let name = edid.name.clone().unwrap_or_default();
                row(&name, value)
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_like_monitor_c() {
        assert_eq!(super::format_refresh(179.877), "179.877");
        assert_eq!(super::format_refresh(60.0), "60");
        assert_eq!(super::format_refresh(59.94), "59.94");
        assert_eq!(
            super::format_value(2560, 1440, 179.877, (600, 340), true),
            "2560x1440 px @ 179.877 Hz - 600x340 mm (27.15 inches, 108.18 ppi) [HDR Compatible]"
        );
        assert_eq!(
            super::format_value(1920, 1080, 0.0, (0, 0), false),
            "1920x1080 px"
        );
    }

    #[test]
    fn finds_hdr_metadata_blocks() {
        let mut edid = vec![0u8; 256];
        // CTA-861 extension: tag 0x02, version 3, DTD offset, and one
        // extended block (tag 0x07) with the HDR Static Metadata DB (6).
        edid[128] = 0x02;
        edid[129] = 0x03;
        edid[130] = 0x04 + 4; // data blocks end after our block
        edid[132] = (0x07 << 5) | 2; // extended block, length 2
        edid[133] = 6; // HDR SMDB
        assert!(super::display::hdr_compatible(&edid));

        // No extension block: too short.
        assert!(!super::display::hdr_compatible(&edid[..128]));
        // Extended tag 5 (video capability) instead of 6/7.
        edid[133] = 5;
        assert!(!super::display::hdr_compatible(&edid));
    }
}
