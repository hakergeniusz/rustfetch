//! `version` module: this binary's own version, ported from version.c:
//! "<project> <version> (<architecture>)".

use crate::{instance::Instance, modules::Row};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let version = env!("CARGO_PKG_VERSION");
    // A debug build says so, like fastfetch's "-debug" suffix.
    let debug = if cfg!(debug_assertions) { "-debug" } else { "" };
    let value = format!("rustfetch {version}{debug} ({})", std::env::consts::ARCH);
    vec![Row::new("Version", value)]
}
