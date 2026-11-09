//! Row types and command inputs shared with the renderer (exported via specta).

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use specta::Type;

/// Implements string-backed SQL conversion and `as_str` for a unit enum.
macro_rules! sql_enum {
    ($name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        impl $name {
            pub fn as_str(&self) -> &'static str {
                match self { $(Self::$variant => $s),+ }
            }
            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some(Self::$variant),)+ _ => None }
            }
        }
        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::from(self.as_str()))
            }
        }
        impl FromSql for $name {
            fn column_result(v: ValueRef<'_>) -> FromSqlResult<Self> {
                let s = v.as_str()?;
                Self::parse(s).ok_or_else(|| FromSqlError::Other(format!("invalid {}: {s}", stringify!($name)).into()))
            }
        }
    };
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CourseMode {
    Stream,
    Download,
}
sql_enum!(CourseMode { Stream => "stream", Download => "download" });

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CourseStatus {
    Active,
    Archived,
    Completed,
}
sql_enum!(CourseStatus { Active => "active", Archived => "archived", Completed => "completed" });

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LessonType {
    Video,
    Reading,
    Lab,
    Quiz,
}
sql_enum!(LessonType { Video => "video", Reading => "reading", Lab => "lab", Quiz => "quiz" });

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
pub enum License {
    #[serde(rename = "youtube")]
    Youtube,
    #[serde(rename = "creativeCommon")]
    CreativeCommon,
}
sql_enum!(License { Youtube => "youtube", CreativeCommon => "creativeCommon" });

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Course {
    pub id: String,
    pub title: String,
    pub slug: String,
    pub description: Option<String>,
    pub instructor: Option<String>,
    pub source_url: Option<String>,
    pub youtube_playlist_id: Option<String>,
    pub cover_path: Option<String>,
    pub tags: Vec<String>,
    pub mode: CourseMode,
    pub status: CourseStatus,
    pub playback_rate: f64,
    pub created_at: String,
    pub updated_at: String,
}

