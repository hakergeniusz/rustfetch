//! Contract tests for the CLI display/render flags.
//!
//! These drive the real binary through `env!("CARGO_BIN_EXE_rustfetch")` so
//! the assertions cover the whole argv -> render pipeline, not just the
//! formatting helpers. No external tools and no extra dependencies.

use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_rustfetch");

/// Stable base argv: skip the two network modules so the tests never wait
/// on DNS/HTTP, force pipe mode (stdout is a pipe in tests anyway), and
/// remove the logo so every emitted line is a module row.
fn base<'a>(extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["--local-only", "--pipe", "--logo", "none"];
    args.extend_from_slice(extra);
    args
}

fn output(args: &[&str]) -> Output {
    match Command::new(BIN).args(args).output() {
        Ok(out) => out,
        Err(err) => panic!("failed to spawn {BIN} with {args:?}: {err}"),
    }
}

fn stdout(args: &[&str]) -> String {
    let out = output(args);
    assert!(
        out.status.success(),
        "binary failed for {args:?}: code {:?}, stderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    match String::from_utf8(out.stdout) {
        Ok(text) => text,
        Err(err) => panic!("stdout was not valid UTF-8 for {args:?}: {err}"),
    }
}

fn rows(text: &str) -> Vec<&str> {
    text.lines().filter(|line| !line.trim().is_empty()).collect()
}

/// The absolute-column jump emitted by `--key-width`, decoded back to its
/// column number. In pipe mode this escape is the only control sequence
/// that remains; it is where the value starts on the terminal.
fn jump_column(row: &str) -> Option<u32> {
    let (_, tail) = row.split_once('\x1b')?;
    let digits = tail.strip_prefix('[')?.split_once('G')?.0;
    digits.parse::<u32>().ok()
}

#[test]
fn key_width_moves_each_value_to_the_requested_column() {
    let text = stdout(&base(&["--structure", "OS:Kernel", "--key-width", "20"]));
    let printed = rows(&text);
    assert_eq!(printed.len(), 2, "expected two module rows: {printed:?}");

    for (row, key) in printed.iter().zip(["OS", "Kernel"]) {
        assert_eq!(jump_column(row), Some(20), "row did not jump to column 20: {row:?}");
        let (prefix, value) = match row.split_once("\x1b[20G") {
            Some(parts) => parts,
            None => panic!("missing 20-column jump in {row:?}"),
        };
        assert_eq!(prefix, format!("{key}: "), "unexpected key prefix in {row:?}");
        assert!(!value.is_empty(), "value missing after the column jump in {row:?}");
        assert!(!value.contains('\x1b'), "unexpected escape in value {value:?}");
    }

    let plain = stdout(&base(&["--structure", "OS:Kernel"]));
    assert!(!plain.contains("\x1b["), "plain pipe rows must not use cursor jumps: {plain:?}");
}

#[test]
fn key_padding_left_adds_exactly_the_requested_spaces() {
    let text = stdout(&base(&["--structure", "OS:Kernel", "--key-padding-left", "4"]));
    let printed = rows(&text);
    assert_eq!(printed.len(), 2, "expected two module rows: {printed:?}");

    for (row, key) in printed.iter().zip(["OS", "Kernel"]) {
        let leading = row.chars().take_while(|ch| *ch == ' ').count();
        assert_eq!(leading, 4, "expected exactly 4 leading spaces in {row:?}");
        assert!(
            row.starts_with(&format!("    {key}: ")),
            "padding was not inserted before the key: {row:?}"
        );
    }

    let plain = stdout(&base(&["--structure", "OS:Kernel"]));
    for row in rows(&plain) {
        assert!(!row.starts_with(' '), "unexpected default padding in {row:?}");
    }
}

