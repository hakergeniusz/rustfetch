//! Per-module key icons: the ffOptionInitModuleArg icon literals from
//! fastfetch 2.71.0's module options inits (one per module), keyed by the
//! port's registry name. Printed ahead of the key text when display.keyType
//! carries the ICON bit and the icon is non-empty. os's #ifdef ladder is
//! resolved to the __linux__ branch, matching the reference build.
//! Regenerate by re-extracting the ffOptionInitModuleArg calls from the
//! upstream module sources.

pub(crate) const MODULE_ICONS: &[(&str, &str)] = &[
    ("battery", "\u{f242}"),
    ("bios", "\u{ef70}"),
    ("bluetooth", "\u{f293}"),
    ("bluetoothradio", "\u{f043b}"),
    ("board", "\u{eabe}"),
    ("bootmgr", "\u{ead3}"),
    ("brightness", "\u{f0bea}"),
    ("btrfs", "\u{f145b}"),
    ("camera", "\u{f0100}"),
    ("chassis", "\u{eba3}"),
    ("codec", "\u{f022b}"),
    ("colors", "\u{efcc}"),
    ("command", "\u{f4b5}"),
    ("cpu", "\u{f4bc}"),
    ("cpucache", "\u{f49b}"),
    ("cpuusage", "\u{f04c5}"),
    ("cursor", "\u{f01bf}"),
    ("custom", "\u{f420}"),
    ("datetime", "\u{eab0}"),
    ("de", "\u{e23c}"),
    ("disk", "\u{f0a0}"),
    ("diskio", "\u{f04c5}"),
    ("display", "\u{f0379}"),
    ("dns", "\u{f01d6}"),
    ("editor", "\u{f178e}"),
    ("font", "\u{e659}"),
    ("gamepad", "\u{f0eb5}"),
    ("gpu", "\u{f0fb2}"),
    ("host", "\u{f0322}"),
    ("icons", "\u{efa8}"),
    ("initsystem", "\u{f0fc4}"),
    ("kernel", "\u{f013}"),
    ("keyboard", "\u{f11c}"),
    ("lm", "\u{f09e8}"),
    ("loadavg", "\u{f251}"),
    ("locale", "\u{f1ab}"),
    ("localip", "\u{f0a5f}"),
    ("media", "\u{f001}"),
    ("memory", "\u{efc5}"),
    ("monitor", "\u{f0e51}"),
    ("mouse", "\u{f037d}"),
    ("netio", "\u{f0f86}"),
    ("opencl", "\u{f1ec}"),
    ("opengl", "\u{f03e}"),
    ("os", "\u{f17c}"),
    ("packages", "\u{f03d6}"),
    ("physicaldisk", "\u{f02ca}"),
    ("physicalmemory", "\u{f046d}"),
    ("player", "\u{f0960}"),
    ("poweradapter", "\u{f06a5}"),
    ("processes", "\u{eba2}"),
    ("publicip", "\u{f0a60}"),
    ("shell", "\u{f489}"),
    ("sound", "\u{f028}"),
    ("swap", "\u{f04e1}"),
    ("terminal", "\u{e795}"),
    ("terminalfont", "\u{f031}"),
    ("terminalsize", "\u{f0c8e}"),
    ("terminaltheme", "\u{f050e}"),
    ("theme", "\u{f027c}"),
    ("title", "\u{f015}"),
    ("top", "\u{f035b}"),
    ("tpm", "\u{f023}"),
    ("uptime", "\u{e641}"),
    ("users", "\u{f007}"),
    ("version", "\u{eb78}"),
    ("vulkan", "\u{f03e}"),
    ("wallpaper", "\u{f0e09}"),
    ("weather", "\u{f0599}"),
    ("wifi", "\u{f1eb}"),
    ("wm", "\u{f488}"),
    ("wmtheme", "\u{f04f8}"),
    ("zpool", "\u{f145b}"),
];

/// The module's default key icon; empty when the module has none
/// (separator, break and logo carry no moduleArgs icon upstream).
pub(crate) fn icon_for(name: &str) -> &'static str {
    MODULE_ICONS
        .iter()
        .find(|(module, _)| *module == name)
        .map_or("", |(_, icon)| icon)
}
