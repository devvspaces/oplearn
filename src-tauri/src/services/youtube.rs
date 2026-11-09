//! YouTube Data API v3 client (metadata only; never scrapes HTML).

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};

use serde::Deserialize;

use crate::db::models::{
    DurationFilter, License, SearchKind, UploadFilter, YtSearchQuery, YtSearchResult,
};
use crate::error::{AppError, AppResult, ErrorCode};
use crate::services::import::{ImportedPlaylist, ImportedVideo, parse_iso8601_duration};

const BASE: &str = "https://www.googleapis.com/youtube/v3";

#[derive(Debug, Deserialize)]
struct ListResponse<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
    #[serde(rename = "nextPageToken")]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Thumb {
    url: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct Thumbs {
    medium: Option<Thumb>,
    high: Option<Thumb>,
    standard: Option<Thumb>,
    maxres: Option<Thumb>,
    default: Option<Thumb>,
}

impl Thumbs {
    fn best(&self) -> Option<String> {
        [
            &self.maxres,
            &self.standard,
            &self.high,
            &self.medium,
            &self.default,
        ]
        .into_iter()
        .flatten()
        .next()
        .map(|t| t.url.clone())
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Snippet {
    title: String,
    description: String,
    channel_title: Option<String>,
    video_owner_channel_title: Option<String>,
    thumbnails: Thumbs,
    resource_id: Option<ResourceId>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ResourceId {
    video_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlaylistResource {
    snippet: Snippet,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ItemStatus {
    privacy_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PlaylistItem {
    snippet: Snippet,
    #[serde(default)]
    status: ItemStatus,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct ContentDetails {
    duration: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct VideoStatus {
    license: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VideoResource {
    id: String,
    snippet: Snippet,
    #[serde(rename = "contentDetails", default)]
    content_details: ContentDetails,
    #[serde(default)]
    status: VideoStatus,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    error: ApiError,
}
#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
    #[serde(default)]
    errors: Vec<ApiErrorItem>,
}
#[derive(Debug, Deserialize)]
struct ApiErrorItem {
    reason: String,
}

fn map_license(s: Option<&str>) -> Option<License> {
    match s? {
        "creativeCommon" => Some(License::CreativeCommon),
        "youtube" => Some(License::Youtube),
        _ => None,
    }
}

fn non_empty(s: String) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Maps an API error body to an `AppError`; quota errors get `ErrorCode::Quota` so callers can fall back.
fn map_api_error(status: u16, body: &str) -> AppError {
    match serde_json::from_str::<ApiErrorBody>(body) {
        Ok(b) => {
            let reason = b
                .error
                .errors
                .first()
                .map(|e| e.reason.as_str())
                .unwrap_or("");
            let code = match reason {
                "quotaExceeded" | "dailyLimitExceeded" | "rateLimitExceeded" => ErrorCode::Quota,
                "keyInvalid" | "badRequest" => ErrorCode::Invalid,
                "playlistNotFound" | "videoNotFound" => ErrorCode::NotFound,
                _ => ErrorCode::Http,
            };
            AppError::new(code, format!("YouTube API: {}", b.error.message))
        }
        Err(_) => AppError::new(
            ErrorCode::Http,
            format!("YouTube API returned HTTP {status}"),
        ),
    }
}

pub enum Auth<'a> {
    /// Data API key (public data).
    Key(&'a str),
    /// OAuth access token (the signed-in user's data, e.g. `mine=true`).
    Bearer(&'a str),
}

pub struct YouTubeApi<'a> {
    pub http: &'a reqwest::Client,
    pub auth: Auth<'a>,
    /// Quota units spent by this client (see `repo::yt_search::add_units`).
    pub units: AtomicI32,
}

/// Data API cost of one call to `path`.
fn cost(path: &str) -> i32 {
    if path == "search" { 100 } else { 1 }
}

impl<'a> YouTubeApi<'a> {
    pub fn new(http: &'a reqwest::Client, api_key: &'a str) -> Self {
        Self {
            http,
            auth: Auth::Key(api_key),
            units: AtomicI32::new(0),
        }
    }

    pub fn bearer(http: &'a reqwest::Client, token: &'a str) -> Self {
        Self {
            http,
            auth: Auth::Bearer(token),
            units: AtomicI32::new(0),
        }
    }

    pub fn units(&self) -> i32 {
        self.units.load(Ordering::Relaxed)
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> AppResult<T> {
        let mut req = self.http.get(format!("{BASE}/{path}")).query(query);
        req = match self.auth {
            Auth::Key(k) => req.query(&[("key", k)]),
            Auth::Bearer(t) => req.bearer_auth(t),
        };
        self.units.fetch_add(cost(path), Ordering::Relaxed);
        let resp = req.send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(map_api_error(status.as_u16(), &body));
        }
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn fetch_playlist(&self, playlist_id: &str) -> AppResult<ImportedPlaylist> {
        let meta: ListResponse<PlaylistResource> = self
            .get("playlists", &[("part", "snippet"), ("id", playlist_id)])
            .await?;
        let meta = meta
            .items
            .into_iter()
            .next()
            .ok_or_else(|| AppError::not_found("playlist"))?;

        let mut items = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut q = vec![
                ("part", "snippet,status"),
                ("playlistId", playlist_id),
                ("maxResults", "50"),
            ];
            if let Some(p) = page.as_deref() {
                q.push(("pageToken", p));
            }
            let r: ListResponse<PlaylistItem> = self.get("playlistItems", &q).await?;
            items.extend(r.items);
            match r.next_page_token {
                Some(t) if items.len() < 5000 => page = Some(t),
                _ => break,
            }
        }
        let ids: Vec<String> = items
            .iter()
            .filter(|i| is_available(i))
            .filter_map(|i| {
                i.snippet
                    .resource_id
                    .as_ref()
                    .and_then(|r| r.video_id.clone())
            })
            .collect();
        let details = self.fetch_videos(&ids).await?;
        Ok(assemble_playlist(playlist_id, meta, items, details))
    }

    pub async fn fetch_video(&self, video_id: &str) -> AppResult<ImportedVideo> {
        let mut m = self.fetch_videos(&[video_id.to_string()]).await?;
        m.remove(video_id)
            .ok_or_else(|| AppError::not_found("video"))
    }

    async fn fetch_videos(&self, ids: &[String]) -> AppResult<HashMap<String, ImportedVideo>> {
        let mut out = HashMap::new();
        for chunk in ids.chunks(50) {
            let joined = chunk.join(",");
            let r: ListResponse<VideoResource> = self
                .get(
                    "videos",
                    &[("part", "snippet,contentDetails,status"), ("id", &joined)],
                )
                .await?;
            for v in r.items {
                let iv = video_from_resource(v);
                out.insert(iv.video_id.clone(), iv);
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SearchId {
    kind: String,
    video_id: Option<String>,
    playlist_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SearchSnippet {
    title: String,
    channel_title: Option<String>,
    published_at: Option<String>,
    thumbnails: Thumbs,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    id: SearchId,
    #[serde(default)]
    snippet: SearchSnippet,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Statistics {
    view_count: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VideoStats {
    id: String,
    #[serde(rename = "contentDetails", default)]
    content_details: ContentDetails,
    #[serde(default)]
    status: VideoStatus,
    #[serde(default)]
    statistics: Statistics,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PlaylistDetails {
    item_count: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct PlaylistMeta {
    id: String,
    #[serde(default)]
    snippet: SearchSnippet,
    #[serde(rename = "contentDetails", default)]
    content_details: PlaylistDetails,
}

/// HTML entities YouTube leaves in search titles (`&#39;`, `&amp;` …).
fn unescape(s: &str) -> String {
    s.replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// `publishedAfter` for an upload-date filter.
fn published_after(f: UploadFilter, now: chrono::DateTime<chrono::Utc>) -> String {
    let days = match f {
        UploadFilter::Week => 7,
        UploadFilter::Month => 31,
        UploadFilter::Year => 365,
    };
    (now - chrono::Duration::days(days)).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl YouTubeApi<'_> {
    /// `search.list` (100 units) plus one details call (1 unit) for duration/views/license or
    /// playlist sizes.
    pub async fn search(&self, q: &YtSearchQuery) -> AppResult<Vec<YtSearchResult>> {
        let kind = match q.kind {
            SearchKind::Video => "video",
            SearchKind::Playlist => "playlist",
        };
        let after = q.upload.map(|u| published_after(u, chrono::Utc::now()));
        let mut query: Vec<(&str, &str)> = vec![
            ("part", "snippet"),
            ("type", kind),
            ("q", q.query.as_str()),
            ("maxResults", "25"),
            ("safeSearch", "none"),
        ];
        if let Some(a) = after.as_deref() {
            query.push(("publishedAfter", a));
        }
        if q.kind == SearchKind::Video {
            if q.cc_only {
                query.push(("videoLicense", "creativeCommon"));
            }
            if let Some(d) = q.duration {
                query.push((
                    "videoDuration",
                    match d {
                        DurationFilter::Short => "short",
                        DurationFilter::Medium => "medium",
                        DurationFilter::Long => "long",
                    },
                ));
            }
        }
        let r: ListResponse<SearchItem> = self.get("search", &query).await?;
        let mut results: Vec<YtSearchResult> = r
            .items
            .into_iter()
            .filter_map(|i| {
                let id = match q.kind {
                    SearchKind::Video => i.id.video_id,
                    SearchKind::Playlist => i.id.playlist_id,
                }?;
                let _ = i.id.kind;
                Some(YtSearchResult {
                    kind: q.kind,
                    id,
                    title: unescape(&i.snippet.title),
                    channel_title: i.snippet.channel_title.map(|c| unescape(&c)),
                    thumbnail_url: i.snippet.thumbnails.best(),
                    duration_sec: None,
                    view_count: None,
                    license: None,
                    published_at: i.snippet.published_at,
                    item_count: None,
                })
            })
            .collect();
        if results.is_empty() {
            return Ok(results);
        }
        let ids = results
            .iter()
            .map(|r| r.id.as_str())
            .collect::<Vec<_>>()
            .join(",");
        match q.kind {
            SearchKind::Video => {
                let d: ListResponse<VideoStats> = self
                    .get(
                        "videos",
                        &[("part", "contentDetails,status,statistics"), ("id", &ids)],
                    )
                    .await?;
                let m: HashMap<String, VideoStats> =
                    d.items.into_iter().map(|v| (v.id.clone(), v)).collect();
                for r in &mut results {
                    if let Some(v) = m.get(&r.id) {
                        r.duration_sec = v
                            .content_details
                            .duration
                            .as_deref()
                            .and_then(parse_iso8601_duration);
                        r.license = map_license(v.status.license.as_deref());
                        r.view_count = v
                            .statistics
                            .view_count
                            .as_deref()
                            .and_then(|s| s.parse().ok());
                    }
                }
            }
            SearchKind::Playlist => {
                let d: ListResponse<PlaylistMeta> = self
                    .get("playlists", &[("part", "contentDetails"), ("id", &ids)])
                    .await?;
                let m: HashMap<String, i32> = d
                    .items
                    .into_iter()
                    .filter_map(|p| Some((p.id, p.content_details.item_count?)))
                    .collect();
                for r in &mut results {
                    r.item_count = m.get(&r.id).copied();
                }
            }
        }
        Ok(results)
    }

    /// The signed-in user's playlists (`mine=true`; needs a Bearer client).
    pub async fn my_playlists(&self) -> AppResult<Vec<YtSearchResult>> {
        let mut out = Vec::new();
        let mut page: Option<String> = None;
        loop {
            let mut q = vec![
                ("part", "snippet,contentDetails"),
                ("mine", "true"),
                ("maxResults", "50"),
            ];
            if let Some(p) = page.as_deref() {
                q.push(("pageToken", p));
            }
            let r: ListResponse<PlaylistMeta> = self.get("playlists", &q).await?;
            out.extend(r.items.into_iter().map(|p| YtSearchResult {
                kind: SearchKind::Playlist,
                id: p.id,
                title: p.snippet.title,
                channel_title: p.snippet.channel_title,
                thumbnail_url: p.snippet.thumbnails.best(),
                duration_sec: None,
                view_count: None,
                license: None,
                published_at: p.snippet.published_at,
                item_count: p.content_details.item_count,
            }));
            match r.next_page_token {
                Some(t) if out.len() < 500 => page = Some(t),
                _ => break,
            }
        }
        Ok(out)
    }

    /// Title of the signed-in user's channel (shown as "Signed in as …").
    pub async fn my_channel_title(&self) -> AppResult<Option<String>> {
        let r: ListResponse<PlaylistResource> = self
            .get("channels", &[("part", "snippet"), ("mine", "true")])
            .await?;
        Ok(r.items.into_iter().next().map(|c| c.snippet.title))
    }
}

fn is_available(i: &PlaylistItem) -> bool {
    let privacy_ok = !matches!(
        i.status.privacy_status.as_deref(),
        Some("private" | "privacyStatusUnspecified")
    );
    let t = i.snippet.title.as_str();
    privacy_ok && t != "Deleted video" && t != "Private video"
}

fn video_from_resource(v: VideoResource) -> ImportedVideo {
    ImportedVideo {
        video_id: v.id,
        title: v.snippet.title,
        description: non_empty(v.snippet.description),
        channel_title: v.snippet.channel_title,
        duration_sec: v
            .content_details
            .duration
            .as_deref()
            .and_then(parse_iso8601_duration),
        license: map_license(v.status.license.as_deref()),
    }
}

fn assemble_playlist(
    playlist_id: &str,
    meta: PlaylistResource,
    items: Vec<PlaylistItem>,
    mut details: HashMap<String, ImportedVideo>,
) -> ImportedPlaylist {
    let mut videos = Vec::new();
    let mut skipped = 0;
    for item in items {
        let id = item
            .snippet
            .resource_id
            .as_ref()
            .and_then(|r| r.video_id.clone());
        match id.and_then(|id| details.remove(&id)) {
            Some(v) if is_available(&item) => videos.push(v),
            _ => skipped += 1,
        }
    }
    ImportedPlaylist {
        playlist_id: playlist_id.to_string(),
        title: meta.snippet.title,
        description: non_empty(meta.snippet.description),
        channel_title: meta
            .snippet
            .channel_title
            .or(meta.snippet.video_owner_channel_title),
        thumbnail_url: meta.snippet.thumbnails.best(),
        videos,
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn assembles_playlist_in_order_skipping_unavailable() {
        let meta: PlaylistResource = serde_json::from_value(json!({
            "snippet": { "title": "6.824", "description": "", "channelTitle": "MIT",
                         "thumbnails": { "medium": { "url": "m.jpg" }, "high": { "url": "h.jpg" } } }
        }))
        .unwrap();
        let items: Vec<PlaylistItem> = serde_json::from_value(json!([
            { "snippet": { "title": "B", "resourceId": { "videoId": "bbbbbbbbbbb" } }, "status": { "privacyStatus": "public" } },
            { "snippet": { "title": "Deleted video", "resourceId": { "videoId": "ddddddddddd" } } },
            { "snippet": { "title": "A", "resourceId": { "videoId": "aaaaaaaaaaa" } }, "status": { "privacyStatus": "public" } }
        ]))
        .unwrap();
        let details: Vec<VideoResource> = serde_json::from_value(json!([
            { "id": "aaaaaaaaaaa", "snippet": { "title": "A", "description": "desc", "channelTitle": "MIT" },
              "contentDetails": { "duration": "PT1H20M" }, "status": { "license": "creativeCommon" } },
            { "id": "bbbbbbbbbbb", "snippet": { "title": "B", "channelTitle": "MIT" },
              "contentDetails": { "duration": "PT5M" }, "status": { "license": "youtube" } }
        ]))
        .unwrap();
        let map = details
            .into_iter()
            .map(|v| (v.id.clone(), video_from_resource(v)))
            .collect();
        let p = assemble_playlist("PLx", meta, items, map);
        assert_eq!(p.thumbnail_url.as_deref(), Some("h.jpg"));
        assert_eq!(p.description, None);
        assert_eq!(p.skipped, 1);
        let ids: Vec<_> = p.videos.iter().map(|v| v.video_id.as_str()).collect();
        assert_eq!(ids, vec!["bbbbbbbbbbb", "aaaaaaaaaaa"]);
        assert_eq!(p.videos[1].duration_sec, Some(4800));
        assert_eq!(p.videos[1].license, Some(License::CreativeCommon));
    }

    #[test]
    fn maps_quota_errors() {
        let body =
            r#"{"error":{"code":403,"message":"quota","errors":[{"reason":"quotaExceeded"}]}}"#;
        assert_eq!(map_api_error(403, body).code, ErrorCode::Quota);
        assert_eq!(map_api_error(500, "<html>").code, ErrorCode::Http);
    }
}
