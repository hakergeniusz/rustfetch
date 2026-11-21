//! rustfetch: fastfetch, rewritten in Rust.

mod cli;
mod config;
mod cpu_tables;
mod cpu_x86;
mod ffont;
mod dbus;
mod instance;
mod json;
mod jsonc;
mod logo;
mod logo_entries;
mod module_formats;
mod modules;
mod networking;
mod platform;
mod render;
mod settings;
mod smbios;
mod tty;
mod util;

/// init.c's resetConsole through the exitSignalHandler: SIGINT/SIGTERM/
/// SIGQUIT restore the console and exit(0). The alternate screen buffer
/// is the only state the port leaves behind, and only watch mode enters
/// it (the hide-cursor escape is not ported).
static WATCH_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn exit_signal_handler(_signal: libc::c_int) {
    const LEAVE_ALT_BUFFER: &[u8] = b"\x1b[?1049l";
    unsafe {
        if WATCH_ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
            libc::write(
                libc::STDOUT_FILENO,
                LEAVE_ALT_BUFFER.as_ptr().cast(),
                LEAVE_ALT_BUFFER.len(),
            );
        }
        libc::exit(0);
    }
}

fn main() {
    let mut cli = cli::Cli::parse();

    // main()'s guard before the config files load: the watch loop cannot
    // share stdout with the JSON document writer.
    if cli.watch_interval > 0 && cli.format_json {
        eprintln!("Error: --dynamic-interval cannot be used with --json");
        std::process::exit(400);
    }

    // ffStart installs these for every run; without a watch interval the
    // handler's reset is empty and the exit code stays exit(0).
    let watch_interval = if cli.watch_interval > 0 {
        WATCH_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
        Some(cli.watch_interval)
    } else {
        None
    };
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = exit_signal_handler as *const () as usize;
        libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut());
        libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut());
        libc::sigaction(libc::SIGQUIT, &action, std::ptr::null_mut());
    }

    // The structure string is colon-separated. Tokens are kept
    // verbatim: known ones select modules, unknown tokens become
    // hidden "<no implementation provided>" rows in text mode and
    // {"type": token, "error": "Unknown module type"} items in JSON
    // mode, and empty tokens are skipped entirely (the
    // structureDisabled gate "contains" them). An empty structure
    // string keeps the default preset, like fastfetch's empty
    // data->structure.
    let mut structure_given = false;
    let mut structure_modules: Vec<String> = Vec::new();
    if let Some(structure) = &cli.structure
        && !structure.is_empty()
    {
        structure_given = true;
        structure_modules.extend(structure.split(':').map(str::to_owned));
    }

    // The command module's text only exists in the JSON config, which the
    // structure selection bypasses; fastfetch refuses it up front (481).
    if structure_modules.iter().any(|module| module.eq_ignore_ascii_case("command")) {
        eprintln!("Error: module `command` is not supported with `--structure`");
        eprintln!(
            "       Its options can only be read from the JSON config, which `--structure` bypasses."
        );
        eprintln!(
            "       Add it to the config instead, e.g. `{{ \"modules\": [ {{ \"type\": \"command\", \"text\": \"uname -r\" }} ] }}`"
        );
        std::process::exit(481);
    }

    // The explicit config already loaded during the option walk; without
    // one, the automatic configDirs search runs here. (logo: null set the
    // NONE type during the walk, before any -l flag could.)
    let config = match cli.config.take() {
        Some(config) => Some(config),
        None => config::load_automatic(cli.config_loaded),
    };

    // The logo options are one upstream struct mutated by the config pass
    // and then the command line, so the CLI wins per-field. Defaults:
    // padding right 4, printRemaining true (ffOptionsInitLogo).
    let cfg = config.as_ref();
    let logo_options = crate::logo::LogoOptions {
        padding_top: cli.logo_padding_top.or(cfg.and_then(|config| config.padding_top)).unwrap_or(0),
        padding_bottom: cli.logo_padding_bottom.or(cfg.and_then(|config| config.padding_bottom)).unwrap_or(0),
        padding_left: cli.logo_padding_left.or(cfg.and_then(|config| config.padding_left)).unwrap_or(0),
        padding_right: cli.logo_padding_right.or(cfg.and_then(|config| config.padding_right)).unwrap_or(4),
        width: cli.logo_width.or(cfg.and_then(|config| config.logo_width)).unwrap_or(0),
        height: cli.logo_height.or(cfg.and_then(|config| config.logo_height)).unwrap_or(0),
        print_remaining: cli.logo_print_remaining.or(cfg.and_then(|config| config.logo_print_remaining)).unwrap_or(true),
    };

    let mut logo_color_overrides: Vec<(usize, &'static str)> = config
        .as_ref()
        .map(|config| config.logo_colors.clone())
        .unwrap_or_default();
    for (slot, color) in &cli.logo_colors {
        logo_color_overrides.retain(|(existing, _)| existing != slot);
        logo_color_overrides.push((*slot, color));
    }
    let instance = instance::Instance::new(
        &cli,
        config.as_ref().is_some_and(|config| config.pipe),
        logo_options,
        logo_color_overrides.leak(),
    );

    if cli.format_json {
        let items = modules::collect_json(&structure_modules, structure_given, config.as_ref(), &instance);
        render::print_json(&items);
        return;
    }

    let (logo, colors) = logo::load(&cli, config.as_ref(), &instance);
    let _ = instance.logo_colors.set(colors);
    // run()'s per-round ffPrintCommandOption: each round re-collects the
    // rows, which re-reads the print-time detections (uptime, load, usage,
    // temperatures); the once-per-process results stay cached, like
    // upstream's instance.state results surviving ffCacheInvalidateAll.
    let mut next_round = || modules::collect(&structure_modules, structure_given, config.as_ref(), &instance);
    render::print(&logo, &instance, watch_interval, &mut next_round);
}
