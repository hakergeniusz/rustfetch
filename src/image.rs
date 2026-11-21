//! The image logo printers that need no pixel data — iTerm2's file
//! escape, kitty's graphics escape, and the `kitten icat` subprocess —
//! ported from logo/image/image.c. sixel, plain kitty and chafa encode
//! pixels, which needs ImageMagick/chafa; the reference build has
//! neither, so they report the exact upstream "not compiled in" errors.
//! Every printer flushes the pending ffStart reset ahead of its raw
//! write, like upstream's fflush(stdout).

use crate::instance::Instance;
use crate::logo::{Logo, LogoOptions, Position, PositionOpt};

pub(crate) enum ImageKind {
    Sixel,
    Kitty,
    KittyDirect,
    KittyIcat,
    Iterm,
    Chafa,
}

/// The CPR-query behavior the renderer must replicate after the dump:
/// who names the failure message, `gated` mirrors the per-printer
/// printError check (iterm's is missing upstream), and `trailing` is
/// written after a successful query.
pub(crate) struct ImageCpr {
    pub who: &'static str,
    pub gated: bool,
    pub trailing: &'static str,
}

/// ffProcessAppendStdOut for `kitten icat`: stdout captured, stderr to
/// the null file, stdin inherited, right-trimmed. Only exit 127 and
/// spawn failures are errors, like ffProcessReadOutput.
fn process_append_stdout(argv: &[&str]) -> Result<String, &'static str> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(argv[0]);
    for arg in &argv[1..] {
        command.arg(arg);
    }
    command
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Err(if error.kind() == std::io::ErrorKind::NotFound {
                "command not found"
            } else {
                "posix_spawnp() failed"
            });
        },
    };
    // ffProcessReadOutput handles only exit 127; anything else passes.
    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(_) => return Err("read(childPipeFd, str, FF_PIPE_BUFSIZ) failed"),
    };
    if !output.status.success() && output.status.code() == Some(127) {
        return Err("command not found");
    }
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(text.trim_end_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']).to_string())
}

/// ffBase64EncodeRaw: standard base64 with padding.
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut chunks = data.chunks_exact(3);
    for chunk in &mut chunks {
        let n = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | chunk[2] as u32;
        for shift in [18u32, 12, 6, 0] {
            out.push(CHARS[((n >> shift) & 63) as usize] as char);
        }
    }
    match chunks.remainder() {
        [a] => {
            let n = (*a as u32) << 16;
            out.push(CHARS[((n >> 18) & 63) as usize] as char);
            out.push(CHARS[((n >> 12) & 63) as usize] as char);
            out.push_str("==");
        },
        [a, b] => {
            let n = ((*a as u32) << 16) | ((*b as u32) << 8);
            out.push(CHARS[((n >> 18) & 63) as usize] as char);
            out.push(CHARS[((n >> 12) & 63) as usize] as char);
            out.push(CHARS[((n >> 6) & 63) as usize] as char);
            out.push('=');
        },
        _ => {},
    }
    out
}

/// ffLogoPrintImageIfExists: the fast-fail ladder, then the printer for
/// the type. None means the load failed and the caller falls back to the
/// detected logo.
pub(crate) fn print_image(
    kind: ImageKind,
    resolved: &str,
    configured: PositionOpt,
    opts: &LogoOptions,
    instance: &Instance,
) -> Option<Logo> {
    let print_error = instance.show_errors;
    if instance.pipe {
        if print_error {
            eprintln!("Logo: Image logo is not supported in pipe mode");
        }
        return None;
    }
    if !std::path::Path::new(resolved).is_file() {
        if print_error {
            eprintln!("Logo: Image source \"{resolved}\" does not exist");
        }
        return None;
    }
    if std::env::var("TERM").as_deref() == Ok("screen") {
        if print_error {
            eprintln!("Logo: Image logo is not supported in terminal multiplexers");
        }
        return None;
    }

    // resolveImageAutoPosition: AUTO resolves against the image's width
    // and sticks, and the printers read the resolved position.
    let columns = crate::modules::terminalsize::logo_terminal_columns();
    let width = opts.width + opts.padding_left + opts.padding_right;
    let position = crate::logo::resolve_position(configured, columns, width);

    match kind {
        ImageKind::Iterm => print_iterm(resolved, opts, position, instance),
        ImageKind::KittyDirect => print_kitty_direct(resolved, opts, position, instance),
        ImageKind::KittyIcat => print_kitty_icat(resolved, opts, position, instance),
        ImageKind::Chafa => {
            if print_error {
                eprintln!("Logo: Chafa support is not compiled in");
            }
            None
        },
        ImageKind::Sixel | ImageKind::Kitty => {
            if print_error {
                eprintln!("Logo: Image Magick support is not compiled in");
            }
            None
        },
    }
}

