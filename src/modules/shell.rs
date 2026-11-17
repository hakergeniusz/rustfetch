//! `shell` module: $SHELL name and version (version spawns `shell --version`
//! like fastfetch does, only for shells known to support the flag).

use crate::{instance::Instance, modules::Row};

const VERSIONABLE: &[&str] = &[
    "bash", "zsh", "fish", "nu", "nushell", "dash", "ksh", "csh", "tcsh", "elvish", "xonsh", "oil",
    "osh", "pwsh",
];

fn shell_version(path: &str) -> Option<String> {
    let output = std::process::Command::new(path).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let first = crate::util::first_line(&String::from_utf8_lossy(&output.stdout)).to_string();

    // bash and fish print "... version X.Y.Z", zsh/nu print "name X.Y.Z ..."
    if let Some((_, after)) = first.split_once("version") {
        let token = after.split_whitespace().next()?;
        Some(token.trim_end_matches(',').to_string())
    } else {
        Some(first.split_whitespace().nth(1)?.to_string())
    }
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Ok(shell) = std::env::var("SHELL") else {
        return Vec::new();
    };
    let Some(name) = shell.rsplit('/').next().map(str::to_string) else {
        return Vec::new();
    };

    let value = if VERSIONABLE.contains(&name.as_str()) {
        match shell_version(&shell) {
            Some(version) => format!("{name} {version}"),
            None => name,
        }
    } else {
        name
    };

    vec![Row::new("Shell", value)]
}
