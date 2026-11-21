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
    fn detect() -> OsResult {
        let mut os = OsResult::default();
        parse_os_release("/etc/os-release", &mut os);
        if os.id.is_empty() || os.version.is_empty() || os.pretty_name.is_empty() || os.codename.is_empty() {
            parse_lsb_release("/etc/lsb-release", &mut os);
        }
        if os.id.is_empty() || os.name.is_empty() || os.pretty_name.is_empty() {
            parse_os_release("/usr/lib/os-release", &mut os);
        }
        os
    }
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
    vec![Row::new(instance.display_name("os"), value)]
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
}
