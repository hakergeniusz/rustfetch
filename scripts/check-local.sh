#!/usr/bin/env bash
# check-local.sh — verify the machine-local fat-LTO binary still works.
# Prints OK/BROKEN with reasons. Exit 0 = healthy, 1 = broken, 2 = usage error.
set -uo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/check-local.sh [OPTIONS]

  -h, --help       Show this help.
  -b, --binary PATH   Local binary (default: target/local/rustfetch).
  -r, --release PATH  Release binary to compare with (default: target/release/rustfetch).
      --quiet      Only print the final verdict.

Checks: exists/executable, --version, no crash (SIGILL/SIGSEGV), stdout parity
with the release binary in pipe mode, zsh completion file present if installed.
EOF
}

bin="target/local/rustfetch"
ref="target/release/rustfetch"
quiet=0
while (($# > 0)); do
  case "$1" in
    -h|--help) usage; exit 0 ;;
    -b|--binary) bin="${2:?--binary needs a path}"; shift 2 ;;
    -r|--release) ref="${2:?--release needs a path}"; shift 2 ;;
    --quiet) quiet=1; shift ;;
    *) printf 'Error: unknown option: %s\n\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
done

cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2
fail=0
say() { [ "$quiet" -eq 1 ] || printf '%s\n' "$*"; }

say "Checking local binary: $bin"
if [[ ! -f "$bin" ]]; then
  printf 'BROKEN: %s does not exist (rebuild with scripts/build-local.sh)\n' "$bin"; exit 1
fi
if [[ ! -x "$bin" ]]; then
  printf 'BROKEN: %s is not executable\n' "$bin"; exit 1
fi

ver="$("$bin" -v 2>&1)"; rc=$?
if [ "$rc" -ne 0 ]; then
  printf 'BROKEN: --version exited %s (likely SIGILL after a CPU change: rebuild locally)\n' "$rc"; exit 1
fi
say "  version: $ver"

out="$(mktemp "${TMPDIR:-/tmp}/rf-check.XXXXXX")"
err="$(mktemp "${TMPDIR:-/tmp}/rf-check.XXXXXX")"
"$bin" --pipe >"$out" 2>"$err"; rc=$?
case "$rc" in
  0) ;;
  132) printf 'BROKEN: SIGILL (illegal instruction) - the binary was built for a different CPU. Rebuild with scripts/build-local.sh\n'; exit 1 ;;
  139) printf 'BROKEN: SIGSEGV - rebuild with scripts/build-local.sh\n'; exit 1 ;;
  *) printf 'BROKEN: --pipe exited %s; stderr: %s\n' "$rc" "$(head -c 200 "$err")"; exit 1 ;;
esac

if [ -x "$ref" ]; then
  "$ref" --pipe >"$err" 2>/dev/null
  if cmp -s "$out" "$err"; then
    say "  output: identical to release"
  else
    say "  output: differs from release (volatile fields or version mismatch; not necessarily broken)"
  fi
fi

if [ -f "$HOME/.local/share/zsh/site-functions/_rustfetch" ]; then
  say "  completion: present"
else
  say "  completion: not installed in ~/.local/share/zsh/site-functions"
fi

printf 'OK: local binary is healthy\n'
exit 0
