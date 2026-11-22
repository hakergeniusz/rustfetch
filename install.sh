#!/bin/sh
# rustfetch universal installer (Debian/Ubuntu, Fedora/RHEL, Arch, Alpine, openSUSE).
# rustfetch is a fastfetch- and hyfetch-inspired system information tool written
# in Rust.
#
# This file is a tiny POSIX-sh bootstrap plus a bash payload kept in a quoted
# here-doc.  That lets the very same file work in three situations:
#   * executed as a file:        ./install.sh | bash install.sh
#   * piped from curl:           curl -fsSL <url> | sh
#   * run via a command string:  sh -c "$(curl -fsSL <url>)"
# POSIX sh never has to parse the bash-only payload (arrays, [[ ]], ...).
#
# This script never installs packages, never invokes sudo on its own, and never
# depends on fastfetch being present at runtime.
if [ -f "$0" ]; then
  case "$0" in
    sh|bash|dash|ash|ksh|zsh|/bin/sh|/bin/bash|/bin/dash|/usr/bin/sh|/usr/bin/bash|/usr/bin/dash)
      unset RUSTFETCH_LOCAL_FILE 2>/dev/null || :
      ;;
    *)
      RUSTFETCH_LOCAL_FILE=$(cd -- "$(dirname -- "$0")" && pwd -P)/$(basename -- "$0")
      export RUSTFETCH_LOCAL_FILE
      ;;
  esac
else
  unset RUSTFETCH_LOCAL_FILE 2>/dev/null || :
fi
exec bash -s -- "$@" <<'RUSTFETCH_PAYLOAD_EOF'
set -euo pipefail

REPO_OWNER="hakergeniusz"
REPO_NAME="rustfetch"
API_BASE="https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases"
ONE_LINER="curl -fsSL https://raw.githubusercontent.com/${REPO_OWNER}/${REPO_NAME}/main/install.sh | sh"

# ---------------------------------------------------------------------------
# Mode detection: local file vs. stdin/pipe.  In remote mode we deliberately
# do NOT try to resolve a local repository root.
# ---------------------------------------------------------------------------
SCRIPT_PATH=""
if [[ -n "${RUSTFETCH_LOCAL_FILE:-}" ]]; then
  # Set by the POSIX bootstrap when this file was executed as a real file.
  SCRIPT_PATH="$RUSTFETCH_LOCAL_FILE"
elif [[ -n "${BASH_SOURCE[0]:-}" \
     && "${BASH_SOURCE[0]}" != "-" \
     && "${BASH_SOURCE[0]}" != "/dev/stdin" \
     && -f "${BASH_SOURCE[0]}" ]]; then
  _src="${BASH_SOURCE[0]}"
  SCRIPT_PATH="$(cd -- "$(dirname -- "$_src")" && pwd -P)/$(basename -- "$_src")"
fi

if [[ -n "$SCRIPT_PATH" ]]; then
  MODE=local
  REPO_ROOT="$(cd -- "$(dirname -- "$SCRIPT_PATH")" && pwd -P)"
else
  MODE=remote
  REPO_ROOT=""
fi

# ---------------------------------------------------------------------------
# Options
# ---------------------------------------------------------------------------
PREFIX=""
USER_INSTALL=false
UNINSTALL=false
YES=false
NO_BUILD=false
VERSION_TAG=""
NO_VERIFY=false
FORCE=false
PRINT_PLAN=false
WORKDIR=""
DEST=""
ORIG_ARGS=("$@")

