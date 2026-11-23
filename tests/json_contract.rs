//! Contract tests for the JSON output modes (`--format json`, `-j`, `--json`).
//!
//! The crate deliberately has no serde/serde_json dependency, so this file
//! carries a small recursive-descent JSON validator. It both checks that the
//! binary emits syntactically valid JSON and gives the tests typed access to
//! the document (top-level array, one object per item, `type` + `result`).
//! The validator is itself exercised against malformed inputs at the end.

use std::process::{Command, Output};

#[derive(Debug, Clone, PartialEq)]
enum JsonValue {
    Null,
    Bool(bool),
    Number(String),
    Str(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(items) => Some(items),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(text) => Some(text),
            _ => None,
        }
    }

    fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(entries) => entries.iter().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    fn has_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
}

/// Byte-level recursive-descent parser; `parse` requires the whole input to
/// be consumed, so trailing garbage is rejected as well.
struct Parser<'a> {
    source: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { source: input.as_bytes(), pos: 0 }
    }

    fn parse(mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        let value = self.value()?;
        self.skip_ws();
        if self.pos != self.source.len() {
            return Err(format!("unexpected trailing data at byte {}", self.pos));
        }
        Ok(value)
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<JsonValue, String> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(JsonValue::Str),
            Some(b't') => self.keyword("true", JsonValue::Bool(true)),
            Some(b'f') => self.keyword("false", JsonValue::Bool(false)),
            Some(b'n') => self.keyword("null", JsonValue::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(format!("unexpected byte at {}", self.pos)),
        }
    }

    fn keyword(&mut self, word: &str, value: JsonValue) -> Result<JsonValue, String> {
        if self.source[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(format!("invalid keyword at {}", self.pos))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.peek() != Some(b'"') {
            return Err(format!("expected string at {}", self.pos));
        }
        self.pos += 1;
        let mut bytes: Vec<u8> = Vec::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err("unterminated string".to_string());
            };
            match byte {
                b'"' => {
                    self.pos += 1;
                    break;
                },
                b'\\' => {
                    self.pos += 1;
                    let Some(escape) = self.peek() else {
                        return Err("unterminated escape".to_string());
                    };
                    self.pos += 1;
                    match escape {
                        b'"' => bytes.push(b'"'),
                        b'\\' => bytes.push(b'\\'),
                        b'/' => bytes.push(b'/'),
                        b'b' => bytes.push(0x08),
                        b'f' => bytes.push(0x0c),
                        b'n' => bytes.push(b'\n'),
                        b'r' => bytes.push(b'\r'),
                        b't' => bytes.push(b'\t'),
                        b'u' => {
                            let code = self.hex4()?;
                            let character = char::from_u32(code).unwrap_or('\u{fffd}');
                            let mut buffer = [0u8; 4];
                            bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                        },
                        _ => return Err(format!("invalid escape at {}", self.pos)),
                    }
                },
                0..=0x1f => return Err(format!("raw control byte in string at {}", self.pos)),
                _ => {
                    bytes.push(byte);
                    self.pos += 1;
                },
            }
        }
        String::from_utf8(bytes).map_err(|error| error.to_string())
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut value = 0u32;
        for _ in 0..4 {
            let Some(byte) = self.peek() else {
                return Err("short unicode escape".to_string());
            };
            let digit = match byte {
                b'0'..=b'9' => u32::from(byte - b'0'),
                b'a'..=b'f' => u32::from(byte - b'a') + 10,
                b'A'..=b'F' => u32::from(byte - b'A') + 10,
                _ => return Err(format!("bad unicode escape at {}", self.pos)),
            };
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<JsonValue, String> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            },
            _ => return Err(format!("bad number at {start}")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(format!("bad fraction at {}", self.pos));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(format!("bad exponent at {}", self.pos));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let Ok(text) = std::str::from_utf8(&self.source[start..self.pos]) else {
            return Err("bad number".to_string());
        };
        Ok(JsonValue::Number(text.to_string()))
    }

    fn array(&mut self) -> Result<JsonValue, String> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JsonValue::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    break;
                },
                _ => return Err(format!("bad array at {}", self.pos)),
            }
        }
        Ok(JsonValue::Array(items))
    }

    fn object(&mut self) -> Result<JsonValue, String> {
        self.pos += 1; // '{'
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JsonValue::Object(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(format!("missing colon at {}", self.pos));
            }
            self.pos += 1;
            let value = self.value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    break;
                },
                _ => return Err(format!("bad object at {}", self.pos)),
            }
        }
        Ok(JsonValue::Object(entries))
    }
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

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "rustfetch failed with {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parse_output(args: &[&str]) -> JsonValue {
    let output = run(args);
    assert_success(&output);
    let text = String::from_utf8_lossy(&output.stdout);
    match Parser::new(&text).parse() {
        Ok(value) => value,
        Err(error) => panic!("invalid JSON ({error}): {text}"),
    }
}

