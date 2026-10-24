//! Eclipse Music addon — port of `fd_eclipse_parse_audio_title()`,
//! `fd_eclipse_audio_quality_score()` and `fd_eclipse_format_track()`
//! (backend.php:13568-14259), following the Kotlin twin
//! (`eclipseParseAudioTitle` / `eclipseFormatTrack`), which the device builds
//! shipped.
//!
//! The hard part is that most Telegram music files are named
//! `Artist.Title.ext` with an **empty `performer`** and a caption that is only a
//! short code, so there is often no separator to split on. Three strategies are
//! tried in order, and the search query is used as a tie-breaker:
//!
//! 1. the caption's `Artist - Title` line (skipping relay noise and `AgAD…` codes);
//! 2. the file name itself (` - `, `by`, or a known-artist prefix);
//! 3. the QUERY: `query = title + extra` ⇒ the prefix is the artist;
//!    `title = query + extra` ⇒ the query is the artist; `query == title` ⇒ split
//!    the query at a known artist name (longest 1-3 word match, else 2 words for a
//!    4+ word query).
//!
//! A final reversed-order pass detects a known artist at the END of the title
//! (`Black.or.White.Michael.Jackson` ⇒ artist `Michael Jackson`, title
//! `Black or White`).

use serde_json::Value;

/// `ECLIPSE_KNOWN_ARTISTS` — copied verbatim from the Kotlin list.
pub const KNOWN_ARTISTS: &[&str] = &[
    "wali", "st12", "tulus", "armada", "dmasiv", "d'masiv", "cokelat", "judika", "tiara andini",
    "tiara", "lyodra", "sheila on 7", "sheila", "dewa 19", "dewa", "peterpan", "noah", "rossa",
    "afgan", "raisa", "yovie & nuno", "yovie", "amuk", "adele", "rihanna", "eminem", "drake",
    "beyonce", "shakira", "sienna spiro", "billie eilish", "ariana grande", "taylor swift",
    "luke combs", "ed sheeran", "bruno mars", "olivia rodrigo", "justin bieber", "dua lipa",
    "lady gaga", "kelly clarkson", "selena gomez", "camila cabello", "doja cat", "katy perry",
    "miley cyrus", "avril lavigne", "mariah carey", "whitney houston", "britney spears",
    "michael jackson", "the weeknd", "post malone", "harry styles", "charlie puth", "shawn mendes",
    "sam smith", "sia", "pink", "coldplay", "maroon 5", "imagine dragons", "playboi carti",
    "doechii", "kendrick lamar", "travis scott", "metro boomin", "future", "21 savage", "sza",
    "frank ocean", "tyler the creator", "asap rocky", "lil baby", "gunna", "young thug",
    "nicki minaj", "cardi b", "megan thee stallion",
];

fn is_known_artist(s: &str) -> bool {
    KNOWN_ARTISTS.contains(&s.trim().to_lowercase().as_str())
}

