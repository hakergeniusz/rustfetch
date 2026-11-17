//! `disk` module: root filesystem usage via statvfs(3).

use crate::{instance::Instance, modules::colorize_percent, modules::Row, util};

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
    // df-style used space: blocks minus free blocks.
    let used = fs.f_blocks.saturating_sub(fs.f_bfree) as u64 * frsize;
    let available = fs.f_bavail as u64 * frsize;
    let pct = util::percent(used, used + available).unwrap_or(0);

    vec![Row::new(
        "Disk (/)",
        format!(
            "{} / {} ({})",
            util::fmt_bytes(used),
            util::fmt_bytes(total),
            colorize_percent(pct, instance.color)
        ),
    )]
}
