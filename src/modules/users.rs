//! `users` module: logged-in users, ported from users_linux.c. utmpx runs
//! first (USER_PROCESS entries; a newer login of the same user replaces the
//! older one); when utmp found nobody, the systemd files under /run/systemd
//! are queried instead (Debian-family systems stopped updating utmp).

use crate::{instance::Instance, modules::Row, util};

struct User {
    name: String,
    host: String,
    login_time_ms: u64,
}

/// ut_user/ut_host are NUL-padded char arrays.
fn c_array_str(bytes: &[libc::c_char]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let bytes: Vec<u8> = bytes[..end].iter().map(|&b| b as u8).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// detectByUtmp: USER_PROCESS entries; a newer login of the same user
/// replaces the older one.
fn detect_by_utmp() -> Vec<User> {
    let mut users: Vec<User> = Vec::new();
    unsafe {
        libc::setutxent();
        loop {
            let entry = libc::getutxent();
            if entry.is_null() {
                break;
            }
            let entry = &*entry;
            if entry.ut_type != libc::USER_PROCESS {
                continue;
            }
            let name = c_array_str(&entry.ut_user);
            let login_time_ms =
                (entry.ut_tv.tv_sec as u64) * 1000 + (entry.ut_tv.tv_usec as u64) / 1000;
            if let Some(user) = users.iter_mut().find(|user| user.name == name) {
                if login_time_ms > user.login_time_ms {
                    user.host = c_array_str(&entry.ut_host);
                    user.login_time_ms = login_time_ms;
                }
            } else {
                users.push(User {
                    name,
                    host: c_array_str(&entry.ut_host),
                    login_time_ms,
                });
            }
        }
        libc::endutxent();
    }
    users
}

/// NAME=, STATE= and REALTIME= out of one /run/systemd/users/<uid> file.
fn detect_user(path: &std::path::Path) -> Option<User> {
    let content = util::read_file(path.to_str()?)?;
    let mut name = None;
    let mut active = false;
    let mut realtime = String::new();
    for (key, value) in util::parse_kv(&content) {
        match key {
            "NAME" => name = Some(value.to_string()),
            "STATE" => active = value == "active",
            "REALTIME" => realtime = value.to_string(),
            _ => {},
        }
    }
    if !active {
        return None;
    }
    // REALTIME= is in microseconds; the ms value drops the last 3 digits.
    let login_time_ms: u64 = realtime
        .get(..realtime.len().saturating_sub(3))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Some(User {
        name: name?,
        host: String::new(),
        login_time_ms,
    })
}

/// The session's host and TTY from /run/systemd/sessions/<session>.
fn fill_session(user: &mut User, session_path: &std::path::Path) {
    let Some(path) = session_path.to_str() else {
        return;
    };
    let Some(content) = util::read_file(path) else {
        return;
    };
    let mut remote_host = String::new();
    let mut tty = String::new();
    let mut service = String::new();
    let mut realtime = String::new();
    for (key, value) in util::parse_kv(&content) {
        match key {
            "REMOTE_HOST" => remote_host = value.to_string(),
            "TTY" => tty = value.to_string(),
            "SERVICE" => service = value.to_string(),
            "REALTIME" => realtime = value.to_string(),
            _ => {},
        }
    }
    if service == "systemd-user" {
        return;
    }
    if !realtime.is_empty() {
        user.login_time_ms = realtime
            .get(..realtime.len().saturating_sub(3))
            .and_then(|s| s.parse().ok())
            .unwrap_or(user.login_time_ms);
    }
    if remote_host.is_empty() {
        user.host = "localhost".to_string();
    } else {
        user.host = remote_host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
    }
    let _ = tty;
}

/// `%F %T` of the login time, like ffTimeToShortStr.
fn login_time_str(msec: u64) -> String {
    if msec == 0 {
        return String::new();
    }
    unsafe {
        let tsec = (msec / 1000) as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&tsec, &mut tm).is_null() {
            return String::new();
        }
        let mut buffer = [0u8; 64];
        let len = libc::strftime(
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            c"%F %T".as_ptr().cast(),
            &tm,
        );
        String::from_utf8_lossy(&buffer[..len]).into_owned()
    }
}

fn detect() -> Option<Vec<User>> {
    let mut users = detect_by_utmp();
    if !users.is_empty() {
        return Some(users);
    }

    let Ok(entries) = std::fs::read_dir("/run/systemd/users") else {
        return Some(users);
    };
    for entry in entries.flatten() {
        // Only regular files; every uid file is one user.
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        if let Some(mut user) = detect_user(&entry.path()) {
            // One session file carries the host of the first session whose
            // service isn't systemd-user.
            if let Ok(sessions) = std::fs::read_dir("/run/systemd/sessions") {
                for session in sessions.flatten() {
                    fill_session(&mut user, &session.path());
                    if !user.host.is_empty() {
                        break;
                    }
                }
            }
            users.push(user);
        }
    }
    Some(users)
}

pub fn run(_instance: &Instance) -> Vec<Row> {
    let Some(users) = detect() else {
        return Vec::new();
    };
    if users.is_empty() {
        // "Unable to detect any users" is an error row, hidden by default.
        return Vec::new();
    }

    
    users
        .iter()
        .enumerate()
        .map(|(index, user)| {
            let index = if users.len() == 1 { 0 } else { index + 1 };
            let key = if index == 0 {
                "Users".to_string()
            } else {
                format!("Users {index}")
            };
            let mut value = user.name.clone();
            if !user.host.is_empty() {
                value.push_str(&format!("@{}", user.host));
            }
            if user.login_time_ms > 0 {
                value.push_str(&format!(" - login time {}", login_time_str(user.login_time_ms)));
            }
            Row::new(&key, value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn trims_microsecond_suffix_from_realtime() {
        // REALTIME= is in microseconds; the port drops the last 3 digits.
        let realtime = "1700000000123456";
        let msec: u64 = realtime
            .get(..realtime.len().saturating_sub(3))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        assert_eq!(msec, 1700000000123);
    }
}
