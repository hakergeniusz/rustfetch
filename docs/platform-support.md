# Platform support

rustfetch is **fully parity-complete on Linux only**. The main crate at the
repository root targets byte-for-byte parity with fastfetch 2.71.0 in pipe,
TTY and JSON modes.

Windows and macOS are served by **two standalone native crates**, not by the
main crate:

| Crate | Path | Dependencies | Binary |
|---|---|---|---|
| Windows | [`platform/windows/`](../platform/windows/) | zero (pure Win32 FFI) | `rustfetch.exe` |
| macOS | [`platform/macos/`](../platform/macos/) | `libc` + `sysctl` | `rustfetch` (universal) |

Both ports ship a reduced, core-module feature set. They are convenience
native builds, **not** parity builds — see [Known limitations](#known-limitations).

---

## Support matrix

| Capability | Linux (main crate) | Windows (native port) | macOS (native port) |
|---|---|---|---|
| Build target | `x86_64-unknown-linux-gnu` (and other Linux targets) | `x86_64-pc-windows-msvc` | universal (`arm64` + `x86_64`) |
| fastfetch 2.71.0 parity | **Full** (pipe, TTY, JSON) | No | No |
| Modules | all 76 modules | 11 core modules | 11 core modules |
| Custom `format` engine | Full (`{}`/`{N}`/`{name}`, conditionals, colors, padding, ranges, …) | Not implemented | Not implemented |
| HyFetch presets & OKHSL | Exact (134 vendored presets) | Basic preset support, not exact | Basic preset support, not exact |
| JSON output | fastfetch-compatible schema | Reduced core schema | Reduced core schema |
| `--pipe` | Yes | Yes | Yes |
| Config files / `--gen-config` / watch mode / completions | Yes | No | No |
| Dependencies | `libc` | zero dependencies | `libc` |

### Supported modules

| Module | Linux | Windows | macOS |
|---|:---:|:---:|:---:|
| `title` | ✅ | ✅ | ✅ |
| `os` | ✅ | ✅ | ✅ |
| `host` | ✅ | ✅ | ✅ |
| `kernel` | ✅ | ✅ | ✅ |
| `uptime` | ✅ | ✅ | ✅ |
| `shell` | ✅ | ✅ | ✅ |
| `terminal` | ✅ | ✅ | ✅ |
| `cpu` | ✅ | ✅ | ✅ |
| `memory` | ✅ | ✅ | ✅ |
| `disk` | ✅ | ✅ | ✅ |
| `localip` | ✅ | ✅ | ✅ |
| all other fastfetch modules (GPU, display, WM/DE, packages, battery, wifi, media, fonts, themes, …) | ✅ | ❌ | ❌ |

Run `rustfetch --list-modules` on the target platform for the authoritative
list.

### Supported flags

| Flag | Linux | Windows | macOS | Notes |
|---|:---:|:---:|:---:|---|
| `--pipe` | ✅ | ✅ | ✅ | plain `key: value` lines |
| `--format json` | ✅ | ✅ | ✅ | reduced JSON schema on the ports |
| `--help` | ✅ | ✅ | ✅ | |
| `--version` | ✅ | ✅ | ✅ | |
| `--list-modules` | ✅ | ✅ | ✅ | |
| `--hyfetch-preset NAME` | ✅ | ✅ | ✅ | basic colors on the ports; not OKHSL-exact |
| module names as positional args | ✅ | ✅ | ✅ | core modules only on the ports |
| config files, `--gen-config`, `--config` | ✅ | ❌ | ❌ | |
| `--logo`, `--small`, image-protocol logos | ✅ | ❌ | ❌ | |
| watch mode, `--hyfetch-mode`, 109 fastfetch flags, zsh completion | ✅ | ❌ | ❌ | |

---

## Build from source

Each port is a standalone crate with its own `Cargo.lock`; build it via its
own `Cargo.toml` (not from the repository root). The commands below are the
ones used by [`.github/workflows/platform.yml`](../.github/workflows/platform.yml)
on native runners.

### Windows (x86_64)

```bash
cargo --locked build --release --manifest-path platform/windows/Cargo.toml
# binary: platform/windows/target/release/rustfetch.exe
```

Run the tests (non-fatal in CI, in case the crate defines no tests yet):

```bash
cargo --locked test --manifest-path platform/windows/Cargo.toml
```

CI stages the binary as `dist/rustfetch-windows-x86_64.exe` and uploads it as
a build artifact. On version-tag pushes (`v*`) the `attach` job adds it to the
release.

### macOS (universal: Apple Silicon + Intel)

Install both Rust std targets, build for each architecture, then combine them
with `lipo`:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin

cargo --locked build --release --target aarch64-apple-darwin --manifest-path platform/macos/Cargo.toml
cargo --locked build --release --target x86_64-apple-darwin --manifest-path platform/macos/Cargo.toml

mkdir -p dist
lipo -create \
  platform/macos/target/aarch64-apple-darwin/release/rustfetch \
  platform/macos/target/x86_64-apple-darwin/release/rustfetch \
  -output dist/rustfetch-macos-universal
chmod +x dist/rustfetch-macos-universal
```

Only Rust std targets are needed — no C toolchain. For a single native
architecture, the short form is:

```bash
cargo --locked build --release --manifest-path platform/macos/Cargo.toml
```

Run the tests (non-fatal in CI):

```bash
cargo --locked test --manifest-path platform/macos/Cargo.toml
```

Release tags attach `dist/rustfetch-macos-universal` to the GitHub release.

### Linux (parity build)

```bash
cargo build --release
# binary: target/release/rustfetch
```

---

## Install

Release assets are attached to version tags at
[github.com/hakergeniusz/rustfetch/releases](https://github.com/hakergeniusz/rustfetch/releases).
Each binary is published with a `.sha256` sidecar; the Windows installer
verifies it when present.

### Windows (PowerShell)

The installer script is [`installers/install.ps1`](../installers/install.ps1).
Run the one-liner in PowerShell:

```powershell
irm https://raw.githubusercontent.com/hakergeniusz/rustfetch/main/installers/install.ps1 | iex
```

It downloads `rustfetch-windows-x86_64.exe` from the latest release and
places `rustfetch.exe` on your `PATH`.

### macOS (Homebrew)

The formula lives in [`installers/rustfetch.rb`](../installers/rustfetch.rb).
Once it is published to the `hakergeniusz/rustfetch` tap:

```bash
brew tap hakergeniusz/rustfetch
brew trust hakergeniusz/rustfetch   # required by newer Homebrew for third-party taps
brew install rustfetch
```

From a checkout you can also install the formula directly:
`brew install ./installers/rustfetch.rb`.

The v0.2.5 release now ships `rustfetch-windows-x86_64.exe` and
`rustfetch-macos-universal` (plus `.sha256` sidecars). The formula installs
**prebuilt** `rustfetch-macos-universal` binary (no
Rust toolchain needed), matching the CI artifact. Its `version`, `url` and
`sha256` must be updated together for each release; the `sha256` field is a
placeholder until the macOS release asset is published. Until the tap is
available, use `brew install ./installers/rustfetch.rb` with the checksum
filled in, download the asset from the release page, or use the
[build commands](#macos-universal-apple-silicon--intel) above.

---

## Known limitations

The native ports intentionally cover only the core use case. They do **not**
provide:

- **No format-engine parity.** The full custom `format` argument engine
  (conditionals, colors, padding, truncation, constants, ranges) exists only
  in the Linux build.
- **No HyFetch OKHSL exactness on the native ports.** `--hyfetch-preset`
  gives basic flag colors, but the exact OKHSL lightness math and
  byte-identical HyFetch 2.1.1 gradient are Linux-only.
- **No byte-for-byte fastfetch parity.** Output on Windows/macOS is not
  compared against fastfetch 2.71.0; only the Linux build is.
- **Reduced JSON schema.** `--format json` on the ports emits only the core
  modules, not fastfetch's full module/JSON surface.
- **Reduced module and flag set:** no GPU/display/wifi/packages/media/etc.
  modules, no config files, `--gen-config`, `--logo`, image logos, watch mode
  or the 109 documented fastfetch flags.
- **No shell completions** outside the Linux zsh completion in `completions/`.

If you need full parity, use the Linux build (native or in a VM/WSL).

---

## Verify locally

After building or installing on the target platform:

1. **Smoke test the CLI surface:**

   ```bash
   rustfetch --version
   rustfetch --help
   rustfetch --list-modules          # expect the 11 core modules
   rustfetch                         # default output
   rustfetch os cpu memory           # positional module selection
   rustfetch --pipe                  # plain key: value lines
   rustfetch --format json           # JSON, one object per module
   rustfetch --hyfetch-preset progress
   ```

2. **Check the architecture of the binary:**
   - Windows: `file rustfetch.exe` (PE32+ x86-64), or check
     `dist/rustfetch-windows-x86_64.exe`.
   - macOS: `lipo -archs dist/rustfetch-macos-universal` should print
     `x86_64 arm64`.

3. **Cross-check the shared core:** compare `rustfetch --pipe` on the port with
   the Linux build for the same 11 modules. Keys and ordering should match;
   values naturally differ by OS.

4. **Run the crate tests:**

   ```bash
   cargo --locked test --manifest-path platform/windows/Cargo.toml   # Windows
   cargo --locked test --manifest-path platform/macos/Cargo.toml     # macOS
   ```

5. **CI check:** push to any branch (or open a PR) and confirm the `Platform
   builds` workflow passes its `windows` and `macos` jobs. Push a `v*` tag to
   confirm the `attach` job uploads `rustfetch-windows-x86_64.exe`,
   `rustfetch-macos-universal` and their `.sha256` sidecars to the release.
   Linux parity checks continue to run in `.github/workflows/ci.yaml`.

6. **CI smoke script:** the workflow runs
   `bash scripts/platform-smoke.sh <binary>` on each staged binary when that
   script is present; run the same command locally for an equivalent check.
