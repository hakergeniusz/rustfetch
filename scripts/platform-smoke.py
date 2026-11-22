#!/usr/bin/env python3
"""platform-smoke.py -- post-build smoke tests for a rustfetch binary.

Runnable on any host with a Python 3 interpreter; no third-party dependencies.

Usage:
    python3 scripts/platform-smoke.py --bin /path/to/rustfetch
    python3 scripts/platform-smoke.py --bin /path/to/rustfetch.exe \
        --json-out smoke-report.json

If --bin is omitted, the RF_BIN environment variable is used.

Expected binary names are ``rustfetch.exe`` (Windows) and ``rustfetch``
(macOS/Linux). The checks are the same as in platform-smoke.sh, so the two
front-ends can be used interchangeably in CI.

Exit codes: 0 = all checks passed, 1 = one or more checks failed,
            2 = usage / binary / report error.

All text emitted by the tested binary is treated strictly as data.
"""

import argparse
import json
import os
import re
import subprocess
import sys

ANSI_CSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]")
ANSI_OSC = re.compile(r"\x1b\][^\x07]*\x07")


def decode(raw: bytes) -> str:
    """Decode binary output lossily so bad bytes never crash the checks."""
    return raw.decode("utf-8", errors="replace")


def strip_ansi(text: str) -> str:
    """Remove ANSI/VT color and OSC sequences from a string."""
    return ANSI_OSC.sub("", ANSI_CSI.sub("", text))


class Smoke:
    def __init__(self, binary: str, min_modules: int, min_fields: int,
                 timeout: float, strict_help_hyfetch: bool = False):
        self.binary = binary
        self.min_modules = min_modules
        self.min_fields = min_fields
        self.timeout = timeout
        self.strict_help_hyfetch = strict_help_hyfetch
        self.checks = []

    def run(self, *args: str):
        """Return (exit_code, stdout_bytes, stderr_bytes) for one invocation."""
        try:
            proc = subprocess.run(
                [self.binary, *args],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=self.timeout,
                check=False,
            )
        except subprocess.TimeoutExpired:
            return None, b"", b""
        except OSError as exc:
            raise RuntimeError(f"could not execute {self.binary!r}: {exc}") from exc
        return proc.returncode, proc.stdout, proc.stderr

    def record(self, name: str, passed: bool, detail: str) -> None:
        self.checks.append(
            {"name": name, "passed": bool(passed), "detail": detail}
        )
        status = "PASS" if passed else "FAIL"
        print(f"  [{status}] {name}: {detail}")

    def check_version(self) -> None:
        rc, out, err = self.run("--version")
        if rc is None:
            return self.record("version", False, "timed out")
        combined = strip_ansi(decode(out) + "\n" + decode(err))
        if rc != 0:
            return self.record("version", False, f"exit code {rc}")
        if re.search(r"rustfetch[^0-9]*[0-9]+\.[0-9]+", combined, re.IGNORECASE):
            return self.record("version", True, "reports rustfetch and a version")
        return self.record("version", False, "no '<rustfetch> <version>' line")

    def check_help(self) -> None:
        rc, out, err = self.run("--help")
        if rc is None:
            return self.record("help", False, "timed out")
        combined = strip_ansi(decode(out) + "\n" + decode(err))
        if rc != 0:
            return self.record("help", False, f"exit code {rc}")
        missing = [f for f in ("--pipe", "--format", "--logo")
                   if f not in combined]
        note = ""
        if "--hyfetch-preset" not in combined:
            if self.strict_help_hyfetch:
                missing.append("--hyfetch-preset")
            else:
                # rustfetch-only flags are intentionally omitted from the
                # verbatim fastfetch help. Verify the flag is accepted
                # instead; --strict-help-hyfetch requires it literally.
                probe_rc, _, _ = self.run("--hyfetch-preset", "rainbow",
                                          "--logo", "none")
                if probe_rc == 0:
                    note = " (--hyfetch-preset verified by probe)"
                else:
                    missing.append("--hyfetch-preset")
        if missing:
            return self.record("help", False,
                               "missing option(s): " + ", ".join(missing))
        return self.record("help", True, "all expected options present" + note)

    def check_list_modules(self) -> None:
        rc, out, err = self.run("--list-modules")
        if rc is None:
            return self.record("list-modules", False, "timed out")
        text = strip_ansi(decode(out) + "\n" + decode(err))
        numbered = len(re.findall(r"^\s*\d+[).]\s+\w", text, re.MULTILINE))
        bare = len(re.findall(r"^\s*\w[^:]{0,40}\s:\s", text, re.MULTILINE))
        # Bare module IDs (Windows "title", macOS "title   User@Host").
        ids = len(re.findall(r"^\s*\w[\w-]*(?:\s|$)", text, re.MULTILINE))
        count = max(numbered, bare, ids)
        if rc != 0:
            return self.record("list-modules", False, f"exit code {rc}")
        if count < self.min_modules:
            return self.record(
                "list-modules", False,
                f"found {count} named modules, need >= {self.min_modules}")
        return self.record("list-modules", True, f"{count} named modules")

    def check_default_run(self) -> None:
        rc, out, err = self.run()
        if rc is None:
            return self.record("default-run", False, "timed out")
        text = strip_ansi(decode(out))
        # Unanchored: a text logo can share a line with the field, e.g.
        # "   `ooo/   OS: Arch Linux".
        kv = re.compile(r"[A-Za-z][A-Za-z0-9 ()/.-]{0,40}:[ \t]+[A-Za-z0-9/]")
        kv_alt = re.compile(r"[A-Za-z][A-Za-z0-9 ()/.-]{0,40}[ \t]{2,}[A-Za-z0-9/]")
        def count_lines(pattern):
            return sum(1 for line in text.splitlines() if pattern.search(line))

        count = count_lines(kv)
        if count < self.min_fields:
            count = max(count, count_lines(kv_alt))
        if rc != 0:
            return self.record("default-run", False, f"exit code {rc}")
        if count < self.min_fields:
            return self.record(
                "default-run", False,
                f"found {count} Key: value lines, need >= {self.min_fields}")
        return self.record("default-run", True, f"{count} Key: value lines")

    def check_pipe(self) -> None:
        rc, out, _err = self.run("--pipe")
        if rc is None:
            return self.record("pipe", False, "timed out")
        if rc != 0:
            return self.record("pipe", False, f"exit code {rc}")
        if not out.strip():
            return self.record("pipe", False, "produced no output")
        esc = out.count(b"\x1b")
        if esc:
            return self.record("pipe", False, f"{esc} ESC byte(s) in output")
        return self.record("pipe", True, "no ESC bytes")

    def check_json(self) -> None:
        rc, out, _err = self.run("--format", "json")
        if rc is None:
            return self.record("json", False, "timed out")
        if rc != 0:
            return self.record("json", False, f"exit code {rc}")
        if not out.strip():
            return self.record("json", False, "empty output")
        try:
            json.loads(decode(out))
        except (ValueError, TypeError) as exc:
            return self.record("json", False, f"output is not valid JSON ({exc})")
        return self.record("json", True, "valid JSON")

    def check_hyfetch(self) -> None:
        # A TTY is usually unavailable in CI, so -- per the spec -- we verify
        # exit status (and absence of a crash message) instead of a PTY.
        rc, out, err = self.run("--hyfetch-preset", "rainbow", "--logo", "none")
        if rc is None:
            return self.record("hyfetch-preset", False, "timed out")
        if rc != 0:
            return self.record("hyfetch-preset", False, f"exit code {rc}")
        if re.search(r"panic|segmentation fault|unreachable code",
                     decode(err), re.IGNORECASE):
            return self.record("hyfetch-preset", False, "crash message on stderr")
        return self.record("hyfetch-preset", True, "no crash")

    def run_all(self) -> None:
        labels = (
            ("[1/7] --version", self.check_version),
            ("[2/7] --help", self.check_help),
            ("[3/7] --list-modules", self.check_list_modules),
            ("[4/7] default run", self.check_default_run),
            ("[5/7] --pipe", self.check_pipe),
            ("[6/7] --format json", self.check_json),
            ("[7/7] --hyfetch-preset rainbow", self.check_hyfetch),
        )
        for label, check in labels:
            print(label)
            check()

    @property
    def passed(self) -> int:
        return sum(1 for c in self.checks if c["passed"])

    @property
    def failed(self) -> int:
        return len(self.checks) - self.passed

    def report(self) -> dict:
        return {
            "binary": self.binary,
            "checks": self.checks,
            "passed": self.passed,
            "failed": self.failed,
            "all_passed": self.failed == 0,
        }