fn in_tmux() -> bool {
    std::env::var("TERM")
        .ok()
        .is_some_and(|term| term.starts_with("screen") || term.starts_with("tmux"))
}

fn read_source(source: &str) -> Option<Vec<u8>> {
    std::fs::read(source).ok()
}

fn print_iterm(
    source: &str,
    opts: &LogoOptions,
    position: Position,
    instance: &Instance,
) -> Option<Logo> {
    let Some(content) = read_source(source) else {
        if instance.show_errors {
            eprintln!("Logo (iterm): Failed to load image file");
        }
        return None;
    };
    let base64 = base64_encode(&content);
    let tmux = in_tmux();

    if opts.width == 0 || opts.height == 0 {
        if position == Position::Right && opts.width == 0 {
            if instance.show_errors {
                eprintln!("Logo (iterm): Must set logo width when using position right");
            }
            return None;
        }
        // The payload only: the renderer puts the position's pre-dump and
        // the ffStart reset ahead of it, and runs the cursor query after.
        let mut content = Vec::new();
        if tmux {
            content.extend_from_slice(b"\x1bPtmux;\x1b");
        }
        if opts.width > 0 {
            content.extend_from_slice(format!("\x1b]1337;File=inline=1;width={}:{}\x07", opts.width, base64).as_bytes());
        } else {
            content.extend_from_slice(format!("\x1b]1337;File=inline=1:{}\x07", base64).as_bytes());
        }
        if tmux {
            content.extend_from_slice(b"\x1b\\");
        }
        Some(Logo::Raw {
            content,
            position,
            static_layout: None,
            cpr: Some(ImageCpr { who: "(iterm)", gated: false, trailing: " \x1b[H" }),
            flush_start: true,
        })
    } else {
        let mut prefix = Vec::new();
        prefix.extend(std::iter::repeat_n(b'\n', opts.padding_top as usize));
        match position {
            Position::Right => {
                prefix.extend_from_slice(format!("\x1b[9999999C\x1b[{}D", opts.padding_right + opts.width).as_bytes());
            },
            Position::Left if opts.padding_left > 0 => {
                prefix.extend_from_slice(format!("\x1b[{}C", opts.padding_left).as_bytes());
            },
            _ => {},
        }
        let mut payload = Vec::new();
        if tmux {
            payload.extend_from_slice(b"\x1bPtmux;\x1b");
        }
        payload.extend_from_slice(
            format!(
                "\x1b]1337;File=inline=1;width={};height={};preserveAspectRatio={}:{}\x07",
                opts.width, opts.height, u32::from(opts.preserve_aspect_ratio), base64
            )
            .as_bytes(),
        );
        if tmux {
            payload.extend_from_slice(b"\x1b\\");
        }
        let mut suffix = vec![b'\n'];
        match position {
            Position::Left => {
                suffix.extend_from_slice(format!("\x1b[{}A", opts.padding_top + opts.height).as_bytes());
            },
            Position::Top => {
                suffix.extend(std::iter::repeat_n(b'\n', opts.padding_right as usize));
            },
            Position::Right => {
                suffix.extend_from_slice(format!("\x1b[1G\x1b[{}A", opts.height).as_bytes());
            },
        }
        let (logo_width, logo_height) = match position {
            Position::Left => ((opts.width + opts.padding_left + opts.padding_right) as usize, (opts.padding_top + opts.height) as usize),
            _ => (0, 0),
        };
        Some(Logo::Raw {
            content: payload,
            position,
            static_layout: Some((prefix, suffix, logo_width, logo_height)),
            cpr: None,
            flush_start: true,
        })
    }
}