fn items(document: &JsonValue) -> &[JsonValue] {
    match document.as_array() {
        Some(items) => items,
        None => panic!("expected a top-level JSON array, got {document:?}"),
    }
}

fn types(items: &[JsonValue]) -> Vec<&str> {
    items
        .iter()
        .map(|item| item.get("type").and_then(JsonValue::as_str).unwrap_or("<missing type>"))
        .collect()
}

#[test]
fn json_output_is_a_valid_array_of_typed_items() {
    let output = run(&["--format", "json", "--pipe", "--local-only"]);
    assert_success(&output);
    let text = String::from_utf8_lossy(&output.stdout);
    let document = match Parser::new(&text).parse() {
        Ok(value) => value,
        Err(error) => panic!("invalid JSON ({error}): {text}"),
    };
    let objects = items(&document);
    assert!(!objects.is_empty(), "the default structure produced no JSON items");
    for object in objects {
        assert!(matches!(object, JsonValue::Object(_)), "item is not an object: {object:?}");
        let name = object.get("type").and_then(JsonValue::as_str).unwrap_or("");
        assert!(!name.is_empty(), "item has no string `type`: {object:?}");
        assert!(
            object.has_key("result") || object.has_key("error"),
            "item has neither `result` nor `error`: {object:?}"
        );
    }
    assert!(
        text.starts_with("[\n  {\n    \"type\": \""),
        "unexpected two-space pretty layout: {text:?}"
    );
    assert!(text.ends_with("]\n"), "JSON document must end with a newline: {text:?}");
}

#[test]
fn structure_selects_exactly_two_items_in_order() {
    let document = parse_output(&["--format", "json", "--pipe", "--structure", "OS:Kernel", "--local-only"]);
    let objects = items(&document);
    assert_eq!(objects.len(), 2, "expected exactly OS and Kernel: {objects:?}");
    assert_eq!(types(objects), ["OS", "Kernel"]);
    for object in objects {
        assert!(object.has_key("result"), "stable module should have a result: {object:?}");
    }
}

#[test]
fn short_and_long_json_flags_are_identical() {
    let short = stdout(&["-j", "--pipe", "--structure", "OS:Kernel", "--local-only"]);
    let long = stdout(&["--json", "--pipe", "--structure", "OS:Kernel", "--local-only"]);
    let format = stdout(&["--format", "json", "--pipe", "--structure", "OS:Kernel", "--local-only"]);
    assert_eq!(short, long, "-j and --json must emit the same document");
    assert_eq!(short, format, "-j and --format json must emit the same document");
}

#[test]
fn json_has_no_ansi_escapes_without_pipe() {
    // stdout is captured (not a TTY), so pipe mode is auto-detected: even
    // without the explicit `--pipe`, no reset/cursor/linewrap bytes may leak
    // into the JSON document.
    let output = run(&["--format", "json", "--structure", "OS:Kernel", "--local-only"]);
    assert_success(&output);
    assert!(
        !output.stdout.contains(&0x1b),
        "JSON output must be free of ANSI escapes: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn json_is_deterministic_for_a_stable_structure() {
    let first = stdout(&["--format", "json", "--pipe", "--structure", "OS:Kernel", "--local-only"]);
    let second = stdout(&["--format", "json", "--pipe", "--structure", "OS:Kernel", "--local-only"]);
    assert_eq!(first, second, "two runs of a stable structure must produce identical JSON");
}

#[test]
fn localip_module_still_emits_valid_json() {
    let document = parse_output(&["--format", "json", "--pipe", "--structure", "LocalIp", "--local-only"]);
    let objects = items(&document);
    assert_eq!(types(objects), ["LocalIp"]);
    assert!(
        objects[0].has_key("result") || objects[0].has_key("error"),
        "LocalIp item must carry a result or error"
    );
}

#[test]
fn validator_rejects_malformed_documents() {
    let bad = [
        "",
        "   ",
        "{",
        "[1,]",
        "{\"a\":1,}",
        "{\"a\" 1}",
        "{\"a\":1 \"b\":2}",
        "{\"a\":tru}",
        "{\"a\":1} trailing",
        "\"unterminated",
        "\"bad\\q\"",
        "01",
        "1.",
        "1e",
        "[true false]",
        "{\"a\": 1 \"}",
    ];
    for input in bad {
        assert!(
            Parser::new(input).parse().is_err(),
            "the validator wrongly accepted {input:?}"
        );
    }
}

#[test]
fn validator_accepts_well_formed_documents() {
    let good = [
        "{}",
        "[]",
        "null",
        "true",
        "-0.0",
        "2e34",
        "\"héllo\"",
        "{\"a\":[1,true,null,\"x\\u00e9\"],\"b\":{}}",
    ];
    for input in good {
        assert!(
            Parser::new(input).parse().is_ok(),
            "the validator wrongly rejected {input:?}"
        );
    }
}
