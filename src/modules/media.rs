//! `media` module: MPRIS now-playing info from the session bus, ported from
//! fastfetch's media_linux.c and modules/media/media.c. The preferred player
//! busses (spotify, vlc, plasma-browser-integration) are tried first, then
//! every other MPRIS name on the bus; the row is printed even when empty.

use std::fmt::Write;

use crate::{
    dbus,
    instance::Instance,
    json::{Json, JsonOutcome},
    modules::Row,
    util,
};

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";

/// One player's flattened MPRIS.Player properties.
#[derive(Default)]
pub(crate) struct Media {
    pub(crate) song: String,
    pub(crate) artist: String,
    pub(crate) album: String,
    pub(crate) url: String,
    pub(crate) status: String,
    pub(crate) cover: String,
    pub(crate) length_ms: u32,
    pub(crate) position_ms: u32,
    pub(crate) player_id: String,
    pub(crate) player: String,
}

/// The upstream early-break condition: every xesam field has a value.
fn xesam_complete(media: &Media) -> bool {
    !media.song.is_empty()
        && !media.artist.is_empty()
        && !media.album.is_empty()
        && !media.url.is_empty()
}

/// The mpris:artUrl cover: only `file:///` URLs count, and the %XX escapes
/// after `file://` are decoded byte-wise; an invalid escape keeps its `%`,
/// a truncated one ends the decode.
fn decode_cover(url: &str) -> String {
    if !url.starts_with("file:///") {
        return String::new();
    }
    let bytes = url.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    // strlen("file://"): decoding starts at the path's leading '/'.
    let mut i = 7;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                break;
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            match u8::from_str_radix(hex, 16) {
                // strtoul consumed both digits
                Ok(byte) => {
                    i += 2;
                    out.push(byte);
                },
                _ => out.push(b'%'),
            }
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    // Upstream appends the decoded bytes verbatim; lossy only replaces
    // sequences that would make the JSON itself invalid UTF-8.
    String::from_utf8_lossy(&out).into_owned()
}

/// parseMprisMetadata: the rendered a{sv} body of the Metadata property.
/// Upstream stops as soon as all four xesam fields are set, skipping any
/// mpris: entries listed after them.
fn parse_metadata(media: &mut Media, value: &str) {
    let Some(inner) = value.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        return;
    };
    for item in util::split_top_level(inner) {
        let Some((key, entry)) = util::split_pair(&item) else {
            continue;
        };
        // Metadata values are variants: <'...'>, <['a', 'b']>, <123>.
        let value = entry.strip_prefix('<').and_then(|s| s.strip_suffix('>')).unwrap_or(entry);
        match util::unquote(key).as_str() {
            "xesam:title" => {
                media.song = util::string_value(value);
                if xesam_complete(media) {
                    break;
                }
            },
            "xesam:album" => {
                media.album = util::string_value(value);
                if xesam_complete(media) {
                    break;
                }
            },
            "xesam:artist" => {
                media.artist = util::string_value(value);
                if xesam_complete(media) {
                    break;
                }
            },
            "xesam:url" => {
                media.url = util::string_value(value);
                if xesam_complete(media) {
                    break;
                }
            },
            "mpris:artUrl" => {
                let url = util::string_value(value);
                media.cover = decode_cover(&url);
            },
            "mpris:length" => {
                if let Ok(length) = value.parse::<i64>()
                    && length > 0
                {
                    media.length_ms = (length / 1000) as u32;
                }
            },
            _ => {},
        }
    }
}

