//! `board` module: motherboard name and version from the DMI tables.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(name) = util::read_trimmed("/sys/class/dmi/id/board_name") else {
        return Vec::new();
    };

    let mut value = name;
    if let Some(version) = util::read_trimmed("/sys/class/dmi/id/board_version")
        .filter(|v| !v.eq_ignore_ascii_case("none") && !v.is_empty())
    {
        value.push_str(&format!(" ({version})"));
    }

    vec![Row::new("Board", value)]
}
