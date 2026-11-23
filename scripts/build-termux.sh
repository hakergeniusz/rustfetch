#!/usr/bin/env bash
# Build, test and install rustfetch natively on a Termux (Android) device.
# Run this ON the device from a rustfetch checkout. See docs/termux.md.
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/build-termux.sh [OPTIONS]

Build rustfetch natively on a Termux device, run the test suite, install the
binary to $PREFIX/bin/rustfetch, then print smoke tests and a diff against the
Termux fastfetch --pipe output.

Options:
  -h, --help       Show this help and exit.
      --no-install Do not copy the binary into $PREFIX/bin.
      --no-test    Skip `cargo --locked test --release`.

Environment:
  PREFIX                Termux prefix (auto-detected; must look like Termux).
  CARGO_TARGET_DIR      Forced to $HOME/.cache/rustfetch-target: Termux has no
                        /tmp and /var/tmp is read-only.
  RUSTFETCH_FASTFETCH   Path to fastfetch for the comparison (default: PATH).

No cross toolchain, Android NDK or root is required: this is a native aarch64
build performed by the device's own rustc/cargo and clang.
EOF
}

do_install=1
do_test=1
while (($# > 0)); do
    case "$1" in
        -h|--help) usage; exit 0 ;;
        --no-install) do_install=0; shift ;;
        --no-test) do_test=0; shift ;;
        *) printf 'Error: unknown option: %s\n\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
done

# --- Termux detection -------------------------------------------------------
is_termux=0
if [[ -n "${PREFIX:-}" && -d "$PREFIX" ]]; then
    if [[ "$PREFIX" == *com.termux* ]] || { [[ -x "$PREFIX/bin/pkg" ]] && [[ "$(uname -o 2>/dev/null || true)" == "Android" ]]; }; then
        is_termux=1
    fi
fi
if (( ! is_termux )); then
    printf 'Error: run this script inside Termux on the device.\n' >&2
    printf '  PREFIX=%q (expected the Termux prefix, e.g. /data/data/com.termux/files/usr)\n' "${PREFIX:-<unset>}" >&2
    printf '  Install rust with: pkg install rust\n' >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    printf 'Error: cargo not found in PATH. Install with: pkg install rust\n' >&2
    exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Termux has no /tmp and /var/tmp is read-only; keep all build artifacts under
# the house-rule cache directory. This is the same as prefixing every cargo
# invocation with CARGO_TARGET_DIR=$HOME/.cache/rustfetch-target.
export CARGO_TARGET_DIR="$HOME/.cache/rustfetch-target"
mkdir -p "$CARGO_TARGET_DIR"

printf '==> Building rustfetch (native aarch64 release build)\n'
printf '    CARGO_TARGET_DIR=%s\n' "$CARGO_TARGET_DIR"
printf '    cargo --locked build --release\n'
cargo --locked build --release

built_bin="$CARGO_TARGET_DIR/release/rustfetch"
if [[ ! -x "$built_bin" ]]; then
    printf 'Error: expected binary %s was not produced\n' "$built_bin" >&2
    exit 1
fi

test_rc=0
if (( do_test )); then
    printf '\n==> Tests: cargo --locked test --release\n'
    if cargo --locked test --release; then
        printf '    tests: PASS\n'
    else
        test_rc=$?
        printf 'WARNING: test suite exited %s (Android-gating work may be incomplete).\n' "$test_rc" >&2
    fi
else
    printf '\n(tests skipped: --no-test)\n'
fi

rf_bin="$built_bin"
if (( do_install )); then
    printf '\n==> Installing to %s/bin/rustfetch\n' "$PREFIX"
    mkdir -p "$PREFIX/bin"
    if command -v install >/dev/null 2>&1; then
        install -m 0755 "$built_bin" "$PREFIX/bin/rustfetch"
    else
        cp "$built_bin" "$PREFIX/bin/rustfetch"
        chmod 0755 "$PREFIX/bin/rustfetch"
    fi
    rf_bin="$PREFIX/bin/rustfetch"
else
    printf '\n(install skipped: --no-install; smoke-testing the build output)\n'
fi

printf '\n==> Smoke test: %s\n' "$rf_bin"
printf -- '--- rustfetch --version ---\n'
"$rf_bin" --version || true
printf -- '--- rustfetch --pipe ---\n'
"$rf_bin" --pipe || true
printf -- '--- rustfetch --format json ---\n'
"$rf_bin" --format json || true

# --- Compare with Termux fastfetch ----------------------------------------
ff="${RUSTFETCH_FASTFETCH:-$(command -v fastfetch 2>/dev/null || true)}"
printf '\n==> Comparison: rustfetch --pipe vs fastfetch --pipe\n'
if [[ -z "$ff" ]]; then
    printf '(fastfetch not found; install it with: pkg install fastfetch)\n'
else
    scratch="$HOME/.cache/rustfetch-termux-smoke"
    mkdir -p "$scratch"
    "$rf_bin" --pipe > "$scratch/rustfetch.pipe" 2>/dev/null || true
    "$ff" --pipe > "$scratch/fastfetch.pipe" 2>/dev/null || true
    ff_ver="$("$ff" --version 2>/dev/null | head -n 1 || true)"
    printf '    rustfetch: %s\n' "$scratch/rustfetch.pipe"
    printf '    fastfetch: %s (%s)\n' "$scratch/fastfetch.pipe" "$ff_ver"
    printf 'NOTE: this device ships fastfetch 2.67.1, not the 2.71.0 used as the\n'
    printf '      parity reference. Structural differences are expected; a non-empty\n'
    printf '      diff is normal and is NOT a rustfetch regression.\n'
    if diff -u "$scratch/fastfetch.pipe" "$scratch/rustfetch.pipe" > "$scratch/diff.txt" 2>&1; then
        printf '    diff: pipe output is byte-identical (unexpected, but fine)\n'
    else
        printf '    diff: differences found; first 40 lines of %s:\n' "$scratch/diff.txt"
        head -n 40 "$scratch/diff.txt" || true
    fi
fi

printf '\nDone.\n'
exit "$test_rc"
