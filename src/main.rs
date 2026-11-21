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
mod module_display_names;
mod module_formats;
mod module_icons;
mod modules;
mod networking;
mod platform;
mod render;
mod settings;
mod smbios;
mod image;
mod spin;
mod wcwidth;
mod tty;
mod util;

/// init.c's resetConsole through the exitSignalHandler: SIGINT/SIGTERM/
/// SIGQUIT restore the console and exit(0) — the linewrap/cursor toggles
/// and the alternate screen buffer are the only state to undo. The flags
/// mirror ffStart's pipe-disabled globals; watch mode additionally enters
/// the alternate buffer.
static WATCH_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LINEWRAP_RESTORE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CURSOR_RESTORE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn exit_signal_handler(_signal: libc::c_int) {
    const RESTORE_LINEWRAP: &[u8] = b"\x1b[?7h";
    const RESTORE_CURSOR: &[u8] = b"\x1b[?25h";
    const LEAVE_ALT_BUFFER: &[u8] = b"\x1b[?1049l";
    unsafe {
        if LINEWRAP_RESTORE.load(std::sync::atomic::Ordering::Relaxed) {
            libc::write(
                libc::STDOUT_FILENO,
                RESTORE_LINEWRAP.as_ptr().cast(),
                RESTORE_LINEWRAP.len(),
            );
        }
        if CURSOR_RESTORE.load(std::sync::atomic::Ordering::Relaxed) {
            libc::write(
                libc::STDOUT_FILENO,
                RESTORE_CURSOR.as_ptr().cast(),
                RESTORE_CURSOR.len(),
            );
        }
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
    // one, the automatic configDirs search ran between the two argv passes
    // in cli.rs. (logo: null set the NONE type during the walk, before any
    // -l flag could.)
    let config = cli.config.take();

    // The logo options are one upstream struct mutated by the config pass
    // and then the command line, so the CLI wins per-field. Defaults:
    // padding right 4, printRemaining true (ffOptionsInitLogo).
    let cfg = config.as_ref();
    let mut logo_options = crate::logo::LogoOptions {
        padding_top: cli.logo_padding_top.or(cfg.and_then(|config| config.padding_top)).unwrap_or(0),
        padding_bottom: cli.logo_padding_bottom.or(cfg.and_then(|config| config.padding_bottom)).unwrap_or(0),
        padding_left: cli.logo_padding_left.or(cfg.and_then(|config| config.padding_left)).unwrap_or(0),
        padding_right: cli.logo_padding_right.or(cfg.and_then(|config| config.padding_right)).unwrap_or(4),
        width: cli.logo_width.or(cfg.and_then(|config| config.logo_width)).unwrap_or(0),
        height: cli.logo_height.or(cfg.and_then(|config| config.logo_height)).unwrap_or(0),
        preserve_aspect_ratio: cli
            .logo_preserve_aspect_ratio
            .or(cfg.and_then(|config| config.preserve_aspect_ratio))
            .unwrap_or(false),
        print_remaining: cli.logo_print_remaining.or(cfg.and_then(|config| config.logo_print_remaining)).unwrap_or(true),
        spin_speed_per_frame: 0.0,
        spin_speed_per_round: 0.0,
        spin_tilt: 0.0,
        spin_angle: 0.0,
    };

    let mut logo_color_overrides: Vec<(usize, &'static str)> = config
        .as_ref()
        .map(|config| config.logo_colors.clone())
        .unwrap_or_default();
    for (slot, color) in &cli.logo_colors {
        logo_color_overrides.retain(|(existing, _)| existing != slot);
        logo_color_overrides.push((*slot, color));
    }
    // The spin options are one upstream struct mutated by the config pass
    // and then the command line; 0.0 means unset for the speeds.
    logo_options.spin_speed_per_frame = cli
        .logo_spin_speed_per_frame
        .or(cfg.and_then(|config| config.spin_speed_per_frame))
        .unwrap_or(0.0);
    logo_options.spin_speed_per_round = cli
        .logo_spin_speed_per_round
        .or(cfg.and_then(|config| config.spin_speed_per_round))
        .unwrap_or(0.0);
    logo_options.spin_tilt = cli.logo_spin_tilt.or(cfg.and_then(|config| config.spin_tilt)).unwrap_or(0.0);
    logo_options.spin_angle = cli.logo_spin_angle.or(cfg.and_then(|config| config.spin_angle)).unwrap_or(0.0);
    // CLI --show-errors overrides the config's showErrors/stat implications.
    let show_errors = cli
        .show_errors
        .unwrap_or(cfg.is_some_and(|config| config.show_errors));
    let stat = cli.stat.or(cfg.and_then(|config| config.stat)).unwrap_or(-1);
    let player_name = cli
        .player_name
        .clone()
        .or_else(|| cfg.map(|config| config.player_name.clone()))
        .unwrap_or_default();
    let disable_linewrap = cli
        .disable_linewrap
        .or(cfg.and_then(|config| config.disable_linewrap))
        .unwrap_or(false);
    let hide_cursor = cli
        .hide_cursor
        .or(cfg.and_then(|config| config.hide_cursor))
        .unwrap_or(false);
    // ffOptionsInitDisplay defaults, merged config-then-CLI like the rest
    // of the surfaces: the colors stay empty unless set (the renderer and
    // the PROP parser decide the fallbacks), keyWidth/keyPaddingLeft
    // default 0, keyType defaults to STRING.
    let display = instance::DisplaySurface {
        bright_color: cli.bright_color.or(cfg.and_then(|config| config.bright_color)).unwrap_or(true),
        key_value_separator: cli
            .key_value_separator
            .clone()
            .or_else(|| cfg.and_then(|config| config.key_value_separator.clone()))
            .unwrap_or_else(|| ": ".to_string()),
        color_keys: cli.color_keys.clone().or_else(|| cfg.and_then(|config| config.color_keys.clone())).unwrap_or_default(),
        color_title: cli
            .color_title
            .clone()
            .or_else(|| cfg.and_then(|config| config.color_title.clone()))
            .unwrap_or_default(),
        color_output: cli
            .color_output
            .clone()
            .or_else(|| cfg.and_then(|config| config.color_output.clone()))
            .unwrap_or_default(),
        color_separator: cli
            .color_separator
            .clone()
            .or_else(|| cfg.and_then(|config| config.color_separator.clone()))
            .unwrap_or_default(),
        key_width: cli.key_width.or(cfg.and_then(|config| config.key_width)).unwrap_or(0),
        key_padding_left: cli.key_padding_left.or(cfg.and_then(|config| config.key_padding_left)).unwrap_or(0),
        key_type: cli.key_type.or(cfg.and_then(|config| config.key_type)).unwrap_or(instance::KEY_TYPE_STRING),
    };
    let instance = instance::Instance::new(
        &cli,
        config.as_ref().is_some_and(|config| config.pipe),
        show_errors,
        stat,
        player_name,
        disable_linewrap,
        hide_cursor,
        display,
        logo_options,
        logo_color_overrides.leak(),
    );
    // The signal handler's resetConsole mirrors the pipe-disabled globals.
    LINEWRAP_RESTORE.store(instance.disable_linewrap, std::sync::atomic::Ordering::Relaxed);
    CURSOR_RESTORE.store(instance.hide_cursor, std::sync::atomic::Ordering::Relaxed);

    if cli.format_json {
        // ffStart runs ahead of the JSON document too (fastfetch.c:801):
        // the reset and the pipe-disabled toggles go out first, while
        // ffFinish never does (the resultDoc branch skips it).
        if !instance.pipe {
            print!("\x1b[m");
        }
        if instance.hide_cursor {
            print!("\x1b[?25l");
        }
        if instance.disable_linewrap {
            print!("\x1b[?7l");
        }
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
    let mut next_round = || {
        let mut stats = Vec::new();
        let rows = modules::collect(&structure_modules, structure_given, config.as_ref(), &instance, &mut stats);
        (rows, stats)
    };
    render::print(&logo, &instance, watch_interval, &mut next_round);
}
