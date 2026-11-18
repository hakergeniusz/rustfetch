//! `tpm` module: the TPM version, ported from tpm_linux.c. The version
//! major comes from /sys/class/tpm/tpm0/tpm_version_major ("2" printed as
//! "2.0"); the device description is ignored by the default print.

use std::path::Path;

use crate::{instance::Instance, modules::Row, util};

fn detect() -> Option<String> {
    if !Path::new("/sys/class/tpm/tpm0").is_dir() {
        return None;
    }
    let major = util::read_trimmed("/sys/class/tpm/tpm0/tpm_version_major")?;
    let version = if major == "2" { "2.0".to_string() } else { major };
    Some(version)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(version) = detect() else {
        // "TPM is not supported by kernel" / "TPM device is not found" are
        // error rows, hidden by default.
        return Vec::new();
    };
    vec![Row::new("TPM", version)]
}
