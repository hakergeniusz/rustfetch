//! Environment-variable expansion for `--install-dir`, kept target-independent
//! so the expansion logic is unit-testable without touching the process env.

/// Expand `%NAME%` references in `input` using `lookup`.
///
/// Unknown variables are left verbatim (matching the PowerShell installer),
/// so a typo does not silently turn into a relative path.
pub fn expand(input: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let Some(relative_end) = input[i + 1..].find('%') {
                let end = i + 1 + relative_end;
                let name = &input[i + 1..end];
                if !name.is_empty() {
                    if let Some(value) = lookup(name) {
                        out.push_str(&value);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        match input[i..].chars().next() {
            Some(ch) => {
                out.push(ch);
                i += ch.len_utf8();
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::expand;

    #[test]
    fn expands_known_variables() {
        let out = expand("%FOO%\\Programs\\%BAR%", |name| match name {
            "FOO" => Some("C:\\Users\\me\\AppData\\Local".to_string()),
            "BAR" => Some("rustfetch".to_string()),
            _ => None,
        });
        assert_eq!(out, "C:\\Users\\me\\AppData\\Local\\Programs\\rustfetch");
    }

    #[test]
    fn leaves_unknown_variables_untouched() {
        assert_eq!(expand("%NOPE%\\bin", |_| None), "%NOPE%\\bin");
    }

    #[test]
    fn handles_empty_and_unicode() {
        assert_eq!(expand("", |_| None), "");
        assert_eq!(expand("caf\u{e9}\\%X%", |_| None), "caf\u{e9}\\%X%");
    }
}
