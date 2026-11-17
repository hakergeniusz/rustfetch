//! `separator` module: dashes spanning the terminal width, like fastfetch.

use crate::{instance::Instance, modules::Row, util};

pub fn run(_instance: &Instance) -> Vec<Row> {
    let width = (util::term_width() as usize).max(40);
    vec![Row::raw("-".repeat(width))]
}
