#!/usr/bin/env bash
# Build rustfetch tuned for THIS machine only. See docs/local-build.md.
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/build-local.sh [OPTIONS]

Build an aggressively optimized rustfetch for the machine running the build.

Options:
  -h, --help     Show this help and exit.
      --no-bench Skip the hyperfine benchmark after building.

Environment:
  RUSTFETCH_LOCAL_FLAGS  Replaces the default RUSTFLAGS for this build.
                         Default: -C target-cpu=native
                         Include -C target-cpu=native yourself if you override it.
  RUSTFETCH_BENCH_ARGS   Extra argv passed identically to both benchmarked
                         binaries (e.g. "--local-only").
  CARGO_TARGET_DIR       Cargo target directory (default: target).
                         The binary is written to <target-dir>/local/rustfetch.

NOT PORTABLE: by default the binary contains instructions for the exact CPU
it was built on. Copying it to another machine (or a VM/migration host) can
crash with SIGILL. Build a separate binary on each target machine.

EOF
}

no_bench=0
while (($# > 0)); do
    case "$1" in
        -h|--help) usage; exit 0 ;;
        --no-bench) no_bench=1; shift ;;
        *) printf 'Error: unknown option: %s\n\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
done

cd "$(dirname "${BASH_SOURCE[0]}")/.."
target_dir="${CARGO_TARGET_DIR:-target}"
local_bin="$target_dir/local/rustfetch"
release_bin="$target_dir/release/rustfetch"

default_flags='-C target-cpu=native'
rustflags="${RUSTFETCH_LOCAL_FLAGS-$default_flags}"

printf '==> Building machine-local rustfetch\n'
printf '    profile: local (opt-level=3, fat LTO, 1 codegen unit, no debug/incremental)\n'
printf '    RUSTFLAGS: %s\n' "$rustflags"
printf '    NOTE: not portable - native CPU instructions are baked in.\n'

RUSTFLAGS="$rustflags" cargo build --profile local --locked

if [[ ! -x "$local_bin" ]]; then
    printf 'Error: expected binary at %s was not produced\n' "$local_bin" >&2
    exit 1
fi

printf '\n==> Binary\n'
printf '    %s\n' "$local_bin"
if command -v stat >/dev/null 2>&1 && stat -c %s "$local_bin" >/dev/null 2>&1; then
    printf '    size: %s bytes\n' "$(stat -c %s "$local_bin")"
else
    printf '    size: %s bytes\n' "$(wc -c < "$local_bin" | tr -d '[:space:]')"
fi
{ du -h "$local_bin" 2>/dev/null || true; } | sed 's/^/    on disk: /'

if (( no_bench )); then
    printf '\n(benchmark skipped: --no-bench)\n'
    exit 0
fi

if ! command -v hyperfine >/dev/null 2>&1; then
    printf '\n(benchmark skipped: hyperfine not found)\n'
    exit 0
fi
if [[ ! -x "$release_bin" ]]; then
    printf '\n(benchmark skipped: %s not found; run cargo build --locked --release first)\n' "$release_bin"
    exit 0
fi

# Use the same argv for both binaries so the comparison measures the build,
# not different module sets. --local-only is used only when both binaries
# understand it (an older release binary may predate the flag).
bench_args=()
if [[ -n "${RUSTFETCH_BENCH_ARGS+x}" ]]; then
    read -r -a bench_args <<< "$RUSTFETCH_BENCH_ARGS"
elif "$local_bin" --local-only >/dev/null 2>&1 && "$release_bin" --local-only >/dev/null 2>&1; then
    bench_args=(--local-only)
fi

quote_cmd() {
    local quoted
    printf -v quoted '%q ' "$@"
    printf '%s' "${quoted% }"
}

local_cmd="$(quote_cmd "$local_bin" "${bench_args[@]}")"
release_cmd="$(quote_cmd "$release_bin" "${bench_args[@]}")"

printf '\n==> hyperfine: local vs release'
if ((${#bench_args[@]})); then
    printf ' (argv: %s)\n' "${bench_args[*]}"
else
    printf ' (default argv)\n'
fi

hyperfine --warmup 5 --runs 30 \
    --command-name 'local' "$local_cmd" \
    --command-name 'release' "$release_cmd"

printf '\nDone. Remember: this binary is machine-local only (native CPU instructions).\n'
