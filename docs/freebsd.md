# FreeBSD port

The FreeBSD port is a **standalone crate** at [`platform/freebsd/`](../platform/freebsd/).
It builds its own binary, also named `rustfetch`, and does not depend on the
Linux root crate. The single dependency is `libc = "0.2"`; there are no
crates.io runtime services, no subprocesses, and no `/proc` or `sysinfo`
usage (FreeBSD provides neither).

> **This is not a fastfetch parity build.** Only the root crate ships the
> full fastfetch 2.71.0 parity behavior. This port implements a small set of
> core modules with reduced JSON output and simplified config/gradient support,
> in the same spirit as the [macOS](../platform/macos/) and
> [Windows](../platform/windows/) ports. See
> [platform support](platform-support.md) for the bigger picture.

## Build

### On FreeBSD

Requirements: Rust 1.85+ (edition 2024) and the `libc` crate in the local
cargo cache or reachable from crates.io. A C toolchain is not needed for
`cargo check`; a real build links against the system libc.

```sh
cd platform/freebsd
cargo build --release
./target/release/rustfetch
```

### Cross-check from Linux (no FreeBSD linker needed)

`cargo check` type-checks the FreeBSD code path without linking:

```sh
rustup target add x86_64-unknown-freebsd
cargo --locked check --target x86_64-unknown-freebsd \
  --manifest-path platform/freebsd/Cargo.toml
```

For a full cross-*link*, a FreeBSD sysroot and linker would also be required;
this repository intentionally does not provide one. CI therefore checks the
FreeBSD target and runs native tests/clippy only. On non-FreeBSD hosts, the
`sysctlbyname` call is behind a no-op shim, so the crate still compiles for
native unit tests of pure logic (formatting, presets, JSONC scanner); all
sysctl-backed rows are omitted there.

## Usage

```text
rustfetch                            default layout, red BSD daemon logo
rustfetch --logo none                no logo
rustfetch --logo freebsd|arch|rust   select a built-in ASCII logo
rustfetch --pipe                     no ANSI colors
rustfetch --format json              one simple JSON object, no logo/colors
rustfetch --list-modules             list available modules
rustfetch --hyfetch-preset rainbow   pride-flag logo gradient
rustfetch --help | --version
rustfetch os cpu memory              only these modules
```

`--hyfetch-preset` accepts `rainbow`, `transgender`, `progress` and `random`.
Both `--flag value` and `--flag=value` forms work for the value-taking flags.

### Modules

| ID | Label | Source |
|---|---|---|
| `title` | User@Host | `getpwuid_r` / `$USER` / `$LOGNAME` + `gethostname` |
| `os` | OS | `uname()` (`sysname` + `release`) |
| `host` | Host | `gethostname()` (falls back to `uname().nodename`) |
| `kernel` | Kernel | `sysctl kern.version` (first line), fallback `uname().release` |
| `uptime` | Uptime | `sysctl kern.boottime` + `clock_gettime(CLOCK_REALTIME)` |
| `shell` | Shell | `$SHELL`, fallback `getpwuid_r().pw_shell` |
| `terminal` | Terminal | `$TERM_PROGRAM`, fallback `$TERM` (skips `dumb`) |
| `cpu` | CPU | `sysctl hw.model` + `hw.ncpu`, optional `dev.cpu.0.freq` |
| `memory` | Memory | `sysctl vm.stats.vm.v_page_count` / `hw.physmem`, free/cache/inactive page counters, `sysconf(_SC_PAGESIZE)` |
| `swap` | Swap | `sysctl vm.swap_total` (`None` when no swap is configured) |
| `disk` | Disk | `statfs("/")`: `f_bsize * f_blocks`, available from `f_bavail` |
| `localip` | Local IP | `getifaddrs()` / `freeifaddrs()`, non-loopback AF_INET addresses |
| `packages` | Packages | `/var/db/pkg` (`local.sqlite` for `pkg`, legacy entries otherwise) |
| `locale` | Locale | `$LC_ALL`, fallback `$LANG` |
| `load` | Load | `getloadavg()` (1/5/15 minute averages; available on demand, not in the default layout) |

Aliases: `user`, `kern`, `term`, `mem`, `ip`, `pkg`, `lang`. Detection failures
remove the corresponding row entirely, so an unavailable module never prints a
misleading `Unknown`. With `--logo none` and no detectable modules, output is
simply empty.

### Config

If `~/.config/fastfetch/config.jsonc` exists, a small hand-written JSONC
scanner (no serde) reads the `modules` array and the logo settings:

```jsonc
{
  "logo": {
    "source": "rust",
    "hyfetch": { "preset": "transgender" }
  },
  "modules": ["title", "os", "kernel", "cpu", "memory", "disk", "localip"]
}
```

`modules` may contain strings or `{"type": "..."}` objects. Comments and
trailing commas are tolerated. CLI flags override config values.

## Known limitations

- No fastfetch 2.71.0 parity: no 76-module set, no custom `format` engine,
  no watch mode, no image protocols, no completion scripts.
- Swap reports only the configured total from `vm.swap_total`. FreeBSD does
  not expose a simple used-swap counter via sysctl, so no percentage is shown.
- Package detection only reports that the `pkg` database is present (or counts
  legacy `/var/db/pkg` entries). Counting installed packages needs SQLite or a
  `pkg` subprocess, neither of which is used here.
- Memory "used" is an approximation: free + cached + inactive pages are treated
  as available. It will not exactly match `top(1)` or `vmstat(1)`.
- `dev.cpu.0.freq` is optional; CPU frequency is omitted on systems or kernels
  that do not export it (common on ARM and virtualized guests).
- GPU, display, WM/DE, battery, Wi-Fi, fonts, themes, users, processes and all
  other non-core fastfetch modules are not implemented.
- JSON is a reduced `{"id": "value"}` object, not the fastfetch schema.
- HyFetch colors use the vendored hex values with the same plain-HSL 0.65
  lightness approximation as the macOS port, not the exact OKHSL math.
- No release binary is built or attached by
  [`.github/workflows/freebsd.yml`](../.github/workflows/freebsd.yml). CI only
  type-checks the FreeBSD target and runs native tests/clippy.
