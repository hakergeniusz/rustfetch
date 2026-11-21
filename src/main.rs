//! rustfetch: fastfetch, rewritten in Rust.

mod cli;
mod config;
mod cpu_tables;
mod cpu_x86;
mod ffont;
mod dbus;
mod format;
mod format_args;
mod genconfig;
mod instance;
mod json;
mod jsonc;
mod hyfetch;
mod hyfetch_presets;
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
    let gen_state = cli::gen_config_state();

    // main()'s guard before the config files load: the watch loop cannot
    // share stdout with the JSON document writer.
    if cli.watch_interval > 0 && (cli.format_json || gen_state.path.is_some()) {
        eprintln!("Error: --dynamic-interval cannot be used with --json");
        std::process::exit(400);
    }

    // fastfetch.c:929 — the gen-config branch of main() runs after both
    // argv passes but before run()/ffStart, so there is no console setup
    // and no module detection for a generated config.
    if genconfig::generate_if_requested(&cli, &gen_state) {
        return;
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
    // ffPrintCommandOption skips disabled tokens before parsing them; the
    // same gate is applied in ffMigrateCommandOptionToJsonc (gen-config).
    if let Some(disabled) = gen_state.structure_disabled.as_deref() {
        structure_modules.retain(|module| {
            !disabled.split(':').any(|name| name.eq_ignore_ascii_case(module))
        });
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
        hyfetch: false,
    };

    let mut logo_color_overrides: Vec<(usize, &'static str)> = config
        .as_ref()
        .map(|config| config.logo_colors.clone())
        .unwrap_or_default();
    for (slot, color) in &cli.logo_colors {
        logo_color_overrides.retain(|(existing, _)| existing != slot);
        logo_color_overrides.push((*slot, color));
    }
    // rustfetch-only HyFetch palette. The config/CLI merge is per-field;
    // explicit logo.color.N / --logo-color-N slots win, then hyfetch fills
    // the remaining nine, then the builtin palette supplies the rest.
    if let Some(hyfetch) = crate::hyfetch::merge(
        cfg.and_then(|config| config.hyfetch.clone()),
        cli.hyfetch_preset.as_deref(),
        cli.hyfetch_mode,
        cli.hyfetch_lightness,
    ) {
        match hyfetch.resolve() {
            Ok(palette) => {
                for (slot, sgr) in palette.into_iter().enumerate() {
                    if !logo_color_overrides.iter().any(|(existing, _)| *existing == slot) {
                        logo_color_overrides.push((slot, sgr.leak()));
                    }
                }
                // A resolved HyFetch palette enables the per-line gradient
                // in the logo parser; without one this stays false and the
                // fastfetch coloring path is unchanged.
                logo_options.hyfetch = true;
            },
            Err(message) => {
                eprintln!("Error: {message}");
                std::process::exit(477);
            },
        }
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
        key_language: cli.key_language.or(cfg.and_then(|config| config.key_language)),
        constants: cfg.and_then(|config| config.constants.clone()).unwrap_or_default(),
    };
    // The FFOptionsDisplay format half: the config sub-objects built one
    // struct in document order; each command-line flag overwrites its own
    // field afterward (upstream's config pass then command-line pass).
    let mut display_format = cfg
        .and_then(|config| config.display_format.clone())
        .unwrap_or_default();
    if let Some(abbreviated) = cli.duration_abbreviation {
        display_format.duration_abbreviation = abbreviated;
    }
    if let Some(space) = cli.duration_space_before_unit {
        display_format.duration_space_before_unit = space;
    }
    if let Some(prefix) = cli.size_binary_prefix {
        display_format.size_binary_prefix = prefix;
    }
    if let Some(ndigits) = cli.size_ndigits {
        display_format.size_ndigits = ndigits as u8;
    }
    if let Some(max_prefix) = cli.size_max_prefix {
        display_format.size_max_prefix = max_prefix;
    }
    if let Some(space) = cli.size_space_before_unit {
        display_format.size_space_before_unit = space;
    }
    if let Some(ndigits) = cli.freq_ndigits {
        display_format.freq_ndigits = ndigits as i8;
    }
    if let Some(space) = cli.freq_space_before_unit {
        display_format.freq_space_before_unit = space;
    }
    if let Some(unit) = cli.temp_unit {
        display_format.temp_unit = unit;
    }
    if let Some(ndigits) = cli.temp_ndigits {
        // The uint8_t cast truncates, upstream has no range check here.
        display_format.temp_ndigits = ndigits as u8;
    }
    if let Some(color) = &cli.temp_color_green {
        display_format.temp_color_green = color.clone();
    }
    if let Some(color) = &cli.temp_color_yellow {
        display_format.temp_color_yellow = color.clone();
    }
    if let Some(color) = &cli.temp_color_red {
        display_format.temp_color_red = color.clone();
    }
    if let Some(space) = cli.temp_space_before_unit {
        display_format.temp_space_before_unit = space;
    }
    if let Some(kind) = cli.percent_type {
        display_format.percent_type = kind as u8;
    }
    if let Some(ndigits) = cli.percent_ndigits {
        display_format.percent_ndigits = ndigits as u8;
    }
    if let Some(color) = &cli.percent_color_green {
        display_format.percent_color_green = color.clone();
    }
    if let Some(color) = &cli.percent_color_yellow {
        display_format.percent_color_yellow = color.clone();
    }
    if let Some(color) = &cli.percent_color_red {
        display_format.percent_color_red = color.clone();
    }
    if let Some(space) = cli.percent_space_before_unit {
        display_format.percent_space_before_unit = space;
    }
    if let Some(width) = cli.percent_width {
        display_format.percent_width = width as u8;
    }
    if let Some(ndigits) = cli.fraction_ndigits {
        display_format.fraction_ndigits = ndigits as i8;
    }
    if let Some(zeros) = cli.fraction_trailing_zeros {
        display_format.fraction_trailing_zeros = zeros;
    }
    if let Some(chars) = &cli.bar_char_elapsed {
        display_format.bar_char_elapsed = chars.clone();
    }
    if let Some(chars) = &cli.bar_char_total {
        display_format.bar_char_total = chars.clone();
    }
    if let Some(border) = &cli.bar_border_left {
        display_format.bar_border_left = border.clone();
    }
    if let Some(border) = &cli.bar_border_right {
        display_format.bar_border_right = border.clone();
    }
    if let Some(border) = &cli.bar_border_left_elapsed {
        display_format.bar_border_left_elapsed = border.clone();
    }
    if let Some(border) = &cli.bar_border_right_elapsed {
        display_format.bar_border_right_elapsed = border.clone();
    }
    if let Some(color) = &cli.bar_color_elapsed {
        display_format.bar_color_elapsed = color.clone();
    }
    if let Some(color) = &cli.bar_color_total {
        display_format.bar_color_total = color.clone();
    }
    if let Some(color) = &cli.bar_color_border {
        display_format.bar_color_border = color.clone();
    }
    if let Some(width) = cli.bar_width {
        display_format.bar_width = width as u8;
    }
    let instance = instance::Instance::new(
        &cli,
        config.as_ref().is_some_and(|config| config.pipe),
        show_errors,
        stat,
        player_name,
        disable_linewrap,
        hide_cursor,
        display,
        display_format,
        logo_options,
        logo_color_overrides.leak(),
    );
    // The signal handler's resetConsole mirrors the pipe-disabled globals.
    LINEWRAP_RESTORE.store(instance.disable_linewrap, std::sync::atomic::Ordering::Relaxed);
    CURSOR_RESTORE.store(instance.hide_cursor, std::sync::atomic::Ordering::Relaxed);

    if cli.format_json {
        // The JSON-config prepare pass parses the prepare-pass modules'
        // args ahead of ffStart, so their color errors abort before the
        // reset goes out (fastfetch.c:792 before :801).
        modules::prepare_pass(&structure_modules, structure_given, config.as_ref(), &instance);
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
