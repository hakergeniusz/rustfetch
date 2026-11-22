# rustfetch — native macOS build

A standalone, native **macOS** build of [rustfetch](../..). It is its own Cargo
package and ships its own binary, also named `rustfetch`. The root crate stays
Linux-only and is not modified by this directory.

Visual identity is shared with the Linux build: a colored ASCII logo on the
left, `Key: value` rows on the right, a `user@host` title with a dashed
underline, and hyfetch-style pride-flag gradients.

## Build on macOS

Requirements: a Rust toolchain (edition 2024 → Rust 1.85+) and the macOS SDK
(`xcode-select --install` is enough; a full Xcode install is not needed).

```sh
cd platform/macos
cargo build --release
./target/release/rustfetch
```

The build is **offline-friendly**: the only dependency is `libc = "0.2"`
(already in the local registry cache). No crates.io access is required. The
release profile uses `opt-level = 3`, thin LTO, `panic = "abort"` and strips
symbols.

### Cross-compilation note

Cross-compiling from Linux is **not** supported out of the box and was **not
done here** (this environment has no macOS `std` target installed). From a
macOS host you can build both architectures after installing the targets:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
```

Building macOS binaries from Linux additionally needs an Apple SDK and a
Darwin linker (e.g. `osxcross`) plus the corresponding Rust targets; CI is
expected to cover that. `--version` reports `rustfetch 0.2.5 (aarch64)` or
`(x86_64)` from `std::env::consts::ARCH`.

## Usage

```
rustfetch                          # default layout, apple logo
rustfetch --logo none              # no logo
rustfetch --logo arch              # arch / neofetch / rust / apple
rustfetch --pipe                   # no ANSI colors
rustfetch --format json            # simple JSON object, no logo/colors
rustfetch --list-modules           # module IDs
rustfetch --hyfetch-preset rainbow # pride-flag logo gradient
rustfetch --help | --version
rustfetch os cpu memory            # only these modules
```

### Modules

`title` (`User@Host`), `os`, `kernel`, `uptime`, `shell`, `terminal`, `cpu`,
`memory`, `disk`, `localip`. Aliases: `user`, `kern`, `term`, `mem`, `ip`.

### Config

If `~/.config/fastfetch/config.jsonc` exists, a tiny hand-written scanner
(no serde) reads:

```jsonc
{
  "logo": {
    "source": "arch",
    "hyfetch": { "preset": "transgender" }
  },
  "modules": ["title", "os", "cpu", "memory", "disk"]
}
```

`modules` may contain strings or `{"type": "..."}` objects. CLI flags override
the config values.

## Syscalls used (and verification notes)

| Row | Source |
|---|---|
| User@Host | `getpwuid_r(getuid())` + `gethostname()` |
| OS | `sysctlbyname("kern.osproductversion")`, `kern.osversion` |
| Kernel | `sysctlbyname("kern.osrelease")` |
| Uptime | `sysctlbyname("kern.boottime")` (`timeval`) + `clock_gettime(CLOCK_REALTIME)` |
| Shell / Terminal | `SHELL` / `TERM_PROGRAM` (fallback `TERM`) environment variables |
| CPU | `sysctlbyname("machdep.cpu.brand_string")` → `hw.model`, plus `hw.ncpu` |
| Memory | `sysctlbyname("hw.memsize")` + `host_statistics(HOST_VM_INFO)` (the data behind `vm_stat`), `sysconf(_SC_PAGESIZE)` |
| Disk | `statfs("/")`: `f_bsize × f_blocks`, available from `f_bavail` |
| Local IP | `getifaddrs()` / `freeifaddrs()`, first AF_INET address per non-loopback interface |

`sysctlbyname`, `getpwuid_r`, `getifaddrs`, `statfs`, `clock_gettime` and
`sysconf` are all declared by the cached `libc` crate; `mach_host_self` and
`host_statistics` are declared locally via `extern "C"` (they live in
libSystem). Used memory is `active + inactive + wired` pages, clamped to total.

**Uncertainty:** the code could not be compiled or run in this Linux environment
(the macOS target `std` is absent), so native behavior relies on CI. The two
areas most worth a real-machine check are (1) the `vm_statistics_data_t` layout
used with `host_statistics` — 12 `u32` counters, stable across macOS versions —
and (2) macOS `getifaddrs` flag semantics (`ifa_flags` is `c_uint`; loopback
interfaces are skipped by `IFF_LOOPBACK`). `clock_gettime` requires macOS 10.12+.

## HyFetch lightness

The embedded presets are the **exact vendored hex values** from hyfetch's
`presets.json` (MIT, Copyright (c) 2015-2023 Dylan Araps): `rainbow`,
`transgender` and `progress`; `random` picks one at runtime. Colors are spread
over the logo lines with hyfetch's `with_length` weighting, so gradients look
the same shape as the Linux build.

The Linux build applies hyfetch's exact OKLab/OKHSL 0.65 lightness floor via a
large dependency-free port. To keep this crate small, **this build approximates
that floor with plain HSL lightness clamped to 0.65** (see `lighten()` in
`src/main.rs`). The result is visually close — dark saturated colors are
lifted to the same pastel range — but is **not byte-identical** to the Linux
build. This tradeoff is deliberate and documented here as permitted by the
task requirements.
