//! The format-args engine: common/impl/format.c ported — the parser behind
//! every module's custom `format` string and ffPrintLogoAndKey's custom
//! key. Grammar: `{}`/`{N}` positionals and `{name}` named args, the
//! conditionals `{?arg}`…`{?}` / `{/arg}`…`{/}`, `{#color}` and `{#}` as a
//! reset, `{$N}`/`{$-N}` constants or `{$NAME}` environment variables, the
//! stop marker `{-}`, and the separator specs `~start,end` (byte range),
//! `:N` (truncate, trim trailing space) and `<N`/`>N`/`|N` (pad, ellipsis
//! on negative N). `{{` escapes to `{`; anything else that fails to
//! resolve prints verbatim (appendInvalidPlaceholder). The TTY expansion
//! closes with a reset; pipe mode drops colors entirely.
//!
//! Two divergences from the C, both documented: truncation and ranges cut
//! at the nearest UTF-8 char boundary below the length (upstream writes
//! the raw bytes), and the `lua:` prefix is treated as a plain format
//! string (upstream runs a Lua interpreter when compiled with one).

use std::cell::RefCell;

use crate::format::DisplayFormat;

/// FFArgType's rendered subset (common/argType.h): the types
/// ffFormatAppendFormatArg turns into text. STRING and STRBUF collapse
/// into one borrowed-str variant — both append the bytes and both count
/// as "set" only when non-empty. FF_ARG_TYPE_BUFFER keeps upstream's
/// placeholder rendering ("buffer(N bytes)") and its never-set status.
///
/// The enum ports FFArgType as a unit; variants without a consumer yet
/// (the per-module format wiring lands module by module) are allowed dead.
#[allow(dead_code)]
pub enum ArgValue<'a> {
    Null,
    Int(i32),
    UInt(u32),
    UInt64(u64),
    UInt16(u16),
    UInt8(u8),
    Str(&'a str),
    Float(f32),
    Double(f64),
    Bool(bool),
    List(&'a [String]),
    Buffer(u32),
}

/// FFformatarg: one `{name}` resolvable from a format string.
pub struct FormatArg<'a> {
    pub name: &'a str,
    pub value: ArgValue<'a>,
}

/// The parser's view of the config state format.c reads off `instance`:
/// the pipe gate ({#color} and the closing reset), display.constants
/// ({$N}), the display format (FLOAT/DOUBLE go through ffStrbufAppendDouble),
/// and the color parser (invalid {#code} defers its exit(479) like the
/// common-arg colors).
pub struct EngineEnv<'a> {
    pub pipe: bool,
    pub constants: &'a [String],
    pub df: &'a DisplayFormat,
    pub colors: crate::cli::ColorState,
    pub pending: &'a RefCell<Option<(i32, String)>>,
}

/// ffFormatAppendFormatArg: one argument's text. FLOAT/DOUBLE pass the
/// display's fraction knobs with the format-args polarity
/// (`!= NEVER` — unlike localip's `== ALWAYS`, NEVER keeps yyjson's
/// padded output here).
pub fn format_arg_append(out: &mut String, arg: &FormatArg, df: &DisplayFormat) {
    match &arg.value {
        ArgValue::Null => {},
        ArgValue::Int(value) => out.push_str(&value.to_string()),
        ArgValue::UInt(value) => out.push_str(&value.to_string()),
        ArgValue::UInt64(value) => out.push_str(&value.to_string()),
        ArgValue::UInt16(value) => out.push_str(&value.to_string()),
        ArgValue::UInt8(value) => out.push_str(&value.to_string()),
        ArgValue::Str(value) => out.push_str(value),
        ArgValue::Float(value) => {
            crate::format::append_double(
                out,
                *value as f64,
                df.fraction_ndigits,
                df.fraction_trailing_zeros != crate::format::FractionTrailingZeros::Never,
            );
        },
        ArgValue::Double(value) => {
            crate::format::append_double(
                out,
                *value,
                df.fraction_ndigits,
                df.fraction_trailing_zeros != crate::format::FractionTrailingZeros::Never,
            );
        },
        ArgValue::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        ArgValue::List(list) => {
            for (i, item) in list.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(item);
            }
        },
        ArgValue::Buffer(length) => out.push_str(&format!("buffer({length} bytes)")),
    }
}

