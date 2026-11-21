#!/usr/bin/env bash
# Compares rustfetch against the installed fastfetch (2.71.0) across the
# format-engine scenarios: pipe/TTY/JSON output, the --percent-*/--temp-*--
# bar-* flags, and the display + module-level JSON config paths.
#
# Modules sampling live data (top, cpuusage, memory, swap, disk …) drift
# between the two runs; a diff there must be read, not trusted. Sound and
# bluetooth print nothing without hardware; they exercise the empty paths.
set -uo pipefail

FF="${FF:-/usr/sbin/fastfetch}"
RS="${RS:-$PWD/target/release/rustfetch}"
FAIL=0

run_tty() { # <label> <args...>
    local label="$1"; shift
    local a a2 b
    a=$(script -qec "$FF $*" /dev/null 2>&1 | sed 's/\x1b/E/g')
    b=$(script -qec "$RS $*" /dev/null 2>&1 | sed 's/\x1b/E/g')
    if [ "$a" = "$b" ]; then
        echo "OK  $label"
        return
    fi
    # Upstream disagreeing with itself means live data moved between the
    # runs; only a self-consistent upstream makes the diff a real break.
    a2=$(script -qec "$FF $*" /dev/null 2>&1 | sed 's/\x1b/E/g')
    if [ "$a" != "$a2" ]; then
        echo "DRIFT $label"
    else
        echo "DIFF $label"
        FAIL=1
    fi
}

run_err() { # <label> <config-string>
    local label="$1" config="$2"
    printf '%s' "$config" > /tmp/formattest.jsonc
    local ao ac bo bc
    ao=$("$FF" --config /tmp/formattest.jsonc -s memory 2>&1 >/dev/null); ac=$?
    bo=$("$RS" --config /tmp/formattest.jsonc -s memory 2>&1 >/dev/null); bc=$?
    if [ "$ao" = "$bo" ] && [ "$ac" = "$bc" ]; then
        echo "OK  err: $label ($ac)"
    else
        echo "DIFF err: $label [$ac]$ao vs [$bc]$bo"
        FAIL=1
    fi
}

# --- pipe / TTY / JSON per module -----------------------------------------
MODULES=(sound top wifi media gpu battery cpu physicaldisk brightness memory
         swap disk btrfs bluetooth gamepad cpuusage)
for m in "${MODULES[@]}"; do
    run_tty "pipe $m" --pipe --structure "$m"
    run_tty "json $m" --pipe --structure "$m" --format json
done

# --- CLI flag scenarios ----------------------------------------------------
run_tty "bar chars+width" --structure battery --bar-char-elapsed ● --bar-char-total - --bar-width 20
run_tty "percent bar" --structure memory --percent-type bar
run_tty "percent num" --structure memory --percent-type num
run_tty "percent mono" --structure battery --percent-type num-color bar-monochrome
run_tty "percent hide-others" --structure sound --percent-type hide-others
run_tty "temp fahrenheit" --structure cpu --temp-unit fahrenheit --temp-ndigits 2 --temp-space-before-unit always
run_tty "temp kelvin tight" --structure gpu --temp-unit kelvin --temp-space-before-unit never
run_tty "percent padding" --structure memory --percent-ndigits 1 --percent-width 5 --percent-space-before-unit always
run_tty "bar border-as-value" --structure battery --bar-border-left-elapsed '<' --bar-border-right-elapsed '>' --percent-type bar
run_tty "bar colors" --structure swap --bar-color-elapsed red --bar-color-total blue --bar-color-border green
run_tty "bar mixed flags" --structure disk --bar-char-elapsed '#' --bar-char-total '.' --bar-width 8 --percent-type bar num

# --- JSON config: display + module level -----------------------------------
cat > /tmp/formattest-full.jsonc <<'EOF'
{
  "display": {
    "percent": { "type": ["num", "bar", "bar-monochrome"], "green": 30, "yellow": 60, "ndigits": 2, "width": 8, "spaceBeforeUnit": "always" },
    "temp": { "unit": "fahrenheit", "ndigits": 2, "color": { "green": "magenta", "yellow": "cyan", "red": "blue" }, "spaceBeforeUnit": "always" },
    "bar": { "char": { "elapsed": "●", "total": "·" }, "border": { "left": "«", "right": "»", "leftElapsed": "[", "rightElapsed": "]" }, "width": 12, "color": { "elapsed": "magenta", "total": "cyan", "border": "yellow" } }
  }
}
EOF
run_tty "display config memory" --config /tmp/formattest-full.jsonc -s memory
run_tty "display config battery" --config /tmp/formattest-full.jsonc -s battery
run_tty "display config gpu" --config /tmp/formattest-full.jsonc -s gpu

cat > /tmp/formattest-modules.jsonc <<'EOF'
{
  "memory": { "percent": { "type": ["bar", "num"], "green": 30, "yellow": 90, "ndigits": 1 } },
  "sound": { "percent": { "type": ["bar", "num", "num-color"] } },
  "top": { "percent": { "type": ["num", "num-color"] } },
  "wifi": { "percent": { "type": ["bar", "num", "num-color"] } },
  "gpu": { "percent": { "type": ["num", "bar"] }, "temp": { "green": 30, "yellow": 50 } },
  "battery": { "percent": { "type": ["bar", "num", "num-color"] }, "temp": true },
  "cpu": { "temp": true },
  "physicaldisk": { "temp": true }
}
EOF
run_tty "module config group1" --config /tmp/formattest-modules.jsonc -s memory,sound,wifi
run_tty "module config group2" --config /tmp/formattest-modules.jsonc -s gpu,battery,cpu,physicaldisk

# --- config error paths (stderr + exit code) -------------------------------
run_err "bar legacy char" '{"display":{"bar":{"charElapsed":"x"}}}'
run_err "bar legacy border" '{"display":{"bar":{"borderRight":"x"}}}'
run_err "percent type bogus" '{"display":{"percent":{"type":"bogus"}}}'
run_err "percent type flag unknown" '{"display":{"percent":{"type":["nope"]}}}'
run_err "temp ndigits 12" '{"display":{"temp":{"ndigits":12}}}'
run_err "temp ndigits -1" '{"display":{"temp":{"ndigits":-1}}}'
run_err "bar non-object" '{"display":{"bar":123}}'
run_err "color invalid" '{"display":{"percent":{"color":{"green":"bogus"}}}}'
run_err "module percent non-obj" '{"memory":{"percent":5}}'
run_err "module temp bool" '{"memory":{"temp":true}}'

if [ "$FAIL" -eq 0 ]; then
    echo "all scenarios matched"
else
    echo "DIFFs above need reading: live-data modules (top, cpuusage, memory …)"
    echo "drift between the two runs; a real break shows structural differences."
fi
exit "$FAIL"
