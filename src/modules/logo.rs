//! `logo` module: port of modules/logo/logo.c, whose text-mode printer
//! always reports "Supported in JSON format only" — the logo itself is
//! rendered outside the module system (see crate::logo). The error row is
//! hidden by default, so selecting the module prints just the logo, like
//! fastfetch.

use crate::{instance::Instance, modules::Row};

pub fn run(_instance: &Instance) -> Vec<Row> {
    // "Supported in JSON format only" is an error row, hidden by default.
    Vec::new()
}