/// strToInt32: strtoll base 10 clamped into an int32 — out-of-range values
/// saturate instead of wrapping, and the consumed length distinguishes
/// "no digits" (strtoll's pend == str) from a parsed value. Leading C
/// whitespace and a sign are consumed before the digits.
fn str_to_int32(value: &str) -> (i32, usize) {
    let bytes = value.as_bytes();
    let mut offset = 0;
    while offset < bytes.len() && matches!(bytes[offset], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
        offset += 1;
    }
    let negative = offset < bytes.len() && bytes[offset] == b'-';
    if offset < bytes.len() && (bytes[offset] == b'+' || bytes[offset] == b'-') {
        offset += 1;
    }
    let start = offset;
    while offset < bytes.len() && bytes[offset].is_ascii_digit() {
        offset += 1;
    }
    if offset == start {
        // strtoll consumed nothing: value 0, pend == str.
        return (0, 0);
    }
    let magnitude = value[start..offset].parse::<i128>().unwrap_or(i128::from(i64::MAX));
    let signed = if negative { -magnitude } else { magnitude };
    (signed.clamp(i32::MIN as i128, i32::MAX as i128) as i32, offset)
}

/// getArgumentIndex: the placeholder head to an argument number — empty
/// means "use the sequential counter" (0), decimal digits a 1-based index
/// (strtoul whole-string parse; 0 falls through to the counter, anything
/// past the count or with trailing garbage is invalid), an English letter
/// a case-insensitive name match, everything else invalid. `positional_len`
/// bounds the numeric path; the named path searches the whole list, so
/// prebuilt-key callers that don't know their module's positional args
/// pass 0 and keep {N}/{} verbatim.
const INVALID_INDEX: u32 = u32::MAX;

fn get_argument_index(spec: &str, args: &[FormatArg], positional_len: u32) -> u32 {
    let Some(&first) = spec.as_bytes().first() else {
        return 0;
    };
    if first.is_ascii_digit() {
        // strtoul: full-string consume and the count check; "0" and an
        // overflow both carry their own outcomes (counter vs invalid).
        let digits_end = spec.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(spec.len());
        let parsed = match spec[..digits_end].parse::<u64>() {
            Ok(value) => value,
            Err(_) => return INVALID_INDEX,
        };
        if digits_end != spec.len() || parsed > positional_len as u64 {
            return INVALID_INDEX;
        }
        return parsed as u32;
    }
    if first.is_ascii_alphabetic() {
        for (i, arg) in args.iter().enumerate() {
            if !arg.name.is_empty() && arg.name.eq_ignore_ascii_case(spec) {
                return i as u32 + 1;
            }
        }
    }
    INVALID_INDEX
}

/// formatArgSet: the conditional truth test — set means the value exists
/// and reads > 0 (strings non-empty, bools true). BUFFER has no disjunct
/// in upstream's || chain, so a buffer argument never counts as set.
fn format_arg_set(arg: &FormatArg) -> bool {
    match &arg.value {
        ArgValue::Null | ArgValue::Buffer(_) => false,
        ArgValue::Int(value) => *value > 0,
        ArgValue::UInt(value) => *value > 0,
        ArgValue::UInt64(value) => *value > 0,
        ArgValue::UInt16(value) => *value > 0,
        ArgValue::UInt8(value) => *value > 0,
        ArgValue::Str(value) => !value.is_empty(),
        ArgValue::Float(value) => *value > 0.0,
        ArgValue::Double(value) => *value > 0.0,
        ArgValue::Bool(value) => *value,
        ArgValue::List(list) => !list.is_empty(),
    }
}

/// ansiEscapeLength: the length of the CSI sequence at `bytes` — `ESC [`
/// then parameter bytes (0x30-0x3F), intermediate bytes (0x20-0x2F) and a
/// final byte (0x40-0x7E) — or 0 when there is none.
fn ansi_escape_length(bytes: &[u8]) -> usize {
    if bytes.first() != Some(&b'\x1b') || bytes.get(1) != Some(&b'[') {
        return 0;
    }
    let mut p = 2;
    while p < bytes.len() && (0x30..=0x3f).contains(&bytes[p]) {
        p += 1;
    }
    while p < bytes.len() && (0x20..=0x2f).contains(&bytes[p]) {
        p += 1;
    }
    if p >= bytes.len() || !(0x40..=0x7e).contains(&bytes[p]) {
        return 0;
    }
    p + 1
}

