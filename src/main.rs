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
    if let Some(structure) = &cli.structure {
        // The structure string is colon-separated; empty tokens vanish.
        cli.modules
            .extend(structure.split(':').filter(|s| !s.is_empty()).map(str::to_owned));
    }

    // The command module's text only exists in the JSON config, which the
    // structure selection bypasses; fastfetch refuses it up front (481).
    if cli.modules.iter().any(|module| module.eq_ignore_ascii_case("command")) {
        eprintln!("Error: module `command` is not supported with `--structure`");
        eprintln!("       Its options can only be read from the JSON config, which `--structure` bypasses.");
        eprintln!("       Add it to the config instead, e.g. `{{ \"modules\": [ {{ \"type\": \"command\", \"text\": \"uname -r\" }} ] }}`");
        std::process::exit(481);
    }

    if cli.list_modules {
        modules::list_modules();
        return;
    }

    let instance = instance::Instance::new(&cli);

    if cli.format == cli::Format::Json {
        let rows = modules::collect(&cli.modules, &instance);
        render::print_json(&rows);
        return;
    }

    let (logo, colors) = logo::load(&cli, &instance);
    let _ = instance.logo_colors.set(colors);
    let rows = modules::collect(&cli.modules, &instance);
    render::print(&logo, &rows, &cli, &instance);
}
