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
run_tty "fraction flags" --structure localip --fraction-ndigits 3 --fraction-trailing-zeros always

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

# localip speed is the only default-format fraction consumer; this box's wlan0
# reports no speed, so the ≥1000 branches stay unit-test territory.
cat > /tmp/formattest-fraction.jsonc <<'EOF'
{
  "localip": { "showSpeed": true },
  "display": { "fraction": { "ndigits": 3, "trailingZeros": "always" } }
}
EOF
run_tty "fraction config localip" --config /tmp/formattest-fraction.jsonc -s localip

# --- uptime custom format --------------------------------------------------
# fastfetch.c:789 sets useJsonConfig = structure.length==0 && configDoc, so
# these must go through a real --config file with NO -s. A top-level
# {"uptime":{...}} object (or -c <(...)) is ignored by both binaries.
# The whole scratch set lives on tmpfs; override with RF_SCRATCH if needed.
RF_SCRATCH="${RF_SCRATCH:-/dev/shm/rf-scratch}"
mkdir -p "$RF_SCRATCH"
UPTIME_CFG="$RF_SCRATCH/formattest-uptime.jsonc"

run_uptime() { # <label> <format JSON literal> [extra args...]
    local label="$1" fmt="$2"; shift 2
    printf '{ "modules": [ { "type": "uptime", "format": %s } ] }\n' "$fmt" > "$UPTIME_CFG"
    run_tty "$label" --config "$UPTIME_CFG" --logo none "$@"
}

# Stable fields: days/hours only roll hourly; boot-time is fixed.
run_uptime "uptime days/hours" '"{days}d{hours}h"'
run_uptime "uptime boot-time" '"{boot-time}"'
run_uptime "uptime years-fraction" '"{years-fraction}"'
# Volatile fields — the run_tty re-run arbitration reports these as DRIFT
# when seconds move between the two binaries, not as a real break.
run_uptime "uptime minutes/seconds" '"{minutes}m{seconds}s"'

# Colors: TTY emits the SGR, --pipe drops it; both paths go through the
# format engine on a real module (ESC is masked to E by run_tty).
run_uptime "uptime color tty" '"{#red}{days}d{#}"'
run_uptime "uptime color codes tty" '"{#1;31}{days}d{#0}"'
run_uptime "uptime color pipe" '"{#red}{days}d{#}"' --pipe

# Padding / truncation / conditionals / constants / positionals / escape.
run_uptime "uptime pad left" '"[{days<6}h]"'
run_uptime "uptime pad right" '"[{days>6}h]"'
run_uptime "uptime pad center" '"[{days|6}h]"'
run_uptime "uptime truncate" '"{boot-time:5}"'
run_uptime "uptime pad-as-truncate" '"{boot-time<5}"'
run_uptime "uptime cond set" '"{?hours}H{?}"'
run_uptime "uptime cond unset" '"{?days}D{?}"'
run_uptime "uptime cond-not unset" '"{/days}N{/}"'
run_uptime "uptime cond-not set" '"{/hours}N{/}"'
run_uptime "uptime positional" '"{}-{0}-{1}"'
run_uptime "uptime escape" '"{{{days}}}"'

cat > "$UPTIME_CFG" <<'EOF'
{
  "display": { "constants": ["AAA", "BBB"] },
  "modules": [ { "type": "uptime", "format": "{$1}-{$2}-{$3}-{$0}-{$+1}" } ]
}
EOF
run_tty "uptime constants" --config "$UPTIME_CFG" --logo none

# Non-string and empty formats fall back to the module's default print.
run_uptime "uptime nonstring format" '5'
run_uptime "uptime empty format" '""'

# An invalid {#color} defers exit(479) (shell status 223) with a stderr message. Keep stdout on
# the PTY (redirecting it would auto-enable pipe and skip colors entirely)
# but split stderr, otherwise the two binaries' interleaving of the final
# reset and the error line differs even though stream contents match.
run_tty_deferred() { # <label> <config-path>
    local label="$1" cfg="$2"
    local fo="$RF_SCRATCH/uptime-ff.out" fe="$RF_SCRATCH/uptime-ff.err"
    local ro="$RF_SCRATCH/uptime-rs.out" re="$RF_SCRATCH/uptime-rs.err"
    local ao ae bo be ac bc
    script -qec "$FF --config $cfg --logo none 2>$fe" /dev/null >"$fo" 2>&1; ac=$?
    script -qec "$RS --config $cfg --logo none 2>$re" /dev/null >"$ro" 2>&1; bc=$?
    ao=$(sed 's/\x1b/E/g' "$fo"); ae=$(cat "$fe")
    bo=$(sed 's/\x1b/E/g' "$ro"); be=$(cat "$re")
    if [ "$ao" = "$bo" ] && [ "$ae" = "$be" ] && [ "$ac" = "$bc" ]; then
        echo "OK  deferred: $label ($ac)"
    else
        echo "DIFF deferred: $label [$ac]$ao/$ae vs [$bc]$bo/$be"
        FAIL=1
    fi
}
printf '{ "modules": [ { "type": "uptime", "format": "{#qq}{days}d" } ] }\n' > "$UPTIME_CFG"
run_tty_deferred "uptime invalid color" "$UPTIME_CFG"

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
run_err "fraction ndigits 10" '{"display":{"fraction":{"ndigits":10}}}'
run_err "fraction ndigits -2" '{"display":{"fraction":{"ndigits":-2}}}'
run_err "fraction trailingZeros bogus" '{"display":{"fraction":{"trailingZeros":"bogus"}}}'

if [ "$FAIL" -eq 0 ]; then
    echo "all scenarios matched"
else
    echo "DIFFs above need reading: live-data modules (top, cpuusage, memory …)"
    echo "drift between the two runs; a real break shows structural differences."
fi
exit "$FAIL"
