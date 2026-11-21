//! `datetime` module: the local time, ported from fastfetch's datetime.c
//! default format `%F %T` (yyyy-MM-dd HH:mm:ss) through localtime + strftime.

use crate::{
    instance::Instance,
    json::JsonOutcome,
    modules::Row,
    util,
};

pub fn run(instance: &Instance) -> Vec<Row> {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&now, &mut tm).is_null() {
            return Vec::new();
        }
        let mut buffer = [0u8; 32];
        let len =
            libc::strftime(buffer.as_mut_ptr().cast(), buffer.len(), c"%F %T".as_ptr().cast(), &tm);
        if len == 0 {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&buffer[..len]).into_owned();
        vec![Row::new(instance.display_name("datetime"), text)]
    }
}

/// ffGenerateDateTimeJsonResult: ffTimeToFullStr(ffTimeGetNow()).
pub fn json(_instance: &Instance) -> JsonOutcome {
    // ffTimeGetNow: clock_gettime(CLOCK_REALTIME) in milliseconds.
    let mut timespec = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime only writes into timespec.
    if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut timespec) } != 0 {
        return JsonOutcome::Result(util::time_to_full_str(0).into());
    }
    let now = (timespec.tv_sec.max(0) as u64) * 1000 + timespec.tv_nsec as u64 / 1_000_000;
    JsonOutcome::Result(util::time_to_full_str(now).into())
}
