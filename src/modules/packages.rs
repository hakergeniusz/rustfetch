//! `packages` module: installed package counts across package managers,
//! detected from on-disk databases without spawning any process.

use crate::{instance::Instance, modules::Row, util};

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
                .map(|entry| count_subdirs(&entry.path().to_string_lossy()))
                .sum()
        })
        .unwrap_or(0)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let mut any = false;
    let mut parts = Vec::new();

    let mut add = |count: u64, label: &str| {
        if count > 0 {
            any = true;
            parts.push(format!("{count} ({label})"));
        }
    };

    // managers in fastfetch's alphabetical print order
    add(count_prefixed_lines("/var/lib/dpkg/status", "Package: "), "dpkg");

    let flatpak_system = flatpak_app_packages("/var/lib/flatpak")
        + flatpak_runtime_packages("/var/lib/flatpak");
    let flatpak_user = std::env::var("HOME")
        .map(|home| {
            flatpak_app_packages(&format!("{home}/.local/share/flatpak"))
                + flatpak_runtime_packages(&format!("{home}/.local/share/flatpak"))
        })
        .unwrap_or(0);
    if flatpak_user > 0 {
        add(flatpak_system, "flatpak-system");
        add(flatpak_user, "flatpak-user");
    } else {
        add(flatpak_system, "flatpak");
    }

    add(count_subdirs("/var/lib/pacman/local"), "pacman");
    add(count_prefixed_lines("/lib/apk/db/installed", "P:"), "apk");
    add(count_nested_dirs("/var/db/pkg"), "portage");
    add(count_files_with_ext("/var/lib/snapd/snaps", "snap"), "snap");

    if !any {
        return Vec::new();
    }
    vec![Row::new("Packages", parts.join(", "))]
}
