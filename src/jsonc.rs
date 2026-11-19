//! A minimal JSON/JSONC value parser, standing in for yyjson on config
//! files. Strict mode (stdin and `.json`) matches plain JSON; JSONC mode
//! (`.jsonc`, auto-loaded config) additionally allows `//` and `/* */`
//! comments and trailing commas. Errors carry yyjson's message and the
//! 0-based byte position of the offending character, which the caller
//! renders as `failed to parse ... at (row, col)`.

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// yyjson_get_uint semantics: bools count as 0/1, floats truncate,
    /// negative ints wrap.
    pub fn as_uint(&self) -> Option<u64> {
        match self {
            Value::UInt(u) => Some(*u),
            Value::Int(i) => Some(*i as u64),
            Value::Float(f) => Some(*f as u64),
            Value::Bool(b) => Some(u64::from(*b)),
            _ => None,
        }
    }

    #[allow(dead_code)] // used as module option decoding grows
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            Value::UInt(u) => Some(*u as i64),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            // yyjson keeps the last occurrence of duplicate keys.
            Value::Object(entries) => entries
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    #[allow(dead_code)] // used as module option decoding grows
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }
}

/// yyjson-style parse failure: message plus the byte offset of the
/// offending character (or of the end of input).
#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub msg: &'static str,
    pub pos: usize,
}

pub struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
    /// comments and trailing commas allowed
    jsonc: bool,
}

impl<'a> Parser<'a> {
    /// Parses `input`; on failure the error position is relative to the
    /// whole input (yyjson reports absolute positions too).
    pub fn parse(input: &'a str, jsonc: bool) -> Result<Value, ParseError> {
        let mut parser = Self {
            input: input.as_bytes(),
            pos: 0,
            jsonc,
        };
        parser.skip_ws();
        let value = parser.value()?;
        parser.skip_ws();
        if parser.pos < parser.input.len() {
            return Err(parser.err("unexpected content after document"));
        }
        Ok(value)
    }

    fn err(&self, msg: &'static str) -> ParseError {
        ParseError {
            msg,
            pos: self.pos,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    /// Whitespace plus, in JSONC mode, comments; positions advance so the
    /// next error lands on real input.
    fn skip_ws(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.pos += 1,
                Some(b'/') if self.jsonc => match self.input.get(self.pos + 1) {
                    Some(b'/') => {
                        self.pos += 2;
                        while let Some(c) = self.bump() {
                            if c == b'\n' {
                                break;
                            }
                        }
                    },
                    Some(b'*') => {
                        self.pos += 2;
                        while self.pos < self.input.len() {
                            if self.input[self.pos] == b'*'
                                && self.input.get(self.pos + 1) == Some(&b'/')
                            {
                                self.pos += 2;
                                break;
                            }
                            self.pos += 1;
                        }
                    },
                    _ => return,
                },
                _ => return,
            }
        }
    }

