#!/usr/bin/env bash
# hyfetch is a DIFFERENT PROGRAM from fastfetch: pride-flag logo coloring is
# a rustfetch-only feature, so there is NO upstream fastfetch byte-parity
# target for it. The only parity obligation tested here is the UNSET path:
# with logo.hyfetch absent and no --hyfetch-* flags, rustfetch must stay
# byte-identical to fastfetch 2.71.0 in pipe, TTY and JSON output, including
# logo.color.N overrides. The second half checks the new flags/config errors
# exit with the documented rustfetch codes (477 config/CLI, 476 missing enum
# value, 478 unknown enum value; shells report those mod 256).
set -uo pipefail

FF="${FF:-/usr/sbin/fastfetch}"
RS="${RS:-$PWD/target/release/rustfetch}"
SCRATCH="${RF_SCRATCH:-/dev/shm/rf-scratch-hy}"
FAIL=0
mkdir -p "$SCRATCH"

quote() { printf '%q ' "$@"; }

# Both runs use the stable "Separator" module: no live data can drift.
compare() { # <label> <mode: pipe|tty|json> <args...>
    local label="$1" mode="$2"; shift 2
    local args cmd a b
    args=$(quote "$@")
    case "$mode" in
        pipe) a=$("$FF" $args 2>&1); b=$("$RS" $args 2>&1) ;;
        tty)  a=$(script -qec "$FF $args" /dev/null 2>&1); b=$(script -qec "$RS $args" /dev/null 2>&1) ;;
        json) a=$("$FF" $args 2>&1); b=$("$RS" $args 2>&1) ;;
    esac
    if [ "$a" = "$b" ]; then
        echo "OK   unset $label"
    else
        echo "DIFF unset $label"
        FAIL=1
    fi
}

expect_exit() { # <label> <expected-exit> <args...>
    local label="$1" expected="$2"; shift 2
    "$RS" "$@" >/dev/null 2>&1
    local code=$?
    if [ "$code" -eq "$expected" ]; then
        echo "OK   exit $label ($code)"
    else
        echo "FAIL exit $label: expected $expected, got $code"
        FAIL=1
    fi
}

cfg_error() { # <label> <config JSON> <expected-exit>
    local label="$1" body="$2" expected="$3"
    printf '%s\n' "$body" > "$SCRATCH/hyfetch-error.jsonc"
    expect_exit "$label" "$expected" -c "$SCRATCH/hyfetch-error.jsonc" --structure Separator
}

# --- UNSET parity: the feature must be completely inert --------------------
compare "pipe" pipe --structure Separator --logo arch --pipe
compare "tty" tty --structure Separator --logo arch
compare "json" json --structure Separator --logo arch --format json
compare "pipe logo colors" pipe --structure Separator --logo arch --pipe --logo-color-1 red --logo-color-2 '#00FF00'
compare "tty logo colors" tty --structure Separator --logo arch --logo-color-1 red --logo-color-2 '#00FF00'
compare "json logo colors" json --structure Separator --logo arch --format json --logo-color-1 red --logo-color-2 '#00FF00'

# A config that does not mention logo.hyfetch must be inert too.
UNSET_CFG="$SCRATCH/hyfetch-unset.jsonc"
cat > "$UNSET_CFG" <<'EOF'
{ "logo": { "type": "builtin", "source": "arch", "color": { "1": "red", "3": "blue" } }, "modules": [] }
EOF
compare "config pipe" pipe -c "$UNSET_CFG" --pipe
compare "config tty" tty -c "$UNSET_CFG"
compare "config json" json -c "$UNSET_CFG" --format json

# --- Enabled output: the new coloring is visible, and CLI wins per slot ----
# Colors need a TTY: --pipe drops every logo escape on both binaries.
tty_run() { script -qec "$*" /dev/null | sed 's/\x1b/E/g'; }
plain=$(tty_run "$RS --structure Separator --logo arch")
colored=$(tty_run "$RS --structure Separator --logo arch --hyfetch-preset transgender")
if [ "$plain" != "$colored" ] && [[ "$colored" == *'E[38;2;85;205;253'* ]]; then
    echo "OK   enabled preset colors the logo"
else
    echo "FAIL enabled preset did not color the logo"
    FAIL=1
fi
override=$(tty_run "$RS --structure Separator --logo arch --hyfetch-preset transgender --logo-color-1 '#FF0000'")
if [[ "$override" == *'E[38;2;255;0;0'* ]]; then
    echo "OK   logo-color-1 wins over hyfetch slot 1"
else
    echo "FAIL logo-color-1 did not win over hyfetch"
    FAIL=1
fi
ansi=$(tty_run "$RS --structure Separator --logo arch --hyfetch-preset transgender --hyfetch-mode 8bit")
if [[ "$ansi" == *'E[38;5;'* ]]; then
    echo "OK   --hyfetch-mode 8bit emits 38;5;N"
else
    echo "FAIL --hyfetch-mode 8bit did not emit 38;5;N"
    FAIL=1
fi
json_plain=$("$RS" --structure Separator --logo arch --format json)
json_colored=$("$RS" --structure Separator --logo arch --format json --hyfetch-preset transgender)
if [ "$json_colored" = "$json_plain" ]; then
    echo "OK   JSON drops logo escapes"
else
    echo "FAIL JSON kept logo escapes"
    FAIL=1
fi

# --- Error exits ------------------------------------------------------------
expect_exit "unknown preset flag" 221 --hyfetch-preset no-such-flag --structure Separator
expect_exit "bad lightness flag" 221 --hyfetch-lightness 1.5 --hyfetch-preset transgender
expect_exit "NaN lightness flag" 221 --hyfetch-lightness NaN --hyfetch-preset transgender
expect_exit "unknown mode flag" 222 --hyfetch-mode 24-bit --hyfetch-preset transgender
expect_exit "missing mode flag value" 220 --hyfetch-mode

cfg_error "unknown hyfetch key" '{"logo":{"hyfetch":{"preset":"rainbow","oops":1}}}' 221
cfg_error "non-object hyfetch" '{"logo":{"hyfetch":"rainbow"}}' 221
cfg_error "unknown config preset" '{"logo":{"hyfetch":{"preset":"nope"}}}' 221
cfg_error "bad lightness config" '{"logo":{"hyfetch":{"preset":"rainbow","lightness":2}}}' 221
cfg_error "NaN lightness config" '{"logo":{"hyfetch":{"preset":"rainbow","lightness":NaN}}}' 221
cfg_error "empty custom preset" '{"logo":{"hyfetch":{"preset":"myflag","customPresets":{"myflag":[]}}}}' 221
cfg_error "reserved custom name" '{"logo":{"hyfetch":{"preset":"random","customPresets":{"random":["#FFF"]}}}}' 221
cfg_error "bad custom hex" '{"logo":{"hyfetch":{"preset":"myflag","customPresets":{"myflag":["blue"]}}}}' 221

if [ "$FAIL" -eq 0 ]; then
    echo "hyfetchtest: PASS"
else
    echo "hyfetchtest: FAIL"
fi
exit "$FAIL"