/// Song fallback: the URL path's basename, %-decoded, up to the query part.
fn url_basename(url: &str) -> String {
    let base = url.rsplit('/').next().unwrap_or(url);
    let mut out = String::new();
    let mut chars = base.chars();
    while let Some(c) = chars.next() {
        if c == '?' {
            break;
        }
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                // strtoul without validation: bad hex decodes to '\0'
                let byte = u8::from_str_radix(&hex, 16).unwrap_or(0);
                out.push(byte as char);
            } else {
                break;
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// getBusProperties: one player bus; None when it has no playable media.
fn get_bus_properties(bus_name: &str) -> Option<Media> {
    let props = dbus::get_all_properties(
        bus_name,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )?;

    let mut media = Media::default();
    for (key, value) in &props {
        match key.as_str() {
            "'Metadata'" => parse_metadata(&mut media, value),
            "'PlaybackStatus'" => media.status = util::unquote(value),
            "'Position'" => {
                if let Ok(position) = value.parse::<i64>()
                    && position > 0
                {
                    media.position_ms = (position / 1000) as u32;
                }
            },
            _ => {},
        }
    }

    if media.song.is_empty() {
        if media.url.is_empty() {
            return None;
        }
        media.song = url_basename(&media.url);
    }

    media.player_id = bus_name.strip_prefix(MPRIS_PREFIX)?.to_string();

    // We found a song, get the player name. dbus calls are extremely slow on
    // musikcube, so its name is set manually.
    media.player = if media.player_id.starts_with("musikcube.instance") {
        "musikcube".to_string()
    } else {
        dbus::get_property_string(
            bus_name,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2",
            "Identity",
        )
        .or_else(|| {
            dbus::get_property_string(
                bus_name,
                "/org/mpris/MediaPlayer2",
                "org.mpris.MediaPlayer2",
                "DesktopEntry",
            )
        })
        .unwrap_or_else(|| media.player_id.clone())
    };

    Some(media)
}

/// getBestBus: the common player busses first, then every other MPRIS name
/// ListNames reports (playerctld excluded).
fn get_best_bus() -> Option<Media> {
    for short in ["spotify", "vlc", "plasma-browser-integration"] {
        if let Some(media) = get_bus_properties(&format!("{MPRIS_PREFIX}{short}")) {
            return Some(media);
        }
    }

    let reply = dbus::call_method_printed(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "ListNames",
    )?;
    // gdbus-style arg-list parens around the string array.
    let inner = reply
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))?
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))?;
    for name in util::split_top_level(inner) {
        let name = util::unquote(&name);
        if let Some(id) = name.strip_prefix(MPRIS_PREFIX)
            && id != "playerctld"
            && let Some(media) = get_bus_properties(&name)
        {
            return Some(media);
        }
    }
    None
}

/// ffDetectMedia: DBus load/connect failures map to upstream's error
/// strings; a run without a matching bus — or with no song — is upstream's
/// "No media found" error. On success the text fields are space-trimmed.
pub(crate) fn detect(player_name: &str) -> Result<Media, &'static str> {
    dbus::load(dbus::DBUS_BUS_SESSION)?;
    // general.playerName: one explicit bus, prefixed with the MPRIS name
    // unless it already carries it (getCustomBus).
    let Some(mut media) = (if player_name.is_empty() {
        get_best_bus()
    } else if let Some(rest) = player_name.strip_prefix(MPRIS_PREFIX) {
        let _ = rest;
        get_bus_properties(player_name)
    } else {
        get_bus_properties(&format!("{MPRIS_PREFIX}{player_name}"))
    }) else {
        return Err("No media found");
    };
    if media.song.is_empty() {
        return Err("No media found");
    }
    trim_right_spaces(&mut media.song);
    trim_right_spaces(&mut media.artist);
    trim_right_spaces(&mut media.album);
    trim_right_spaces(&mut media.player);
    Ok(media)
}

/// The YouTube-style suffixes ffStrbufRemoveStrings strips from song titles;
/// every occurrence of each is removed.
const REMOVE_STRINGS: &[&str] = &[
    "(Official Music Video)",
    "(Official Video)",
    "(Music Video)",
    "(Official HD Video)",
    "[Official Music Video]",
    "[Official Video]",
    "[Music Video]",
    "[Official HD Video]",
    "| Official Music Video",
    "| Official Video",
    "| Music Video",
    "| Official HD Video",
    "[Official Audio]",
    "[Audio]",
    "(Audio)",
    "| Official Audio",
    "| Audio",
    "| OFFICIAL AUDIO",
    "(Lyric Video)",
    "(Official Lyric Video)",
    "(Lyrics)",
    "[Lyric Video]",
    "[Official Lyric Video]",
    "[Lyrics]",
    "| Lyric Video",
    "| Official Lyric Video",
    "| Lyrics",
];

fn remove_all(haystack: &mut String, needle: &str) {
    while let Some(pos) = haystack.find(needle) {
        haystack.replace_range(pos..pos + needle.len(), "");
    }
}

fn trim_right_spaces(text: &mut String) {
    while text.ends_with(' ') {
        text.pop();
    }
}

fn remove_ign_case_end(text: &mut String, end: &str) {
    if let Some(cut) = text.len().checked_sub(end.len())
        && text.is_char_boundary(cut)
        && text[cut..].eq_ignore_ascii_case(end)
    {
        text.truncate(cut);
    }
}