/// `ucwords()`: first letter of every space-separated word upper-cased.
pub fn ucwords(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = String::new();
                    out.extend(first.to_uppercase());
                    out.push_str(chars.as_str());
                    out
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn rx(p: &str) -> Option<fancy_regex::Regex> {
    fancy_regex::Regex::new(p).ok()
}

fn rep(p: &str, text: &str, with: &str) -> String {
    match rx(p) {
        Some(re) => re.replace_all(text, with).into_owned(),
        None => text.to_string(),
    }
}

fn has(p: &str, text: &str) -> bool {
    rx(p).map(|re| re.is_match(text).unwrap_or(false)).unwrap_or(false)
}

fn collapse(s: &str) -> String {
    rep(r"\s+", s, " ").trim().to_string()
}

/// `(artist, title, isrc)`.
pub fn parse_audio_title(raw_title: &str, caption: &str) -> (String, String, String) {
    let mut name = rep(r"^\d+[\s.\-_]+", raw_title.trim(), "");
    let media = r"(?i)\.(mp3|m4a|flac|wav|ogg|opus|aac|mp4|mkv|mpg)$";
    // Strip trailing media extensions in a LOOP: `song.mp3.mp3` is real.
    for _ in 0..4 {
        if !has(media, &name) {
            break;
        }
        name = rep(media, &name, "");
    }
    name = name
        .replace('\u{2018}', "'")
        .replace('\u{2019}', "'")
        .replace('`', "'");
    name = rep(r"(?i)\bd\.masiv\b", &name, "D'MASIV");
    name = rep(r"(?i)\bd\s+masiv\b", &name, "D'MASIV");
    name = rep(r"\s*\[[^\]]*\]|\s*\([^\)]*\)", &name, " ");
    name = rep(
        r"(?i)\b(?:1080p|720p|480p|140|251|599|vc\s*trinity|nagaswara|youtube|official\s*(?:video|audio|lyric|lyrics)?|audio|lyrics?|lyrics\s*video|hd|hq|live|remix|cover|karaoke|acoustic|video)\b",
        &name,
        " ",
    );
    name = collapse(&name.replace('.', " ").replace('_', " "));

    let mut artist = String::new();
    let mut title = name.clone();
    let mut isrc = String::new();

    if !caption.is_empty() {
        if let Some(re) = rx(r"\[([A-Z]{2}[A-Z0-9]{3}[0-9]{7})\]") {
            if let Ok(Some(caps)) = re.captures(caption) {
                if let Some(g) = caps.get(1) {
                    isrc = g.as_str().to_string();
                }
            }
        }
        for line_raw in caption.split('\n') {
            if !artist.is_empty() {
                break;
            }
            let line = line_raw.trim();
            if has(r"(?i)^(?:forwarded\s+from|forwarded|via|from)\b", line) {
                continue;
            }
            if let Some(re) = rx("^([^\\-]+?)\\s*[\\-\u{2013}\u{2014}]\\s*([^\\[]+)") {
                if let Ok(Some(caps)) = re.captures(line) {
                    let ca = caps.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
                    let ct = caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
                    if !ca.is_empty()
                        && !ct.is_empty()
                        && !ca.starts_with('#')
                        && !ca.starts_with('@')
                        && !has(r"^(?:AgAD|AQAD)", &ca)
                    {
                        artist = ca;
                        title = ct;
                    }
                }
            }
        }
    }

    if artist.is_empty() {
        let split = rx("^([^\\-]+?)\\s*[\\-\u{2013}\u{2014}]\\s*(.+)$")
            .and_then(|re| re.captures(&name).ok().flatten())
            .map(|caps| {
                (
                    caps.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
                    caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default(),
                )
            });
        if let Some((a, t)) = split {
            artist = a;
            title = t;
        } else if let Some(caps) = rx("(?i)^(.+?)\\s+by\\s+(.+)$")
            .and_then(|re| re.captures(&name).ok().flatten())
        {
            title = caps.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
            artist = caps.get(2).map(|m| m.as_str().trim().to_string()).unwrap_or_default();
        } else {
            let words: Vec<&str> = name.split(' ').filter(|w| !w.is_empty()).collect();
            if words.len() >= 2 {
                if words.len() >= 3 {
                    let two = format!("{} {}", words[0], words[1]);
                    if is_known_artist(&two) {
                        artist = two;
                        title = words[2..].join(" ");
                    }
                }
                if artist.is_empty() && is_known_artist(words[0]) {
                    artist = words[0].to_string();
                    title = words[1..].join(" ");
                }
            }
        }
    }

    title = collapse(&rep(r"[^\p{L}\p{N}\s']+", &title, " "));
    if artist.is_empty() {
        artist = name;
    }
    (artist, title, isrc)
}

/// `fd_eclipse_audio_quality_score()` — higher is better.
pub fn quality_score(ext: &str, size_bytes: u64) -> i64 {
    let weight = match ext.to_lowercase().as_str() {
        "flac" | "wav" => 2000,
        "m4a" | "alac" => 1200,
        "aac" | "opus" => 800,
        "mp3" => 400,
        _ => 100,
    };
    let bonus = std::cmp::min(500, (size_bytes as f64 / (100.0 * 1024.0)).round() as i64);
    weight + bonus
}

fn str_at(item: &Value, key: &str) -> String {
    item.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

/// `fd_eclipse_format_track()` — one `track` object for the addon.
pub fn format_track(item: &Value, search_query: &str) -> Value {
    let caption = item.get("caption").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let (mut artist, mut title, isrc) = parse_audio_title(&str_at(item, "title"), &caption);
    let mut performer = str_at(item, "performer");

    let bot_line = |cap: &str| -> Option<String> {
        rx("@\\w+\\s+(.+)$")
            .and_then(|re| re.captures(cap).ok().flatten())
            .and_then(|caps| caps.get(1).map(|m| m.as_str().trim().to_string()))
    };

    if performer.is_empty() && !caption.is_empty() {
        if let Some(rest) = bot_line(&caption) {
            let words: Vec<&str> = rest.split_whitespace().collect();
            if words.len() >= 2 {
                let candidate = words[words.len() - 1];
                if candidate.chars().count() >= 3
                    && !has(r"(?i)^(?:m4a|mp3|flac|wav|ogg|opus)$", candidate)
                {
                    performer = candidate.to_string();
                }
            }
        }
    }
    if !performer.is_empty() {
        artist = performer.clone();
        let pattern = format!(
            "(?i)^{}[\\s\\-\u{2013}\u{2014}:]+",
            fancy_regex::escape(&performer)
        );
        title = collapse(&rep(&pattern, &title, ""));
    }
    if artist.is_empty() || artist.eq_ignore_ascii_case(&title) {
        if let Some(rest) = bot_line(&caption) {
            let words: Vec<&str> = rest.split_whitespace().collect();
            if words.len() >= 2 {
                artist = words[words.len() - 1].to_string();
            }
        }
    }

    // Query-based splitting (the three rules in the module docs).
    if !search_query.is_empty() && (artist.is_empty() || artist.eq_ignore_ascii_case(&title)) {
        let norm = |s: &str| collapse(&rep(r"[^\p{L}\p{N}\s]+", s, " ").to_lowercase());
        let norm_title = norm(&title);
        let norm_query = norm(search_query);
        if !norm_title.is_empty() && !norm_query.is_empty() {
            if norm_query.starts_with(&norm_title) {
                let extra = norm_query[norm_title.len()..].trim().to_string();
                if !extra.is_empty() {
                    artist = title.clone();
                    title = ucwords(&extra);
                }
            } else if norm_title.starts_with(&norm_query) {
                let extra = norm_title[norm_query.len()..].trim().to_string();
                if !extra.is_empty() {
                    artist = search_query.to_string();
                    title = ucwords(&extra);
                }
            } else if norm_title == norm_query {
                let qw: Vec<&str> = norm_query.split_whitespace().collect();
                let mut split_at = 0usize;
                let upper = std::cmp::min(3, qw.len().saturating_sub(1));
                for n in (1..=upper).rev() {
                    if is_known_artist(&qw[..n].join(" ")) {
                        split_at = n;
                        break;
                    }
                }
                if split_at == 0 && qw.len() >= 4 {
                    split_at = 2;
                }
                if split_at > 0 && split_at < qw.len() {
                    artist = ucwords(&qw[..split_at].join(" "));
                    title = ucwords(&qw[split_at..].join(" "));
                }
            }
        }
    }

    // Reversed order: a known artist at the END of the title.
    if artist.is_empty() || artist.eq_ignore_ascii_case(&title) {
        let tw: Vec<&str> = title.split_whitespace().collect();
        if tw.len() >= 3 {
            let upper = std::cmp::min(3, tw.len() - 1);
            for n in (1..=upper).rev() {
                let tail = tw[tw.len() - n..].join(" ");
                if is_known_artist(&tail) {
                    artist = ucwords(&tail);
                    title = ucwords(&tw[..tw.len() - n].join(" "));
                    break;
                }
            }
        }
    }

    let mut ext = str_at(item, "extension").to_lowercase();
    if ext.is_empty() || ext == "noext" {
        ext = "mp3".to_string();
    }
    let duration = item.get("duration").and_then(|v| v.as_i64()).unwrap_or(0);

    let mut track = serde_json::json!({
        "id": str_at(item, "short_code"),
        "title": title,
        "artist": artist,
        "duration": duration,
        "durationMs": if duration > 0 { duration * 1000 } else { 180000 },
        // Eclipse/Android cannot decode ALAC in `.m4a`; the download path
        // decodes those to PCM and serves them as WAV, so the advertised
        // format (and the served URL extension) follow suit.
        "format": if ext == "m4a" || ext == "alac" { "wav".to_string() } else { ext },
    });
    if !isrc.is_empty() {
        track["isrc"] = serde_json::json!(isrc);
    }
    let thumb = str_at(item, "thumbnail_url");
    if !thumb.is_empty() {
        track["artworkURL"] = serde_json::json!(thumb);
    }
    track
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_artist_dash_title_from_caption() {
        let (artist, title, _) = parse_audio_title(
            "01 Adele - Hello.mp3",
            "Adele - Hello\nsome other text\n[AABB1122333]",
        );
        assert_eq!(artist, "Adele");
        assert_eq!(title, "Hello");
    }

    #[test]
    fn splits_dash_in_filename_and_strips_noise() {
        let (artist, title, _) = parse_audio_title("02 Wali - Yank.mp3", "");
        assert_eq!(artist, "Wali");
        assert_eq!(title, "Yank");
    }

    #[test]
    fn uses_known_artist_prefix() {
        let (artist, title, _) = parse_audio_title("Tulus Monokrom", "");
        assert_eq!(artist, "Tulus");
        assert_eq!(title, "Monokrom");
    }

    #[test]
    fn reversed_order_detects_trailing_artist() {
        let track = format_track(
            &serde_json::json!({
                "title": "Black.or.White.Michael.Jackson",
                "caption": "",
                "short_code": "abc123",
                "extension": "mp3",
                "duration": 200
            }),
            "",
        );
        assert_eq!(track["artist"], "Michael Jackson");
        assert_eq!(track["title"], "Black Or White");
        assert_eq!(track["durationMs"], 200000);
    }

    #[test]
    fn query_split_title_plus_artist() {
        // query = "<title> <artist>" ⇒ the prefix is the artist.
        let track = format_track(
            &serde_json::json!({
                "title": "Timeless",
                "caption": "",
                "short_code": "x1",
                "extension": "m4a"
            }),
            "Timeless The Weeknd",
        );
        assert_eq!(track["artist"], "Timeless");
        assert_eq!(track["title"], "The Weeknd");
        // m4a is advertised as wav because the downloader decodes it to PCM.
        assert_eq!(track["format"], "wav");
        assert_eq!(track["durationMs"], 180000);
    }

    #[test]
    fn quality_scores_order() {
        assert!(quality_score("flac", 10_000_000) > quality_score("m4a", 10_000_000));
        assert!(quality_score("m4a", 10_000_000) > quality_score("mp3", 10_000_000));
        assert!(quality_score("mp3", 100 * 1024) > quality_score("mp3", 0));
    }
}
