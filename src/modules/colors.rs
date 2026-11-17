//! `colors` module: the classic two rows of terminal color blocks.

use crate::instance::Instance;
use crate::modules::Row;

const BLOCK: &str = "███";

fn blocks(range: std::ops::RangeInclusive<u8>) -> String {
    range
        .map(|code| format!("\x1b[{code}m{BLOCK}\x1b[0m"))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn run(instance: &Instance) -> Vec<Row> {
    if !instance.color {
        return Vec::new();
    }
    vec![Row::raw(blocks(31..=36)), Row::raw(blocks(91..=96))]
}
