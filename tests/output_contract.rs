//! Output-shape contract tests: assertions that hold on any machine,
//! independent of the live system values rustfetch reports.

mod support;

use support::{
    expand_cursor_columns, is_version_line, is_version_raw, module_keys, module_lines, run, run_ok,
    split_module_line, strip_ansi,
};

const DEFAULT_STRUCTURE: &str = "Title:Separator:OS:Host:Kernel:Uptime:Packages:Shell:Display:DE:WM:WMTheme:Theme:Icons:Font:Cursor:Terminal:TerminalFont:CPU:GPU:Memory:Swap:Disk:LocalIp:Battery:PowerAdapter:Locale:Break:Colors";

fn rendered_rows(output: &str) -> Vec<&str> {
    output.lines().filter(|line| !line.trim().is_empty()).collect()
}

fn listed_module_name(line: &str) -> Option<&str> {
    let (prefix, _) = line.split_once(':')?;
    let name = match prefix.split_once(')') {
        Some((_, rest)) => rest,
        None => prefix,
    };
    let name = name.trim();
    (!name.is_empty()).then_some(name)
}

#[test]
fn pipe_has_no_ansi_and_matches_non_tty() {
    // A structure without the Colors module keeps every row free of control
    // sequences; the Colors module deliberately prints bare color rows.
    let piped = run_ok(&["--pipe", "--structure", "OS:Kernel", "--logo", "none"]);
    let plain = run_ok(&["--structure", "OS:Kernel", "--logo", "none"]);

    assert_eq!(piped.stdout_bytes, plain.stdout_bytes, "pipe and non-TTY stdout diverged");
    assert!(!piped.stdout().contains("\x1b["), "pipe stdout contains a CSI sequence");
    assert!(!piped.stdout().contains("\x1b]"), "pipe stdout contains an OSC sequence");
}

#[test]
fn pipe_module_lines_are_key_value_pairs() {
    let output = run_ok(&["--pipe", "--logo", "none"]);
    let lines = module_lines(output.stdout());
    assert!(!lines.is_empty(), "expected at least one module row");

    for line in lines {
        let (key, _) = match split_module_line(line) {
            Some(pair) => pair,
            None => panic!("module row is not `key: value` with an exact \": \" separator: {line:?}"),
        };
        assert!(!key.is_empty(), "empty key in {line:?}");
        assert!(!key.contains('\x1b'), "ANSI escape in key: {key:?}");
        assert_eq!(strip_ansi(key), key, "key changed after ANSI stripping: {key:?}");
    }
}

#[test]
fn key_column_alignment_is_consistent() {
    let output = run_ok(&[
        "--pipe",
        "--logo",
        "none",
        "--key-width",
        "20",
        "--structure",
        "OS:Kernel:Uptime",
    ]);
    let rows = rendered_rows(output.stdout());
    assert_eq!(rows.len(), 3, "expected exactly three module rows: {:?}", rows);

    for row in rows {
        assert_eq!(row.matches("\x1b[20G").count(), 1, "unexpected cursor jump in {row:?}");
        let expanded = expand_cursor_columns(row);
        assert!(expanded.len() > 20, "row too short for a 20-byte value column: {row:?}");
        let value = &expanded[20..];
        assert!(!value.trim().is_empty(), "no value at byte offset 20 in {row:?}");
    }
}

#[test]
fn structure_selection_and_disabled_modules() {
    let only_os = run_ok(&["--pipe", "--logo", "none", "--structure", "OS"]);
    assert_eq!(module_keys(only_os.stdout()).as_slice(), &["OS"]);
    assert!(!only_os.stdout().contains("Kernel:"), "unselected Kernel row was printed");

    let two = run_ok(&["--pipe", "--logo", "none", "--structure", "OS:Kernel"]);
    assert_eq!(module_keys(two.stdout()).as_slice(), &["OS", "Kernel"]);

    let disabled = run_ok(&[
        "--pipe",
        "--logo",
        "none",
        "--structure",
        "OS:Kernel:Uptime",
        "--structure-disabled",
        "Kernel",
    ]);
    assert_eq!(module_keys(disabled.stdout()).as_slice(), &["OS", "Uptime"]);
    assert!(!disabled.stdout().contains("Kernel:"), "disabled Kernel row was printed");
}

