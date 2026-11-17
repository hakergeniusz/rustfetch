//! `disk` module: root filesystem usage via statvfs(3).

use crate::{instance::Instance, modules::Row, modules::colorize_percent, util};

/// Filesystem name for the "/" mount from /proc/self/mounts (e.g. btrfs).
fn root_filesystem() -> Option<String> {
    util::read_file("/proc/self/mounts")?.lines().find_map(|line| {
        let fields: Vec<&str> = line.split_whitespace().collect();
        (fields.len() >= 3 && fields[1] == "/").then(|| fields[2].to_string())
    })
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(root) = std::ffi::CString::new("/") else {
        return Vec::new();
    };
    // SAFETY: statvfs only writes into the caller-provided POD buffer.
    let mut fs = unsafe { std::mem::zeroed::<libc::statvfs>() };
    // SAFETY: `root` is a valid NUL-terminated path for the duration of the call.
    if unsafe { libc::statvfs(root.as_ptr(), &mut fs) } != 0 {
        return Vec::new();
    }

    let frsize = fs.f_frsize as u64;
    let total = fs.f_blocks as u64 * frsize;
    // fastfetch's default calcType FREE: used = blocks - free (not just
    // non-root-reserved available), and the percentage is over total.
    let used = fs.f_blocks.saturating_sub(fs.f_bfree) as u64 * frsize;
    let pct = util::percent(used, total).unwrap_or(0);

    let mut value = format!(
        "{} / {} ({})",
        util::fmt_bytes(used),
        util::fmt_bytes(total),
        colorize_percent(pct, instance.color)
    );
    if let Some(fstype) = root_filesystem() {
        value.push_str(&format!(" - {fstype}"));
    }

    vec![Row::new("Disk (/)", value)]
}
