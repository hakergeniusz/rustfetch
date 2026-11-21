#!/usr/bin/env bash
# Build a .deb from an already-built rustfetch binary, using only dpkg-deb.
# No cargo-deb, no debhelper, no automatic package installation.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
VERSION="${VERSION:-0.1.0}"
RELEASE="${RELEASE:-1}"
ARCH="${ARCH:-$(dpkg --print-architecture 2>/dev/null || printf 'amd64')}"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
BUILD_DIR="${BUILD_DIR:-/dev/shm/rf-scratch-pkg/deb-build}"

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  TARGET_DIR="$CARGO_TARGET_DIR"
else
  TARGET_DIR="$ROOT/target"
fi
BIN="${RUSTFETCH_BIN:-$TARGET_DIR/release/rustfetch}"

info() { printf '[*] %s\n' "$*"; }
ok()   { printf '[+] %s\n' "$*"; }
die()  { printf '[ERROR] %s\n' "$*" >&2; exit 1; }

command -v dpkg-deb >/dev/null 2>&1 || die "dpkg-deb not found (install the 'dpkg-dev' package)"
[[ -f "$BIN" ]] || die "binary not found at $BIN (run 'cargo build --release' first, or set RUSTFETCH_BIN)"

PKG="rustfetch_${VERSION}-${RELEASE}_${ARCH}"
STAGE="$BUILD_DIR/$PKG"
OUT="$OUT_DIR/${PKG}.deb"

info "Staging Debian package: $STAGE"
rm -rf -- "$STAGE"
mkdir -p "$STAGE/DEBIAN" \
         "$STAGE/usr/bin" \
         "$STAGE/usr/share/doc/rustfetch" \
         "$STAGE/usr/share/licenses/rustfetch"

install -m 0755 "$BIN" "$STAGE/usr/bin/rustfetch"
install -m 0644 "$ROOT/LICENSE" "$STAGE/usr/share/doc/rustfetch/copyright"
install -m 0644 "$ROOT/README.md" "$STAGE/usr/share/doc/rustfetch/README.md"
install -m 0644 "$ROOT/LICENSE" "$STAGE/usr/share/licenses/rustfetch/LICENSE"

cat > "$STAGE/DEBIAN/control" <<EOF
Package: rustfetch
Version: ${VERSION}-${RELEASE}
Section: utils
Priority: optional
Architecture: ${ARCH}
Depends: libc6, libgcc-s1
Maintainer: hakergeniusz <hakergeniusz@users.noreply.github.com>
Homepage: https://github.com/hakergeniusz/rustfetch
Description: Fastfetch, rewritten in Rust
 A fast, dependency-light system information tool written in Rust. It mirrors
 fastfetch's logo database, colors and modules, reading system information
 directly from procfs, sysfs, DMI and pci.ids.
 .
 The binary is self-contained apart from the standard C runtime libraries
 (libc6 and libgcc-s1). The fastfetch package is NOT required at runtime.
EOF

# Newer dpkg-deb supports --root-owner-group; older ones do not.
HELP_TEXT="$(dpkg-deb --help 2>&1 || true)"
OWNER_FLAG=""
[[ "$HELP_TEXT" == *--root-owner-group* ]] && OWNER_FLAG="--root-owner-group"

mkdir -p -- "$OUT_DIR"
info "Running: dpkg-deb --build $OWNER_FLAG $STAGE $OUT"
# shellcheck disable=SC2086
dpkg-deb --build $OWNER_FLAG "$STAGE" "$OUT"
ok "Built $OUT"
