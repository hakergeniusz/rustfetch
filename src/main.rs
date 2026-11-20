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
mod logo_data;
mod logo_entries;
mod modules;
mod networking;
mod platform;
mod render;
mod settings;
mod smbios;
mod util;

use clap::Parser;

fn main() {
    let mut cli = cli::Cli::parse();

    // fastfetch rejects positional arguments during its argv walk; module
    // selection is -s/--structure only. 400 is upstream's exit code (the
    // shell reports it as 400 & 0xFF = 144, like C's exit(400)).
    if let Some(arg) = cli.modules.first() {
        eprintln!("Error: invalid option: {arg}. An option must start with `-`");
        std::process::exit(400);
    }

    // Print/list options exit during the argv walk, before the structure
    // is prepared — `--list-modules` ignores a later `-s command`.
    if cli.list_modules {
        modules::list_modules();
        return;
    }

    let mut structure_given = false;
    if let Some(structure) = &cli.structure {
        // The structure string is colon-separated. Tokens are kept
        // verbatim: known ones select modules, unknown tokens become
        // hidden "<no implementation provided>" rows in text mode and
        // {"type": token, "error": "Unknown module type"} items in JSON
        // mode, and empty tokens are skipped entirely (the
        // structureDisabled gate "contains" them). An empty structure
        // string keeps the default preset, like fastfetch's empty
        // data->structure.
        if !structure.is_empty() {
            structure_given = true;
            cli.modules.extend(structure.split(':').map(str::to_owned));
        }
    }

    // The command module's text only exists in the JSON config, which the
    // structure selection bypasses; fastfetch refuses it up front (481).
    if cli.modules.iter().any(|module| module.eq_ignore_ascii_case("command")) {
        eprintln!("Error: module `command` is not supported with `--structure`");
        eprintln!(
            "       Its options can only be read from the JSON config, which `--structure` bypasses."
        );
        eprintln!(
            "       Add it to the config instead, e.g. `{{ \"modules\": [ {{ \"type\": \"command\", \"text\": \"uname -r\" }} ] }}`"
        );
        std::process::exit(481);
    }

    // The config loads before the option-derived state (Instance), like
    // upstream's two argv passes; errors exit inside the loader.
    let mut config_loaded = false;
    let config = match &cli.config {
        Some(value) => Some(config::load_explicit(value, &mut config_loaded)),
        None => config::load_automatic(config_loaded),
    };

    // `logo: null` disables the logo unless --logo said something (the
    // option pass runs after the config, like upstream).
    if config.as_ref().is_some_and(|config| config.logo_none) && cli.logo.is_none() {
        cli.logo = Some("none".to_string());
    }

    let instance = instance::Instance::new(
        &cli,
        config.as_ref().is_some_and(|config| config.pipe),
        config.as_ref().and_then(|config| config.padding_top),
    );

    if cli.format == cli::Format::Json {
        let items = modules::collect_json(&cli.modules, structure_given, config.as_ref(), &instance);
        render::print_json(&items);
        return;
    }

    let (logo, colors) = logo::load(&cli, &instance);
    let _ = instance.logo_colors.set(colors);
    let rows = modules::collect(&cli.modules, structure_given, config.as_ref(), &instance);
    render::print(&logo, &rows, &cli, &instance);
}
