//! rustfetch: fastfetch, rewritten in Rust.

mod cli;
mod dbus;
mod instance;
mod logo;
mod logo_data;
mod modules;
mod render;
mod settings;
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
        // The structure string is colon-separated; empty tokens vanish
        // from the module list (fastfetch turns them into hidden "<no
        // implementation provided>" error rows). An empty structure string
        // keeps the default preset, like fastfetch's empty data->structure.
        if !structure.is_empty() {
            structure_given = true;
            cli.modules
                .extend(structure.split(':').filter(|s| !s.is_empty()).map(str::to_owned));
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

    let instance = instance::Instance::new(&cli);

    if cli.format == cli::Format::Json {
        let rows = modules::collect(&cli.modules, structure_given, &instance);
        render::print_json(&rows);
        return;
    }

    let (logo, colors) = logo::load(&cli, &instance);
    let _ = instance.logo_colors.set(colors);
    let rows = modules::collect(&cli.modules, structure_given, &instance);
    render::print(&logo, &rows, &cli, &instance);
}