#[test]
fn separator_replaces_the_colon_separator_on_every_row() {
    let text = stdout(&base(&["--structure", "OS:Kernel", "--separator", " => "]));
    let printed = rows(&text);
    assert_eq!(printed.len(), 2, "expected two module rows: {printed:?}");

    for (row, key) in printed.iter().zip(["OS", "Kernel"]) {
        assert!(
            row.starts_with(&format!("{key} => ")),
            "separator was not applied to {row:?}"
        );
        assert!(
            !row.starts_with(&format!("{key}: ")),
            "old ': ' separator survived in {row:?}"
        );
        let (_, value) = match row.split_once(" => ") {
            Some(parts) => parts,
            None => panic!("missing ' => ' separator in {row:?}"),
        };
        assert!(!value.is_empty(), "no value after separator in {row:?}");
        assert!(!row.contains('\x1b'), "pipe rows must not contain escapes: {row:?}");
    }
}

#[test]
fn key_type_controls_icons_and_key_text() {
    let plain = stdout(&base(&["--structure", "OS:Kernel"]));
    let mut values = Vec::new();
    for row in rows(&plain) {
        match row.split_once(": ") {
            Some((_, value)) => values.push(value),
            None => panic!("default row had no ': ' separator: {row:?}"),
        }
    }

    let none = stdout(&base(&["--structure", "OS:Kernel", "--key-type", "none"]));
    assert_eq!(rows(&none), values, "none must print values without keys");

    let icon = stdout(&base(&["--structure", "OS:Kernel", "--key-type", "icon"]));
    let icon_rows = rows(&icon);
    assert_eq!(icon_rows.len(), 2, "expected two icon rows: {icon_rows:?}");
    for row in &icon_rows {
        let first = row.chars().next().unwrap_or(' ');
        assert!(!first.is_whitespace(), "icon row had no non-space prefix: {row:?}");
        assert!(!row.contains("OS:"), "key text leaked into icon row: {row:?}");
        assert!(!row.contains("Kernel:"), "key text leaked into icon row: {row:?}");
    }

    let both = stdout(&base(&["--structure", "OS:Kernel", "--key-type", "both"]));
    let both_rows = rows(&both);
    assert_eq!(both_rows.len(), 2, "expected two 'both' rows: {both_rows:?}");
    for (row, key) in both_rows.iter().zip(["OS", "Kernel"]) {
        let first = row.chars().next().unwrap_or(' ');
        assert!(!first.is_whitespace(), "icon missing from 'both' row: {row:?}");
        assert!(
            row.contains(&format!("{key}: ")),
            "key string missing from 'both' row: {row:?}"
        );
    }
}

#[test]
fn color_keys_is_pipe_suppressed_and_emits_red_on_a_terminal() {
    let piped = stdout(&base(&["--structure", "OS:Kernel", "--color-keys", "red"]));
    assert!(!piped.contains('\x1b'), "pipe mode must win over --color-keys: {piped:?}");

    // stdout is a pipe during tests, so an explicit `--pipe false` is
    // required to exercise the color path at all.
    let colored = stdout(&[
        "--local-only",
        "--pipe",
        "false",
        "--logo",
        "none",
        "--structure",
        "OS:Kernel",
        "--color-keys",
        "red",
    ]);
    let red_at = match colored.find("\x1b[31m") {
        Some(position) => position,
        None => panic!("no red foreground sequence in {colored:?}"),
    };
    let key_at = match colored.find("OS") {
        Some(position) => position,
        None => panic!("no key text in {colored:?}"),
    };
    assert!(red_at < key_at, "red must be emitted before the key: {colored:?}");
    assert!(colored.contains("\x1b[m"), "missing reset after colored key: {colored:?}");
}

#[test]
fn bright_color_toggles_the_bold_sequence() {
    let common = [
        "--local-only",
        "--pipe",
        "false",
        "--logo",
        "none",
        "--structure",
        "OS:Kernel",
        "--color-keys",
        "red",
    ];
    let mut on_args = common.to_vec();
    on_args.extend_from_slice(&["--bright-color", "true"]);
    let mut off_args = common.to_vec();
    off_args.extend_from_slice(&["--bright-color", "false"]);

    let on = stdout(&on_args);
    let off = stdout(&off_args);
    assert!(on.contains("\x1b[1m"), "bright=true must emit bold: {on:?}");
    assert!(!off.contains("\x1b[1m"), "bright=false must not emit bold: {off:?}");
    assert!(on.contains("\x1b[31m") && off.contains("\x1b[31m"), "both runs must be red");
}

