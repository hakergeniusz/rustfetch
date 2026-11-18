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
    let cli = cli::Cli::parse();

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

    let logo = logo::load(&cli, &instance);
    let rows = modules::collect(&cli.modules, &instance);
    render::print(&logo, &rows, &cli, &instance);
}
