//! `os` module: the os-release/lsb-release identity, ported from
//! detection/os/os_linux.c. The JSON result is the parsed struct verbatim;
//! the default text output builds the pretty name when PRETTY_NAME is
//! missing (buildOutputDefault) and appends the architecture when absent.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    platform, util,
};

#[derive(Default)]
pub struct OsResult {
    build_id: String,
    codename: String,
    id: String,
    id_like: String,
    name: String,
    pretty_name: String,
    variant: String,
    variant_id: String,
    version: String,
    version_id: String,
}

impl OsResult {
    /// Whether a key was parsed already; ffParsePropFileValues leaves the
    /// first occurrence standing.
    fn set(&mut self, key: &str, value: &str) {
        let target = match key {
            "PRETTY_NAME" => &mut self.pretty_name,
            "NAME" => &mut self.name,
            "ID" => &mut self.id,
            "ID_LIKE" => &mut self.id_like,
            "VARIANT" => &mut self.variant,
            "VARIANT_ID" => &mut self.variant_id,
            "VERSION" => &mut self.version,
            "VERSION_ID" => &mut self.version_id,
            "VERSION_CODENAME" | "CODENAME" => &mut self.codename,
            "BUILD_ID" => &mut self.build_id,
            _ => return,
        };
        if target.is_empty() {
            *target = value.to_string();
        }
    }

    fn parse(&mut self, content: &str, keys: &[&str]) {
        for (key, value) in util::parse_kv(content) {
            if keys.contains(&key) {
                self.set(key, value);
            }
        }
    }
}

/// parseOsRelease's query list.
fn parse_os_release(path: &str, result: &mut OsResult) -> bool {
    match util::read_file(path) {
        Some(content) => {
            result.parse(
                &content,
                &[
                    "PRETTY_NAME",
                    "NAME",
                    "ID",
                    "ID_LIKE",
                    "VARIANT",
                    "VARIANT_ID",
                    "VERSION",
                    "VERSION_ID",
                    "VERSION_CODENAME",
                    "CODENAME",
                    "BUILD_ID",
                ],
            );
            true
        },
        None => false,
    }
}

/// parseLsbRelease maps DISTRIB_* onto different fields.
fn parse_lsb_release(path: &str, result: &mut OsResult) -> bool {
    let Some(content) = util::read_file(path) else {
        return false;
    };
    for (key, value) in util::parse_kv(&content) {
        let target = match key {
            "DISTRIB_ID" => &mut result.id,
            "DISTRIB_DESCRIPTION" => &mut result.pretty_name,
            "DISTRIB_RELEASE" => &mut result.version,
            "DISTRIB_CODENAME" => &mut result.codename,
            _ => continue,
        };
        if target.is_empty() {
            *target = value.to_string();
        }
    }
    true
}

impl OsResult {
    /// detectOS: os-release, then lsb-release for gaps, then
    /// /usr/lib/os-release for more. (Bedrock and the HarmonyOS fallback
    /// need no porting: bedrock installs are not detectable here and the
    /// HarmonyOS branch keys off sysname.)
    pub(crate) fn detect() -> OsResult {
        let mut os = OsResult::default();
        parse_os_release("/etc/os-release", &mut os);
        if os.id.is_empty() || os.version.is_empty() || os.pretty_name.is_empty() || os.codename.is_empty() {
            parse_lsb_release("/etc/lsb-release", &mut os);
        }
        if os.id.is_empty() || os.name.is_empty() || os.pretty_name.is_empty() {
            parse_os_release("/usr/lib/os-release", &mut os);
        }
        apply_distro_quirks(&mut os);
        os
    }

    /// Logo detection consumes the same quirked identity as the OS module,
    /// so a Kubuntu or Fedora Silverblue fixture picks its own logo.
    pub(crate) fn identity(&self) -> (Option<String>, Option<String>, Vec<String>) {
        let id = (!self.id.is_empty()).then(|| self.id.to_lowercase());
        let name = (!self.pretty_name.is_empty()).then(|| self.pretty_name.clone());
        let id_like = self
            .id_like
            .split_whitespace()
            .map(|value| value.to_lowercase())
            .collect();
        (id, name, id_like)
    }
}

