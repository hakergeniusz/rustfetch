//! `title` module: user@host, ported from title.c. Each part is bold and
//! colored with the logo's title color (colorAt defaults to unset, so the
//! @ stays plain); the renderer prints the keyless row's leading reset.

use crate::{instance::Instance, modules::Row, util};

pub fn run(instance: &Instance) -> Vec<Row> {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    let host = util::read_trimmed("/etc/hostname").unwrap_or_else(|| "unknown".to_string());

    // appendText: bold, then the logo title color, then reset.
    let color = instance.logo_colors.get().and_then(|colors| colors.title);
    let wrap = |text: &str| {
        if !instance.color {
            return text.to_string();
        }
        let mut out = String::from("\x1b[1m");
        if let Some(code) = color {
            out.push_str(&format!("\x1b[{code}m"));
        }
        out.push_str(text);
        out.push_str("\x1b[m");
        out
    };

    let value = format!("{}@{}", wrap(&user), wrap(&host));
    vec![Row::bare(value)]
}