    fn value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            None => Err(self.err("unexpected end of data")),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(_) => Err(self.err("unexpected character, expected a JSON value")),
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, ParseError> {
        if self.input[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                self.pos += 1;
            }
            Err(self.err("unexpected end of data"))
        }
    }

    fn object(&mut self) -> Result<Value, ParseError> {
        self.pos += 1; // '{'
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(entries));
        }
        loop {
            self.skip_ws();
            let Some(c) = self.peek() else {
                return Err(self.err("unexpected end of data"));
            };
            if c != b'"' {
                return Err(self.err("unexpected character, expected a JSON value"));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.err("unexpected character, expected ':' after key"));
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    let comma_pos = self.pos;
                    self.pos += 1;
                    self.skip_ws();
                    match self.peek() {
                        Some(b'}') => {
                            if self.jsonc {
                                self.pos += 1;
                                return Ok(Value::Object(entries));
                            }
                            return Err(ParseError {
                                msg: "trailing comma is not allowed",
                                pos: comma_pos,
                            });
                        },
                        None => return Err(self.err("unexpected end of data")),
                        _ => {},
                    }
                },
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(entries));
                },
                None => return Err(self.err("unexpected end of data")),
                _ => return Err(self.err("unexpected character, expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Value, ParseError> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    let comma_pos = self.pos;
                    self.pos += 1;
                    self.skip_ws();
                    match self.peek() {
                        Some(b']') => {
                            if self.jsonc {
                                self.pos += 1;
                                return Ok(Value::Array(items));
                            }
                            return Err(ParseError {
                                msg: "trailing comma is not allowed",
                                pos: comma_pos,
                            });
                        },
                        None => return Err(self.err("unexpected end of data")),
                        _ => {},
                    }
                },
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(items));
                },
                None => return Err(self.err("unexpected end of data")),
                _ => return Err(self.err("unexpected character, expected ',' or ']'")),
            }
        }
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            let Some(c) = self.bump() else {
                return Err(self.err("unexpected end of data"));
            };
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    // yyjson reports escape errors at the backslash.
                    let backslash = self.pos - 1;
                    let Some(esc) = self.bump() else {
                        return Err(self.err("unexpected end of data"));
                    };
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000C}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4(backslash)?;
                            let ch = if (0xD800..0xDC00).contains(&first) {
                                if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                                    return Err(ParseError {
                                        msg: "no low surrogate in string",
                                        pos: backslash,
                                    });
                                }
                                let second = self.hex4(backslash)?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return Err(ParseError {
                                        msg: "no low surrogate in string",
                                        pos: backslash,
                                    });
                                }
                                let combined =
                                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                                char::from_u32(combined).unwrap_or('\u{FFFD}')
                            } else if (0xDC00..0xE000).contains(&first) {
                                return Err(ParseError {
                                    msg: "invalid high surrogate in string",
                                    pos: backslash,
                                });
                            } else {
                                char::from_u32(first).unwrap_or('\u{FFFD}')
                            };
                            out.push(ch);
                        },
                        _ => {
                            return Err(ParseError {
                                msg: "invalid escaped sequence in string",
                                pos: backslash,
                            })
                        },
                    }
                },
                _ => {
                    // Multi-byte UTF-8 passes through verbatim.
                    if c < 0x80 {
                        out.push(c as char);
                    } else {
                        let start = self.pos - 1;
                        let width = utf8_width(c);
                        let end = start + width;
                        if end > self.input.len() {
                            return Err(self.err("unexpected end of data"));
                        }
                        self.pos = end;
                        match std::str::from_utf8(&self.input[start..end]) {
                            Ok(text) => out.push_str(text),
                            Err(_) => return Err(self.err("invalid UTF-8 character in string")),
                        }
                    }
                },
            }
        }
    }

    fn hex4(&mut self, backslash: usize) -> Result<u32, ParseError> {
        let Some(hex) = self.input.get(self.pos..self.pos + 4) else {
            return Err(ParseError {
                msg: "invalid escaped sequence in string",
                pos: backslash,
            });
        };
        let Ok(text) = std::str::from_utf8(hex) else {
            return Err(ParseError {
                msg: "invalid escaped sequence in string",
                pos: backslash,
            });
        };
        let value = match u32::from_str_radix(text, 16) {
            Ok(value) => value,
            Err(_) => {
                return Err(ParseError {
                    msg: "invalid escaped sequence in string",
                    pos: backslash,
                })
            },
        };
        self.pos += 4;
        Ok(value)
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    // yyjson points at the number's first digit
                    return Err(ParseError {
                        msg: "number with leading zero is not allowed",
                        pos: start,
                    });
                }
            },
            Some(c) if c.is_ascii_digit() => {
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.pos += 1;
                }
            },
            _ => return Err(self.err("unexpected character, expected a JSON value")),
        }

        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
                return Err(self.err("invalid number format"));
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
                return Err(self.err("invalid number format"));
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.pos += 1;
            }
        }

        let text = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| self.err("invalid number format"))?;
        if is_float {
            text.parse::<f64>()
                .map(Value::Float)
                .map_err(|_| self.err("invalid number format"))
        } else if text.starts_with('-') {
            text.parse::<i64>()
                .map(Value::Int)
                .map_err(|_| self.err("invalid number format"))
        } else {
            match text.parse::<u64>() {
                Ok(u) => Ok(Value::UInt(u)),
                Err(_) => text
                    .parse::<f64>()
                    .map(Value::Float)
                    .map_err(|_| self.err("invalid number format")),
            }
        }
    }
}

fn utf8_width(first: u8) -> usize {
    match first {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 1,
    }
}