def parse_args(argv):
    parser = argparse.ArgumentParser(
        description="Smoke-test a built rustfetch binary (Windows/macOS/Linux)."
    )
    parser.add_argument(
        "--bin", default=os.environ.get("RF_BIN"),
        help="path to the rustfetch binary (default: $RF_BIN)",
    )
    parser.add_argument(
        "--json-out", metavar="REPORT",
        help="write a machine-readable JSON report to REPORT",
    )
    parser.add_argument(
        "--min-modules", type=int, default=8,
        help="minimum named modules from --list-modules (default: 8)",
    )
    parser.add_argument(
        "--min-fields", type=int, default=8,
        help="minimum Key: value lines from a default run (default: 8)",
    )
    parser.add_argument(
        "--timeout", type=float, default=60.0,
        help="per-invocation timeout in seconds (default: 60)",
    )
    parser.add_argument(
        "--strict-help-hyfetch", action="store_true",
        help="require --hyfetch-preset literally in --help (it is a "
             "rustfetch-only flag normally omitted from the verbatim help)",
    )
    return parser.parse_args(argv)


def main(argv=None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)

    if not args.bin:
        print("ERROR: no binary given; pass --bin PATH or set RF_BIN", file=sys.stderr)
        return 2
    if not os.path.isfile(args.bin):
        print(f"ERROR: binary not found: {args.bin}", file=sys.stderr)
        return 2
    if not os.access(args.bin, os.X_OK):
        print(f"ERROR: binary is not executable: {args.bin}", file=sys.stderr)
        return 2

    print("rustfetch platform smoke tests")
    print(f"Binary: {args.bin}\n")

    smoke = Smoke(args.bin, args.min_modules, args.min_fields, args.timeout,
                  args.strict_help_hyfetch)
    try:
        smoke.run_all()
    except RuntimeError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2

    print("\n=== Summary ===")
    print(f"Passed: {smoke.passed}   Failed: {smoke.failed}")
    result = smoke.report()
    if smoke.failed == 0:
        print("RESULT: PASS")
    else:
        print(f"RESULT: FAIL ({smoke.failed} check(s) failed)")

    if args.json_out:
        try:
            with open(args.json_out, "w", encoding="utf-8") as handle:
                json.dump(result, handle, indent=2)
                handle.write("\n")
            print(f"Report written to: {args.json_out}")
        except OSError as exc:
            print(f"ERROR: cannot write report {args.json_out!r}: {exc}",
                  file=sys.stderr)
            return 2

    return 0 if smoke.failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
