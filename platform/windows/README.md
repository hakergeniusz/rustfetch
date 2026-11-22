# rustfetch — Windows build

Native Windows port of [rustfetch](../../../README.md) 0.2.5. This is a
**standalone crate**: it builds its own binary also named `rustfetch` and does
not depend on the Linux crate. It uses **zero crates.io dependencies** — every
Win32 call is declared locally with `extern "system"` blocks.

## Build

On Windows (MSVC or GNU toolchain):

```powershell
cargo build --release --manifest-path platform/windows/Cargo.toml
```

Cross-compiling from Linux, if the target std is installed:

```bash
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu --manifest-path platform/windows/Cargo.toml
```

The binary is written to `platform/windows/target/<triple>/release/rustfetch.exe`
(or `target/release/rustfetch` when built from inside the crate directory).
It links only system libraries: `kernel32`, `advapi32`, `ntdll`, `ws2_32` — no
external DLLs need to be shipped.

## Usage

```text
rustfetch                          colored ASCII logo + Key: value rows
rustfetch --logo none              no logo
rustfetch --logo generic           pick a built-in logo (windows, generic)
rustfetch --hyfetch-preset rainbow color the logo with a pride-flag gradient
rustfetch --pipe                   no ANSI escapes (for scripts)
rustfetch --format json            one JSON object
rustfetch --list-modules           valid module names
rustfetch --version                rustfetch 0.2.5 (x86_64)
rustfetch --help
```

`--hyfetch-preset` accepts `rainbow`, `transgender`, `progress`, `random` and
several extras (`nonbinary`, `bisexual`, `pansexual`, `agender`, `lesbian`,
`asexual`, `aromantic`). Both `--flag value` and `--flag=value` work.

A config file is read from `%APPDATA%\fastfetch\config.jsonc` when present
(override with `--config PATH`). Comments and trailing commas are tolerated;
a minimal hand-written scanner (no serde) understands just two things:

```jsonc
{
  "logo": { "hyfetch": { "preset": "transgender" } },
  "modules": ["title", "os", "kernel", "uptime", "cpu", "memory", "disk", "localip"]
}
```

Command-line flags win over the config file.

## Implementation notes / choices

- **Colors:** ANSI is enabled at startup with `SetConsoleMode` +
  `ENABLE_VIRTUAL_TERMINAL_PROCESSING` (0x0004). Colors are suppressed for
  `--pipe`, `--format json`, `NO_COLOR`, and when stdout is redirected
  (GetConsoleMode fails). No escapes are emitted in those modes.
- **HyFetch colors:** the vendored hex values are used **verbatim** as RGB
  (e.g. transgender `#55CDFD → #F6AAB7 → #FFFFFF → #F6AAB7 → #55CDFD`).
  The upstream OKHSL re-lightening to lightness 0.65 is deliberately skipped:
  it would require a full color-science implementation for no visual gain on
  these already-pale palettes. Each logo line gets one exact palette color,
  spread evenly across all lines (floor-interpolated indices).
- **OS / Kernel:** `RtlGetVersion` (ntdll) is preferred because `GetVersionExW`
  can report a downgraded version without a manifest; `GetVersionExW` is the
  fallback. Build number comes from the same structure.
- **CPU:** name and ~MHz from the registry
  (`HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0`); core count from
  `GetSystemInfo`.
- **GPU:** optional; reads `DriverDesc` from the display-adapter class registry
  key. The row is omitted when no adapter is found.
- **Local IP:** a UDP `connect()` (8.8.8.8:53 — no packets are sent) plus
  `getsockname()` selects the local address, avoiding the fragile
  `IP_ADAPTER_INFO` linked-list layout. Falls back to `127.0.0.1`.
- **Disk:** one row per fixed drive (bitmask from `GetLogicalDrives`, sizes from
  `GetDiskFreeSpaceExW`).

## Uncertainty

Compilation could not be verified locally (the Windows std target is not
installed here). FFI declarations follow the MSDN ABI; the only delicate
point is the x86/x64 pointer-sized layout of `SYSTEM_INFO` and `HANDLE`, which
are represented as pointer-sized Rust types so they match either target.

## License

MIT.
