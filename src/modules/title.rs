//! `title` module: user@host, ported from title.c. Each part is bold and
//! colored with the logo's title color (colorAt defaults to unset, so the
//! @ stays plain); the renderer prints the keyless row's leading reset.
//! The identity fields come from the platform struct (passwd + uname),
//! which ffGenerateTitleJsonResult serializes verbatim.

use crate::{
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    platform,
};

pub fn run(instance: &Instance) -> Vec<Row> {
    // instance.state.titleFqdn: the separator's default length reads it.
    // Like upstream, it survives after the title module ran.
    if let Some(entry) = instance.entry
        && let Some(value) = entry.get("fqdn")
    {
        instance.title_fqdn.set(value.as_bool().unwrap_or(false));
    }
    let platform = platform::platform();
    let user = &platform.user_name;
    // ffStrbufSubstrBeforeFirstC: unless fqdn, the hostname cuts at the
    // first dot; the JSON result keeps the full platform hostname.
    let host = if instance.title_fqdn.get() {
        &platform.host_name[..]
    } else {
        &platform.host_name[..platform.host_name.find('.').unwrap_or(platform.host_name.len())]
    };

    // appendText: the bright bolt (display.brightColor-gated), then the
    // effective title color (the module's colorUser is not set through
    // module options today), then reset — all skipped in pipe mode.
    let color = instance.title_color();
    let wrap = |text: &str| {
        if !instance.color {
            return text.to_string();
        }
        let mut out = String::new();
        if instance.display.bright_color {
            out.push_str("\x1b[1m");
        }
        if let Some(code) = color {
            out.push_str(&format!("\x1b[{code}m"));
        }
        out.push_str(text);
        out.push_str("\x1b[m");
        out
    };

    let value = format!("{}@{}", wrap(user), wrap(host));
    vec![Row::bare(value)]
}

/// ffGenerateTitleJsonResult: the platform identity, in its fixed order.
pub fn json(_instance: &Instance) -> JsonOutcome {
    let platform = platform::platform();
    let mut result = Json::obj();
    result.push("userId", platform.uid as u64);
    result.push("userName", platform.user_name.clone());
    result.push("fullUserName", platform.full_user_name.clone());
    result.push("hostName", platform.host_name.clone());
    result.push("homeDir", platform.home_dir.clone());
    result.push("exePath", platform.exe_path.clone());
    result.push("userShell", platform.user_shell.clone());
    result.push("pid", platform.pid as u64);
    result.push("cwd", platform.cwd.clone());
    JsonOutcome::Result(result)
}