#[test]
fn bar_width_controls_the_number_of_cells_in_the_bracketed_bar() {
    let text = stdout(&base(&[
        "--structure",
        "Memory",
        "--bar-width",
        "20",
        "--percent-type",
        "2",
        "--bar-char-elapsed",
        "#",
        "--bar-char-total",
        ".",
    ]));
    let row = match rows(&text).into_iter().find(|line| line.starts_with("Memory: ")) {
        Some(row) => row,
        None => panic!("no Memory row in {text:?}"),
    };
    let (_, after_left) = match row.split_once("[ ") {
        Some(parts) => parts,
        None => panic!("missing left border in {row:?}"),
    };
    let (cells, _) = match after_left.split_once(" ]") {
        Some(parts) => parts,
        None => panic!("missing right border in {row:?}"),
    };
    assert_eq!(cells.chars().count(), 20, "bar had the wrong number of cells: {row:?}");
    assert!(
        cells.chars().all(|ch| ch == '#' || ch == '.'),
        "bar contained unexpected characters: {cells:?}"
    );
}

fn size_suffixes(text: &str) -> Vec<&str> {
    const UNITS: &[&str] = &["KiB", "MiB", "GiB", "TiB", "PiB", "kB", "MB", "GB", "TB", "PB"];
    UNITS.iter().copied().filter(|unit| text.contains(unit)).collect()
}

#[test]
fn size_binary_prefix_switches_disk_suffixes() {
    let si = stdout(&base(&["--structure", "Disk", "--size-binary-prefix", "SI"]));
    let iec = stdout(&base(&["--structure", "Disk", "--size-binary-prefix", "IEC"]));
    let si_units = size_suffixes(&si);
    let iec_units = size_suffixes(&iec);

    assert!(!si_units.is_empty() && !iec_units.is_empty(), "no disk sizes found: {si:?} / {iec:?}");
    assert!(
        iec_units.iter().any(|unit| unit.ends_with("iB")),
        "IEC did not produce binary suffixes: {iec:?}"
    );
    assert!(
        si_units.iter().all(|unit| !unit.ends_with("iB")),
        "SI produced binary suffixes: {si:?}"
    );
    assert_ne!(si, iec, "the prefix flag had no effect on the Disk row");
}

#[test]
fn temp_unit_switches_the_trailing_letter_when_a_sensor_is_exposed() {
    let celsius = stdout(&base(&["--structure", "CPU", "--temp-unit", "C"]));
    let fahrenheit = stdout(&base(&["--structure", "CPU", "--temp-unit", "F"]));

    let c_has_unit = celsius.contains("\u{00b0}C");
    let f_has_unit = fahrenheit.contains("\u{00b0}F");
    if c_has_unit || f_has_unit {
        assert!(c_has_unit, "C run has no Celsius suffix: {celsius:?}");
        assert!(f_has_unit, "F run has no Fahrenheit suffix: {fahrenheit:?}");
        assert!(!celsius.contains("\u{00b0}F"), "C run printed Fahrenheit: {celsius:?}");
        assert!(!fahrenheit.contains("\u{00b0}C"), "F run printed Celsius: {fahrenheit:?}");
    } else {
        // A container usually exposes no CPU thermal zone and the CPU
        // module then prints no temperature at all (temp is off by
        // default). Both flag values are still accepted and render the
        // same well-formed row set.
        assert_eq!(celsius, fahrenheit, "temp-unit changed a senseless CPU row");
        assert!(!rows(&celsius).is_empty(), "CPU module produced no rows");
    }
}
