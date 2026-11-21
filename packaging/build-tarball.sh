#!/usr/bin/env bash
# Produce a release tarball containing the binary, LICENSE and README:
#   rustfetch-0.1.0-x86_64-unknown-linux-gnu.tar.zst
# Requires `tar --zstd` or the `zstd` binary.
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
VERSION="${VERSION:-0.1.0}"
TARGET="${TARGET:-x86_64-unknown-linux-gnu}"
OUT_DIR="${OUT_DIR:-$ROOT/dist}"
BUILD_DIR="${BUILD_DIR:-/dev/shm/rf-scratch-pkg/tarball-build}"

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  TARGET_DIR="$CARGO_TARGET_DIR"
else
  TARGET_DIR="$ROOT/target"
fi
BIN="${RUSTFETCH_BIN:-$TARGET_DIR/release/rustfetch}"

info() { printf '[*] %s\n' "$*"; }
ok()   { printf '[+] %s\n' "$*"; }
die()  { printf '[ERROR] %s\n' "$*" >&2; exit 1; }

[[ -f "$BIN" ]] || die "binary not found at $BIN (run 'cargo build --release' first, or set RUSTFETCH_BIN)"

NAME="rustfetch-${VERSION}-${TARGET}"
STAGE="$BUILD_DIR/$NAME"
OUT="$OUT_DIR/${NAME}.tar.zst"

info "Staging tarball tree: $STAGE"
rm -rf -- "$STAGE"
mkdir -p -- "$STAGE"
install -m 0755 "$BIN" "$STAGE/rustfetch"
install -m 0644 "$ROOT/LICENSE" "$STAGE/LICENSE"
install -m 0644 "$ROOT/README.md" "$STAGE/README.md"

mkdir -p -- "$OUT_DIR"
if tar --zstd --version >/dev/null 2>&1; then
  info "Creating $OUT with tar --zstd"
  tar --zstd -cf "$OUT" -C "$STAGE" rustfetch LICENSE README.md
elif command -v zstd >/dev/null 2>&1; then
  info "Creating $OUT with tar | zstd"
  tar -cf - -C "$STAGE" rustfetch LICENSE README.md | zstd -q -o "$OUT"
else
  die "neither 'tar --zstd' nor 'zstd' is available; install zstd and retry"
fi

ok "Built $OUT"
info "sha256: $(sha256sum "$OUT" | cut -d' ' -f1)"
info "Copy this hash into packaging/bin/PKGBUILD sha256sums=('...')."
