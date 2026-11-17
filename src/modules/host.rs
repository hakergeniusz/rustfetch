//! `host` module: device model from the DMI tables.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(name) = util::read_trimmed("/sys/class/dmi/id/product_name") else {
        return Vec::new();
    };

    // Versions like "None" or a bare revision number ("1.0") add nothing;
    // real versions ("ThinkPad T14 Gen 3") always contain letters.
    let version = util::read_trimmed("/sys/class/dmi/id/product_version")
        .filter(|v| !v.eq_ignore_ascii_case("none") && *v != name)
        .filter(|v| !v.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ' '));

    let value = match version {
        Some(version) => format!("{name} ({version})"),
        None => name,
    };
    vec![Row::new("Host", value)]
}
