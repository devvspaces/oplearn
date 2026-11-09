//! WebVTT parsing into transcript cues.
//!
//! Handles two shapes:
//! - **manual** subtitles: ordinary cues, kept as-is (multi-line cues joined with spaces);
//! - **YouTube auto-captions** ("rolling"): each cue repeats the previous line and adds a new one
//!   with inline word timings (`<00:00:01.840><c> right.</c>`), interleaved with ~10 ms cues that
//!   repeat the full line. With `rolling = true` we keep only the newly added text per cue.

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Parses `HH:MM:SS.mmm` or `MM:SS.mmm` (comma also accepted) into seconds.
pub fn parse_timestamp(s: &str) -> Option<f64> {
    let s = s.trim().replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (
            h.parse::<f64>().ok()?,
            m.parse::<f64>().ok()?,
            s.parse::<f64>().ok()?,
        ),
        // MM:SS.mmm; NPTEL's files keep counting minutes past 60.
        [m, s] => (0.0, m.parse::<f64>().ok()?, s.parse::<f64>().ok()?),
        _ => return None,
    };
    ((m < 60.0 || parts.len() == 2) && sec < 60.0).then_some(h * 3600.0 + m * 60.0 + sec)
}

/// Removes `<...>` tags (voice/class/italic/inline timestamps) and decodes common entities.
pub fn clean_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

struct RawCue {
    start: f64,
    end: f64,
    lines: Vec<String>,
}

fn raw_cues(input: &str) -> Vec<RawCue> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in normalized.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(ti) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let first = lines.first().map(|l| l.trim_start()).unwrap_or("");
        if first.starts_with("NOTE") || first.starts_with("STYLE") || first.starts_with("REGION") {
            continue;
        }
        let timing = lines[ti];
        let (a, rest) = match timing.split_once("-->") {
            Some(x) => x,
            None => continue,
        };
        let b = rest.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (parse_timestamp(a), parse_timestamp(b)) else {
            continue;
        };
        cues.push(RawCue {
            start,
            end,
            lines: lines[ti + 1..].iter().map(|l| l.to_string()).collect(),
        });
    }
    cues
}

pub fn parse(input: &str, rolling: bool) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::new();
    let mut last_line = String::new();
    for raw in raw_cues(input) {
        let lines: Vec<String> = raw
            .lines
            .iter()
            .map(|l| clean_text(l))
            .filter(|l| !l.is_empty())
            .collect();
        let text = if rolling {
            // Skip the ~10 ms "snapshot" cues that only repeat the finished line.
            if raw.end - raw.start < 0.05 {
                continue;
            }
            let fresh: Vec<&String> = lines.iter().skip_while(|l| **l == last_line).collect();
            if let Some(l) = fresh.last() {
                last_line = (*l).clone();
            }
            fresh
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            lines.join(" ")
        };
        if text.is_empty() {
            continue;
        }
        out.push(Cue {
            start: raw.start,
            end: raw.end.max(raw.start),
            text,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(parse_timestamp("00:00:01.680"), Some(1.68));
        assert_eq!(parse_timestamp("01:02:03,500"), Some(3723.5));
        assert_eq!(parse_timestamp("02:03.000"), Some(123.0));
        assert_eq!(parse_timestamp("nope"), None);
        assert_eq!(parse_timestamp("00:61:00.000"), None);
    }

    #[test]
    fn cleans_tags_and_entities() {
        assert_eq!(
            clean_text("Let's<00:00:03.560><c> get</c><00:00:03.720><c> started.</c>"),
            "Let's get started."
        );
        assert_eq!(clean_text("<v Speaker>Hi &amp; bye</v>"), "Hi & bye");
        assert_eq!(clean_text("  a &nbsp; b  "), "a b");
    }

    #[test]
    fn parses_youtube_auto_captions_without_duplicates() {
        let cues = parse(include_str!("fixtures/auto_captions.vtt"), true);
        let texts: Vec<&str> = cues.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(
            &texts[..4],
            &[
                "All right.",
                "Let's get started.",
                "This is 6824 distributed systems.",
                "Um"
            ]
        );
        assert_eq!(cues[0].start, 1.68);
        assert_eq!(cues[2].start, 6.04);
        // No cue repeats its predecessor and none are the 10 ms snapshots.
        assert!(cues.windows(2).all(|w| w[0].text != w[1].text));
        assert!(cues.iter().all(|c| c.end - c.start >= 0.05));
        assert!(cues.windows(2).all(|w| w[0].start <= w[1].start));
    }

    #[test]
    fn parses_manual_subtitles() {
        let cues = parse(include_str!("fixtures/manual_captions.vtt"), false);
        assert_eq!(cues.len(), 4);
        assert_eq!(cues[0].text, "Welcome to the course & thanks for joining.");
        assert_eq!(cues[1].text, "Today we cover RPC.");
        assert_eq!(cues[1].end, 5.25);
        // Manual subs may legitimately repeat a line.
        assert_eq!(cues[2].text, "Yes.");
        assert_eq!(cues[3].text, "Yes.");
        assert_eq!(cues[3].start, 3600.0);
    }

    #[test]
    fn tolerates_crlf_and_garbage() {
        let cues = parse(
            "WEBVTT\r\n\r\n00:00:01.000 --> 00:00:02.000\r\nHello\r\n\r\nbroken --> line\r\nx\r\n",
            false,
        );
        assert_eq!(
            cues,
            vec![Cue {
                start: 1.0,
                end: 2.0,
                text: "Hello".into()
            }]
        );
        assert!(parse("", true).is_empty());
    }
}
