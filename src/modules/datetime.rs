//! `datetime` module: the local time, ported from fastfetch's datetime.c
//! default format `%F %T` (yyyy-MM-dd HH:mm:ss) through localtime + strftime.

use crate::{instance::Instance, modules::Row};

pub fn run(_instance: &Instance) -> Vec<Row> {
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
        vec![Row::new("Date & Time", text)]
    }
}
