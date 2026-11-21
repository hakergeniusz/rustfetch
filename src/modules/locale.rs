//! `locale` module: system locale from the environment.

use crate::{
    instance::Instance,
    json::JsonOutcome,
    modules::Row,
};

pub fn run(instance: &Instance) -> Vec<Row> {
    let locale = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "unknown".to_string());

    vec![Row::new(instance.display_name("locale"), locale)]
}

/// ffDetectLocale: LC_ALL, then LANG, then the current LC_TIME locale.
fn detect() -> Result<String, &'static str> {
    for key in ["LC_ALL", "LANG"] {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            return Ok(value);
        }
    }
    // SAFETY: setlocale with a null locale only queries the current one.
    let current = unsafe { libc::setlocale(libc::LC_TIME, std::ptr::null()) };
    if !current.is_null() {
        // SAFETY: the returned string lives in the C locale data.
        let value = unsafe { std::ffi::CStr::from_ptr(current) }.to_string_lossy();
        if !value.is_empty() {
            return Ok(value.into_owned());
        }
    }
    Err("Failed to detect locale")
}

/// ffGenerateLocaleJsonResult.
pub fn json(_instance: &Instance) -> JsonOutcome {
    match detect() {
        Err(error) => JsonOutcome::Error(error.to_string()),
        Ok(locale) => JsonOutcome::Result(locale.into()),
    }
}
