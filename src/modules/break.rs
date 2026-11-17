//! `break` module: an empty line between rows, like fastfetch's Break.

use crate::{instance::Instance, modules::Row};

pub fn run(_instance: &Instance) -> Vec<Row> {
    vec![Row::raw("")]
}
