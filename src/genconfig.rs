//! fastfetch.c's non-interactive config generation:
//! `setupGenConfigPath` / `generateConfigFile` / `writeConfigFile`, plus the
//! `FF_RESULT_DOC_TYPE_CONFIG` half of commandoption.c's
//! `ffMigrateCommandOptionToJsonc`.
//!
//! Only the non-interactive path is ported. Upstream's TTY CUI
//! (`common/impl/genconfig.c`) is not implemented here; the harness (and
//! `$NO_COLOR`) always takes this path.

use std::io::Write as _;
use std::path::Path;

use crate::cli::{Cli, GenConfigState};
use crate::json::{self, Json};

const SCHEMA: &str = "https://github.com/fastfetch-cli/fastfetch/raw/master/doc/json_schema.json";

/// Called from main() right after the argv walk. Returns false when
/// `--gen-config` was absent so the normal fetch run can continue.
pub(crate) fn generate_if_requested(cli: &Cli, state: &GenConfigState) -> bool {
    let Some(path) = state.path.as_deref() else {
        return false;
    };

    // generateConfigFile runs this before anything else, including the
    // special `-` stdout path.
    if Path::new(path).exists() {
        eprintln!("Error: file `{path}` exists. Please remove it before generating a new one");
        std::process::exit(477);
    }

    let content = build_document(cli, state);

    if path == "-" {
        write_stdout(&content);
        return true;
    }

    if !write_file(path, content.as_bytes()) {
        // Upstream uses printf (stdout), not stderr, for this one.
        write_stdout(&format!("Error: failed to write file in `{path}`\n"));
        std::process::exit(1);
    }

    write_stdout(&format!(
        "✓ Configuration file generated: `{path}`\n\
         * Tip: Use a JSON schema-aware editor for better editing experience\n\
         * Documentation: https://github.com/fastfetch-cli/fastfetch/wiki/Configuration\n"
    ));
    true
}

/// The generated document is a JSON object with `$schema` first, then
/// (only when at least one module survives) a `modules` array of the
/// lowercased registered names. Module options are not written in the
/// minimal CONFIG mode that non-interactive generation uses.
fn build_document(cli: &Cli, state: &GenConfigState) -> String {
    let structure = cli
        .structure
        .as_deref()
        .filter(|structure| !structure.is_empty())
        .unwrap_or(crate::modules::DEFAULT_STRUCTURE_TEXT);

    let mut root = Json::obj();
    root.push("$schema", SCHEMA);

    let names: Vec<String> = structure
        .split(':')
        .filter(|token| {
            !state.structure_disabled.as_deref().is_some_and(|disabled| {
                disabled.split(':').any(|name| name.eq_ignore_ascii_case(token))
            })
        })
        .filter_map(crate::cli::canonical_module_name)
        .map(|name| name.to_ascii_lowercase())
        .collect();

    if !names.is_empty() {
        root.push("modules", Json::Array(names.into_iter().map(Json::Str).collect()));
    }

    // yyjson_mut_write with PRETTY_TWO_SPACES | NEWLINE_AT_END.
    json::to_pretty(&root)
}

/// ffWriteFileData: open/truncate (0644), creating missing parent folders
/// on ENOENT, then one write.
fn write_file(path: &str, content: &[u8]) -> bool {
    let write = || -> std::io::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o644);
        }
        match options.open(path) {
            Ok(mut file) => file.write_all(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if let Some(parent) = Path::new(path).parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut file = options.open(path)?;
                file.write_all(content)
            },
            Err(error) => Err(error),
        }
    };
    write().is_ok()
}

/// Upstream's yyjson_mut_write_fp / printf paths ignore write errors; do
/// the same so a closed stdout cannot panic through `println!`.
fn write_stdout(content: &str) {
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(content.as_bytes());
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_uses_the_default_template() {
        let cli = crate::cli::Cli::parse_from(&["rustfetch", "--gen-config", "/tmp/generated.jsonc"]);
        let expected = "{\n  \"$schema\": \"https://github.com/fastfetch-cli/fastfetch/raw/master/doc/json_schema.json\",\n  \"modules\": [\n    \"title\",\n    \"separator\",\n    \"os\",\n    \"host\",\n    \"kernel\",\n    \"uptime\",\n    \"packages\",\n    \"shell\",\n    \"display\",\n    \"de\",\n    \"wm\",\n    \"wmtheme\",\n    \"theme\",\n    \"icons\",\n    \"font\",\n    \"cursor\",\n    \"terminal\",\n    \"terminalfont\",\n    \"cpu\",\n    \"gpu\",\n    \"memory\",\n    \"swap\",\n    \"disk\",\n    \"localip\",\n    \"battery\",\n    \"poweradapter\",\n    \"locale\",\n    \"break\",\n    \"colors\"\n  ]\n}\n";
        assert_eq!(build_document(&cli, &crate::cli::gen_config_state()), expected);
    }

    #[test]
    fn document_filters_unknown_and_disabled_modules() {
        let cli = crate::cli::Cli::parse_from(&[
            "rustfetch",
            "-s",
            "OS:bogus:CPU:MEMORY:Break:os",
            "--structure-disabled",
            "cpu:colorS",
            "--gen-config",
            "/tmp/generated.jsonc",
        ]);
        assert_eq!(
            build_document(&cli, &crate::cli::gen_config_state()),
            "{\n  \"$schema\": \"https://github.com/fastfetch-cli/fastfetch/raw/master/doc/json_schema.json\",\n  \"modules\": [\n    \"os\",\n    \"memory\",\n    \"break\",\n    \"os\"\n  ]\n}\n"
        );
    }

    #[test]
    fn document_with_no_modules_omits_the_key() {
        let cli = crate::cli::Cli::parse_from(&[
            "rustfetch",
            "-s",
            "bogus",
            "--gen-config",
            "/tmp/generated.jsonc",
        ]);
        assert_eq!(
            build_document(&cli, &crate::cli::gen_config_state()),
            "{\n  \"$schema\": \"https://github.com/fastfetch-cli/fastfetch/raw/master/doc/json_schema.json\"\n}\n"
        );
    }
}
