# Rustfetch

Fastfetch, rewritten in Rust.

A fast, dependency-light system information tool that mirrors fastfetch's
behavior: fastfetch's logo database (534 distro logos with per-distro color
tables), fastfetch-style `$N` logo color placeholders, per-distro key colors,
and a growing set of ported modules. Everything is read directly from procfs,
sysfs, DMI and pci.ids — process spawning only happens for `shell --version`,
DE version detection and GNOME settings.

Current modules: `title`, `separator`, `os`, `host`, `kernel`, `uptime`,
`packages`, `shell`, `display`, `de`, `wm`, `wmtheme`, `theme`, `icons`,
`font`, `cursor`, `terminal`, `cpu`, `gpu`, `memory`, `swap`, `disk`,
`localip`, `locale`, `battery`, `colors`.

Linux only for now.

## Build

```bash
cargo build --release
# binary at target/release/rustfetch
```

Runtime dependencies for the GPU module: `pci.ids` (Arch: `hwdata`,
Debian: `pci.ids`). Display detection on GNOME talks to mutter via `gdbus`.

## Usage

```bash
rustfetch                 # default preset, logo auto-detected from /etc/os-release
rustfetch os cpu gpu      # only these modules
rustfetch --list-modules  # module list
rustfetch --logo arch     # force a logo by distro id (see src/logo/)
rustfetch --logo none     # no logo
rustfetch --small         # small logo variant when available
rustfetch --pipe          # plain key: value lines for scripting
rustfetch --format json   # fastfetch-style JSON, one object per module
```

Colors follow `NO_COLOR` and are disabled automatically when stdout is not a
tty.

## Credits

- [fastfetch](https://github.com/fastfetch-cli/fastfetch) — logo database,
  color tables and module behavior this port follows (MIT).
- [ahaoboy/neofetch](https://github.com/ahaoboy/neofetch) and
  [Macchina-CLI/macchina](https://github.com/Macchina-CLI/macchina) — design
  reference points (both MIT).

## License

MIT — see [LICENSE](LICENSE).
