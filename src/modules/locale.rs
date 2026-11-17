//! `locale` module: system locale from the environment.

use crate::{instance::Instance, modules::Row};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let locale = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_else(|_| "unknown".to_string());

    vec![Row::new("Locale", locale)]
}
