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
mod modules;
mod networking;
mod platform;
mod render;
mod settings;
mod smbios;
mod util;

fn main() {
    let mut cli = cli::Cli::parse();

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

    let instance = instance::Instance::new(
        &cli,
        config.as_ref().is_some_and(|config| config.pipe),
        config.as_ref().and_then(|config| config.padding_top),
    );

    if cli.format_json {
        let items = modules::collect_json(&structure_modules, structure_given, config.as_ref(), &instance);
        render::print_json(&items);
        return;
    }

    let (logo, colors) = logo::load(&cli, &instance);
    let _ = instance.logo_colors.set(colors);
    let rows = modules::collect(&structure_modules, structure_given, config.as_ref(), &instance);
    render::print(&logo, &rows, &instance);
}
