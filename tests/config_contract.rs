//! Contract tests for JSONC config loading through `-c` / `--config`.
//!
//! Every test writes its config into a private scratch directory under
//! $XDG_RUNTIME_DIR or /dev/shm (falling back to the system temp dir) and
//! removes that directory through a Drop guard.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = scratch_root();
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|time| time.as_nanos()).unwrap_or(0);
        for _ in 0..128 {
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = root.join(format!("rustfetch-config-contract-{}-{nanos}-{id}", std::process::id()));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self { path },
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("cannot create {}: {error}", path.display()),
            }
        }
        panic!("could not allocate a scratch directory");
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, name: &str, contents: &str) -> String {
        let path = self.path.join(name);
        if let Err(error) = std::fs::write(&path, contents) {
            panic!("cannot write {}: {error}", path.display());
        }
        path.to_string_lossy().into_owned()
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn scratch_root() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        if let Ok(value) = std::env::var("XDG_RUNTIME_DIR") {
            let path = PathBuf::from(value);
            if path.is_dir() {
                return path;
            }
        }
        let path = PathBuf::from("/dev/shm");
        if path.is_dir() {
            return path;
        }
    }
    std::env::temp_dir()
}

fn run(args: &[&str]) -> Output {
    match Command::new(env!("CARGO_BIN_EXE_rustfetch"))
        .args(args)
        .env("NO_CONFIG", "1")
        .env("LC_ALL", "C")
        .output()
    {
        Ok(output) => output,
        Err(error) => panic!("failed to spawn rustfetch: {error}"),
    }
}

