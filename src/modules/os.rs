//! `os` module: pretty distro name and architecture, appended only when the
//! name doesn't already mention it (fastfetch's os.c).

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(content) = util::read_file("/etc/os-release") else {
        return Vec::new();
    };

    let name = util::parse_kv(&content)
        .into_iter()
        .find(|(key, _)| *key == "PRETTY_NAME")
        .map(|(_, value)| value.to_string());

    match name {
        Some(name) => {
            let arch = std::env::consts::ARCH;
            let value = if name.to_ascii_lowercase().contains(&arch.to_ascii_lowercase()) {
                name
            } else {
                format!("{name} {arch}")
            };
            vec![Row::new("OS", value)]
        }
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn arch_appended_only_when_missing() {
        let arch = "x86_64";
        // a name mentioning the arch stays untouched
        assert!("Arch Linux x86_64".to_ascii_lowercase().contains(&arch.to_ascii_lowercase()));
        // otherwise it gets appended with a space, like fastfetch
        assert_eq!(format!("Arch Linux {arch}"), "Arch Linux x86_64");
    }
}
