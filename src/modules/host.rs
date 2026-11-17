//! `host` module: device model from the DMI tables.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(name) = util::read_trimmed("/sys/class/dmi/id/product_name") else {
        return Vec::new();
    };

    // Version strings like "None" or a copy of the name add nothing.
    let version = util::read_trimmed("/sys/class/dmi/id/product_version")
        .filter(|v| !v.eq_ignore_ascii_case("none") && *v != name);

    let value = match version {
        Some(version) => format!("{name} ({version})"),
        None => name,
    };
    vec![Row::new("Host", value)]
}
