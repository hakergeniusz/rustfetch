#!/usr/bin/env bash
# Compare rustfetch (release) against fastfetch, hyfetch and neofetch.
# Uses ABSOLUTE paths: ~/.local/bin/fastfetch and ~/.local/bin/hyfetch are
# user wrappers around rustfetch, so PATH lookup would not be a fair test.
# Neofetch is optional; install it to include it in the benchmark.
set -uo pipefail

RF="${RF:-/home/hakergeniusz/.local/bin/rustfetch}"
FF="${FF:-/usr/sbin/fastfetch}"
HY="${HY:-/usr/bin/hyfetch}"
# Neofetch was removed from Arch's repos (AUR only). Fall back to the
# upstream standalone shell script cached in RAM, run via bash.
NEO_PATH="$(command -v neofetch 2>/dev/null || true)"
if [[ -z "$NEO_PATH" && -f /dev/shm/rf-scratch/tool-compare/neofetch ]]; then
  NEO_PATH=/dev/shm/rf-scratch/tool-compare/neofetch
fi
NEO_CMD=""
if [[ -n "$NEO_PATH" ]]; then
  if [[ -x "$NEO_PATH" ]]; then
    NEO_CMD="$NEO_PATH"
  else
    NEO_CMD="bash $NEO_PATH"
  fi
fi
OUT="${OUT:-/dev/shm/rf-scratch/tool-compare}"
mkdir -p "$OUT"

command -v hyperfine >/dev/null || { echo "hyperfine required" >&2; exit 1; }

echo "=== Versions ==="
"$RF" -v
"$FF" --version
"$HY" --version 2>&1 | head -1
if [[ -n "$NEO_CMD" ]]; then eval "$NEO_CMD --version" 2>&1 | head -1; else echo "neofetch: NOT AVAILABLE (install it or cache the upstream script in RAM)"; fi

echo
echo "=== Output shape (pipe/non-interactive) ==="
printf '%-26s %8s %8s %8s\n' tool lines bytes colors
for spec in "rustfetch|$RF --pipe" "fastfetch|$FF --pipe" "hyfetch|$HY"; do
  name="${spec%%|*}"; cmd="${spec#*|}"
  out="$OUT/$name.out"
  eval "$cmd" > "$out" 2>/dev/null
  lines=$(wc -l < "$out")
  bytes=$(wc -c < "$out")
  colors=$(rg -o '38;[25];[0-9;]+' "$out" 2>/dev/null | sort -u | wc -l)
  printf '%-26s %8s %8s %8s\n' "$name" "$lines" "$bytes" "$colors"
done
if [[ -n "$NEO_CMD" ]]; then
  eval "$NEO_CMD" > "$OUT/neofetch.out" 2>/dev/null
  printf '%-26s %8s %8s %8s\n' neofetch "$(wc -l < "$OUT/neofetch.out")" "$(wc -c < "$OUT/neofetch.out")" "$(rg -o '38;[25];[0-9;]+' "$OUT/neofetch.out" | sort -u | wc -l)"
fi

echo
echo "=== Timing (hyperfine, 5 warmup / 20 runs) ==="
HF_ARGS=(--warmup 5 --runs 20 --export-markdown "$OUT/bench.md")
HF_CMD=("${RF} --pipe" "${FF} --pipe" "${HY}")
if [[ -n "$NEO_CMD" ]]; then HF_CMD+=("$NEO_CMD"); fi
hyperfine "${HF_ARGS[@]}" "${HF_CMD[@]}"
echo
echo "Markdown report: $OUT/bench.md"
