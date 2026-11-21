//! `player` module: the active media player's name, ported from player.c.
//! An alias of `media` — same MPRIS detection, but the value is just the
//! player ("Chrome"), prefixed with the website when the track's URL points
//! at one ("Youtube (Chrome)").

use crate::{
    instance::Instance,
    json::JsonOutcome,
    modules::{Row, media},
};

pub fn run(instance: &Instance) -> Vec<Row> {
    let Ok(media) = media::detect(&instance.player_name) else {
        // The shared media detection errors ("No media found", DBus
        // failures) are error rows, hidden by default.
        return Vec::new();
    };

    // Website players show the site name, except the ones that are surely
    // not websites even with a URL set.
    let mut pretty = String::new();
    if !media.url.is_empty()
        && !media.player_id.eq_ignore_ascii_case("spotify")
        && !media.player_id.eq_ignore_ascii_case("vlc")
    {
        for prefix in ["https://www.", "http://www.", "https://", "http://"] {
            if let Some(rest) = media.url.strip_prefix(prefix) {
                pretty = rest.to_string();
                break;
            }
        }
    }

    // Site name only: drop the path, then the TLD.
    if !pretty.is_empty() {
        pretty = pretty.split('/').next().unwrap_or("").to_string();
        if let Some(dot) = pretty.rfind('.') {
            pretty.truncate(dot);
        }
    }

    let is_custom = !pretty.is_empty();

    // Single-component names get capitalized.
    if is_custom && !pretty.contains('.') {
        let mut chars = pretty.chars();
        if let Some(first) = chars.next() {
            pretty = first.to_uppercase().collect::<String>() + chars.as_str();
        }
    }

    if is_custom {
        pretty.push_str(" (");
    }
    pretty.push_str(&media.player);
    if is_custom {
        pretty.push(')');
    }

    vec![Row::new(instance.display_name("player"), pretty)]
}



/// ffGeneratePlayerJsonResult: the module is a Media alias.
pub fn json(_instance: &Instance) -> JsonOutcome {
    JsonOutcome::Error("Player module is an alias of Media module".to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn prettifies_website_players() {
        let cases: [(&str, &str); 4] = [
            ("https://www.youtube.com/watch?v=x", "Youtube"),
            ("http://example.co.uk/a/b", "example.co"),
            ("https://open.spotify.com/track/1", "open.spotify"),
            ("file:///home/user/song.mp3", ""),
        ];
        for (url, expected) in cases {
            let mut pretty = String::new();
            for prefix in ["https://www.", "http://www.", "https://", "http://"] {
                if let Some(rest) = url.strip_prefix(prefix) {
                    pretty = rest.to_string();
                    break;
                }
            }
            if !pretty.is_empty() {
                pretty = pretty.split('/').next().unwrap_or("").to_string();
                if let Some(dot) = pretty.rfind('.') {
                    pretty.truncate(dot);
                }
            }

            let is_custom = !pretty.is_empty();
            if is_custom && !pretty.contains('.') {
                let mut chars = pretty.chars();
                if let Some(first) = chars.next() {
                    pretty = first.to_uppercase().collect::<String>() + chars.as_str();
                }
            }
            assert_eq!(pretty, expected, "url {url}");
        }
    }
}