fn print_kitty_direct(
    source: &str,
    opts: &LogoOptions,
    position: Position,
    instance: &Instance,
) -> Option<Logo> {
    if !std::path::Path::new(source).is_file() {
        if instance.show_errors {
            eprintln!("Logo (kitty-direct): Failed to load image file");
        }
        return None;
    }
    // The source path itself is the payload, base64 encoded.
    let base64 = base64_encode(source.as_bytes());
    let tmux = in_tmux();

    if opts.width == 0 || opts.height == 0 {
        if position == Position::Right && opts.width == 0 {
            if instance.show_errors {
                eprintln!("Logo (iterm): Must set logo width when using position right");
            }
            return None;
        }
        let mut content = Vec::new();
        if tmux {
            content.extend_from_slice(b"\x1bPtmux;\x1b");
        }
        if opts.width > 0 {
            content.extend_from_slice(format!("\x1b_Ga=T,f=100,t=f,c={};{}", opts.width, base64).as_bytes());
        } else {
            content.extend_from_slice(format!("\x1b_Ga=T,f=100,t=f;{}", base64).as_bytes());
        }
        if tmux {
            content.push(b'\x1b');
        }
        content.extend_from_slice(b"\x1b\\");
        if tmux {
            content.extend_from_slice(b"\x1b\\");
        }
        Some(Logo::Raw {
            content,
            position,
            static_layout: None,
            cpr: Some(ImageCpr { who: "(kitty-direct)", gated: true, trailing: "\x1b[H" }),
            flush_start: true,
        })
    } else {
        let mut prefix = Vec::new();
        prefix.extend(std::iter::repeat_n(b'\n', opts.padding_top as usize));
        match position {
            Position::Right => {
                prefix.extend_from_slice(format!("\x1b[9999999C\x1b[{}D", opts.padding_right + opts.width).as_bytes());
            },
            Position::Left if opts.padding_left > 0 => {
                prefix.extend_from_slice(format!("\x1b[{}C", opts.padding_left).as_bytes());
            },
            _ => {},
        }
        let mut payload = Vec::new();
        if tmux {
            payload.extend_from_slice(b"\x1bPtmux;\x1b");
        }
        payload.extend_from_slice(format!("\x1b_Ga=T,f=100,t=f,c={},r={};{}\x1b\\", opts.width, opts.height, base64).as_bytes());
        if tmux {
            payload.extend_from_slice(b"\x1b\\");
        }
        let mut suffix = vec![b'\n'];
        match position {
            Position::Left => {
                suffix.extend_from_slice(format!("\x1b[{}A", opts.padding_top + opts.height).as_bytes());
            },
            Position::Top => {
                suffix.extend(std::iter::repeat_n(b'\n', opts.padding_right as usize));
            },
            Position::Right => {
                suffix.extend_from_slice(format!("\x1b[1G\x1b[{}A", opts.height).as_bytes());
            },
        }
        let (logo_width, logo_height) = match position {
            Position::Left => ((opts.width + opts.padding_left + opts.padding_right) as usize, (opts.padding_top + opts.height) as usize),
            _ => (0, 0),
        };
        Some(Logo::Raw {
            content: payload,
            position,
            static_layout: Some((prefix, suffix, logo_width, logo_height)),
            cpr: None,
            flush_start: true,
        })
    }
}

/// `kitten icat` positioning-sequence scanner: `\e[<n>C` (relative) or
/// `\e[<row>;<col>H` (absolute). Returns Some((end, first, second, kind))
/// with kind 1 for C and 2 for H, None otherwise.
fn parse_icat_csi(data: &[u8], pos: usize) -> Option<(usize, u32, u32, i32)> {
    if pos + 2 >= data.len() || data[pos] != b'\x1b' || data[pos + 1] != b'[' {
        return None;
    }
    let mut values = [0u32, 0];
    let mut count = 0usize;
    let mut i = pos + 2;
    while i < data.len() {
        let c = data[i];
        if c.is_ascii_digit() {
            if count == 0 {
                count = 1;
            }
            values[count - 1] = values[count - 1].wrapping_mul(10).wrapping_add((c - b'0') as u32);
            i += 1;
        } else if c == b';' && count == 1 {
            count = 2;
            i += 1;
        } else {
            break;
        }
    }
    if i >= data.len() {
        return None;
    }
    if data[i] == b'C' && count == 1 {
        return Some((i + 1, values[0], 0, 1));
    }
    if data[i] == b'H' && count == 2 {
        return Some((i + 1, values[0], values[1], 2));
    }
    None
}

/// appendIcatOutput: drop the carriage return and the one positioning
/// sequence `kitten` writes ahead of its graphics escape, re-applying the
/// horizontal centering it applied inside the requested rectangle.
fn append_icat_output(buf: &mut Vec<u8>, output: &str, opts: &LogoOptions) {
    let data = output.as_bytes();
    let mut pos = 0usize; // 0 = layout not recognised
    let mut offset = 0u32;

    if !data.is_empty() && data[0] == b'\r' {
        let mut cursor = 1usize;
        if let Some((end, first, second, kind)) = parse_icat_csi(data, cursor) {
            if kind == 1 {
                offset = first;
                cursor = end;
            } else if kind == 2 && second >= opts.padding_left + 1 {
                offset = second - opts.padding_left - 1;
                cursor = end;
            }
        }
        if cursor + 1 < data.len() && data[cursor] == b'\x1b' && data[cursor + 1] == b'_' {
            pos = cursor;
        }
    }

    if pos == 0 {
        // Unrecognised layout: keep it as is, positioning included.
        buf.extend_from_slice(data);
        return;
    }
    if offset > 0 {
        buf.extend_from_slice(format!("\x1b[{}C", offset).as_bytes());
    }
    buf.extend_from_slice(&data[pos..]);
}