fn should_ignore_char(c: char) -> bool {
    c == ' ' || c == '\t' || c == '-' || c == '.'
}

/// artistInSongTitle: whether the artist name appears at the song title's
/// start, ignoring blanks, '-' and '.' and case.
fn artist_in_song_title(song: &str, artist: &str) -> bool {
    let mut song = song.chars();
    let mut artist = artist.chars();
    let mut song_char = song.next();
    let mut artist_char = artist.next();
    loop {
        while song_char.is_some_and(should_ignore_char) {
            song_char = song.next();
        }
        while artist_char.is_some_and(should_ignore_char) {
            artist_char = artist.next();
        }
        let Some(a) = artist_char else {
            return true;
        };
        let Some(s) = song_char else {
            return false;
        };
        // C compares byte-wise with tolower, which leaves non-ASCII alone.
        if !s.eq_ignore_ascii_case(&a) {
            return false;
        }
        song_char = song.next();
        artist_char = artist.next();
    }
}

/// appendProgress: `m:ss / m:ss`, hours prepended past the one-hour mark.
fn append_progress(out: &mut String, media: &Media) {
    let s_len = media.length_ms / 1000;
    let s_pos = media.position_ms / 1000;
    if s_len > 60 * 60 {
        let _ = write!(
            out,
            "{:02}:{:02}:{:02} / {:02}:{:02}:{:02}",
            s_pos / 3600,
            (s_pos % 3600) / 60,
            s_pos % 60,
            s_len / 3600,
            (s_len % 3600) / 60,
            s_len % 60
        );
    } else {
        let _ = write!(
            out,
            "{:02}:{:02} / {:02}:{:02}",
            s_pos / 60,
            s_pos % 60,
            s_len / 60,
            s_len % 60
        );
    }
}

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(media) = detect(&instance.player_name) else {
        // Both upstream errors ("Failed to load/connect to DBus", "No media
        // found") are error rows, hidden by default.
        return Vec::new();
    };

    let mut song_pretty = media.song.clone();
    for remove in REMOVE_STRINGS {
        remove_all(&mut song_pretty, remove);
    }
    trim_right_spaces(&mut song_pretty);
    if song_pretty.is_empty() {
        song_pretty = media.song.clone();
    }

    let mut artist_pretty = media.artist.clone();
    remove_ign_case_end(&mut artist_pretty, " - Topic");
    remove_ign_case_end(&mut artist_pretty, "VEVO");
    trim_right_spaces(&mut artist_pretty);
    if artist_in_song_title(&song_pretty, &artist_pretty) {
        artist_pretty.clear();
    }

    let mut value = String::new();
    if !artist_pretty.is_empty() {
        let _ = write!(value, "{artist_pretty} - ");
    }
    value.push_str(&song_pretty);

    if media.length_ms > 0 {
        value.push_str(" - ");
        append_progress(&mut value, &media);

        // The media module hardcodes green=100/yellow=100 in its percent
        // config: the number is green, red only once the position runs past
        // the length (compared before rounding, like ffPercentAppendNum).
        let pct = media.position_ms as f64 * 100.0 / media.length_ms as f64;
        let pct_text = format!("{pct:.0}");
        if instance.color {
            let code = if pct > 100.0 { "91" } else { "32" };
            let _ = write!(value, " (\x1b[{code}m{pct_text}%\x1b[m)");
        } else {
            let _ = write!(value, " ({pct_text}%)");
        }
    }

    if !media.status.is_empty() {
        let _ = write!(value, " [{}]", media.status);
    }

    vec![Row::new("Media", value)]
}

