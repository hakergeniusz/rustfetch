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