fn print_kitty_icat(
    source: &str,
    opts: &LogoOptions,
    position: Position,
    instance: &Instance,
) -> Option<Logo> {
    if !std::path::Path::new(source).is_file() {
        if instance.show_errors {
            eprintln!("Logo (kitty-icat): Failed to load image file");
        }
        return None;
    }

    match position {
        Position::Top => {
            if opts.width > 0 {
                if instance.show_errors {
                    eprintln!("Logo (kitty-icat): position top is not supported when logo width is set");
                }
                return None;
            }
        },
        Position::Right => {
            if instance.show_errors {
                eprintln!("Logo (kitty-icat): position right is not supported");
            }
            return None;
        },
        Position::Left => {},
    }

    let icat_output = if opts.width > 0 {
        let place = format!(
            "--place={}x{}@{}x{}",
            opts.width,
            if opts.height == 0 { 9999 } else { opts.height },
            opts.padding_left,
            opts.padding_top
        );
        process_append_stdout(&["kitten", "icat", "-n", "--stdin=no", "--align=center", &place, "--scale-up", source])
    } else {
        process_append_stdout(&["kitten", "icat", "-n", "--stdin=no", "--align=left", source])
    };
    let icat_output = match icat_output {
        Ok(output) => output,
        Err(error) => {
            if instance.show_errors {
                eprintln!("Logo (kitty-icat): running `kitten icat` failed {error}");
            }
            return None;
        },
    };
    if icat_output.is_empty() {
        if instance.show_errors {
            eprintln!("Logo (kitty-icat): `kitten icat` returned empty output");
        }
        return None;
    }

    let mut content = Vec::new();
    append_icat_output(&mut content, &icat_output, opts);

    Some(Logo::Raw {
        content,
        position,
        static_layout: None,
        cpr: Some(ImageCpr { who: "(kitty-icat)", gated: true, trailing: "\x1b[H" }),
        flush_start: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
        // Bytes with the high bit set go through unsigned widening.
        assert_eq!(base64_encode(&[0xFF, 0xEF, 0x01]), "/+8B");
    }

    #[test]
    fn icat_csi_parses_both_forms() {
        let data = b"\r\x1b[5C\x1b_Ga=T,f=100,t=f;AAAA\x1b\\";
        let (end, first, second, kind) = parse_icat_csi(data, 1).unwrap();
        assert_eq!((end, first, second, kind), (5, 5, 0, 1));
        let data = b"\r\x1b[3;11H\x1b_Ga=T,f=100,t=f;AAAA\x1b\\";
        let (end, first, second, kind) = parse_icat_csi(data, 1).unwrap();
        assert_eq!((end, first, second, kind), (8, 3, 11, 2));
        // No digits before the terminator, or a non-CSI payload: none.
        assert!(parse_icat_csi(b"\r\x1b[Cx", 1).is_none());
        assert!(parse_icat_csi(b"\rx_no_escape", 1).is_none());
    }

    #[test]
    fn icat_output_drops_positioning_and_keeps_centering() {
        let mut opts = LogoOptions::default();
        opts.padding_left = 3;
        let mut buf = Vec::new();
        append_icat_output(&mut buf, "\r\x1b[7C\x1b_Ga=T,f=100,t=f;AAAA\x1b\\", &opts);
        assert_eq!(String::from_utf8(buf).unwrap(), "\x1b[7C\x1b_Ga=T,f=100,t=f;AAAA\x1b\\");
        // The absolute --place move keeps only the centering offset.
        let mut buf = Vec::new();
        append_icat_output(&mut buf, "\r\x1b[1;9H\x1b_Ga=T,f=100,t=f;AAAA\x1b\\", &opts);
        assert_eq!(String::from_utf8(buf).unwrap(), "\x1b[5C\x1b_Ga=T,f=100,t=f;AAAA\x1b\\");
        // An unrecognised layout passes through untouched.
        let mut buf = Vec::new();
        append_icat_output(&mut buf, "\x1b_Ga=T,f=100,t=f;AAAA\x1b\\", &opts);
        assert_eq!(String::from_utf8(buf).unwrap(), "\x1b_Ga=T,f=100,t=f;AAAA\x1b\\");
    }
}
