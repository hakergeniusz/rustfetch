//! `packages` module: installed package counts across package managers,
//! detected from on-disk databases without spawning any process. The JSON
//! result emits `all` plus every manager count above zero, in
//! ffGeneratePackagesJsonResult's alphabetical field order.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

/// Counts subdirectories of a directory (0 when missing).
fn count_subdirs(path: &str) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
                .count() as u64
        })
        .unwrap_or(0)
}

/// Counts directories two levels deep, e.g. /var/db/pkg/<category>/<package>.
fn count_nested_dirs(path: &str) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
                .map(|entry| count_subdirs(entry.path().to_str().unwrap_or("")))
                .sum()
        })
        .unwrap_or(0)
}

/// Counts files with the given extension (0 when missing).
fn count_files_with_ext(path: &str, ext: &str) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|e| e == ext))
                .count() as u64
        })
        .unwrap_or(0)
}

/// Counts lines starting with a prefix in a database file (0 when missing).
fn count_prefixed_lines(path: &str, prefix: &str) -> u64 {
    util::read_file(path)
        .map(|content| content.lines().filter(|line| line.starts_with(prefix)).count() as u64)
        .unwrap_or(0)
}

/// fastfetch's getFlatpakAppPackages: app dirs with a live `current` link.
fn flatpak_app_packages(base: &str) -> u64 {
    std::fs::read_dir(format!("{base}/app"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|entry| entry.path().join("current").exists())
                .count() as u64
        })
        .unwrap_or(0)
}

/// fastfetch's getFlatpakRuntimePackages: version dirs under
/// runtime/<name>/<arch>/, skipping .Locale/.Debug components.
fn flatpak_runtime_packages(base: &str) -> u64 {
    std::fs::read_dir(format!("{base}/runtime"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    !name.ends_with(".Locale") && !name.ends_with(".Debug")
                })
                .map(|entry| count_nested_dirs(&entry.path().to_string_lossy()))
                .sum()
        })
        .unwrap_or(0)
}

/// The manager counts ffDetectPackages fills; only the managers this port
/// detects are listed, the rest stay 0. `all` is their sum.
#[derive(Default)]
pub struct PackagesResult {
    dpkg: u64,
    flatpak_system: u64,
    flatpak_user: u64,
    pacman: u64,
    apk: u64,
    emerge: u64,
    snap: u64,
    pacman_branch: String,
}

impl PackagesResult {
    /// result->all += ((uint32_t*) result)[i]: the sum of every count.
    fn all(&self) -> u64 {
        self.dpkg
            + self.flatpak_system
            + self.flatpak_user
            + self.pacman
            + self.apk
            + self.emerge
            + self.snap
    }
}

fn detect() -> PackagesResult {
    let mut counts = PackagesResult {
        dpkg: count_prefixed_lines("/var/lib/dpkg/status", "Package: "),
        flatpak_system: flatpak_app_packages("/var/lib/flatpak")
            + flatpak_runtime_packages("/var/lib/flatpak"),
        flatpak_user: std::env::var("HOME")
            .map(|home| {
                flatpak_app_packages(&format!("{home}/.local/share/flatpak"))
                    + flatpak_runtime_packages(&format!("{home}/.local/share/flatpak"))
            })
            .unwrap_or(0),
        pacman: count_subdirs("/var/lib/pacman/local"),
        apk: count_prefixed_lines("/lib/apk/db/installed", "P:"),
        emerge: count_nested_dirs("/var/db/pkg"),
        snap: count_files_with_ext("/var/lib/snapd/snaps", "snap"),
        pacman_branch: String::new(),
    };
    // Manjaro reports its update branch from pacman-mirrors.conf; the file
    // existing without a Branch line means the default branch.
    if let Some(content) = util::read_file("/etc/pacman-mirrors.conf") {
        counts.pacman_branch = content
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once('=')?;
                (key.trim() == "Branch").then(|| value.trim().to_string())
            })
            .unwrap_or_else(|| "stable".to_string());
    }
    counts
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let counts = detect();
    let mut any = false;
    let mut parts = Vec::new();

    let mut add = |count: u64, label: &str| {
        if count > 0 {
            any = true;
            parts.push(format!("{count} ({label})"));
        }
    };

    // managers in fastfetch's alphabetical print order
    add(counts.dpkg, "dpkg");

    if counts.flatpak_user > 0 {
        add(counts.flatpak_system, "flatpak-system");
        add(counts.flatpak_user, "flatpak-user");
    } else {
        add(counts.flatpak_system, "flatpak");
    }

    add(counts.pacman, "pacman");
    add(counts.apk, "apk");
    add(counts.emerge, "portage");
    add(counts.snap, "snap");

    if !any {
        return Vec::new();
    }
    vec![Row::new("Packages", parts.join(", "))]
}

/// ffGeneratePackagesJsonResult: `all` plus each nonzero manager count,
/// alphabetical, plus pacmanBranch when set.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let counts = detect();
    let mut result = Json::obj();
    result.push("all", counts.all());
    let entries = [
        ("apk", counts.apk),
        ("dpkg", counts.dpkg),
        ("emerge", counts.emerge),
        ("flatpakSystem", counts.flatpak_system),
        ("flatpakUser", counts.flatpak_user),
        ("pacman", counts.pacman),
        ("snap", counts.snap),
    ];
    for (name, count) in entries {
        if count > 0 {
            result.push(name, count);
        }
    }
    if !counts.pacman_branch.is_empty() {
        result.push("pacmanBranch", counts.pacman_branch);
    }
    JsonOutcome::Result(result)
}
