//! `lm` module: login (display) manager, ported from fastfetch's LM module.
//! Detected from running process names, version via a best-effort spawn.

use crate::{instance::Instance, modules::Row, util};

/// Known display managers, in lookup order.
const MANAGERS: &[&str] = &[
    "gdm3", "gdm", "sddm", "lightdm", "ly", "lxdm", "slim", "xdm", "greetd", "lemurs",
];

fn pretty_name(comm: &str) -> &str {
    match comm {
        "gdm3" | "gdm" => "GDM",
        "sddm" => "SDDM",
        "lightdm" => "LightDM",
        "lxdm" => "LXDM",
        "slim" => "SLiM",
        other => other,
    }
}

fn version_of(comm: &str) -> Option<String> {
    let program = match comm {
        "gdm3" => "gdm",
        other => other,
    };
    let output = std::process::Command::new(program).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    util::first_line(&stdout).split_whitespace().next_back().map(str::to_string)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let names = util::process_comms();
    let Some(found) = MANAGERS.iter().find(|name| names.contains(**name)) else {
        return Vec::new();
    };

    let value = match version_of(found) {
        Some(version) => format!("{} {version}", pretty_name(found)),
        None => pretty_name(found).to_string(),
    };
    vec![Row::new("Login Manager", value)]
}