/// Converts a 0-based byte offset to yyjson's 1-based (row, col).
pub fn locate_pos(input: &str, pos: usize) -> (usize, usize) {
    let pos = pos.min(input.len());
    let mut row = 1;
    let mut line_start = 0;
    for (index, byte) in input.as_bytes()[..pos].iter().enumerate() {
        if *byte == b'\n' {
            row += 1;
            line_start = index + 1;
        }
    }
    (row, pos - line_start + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(input: &str) -> Value {
        let Ok(value) = Parser::parse(input, true) else {
            panic!("expected {input} to parse");
        };
        value
    }

    fn parse_err(input: &str, jsonc: bool) -> ParseError {
        let Err(error) = Parser::parse(input, jsonc) else {
            panic!("expected {input} to fail");
        };
        error
    }

    #[test]
    fn parses_scalars_and_containers() {
        assert_eq!(parse_ok("true"), Value::Bool(true));
        assert_eq!(parse_ok("null"), Value::Null);
        assert_eq!(parse_ok("-12"), Value::Int(-12));
        assert_eq!(parse_ok("18446744073709551615"), Value::UInt(u64::MAX));
        assert_eq!(parse_ok("1.5"), Value::Float(1.5));
        let object = parse_ok(r#"{"a": 1, "b": ["x", false]}"#);
        assert_eq!(object.get("a"), Some(&Value::UInt(1)));
        assert_eq!(
            object.get("b"),
            Some(&Value::Array(vec![Value::Str("x".into()), Value::Bool(false)]))
        );
        // duplicate keys: last wins
        assert_eq!(parse_ok(r#"{"k":1,"k":2}"#).get("k"), Some(&Value::UInt(2)));
    }

    #[test]
    fn jsonc_allows_comments_and_trailing_commas() {
        let text = "{\n // line\n /* block */\n \"a\": [1, 2,],\n}";
        assert_eq!(parse_ok(text).get("a"), Some(&Value::Array(vec![Value::UInt(1), Value::UInt(2)])));
        // strict mode rejects both; the comment is hit before the comma here
        assert_eq!(parse_err(text, false).msg, "unexpected character, expected a JSON value");
        assert_eq!(parse_err("[1,]", false).msg, "trailing comma is not allowed");
        assert_eq!(
            parse_err("// only a comment\n", false),
            ParseError { msg: "unexpected character, expected a JSON value", pos: 0 }
        );
    }

    #[test]
    fn errors_match_yyjson_messages_and_positions() {
        let cases: [(&str, bool, &'static str, usize); 10] = [
            ("[1,]", false, "trailing comma is not allowed", 2),
            (r#"{"a":1,}"#, false, "trailing comma is not allowed", 6),
            (r#"{"a":}"#, false, "unexpected character, expected a JSON value", 5),
            ("{", false, "unexpected end of data", 1),
            (r#"{"a" 1}"#, false, "unexpected character, expected ':' after key", 5),
            ("[1 2]", false, "unexpected character, expected ',' or ']'", 3),
            ("tru", false, "unexpected end of data", 3),
            (r#"{"a":01}"#, false, "number with leading zero is not allowed", 5),
            (r#"{"a":1"#, false, "unexpected end of data", 6),
            ("[1,", false, "unexpected end of data", 3),
        ];
        for (input, jsonc, msg, pos) in cases {
            let error = parse_err(input, jsonc);
            assert_eq!(error.msg, msg, "input {input}");
            assert_eq!(error.pos, pos, "input {input}");
        }
    }

    #[test]
    fn parses_string_escapes() {
        assert_eq!(
            parse_ok(r#""a\n\t\"\\\/\u0041é""#),
            Value::Str("a\n\t\"\\/Aé".into())
        );
        assert_eq!(parse_ok(r#""😀""#), Value::Str("😀".into()));
        assert_eq!(parse_ok(r#""😀""#), Value::Str("😀".into()));
        // escape errors point at the backslash, like yyjson
        assert_eq!(
            parse_err(r#""\u12""#, false),
            ParseError { msg: "invalid escaped sequence in string", pos: 1 }
        );
        assert_eq!(
            parse_err(r#""\q""#, false),
            ParseError { msg: "invalid escaped sequence in string", pos: 1 }
        );
        assert_eq!(
            parse_err(r#""\ud800""#, false),
            ParseError { msg: "no low surrogate in string", pos: 1 }
        );
        assert_eq!(
            parse_err(r#""\udc00""#, false),
            ParseError { msg: "invalid high surrogate in string", pos: 1 }
        );
        assert_eq!(parse_ok(r#""\ud800\udc00""#), Value::Str("𐀀".into()));
    }

    #[test]
    fn locate_pos_counts_rows() {
        assert_eq!(locate_pos("ab\ncd", 0), (1, 1));
        assert_eq!(locate_pos("ab\ncd", 3), (2, 1));
        assert_eq!(locate_pos("ab\ncd", 5), (2, 3));
        // out-of-range positions clamp to the end
        assert_eq!(locate_pos("ab", 99), (1, 3));
    }
}
