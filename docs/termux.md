# Building rustfetch on Termux (Android)

This guide builds the **main Linux crate** natively on a Termux device
(aarch64 Android). It is not a cross-compile: the device's own `rustc`, `cargo`
and clang do the work, so no NDK, no `cross` and no root are needed. The
reproducible driver is [`scripts/build-termux.sh`](../scripts/build-termux.sh);
the optional [`termux.yml`](../.github/workflows/termux.yml) workflow only
type-checks the Android target in CI.

The desktop parity target is fastfetch **2.71.0**. The Termux device described
here has fastfetch **2.67.1**, so output will differ structurally - see
[Compare with Termux fastfetch](#compare-with-termux-fastfetch).

## Setup

Install Termux from F-Droid or the upstream GitHub releases (the Play Store
build is outdated). Then, inside Termux:

```bash
pkg update && pkg upgrade
pkg install rust clang git
```

`pkg install rust` provides `rustc` and `cargo` (1.100.0 on this device).
`clang` supplies the linker used by rustc on Termux. Verify:

```bash
rustc --version    # rustc 1.100.0
cargo --version
clang --version
```

**`termux-setup-storage` is NOT needed.** rustfetch reads `/proc`, `/sys` and
`$HOME`, and never touches shared/internal storage. Skip it entirely.

Get the source somewhere under `$HOME` (Termux's `$HOME` is
`/data/data/com.termux/files/home`):

```bash
cd "$HOME"
git clone https://github.com/hakergeniusz/rustfetch.git
cd rustfetch
```

You can also just `cd` into an existing checkout; the script finds the repo root
relative to itself.

## Build

From the repository root:

```bash
chmod +x scripts/build-termux.sh
scripts/build-termux.sh
```

The script:

1. Verifies it is running inside Termux (`$PREFIX` looks like Termux) and that
   `cargo` exists.
2. Exports `CARGO_TARGET_DIR=$HOME/.cache/rustfetch-target` and runs exactly:

   ```bash
   cargo --locked build --release
   ```

3. Runs `cargo --locked test --release`.
4. Installs the binary to `$PREFIX/bin/rustfetch`.
5. Prints smoke output (`rustfetch --version`, `rustfetch --pipe`,
   `rustfetch --format json`) and a `diff` against `fastfetch --pipe`.

All build artifacts live under `$HOME/.cache/rustfetch-target`, never into the
checkout's `target/`. Useful flags:

| Flag | Effect |
| --- | --- |
| `-h`, `--help` | Print help and exit. |
| `--no-install` | Keep the binary only in `$HOME/.cache/rustfetch-target/release/`; smoke-test that copy. |
| `--no-test` | Skip the `cargo --locked test --release` step. |

Equivalent manual build without the script:

```bash
export CARGO_TARGET_DIR="$HOME/.cache/rustfetch-target"
mkdir -p "$CARGO_TARGET_DIR"
cargo --locked build --release
cargo --locked test --release
```

## Install

By default the script copies the release binary to `$PREFIX/bin/rustfetch`,
which is already on `PATH` in Termux. Verify:

```bash
rustfetch --version
rustfetch --pipe
rustfetch --format json
```

Manual equivalent:

```bash
install -m 0755 "$HOME/.cache/rustfetch-target/release/rustfetch" \
  "$PREFIX/bin/rustfetch"
```

There is no root/magisk step: `$PREFIX/bin` is owned by the Termux user. With
`--no-install`, skip this and run the binary from the cache path directly.

## Compare with Termux fastfetch

Install the reference tool from Termux packages:

```bash
pkg install fastfetch
```

`scripts/build-termux.sh` does this comparison automatically: it writes
`rustfetch --pipe` and `fastfetch --pipe` to
`$HOME/.cache/rustfetch-termux-smoke/` and runs `diff -u`, printing the first 40
lines of any difference. It also prints a reminder that the device has 2.67.1.

Manual comparison:

```bash
rf="$HOME/.cache/rustfetch-termux-smoke"
mkdir -p "$rf"
rustfetch --pipe   > "$rf/rustfetch.pipe"
fastfetch --pipe   > "$rf/fastfetch.pipe"
diff -u "$rf/fastfetch.pipe" "$rf/rustfetch.pipe" | head -n 40
```

Set `RUSTFETCH_FASTFETCH=/path/to/fastfetch` if you want the script to use a
different binary.

Expect a **non-empty diff**. The 2.67.1 build emits a different module set and
different key names than the 2.71.0 reference this project mirrors, so changed
rows and missing/new fields are normal and are not rustfetch regressions. Focus
on shared keys (e.g. `OS`, `Host`, `Kernel`, `Uptime`, `CPU`, `Memory`, `Shell`)
for a rough sanity check.

## Known Android limitations

Termux runs on the Android kernel (4.4 here) under SELinux and bionic libc, not
a desktop glibc distribution. Several fastfetch data sources are unavailable or
truncated:

- **`/proc` restrictions.** Many `/proc` files are absent, restricted by SELinux,
  or contain Android-specific values. `cpuinfo`/`meminfo`/`uptime` usually work;
  process, mount and network entries can be incomplete.
- **No DMI / SMBIOS / sysfs identity.** `/sys/class/dmi/id` and similar are not
  exposed, so `host`, `bios`, `chassis` and `board` cannot report vendor, model
  or serial. Values fall back to generic or empty strings.
- **No systemd.** Android uses its own init, so `initsystem` and service-related
  rows are not meaningful.
- **Battery, wifi and localip may work partially.** Battery depends on
  `/sys/class/power_supply`; wifi ioctls and netlink queries for `localip`/
  `netio` may be blocked by SELinux or return reduced data. Treat these as
  best-effort.
- **glibc-only APIs are gated for Android.** Desktop code paths using
  `gnu_get_libc_version`, `utmpx`, netlink `rtattr` and glibc-specific ioctl
  request numbers are being compiled out or replaced so the root crate builds
  against bionic. Until that work lands, the build or `cargo check --target
  aarch64-linux-android` will fail on those symbols; modules that depend on them
  show reduced output on device even after it builds.
- **Not a parity target.** Byte-for-byte parity is only claimed for desktop
  Linux with fastfetch 2.71.0. Android output is best-effort.

## Troubleshooting

- **No `/tmp`.** Termux has no `/tmp`. `mktemp` without `-p DIR` and tools that
  hard-code `/tmp` will fail. This script keeps all scratch and build data under
  `$HOME/.cache/` (or you can use `$PREFIX/tmp`). For ad-hoc temp files, use
  `mktemp -d -p "$HOME/.cache"`.
- **System cache dirs are read-only on Android.** Desktop hosts use the house
  rule: persistent cargo caches at `/var/cache/zstd5/rustfetch-target`. Termux
  has no writable `/var/cache` or `/var/tmp`, so builds here necessarily fall
  back to `$HOME/.cache/rustfetch-target`. Do not point `CARGO_TARGET_DIR` at
  either system path; it fails with "permission denied".
- **Root is not required.** The whole flow works as the normal Termux user;
  magisk root is neither used nor needed. Avoid building as root - it can leave
  root-owned files in `$HOME` that later break user builds. If you already
  granted root to Termux, drop it (`su` session exited) before building.
- **`cargo --locked` fails about `Cargo.lock`.** Run the script from the repo
  root and do not edit `Cargo.lock`; `--locked` refuses to update it.
- **Linker errors (`cc`/`clang` not found).** Install the linker:
  `pkg install clang`.
- **Compile errors mentioning `gnu_get_libc_version`, `rtattr`, `utmpx` or
  ioctl request types.** These are the glibc-only APIs currently being gated
  for Android. Pull the latest `main` (or the branch with those changes) and
  rebuild; the CI `termux.yml` check turns green at the same time.
- **Smoke output looks empty.** Some modules legitimately return nothing on
  Android (see [Known Android limitations](#known-android-limitations)).
  `--version`, `--pipe` and `--format json` themselves should still exit 0.

## CI cross-check

[`.github/workflows/termux.yml`](../.github/workflows/termux.yml) runs
`cargo --locked check --target aarch64-linux-android` on every push/PR. This is
a `check` (no linking), so it needs only `rustup target add
aarch64-linux-android` and no Cargo dependencies or NDK. It catches Android
compile regressions such as the glibc-only APIs above; the actual build,
install and fastfetch comparison still happen on the device via
`scripts/build-termux.sh`.