/// Library card data: course plus aggregate progress.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CourseSummary {
    pub course: Course,
    pub lesson_count: i32,
    pub completed_count: i32,
    pub total_duration_sec: i32,
    pub next_lesson_id: Option<String>,
    pub next_lesson_title: Option<String>,
    pub last_studied_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub id: String,
    pub course_id: String,
    pub title: String,
    pub position: i32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    pub id: String,
    pub module_id: String,
    pub position: i32,
    #[serde(rename = "type")]
    pub lesson_type: LessonType,
    pub title: String,
    pub description: Option<String>,
    pub url: Option<String>,
    pub youtube_video_id: Option<String>,
    pub channel_title: Option<String>,
    pub license: Option<License>,
    pub duration_sec: Option<i32>,
    pub local_video_path: Option<String>,
    /// Resolution actually downloaded, e.g. "720p".
    pub local_quality: Option<String>,
    pub thumbnail_path: Option<String>,
    pub completed_at: Option<String>,
    pub due_date: Option<String>,
    /// From `watch_progress`, if any.
    pub last_position_sec: Option<f64>,
    /// For document lessons: the primary material's kind ("assignment", "lecture_notes", …).
    pub doc_kind: Option<String>,
    /// Page count of that material, when known.
    pub doc_pages: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModuleWithLessons {
    pub module: Module,
    pub lessons: Vec<Lesson>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CourseDetail {
    pub course: Course,
    pub modules: Vec<ModuleWithLessons>,
}

/// Everything the player needs for one lesson.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LessonContext {
    pub lesson: Lesson,
    pub course: Course,
    pub module_title: String,
    pub prev_lesson_id: Option<String>,
    pub next_lesson_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct NewCourse {
    pub title: String,
    #[specta(optional)]
    pub description: Option<String>,
    #[specta(optional)]
    pub instructor: Option<String>,
    #[specta(optional)]
    pub source_url: Option<String>,
    #[specta(optional)]
    pub cover_path: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct CourseUpdate {
    #[specta(optional)]
    pub title: Option<String>,
    #[specta(optional)]
    pub description: Option<String>,
    #[specta(optional)]
    pub instructor: Option<String>,
    #[specta(optional)]
    pub source_url: Option<String>,
    #[specta(optional)]
    pub cover_path: Option<String>,
    #[specta(optional)]
    pub tags: Option<Vec<String>>,
    #[specta(optional)]
    pub status: Option<CourseStatus>,
    #[specta(optional)]
    pub mode: Option<CourseMode>,
    #[specta(optional)]
    pub playback_rate: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct LessonUpdate {
    #[specta(optional)]
    pub title: Option<String>,
    #[specta(optional)]
    pub description: Option<String>,
    #[specta(optional)]
    pub url: Option<String>,
    #[specta(optional)]
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewLesson {
    pub module_id: String,
    #[serde(rename = "type")]
    pub lesson_type: LessonType,
    pub title: String,
    #[specta(optional)]
    pub description: Option<String>,
    #[specta(optional)]
    pub url: Option<String>,
}

/// One module's full lesson order, used by drag-and-drop to persist a layout.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModuleOrder {
    pub module_id: String,
    pub lesson_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchProgress {
    pub lesson_id: String,
    pub last_position_sec: f64,
    pub watched_ratio: f64,
}

// ---------------------------------------------------------------- Phase 2: annotations etc.

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationType {
    Note,
    Bookmark,
    Question,
    Todo,
    Reference,
}
sql_enum!(AnnotationType {
    Note => "note",
    Bookmark => "bookmark",
    Question => "question",
    Todo => "todo",
    Reference => "reference"
});

impl AnnotationType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Bookmark => "Bookmark",
            Self::Question => "Question",
            Self::Todo => "To-do",
            Self::Reference => "Reference",
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Annotation {
    pub id: String,
    pub lesson_id: String,
    #[serde(rename = "type")]
    pub kind: AnnotationType,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub title: Option<String>,
    pub body_md: Option<String>,
    /// question: open | researching | answered; todo: open | in_progress | done
    pub status: Option<String>,
    /// todo: low | medium | high
    pub priority: Option<String>,
    pub due_date: Option<String>,
    pub url: Option<String>,
    /// reference: paper | article | repo | book | video | other
    pub ref_type: Option<String>,
    pub answer_md: Option<String>,
    pub tags: Vec<String>,
    pub ai_generated: bool,
    pub created_at: String,
    pub updated_at: String,
    /// Page anchor on a PDF material (both set, or neither).
    pub material_id: Option<String>,
    pub page: Option<i32>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewAnnotation {
    pub lesson_id: String,
    #[serde(rename = "type")]
    pub kind: AnnotationType,
    #[specta(optional)]
    pub start_sec: Option<f64>,
    #[specta(optional)]
    pub end_sec: Option<f64>,
    #[specta(optional)]
    pub title: Option<String>,
    #[specta(optional)]
    pub body_md: Option<String>,
    #[specta(optional)]
    pub status: Option<String>,
    #[specta(optional)]
    pub priority: Option<String>,
    #[specta(optional)]
    pub due_date: Option<String>,
    #[specta(optional)]
    pub url: Option<String>,
    #[specta(optional)]
    pub ref_type: Option<String>,
    #[specta(optional)]
    pub answer_md: Option<String>,
    #[serde(default)]
    #[specta(optional)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    #[specta(optional)]
    pub material_id: Option<String>,
    #[serde(default)]
    #[specta(optional)]
    pub page: Option<i32>,
}

/// Partial update. For text fields, `Some("")` clears the value.
#[derive(Debug, Clone, Default, Deserialize, Type)]
#[serde(rename_all = "camelCase", default)]
pub struct AnnotationUpdate {
    #[specta(optional)]
    pub start_sec: Option<f64>,
    #[specta(optional)]
    pub title: Option<String>,
    #[specta(optional)]
    pub body_md: Option<String>,
    #[specta(optional)]
    pub status: Option<String>,
    #[specta(optional)]
    pub priority: Option<String>,
    #[specta(optional)]
    pub due_date: Option<String>,
    #[specta(optional)]
    pub url: Option<String>,
    #[specta(optional)]
    pub ref_type: Option<String>,
    #[specta(optional)]
    pub answer_md: Option<String>,
    #[specta(optional)]
    pub tags: Option<Vec<String>>,
    /// Moves a page anchor (keeps the material).
    #[specta(optional)]
    pub page: Option<i32>,
}

/// An annotation plus where it lives, for course-wide views and export.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationInContext {
    pub annotation: Annotation,
    pub lesson_title: String,
    pub youtube_video_id: Option<String>,
    pub module_id: String,
    pub module_title: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptCue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Transcript {
    pub lesson_id: String,
    /// manual | auto | none (checked, nothing available) | null (never fetched)
    pub source: Option<String>,
    pub lang: Option<String>,
    pub fetched_at: Option<String>,
    pub cues: Vec<TranscriptCue>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Chapter {
    pub id: String,
    pub start_sec: f64,
    pub end_sec: Option<f64>,
    pub title: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NotebookScope {
    Lesson,
    Course,
}
sql_enum!(NotebookScope { Lesson => "lesson", Course => "course" });

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Notebook {
    pub scope: NotebookScope,
    pub scope_id: String,
    pub body_md: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub course_id: String,
    pub course_title: String,
    pub lesson_id: Option<String>,
    pub lesson_title: Option<String>,
    pub start_sec: Option<f64>,
    /// Text with matches wrapped in `<mark>…</mark>`; everything else is plain text.
    pub snippet: String,
    pub annotation_type: Option<AnnotationType>,
    /// Flashcard or material id for those groups.
    pub item_id: Option<String>,
    /// PDF page (1-based) for material hits.
    pub page: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub transcripts: Vec<SearchHit>,
    pub annotations: Vec<SearchHit>,
    pub lessons: Vec<SearchHit>,
    pub notebooks: Vec<SearchHit>,
    pub flashcards: Vec<SearchHit>,
    pub materials: Vec<SearchHit>,
}

// ---------------------------------------------------------------- Phase 3: downloads

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DownloadStatus {
    Queued,
    Running,
    Paused,
    Done,
    Error,
}
sql_enum!(DownloadStatus {
    Queued => "queued",
    Running => "running",
    Paused => "paused",
    Done => "done",
    Error => "error"
});

/// A download job joined with its lesson and course, for the Downloads screen and events.
#[derive(Debug, Clone, Serialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: String,
    pub lesson_id: String,
    pub lesson_title: String,
    pub course_id: String,
    pub course_title: String,
    pub course_slug: String,
    pub youtube_video_id: Option<String>,
    pub duration_sec: Option<i32>,
    pub status: DownloadStatus,
    pub quality: String,
    /// Resolution actually downloaded (set once done).
    pub local_quality: Option<String>,
    pub progress: f64,
    pub bytes_done: Option<f64>,
    pub bytes_total: Option<f64>,
    pub speed_bps: Option<f64>,
    pub eta_sec: Option<i32>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub attempts: i32,
    pub next_retry_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CourseStorage {
    pub course_id: String,
    pub course_title: String,
    pub bytes: f64,
    pub downloaded_lessons: i32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub total_bytes: f64,
    pub media_dir: String,
    pub courses: Vec<CourseStorage>,
}

// ---- Phase 4: progress and discipline ----

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    /// Local time, `HH:MM`.
    pub time: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct StudySettings {
    pub daily_goal_min: i32,
    /// `mon`..`sun`.
    pub study_days: Vec<String>,
    /// Watched fraction that auto-completes a video (0.5..=1.0).
    pub completion_threshold: f64,
    pub reminders: Vec<Reminder>,
    pub behind_nudge: bool,
}

impl Default for StudySettings {
    fn default() -> Self {
        Self {
            daily_goal_min: 45,
            study_days: ["mon", "tue", "wed", "thu", "fri"]
                .map(String::from)
                .to_vec(),
            completion_threshold: 0.9,
            reminders: vec![Reminder {
                time: "19:00".into(),
                enabled: false,
            }],
            behind_nudge: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ActivitySource {
    Notes,
    Review,
    Timer,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchSegment {
    pub from: f64,
    pub to: f64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WatchResult {
    pub watched_ratio: f64,
    /// True when this call auto-completed the lesson.
    pub completed_now: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlannedLesson {
    pub lesson_id: String,
    pub lesson_title: String,
    pub course_id: String,
    pub course_title: String,
    pub duration_sec: Option<i32>,
    pub planned_date: String,
    pub done: bool,
    pub doc_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CoursePlan {
    pub target: Option<crate::services::progress::schedule::Target>,
    pub items: Vec<PlannedLesson>,
    pub status: Option<crate::services::progress::schedule::PlanStatus>,
    pub finish_date: Option<String>,
    /// "outline" or "custom" (an applied AI plan's order).
    pub order: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DayActivity {
    /// Local date `YYYY-MM-DD`.
    pub date: String,
    pub minutes: f64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BehindCourse {
    pub course_id: String,
    pub course_title: String,
    pub lessons: i32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TodayView {
    pub goal_min: i32,
    pub minutes_today: f64,
    pub streak: crate::services::progress::streak::Streak,
    /// Planned for today plus anything overdue, oldest first.
    pub planned: Vec<PlannedLesson>,
    pub due_cards: i32,
    pub overdue_todos: i32,
    pub open_questions: i32,
    pub behind: Vec<BehindCourse>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenItem {
    pub id: String,
    pub lesson_id: String,
    pub lesson_title: String,
    pub title: String,
    pub start_sec: Option<f64>,
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WeeklyReview {
    pub week_start: String,
    pub days: Vec<DayActivity>,
    pub minutes: f64,
    pub lessons_completed: Vec<PlannedLesson>,
    pub open_questions: Vec<OpenItem>,
    pub overdue_todos: Vec<OpenItem>,
    pub next_week: Vec<PlannedLesson>,
}

// ---- Phase 5: flashcards, search, accounts ----

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Flashcard {
    pub id: String,
    pub lesson_id: Option<String>,
    pub lesson_title: Option<String>,
    pub course_title: Option<String>,
    pub source_sec: Option<f64>,
    pub source_material_id: Option<String>,
    pub source_page: Option<i32>,
    pub front_md: String,
    pub back_md: String,
    pub ease: f64,
    pub interval_days: i32,
    pub repetitions: i32,
    pub due_at: String,
    pub last_reviewed_at: Option<String>,
    pub ai_generated: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewFlashcard {
    #[specta(optional)]
    #[serde(default)]
    pub lesson_id: Option<String>,
    #[specta(optional)]
    #[serde(default)]
    pub source_sec: Option<f64>,
    #[specta(optional)]
    #[serde(default)]
    pub source_material_id: Option<String>,
    #[specta(optional)]
    #[serde(default)]
    pub source_page: Option<i32>,
    pub front_md: String,
    pub back_md: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationWithCourse {
    pub annotation: Annotation,
    pub lesson_title: String,
    pub course_id: String,
    pub course_title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SearchKind {
    Video,
    Playlist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DurationFilter {
    /// Under 4 minutes.
    Short,
    /// 4–20 minutes.
    Medium,
    /// Over 20 minutes.
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum UploadFilter {
    Week,
    Month,
    Year,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct YtSearchQuery {
    pub query: String,
    pub kind: SearchKind,
    #[specta(optional)]
    #[serde(default)]
    pub duration: Option<DurationFilter>,
    #[specta(optional)]
    #[serde(default)]
    pub upload: Option<UploadFilter>,
    #[serde(default)]
    pub cc_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct YtSearchResult {
    pub kind: SearchKind,
    /// Video id or playlist id.
    pub id: String,
    pub title: String,
    pub channel_title: Option<String>,
    pub thumbnail_url: Option<String>,
    pub duration_sec: Option<i32>,
    pub view_count: Option<f64>,
    pub license: Option<License>,
    pub published_at: Option<String>,
    pub item_count: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct YtSearchResponse {
    pub results: Vec<YtSearchResult>,
    pub source: crate::services::import::ImportSource,
    pub cached: bool,
    /// Why the fallback was used / which filters were ignored.
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QuotaInfo {
    pub day: String,
    pub used: i32,
    pub limit: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoogleAccount {
    /// An OAuth client id has been entered.
    pub configured: bool,
    pub client_id: Option<String>,
    pub signed_in: bool,
    pub channel_title: Option<String>,
}

// ---- AI course restructure ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StructureModule {
    pub title: String,
    #[specta(optional)]
    #[serde(default)]
    pub summary: Option<String>,
    pub lesson_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RestructureProposal {
    pub modules: Vec<StructureModule>,
    pub rationale: String,
    /// Lessons the model left out (placed in "Unsorted") or invented ids it returned (dropped).
    pub unsorted: i32,
    pub dropped: i32,
    pub model: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AiEstimate {
    /// Lessons / cards / questions the call covers.
    pub items: i32,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cost_usd: f64,
    pub model: String,
    pub month_spent_usd: f64,
    pub limit_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AiSettings {
    pub enabled: bool,
    pub model: String,
    /// Optional monthly spending limit in USD; AI calls are refused once reached.
    #[specta(optional)]
    pub monthly_limit_usd: Option<f64>,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "claude-opus-5-5".into(),
            monthly_limit_usd: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageRow {
    pub at: String,
    pub feature: String,
    pub model: String,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub cost_usd: f64,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AiFeatureUsage {
    pub feature: String,
    pub calls: i32,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageSummary {
    pub today_usd: f64,
    pub month_usd: f64,
    pub all_time_usd: f64,
    pub month_calls: i32,
    pub limit_usd: Option<f64>,
    pub by_feature: Vec<AiFeatureUsage>,
    pub recent: Vec<AiUsageRow>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AiStatus {
    pub settings: AiSettings,
    pub has_key: bool,
}

// ---- AI flashcards & quiz banks ----

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlashcardDraft {
    pub front_md: String,
    pub back_md: String,
    pub source_sec: Option<f64>,
    #[serde(default)]
    pub source_material_id: Option<String>,
    #[serde(default)]
    pub source_page: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum QuizKind {
    Mcq,
    Short,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum QuizScope {
    Lesson,
    Module,
    Course,
}

/// Where a hint points: a video moment or a PDF page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QuizHint {
    None,
    #[serde(rename_all = "camelCase")]
    Video {
        lesson_id: String,
        sec: f64,
    },
    #[serde(rename_all = "camelCase")]
    Pdf {
        material_id: String,
        page: i32,
        lesson_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QuizDraft {
    pub lesson_id: Option<String>,
    pub kind: QuizKind,
    pub prompt_md: String,
    pub choices: Vec<String>,
    pub answer_index: Option<i32>,
    pub answer_md: String,
    pub explanation_md: String,
    pub hint: QuizHint,
    pub difficulty: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QuizQuestion {
    pub id: String,
    pub course_id: String,
    #[serde(flatten)]
    pub draft: QuizDraft,
    pub lesson_title: Option<String>,
    pub ai_generated: bool,
    pub attempts: i32,
    pub correct: i32,
    pub last_correct: Option<bool>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PreviewVideo {
    pub id: String,
    pub title: String,
    pub duration_sec: Option<i32>,
    pub license: Option<License>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistPreview {
    pub playlist_id: String,
    pub title: String,
    pub description: Option<String>,
    pub channel_title: Option<String>,
    pub thumbnail_url: Option<String>,
    pub videos: Vec<PreviewVideo>,
    pub total_sec: i32,
    pub skipped: i32,
}

// ---- Providers & course materials ----

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Material {
    pub id: String,
    pub course_id: String,
    pub lesson_id: Option<String>,
    /// For solutions: the material this solves.
    pub solution_of: Option<String>,
    pub kind: String,
    pub title: String,
    pub section: Option<String>,
    pub description: Option<String>,
    /// Online page (e.g. the OCW resource page).
    pub source_url: Option<String>,
    pub file_url: Option<String>,
    pub local_path: Option<String>,
    pub file_type: Option<String>,
    pub file_size: Option<f64>,
    pub page_count: Option<i32>,
    pub has_html: bool,
    pub position: i32,
}

#[derive(Debug, Clone, Default)]
pub struct NewMaterial {
    pub course_id: String,
    pub lesson_id: Option<String>,
    pub solution_of: Option<String>,
    pub provider_uid: Option<String>,
    pub kind: String,
    pub title: String,
    pub section: Option<String>,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub file_url: Option<String>,
    pub local_path: Option<String>,
    pub file_type: Option<String>,
    pub file_size: Option<i64>,
    pub html: Option<String>,
    pub position: i32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CourseInfo {
    pub provider: String,
    pub provider_ref: Option<String>,
    /// Provider metadata (OCW: number, term, level, instructors, topics, license, url…).
    pub meta: Option<crate::services::providers::ocw::OcwMeta>,
    /// Metadata for the other providers (NPTEL, OpenStax, LibreTexts, Open Yale).
    pub provider_meta: Option<crate::services::providers::ProviderMeta>,
    pub syllabus_id: Option<String>,
    pub calendar_id: Option<String>,
    pub pages: Vec<Material>,
}

// ---- AI study plan ----

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanDraftItem {
    pub lesson_id: String,
    pub title: String,
    pub date: String,
    pub duration_sec: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanDraftWeek {
    pub focus: String,
    pub items: Vec<PlanDraftItem>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanProposal {
    pub weeks: Vec<PlanDraftWeek>,
    pub rationale: String,
    pub cost_usd: f64,
    pub model: String,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanAssignment {
    pub lesson_id: String,
    pub date: String,
}

// ---- Phase 6: AI study assist ----

/// What the learner is looking at: a lesson, optionally a moment or a document page.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AssistFocus {
    pub lesson_id: String,
    #[serde(default)]
    #[specta(optional)]
    pub sec: Option<f64>,
    #[serde(default)]
    #[specta(optional)]
    pub material_id: Option<String>,
    #[serde(default)]
    #[specta(optional)]
    pub page: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum AssistKind {
    /// Explain a moment or page (optional question).
    Explain,
    /// Answer a question annotation.
    Answer,
    /// Summarise a whole lesson.
    Summary,
}

/// A cited source in an AI answer, resolved to a real moment or page.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    /// "12:34" or "p.3".
    pub label: String,
    pub lesson_id: Option<String>,
    pub sec: Option<f64>,
    pub material_id: Option<String>,
    pub page: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AssistAnswer {
    /// Markdown with anchors replaced by readable labels.
    pub markdown: String,
    pub sources: Vec<SourceRef>,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceDraft {
    pub title: String,
    pub author: Option<String>,
    /// paper | article | book | video | other
    pub ref_type: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceSuggestions {
    pub items: Vec<ReferenceDraft>,
    pub cost_usd: f64,
}