info() { printf '[*] %s\n' "$*"; }
ok()   { printf '[+] %s\n' "$*"; }
warn() { printf '[!] %s\n' "$*" >&2; }
die()  { printf '[ERROR] %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<EOF
Usage: install.sh [OPTIONS]

Install or remove the 'rustfetch' executable.

Modes:
  Local   - run as a file (./install.sh); builds from the repository.
  Remote  - piped from curl or run via 'sh -c "\$(curl ...)"'; downloads a
            prebuilt release asset (or builds from the tagged source).

Options:
  --prefix <dir>   Install into <dir>/bin (default: /usr/local as root,
                   \$HOME/.local as a normal user).
  --user           Force a user install (\$HOME/.local), even when root.
  --uninstall      Remove the installed 'rustfetch' executable.
  --yes, -y        Do not prompt before overwriting an existing binary
                   (remote mode already replaces by default).
  --no-build       Skip 'cargo build --release' and install an existing
                   target/release/rustfetch (or \$CARGO_TARGET_DIR/release/rustfetch).
  --version <tag>  Pin a release tag in remote mode (e.g. --version v0.1.0).
                   Without it, the latest release is used.
  --no-verify      Remote mode: skip SHA256 verification of the release asset.
  --force          Always replace an existing binary, without prompting
                   (also skips the "already up to date" shortcut).
  --print-plan     Dry run: print the detected mode, URLs and install path,
                   then exit. No network access and no files are changed.
  -h, --help       Show this help.

Environment:
  CARGO_TARGET_DIR    Cargo target directory (defaults to <repo>/target).
  RUSTFETCH_API_URL   Override the GitHub release API URL (file:// paths and
                      plain absolute paths are accepted; useful for mirrors
                      and for offline testing).
  RUSTFETCH_ASSET_URL Override the release asset download URL directly.

One-liner install:
  ${ONE_LINER}
  sh -c "\$(curl -fsSL https://raw.githubusercontent.com/${REPO_OWNER}/${REPO_NAME}/main/install.sh)"

rustfetch is a fastfetch- and hyfetch-inspired system information tool written
in Rust. It is self-contained at runtime (standard libc/libm/libgcc_s only);
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
    --user)       USER_INSTALL=true; shift ;;
    --uninstall)  UNINSTALL=true; shift ;;
    --yes|-y)     YES=true; shift ;;
    --no-build)   NO_BUILD=true; shift ;;
    --version)
      (($# >= 2)) || die "--version requires a tag"
      VERSION_TAG="$2"; shift 2 ;;
    --version=*)
      VERSION_TAG="${1#*=}"; shift ;;
    --no-verify)  NO_VERIFY=true; shift ;;
    --force)      FORCE=true; shift ;;
    --print-plan) PRINT_PLAN=true; shift ;;
    -h|--help)    usage; exit 0 ;;
    *)            die "unknown option: $1 (try --help)" ;;
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
  if [[ -n "$SCRIPT_PATH" ]]; then
    printf '  sudo '
    printf '%q ' "$SCRIPT_PATH" "${ORIG_ARGS[@]}"
    printf '\n'
  else
    printf "  sudo sh -c '%s -- --prefix %q'\n" "$ONE_LINER" "$PREFIX"
  fi
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
    *debian*|*ubuntu*)                                  printf '%s' 'sudo apt install -y cargo rustc' ;;
    *fedora*|*rhel*|*centos*|*rocky*|*alma*|*mageia*) printf '%s' 'sudo dnf install -y cargo rustc' ;;
    *arch*|*manjaro*|*endeavouros*)                     printf '%s' 'sudo pacman -S --needed rust' ;;
    *alpine*)                                           printf '%s' 'sudo apk add cargo rust' ;;
    *suse*|*opensuse*)                                  printf '%s' 'sudo zypper install -y cargo rustc' ;;
    *)                                                  printf '%s' 'install Rust from https://rustup.rs' ;;
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

