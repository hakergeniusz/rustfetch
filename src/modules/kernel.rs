//! `kernel` module: kernel type and release.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(release) = util::read_trimmed("/proc/sys/kernel/osrelease") else {
        return Vec::new();
    };
    let kind = util::read_trimmed("/proc/sys/kernel/ostype").unwrap_or_else(|| "Linux".to_string());

    vec![Row::new("Kernel", format!("{kind} {release}"))]
}