/// fastfetch's `ffDetectOSImpl` adjusts a few distro identities after parsing
/// os-release. Port the deterministic parts: Ubuntu's pretty name, Debian's
/// version from `/etc/debian_version`, Fedora Atomic variants, LMDE and Astra.
/// File- and subprocess-based flavour detection on exotic Ubuntu/Debian spins
/// is intentionally not reproduced.
fn apply_distro_quirks(os: &mut OsResult) {
    if os.id == "ubuntu" {
        if !apply_ubuntu_flavour(os) && !os.pretty_name.ends_with(" (development branch)") {
            // VERSION carries the codename on Ubuntu, e.g.
            // "24.04 LTS (Noble Numbat)"; PRETTY_NAME does not.
            os.pretty_name = format!("{} {}", os.name, os.version);
        }
    } else if os.id == "debian" {
        if let Some(content) = util::read_file("/etc/debian_version") {
            let version = content.trim();
            if !version.is_empty() {
                os.version_id = version.to_string();
                os.pretty_name = format!("{} {} ({})", os.name, version, os.codename);
            }
        }
    } else if os.id == "fedora"
        && ["coreos", "kinoite", "sericea", "silverblue"].contains(&os.variant_id.as_str())
    {
        os.id = format!("{}-{}", os.id, os.variant_id);
        os.id_like = "fedora".to_string();
    } else if os.id == "linuxmint" && os.name == "LMDE" {
        os.id = "lmde".to_string();
        os.id_like = "linuxmint".to_string();
    } else if os.id == "astra" {
        if os.version.is_empty() {
            os.version = os.version_id.split('_').next().unwrap_or_default().to_string();
        }
        if !os.version.is_empty() {
            os.pretty_name.push(' ');
            os.pretty_name.push_str(&os.version);
        }
    }
}

/// The env-driven part of fastfetch's `getUbuntuFlavour`. Returns true when
/// the pretty name must be left alone; false means the caller recomputes it
/// with the flavour's NAME.
fn apply_ubuntu_flavour(os: &mut OsResult) -> bool {
    if os.pretty_name.starts_with("Armbian") {
        os.name = "Armbian".to_string();
        os.id_like = std::mem::take(&mut os.id);
        os.id = "armbian".to_string();
        os.version_id.clear();
        let rest = os.pretty_name.trim_start_matches("Armbian").trim_start();
        os.version_id = rest.split_whitespace().next().unwrap_or_default().to_string();
        return true;
    }
    for (prefix, name, id) in [
        ("Linux Lite ", "Linux Lite", "linuxlite"),
        ("Rhino Linux ", "Rhino Linux", "rhinolinux"),
        ("VanillaOS ", "VanillaOS", "vanilla"),
    ] {
        if os.pretty_name.starts_with(prefix) {
            os.name = name.to_string();
            os.id = id.to_string();
            os.id_like = "ubuntu".to_string();
            if prefix != "VanillaOS " {
                os.version_id = os.pretty_name[prefix.len()..].to_string();
            }
            return true;
        }
    }
    if let Some(dirs) = std::env::var_os("XDG_CONFIG_DIRS").and_then(|value| value.into_string().ok()) {
        for (needle, name, id) in [
            ("kde", "Kubuntu", "kubuntu"),
            ("plasma", "Kubuntu", "kubuntu"),
            ("kubuntu", "Kubuntu", "kubuntu"),
            ("xfce", "Xubuntu", "xubuntu"),
            ("xubuntu", "Xubuntu", "xubuntu"),
            ("lxqt", "Lubuntu", "lubuntu"),
            ("lubuntu", "Lubuntu", "lubuntu"),
            ("budgie", "Ubuntu Budgie", "ubuntu-budgie"),
            ("cinnamon", "Ubuntu Cinnamon", "ubuntu-cinnamon"),
            ("mate", "Ubuntu MATE", "ubuntu-mate"),
            ("ukui", "Ubuntu Kylin", "ubuntu-kylin"),
            ("sway", "Ubuntu Sway", "ubuntu-sway"),
            ("touch", "Ubuntu Touch", "ubuntu-touch"),
            ("unity", "Ubuntu Unity", "ubuntu-unity"),
        ] {
            if dirs.contains(needle) {
                os.name = name.to_string();
                os.id = id.to_string();
                os.id_like = "ubuntu".to_string();
                return false;
            }
        }
    }
    false
}

