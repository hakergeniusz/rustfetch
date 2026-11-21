//! `break` module: an empty line between rows, like fastfetch's Break.
//! Its JSON object carries no options — ffParseBreakJsonObject skips
//! type/condition and errors on every other key (ffPrintError with null
//! moduleArgs, so the key stays "Break", showErrors-gated).

use crate::{instance::Instance, modules::Row};

pub fn run(instance: &Instance) -> Vec<Row> {
    let mut rows = Vec::new();
    if let Some(crate::jsonc::Value::Object(entries)) = instance.entry {
        for (key, _value) in entries {
            if key == "type" || key == "condition" {
                continue;
            }
            rows.push(Row::error(instance.display_name("break"), format!("Unknown JSON key {key}")));
        }
    }
    rows.push(Row::raw(""));
    rows
}
