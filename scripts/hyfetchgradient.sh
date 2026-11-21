#!/usr/bin/env bash
# HyFetch per-line gradient smoke test. With a HyFetch preset the arch logo
# must show the whole flag (one palette slot per line), not the single color
# fastfetch's `$1`/`$2` placeholders happen to recycle. The unset path must
# stay byte-identical to fastfetch 2.71.0 — the feature is fully inert when
# neither --hyfetch-* nor logo.hyfetch is present.
set -uo pipefail

FF="${FF:-/usr/sbin/fastfetch}"
RS="${RS:-${CARGO_TARGET_DIR:-$PWD/target}/release/rustfetch}"
[ -x "$RS" ] || RS="$(command -v rustfetch)"
SCRATCH="${RF_SCRATCH:-/dev/shm/rf-scratch-gradient}"
FAIL=0
mkdir -p "$SCRATCH"

quote() { printf '%q ' "$@"; }

# Byte-for-byte parity of the UNSET path in pipe and tty mode. Separator is
# a static structure, so no live data can drift between the two binaries.
compare() { # <label> <mode: pipe|tty> <args...>
    local label="$1" mode="$2"; shift 2
    local args a b
    args=$(quote "$@")
    case "$mode" in
        pipe) a=$("$FF" $args 2>&1); b=$("$RS" $args 2>&1) ;;
        tty)  a=$(script -qec "$FF $args" /dev/null 2>&1); b=$(script -qec "$RS $args" /dev/null 2>&1) ;;
    esac
    if [ "$a" = "$b" ]; then
        echo "OK   unset $label"
    else
        echo "DIFF unset $label"
        FAIL=1
    fi
}

# Unique truecolor SGR codes emitted by a pty run. `script` provides the
# terminal hyfetch coloring needs; ESC is made printable for rg/sort.
truecolors() { # <pty command string>
    script -qec "$1" /dev/null 2>&1 | sed 's/\x1b/E/g' \
        | rg -o 'E\[38;2;[0-9]+;[0-9]+;[0-9]+' | sort -u
}

# A config that never mentions logo.hyfetch must be inert as well.
UNSET_CFG="$SCRATCH/hyfetch-unset.jsonc"
cat > "$UNSET_CFG" <<'EOF'
{ "logo": { "type": "builtin", "source": "arch", "color": { "1": "red", "3": "blue" } }, "modules": [] }
EOF

compare "pipe" pipe --structure Separator --logo arch --pipe
compare "tty" tty --structure Separator --logo arch
compare "config pipe" pipe -c "$SCRATCH/hyfetch-unset.jsonc" --pipe

# --- Enabled: every flag color is present -----------------------------------
rainbow=$(truecolors "$RS --hyfetch-preset rainbow --logo arch -s os")
rainbow_count=$(printf '%s\n' "$rainbow" | sed '/^$/d' | wc -l)
trans=$(truecolors "$RS --hyfetch-preset transgender --logo arch -s os")
trans_count=$(printf '%s\n' "$trans" | sed '/^$/d' | wc -l)
echo "INFO rainbow distinct colors: $rainbow_count"
echo "INFO transgender distinct colors: $trans_count"
if [ "$rainbow_count" -ge 6 ]; then
    echo "OK   rainbow gradient >= 6 colors"
else
    echo "FAIL rainbow gradient: only $rainbow_count colors"
    FAIL=1
fi
# Oracle hyfetch -p rainbow -m rgb --distro Arch uses exactly these six.
for code in "255;100;83" "255;141;0" "255;238;0" "4;191;53" "105;154;255" "229;77;255"; do
    if [[ "$rainbow" == *"38;2;$code"* ]]; then
        echo "OK   rainbow has $code"
    else
        echo "FAIL rainbow missing $code"
        FAIL=1
    fi
done
if [ "$trans_count" -ge 3 ]; then
    echo "OK   transgender gradient >= 3 colors"
else
    echo "FAIL transgender gradient: only $trans_count colors"
    FAIL=1
fi
for code in "85;205;253" "246;170;183" "255;255;255"; do
    if [[ "$trans" == *"38;2;$code"* ]]; then
        echo "OK   transgender has $code"
    else
        echo "FAIL transgender missing $code"
        FAIL=1
    fi
done

if [ "$FAIL" -eq 0 ]; then
    echo "hyfetchgradient: PASS"
else
    echo "hyfetchgradient: FAIL"
fi
exit "$FAIL"
