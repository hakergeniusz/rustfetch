# Rustfetch

A fastfetch- and hyfetch-inspired system information tool written in Rust.

A dependency-light Rust rewrite that mirrors fastfetch's
behavior: fastfetch's logo database (534 distro logos with per-distro color
tables), fastfetch-style `$N` logo color placeholders, per-distro key colors,
and all 76 fastfetch modules ported — from `os` and `gpu` to `wifi`
(nl80211), `sound` (the PulseAudio native protocol and the ALSA control
ioctls) and `weather` (wttr.in). Everything is read directly from procfs,
sysfs, DMI and pci.ids; process spawning only happens for per-terminal and
DE version detection and GNOME settings.

Output is verified byte-for-byte against fastfetch 2.71.0 on Arch Linux in
both pipe and TTY modes.

A fastfetch `~/.config/fastfetch/config.jsonc` with a `modules` array is
honored as the preset, so existing fastfetch configs keep working (unknown
module names are skipped).

Linux only.

The default preset mirrors fastfetch's default output; extra modules run by
name, e.g. `rustfetch bios physicaldisk lm`.

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

## Benchmarks

hyperfine, default module set, same machine (Arch Linux, ghostty):

| | fastfetch 2.71.0 | rustfetch 0.2.1 |
|---|---|---|
| wall time | 10.8 ms ± 1.4 ms | **5.8 ms ± 0.7 ms** |
| peak memory | 12.2 MiB | **10.7 MiB** |

## Credits

- [fastfetch](https://github.com/fastfetch-cli/fastfetch) — logo database,
  color tables and module behavior this port follows (MIT).
- [hyfetch](https://github.com/hykilpikonna/hyfetch) — inspiration for the
  per-line gradient logo coloring (MIT).
- [ahaoboy/neofetch](https://github.com/ahaoboy/neofetch) and
  [Macchina-CLI/macchina](https://github.com/Macchina-CLI/macchina) — design
  reference points (both MIT).

## License

MIT — see [LICENSE](LICENSE).

## Shell completion

A zsh completion for all fastfetch-compatible flags, enum values, module names, logo names and the 134 HyFetch presets ships at `completions/_rustfetch`. `install.sh` and the distro packages place it in `share/zsh/site-functions`; add that directory to your `$fpath` if you install manually.
