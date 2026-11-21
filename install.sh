#!/usr/bin/env bash
# rustfetch universal installer (Debian/Ubuntu, Fedora/RHEL, Arch, Alpine, openSUSE).
# This script never installs packages, never invokes sudo on its own, and never
# depends on fastfetch being present at runtime.
set -euo pipefail

SCRIPT_PATH="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)/$(basename -- "${BASH_SOURCE[0]}")"
REPO_ROOT="$(cd -- "$(dirname -- "$SCRIPT_PATH")" && pwd -P)"

PREFIX=""
USER_INSTALL=false
UNINSTALL=false
YES=false
NO_BUILD=false
ORIG_ARGS=("$@")

info() { printf '[*] %s\n' "$*"; }
ok()   { printf '[+] %s\n' "$*"; }
warn() { printf '[!] %s\n' "$*" >&2; }
die()  { printf '[ERROR] %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<EOF
Usage: install.sh [OPTIONS]

Install or remove the 'rustfetch' executable.

Options:
  --prefix <dir>   Install into <dir>/bin (default: /usr/local as root,
                   \$HOME/.local as a normal user).
  --user           Force a user install (\$HOME/.local), even when root.
  --uninstall      Remove the installed 'rustfetch' executable.
  --yes, -y        Do not prompt before overwriting an existing binary.
  --no-build       Skip 'cargo build --release' and install an existing
                   target/release/rustfetch (or \$CARGO_TARGET_DIR/release/rustfetch).
  -h, --help       Show this help.

Environment:
  CARGO_TARGET_DIR   Cargo target directory (defaults to <repo>/target).

rustfetch is self-contained at runtime (standard libc/libm/libgcc_s only);
fastfetch is NOT required.
EOF
}

while (($#)); do
  case "$1" in
    --prefix)
      (($# >= 2)) || die "--prefix requires a directory"
      PREFIX="$2"; shift 2 ;;
    --prefix=*)
      PREFIX="${1#*=}"; shift ;;
    --user)      USER_INSTALL=true; shift ;;
    --uninstall) UNINSTALL=true; shift ;;
    --yes|-y)    YES=true; shift ;;
    --no-build)  NO_BUILD=true; shift ;;
    -h|--help)   usage; exit 0 ;;
    *)           die "unknown option: $1 (try --help)" ;;
  esac
done

# Resolve the effective prefix.
if $USER_INSTALL; then
  PREFIX="${HOME:-$PWD}/.local"
elif [[ -z "$PREFIX" ]]; then
  if [[ $(id -u) -eq 0 ]]; then
    PREFIX=/usr/local
  else
    PREFIX="${HOME:-$PWD}/.local"
  fi
fi
PREFIX="${PREFIX%/}"
[[ -n "$PREFIX" && "$PREFIX" != "/" ]] || die "refusing to use '/' as an installation prefix"
BINDIR="$PREFIX/bin"

is_system_prefix() {
  case "$1" in
    /usr|/usr/*|/opt|/opt/*) return 0 ;;
    *) return 1 ;;
  esac
}

# Never silently escalate: require the user to run sudo themselves.
if [[ $(id -u) -ne 0 ]] && is_system_prefix "$PREFIX"; then
  warn "Refusing to write to $BINDIR as a non-root user."
  info "Re-run this script with sudo, using this exact command:"
  printf '  sudo '
  printf '%q ' "$SCRIPT_PATH" "${ORIG_ARGS[@]}"
  printf '\n'
  exit 1
fi

detect_distro() {
  local id="" idlike=""
  if [[ -r /etc/os-release ]]; then
    # shellcheck disable=SC1091
    . /etc/os-release
    id="${ID:-}"
    idlike="${ID_LIKE:-}"
  fi
  printf '%s %s\n' "$id" "$idlike"
}

rust_hint() {
  local id idlike s
  read -r id idlike < <(detect_distro)
  s=" ${id} ${idlike} "
  case "$s" in
    *debian*|*ubuntu*)                       printf '%s' 'sudo apt install -y cargo rustc' ;;
    *fedora*|*rhel*|*centos*|*rocky*|*alma*|*mageia*) printf '%s' 'sudo dnf install -y cargo rustc' ;;
    *arch*|*manjaro*|*endeavouros*)          printf '%s' 'sudo pacman -S --needed rust' ;;
    *alpine*)                                printf '%s' 'sudo apk add cargo rust' ;;
    *suse*|*opensuse*)                       printf '%s' 'sudo zypper install -y cargo rustc' ;;
    *)                                       printf '%s' 'install Rust from https://rustup.rs' ;;
  esac
}

require_build_tools() {
  local missing=false
  command -v cargo >/dev/null 2>&1 || missing=true
  command -v rustc >/dev/null 2>&1 || missing=true
  if $missing; then
    warn "Build toolchain incomplete: both 'cargo' and 'rustc' are required."
    info "Distro-specific install hint: $(rust_hint)"
    info "This script does NOT install packages automatically."
    exit 1
  fi
}

# Where to find / build the binary.
if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  TARGET_DIR="$CARGO_TARGET_DIR"
else
  TARGET_DIR="$REPO_ROOT/target"
fi
BIN_SRC="$TARGET_DIR/release/rustfetch"

info "rustfetch installer"
info "Repository : $REPO_ROOT"
info "Prefix     : $PREFIX"
info "Binary dir : $BINDIR"
info "Target dir : $TARGET_DIR"

if $UNINSTALL; then
  info "Uninstalling $BINDIR/rustfetch"
  if [[ -e "$BINDIR/rustfetch" || -L "$BINDIR/rustfetch" ]]; then
    rm -f -- "$BINDIR/rustfetch"
    ok "Removed $BINDIR/rustfetch"
  else
    ok "Nothing to remove; rustfetch is already uninstalled"
  fi
  exit 0
fi

if $NO_BUILD; then
  info "Skipping build (--no-build)"
else
  require_build_tools
  info "Building with: CARGO_TARGET_DIR='$TARGET_DIR' cargo build --release"
  (cd "$REPO_ROOT" && CARGO_TARGET_DIR="$TARGET_DIR" cargo build --release)
  ok "Build finished"
fi

[[ -f "$BIN_SRC" ]] || die "binary not found at $BIN_SRC (build first or drop --no-build)"

mkdir -p -- "$BINDIR"
DEST="$BINDIR/rustfetch"

if [[ -e "$DEST" ]] && cmp -s -- "$BIN_SRC" "$DEST"; then
  ok "Already up to date: $DEST"
else
  if [[ -e "$DEST" ]] && ! $YES; then
    warn "$DEST exists and differs from the source binary."
    read -r -p "Overwrite it? [y/N] " answer
    [[ "$answer" == [yY]* ]] || die "Aborted by user"
  fi
  install -m 0755 -- "$BIN_SRC" "$DEST"
  ok "Installed $DEST (mode 0755)"
fi

if "$DEST" --version >/dev/null 2>&1; then
  ok "Smoke test passed: '$DEST --version'"
else
  warn "Smoke test with --version failed (binary was still installed)"
fi

case ":${PATH:-}:" in
  *":$BINDIR:"*) ;;
  *) info "Note: $BINDIR is not in PATH; add it if 'rustfetch' is not found." ;;
esac

ok "Done."
