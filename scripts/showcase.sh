#!/usr/bin/env bash
# showcase.sh — render examples/showcase.jsonc with colors into the terminal.
# When stdout is redirected, a pseudo-TTY is allocated via script(1) so the
# binary still emits ANSI escapes, and the pty's trailing CRs are stripped.

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/showcase.sh [OPTIONS]

Render rustfetch with examples/showcase.jsonc into the current terminal,
preserving ANSI colors. If stdout is a TTY the binary is exec'd directly;
if stdout is redirected a pseudo-TTY is used so colors are not stripped.

Options:
  -h, --help       Show this help and exit.
      --out PATH   Write the colored output (ANSI escapes preserved) to PATH.
      --png PATH   Same as --out; the file is an ANSI typescript you can turn
                   into a screenshot, e.g.:
                     cat PATH                  # replay in a terminal, screenshot it
                     ansi2png PATH > out.png   # if ansi2png is installed

Environment:
  RUSTFETCH_BIN    Override the rustfetch binary path.

Run from the repository root: the config path is relative to it.
EOF
}

out_file=
while (($#)); do
  case $1 in
    -h|--help) usage; exit 0 ;;
    --out|--png)
      if (($# < 2)); then
        printf 'error: %s requires a PATH\n' "$1" >&2
        usage >&2
        exit 2
      fi
      out_file=$2
      shift 2
      ;;
    *)
      printf 'error: unknown option: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

# --- locate the binary -----------------------------------------------------
if [[ -n ${RUSTFETCH_BIN:-} ]]; then
  bin=$RUSTFETCH_BIN
elif [[ -x ./target/release/rustfetch ]]; then
  bin=./target/release/rustfetch
elif bin=$(command -v rustfetch 2>/dev/null); then
  :
else
  printf 'error: rustfetch not found; set RUSTFETCH_BIN or run cargo build --release\n' >&2
  exit 127
fi

config=examples/showcase.jsonc
cmd=("$bin" --config "$config")

# --- helpers ---------------------------------------------------------------
# The pty translates LF to CRLF (ONLCR); drop that trailing CR on each line.
strip_cr() {
  if command -v sed >/dev/null 2>&1; then
    sed 's/\r$//'
  else
    tr -d '\r'
  fi
}

# Run through a pty so rustfetch sees a TTY and emits colors.
run_pty() {
  if ! command -v script >/dev/null 2>&1; then
    # Portable fallback: no pty allocator available, colors may be absent.
    printf 'warning: script(1) not found; colors cannot be forced\n' >&2
    "${cmd[@]}"
    return
  fi

  local cmd_str
  printf -v cmd_str '%q ' "${cmd[@]}"

  if script -qec true /dev/null >/dev/null 2>&1; then
    # Linux / util-linux script(1): -q quiet, -e exit status, -c command.
    # /dev/null is the typescript; stdout carries the captured output.
    script -qec "$cmd_str" /dev/null | strip_cr
  else
    # macOS / BSD script(1): script -q typescript command [args...]
    script -q /dev/null "${cmd[@]}" | strip_cr
  fi
}

# --- act -------------------------------------------------------------------
if [[ -n $out_file ]]; then
  run_pty > "$out_file"
  printf 'Wrote colored output to %s\n' "$out_file" >&2
  exit 0
fi

if [[ -t 1 ]]; then
  exec "$bin" --config "$config"
fi

run_pty
