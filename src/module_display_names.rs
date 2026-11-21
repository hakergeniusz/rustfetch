//! Per-module display names: the `.en` entries of fastfetch 2.71.0's
//! FFModuleInfo displayName tables (one per module), keyed by the port's
//! registry name. This is the text ffPrintLogoAndKey prints for a row
//! without a custom key (plus " %u" when the module indexes its rows) and
//! the `{module-name}` format argument of custom keys. Commit-wise this
//! table is also the anchor for --key-language's i18n variants.
//! Regenerate by re-extracting the `.displayName` blocks from the upstream
//! module sources.

/// The display name, in upstream registration order.
pub(crate) const MODULE_DISPLAY_NAMES: &[(&str, &str)] = &[
    ("battery", "Battery"),
    ("bios", "BIOS"),
    ("bluetooth", "Bluetooth"),
    ("bluetoothradio", "Bluetooth Radio"),
    ("board", "Board"),
    ("bootmgr", "Boot Manager"),
    ("break", "Break"),
    ("brightness", "Brightness"),
    ("btrfs", "BTRFS"),
    ("camera", "Camera"),
    ("chassis", "Chassis"),
    ("codec", "Codec"),
    ("colors", "Colors"),
    ("command", "Command"),
    ("cpu", "CPU"),
    ("cpucache", "CPU Cache"),
    ("cpuusage", "CPU Usage"),
    ("cursor", "Cursor"),
    ("custom", "Custom"),
    ("datetime", "Date & Time"),
    ("de", "Desktop Environment"),
    ("disk", "Disk"),
    ("diskio", "Disk I/O"),
    ("display", "Display"),
    ("dns", "DNS"),
    ("editor", "Editor"),
    ("font", "Font"),
    ("gamepad", "Gamepad"),
    ("gpu", "GPU"),
    ("host", "Host"),
    ("icons", "Icons"),
    ("initsystem", "Init System"),
    ("kernel", "Kernel"),
    ("keyboard", "Keyboard"),
    ("lm", "Login Manager"),
    ("loadavg", "Load Average"),
    ("locale", "Locale"),
    ("localip", "Local IP"),
    ("logo", "Logo"),
    ("media", "Media"),
    ("memory", "Memory"),
    ("monitor", "Monitor"),
    ("mouse", "Mouse"),
    ("netio", "Network I/O"),
    ("opencl", "OpenCL"),
    ("opengl", "OpenGL"),
    ("os", "OS"),
    ("packages", "Packages"),
    ("physicaldisk", "Physical Disk"),
    ("physicalmemory", "Physical Memory"),
    ("player", "Media Player"),
    ("poweradapter", "Power Adapter"),
    ("processes", "Processes"),
    ("publicip", "Public IP"),
    ("separator", "Separator"),
    ("shell", "Shell"),
    ("sound", "Sound"),
    ("swap", "Swap"),
    ("terminal", "Terminal"),
    ("terminalfont", "Terminal Font"),
    ("terminalsize", "Terminal Size"),
    ("terminaltheme", "Terminal Theme"),
    ("theme", "Theme"),
    ("title", "Title"),
    ("top", "Top Processes"),
    ("tpm", "TPM"),
    ("uptime", "Uptime"),
    ("users", "Users"),
    ("version", "Version"),
    ("vulkan", "Vulkan"),
    ("wallpaper", "Wallpaper"),
    ("weather", "Weather"),
    ("wifi", "Wi-Fi"),
    ("wm", "Window Manager"),
    ("wmtheme", "WM Theme"),
    ("zpool", "Zpool"),
];

pub(crate) fn display_name_for(name: &str) -> &'static str {
    MODULE_DISPLAY_NAMES
        .iter()
        .find(|(module, _)| *module == name)
        .map_or("", |(_, display)| display)
}
