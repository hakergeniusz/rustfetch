//! `tpm` module: the TPM version, ported from tpm_linux.c. The version
//! major comes from /sys/class/tpm/tpm0/tpm_version_major ("2" printed as
//! "2.0"); the device description feeds the JSON only.

use std::path::Path;

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

pub struct TpmResult {
    pub version: String,
    pub description: String,
}

/// ffDetectTPM.
pub fn detect() -> Result<TpmResult, &'static str> {
    if !Path::new("/sys/class/tpm/tpm0/").is_dir() {
        if !Path::new("/sys/class/tpm/").is_dir() {
            return Err("TPM is not supported by kernel");
        }
        return Err("TPM device is not found");
    }

    let mut result = TpmResult {
        version: String::new(),
        description: String::new(),
    };
    if let Some(major) = util::read_trimmed("/sys/class/tpm/tpm0/tpm_version_major") {
        result.version = if major == "2" { "2.0".to_string() } else { major };
    }
    result.description =
        util::read_trimmed("/sys/class/tpm/tpm0/device/description").unwrap_or_default();
    Ok(result)
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(tpm) = detect() else {
        // The detection errors are error rows, hidden by default.
        return Vec::new();
    };
    if tpm.version.is_empty() {
        return Vec::new();
    }
    vec![Row::new(instance.display_name("tpm"), tpm.version)]
}

/// ffGenerateTPMJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(tpm) => {
            let mut obj = Json::obj();
            obj.push("version", tpm.version);
            obj.push("description", tpm.description);
            JsonOutcome::Result(obj)
        },
    }
}