#[test]
fn list_and_print_flags_are_complete_and_non_empty() {
    let modules = run_ok(&["--list-modules"]);
    let names: Vec<&str> = modules
        .stdout()
        .lines()
        .filter_map(listed_module_name)
        .collect();
    for name in DEFAULT_STRUCTURE.split(':') {
        assert!(names.contains(&name), "--list-modules is missing {name}");
    }

    let structure = run_ok(&["--print-structure"]);
    assert_eq!(structure.stdout().trim(), DEFAULT_STRUCTURE);

    // These are always populated, on a bare CI runner too.
    for flag in [
        "--list-config-paths",
        "--list-data-paths",
        "--list-logos",
        "--list-features",
    ] {
        let output = run_ok(&[flag]);
        assert!(!output.stdout().trim().is_empty(), "{flag} produced no output");
    }

    // --list-presets reflects installed fastfetch presets; a clean CI image
    // has none, so require a clean success (and valid UTF-8) rather than
    // assuming the host happens to ship a preset collection.
    let presets = run_ok(&["--list-presets"]);
    assert!(std::str::from_utf8(presets.stdout_bytes).is_ok());
}

#[test]
fn pipe_output_is_deterministic() {
    let first = run_ok(&["--pipe", "--structure", "OS:Kernel"]);
    let second = run_ok(&["--pipe", "--structure", "OS:Kernel"]);
    assert_eq!(first.stdout_bytes, second.stdout_bytes, "two identical runs differed");
}

#[test]
fn error_paths_exit_nonzero_without_panics() {
    let cases: &[(&[&str], &str)] = &[
        (&["--definitely-not-a-rustfetch-flag"], "unknown option"),
        (&["--OS"], "unknown module key"),
        (&["--key-type", "bogus"], "unknown --key-type value"),
    ];
    for (args, expected) in cases {
        let result = run(args);
        assert_ne!(result.status, 0, "rustfetch {args:?} unexpectedly succeeded");
        assert!(result.stderr().contains(expected), "stderr for {args:?} lacks {expected:?}");
        assert!(!result.stderr().contains("panicked at"), "rustfetch {args:?} panicked");
    }
}

#[test]
fn version_strings_have_documented_shape() {
    let version = run_ok(&["--version"]);
    let text = version.stdout().trim();
    assert!(is_version_line(text), "unexpected --version output: {text:?}");

    let raw = run_ok(&["--version-raw"]);
    let text = raw.stdout().trim();
    assert!(is_version_raw(text), "unexpected --version-raw output: {text:?}");
}

#[test]
fn all_outputs_are_valid_utf8() {
    let commands: &[&[&str]] = &[
        &["--pipe", "--logo", "none"],
        &["--pipe", "--structure", "OS:Kernel", "--logo", "none"],
        &["--structure", "OS:Kernel", "--logo", "none"],
        &["--pipe", "--logo", "none", "--structure", "OS:Kernel:Uptime", "--key-width", "20"],
        &["--structure", "OS"],
        &["--structure", "OS:Kernel"],
        &["--structure", "OS:Kernel:Uptime", "--structure-disabled", "Kernel"],
        &["--list-modules"],
        &["--print-structure"],
        &["--list-config-paths"],
        &["--list-data-paths"],
        &["--list-logos"],
        &["--list-presets"],
        &["--list-features"],
        &["--version"],
        &["--version-raw"],
        &["--definitely-not-a-rustfetch-flag"],
        &["--OS"],
        &["--key-type", "bogus"],
    ];
    for args in commands {
        let result = run(args);
        assert!(
            std::str::from_utf8(&result.stdout_bytes).is_ok(),
            "stdout for {args:?} is not valid UTF-8"
        );
        assert!(
            std::str::from_utf8(&result.stderr_bytes).is_ok(),
            "stderr for {args:?} is not valid UTF-8"
        );
    }
}

#[test]
fn shared_helpers_handle_ansi_and_row_filtering() {
    let sample = "user@host\n-----------\nOS: Arch\n\n\x1b[40m   \x1b[m\nKernel: Linux\n";
    let lines = module_lines(sample);
    assert_eq!(lines.as_slice(), &["OS: Arch", "Kernel: Linux"]);
    assert_eq!(module_keys("OS: Arch\nKernel: Linux\n").as_slice(), &["OS", "Kernel"]);
    assert_eq!(strip_ansi("OS: \x1b[36mArch\x1b[0m"), "OS: Arch");
    assert_eq!(strip_ansi("\x1b]0;title\x07Kernel: Linux"), "Kernel: Linux");
    assert_eq!(expand_cursor_columns("OS: \x1b[20GArch"), "OS:                 Arch");
}
