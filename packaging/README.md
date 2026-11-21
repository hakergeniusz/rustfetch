# Packaging rustfetch

`rustfetch` is a single self-contained Rust binary. At runtime it needs only
the standard system libraries (`libc`, `libm`, `libgcc_s`). Logos, distro data
and module code are compiled into the binary with `include_str!` — there is
**no runtime dependency on fastfetch and no runtime dependency on any
fastfetch package**. fastfetch is not detected, required, or invoked.

All packaging defaults to building from the repository root (`/home/.../rustfetch`).
To limit SSD writes, the helper scripts stage under `/dev/shm/rf-scratch-pkg/`
and write only the final artifact into `dist/`. Set `CARGO_TARGET_DIR` to a
tmpfs path (e.g. `/dev/shm/rustfetch-target`) when building yourself.

## Path overview

| Path | Purpose |
|---|---|
| `install.sh` (repo root) | Universal installer/uninstaller for all major distros. |
| `PKGBUILD` (repo root) | Arch source-build PKGBUILD (`cargo build --release`). |
| `packaging/README.md` | This document. |
| `packaging/debian/` | Debian/Ubuntu source-package metadata (`control`, `rules`, `changelog`, `compat`, `source/format`). |
| `packaging/build-deb.sh` | Build a `.deb` with only `dpkg-deb` (no `cargo-deb`, no debhelper). |
| `packaging/rustfetch.spec` | Fedora/RHEL RPM spec (source build with Cargo). |
| `packaging/build-rpm.sh` | Wrapper around `rpmbuild -ba` with a configurable topdir. |
| `packaging/bin/PKGBUILD` | Arch PKGBUILD that installs a prebuilt `.tar.zst` release asset. |
| `packaging/build-tarball.sh` | Produce `rustfetch-0.1.0-x86_64-unknown-linux-gnu.tar.zst` (binary + LICENSE + README). |

## Prerequisites per distro

| Distro | Build tools | Package tools |
|---|---|---|
| Debian / Ubuntu | `sudo apt install -y cargo rustc` | `sudo apt install -y dpkg-dev debhelper` (for `.deb`) |
| Fedora / RHEL / Rocky / Alma | `sudo dnf install -y cargo rustc` | `sudo dnf install -y rpm-build` (for `.rpm`) |
| Arch / Manjaro | `sudo pacman -S --needed rust` | `base-devel` (for `makepkg`) |
| Alpine | `sudo apk add cargo rust` | — |
| openSUSE | `sudo zypper install -y cargo rustc` | `sudo zypper install -y rpm-build` (for `.rpm`) |

`install.sh` prints the matching hint if `cargo`/`rustc` are missing. It never
installs packages and never runs `sudo` for you.

## 1. Universal installer (`install.sh`)

Run from any directory; the script resolves the repo root from its own path.

```bash
# Build and install for the current user (default for non-root: ~/.local)
./install.sh

# Root install into /usr/local (default for root)
sudo ./install.sh

# Install into a custom prefix -> <prefix>/bin/rustfetch
./install.sh --prefix /opt/rustfetch

# Force a user install
./install.sh --user

# Install an already-built binary without rebuilding
CARGO_TARGET_DIR=/dev/shm/rustfetch-target ./install.sh --no-build --prefix ~/.local

# Honour a custom Cargo target directory while building
CARGO_TARGET_DIR=/dev/shm/rustfetch-target ./install.sh --prefix ~/.local

# Non-interactive (no overwrite prompt)
./install.sh --yes

# Uninstall
./install.sh --uninstall
./install.sh --prefix /opt/rustfetch --uninstall
```

Options: `--prefix <dir>`, `--user`, `--uninstall`, `--yes`, `--no-build`,
`-h/--help`. The binary is installed with mode `0755`. If a non-root user asks
for a system prefix, the script prints the exact `sudo` command and exits
without doing anything.

## 2. Debian / Ubuntu

### Quick `.deb` from the existing binary (dependency-free)

```bash
# Uses target/release/rustfetch (or $CARGO_TARGET_DIR/release/rustfetch)
packaging/build-deb.sh
# -> dist/rustfetch_0.1.0-1_amd64.deb
sudo apt install ./dist/rustfetch_0.1.0-1_amd64.deb
```

Environment overrides: `VERSION`, `RELEASE`, `ARCH`, `OUT_DIR`, `BUILD_DIR`,
`RUSTFETCH_BIN`, `CARGO_TARGET_DIR`. Requires only `dpkg-deb` (from
`dpkg-dev`). The generated control sets `Depends: libc6, libgcc-s1`.

### Debian source build (debhelper)

```bash
sudo apt install -y debhelper dpkg-dev rustc cargo
cp -r packaging/debian .          # place debian/ at the repository root
dpkg-buildpackage -us -uc -b
```

`packaging/debian/rules` runs `cargo build --release` and installs the binary
plus `README.md`/`LICENSE` (as `copyright`).

## 3. Fedora / RHEL

```bash
sudo dnf install -y rpm-build rust cargo
# Builds from a clean source tarball made with `git archive`; spec builds with Cargo.
packaging/build-rpm.sh
# -> rpmbuild/RPMS/.../rustfetch-0.1.0-1.*.rpm
sudo dnf install rpmbuild/RPMS/*/rustfetch-0.1.0-1.*.rpm
```

Set `RPM_TOPDIR=/path/to/topdir` (or `TOPDIR=...`) to change the rpmbuild tree.
The spec declares `License: MIT`, `BuildRequires: rust, cargo`,
`Requires: glibc, libgcc`, a proper `%install` and `%files`.

## 4. Arch / Manjaro

### Source build (from the repo root)

```bash
sudo pacman -S --needed base-devel rust
makepkg -si -p PKGBUILD
```

`PKGBUILD` fetches the `v0.1.0` source tarball, runs `cargo build --release`,
and installs the binary, LICENSE and README. `sha256sums` is `SKIP`; fill it
with `sha256sum` of the release tarball for reproducible builds.

### Prebuilt tarball + bin PKGBUILD

```bash
# 1. Create the tarball from the existing release binary
packaging/build-tarball.sh
# -> dist/rustfetch-0.1.0-x86_64-unknown-linux-gnu.tar.zst

# 2. Fill in the checksum in packaging/bin/PKGBUILD
sha256sum dist/rustfetch-0.1.0-x86_64-unknown-linux-gnu.tar.zst
# replace sha256sums=('REPLACE_WITH_SHA256_OF_RELEASE_TARBALL')
# with the printed hash

# 3. Put the tarball next to packaging/bin/PKGBUILD and build
cp dist/rustfetch-0.1.0-x86_64-unknown-linux-gnu.tar.zst packaging/bin/
cd packaging/bin && makepkg -si
```

`build-tarball.sh` needs `tar --zstd` or `zstd`; it reports an error if neither
is available. It prints the SHA-256 for convenience.

## Runtime note

fastfetch is **not** required at runtime. A user's existing
`~/.config/fastfetch/config.jsonc` may be read as a compatibility preset, but
the `fastfetch` executable/package itself is neither required nor called.