fn stdout(args: &[&str]) -> String {
    String::from_utf8_lossy(&run(args).stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn non_empty_lines(output: &Output) -> Vec<String> {
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().map(str::to_owned).filter(|line| !line.is_empty()).collect()
}

fn assert_no_panic(output: &Output) {
    assert!(
        !stderr(output).contains("panicked"),
        "rustfetch panicked: {}",
        stderr(output)
    );
}

#[test]
fn string_module_list_selects_os_and_kernel_only() {
    let dir = ScratchDir::new();
    let config = dir.write("modules.jsonc", r#"{"modules":["os","kernel"]}"#);
    let output = run(&["-c", &config, "--pipe", "--logo", "none", "--local-only"]);
    assert!(output.status.success(), "config load failed: {}", stderr(&output));
    assert_no_panic(&output);

    let lines = non_empty_lines(&output);
    assert_eq!(lines.len(), 2, "expected exactly two module lines: {lines:?}");
    assert!(lines[0].starts_with("OS:"), "first line must be OS: {}", lines[0]);
    assert!(lines[1].starts_with("Kernel:"), "second line must be Kernel: {}", lines[1]);

    let baseline = stdout(&["--structure", "OS:Kernel", "--pipe", "--logo", "none", "--local-only"]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        baseline.trim_end(),
        "the config's module list must match --structure OS:Kernel"
    );
}

#[test]
fn module_args_change_the_rendered_line() {
    let dir = ScratchDir::new();
    let baseline = stdout(&["--structure", "OS", "--pipe", "--logo", "none", "--local-only"]);

    // The literal task config: module args are accepted and still produce a
    // single OS line (the OS module does not use `format` as a value template).
    let literal = dir.write("os-format.jsonc", r#"{"modules":[{"type":"os","format":"{name}"}]}"#);
    let literal_output = stdout(&["-c", &literal, "--pipe", "--logo", "none", "--local-only"]);
    assert!(literal_output.starts_with("OS:"), "OS row missing: {literal_output:?}");
    assert_eq!(literal_output.lines().count(), 1, "expected one OS row: {literal_output:?}");

    // Hiding the custom key leaves only the OS value, so the line differs
    // from the default `OS: ...` formatting.
    let hidden_key = dir.write("os-hidden-key.jsonc", r#"{"modules":[{"type":"os","format":"{name}","key":" "}]}"#);
    let hidden_output = stdout(&["-c", &hidden_key, "--pipe", "--logo", "none", "--local-only"]);
    assert!(!hidden_output.contains("OS:"), "the OS key must be hidden: {hidden_output:?}");
    let os_value = baseline.strip_prefix("OS: ").unwrap_or(baseline.as_str()).trim();
    assert_eq!(hidden_output.trim(), os_value, "the keyless row must be just the OS value");

    // Modules that do treat `format` as a value template honour it too.
    let kernel_format = dir.write("kernel-format.jsonc", r#"{"modules":[{"type":"kernel","format":"release={release}"}]}"#);
    let kernel_output = stdout(&["-c", &kernel_format, "--pipe", "--logo", "none", "--local-only"]);
    assert!(
        kernel_output.contains("Kernel: release="),
        "the custom kernel format was not applied: {kernel_output:?}"
    );
}

#[test]
fn jsonc_comments_and_trailing_commas_are_accepted() {
    let dir = ScratchDir::new();
    let config = dir.write(
        "commented.jsonc",
        "{\n  // a line comment\n  \"modules\": [\"os\", \"kernel\",], /* a block comment */\n}\n",
    );
    let output = run(&["-c", &config, "--pipe", "--logo", "none", "--local-only"]);
    assert!(output.status.success(), "JSONC was rejected: {}", stderr(&output));
    assert_no_panic(&output);
    let baseline = stdout(&["--structure", "OS:Kernel", "--pipe", "--logo", "none", "--local-only"]);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        baseline.trim_end(),
        "JSONC comments/trailing commas must not change the module list"
    );
}

#[test]
fn missing_config_is_a_clean_error() {
    let dir = ScratchDir::new();
    let missing = dir.path().join("definitely-missing-rustfetch-config.jsonc");
    let missing = missing.to_string_lossy().into_owned();
    let output = run(&["-c", &missing, "--pipe", "--logo", "none", "--local-only"]);
    assert!(!output.status.success(), "a missing config must not exit successfully");
    assert!(output.status.code().is_some_and(|code| code != 0), "expected a non-zero exit code");
    assert!(
        stderr(&output).contains("couldn't find config"),
        "expected a clear missing-config error: {}",
        stderr(&output)
    );
    assert_no_panic(&output);
}

#[test]
fn malformed_jsonc_is_a_clean_error() {
    let dir = ScratchDir::new();
    let config = dir.write("malformed.jsonc", "{\n  \"modules\": [\n");
    let output = run(&["-c", &config, "--pipe", "--logo", "none", "--local-only"]);
    assert!(!output.status.success(), "malformed JSONC must not exit successfully");
    assert!(output.status.code().is_some_and(|code| code != 0), "expected a non-zero exit code");
    assert!(
        stderr(&output).contains("failed to parse JSON config file"),
        "expected a parse error message: {}",
        stderr(&output)
    );
    assert_no_panic(&output);
}

#[test]
fn unknown_module_reports_a_clean_error_line() {
    let dir = ScratchDir::new();
    let config = dir.write(
        "unknown.jsonc",
        r#"{"display":{"showErrors":true},"modules":["totally-not-a-module-xyz"]}"#,
    );
    let output = run(&["-c", &config, "--pipe", "--logo", "none", "--local-only"]);
    assert!(output.status.success(), "an unknown module must not abort: {}", stderr(&output));
    assert_no_panic(&output);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("totally-not-a-module-xyz"), "error line must name the module: {text:?}");
    assert!(text.contains("<no implementation provided>"), "expected the error text: {text:?}");

    let json_output = run(&["-c", &config, "--format", "json", "--pipe", "--local-only"]);
    assert!(json_output.status.success(), "JSON mode must not abort: {}", stderr(&json_output));
    assert_no_panic(&json_output);
    let json = String::from_utf8_lossy(&json_output.stdout);
    assert!(json.contains("\"type\": \"totally-not-a-module-xyz\""), "missing item type: {json:?}");
    assert!(json.contains("\"error\": \"Unknown module type\""), "missing item error: {json:?}");
}