/// ffGenerateMediaJsonResult.
pub fn json(instance: &Instance) -> JsonOutcome {
    let media = match detect(&instance.player_name) {
        Ok(media) => media,
        Err(error) => return JsonOutcome::Error(error.to_string()),
    };

    let mut result = Json::obj();
    let mut song = Json::obj();
    song.push("name", media.song);
    song.push("artist", media.artist);
    song.push("album", media.album);
    song.push("status", media.status);
    song.push("length", media.length_ms as u64);
    song.push("position", media.position_ms as u64);
    song.push(
        "cover",
        if media.cover.is_empty() {
            Json::Null
        } else {
            media.cover.into()
        },
    );
    result.push("song", song);

    let mut player = Json::obj();
    player.push("name", media.player);
    player.push("id", media.player_id);
    player.push("url", media.url);
    result.push("player", player);

    JsonOutcome::Result(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_rendered_containers() {
        assert_eq!(util::split_top_level("'a', 'b, c', 'd'"), ["'a'", "'b, c'", "'d'"]);
        assert_eq!(
            util::split_top_level("'k': <'v'>, 'k2': <['x', 'y']>"),
            ["'k': <'v'>", "'k2': <['x', 'y']>"]
        );
        assert_eq!(util::split_pair("'xesam:title': <'T'>"), Some(("'xesam:title'", "<'T'>")));
        assert_eq!(util::split_pair("no pair here"), None);
    }

    #[test]
    fn unquotes_escapes() {
        assert_eq!(util::unquote("'it\\'s'"), "it's");
        assert_eq!(util::unquote("'back\\\\slash'"), "back\\slash");
        assert_eq!(util::unquote("'plain'"), "plain");
        assert_eq!(util::unquote("123"), "123");
    }

    #[test]
    fn parses_metadata_dict() {
        let mut media = Media::default();
        parse_metadata(
            &mut media,
            "{ 'mpris:length': <237000>, 'xesam:title': <'Song Name'>, \
             'xesam:artist': <['One', 'Two']>, 'xesam:album': <'Album'>, \
             'xesam:url': <'file:///a%20b.mp3'> }",
        );
        assert_eq!(media.length_ms, 237);
        assert_eq!(media.song, "Song Name");
        assert_eq!(media.artist, "One, Two");
        assert_eq!(media.album, "Album");
        assert_eq!(media.url, "file:///a%20b.mp3");
    }

    #[test]
    fn decodes_arturl_covers() {
        assert_eq!(decode_cover("file:///home/u/a%20b.jpg"), "/home/u/a b.jpg");
        assert_eq!(decode_cover("https://x/y.jpg"), "");
        assert_eq!(decode_cover("file:///a%zz.jpg"), "/a%zz.jpg");
        assert_eq!(decode_cover("file:///a%2"), "/a"); // truncated escape ends the decode
        assert_eq!(decode_cover("file:///"), "/");
    }

    #[test]
    fn stops_after_all_xesam_fields() {
        let mut media = Media::default();
        parse_metadata(
            &mut media,
            "{ 'mpris:length': <237000>, 'xesam:title': <'T'>, 'xesam:artist': <['A']>, \
             'xesam:album': <'L'>, 'xesam:url': <'u'>, 'mpris:artUrl': <'file:///c.jpg'> }",
        );
        // All four xesam fields were set at the url entry, so the artUrl
        // after it is skipped — length, listed before, survives.
        assert_eq!(media.length_ms, 237);
        assert_eq!(media.song, "T");
        assert_eq!(media.cover, "");
    }

    #[test]
    fn decodes_url_basenames() {
        assert_eq!(url_basename("file:///a%20b.mp3"), "a b.mp3");
        assert_eq!(url_basename("https://x/y/watch?v=1&t=2"), "watch");
        assert_eq!(url_basename("no-slash"), "no-slash");
        assert_eq!(url_basename("ends%2"), "ends");
    }

    #[test]
    fn cleans_titles_and_artists() {
        let mut song = "Song (Official Music Video)".to_string();
        for remove in REMOVE_STRINGS {
            remove_all(&mut song, remove);
        }
        trim_right_spaces(&mut song);
        assert_eq!(song, "Song");

        let mut artist = "Channel - Topic".to_string();
        remove_ign_case_end(&mut artist, " - Topic");
        assert_eq!(artist, "Channel");

        let mut artist = "ArtistVEVO".to_string();
        remove_ign_case_end(&mut artist, "VEVO");
        assert_eq!(artist, "Artist");
    }

    #[test]
    fn artist_prefix_matches_ignore_blanks() {
        assert!(artist_in_song_title("Artist - Song", "Artist"));
        assert!(artist_in_song_title("the Artist - Song", "The artist"));
        assert!(!artist_in_song_title("Other - Song", "Artist"));
        assert!(artist_in_song_title("", ""));
    }

    #[test]
    fn formats_progress() {
        let media = |length_ms: u32, position_ms: u32| Media {
            length_ms,
            position_ms,
            ..Media::default()
        };
        let mut out = String::new();
        append_progress(&mut out, &media(37_000, 12_000));
        assert_eq!(out, "00:12 / 00:37");

        let mut out = String::new();
        append_progress(&mut out, &media(3_661_000, 1_234_000));
        assert_eq!(out, "00:20:34 / 01:01:01");
    }
}