# Effective GitHub release API URL (no network access performed here).
release_api_url() {
  if [[ -n "${RUSTFETCH_API_URL:-}" ]]; then
    case "$RUSTFETCH_API_URL" in
      /*) printf 'file://%s\n' "$RUSTFETCH_API_URL" ;;
      *)  printf '%s\n' "$RUSTFETCH_API_URL" ;;
    esac
    return
  fi
  if [[ -n "$VERSION_TAG" ]]; then
    printf '%s/tags/%s\n' "$API_BASE" "$VERSION_TAG"
  else
    printf '%s/latest\n' "$API_BASE"
  fi
}

# ---------------------------------------------------------------------------
# Shared installer helpers
# ---------------------------------------------------------------------------
install_binary() {
  local src="$1"
  [[ -f "$src" ]] || die "binary not found at $src"
  mkdir -p -- "$BINDIR"
  DEST="$BINDIR/rustfetch"
  if [[ -e "$DEST" ]] && cmp -s -- "$src" "$DEST" && ! $FORCE; then
    ok "Already up to date: $DEST"
  else
    if [[ -e "$DEST" ]] && ! $YES && ! $FORCE; then
      if [[ "$MODE" == remote ]]; then
        # A one-liner install cannot prompt (stdin is the script), and the
        # user already asked to install by running it. Replace by default
        # with a clear notice; --force/--yes remain explicit overrides.
        warn "$DEST exists and differs; replacing it with the release binary"
      else
        warn "$DEST exists and differs from the source binary."
        read -r -p "Overwrite it? [y/N] " answer
        [[ "$answer" == [yY]* ]] || die "Aborted by user"
      fi
    fi
    install -m 0755 -- "$src" "$DEST"
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
}

do_uninstall() {
  info "Uninstalling $BINDIR/rustfetch"
  if [[ -e "$BINDIR/rustfetch" || -L "$BINDIR/rustfetch" ]]; then
    rm -f -- "$BINDIR/rustfetch"
    ok "Removed $BINDIR/rustfetch"
  else
    ok "Nothing to remove; rustfetch is already uninstalled"
  fi
}

finish_ok() {
  printf '\n'
  ok "Success! Installed: $DEST"
  info "Call it with: $DEST --version   (or 'rustfetch' if $BINDIR is in PATH)"
}

fetch() {
  local url="$1" out="$2"
  info "Fetching: $url"
  curl -fsSL --retry 3 --retry-delay 1 "$url" -o "$out"
}

make_workdir() {
  # Prefer RAM to limit SSD writes.
  if [[ -w /dev/shm ]]; then
    mktemp -d /dev/shm/rustfetch-install.XXXXXX
  else
    mktemp -d
  fi
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

extract_archive() {
  local arc="$1" dest="$2"
  case "$arc" in
    *.tar.zst|*.zst)
      if tar --zstd -xf "$arc" -C "$dest" 2>/dev/null; then return 0; fi
      if command -v zstd >/dev/null 2>&1; then
        if zstd -dc -- "$arc" | tar -xf - -C "$dest"; then return 0; fi
      fi
      warn "Cannot extract zstd archive (need 'tar --zstd' or 'zstd')"
      return 1
      ;;
    *)
      if tar -xf "$arc" -C "$dest"; then return 0; fi
      warn "Cannot extract archive: $arc"
      return 1
      ;;
  esac
}

find_binary() {
  local root="$1"
  if [[ -f "$root/rustfetch" ]]; then
    printf '%s\n' "$root/rustfetch"
    return 0
  fi
  shopt -s globstar nullglob
  local candidates=( "$root"/**/rustfetch )
  if ((${#candidates[@]})); then
    printf '%s\n' "${candidates[0]}"
    return 0
  fi
  return 1
}

# Parse the GitHub release JSON into tab-separated key/value lines.
# Uses python3 when available (no jq needed); a small sed/awk fallback otherwise.
parse_release_json() {
  local jf="$1"
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$jf" <<'PY'
import json, sys
try:
    with open(sys.argv[1], encoding="utf-8") as fh:
        data = json.load(fh)
except Exception as exc:
    sys.stderr.write("rustfetch: could not parse release JSON: %s\n" % exc)
    sys.exit(2)

def emit(key, val):
    val = "" if val is None else str(val).replace("\t", " ").replace("\n", " ")
    sys.stdout.write("%s\t%s\n" % (key, val))

emit("tag", data.get("tag_name"))
assets = data.get("assets") or []
zsts = [a for a in assets if str(a.get("name", "")).endswith(".tar.zst")]
pref = [a for a in zsts if "x86_64-unknown-linux-gnu" in a.get("name", "")]
chosen = (pref or zsts or [None])[0]
if chosen:
    emit("asset_name", chosen.get("name"))
    emit("asset_url", chosen.get("browser_download_url"))
shas = [a for a in assets if str(a.get("name", "")).endswith(".sha256")]
if shas:
    nm = str((chosen or {}).get("name", ""))
    exact = [a for a in shas if a.get("name") == nm + ".sha256"]
    emit("sha_url", (exact or shas)[0].get("browser_download_url"))
if data.get("tarball_url"):
    emit("source_url", data.get("tarball_url"))
PY
    return 0
  fi

  # --- sed/awk fallback when python3 is unavailable ---
  local tag source_url asset_name asset_url sha_name sha_url
  tag="$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$jf" | head -n1)"
  source_url="$(sed -n 's/.*"tarball_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$jf" | head -n1)"
  [[ -n "$tag" ]] && printf 'tag\t%s\n' "$tag"
  [[ -n "$source_url" ]] && printf 'source_url\t%s\n' "$source_url"
  asset_name="$(sed -n 's/.*"name"[[:space:]]*:[[:space:]]*"\([^"]*\.tar\.zst\)".*/\1/p' "$jf" | head -n1)"
  if [[ -n "$asset_name" ]]; then
    asset_url="$(awk -v n="$asset_name" '
      index($0, "\"" n "\"") { found = 1 }
      found && /"browser_download_url"/ {
        sub(/^.*"browser_download_url"[[:space:]]*:[[:space:]]*"/, "")
        sub(/".*$/, "")
        print
        exit
      }
    ' "$jf")"
    printf 'asset_name\t%s\n' "$asset_name"
    [[ -n "$asset_url" ]] && printf 'asset_url\t%s\n' "$asset_url"
  fi
  sha_name="$(sed -n 's/.*"name"[[:space:]]*:[[:space:]]*"\([^"]*\.sha256\)".*/\1/p' "$jf" | head -n1)"
  if [[ -n "$sha_name" ]]; then
    sha_url="$(awk -v n="$sha_name" '
      index($0, "\"" n "\"") { found = 1 }
      found && /"browser_download_url"/ {
        sub(/^.*"browser_download_url"[[:space:]]*:[[:space:]]*"/, "")
        sub(/".*$/, "")
        print
        exit
      }
    ' "$jf")"
    [[ -n "$sha_url" ]] && printf 'sha_url\t%s\n' "$sha_url"
  fi
  return 0
}

# ---------------------------------------------------------------------------
# Dry-run plan (no network, no file changes)
# ---------------------------------------------------------------------------
if $PRINT_PLAN; then
  info "rustfetch installer (plan only; --print-plan)"
  info "Mode       : $MODE"
  info "Prefix     : $PREFIX"
  info "Binary dir : $BINDIR"
  if [[ "$MODE" == local ]]; then
    info "Repository : $REPO_ROOT"
    if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
      info "Target dir : $CARGO_TARGET_DIR"
    else
      info "Target dir : $REPO_ROOT/target"
    fi
  else
    info "API URL    : $(release_api_url)"
    info "Version    : ${VERSION_TAG:-latest}"
    if [[ -n "${RUSTFETCH_ASSET_URL:-}" ]]; then
      info "Asset URL  : $RUSTFETCH_ASSET_URL (override)"
    else
      info "Asset URL  : <resolved from the release API at install time>"
    fi
    info "Verify     : $($NO_VERIFY && printf no || printf yes)"
  fi
  exit 0
fi

# ---------------------------------------------------------------------------
# Local mode (unchanged behaviour: build from the repo with --release)
# ---------------------------------------------------------------------------
if [[ "$MODE" == local ]]; then
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
    do_uninstall
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

  install_binary "$BIN_SRC"
  ok "Done."
  exit 0
fi

# ---------------------------------------------------------------------------
# Remote mode
# ---------------------------------------------------------------------------
remote_install() {
  local api_url tag="" asset_name="" asset_url="" sha_url="" source_url=""
  api_url="$(release_api_url)"

  info "rustfetch installer (remote mode)"
  info "Prefix     : $PREFIX"
  info "Binary dir : $BINDIR"
  info "API URL    : $api_url"

  if $UNINSTALL; then
    do_uninstall
    return 0
  fi

  WORKDIR="$(make_workdir)"
  trap 'if [[ -n "${WORKDIR:-}" && -d "$WORKDIR" ]]; then rm -rf -- "$WORKDIR"; fi' EXIT

  info "Querying release information ..."
  local json="$WORKDIR/release.json"
  fetch "$api_url" "$json" || die "could not query release API: $api_url"

  while IFS=$'\t' read -r key value; do
    case "$key" in
      tag)        tag="$value" ;;
      asset_name) asset_name="$value" ;;
      asset_url)  asset_url="$value" ;;
      sha_url)    sha_url="$value" ;;
      source_url) source_url="$value" ;;
    esac
  done < <(parse_release_json "$json")

  if [[ -n "${RUSTFETCH_ASSET_URL:-}" ]]; then
    asset_url="$RUSTFETCH_ASSET_URL"
    [[ -n "$asset_name" ]] || asset_name="$(basename -- "$asset_url")"
    info "Using RUSTFETCH_ASSET_URL override"
  fi

  if [[ -n "$asset_url" ]]; then
    info "Release tag : ${tag:-${VERSION_TAG:-unknown}}"
    info "Asset       : ${asset_name:-$(basename -- "$asset_url")}"
    if install_from_asset "$asset_url" "${asset_name:-$(basename -- "$asset_url")}" "$sha_url"; then
      :
    else
      warn "Prebuilt asset install failed; falling back to a source build"
      source_build "${tag:-${VERSION_TAG:-}}" "$source_url"
    fi
  else
    warn "No .tar.zst release asset found; falling back to a source build"
    source_build "${tag:-${VERSION_TAG:-}}" "$source_url"
  fi
}

install_from_asset() {
  local url="$1" name="$2" sumurl="$3"
  local archive="$WORKDIR/$name"
  fetch "$url" "$archive" || return 1

  if $NO_VERIFY; then
    info "Checksum verification skipped (--no-verify)"
  elif [[ -n "$sumurl" ]]; then
    local sumfile="$WORKDIR/$name.sha256"
    info "Fetching SHA256 sidecar: $sumurl"
    fetch "$sumurl" "$sumfile" || { warn "Could not fetch checksum sidecar"; return 1; }
    local expected actual
    expected="$(awk 'NR==1 {print $1}' "$sumfile")"
    [[ -n "$expected" ]] || { warn "Empty checksum sidecar"; return 1; }
    actual="$(sha256_of "$archive")"
    if [[ "$actual" != "$expected" ]]; then
      die "SHA256 mismatch: expected '$expected', got '$actual'"
    fi
    ok "SHA256 verified: $actual"
  else
    info "No .sha256 sidecar published; SHA256 for transparency: $(sha256_of "$archive")"
  fi

  local ex="$WORKDIR/extract"
  mkdir -p -- "$ex"
  extract_archive "$archive" "$ex" || return 1
  local bin
  bin="$(find_binary "$ex")" || { warn "rustfetch binary not found inside the release asset"; return 1; }
  install_binary "$bin"
  finish_ok
}

source_build() {
  local tag_ref="$1" src_url="$2"
  local ref="${tag_ref:-master}"
  if [[ -z "$src_url" ]]; then
    src_url="https://github.com/${REPO_OWNER}/${REPO_NAME}/archive/refs/tags/${ref}.tar.gz"
  fi
  info "Source-build fallback (cargo build --release)"
  info "Source URL  : $src_url"

  require_build_tools

  local srcarc="$WORKDIR/source.tar.gz"
  fetch "$src_url" "$srcarc" || die "could not download source tarball: $src_url"
  local srcdir="$WORKDIR/src"
  mkdir -p -- "$srcdir"
  extract_archive "$srcarc" "$srcdir" || die "could not extract source tarball"

  local crate_dir="$srcdir"
  if [[ ! -f "$crate_dir/Cargo.toml" ]]; then
    shopt -s globstar nullglob
    local manifests=( "$srcdir"/**/Cargo.toml )
    ((${#manifests[@]})) || die "Cargo.toml not found in the source tarball"
    crate_dir="$(dirname -- "${manifests[0]}")"
  fi

  local target="$WORKDIR/target"
  if [[ -w /dev/shm ]]; then
    target="/dev/shm/rustfetch-target"
  fi
  info "Building with: CARGO_TARGET_DIR='$target' cargo build --release"
  if ! (cd "$crate_dir" && CARGO_TARGET_DIR="$target" cargo build --release); then
    die "cargo build --release failed"
  fi
  ok "Build finished"

  install_binary "$target/release/rustfetch"
  finish_ok
}

remote_install
ok "Done."
RUSTFETCH_PAYLOAD_EOF
