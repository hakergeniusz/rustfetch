# fastfetch output parity tests

`tests/fastfetch_parity.rs` checks rustfetch's parity mode byte-for-byte against
the exact upstream reference, **fastfetch 2.71.0**. Unlike the flag matrix,
which reports live-data differences as non-fatal, these tests fail on any byte
difference in stdout, stderr or exit status.

## Running locally

```sh
curl -fsSL -o /tmp/fastfetch.tar.gz \
  https://github.com/fastfetch-cli/fastfetch/releases/download/2.71.0/fastfetch-linux-amd64.tar.gz
mkdir -p /tmp/fastfetch-271
tar -xzf /tmp/fastfetch.tar.gz -C /tmp/fastfetch-271
export RUSTFETCH_FASTFETCH=/tmp/fastfetch-271/fastfetch-linux-amd64/usr/bin/fastfetch
cargo test --test fastfetch_parity
```

Without `RUSTFETCH_FASTFETCH`, the test looks for `/usr/sbin/fastfetch` and
`/usr/bin/fastfetch`. An explicitly set path is authoritative: there is no
silent fallback to a different system binary. The test verifies
`--version-raw` is exactly `2.71.0` before comparing anything. If no matching
reference is found, the tests print a skip message and pass, so native ports
and other platforms are unaffected.

The suite is gated to Linux, because byte-for-byte parity is a property of the
Linux build only. The Windows, macOS, FreeBSD and Termux builds have separate
smoke tests.

## What is compared

Every comparison runs both binaries with identical arguments and environment.
`NO_CONFIG=1` prevents an ambient config file from changing the output, and
rustfetch is launched with `RUSTFETCH_PARITY_VERSION=1`.

| Test | Coverage |
|---|---|
| `every_builtin_distro_logo_matches_in_pipe_mode` | All built-in logos, including the distro logos (Arch, Ubuntu, Debian, Fedora, Alpine, NixOS, etc.), in pipe mode |
| `environment_matrix_matches_in_pipe_and_json_modes` | 12 environments (locale, terminal, color depth, timezone, SSH, CI, desktop session, shell) in pipe and JSON modes |
| `distro_logo_color_overrides_match_across_environments` | `--logo-color-1`/`--logo-color-2` on five distro logos under four environments |
| `pty_output_matches_at_multiple_widths_and_terminals` | Raw PTY bytes at 20, 40, 80, 120 and 200 columns with xterm-256color, dumb, `NO_COLOR=1` and truecolor |
| `distro::distro_identities_match_in_pipe_and_json_modes` | 12 real distro identities (Ubuntu, Debian, Fedora, Arch, Alpine, Linux Mint, openSUSE, Gentoo, Void, NixOS, RHEL, Manjaro) in pipe and JSON |
| `distro::ubuntu_flavours_match_with_xdg_config_dirs` | Kubuntu, Xubuntu and Ubuntu MATE identities via `XDG_CONFIG_DIRS` |

The distro tests mount a tmpfs over `/etc` inside a private user namespace and
copy a fixture `os-release` into it, so both binaries see a fake distro instead
of the host. This exercises OS detection, distro logo selection and the
quirked identity that fastfetch applies after parsing os-release (for example
Ubuntu's pretty name uses `NAME + VERSION`, which carries the codename).
Fixtures live in `tests/fixtures/os-release/`. If `unshare -Urm` is unavailable
or forbidden, those tests print a skip message and pass; add the missing
fixture rather than weakening the assertion.

The comparisons use the deterministic `Separator` module and an explicit logo,
so live system values cannot drift between the two invocations. Terminal width
is set with `TIOCSWINSZ` on a real PTY, and output is read from the master until
the child exits; this exercises the same code path a user sees in a terminal.

## CI

The `fastfetch-parity` job in `.github/workflows/ci.yaml` downloads the pinned
2.71.0 Linux tarball from the upstream release, checks its version, and runs
only this test suite. No assumptions are made about the runner image's
fastfetch version.

## Adding a case

Add an entry to `ENVIRONMENTS` for a new environment variable combination, or
append a logo/color pair to the dedicated test. Keep to stable modules: a case
that prints uptime, temperatures or disk usage would be flaky even when both
binaries are correct. If a new case needs a different terminal size or a new
`TERM` value, extend the PTY test matrix rather than adding sleeps.