/// buildOutputDefault: the name when PRETTY_NAME is missing.
fn build_output_default(os: &OsResult, sysname: &str) -> String {
    let mut result = if !os.name.is_empty() {
        os.name.clone()
    } else if !os.pretty_name.is_empty() {
        os.pretty_name.clone()
    } else if !os.id.is_empty() {
        os.id.clone()
    } else {
        sysname.to_string()
    };

    let contains_ign_case = |haystack: &str, needle: &str| haystack.to_lowercase().contains(&needle.to_lowercase());

    if !os.codename.is_empty() && !contains_ign_case(&result, &os.codename) {
        result.push(' ');
        result.push_str(&os.codename);
    }
    if !os.version_id.is_empty() && !contains_ign_case(&result, &os.version_id) {
        result.push(' ');
        result.push_str(&os.version_id);
    } else if os.version_id.is_empty() && !os.version.is_empty() && !contains_ign_case(&result, &os.version) {
        result.push(' ');
        result.push_str(&os.version);
    }
    if !os.variant.is_empty() && !contains_ign_case(&result, &os.variant) {
        result.push_str(&format!(" ({})", os.variant));
    } else if os.variant.is_empty() && !os.variant_id.is_empty() && !contains_ign_case(&result, &os.variant_id) {
        result.push_str(&format!(" ({})", os.variant_id));
    }
    result
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let os = OsResult::detect();
    if os.name.is_empty() && os.pretty_name.is_empty() && os.id.is_empty() {
        // "Could not detect OS" is an error row, hidden by default.
        return Vec::new();
    }

    let architecture = &platform::platform().sysinfo.architecture;
    let mut value = if !os.pretty_name.is_empty() {
        os.pretty_name.clone()
    } else {
        build_output_default(&os, &platform::platform().sysinfo.name)
    };
    if !value.to_lowercase().contains(&architecture.to_lowercase()) {
        value.push(' ');
        value.push_str(architecture);
    }
    // os.c prebuilds the custom key against [sysname, name, icon,
    // module-name]; `name` is the raw NAME field, not the composed value
    // printed on the right-hand side.
    let key_args = vec![
        crate::modules::RowKeyArg::text("sysname", platform::platform().sysinfo.name.clone()),
        crate::modules::RowKeyArg::text("name", os.name.clone()),
    ];
    vec![Row::new(instance.display_name("os"), value).with_key_args(key_args)]
}

/// ffGenerateOSJsonResult: all ten fields, empty ones included.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let os = OsResult::detect();
    if os.name.is_empty() && os.pretty_name.is_empty() && os.id.is_empty() {
        return JsonOutcome::Error("Could not detect OS".to_string());
    }
    let mut result = Json::obj();
    result.push("buildID", os.build_id);
    result.push("codename", os.codename);
    result.push("id", os.id);
    result.push("idLike", os.id_like);
    result.push("name", os.name);
    result.push("prettyName", os.pretty_name);
    result.push("variant", os.variant);
    result.push("variantID", os.variant_id);
    result.push("version", os.version);
    result.push("versionID", os.version_id);
    JsonOutcome::Result(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arch_appended_only_when_missing() {
        let arch = "x86_64";
        // a name mentioning the arch stays untouched
        assert!("Arch Linux x86_64".to_ascii_lowercase().contains(&arch.to_ascii_lowercase()));
        // otherwise it gets appended with a space, like fastfetch
        assert_eq!(format!("Arch Linux {arch}"), "Arch Linux x86_64");
    }

    #[test]
    fn build_output_default_appends_missing_parts() {
        let os = OsResult {
            name: "Debian GNU/Linux".into(),
            version_id: "12".into(),
            ..Default::default()
        };
        assert_eq!(build_output_default(&os, "Linux"), "Debian GNU/Linux 12");

        // version_id present but already contained: nothing appended
        let os = OsResult {
            name: "Debian 12".into(),
            version_id: "12".into(),
            ..Default::default()
        };
        assert_eq!(build_output_default(&os, "Linux"), "Debian 12");
    }

    #[test]
    fn first_occurrence_of_a_key_wins() {
        let mut os = OsResult::default();
        os.parse("ID=arch\nID=other\n", &["ID"]);
        assert_eq!(os.id, "arch");
    }

    #[test]
    fn ubuntu_pretty_name_includes_the_codename_from_version() {
        let mut os = OsResult {
            id: "ubuntu".into(),
            name: "Ubuntu".into(),
            version: "24.04 LTS (Noble Numbat)".into(),
            version_id: "24.04".into(),
            pretty_name: "Ubuntu 24.04 LTS".into(),
            ..Default::default()
        };
        apply_distro_quirks(&mut os);
        assert_eq!(os.pretty_name, "Ubuntu 24.04 LTS (Noble Numbat)");
    }

    #[test]
    fn ubuntu_development_branch_keeps_its_pretty_name() {
        let mut os = OsResult {
            id: "ubuntu".into(),
            name: "Ubuntu".into(),
            version: "25.04 (development branch)".into(),
            pretty_name: "Ubuntu 25.04 (development branch)".into(),
            ..Default::default()
        };
        apply_distro_quirks(&mut os);
        assert_eq!(os.pretty_name, "Ubuntu 25.04 (development branch)");
    }

    #[test]
    fn fedora_atomic_variants_extend_the_id() {
        let mut os = OsResult {
            id: "fedora".into(),
            variant_id: "silverblue".into(),
            ..Default::default()
        };
        apply_distro_quirks(&mut os);
        assert_eq!(os.id, "fedora-silverblue");
        assert_eq!(os.id_like, "fedora");
    }
}