/// skipAnsiEscape: strip every leading CSI off `temp` (they don't count
/// toward the truncation/padding length and go straight to the output),
/// then peel the trailing run of CSIs that exactly tiles the suffix into
/// `trailing` so the caller can re-append it after any padding.
fn skip_ansi_escape(temp: &mut String, out: &mut String, trailing: &mut String) {
    let prefix = {
        let bytes = temp.as_bytes();
        let mut prefix = 0;
        while bytes.len() - prefix >= 2 {
            let len = ansi_escape_length(&bytes[prefix..]);
            if len == 0 {
                break;
            }
            prefix += len;
        }
        prefix
    };
    if prefix > 0 {
        out.push_str(&temp[..prefix]);
        temp.drain(..prefix);
    }

    let end = {
        let bytes = temp.as_bytes();
        let mut end = bytes.len();
        while end >= 3 {
            let mut start = end - 1;
            while start > 0 && bytes[start] != b'\x1b' {
                start -= 1;
            }
            let len = ansi_escape_length(&bytes[start..]);
            if len == 0 || start + len != end {
                break;
            }
            end = start;
        }
        end
    };
    if end < temp.len() {
        trailing.push_str(&temp[end..]);
        temp.truncate(end);
    }
}

/// ffStrbufTrimRightSpace's C isspace set.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// ffStrbufSubstrBefore is byte-based; the port cuts at the nearest char
/// boundary below the length so its Strings stay valid UTF-8 (upstream
/// writes the raw bytes — a mid-codepoint cut is a documented divergence).
fn byte_floor(value: &str, mut end: usize) -> usize {
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// findConditionalEnd: the index of the `}` closing the `{<marker>}`
/// conditional opened at `close` (the index of the opening placeholder's
/// `}`), or the format length when there is none. Only a bare `{?}`/`{/}`
/// closes; a nested opener with a VALID argument deepens, so an invalid
/// opener inside the skipped part never hides the outer close.
fn find_conditional_end(template: &str, close: usize, marker: u8, args: &[FormatArg], positional_len: u32) -> usize {
    let bytes = template.as_bytes();
    let mut depth = 0;
    let mut i = close + 1;
    while i < bytes.len() {
        if bytes[i] != b'{' {
            i += 1;
            continue;
        }
        i += 1;
        if i >= bytes.len() {
            break;
        }
        // `{{` is an escaped brace, not a placeholder start.
        if bytes[i] == b'{' {
            i += 1;
            continue;
        }
        let i_end = template[i..].find('}').map_or(bytes.len(), |at| i + at);
        if bytes[i] == marker {
            if i_end - i == 1 {
                if depth == 0 {
                    return i_end;
                }
                depth -= 1;
            } else {
                let index = get_argument_index(&template[i + 1..i_end], args, positional_len);
                if index != 0 && index != INVALID_INDEX {
                    depth += 1;
                }
            }
        }
        i = i_end + 1;
    }
    bytes.len()
}

/// ffParseFormatString without the lua: prefix: parse the format string
/// against `args`. `positional_len` is the count the sequential counter
/// and numeric placeholders resolve against (the full arg list length for
/// real format strings; 0 for prebuilt keys whose module positionals the
/// port doesn't model).
pub fn parse_format_string(template: &str, args: &[FormatArg], positional_len: u32, env: &EngineEnv) -> String {
    let mut out = String::new();
    let bytes = template.as_bytes();
    let mut arg_counter = 0u32;
    let mut num_open_ifs = 0u32;
    let mut num_open_not_ifs = 0u32;
    let mut i = 0;
    while i < bytes.len() {
        // A literal run copies over whole; `{` never appears inside a
        // multi-byte UTF-8 sequence, so the search lands on boundaries.
        if bytes[i] != b'{' {
            let next = bytes[i..]
                .iter()
                .position(|&b| b == b'{')
                .map_or(bytes.len(), |at| i + at);
            out.push_str(&template[i..next]);
            i = next;
            continue;
        }

        i += 1;
        // Unmatched trailing '{'.
        if i >= bytes.len() {
            out.push('{');
            break;
        }
        // `{{` evaluates to a single `{`.
        if bytes[i] == b'{' {
            out.push('{');
            i += 1;
            continue;
        }

        // The placeholder value runs to the next `}` (or the end); `i`
        // stops on that `}` like upstream, and the invalid-placeholder
        // printer closes with one whenever it was actually found.
        let i_end = template[i..].find('}').map_or(bytes.len(), |at| i + at);
        let value = &template[i..i_end];
        let closed = |out: &mut String, start: &str, text: &str| {
            // appendInvalidPlaceholder: verbatim bytes, brace only when the
            // placeholder had a closer. The conditional branches strip
            // their marker off `text` first, like upstream's
            // ffStrbufSubstrAfter mutation.
            out.push_str(start);
            out.push_str(text);
            if i_end < bytes.len() {
                out.push('}');
            }
        };

        if value.len() == 1 {
            match value.as_bytes()[0] {
                // `{-}` stops the whole expansion.
                b'-' => break,
                // A bare close with nothing open prints verbatim.
                b'?' => {
                    if num_open_ifs == 0 {
                        closed(&mut out, "{", value);
                    } else {
                        num_open_ifs -= 1;
                    }
                    i = i_end + 1;
                    continue;
                },
                b'/' => {
                    if num_open_not_ifs == 0 {
                        closed(&mut out, "{", value);
                    } else {
                        num_open_not_ifs -= 1;
                    }
                    i = i_end + 1;
                    continue;
                },
                // `{#}` closes a color; pipe mode prints nothing.
                b'#' => {
                    if !env.pipe {
                        out.push_str("\x1b[m");
                    }
                    i = i_end + 1;
                    continue;
                },
                _ => {},
            }
        }

        match value.as_bytes().first() {
            // `{?arg}`: print the block while the argument is set.
            Some(&b'?') => {
                let index = get_argument_index(&value[1..], args, positional_len);
                if index == 0 || index == INVALID_INDEX {
                    closed(&mut out, "{?", &value[1..]);
                } else if format_arg_set(&args[index as usize - 1]) {
                    num_open_ifs += 1;
                } else {
                    // Skip to the matching close without printing.
                    i = find_conditional_end(template, i, b'?', args, positional_len) + 1;
                    continue;
                }
            },
            // `{/arg}`: the negated form.
            Some(&b'/') => {
                let index = get_argument_index(&value[1..], args, positional_len);
                if index == 0 || index == INVALID_INDEX {
                    closed(&mut out, "{/", &value[1..]);
                } else if !format_arg_set(&args[index as usize - 1]) {
                    num_open_not_ifs += 1;
                } else {
                    i = find_conditional_end(template, i, b'/', args, positional_len) + 1;
                    continue;
                }
            },
            // `{#code}`: a color, dropped wholesale in pipe mode.
            Some(&b'#') => {
                if !env.pipe {
                    out.push_str("\x1b[");
                    out.push_str(&env.colors.parse_color_deferred(&value[1..], env.pending));
                    out.push('m');
                }
            },
            // `{$name}` an environment variable, `{$N}` a constant.
            Some(&b'$') => {
                let (signed, consumed) = str_to_int32(&value[1..]);
                if consumed == 0 {
                    match std::env::var_os(&value[1..]) {
                        Some(env_value) => out.push_str(&env_value.to_string_lossy()),
                        None => closed(&mut out, "{", value),
                    }
                } else {
                    // A constant: 1-based, negatives count from the end.
                    // A too-negative index wraps far past the end in the C
                    // arithmetic; the range check rejects it all the same.
                    let index = if signed < 0 {
                        env.constants.len() as i64 + signed as i64
                    } else {
                        signed as i64 - 1
                    };
                    if consumed != value.len() - 1 || index < 0 || index as usize >= env.constants.len() {
                        closed(&mut out, "{", value);
                    } else {
                        out.push_str(&env.constants[index as usize]);
                    }
                }
            },
            _ => {},
        }

        // The `{?}`/`{/}`/`{#}`/`{$…}` paths above advanced nothing by
        // themselves; move past the placeholder's closer for them.
        if matches!(
            value.as_bytes().first(),
            Some(&b'?') | Some(&b'/') | Some(&b'#') | Some(&b'$')
        ) {
            i = i_end + 1;
            continue;
        }

        // An argument reference, possibly with a separator spec.
        let sep_at = value
            .bytes()
            .position(|b| matches!(b, b':' | b'<' | b'>' | b'|' | b'~'));
        let (head, sep, tail) = match sep_at {
            Some(at) => (&value[..at], value.as_bytes()[at], Some(&value[at + 1..])),
            None => (value, 0, None),
        };
        let mut index = get_argument_index(head, args, positional_len);
        if index == 0 {
            arg_counter += 1;
            index = arg_counter;
            if index > positional_len {
                closed(&mut out, "{", value);
                i = i_end + 1;
                continue;
            }
        }
        if index == INVALID_INDEX {
            closed(&mut out, "{", value);
            i = i_end + 1;
            continue;
        }
        let arg = &args[index as usize - 1];
        let Some(sep) = (sep != 0).then_some(sep) else {
            format_arg_append(&mut out, arg, env.df);
            i = i_end + 1;
            continue;
        };
        let Some(tail) = tail else {
            format_arg_append(&mut out, arg, env.df);
            i = i_end + 1;
            continue;
        };

        if sep == b'~' {
            // A byte range of the rendered value; the rendered prefix
            // escapes go out untouched and a trailing escape run survives
            // the cut, re-appended after the range.
            let mut temp = String::new();
            format_arg_append(&mut temp, arg, env.df);
            let mut trailing = String::new();
            skip_ansi_escape(&mut temp, &mut out, &mut trailing);

            let (start, mut consumed) = str_to_int32(tail);
            let mut end = 0i64;
            let mut has_end = false;
            if tail.as_bytes().get(consumed) == Some(&b',') {
                has_end = true;
                let (parsed, second) = str_to_int32(&tail[consumed + 1..]);
                end = parsed as i64;
                consumed += 1 + second;
            }
            if consumed == tail.len() {
                let mut start = start as i64;
                if start < 0 {
                    start += temp.len() as i64;
                }
                if start >= 0 && (start as usize) < temp.len() {
                    if !has_end {
                        let from = byte_floor(&temp, start as usize);
                        out.push_str(&temp[from..]);
                    } else {
                        if end < 0 {
                            end += temp.len() as i64;
                        }
                        if end >= 0 && end > temp.len() as i64 {
                            end = temp.len() as i64;
                        }
                        if end > start {
                            let from = byte_floor(&temp, start as usize);
                            let to = byte_floor(&temp, end as usize);
                            out.push_str(&temp[from..to]);
                        }
                    }
                }
            }
            if !trailing.is_empty() {
                out.push_str(&trailing);
            }
            if consumed != tail.len() {
                closed(&mut out, "{", value);
            }
            i = i_end + 1;
            continue;
        }

        // Truncation and padding: `:N` cuts and trims trailing spaces,
        // `<N`/`>N`/`|N` pad left/right/both, a negative N takes an
        // ellipsis. Embedded escapes leave the padding math alone.
        let (trunc_raw, consumed) = str_to_int32(tail);
        if consumed != tail.len() {
            closed(&mut out, "{", value);
            i = i_end + 1;
            continue;
        }
        let mut ellipsis = false;
        let mut trunc = trunc_raw;
        if trunc < 0 {
            ellipsis = true;
            trunc = if trunc == i32::MIN { i32::MAX } else { -trunc };
        }
        let trunc = trunc as u32;
        let mut temp = String::new();
        format_arg_append(&mut temp, arg, env.df);
        let mut trailing = String::new();
        skip_ansi_escape(&mut temp, &mut out, &mut trailing);

        let length = temp.len() as u32;
        if length == trunc {
            out.push_str(&temp);
        } else if length > trunc {
            if sep == b':' {
                let mut cut = byte_floor(&temp, trunc as usize);
                // ffStrbufTrimRightSpace walks back over C isspace bytes;
                // the trim can only ever remove ASCII, so `cut` stays a
                // char boundary.
                let temp_bytes = temp.as_bytes();
                while cut > 0 && is_space(temp_bytes[cut - 1]) {
                    cut -= 1;
                }
                out.push_str(&temp[..cut]);
            } else {
                let cut = byte_floor(&temp, if ellipsis { trunc - 1 } else { trunc } as usize);
                out.push_str(&temp[..cut]);
                if ellipsis {
                    out.push('…');
                }
            }
        } else if sep == b':' {
            out.push_str(&temp);
        } else {
            let padding = (trunc - length) as usize;
            if sep == b'<' {
                out.push_str(&temp);
                out.push_str(&" ".repeat(padding));
            } else if sep == b'>' {
                out.push_str(&" ".repeat(padding));
                out.push_str(&temp);
            } else {
                let half = padding / 2;
                out.push_str(&" ".repeat(half));
                out.push_str(&temp);
                out.push_str(&" ".repeat(padding - half));
            }
        }
        if !trailing.is_empty() {
            out.push_str(&trailing);
        }
        i = i_end + 1;
    }

    if !env.pipe {
        out.push_str("\x1b[m");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn render_with(template: &str, args: &[FormatArg], pipe: bool, constants: &[String]) -> String {
        let df = DisplayFormat::default();
        let pending = RefCell::new(None);
        parse_format_string(
            template,
            args,
            args.len() as u32,
            &EngineEnv {
                pipe,
                constants,
                df: &df,
                colors: crate::cli::ColorState::fresh(),
                pending: &pending,
            },
        )
    }

    fn render(template: &str, args: &[FormatArg]) -> String {
        render_with(template, args, false, &[])
    }

    fn pipe_render(template: &str, args: &[FormatArg]) -> String {
        render_with(template, args, true, &[])
    }

    fn named(name: &'static str, value: &'static str) -> FormatArg<'static> {
        FormatArg { name, value: ArgValue::Str(value) }
    }

    const ABC: &[FormatArg] = &[
        FormatArg { name: "a", value: ArgValue::Str("abcdef") },
        FormatArg { name: "b", value: ArgValue::Str("") },
        FormatArg { name: "c", value: ArgValue::Str("x") },
    ];

    #[test]
    fn literals_and_escapes() {
        assert_eq!(render("hello world", &[]), "hello world\x1b[m");
        assert_eq!(render("a{{b", &[]), "a{b\x1b[m");
        assert_eq!(render("a{{b}c", &[]), "a{b}c\x1b[m");
        // An unmatched trailing brace prints itself.
        assert_eq!(render("x{", &[]), "x{\x1b[m");
        assert_eq!(render("{unclosed", &[]), "{unclosed\x1b[m");
    }

    #[test]
    fn positionals() {
        // Bare placeholders count sequentially and don't share with
        // explicit numbers ({2} leaves the counter at 1); a counter past
        // the end prints verbatim. {b} renders empty.
        assert_eq!(render("{}/{}/{}", ABC), "abcdef//x\x1b[m");
        assert_eq!(render("{}/{}/{}/{}", ABC), "abcdef//x/{}\x1b[m");
        assert_eq!(render("a{}/{2}/{}", ABC), "aabcdef//\x1b[m");
        // "0" and "01" are the counter and arg 1; trailing garbage and an
        // overflow are verbatim.
        assert_eq!(render("{0}|{01}", ABC), "abcdef|abcdef\x1b[m");
        assert_eq!(render("{4}|{1x}|{99999999999}", ABC), "{4}|{1x}|{99999999999}\x1b[m");
        // Names resolve case-insensitively; unknown names stay verbatim.
        assert_eq!(render("{a}/{A}/{nope}", ABC), "abcdef/abcdef/{nope}\x1b[m");
        // `{-}` stops the expansion; the reset still closes.
        assert_eq!(render("a{-}b", &[]), "a\x1b[m");
        assert_eq!(render("{-x}", ABC), "{-x}\x1b[m");
    }

    #[test]
    fn arg_types() {
        let args = [
            FormatArg { name: "i", value: ArgValue::Int(-5) },
            FormatArg { name: "u", value: ArgValue::UInt(7) },
            FormatArg { name: "u64", value: ArgValue::UInt64(u64::MAX) },
            FormatArg { name: "u8", value: ArgValue::UInt8(200) },
            FormatArg { name: "f", value: ArgValue::Float(2.5) },
            FormatArg { name: "d", value: ArgValue::Double(2.0) },
            FormatArg { name: "b", value: ArgValue::Bool(true) },
            FormatArg { name: "list", value: ArgValue::List(&["a".into(), "b".into()]) },
            FormatArg { name: "buf", value: ArgValue::Buffer(3) },
            FormatArg { name: "nil", value: ArgValue::Null },
        ];
        assert_eq!(
            render("{i}|{u}|{u64}|{u8}|{f}|{d}|{b}|{list}|{buf}|{nil}", &args),
            // FLOAT/DOUBLE keep yyjson's padded output: the engine passes
            // `!= NEVER`, so the DEFAULT type pads too.
            "-5|7|18446744073709551615|200|2.50|2.00|true|a, b|buffer(3 bytes)|\x1b[m"
        );
    }

    #[test]
    fn float_polarity_is_not_never() {
        // The format-args engine passes `!= NEVER` to ffStrbufAppendDouble,
        // so NEVER keeps yyjson's padded output — the opposite of localip's
        // `== ALWAYS` speed formatter.
        let df = DisplayFormat::default();
        let pending = RefCell::new(None);
        let args = [FormatArg { name: "d", value: ArgValue::Double(2.5) }];
        let variants = [
            // DEFAULT and ALWAYS pass `!= NEVER` (true): padded. NEVER is
            // the only stripping variant in the format-args engine.
            (crate::format::FractionTrailingZeros::Default, "2.50"),
            (crate::format::FractionTrailingZeros::Always, "2.50"),
            (crate::format::FractionTrailingZeros::Never, "2.5"),
        ];
        for (zeros, expected) in variants {
            let df = DisplayFormat { fraction_trailing_zeros: zeros, ..df.clone() };
            let out = parse_format_string(
                "{d}",
                &args,
                1,
                &EngineEnv {
                    pipe: true,
                    constants: &[],
                    df: &df,
                    colors: crate::cli::ColorState::fresh(),
                    pending: &pending,
                },
            );
            assert_eq!(out, expected);
        }
    }

    #[test]
    fn colors() {
        // TTY renders {#code} and closes with a reset; pipe drops both.
        assert_eq!(render("{#31}x", ABC), "\x1b[31mx\x1b[m");
        assert_eq!(pipe_render("{#31}x", ABC), "x");
        // `{#}` resets; an empty code is an empty parameter string.
        assert_eq!(render("{#}x", ABC), "\x1b[mx\x1b[m");
        assert_eq!(pipe_render("{#}x", ABC), "x");
        // An invalid code defers its exit(479) and renders an empty SGR.
        let df = DisplayFormat::default();
        let pending = RefCell::new(None);
        let env = EngineEnv {
            pipe: false,
            constants: &[],
            df: &df,
            colors: crate::cli::ColorState::fresh(),
            pending: &pending,
        };
        assert_eq!(parse_format_string("{#qq}x", ABC, 3, &env), "\x1b[mx\x1b[m");
        let slot = pending.borrow();
        assert_eq!(
            slot.as_ref().map(|(code, message)| (message.as_str(), *code)),
            Some(("Error: invalid color code found: qq", 479))
        );
    }

    #[test]
    fn env_and_constants() {
        let constants: [String; 2] = ["k0".into(), "k1".into()];
        let render_const = |template: &str| render_with(template, ABC, true, &constants);
        assert_eq!(render_const("{$1}|{$2}"), "k0|k1");
        // 0 is out of range (1-based), {$+1} parses as 1, negatives count
        // from the end, garbage tails are verbatim.
        assert_eq!(render_const("{$0}|{$+1}|{$-1}|{$-3}|{$1x}"), "{$0}|k0|k1|{$-3}|{$1x}");
        // No digits: an environment variable. Unset names stay verbatim.
        // SAFETY: single-threaded use of a test-exclusive variable name.
        unsafe { std::env::set_var("FF_FORMAT_ARGS_TEST", "hello") };
        assert_eq!(render_const("{$FF_FORMAT_ARGS_TEST}|{$FF_FORMAT_ARGS_NOPE}"), "hello|{$FF_FORMAT_ARGS_NOPE}");
        assert_eq!(render_const("{$ }"), "{$ }");
    }

    #[test]
    fn conditionals() {
        // {a} is set, {b} is empty.
        assert_eq!(render("{?a}yes{?}", ABC), "yes\x1b[m");
        assert_eq!(render("{?b}no{?}", ABC), "\x1b[m");
        assert_eq!(render("{/b}yes{/}", ABC), "yes\x1b[m");
        assert_eq!(render("{/a}no{/}", ABC), "\x1b[m");
        // An invalid conditional prints verbatim; a bare close with
        // nothing open too.
        assert_eq!(render("{?x}y{?}", ABC), "{?x}y{?}\x1b[m");
        assert_eq!(render("{?}x", ABC), "{?}x\x1b[m");
        // A skipped block hides nested conditionals — an invalid opener
        // never deepens, so the outer close still matches.
        assert_eq!(render("{?b}y{?nope}z{?}", ABC), "\x1b[m");
        // A valid nested opener deepens: two closes to end.
        assert_eq!(render("{?a}m{?a}n{?}{?}", ABC), "mn\x1b[m");
        // The block stays open to the end when the close is missing.
        assert_eq!(render("{?a}x", ABC), "x\x1b[m");
        // Conditionals resolve numbers and names alike.
        assert_eq!(render("{?3}y{?}", ABC), "y\x1b[m");
        // A nested unset conditional skips to its own close.
        assert_eq!(render("{?a}x{?b}y{?}{?}", ABC), "x\x1b[m");
    }

    #[test]
    fn ranges() {
        assert_eq!(render("{a~1}", ABC), "bcdef\x1b[m");
        assert_eq!(render("{a~1,3}", ABC), "bc\x1b[m");
        assert_eq!(render("{a~-1}", ABC), "f\x1b[m");
        assert_eq!(render("{a~-2,-1}", ABC), "e\x1b[m");
        assert_eq!(render("{a~,2}", ABC), "ab\x1b[m");
        // An empty end is end 0; a valid range past the string is empty.
        assert_eq!(render("{a~1,}", ABC), "\x1b[m");
        assert_eq!(render("{a~9}", ABC), "\x1b[m");
        assert_eq!(render("{a~-9}", ABC), "\x1b[m");
        // Garbage tails are verbatim, with the closer restored.
        assert_eq!(render("{a~1x}", ABC), "{a~1x}\x1b[m");
        assert_eq!(render("{a~1,x}", ABC), "{a~1,x}\x1b[m");
    }

    #[test]
    fn trunc_and_padding() {
        assert_eq!(render("{a:3}", ABC), "abc\x1b[m");
        // `:` trims the trailing spaces of the cut.
        assert_eq!(render("{b:2}!", ABC), "!\x1b[m");
        assert_eq!(render("{s:3}!", &[named("s", "a  b")]), "a!\x1b[m");
        // A negative `:` length cuts without an ellipsis; only the align
        // specs (< > |) take one.
        assert_eq!(render("{a:-3}", ABC), "abc\x1b[m");
        assert_eq!(render("{a<-3}", ABC), "ab…\x1b[m");
        assert_eq!(render("{a<-1}", ABC), "…\x1b[m");
        // Padding fills with spaces; `|` splits the run, extra right.
        assert_eq!(render("[{c<3}]", ABC), "[x  ]\x1b[m");
        assert_eq!(render("[{c>3}]", ABC), "[  x]\x1b[m");
        assert_eq!(render("[{a|8}]", ABC), "[ abcdef ]\x1b[m");
        assert_eq!(render("[{a|7}]", ABC), "[abcdef ]\x1b[m");
        // A shorter value passes `:` through unpadded but pads the align
        // specs.
        assert_eq!(render("[{c:9}]|[{a<9}]", ABC), "[x]|[abcdef   ]\x1b[m");
        // Exact length passes through.
        assert_eq!(render("[{a:6}]", ABC), "[abcdef]\x1b[m");
        // Garbage lengths are verbatim.
        assert_eq!(render("{a:x}|{a:1x}", ABC), "{a:x}|{a:1x}\x1b[m");
    }

    #[test]
    fn escapes_around_truncation() {
        // Escapes ride along: the prefix goes out before the padded body,
        // the trailing run is re-appended after it.
        let colored = [named("a", "\x1b[31mred\x1b[m")];
        assert_eq!(render("{a:7}", &colored), "\x1b[31mred\x1b[m\x1b[m");
        assert_eq!(render("{a:3}", &colored), "\x1b[31mred\x1b[m\x1b[m");
        assert_eq!(render("[{a<5}]", &colored), "[\x1b[31mred  \x1b[m]\x1b[m");
        assert_eq!(render("{a~0,4}", &colored), "\x1b[31mred\x1b[m\x1b[m");
        // Two stacked prefixes strip together.
        let stacked = [named("a", "\x1b[31m\x1b[1mred")];
        assert_eq!(render("[{a<3}]", &stacked), "[\x1b[31m\x1b[1mred]\x1b[m");
        // A non-CSI escape stays in the body and counts toward the length;
        // the byte cut lands inside it and the bytes print verbatim.
        let osc = [named("a", "x\x1b]8;;link\x1b\\y")];
        assert_eq!(render("{a:2}", &osc), "x\x1b\x1b[m");
    }
}
