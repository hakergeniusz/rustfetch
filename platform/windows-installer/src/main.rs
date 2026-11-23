//! rustfetch-setup - a plain, double-clickable Windows installer for rustfetch.
//!
//! No PowerShell, no execution-policy dance, no administrator rights. The
//! download, hashing and PATH logic live in target-independent modules so the
//! pure parts can be tested on Linux; the WinHTTP/advapi32 FFI is isolated in
//! `win` and cfg-gated to Windows.

// On non-Windows hosts the pure modules are compiled only for `cargo test`;
// their production caller (`win`) is cfg-gated out. CI builds natively on
// Windows, where every item below is used.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

mod cli;
mod env;
mod sha256;
mod urls;

#[cfg(target_os = "windows")]
mod win;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match cli::parse(&args) {
        Ok(opts) => opts,
        Err(message) => {
            eprintln!("[rustfetch-setup] ERROR: {}", message);
            eprintln!("{}", cli::HELP);
            std::process::exit(2);
        }
    };

    if opts.help {
        print!("{}", cli::HELP);
        return;
    }

    #[cfg(target_os = "windows")]
    {
        if let Err(message) = win::run(&opts) {
            eprintln!("[rustfetch-setup] ERROR: {}", message);
            std::process::exit(1);
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Keeps `cargo test` and `cargo clippy` usable on non-Windows hosts.
        eprintln!("[rustfetch-setup] This installer only runs on Windows.");
        std::process::exit(1);
    }
}
