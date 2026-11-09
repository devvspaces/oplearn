//! Source-agnostic import types and YouTube URL parsing.
//!
//! Both the Data API client (`youtube.rs`) and the yt-dlp fallback (`ytdlp.rs`)
//! normalise into these types; `db::repo::import` turns them into rows.

use serde::Serialize;
use specta::Type;
use url::Url;

use crate::db::models::License;

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedVideo {
    pub video_id: String,
    pub title: String,
    pub description: Option<String>,
    pub channel_title: Option<String>,
    pub duration_sec: Option<i32>,
    pub license: Option<License>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedPlaylist {
    pub playlist_id: String,
    pub title: String,
    pub description: Option<String>,
    pub channel_title: Option<String>,
    pub thumbnail_url: Option<String>,
    pub videos: Vec<ImportedVideo>,
    /// Entries that were private/deleted/unavailable and therefore skipped.
    pub skipped: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImportSource {
    Api,
    Ytdlp,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub course_id: String,
    pub added: i32,
    pub skipped: i32,
    pub source: ImportSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedUrl {
    pub video_id: Option<String>,
    pub playlist_id: Option<String>,
}

fn is_video_id(s: &str) -> bool {
    s.len() == 11
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn is_playlist_id(s: &str) -> bool {
    const PREFIXES: [&str; 8] = ["PL", "UU", "LL", "FL", "OL", "RD", "UL", "PU"];
    s.len() >= 12
        && PREFIXES.iter().any(|p| s.starts_with(p))
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Parses YouTube URLs (watch, playlist, youtu.be, shorts, embed, live) or bare IDs.
pub fn parse_youtube_url(input: &str) -> Option<ParsedUrl> {
    let input = input.trim();
    if is_video_id(input) {
        return Some(ParsedUrl {
            video_id: Some(input.into()),
            playlist_id: None,
        });
    }
    if is_playlist_id(input) {
        return Some(ParsedUrl {
            video_id: None,
            playlist_id: Some(input.into()),
        });
    }
    let with_scheme = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let url = Url::parse(&with_scheme).ok()?;
    let host = url
        .host_str()?
        .trim_start_matches("www.")
        .trim_start_matches("m.")
        .to_ascii_lowercase();
    let q = |k: &str| {
        url.query_pairs()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.into_owned())
    };
    let mut out = ParsedUrl {
        playlist_id: q("list").filter(|l| is_playlist_id(l)),
        ..Default::default()
    };
    let segs: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    match host.as_str() {
        "youtu.be" => out.video_id = segs.first().map(|s| s.to_string()),
        "youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => match segs.as_slice() {
            ["watch"] => out.video_id = q("v"),
            ["shorts" | "embed" | "live" | "v", id, ..] => out.video_id = Some(id.to_string()),
            ["playlist"] => {}
            _ => {}
        },
        _ => return None,
    }
    out.video_id = out.video_id.filter(|v| is_video_id(v));
    (out.video_id.is_some() || out.playlist_id.is_some()).then_some(out)
}

/// Thumbnail URL for a video, served by YouTube's image CDN.
pub fn thumbnail_url(video_id: &str) -> String {
    format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg")
}

/// Parses ISO 8601 durations as returned by the Data API (e.g. `PT1H2M3S`, `P1DT2H`).
pub fn parse_iso8601_duration(s: &str) -> Option<i32> {
    let rest = s.strip_prefix('P')?;
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) => (d, t),
        None => (rest, ""),
    };
    fn sum(part: &str, units: &[(char, i64)]) -> Option<i64> {
        let mut total = 0i64;
        let mut num = String::new();
        for c in part.chars() {
            if c.is_ascii_digit() || c == '.' {
                num.push(c);
            } else {
                let mult = units.iter().find(|(u, _)| *u == c)?.1;
                let v: f64 = num.parse().ok()?;
                total += (v * mult as f64) as i64;
                num.clear();
            }
        }
        num.is_empty().then_some(total)
    }
    let d = sum(date, &[('W', 604_800), ('D', 86_400)])?;
    let t = sum(time, &[('H', 3600), ('M', 60), ('S', 1)])?;
    i32::try_from(d + t).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(v: Option<&str>, l: Option<&str>) -> Option<ParsedUrl> {
        Some(ParsedUrl {
            video_id: v.map(Into::into),
            playlist_id: l.map(Into::into),
        })
    }

    #[test]
    fn parses_urls() {
        let pl = "PLrw6a1wE39_tb2fErI4-WkMbsvGQk9_UB";
        assert_eq!(
            parse_youtube_url(&format!("https://www.youtube.com/playlist?list={pl}")),
            p(None, Some(pl))
        );
        assert_eq!(
            parse_youtube_url(&format!(
                "https://youtube.com/watch?v=cQP8WApzIQQ&list={pl}&index=2"
            )),
            p(Some("cQP8WApzIQQ"), Some(pl))
        );
        assert_eq!(
            parse_youtube_url("https://youtu.be/cQP8WApzIQQ?t=30"),
            p(Some("cQP8WApzIQQ"), None)
        );
        assert_eq!(
            parse_youtube_url("m.youtube.com/shorts/cQP8WApzIQQ"),
            p(Some("cQP8WApzIQQ"), None)
        );
        assert_eq!(
            parse_youtube_url("https://www.youtube.com/embed/cQP8WApzIQQ"),
            p(Some("cQP8WApzIQQ"), None)
        );
        assert_eq!(
            parse_youtube_url("cQP8WApzIQQ"),
            p(Some("cQP8WApzIQQ"), None)
        );
        assert_eq!(parse_youtube_url(pl), p(None, Some(pl)));
        assert_eq!(parse_youtube_url("https://vimeo.com/123"), None);
        assert_eq!(parse_youtube_url("https://youtube.com/watch?v=short"), None);
        assert_eq!(parse_youtube_url("not a url"), None);
    }

    #[test]
    fn parses_durations() {
        assert_eq!(parse_iso8601_duration("PT1H2M3S"), Some(3723));
        assert_eq!(parse_iso8601_duration("PT45S"), Some(45));
        assert_eq!(parse_iso8601_duration("PT1H"), Some(3600));
        assert_eq!(parse_iso8601_duration("P1DT1S"), Some(86_401));
        assert_eq!(parse_iso8601_duration("P0D"), Some(0));
        assert_eq!(parse_iso8601_duration("garbage"), None);
        assert_eq!(parse_iso8601_duration("PT1X"), None);
    }
}
