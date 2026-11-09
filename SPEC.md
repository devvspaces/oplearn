# Lectern: Build Spec

> This file is the source of truth for Lectern. Build in the phases in §12 and meet each phase's acceptance criteria before starting the next. Any implementation decision that deviates from this spec is recorded here in the same commit, with the reason (see §16, Deviations log).

---

## 1. Problem

Some of the best learning material online is long-form lectures on YouTube, such as the MIT Distributed Systems playlist and university courses. Watching it there is a poor way to study:

- **No offline access.** Videos can't be watched reliably without a connection.
- **No study tools.** You can't attach notes to a moment in a video, bookmark points, log questions to research, track to-dos, or collect references.
- **No structure.** A playlist is a flat list. It doesn't look or behave like a course (modules, lectures, readings, labs).
- **No accountability.** Nothing tracks completion, pace, or deadlines, so it's easy to drift and never finish.

## 2. Product vision

A desktop app that turns any YouTube playlist or set of videos into a **Coursera-style course** you own. Videos can be downloaded for offline viewing, and every course comes with:

- timestamped notes, bookmarks and research questions
- to-dos and references
- transcripts
- progress tracking and a study schedule that keeps you moving

**Working name:** Lectern

## 3. Goals and non-goals

### Goals

1. Search YouTube and import videos and playlists into a local library.
2. Organize content into **Courses → Modules → Lessons**. Lesson types are video, reading (link or PDF) and lab/assignment.
3. Download videos (plus subtitles, chapters and thumbnails) for offline playback, through a robust queue.
4. A study player with timestamped notes, bookmarks, questions, to-dos, references and a synced transcript.
5. Progress tracking and discipline features: completion, schedules, deadlines, streaks and reminders.
6. Full-text search across transcripts and notes.
7. Optional AI help ("explain this moment") using the user's own API key.
8. Local-first: all data lives on the user's machine and can be exported to Markdown or JSON at any time.

### Non-goals (v1)

- No multi-user support, cloud sync server, or mobile app.
- No redistribution, sharing, or re-uploading of downloaded videos.
- No circumventing DRM, paywalls, or YouTube Premium protections.

## 4. Legal and Terms of Service guardrails (must implement)

YouTube's Terms of Service generally prohibit downloading videos except through YouTube's own features, or where the content's license or owner allows it. The app must:

1. Support **Stream mode** for every feature. Playback through the official YouTube IFrame Player API must work with notes, bookmarks, progress and transcripts. **Download mode** is optional and set per course.
2. Show a one-time notice in the download settings: the user is responsible for downloading only content they have the right to download. Examples include Creative Commons–licensed lectures such as MIT OpenCourseWare, content they own, or content whose creator permits it.
3. Store and display each video's **license** where available (YouTube reports `creativeCommon` or `youtube`), and badge CC-licensed videos in search results and the course view.
4. Never strip attribution. Keep the channel, title, original URL and license with every lesson.
5. Ship **browser-cookie / account-authenticated downloading off by default**, behind an "Advanced" toggle with a clear warning (see §7.9).

## 5. Tech stack

| Concern | Choice | Notes |
| --- | --- | --- |
| App shell | **Tauri v2** (Rust core) | Small, fast, native webview. Rust owns the DB, child processes, secrets and background work. The renderer is UI only. |
| IPC | Tauri commands + **tauri-specta** | `src/bindings.ts` is generated from Rust signatures, so there is a single typed contract. Rust (serde) validates every input. |
| UI | **React 19 + TypeScript (strict)**, **Tailwind v4**, shadcn-style components on **Radix** primitives, lucide icons | |
| Routing | **TanStack Router** (file routes, hash history) | |
| State / data fetching | **TanStack Query** (server state) + **Zustand** (UI-only state) | |
| Database | **SQLite** via **rusqlite** (`bundled`, which includes **FTS5**) | Hand-rolled `user_version` migrator over versioned `.sql` files in `src-tauri/migrations/`. |
| Secrets | **keyring** crate | macOS Keychain, Windows Credential Manager, Secret Service |
| HTTP | **reqwest** (rustls) | YouTube Data API v3, release checks |
| Downloader | **yt-dlp** sidecar (`bundle.externalBin`), auto-updatable, + **ffmpeg** (Phase 3) | YouTube is never scraped directly. All invocations go through `services/ytdlp.rs`. Download formats are configurable. |
| Local playback | HTML5 `<video>` with a custom control layer (Phase 3) | Shares the `PlayerAdapter` interface with the YouTube IFrame player used in Stream mode. |
| YouTube metadata/search | **YouTube Data API v3** (API key and/or OAuth) | Fallback: `yt-dlp --flat-playlist --dump-single-json` and `ytsearch20:<query>`. |
| Notes editor | **TipTap** (Markdown in/out), Phase 2 | |
| Drag and drop | **dnd-kit** | Keyboard sensor for accessibility. |
| AI (optional) | Anthropic API (user-supplied key), configurable model. Pluggable providers (OpenAI, Gemini). | |
| Tests | `cargo test` (Rust services, repos, parsers, algorithms), **Vitest** (pure TS), **Playwright** smoke (renderer with mocked IPC) | |
| Packaging | `tauri build`: `.dmg`/`.app`, `.msi`/NSIS, `.deb`/`.AppImage`/`.rpm`. GitHub Actions matrix in Phase 6. | |

Package manager: **pnpm**. Lint and format: **ESLint + Prettier**, `cargo fmt` and `clippy`.

## 6. Architecture

```
┌──────────────────────── Renderer (React, webview) ───────────────┐
│  Today │ Library │ Course │ Player+Notes │ Search │ Planner │ Settings │
└───────────────▲──────────────────────────────────────────────────┘
                │ typed IPC (tauri-specta generated bindings; serde-validated)
┌───────────────┴──────────── Rust core (src-tauri) ───────────────┐
│ db (rusqlite, migrations, repos) │ Download manager (queue, workers) │
│ youtube.rs (Data API)            │ ytdlp.rs (sidecar runner, updater)│
│ transcripts (VTT→rows)           │ scheduler (reminders, streaks)    │
│ export (MD/JSON)                 │ ai (optional)                     │
│ secrets / auth (keychain, Google OAuth loopback)                     │
└──────────────────────────────────────────────────────────────────┘
          │                         │
   ~/Lectern/library.db     ~/Lectern/media/<courseSlug>/<lessonId>/
                              video.mp4, subs.<lang>.vtt, thumb.jpg, info.json
```

- All commands are registered in `src-tauri/src/lib.rs` (`specta_builder`). Their argument and return types are exported to `src/bindings.ts` (`pnpm bindings`).
- The renderer never touches the filesystem or spawns processes. Capabilities allow only `core:default`, `opener:default` and the dialog permissions.
- The data directory (default `~/Lectern`) is configurable, so a library can live on an external drive. Its location is stored in `<OS config dir>/com.lectern.app/bootstrap.json`.
- Graceful shutdown: on window close or app quit, Rust emits `flush-request`. The renderer flushes pending writes (such as the playback position) and calls `flush_complete`. A 1.5 s timeout guarantees exit.

## 7. Features in detail

### 7.1 Library and courses

- **Import a playlist URL.** This creates a Course with one Module ("Lectures") holding one Lesson per video, in playlist order. Title, description, channel and thumbnail are fetched.
- **Import a single video URL** into a chosen course or module.
- **Create a course manually.** Set the title, description, instructor, source URL, cover image and tags.
- **Module editor.** Drag and drop to reorder lessons and move them between modules, and split a flat playlist into modules (e.g. "Week 1: MapReduce & RPC").
- **Lesson types:**
  - `video`: a YouTube video, with an optional local file.
  - `reading`: a URL or a local PDF, opened in the app (pdf.js) or in the browser.
  - `lab`: a free-form assignment with a description, links, a checklist and a due date.
  - `quiz`: generated or hand-written flashcards (see §7.8).
- **Course resources:** syllabus link, course website, GitHub repo, papers.
- **Re-sync a playlist** to pick up newly added videos without losing notes or progress.
- **Library views:**
  - a grid of course cards showing the cover, a progress ring, the next lesson and due status
  - filters: in progress, not started, completed, archived
  - sorting

### 7.2 YouTube search

- A search bar in the app with filters for duration, upload date, Creative Commons only, and type (video or playlist).
- Results show the thumbnail, title, channel, duration, view count and license badge, with "Add to course" and "Create course from playlist" actions.
- **Quota awareness.** A Data API `search.list` call costs 100 units out of the default 10,000 per day. The app must:
  - cache results for 24 hours
  - show the estimated remaining quota in Settings
  - fall back to yt-dlp search automatically when the quota runs out or no API key is set

### 7.3 Downloads

- A per-course toggle sets the course to **Stream** or **Download**. Individual lessons can override it.
- A **download queue** with:
  - configurable concurrency (default 2)
  - pause, resume and cancel
  - automatic retries with exponential backoff
  - resumption of partial downloads after the app restarts
- **Quality presets:** 360p, 480p, 720p (default) and 1080p. Prefer H.264/AAC MP4 for universal playback, and show an estimated size before downloading.
- **Fetched alongside each video:**
  - subtitles: manual first, auto-generated second, in user-selected languages (default English)
  - chapters
  - thumbnail
  - `info.json`
- **Storage management:** total disk usage per course, "delete local files but keep notes", and a configurable storage location.
- **yt-dlp updater:** on startup, check for a newer release (at most once a day) and install it into `<data_dir>/bin/`, because the signed app bundle is read-only. Show the yt-dlp version in Settings.
- **Errors** appear in a Downloads panel with human-readable messages (age-restricted, private, removed, region-locked, network) and a "Retry" button.
- **Invariant:** a lesson's local file path is recorded only after ffmpeg has finished merging and the file passes a size/duration sanity check.

### 7.4 Study player

- The player plays local files (Download mode) or the YouTube embed (Stream mode) through one shared `PlayerAdapter` interface: `play/pause/seek/getTime/setRate/onTimeUpdate`.
- **Controls:**
  - speed from 0.5× to 3× in 0.25 steps, remembered per course
  - ±5 s and ±10 s skip
  - frame step
  - volume
  - fullscreen, theatre mode and picture-in-picture
- **A–B loop** to replay a hard section repeatedly.
- **Chapters** on the timeline, from YouTube chapters or user-defined.
- **Timeline markers** for notes, bookmarks, questions and to-dos, colour-coded. Clicking a marker seeks to it.
- **Resume** from the last position, rewinding 5 seconds for context.
- **Transcript panel:**
  - synced and auto-scrolling, with the current line highlighted
  - clicking a line seeks to it
  - search within the transcript
  - select text to turn it into a note or question
- **Keyboard-first shortcuts** (all configurable):

  | Key | Action |
  | --- | --- |
  | `Space` | Play/pause |
  | `J` / `L` | −10 s / +10 s |
  | `←` / `→` | −5 s / +5 s |
  | `N` | New note at current time |
  | `B` | Bookmark |
  | `Q` | Question |
  | `T` | To-do |
  | `[` / `]` | Speed down/up |
  | `A` | Toggle A–B loop |
  | `F` | Fullscreen |
  | `/` | Focus transcript search |

- Creating a note **pauses playback** by default (configurable) and stamps the current time.
- **Frame capture:** grab the current frame (local files only) and insert it into a note as an image. Useful for whiteboard diagrams.

### 7.5 Annotations: notes, bookmarks, questions, to-dos, references

Every annotation belongs to a lesson, can have a timestamp (or a time range), and supports tags.

| Type | Purpose | Extra fields |
| --- | --- | --- |
| **Note** | Rich-text (Markdown) thoughts at a moment | body, optional image(s) |
| **Bookmark** | Quick "come back here" marker | label |
| **Question** | Something not understood, to research | status: `open` / `researching` / `answered`; answer (Markdown); links |
| **To-do** | Action item, e.g. "implement Raft leader election" | status, due date, priority |
| **Reference** | Paper, article, repo or book mentioned | title, URL, type, citation text |

- **Side panel** in the player, filtered to the current lesson, with tabs for All, Notes, Bookmarks, Questions, To-dos and References. Annotations near the current timestamp are highlighted.
- **Lesson notebook:** a free-form Markdown document per lesson, in addition to the timestamped items.
- **Course notebook:** every annotation in a course, grouped by module and lesson. Timestamps are clickable links that open the player at that moment.
- **Global views:**
  - "Open Questions" across all courses
  - "To-dos" with due dates, also shown in the Planner
  - a "References" library with export to BibTeX
- **Research flow for questions:**
  - one click opens a web search for the question text plus the course title
  - an "Ask AI" button (§7.7)
  - save the answer and mark the question answered
- **Export:**
  - one Markdown file per course (Obsidian-friendly, with optional `[[links]]`), with each timestamp written as `[12:34](https://youtu.be/<id>?t=754)`
  - full JSON export and import for backup

### 7.6 Progress and discipline

- **Lesson completion:**
  - A video is complete automatically at ≥ 90% watched (configurable). Watched coverage is tracked as merged watched intervals, not just the last position, so skipping to the end doesn't count.
  - Lessons can also be marked complete or incomplete manually.
  - Readings and labs are completed manually or by ticking every checklist item.
- **Course progress:** percentage of lessons and of total minutes, a module-level breakdown, and a "Continue: next lesson" button.
- **Study plan per course:**
  - The user sets a **target finish date** _or_ a **weekly pace** (e.g. 3 lectures/week, or 5 hours/week).
  - The app generates a schedule of lessons per day or week across the user's chosen study days.
  - A status indicator shows **On track / Behind by N lessons / Ahead**, and the user can "rebalance" the remaining schedule.
- **Daily goal** in study minutes (default 45), shown as a ring on the home screen.
- **Streaks:** consecutive days meeting the daily goal, with a "freeze" allowance (1 per week) so a streak survives one missed day.
- **Activity heatmap** (GitHub-style) of study minutes per day.
- **Session timer / Pomodoro (optional):** a focus timer that logs a study session. Only actual playback time and active note-taking time count toward the goal.
- **Reminders:** desktop notifications at user-chosen times ("You planned Lecture 6 today, 0/45 min done"), plus a nudge when a course falls behind schedule.
- **Weekly review screen:** minutes studied, lessons completed, open questions, overdue to-dos, and next week's plan.

### 7.7 AI assist (optional, off by default)

The user enables AI in Settings and enters an API key, which is stored in the OS keychain.

- **Explain this moment:** sends the transcript window (±90 seconds around the current time), the lesson title and the user's question. Returns an explanation the user can save as the answer to a Question.
- **Summarize lesson:** a Markdown summary with key concepts and timestamps.
- **Generate flashcards** from the transcript and the user's notes (see §7.8).
- **Suggest references:** papers and concepts mentioned in the lecture, added as Reference annotations for the user to confirm.
- AI features must clearly mark AI-generated content, show the estimated token cost before sending, and never send data without an explicit user action.

### 7.8 Flashcards and spaced repetition

- Each card has a front and a back, plus an optional source lesson and timestamp so you can jump back to where the concept was taught.
- Create cards from a note, from a transcript selection, or with AI (§7.7).
- **SM-2** scheduling with Again / Hard / Good / Easy buttons. Due cards form a daily review queue on the home screen, and review time counts toward study minutes.

### 7.9 Accounts and sign-in

- **Google sign-in (OAuth 2.0, installed-app / loopback flow)** with the `youtube.readonly` scope, used to:
  - list the user's own playlists and subscriptions and import them as courses
  - attribute API quota to the user's own project, if they supply a client ID
- Tokens are stored in the OS keychain, never in SQLite or plain files. Provide sign-out and revoke.
- **Limitations to show in the UI:**
  - the YouTube API does not expose Watch Later or watch history
  - **YouTube Premium offline downloads cannot be used by third-party apps**, so Premium makes no difference to this app's downloads
- **Advanced (off by default):** use yt-dlp's `--cookies-from-browser` to access videos visible only to the signed-in account, such as unlisted course videos. Show a warning that this may violate YouTube's Terms and can put the account at risk. Never ask for or store a Google password.

### 7.10 Search across everything

- SQLite **FTS5** indexes cover transcripts (per cue), annotations, lesson titles and descriptions, and notebooks.
- Global search (`Ctrl/Cmd+K`) groups results by type. A transcript or annotation result opens the player at that timestamp.

### 7.11 Settings

Settings covers:

- data directory
- download quality, concurrency and subtitle languages
- default mode (Stream/Download)
- completion threshold
- daily goal and study days
- reminder times
- keyboard shortcuts
- YouTube API key and Google account
- AI key and model
- theme (light/dark/system)
- export/import
- yt-dlp version and update

## 8. Data model (SQLite)

All IDs are ULIDs (text), except where an integer key is needed for FTS5 (noted below). Every table has `created_at` and `updated_at` (ISO 8601 UTC). The authoritative schema is `src-tauri/migrations/0001_init.sql`.

```text
courses(id, title, slug, description, instructor, source_url, youtube_playlist_id NULL,
        cover_path, tags_json, mode: 'stream'|'download', status: 'active'|'archived'|'completed',
        target_finish_date NULL, pace_type NULL: 'date'|'lessons_per_week'|'minutes_per_week',
        pace_value NULL, playback_rate REAL DEFAULT 1.0)

modules(id, course_id FK, title, position INT)

lessons(id, module_id FK, position INT, type: 'video'|'reading'|'lab'|'quiz',
        title, description, url NULL, youtube_video_id NULL, channel_title NULL,
        license NULL: 'youtube'|'creativeCommon', duration_sec NULL,
        local_video_path NULL, thumbnail_path NULL, mode_override NULL,
        completed_at NULL, due_date NULL, checklist_json NULL)

chapters(id, lesson_id FK, start_sec, end_sec NULL, title, source: 'youtube'|'user')

transcript_cues(id INTEGER PK, lesson_id FK, lang, start_sec REAL, end_sec REAL, text)
  -- plus FTS5 transcript_fts(text, content='transcript_cues'), kept in sync by triggers

watch_progress(lesson_id PK FK, last_position_sec REAL, watched_intervals_json, watched_ratio REAL)

annotations(id, rid INTEGER UNIQUE, lesson_id FK, type: 'note'|'bookmark'|'question'|'todo'|'reference',
            start_sec REAL NULL, end_sec REAL NULL, title NULL, body_md NULL,
            status NULL, priority NULL, due_date NULL, url NULL, ref_type NULL,
            answer_md NULL, tags_json, ai_generated BOOL DEFAULT 0)
  -- plus FTS5 annotations_fts(title, body_md, answer_md, content_rowid='rid'), kept in sync by triggers

attachments(id, annotation_id FK, kind: 'image'|'file', path)
notebooks(id, scope: 'lesson'|'course', scope_id, body_md)  UNIQUE(scope, scope_id)
resources(id, course_id FK, title, url, kind: 'syllabus'|'site'|'repo'|'paper'|'other')
study_sessions(id, lesson_id NULL FK, started_at, ended_at, active_seconds INT, source: 'playback'|'notes'|'review'|'timer')
plan_items(id, course_id FK, lesson_id FK, planned_date, done BOOL)
flashcards(id, lesson_id NULL FK, source_sec NULL, front_md, back_md,
           ease REAL DEFAULT 2.5, interval_days INT DEFAULT 0, repetitions INT DEFAULT 0,
           due_at, ai_generated BOOL)
downloads(id, lesson_id FK, status: 'queued'|'running'|'paused'|'done'|'error',
          quality, progress REAL, bytes_total NULL, error_code NULL, error_message NULL, attempts INT)
settings(key PK, value_json)
```

Every schema change is a new numbered migration file; released migrations are never edited. `pnpm seed` imports a demo course from a small public Creative Commons playlist. The default is MIT 6.824; override it with `LECTERN_SEED_PLAYLIST`.

## 9. Key algorithms

- **Watched coverage:** on each `timeupdate` (throttled to 1 s):
  - if playback is continuous (gap < 2 s at normal rate), extend the current interval `[start, now]`; otherwise start a new one
  - merge overlapping intervals
  - persist every 10 s and on pause/close
  - `watched_ratio = sum(merged intervals) / duration`
- **Study minutes:** count playback seconds only while the window is focused and the video is playing, plus seconds with keyboard activity in the notes editor (idle timeout 60 s).
- **Schedule generation:**
  - inputs: the remaining lessons (with durations), the study days, and either a target date or a pace
  - distribute lessons greedily so each study day's minutes stay ≤ max(daily goal, longest single lesson)
  - recompute on "rebalance"
  - status = planned lessons up to today versus completed lessons
- **Streak:** a day is a local calendar day, and it counts if active minutes ≥ daily goal. At most one freeze per ISO week is applied automatically to bridge a single missed day.
- **SM-2:** the standard algorithm, minimum ease 1.3, mapping Again=0, Hard=3, Good=4, Easy=5.

## 10. UI screens

1. **Home / Today:** daily goal ring, streak, "Continue learning" cards, today's planned lessons, due flashcards, overdue to-dos, open-question count.
2. **Library:** course grid with progress and filters, plus a "New course" menu (from playlist URL, from search, manual).
3. **Course page:**
   - header: cover, instructor, progress, plan status
   - module accordion with lessons: type icon, duration, ✓ completed, ↓ downloaded/streaming badge, due date
   - Resources, Notebook and Plan tabs
4. **Player / Study view:**
   - the player, on the left or top
   - annotation and transcript panels on the right, tabbed and resizable
   - lesson navigation: prev/next, mark complete
5. **Search (YouTube):** results with filters and add actions.
6. **Planner:** a week/month calendar of plan items and to-dos, the heatmap, and the weekly review.
7. **Questions / To-dos / References:** global list views with filters.
8. **Flashcards review.**
9. **Downloads panel:** queue, progress, errors, disk usage.
10. **Settings.**

Design: clean, calm and study-focused, dark mode by default. The layout should feel like Coursera's course outline combined with a note-taking app. Everything must be usable from the keyboard.

## 11. Non-functional requirements

- **Performance:**
  - a library of 50 courses / 2,000 lessons / 100k transcript cues stays responsive
  - global search returns in < 150 ms
  - the app starts in < 3 s
- **Reliability:**
  - no data loss on crash
  - SQLite in WAL mode
  - writes debounced but flushed on quit (the `flush-request` handshake)
  - automatic daily DB backup, keeping the last 7
- **Security:**
  - a strict CSP in `tauri.conf.json`
  - minimal capabilities
  - every command input validated by serde and the repository layer
  - secrets only in the OS keychain
  - all paths canonicalized under the data dir before any file operation
- **Privacy:** no telemetry. Network calls go only to:
  - YouTube/Google APIs and the YouTube embed
  - yt-dlp release checks
  - the AI provider, only when the user triggers it
- **Accessibility:** WCAG AA contrast, focus-visible states, ARIA labels on player controls.
- **Cross-platform:** Windows, macOS and Linux. Per-platform yt-dlp (and later ffmpeg) binaries are fetched by `scripts/fetch-binaries.ts` with SHA-256 verification and bundled as sidecars.

## 12. Build phases and acceptance criteria

### Phase 0: Scaffold

- Tauri v2 + React + TS + Tailwind, SQLite with the initial migration, a typed IPC contract (tauri-specta), ESLint/Prettier, Vitest, `cargo test`, a Playwright smoke test, and a `README.md` with setup steps.
- ✅ Done when:
  - `pnpm app` opens the app
  - `pnpm test` passes
  - `pnpm app:build` produces an installer for the current OS

### Phase 1: Library and Stream mode MVP

- Scope:
  - import a playlist or video URL (Data API if a key is set, otherwise yt-dlp `--flat-playlist`)
  - course, module and lesson CRUD with drag-and-drop
  - YouTube IFrame player via `PlayerAdapter`
  - resume position
  - manual completion
- ✅ Done when:
  - importing a 20-video playlist creates a course in order
  - a lecture plays, the app is closed and reopened, and it resumes
  - lessons can be reordered into modules

### Phase 2: Annotations and transcripts

- Scope:
  - notes, bookmarks, questions, to-dos and references, with timestamps and timeline markers
  - shortcuts
  - lesson and course notebooks
  - subtitle fetching (yt-dlp `--write-subs --write-auto-subs --skip-download` in Stream mode), parsed into cues
  - a synced transcript panel and FTS search
  - Markdown export
- ✅ Done when:
  - pressing `N` mid-video creates a note at the right time, and clicking its marker seeks there
  - the transcript highlights the current line
  - global search finds a word spoken in a lecture and opens the player at that moment
  - course export produces valid Markdown with working `youtu.be` timestamp links

### Phase 3: Downloads

- Scope:
  - queue, quality presets, and subtitles/chapters/thumbnails
  - pause/resume/retry and a persistent queue
  - disk usage
  - the yt-dlp updater and the ToS notice
  - local playback through the same `PlayerAdapter`, including frame capture
- ✅ Done when:
  - a playlist downloads at 720p with 2 concurrent jobs
  - killing the app mid-download and restarting resumes the downloads
  - a downloaded lecture plays offline with Wi-Fi disabled, and all annotations still work

### Phase 4: Progress and discipline

- Scope:
  - watched-interval coverage, auto-completion and study sessions
  - daily goal, streaks with freeze, and the heatmap
  - study plans with on-track status and rebalancing
  - reminders, the weekly review and the Home/Today screen
- ✅ Done when:
  - skipping to the end of a video does not mark it complete, and watching 90% does
  - setting a target date generates a plan, and missing a day shows "Behind by N" until rebalanced
  - unit tests cover the coverage, schedule, streak and SM-2 logic

### Phase 5: Search, accounts, flashcards

- Scope:
  - YouTube search with filters, the CC badge, and quota tracking with fallback
  - Google OAuth (loopback) to import the user's playlists, with tokens in the keychain
  - flashcards with SM-2 review
- ✅ Done when:
  - searching "distributed systems lecture" with the CC-only filter returns badged results that can be added to a course in one click
  - signing in lists the user's playlists
  - a due card appears in Today, and its schedule updates after review

### Phase 6: AI assist and polish ✅

- Scope:
  - ✅ "Explain this moment" (E, or "Explain this page" on documents), "Ask AI" on questions, lesson summary into the notebook, and reference suggestions; plus the earlier flashcard, quiz, plan and restructure features
  - ✅ cost preview, monthly budget and AI-content labels
  - ✅ daily database backups (7 kept), manual backup and restore, full JSON export/import
  - ✅ packaging for all three OSes: GitHub Actions matrix (`.github/workflows/build.yml`), unsigned
  - ✅ an onboarding flow: choose data folder, optional keys, first course import
- ✅ Done when:
  - with AI disabled, no network calls go to the AI provider
  - with AI enabled, "Explain" returns an answer that can be saved to a question and is marked AI-generated
  - a fresh install walks through onboarding to a first imported course

### Phase 7: Providers (open course libraries beyond YouTube)

Every provider implements the same checklist: **search**, **preview** (contents and size before import), **import** (modules, lessons, materials, transcripts, metadata), **attribution** (license and a link back to the source on every item), and **re-sync** (add new items without touching the user's work). Code lives in `src-tauri/src/services/providers/`.

| Provider | License | Status | Notes |
| --- | --- | --- | --- |
| YouTube playlists | per video (CC filter) | ✅ | Data API with yt-dlp fallback |
| **MIT OpenCourseWare** | CC BY-NC-SA 4.0 | ✅ | Search via the MIT Learn API. Import from the course's offline ZIP: syllabus, calendar → modules, lecture videos (YouTube) with caption transcripts, lecture notes, readings, problem sets and exams with linked solutions, recitations. PDFs are searchable and used as AI context. |
| **NPTEL** | CC BY-NC-SA (as stated on nptel.ac.in) | ✅ | Catalogue and courses from SvelteKit `__data.json` (devalue). Weeks → modules, lectures → YouTube video lessons with English subtitles as transcripts; "Web" courses become reading lessons that open online. About: overview, weekly topics, books, instructor. |
| **OpenStax** | per book (CMS `license_name`/`license_url`) | ✅ | CMS catalogue (retired books hidden) + archive book tree/page JSON. Chapters → modules, sections → reading lessons with local HTML (figures stored locally, MathML kept), searchable and used as AI context. Optional full-book PDF. Preface → About. |
| **LibreTexts** | per book (`license` + `licenseversion` tag) | ✅ | Commons catalogue + TOC. The official per-page PDF ZIP gives one PDF per page, so each page is a reading lesson with its own (indexed) PDF. |
| **Open Yale Courses** | CC BY-NC-SA 3.0 US | ✅ | HTML pages (no API), 1 request/s. Sessions grouped into parts ending at each exam; lectures are YouTube lessons with Yale's chapter times and full transcript (a searchable material); handouts, problem sets and exams (with solutions) are downloaded. |
| ~~Khan Academy~~ | | dropped | Its API is gone (410) and its terms forbid scraping. Khan's YouTube playlists import through the YouTube tab. |

All providers but MIT OCW share `provider_search` / `provider_preview` / `provider_import` (`commands/providers.rs`); each module turns provider data into a `build::Plan`, and `build::write` creates the course. Re-sync is not built yet for any provider.

- ✅ Done when (NPTEL, OpenStax, LibreTexts, Open Yale):
  - each can be searched, previewed (outline, counts, license) and imported from Discover
  - imported courses show the license and a link back in About
  - section text, transcripts and PDFs are found from ⌘K and used by the AI features
  - `cargo test --lib -- --ignored live_provider --nocapture` imports a trimmed real course from each site

- ✅ Done when (MIT OCW):
  - searching "mathematics for computer science" and importing 6.042J gives calendar modules, transcripts available immediately, problem sets with solutions behind a reveal, and an About tab with the syllabus and instructors
  - a PDF page is found from ⌘K and opens at that page
  - the AI plan uses the syllabus and calendar

## 13. Repository layout

```
lectern/
  SPEC.md  README.md  CLAUDE.md
  package.json  pnpm-lock.yaml  vite.config.ts  playwright.config.ts
  scripts/fetch-binaries.ts            # yt-dlp (+ffmpeg later) per target triple, sha256-verified
  src-tauri/
    Cargo.toml  tauri.conf.json  capabilities/default.json
    binaries/                          # sidecars (gitignored, fetched by script)
    migrations/0001_init.sql
    src/
      lib.rs (command registry, specta export, shutdown hooks)  main.rs  bin/seed.rs
      error.rs  paths.rs  state.rs  shutdown.rs
      db/ (mod.rs migrator, models.rs, repo/{courses,modules,lessons,progress,settings,import}.rs)
      services/ (youtube.rs, ytdlp.rs, import.rs, secrets.rs; later downloads, transcripts,
                 progress, planner, srs, export, auth, ai, backup, notifications)
      commands/ (library.rs, import.rs, player.rs, settings.rs; one file per domain)
  src/
    bindings.ts (generated)  main.tsx  index.css
    app/routes/ (file routes)  app/routeTree.gen.ts (generated)
    features/ (library, course, player, settings; later annotations, transcript, search,
               planner, flashcards, downloads, onboarding)
    components/ui/
    lib/ (api.ts, player-adapter.ts, youtube-iframe.ts, hotkeys.ts, flush.ts, time.ts)
    stores/ (zustand UI state)
  tests/ (unit/, e2e/)
```

## 14. Working rules

1. Build phase by phase, and confirm each phase's acceptance criteria (run the app and tests) before moving on. Commit at the end of each phase with a clear message.
2. Keep business logic (coverage, planner, streaks, SRS, VTT parsing, schedule, layout moves) in pure, unit-tested modules: Rust for anything that touches data, and TS for UI-only logic. No logic in React components that can't be tested.
3. Never call YouTube's private endpoints or scrape HTML. Use the Data API, the IFrame API, or yt-dlp.
4. Wrap every yt-dlp/ffmpeg invocation in `services/ytdlp.rs`, with typed options, progress parsing (`--newline --progress-template`), timeouts and cancellation.
5. Prefer boring, well-maintained libraries. Ask before adding a large dependency not listed in §5.
6. Keep this spec as the source of truth. Record any deviation in §16 in the same commit.
7. Maintain `CLAUDE.md` with build/test commands, architecture notes and gotchas discovered along the way.

## 15. Future ideas (post-v1)

- Import from other sources: Vimeo, university sites (e.g. MIT OCW zip downloads), local video folders.
- Sync the library between devices via a user-chosen folder (Syncthing/Dropbox), using CRDT-merged annotations.
- A mobile companion app for flashcard review and offline viewing.
- Shareable "course packs": an export of structure and notes **without** video files, which re-download from source.
- Automatic module splitting from lecture titles and durations.
- Lab runner integration: link a lab to a local Git repo and show test results, e.g. for the MIT 6.5840 Raft labs.
- Obsidian/Notion two-way sync of notes.

## 16. Deviations log

| Date | Change | Why |
| --- | --- | --- |
| 2026-11-07 | **Electron → Tauri v2 with a Rust core.** better-sqlite3/Drizzle → rusqlite + SQL migrations. keytar → `keyring`. The zod IPC contract → tauri-specta generated bindings. electron-vite → Vite. | Smaller and faster app. One language owns the data, processes and secrets. Types are generated from the Rust source instead of being maintained by hand. |
| 2026-11-07 | Migrations use a ~40-line `user_version` migrator instead of `rusqlite_migration`. | `rusqlite_migration` pinned an older rusqlite/libsqlite3-sys, which conflicts with the `links = "sqlite3"` rule. |
| 2026-11-07 | `transcript_cues.id` is an INTEGER rowid, and `annotations` gets an extra `rid INTEGER UNIQUE`. | FTS5 external-content tables need an integer rowid. |
| 2026-11-07 | Lesson drag-and-drop persists through a single `apply_layout(course, [{moduleId, lessonIds}])` command instead of separate `move_lesson` and `reorder_lessons` commands. | One transactional, validated write covers every reorder or move. It can't leave positions half-updated. |
| 2026-11-07 | In Stream mode, thumbnails are remote `i.ytimg.com` URLs; local thumbnail caching moves to Phase 3. | It isn't needed for streaming. Downloads cache thumbnails anyway. |
| 2026-11-07 | Theme lives in renderer storage (zustand persist), not in the `settings` table. | It is a pure UI preference, and the renderer must read it before first paint. |
| 2026-11-07 | Playwright tests run the built renderer in Chromium with mocked IPC, not the native app. | Tauri has no WebDriver on macOS. Native flows are checked manually per phase. |
| 2026-11-07 | Stream-mode YouTube player runs in a bridge page served by Rust from `http://127.0.0.1:<random port>/player` and is driven with `postMessage`. The app itself stays on the Tauri origin. | Packaged apps are served from `tauri://localhost`, so YouTube gets no valid referrer and fails with error 153. Serving the whole app from localhost would expose IPC to whatever process binds that port; the bridge page has no IPC access. |
| 2026-11-07 | Lessons and notebooks get self-contained FTS5 tables (`lessons_fts`, `notebooks_fts`) keyed by an UNINDEXED text id, kept in sync by triggers (migration 0002). | Their primary keys are text ULIDs; external-content FTS keyed by implicit rowid could drift after VACUUM. |
| 2026-11-07 | Subtitles are fetched by first probing the available tracks (`-J`), then downloading exactly one: a manual track in a preferred language, otherwise the original-language auto track (`<lang>-orig`). Translated auto tracks are a last resort. | Requesting several tracks or YouTube's machine-translated `en` track reliably returns HTTP 429 on the timedtext endpoint. |
| 2026-11-07 | Keyboard shortcuts are fixed in Phase 2; making them configurable moves to the Settings work. Global Questions/To-dos/References views and BibTeX export move to Phase 4–5. "Ask AI" is shown disabled until Phase 6. | Keeps Phase 2 focused on its acceptance criteria. |
| 2026-11-07 | Bulk transcript fetching is sequential with a 1.5 s pause between lessons. | Avoids YouTube rate limits. |
| 2026-11-07 | No JavaScript runtime (deno) is bundled for yt-dlp. | Spike: without one, YouTube still serves H.264 720p DASH formats at full speed (~2 MB/s measured, not throttled to real time). Saves 40–100 MB per platform. If YouTube changes this, the daily yt-dlp auto-update plus an optional runtime sidecar are the fallback. |
| 2026-11-07 | ffmpeg/ffprobe sidecars come from ffmpeg.martin-riedl.de 9.0.2 (macOS and Linux, pinned, per-file sha256) and from yt-dlp/FFmpeg-Builds `latest` (Windows; not pinnable, verified against its checksums file). | These are checksum-published static builds for each platform. |
| 2026-11-07 | Frame capture uses `ffmpeg -ss <t> -frames:v 1` on the downloaded file, not a `<canvas>` grab. Images are stored under `media/<course>/<lesson>/frames/` and referenced in Markdown as `lectern-attachment:<data-relative path>`. Export copies them to `<name>.assets/`. | Asset-protocol video is cross-origin to the app, so canvas reads would be tainted. Relative links keep notes portable when the data folder moves. |
| 2026-11-07 | Download size before downloading is an estimate (duration × preset bitrate), not a per-video probe. | Probing each video takes 30–60 s. The real size appears as soon as a download starts. |
| 2026-11-07 | The thumbnail for downloaded lessons is saved as `video.jpg`. | yt-dlp ignores the `thumbnail:` output template after `--convert-thumbnails`. |
| 2026-11-07 | Dev server port is 1430 (not Tauri's default 1420). | It avoids clashing with other local Tauri projects. |
| 2026-11-07 | Downloaded videos are served by the loopback server (`/media/<token>/…`, Range support) instead of Tauri's asset protocol. Downloads add `-movflags +faststart`. | WKWebView played `asset://` video without audio. The random per-launch token and media-folder path check keep other local pages out. |
| 2026-11-08 | `lessons.local_quality` records the **actual** downloaded height from ffprobe (snapped to 360p/720p/…), not the requested preset. | YouTube may not offer the requested height; the chip should tell the truth. |
| 2026-11-08 | Watched coverage is sent by the renderer as segments (`record_watch`), merged and judged in Rust (`services/progress/intervals.rs`). Study minutes are wall-clock seconds while playing with the window focused (or PiP), plus notes keystroke activity (60 s idle), plus review time. | Rust owns the algorithm; the renderer only sees time updates. |
| 2026-11-08 | Planner shows a week view (with prev/next) plus a 12-month heatmap; no month calendar. | The heatmap covers the long view; a month grid duplicated it. |
| 2026-11-08 | Reminders fire from a Rust loop via `tauri-plugin-notification` (5-minute window after the set time, once per day), plus an evening (18:00) behind-schedule nudge. | Works without the window being open in front. |
| 2026-11-08 | Data API quota day is approximated as UTC−8 (ignores DST). Searches fall back to yt-dlp when there's no key, the estimate is spent, or the API returns quotaExceeded; the fallback can't filter by license, duration or date and says so. | No tz database dependency for an estimate. |
| 2026-11-08 | Google sign-in uses the user's own OAuth "Desktop app" client (id in settings, secret + refresh token in the keychain). | Shipping a client id needs Google app verification for `youtube.readonly`. |
| 2026-11-08 | Flashcards are not added to FTS search. | §7.10 doesn't list them; Review and the lesson Cards tab cover lookup. |
| 2026-11-08 | Global Questions / To-dos / References views delivered in Phase 5 (`/notes/$kind`). | Deferred from Phase 2. |
| 2026-11-08 | **AI course restructure** pulled forward from Phase 6: course ⋯ → Restructure with AI. Raw HTTP to the Messages API (no Rust SDK), `claude-opus-5-5` by default (Sonnet 5.5 selectable), structured output via `output_config.format`, `fallbacks: "default"`. Cost is estimated locally before sending; the proposal is validated (every lesson exactly once) and editable before applying; one-step undo. | User request. |
| 2026-11-08 | **Find on YouTube** is now **Discover**, with a YouTube \| MIT OpenCourseWare switcher. YouTube playlists can be previewed (video list, total length, CC count) before importing; previews are cached for 24 h. | User request. |
| 2026-11-08 | Planner has a Month view in addition to Week; the month cells are tinted by minutes studied. | User request. |
| 2026-11-08 | Flashcards and course-material PDF pages are in ⌘K search (`flashcards_fts`, `material_pages_fts`). Supersedes the earlier "flashcards not in FTS" row. Quiz questions are not indexed. | User request. |
| 2026-11-08 | AI usage ledger (`ai_usage`) with today / month / all-time totals per feature and an optional monthly USD cap checked before every call. All AI features go through `services::ai::run`. | User request. |
| 2026-11-08 | AI flashcards and AI quiz banks (MCQ + self-graded short answer). Hints cite anchors (`T:L1@754`, `P:M1#3`) resolved to a video moment or PDF page; drafts are reviewed before saving. Wrong answers can become flashcards. | User request. |
| 2026-11-08 | OCW documents are **`reading` lessons linked to a local material**, not a new `document` lesson type: adding a type means rebuilding `lessons`, and SQLite would cascade-delete its child rows during the swap. Reading lessons now always open in-app (PDF viewer or an Open link button). Notes on documents have no timestamp. Re-confirmed 2026-11-09: kept by design, with documents labelled everywhere by their material kind (`Lesson.docKind`: Problem set, Lecture notes, Exam…). | Data safety. |
| 2026-11-08 | OCW videos have no duration in the metadata, so lessons take the last caption cue's end time. Imported transcripts come from the course ZIP's VTT files, with no YouTube fetch. | Avoids 429s and is instant. |
| 2026-11-08 | ~~Release builds use `panic = "unwind"` to contain `pdf-extract` panics.~~ Superseded 2026-11-09: PDF text is extracted by the app binary in helper mode (`--extract-pdf-text`, `services::pdf_text`), at most 3 at a time with a 20 s limit each; a crash or hang only skips that PDF. Release builds are back to `panic = "abort"`. | Robustness. |
| 2026-11-08 | AI study plan ("Plan with AI"): weeks from Claude (using the syllabus, calendar and materials when present), placed on study days by `schedule::schedule_weeks`. Applying sets a date target. Since 2026-11-09 it also sets `courses.plan_order = 'custom'`, so Rebalance keeps the plan's order (lessons added later go last) until the user picks "Use outline order"; a restructure resets it to outline. | User request. |
| 2026-11-09 | Own PDF viewer (pdf.js 4.10, `features/materials/PdfViewer.tsx`) instead of WebKit's iframe viewer, which can't report the current page. Notes and cards anchor to (material, page) (`annotations.material_id/page`, `flashcards.source_material_id/source_page`, migration 0007); AI context renders page notes as `[N:M1#3]`. pdf.js fetches from the loopback `/media` route, which now answers CORS for the app's origins only. If the import skipped a PDF's text, the viewer extracts it on first open (`set_material_pages`, fills only empty materials). | User decision. |
| 2026-11-09 | AI study assist: Explain (±90 s of transcript and notes around the moment, or ±1 page of a document), Ask AI (answers a question annotation), lesson summary (appended to the lesson notebook under "AI summary (generated …)"), reference suggestions. Answers cite anchors, which are rendered as "(12:34, p.3)" plus clickable sources; references carry **no URLs** (each has a web-search action) so none are invented. | Avoid hallucinated links. |
| 2026-11-09 | Backups are `VACUUM INTO` snapshots in `<data>/backups/` (daily kept 7 days; manual and pre-restore safety backups kept). Restore and JSON import always take a safety backup first. The JSON export covers every user table plus PDF page text, **not media files** (videos, PDFs, attachments stay in the data folder). Caches (`yt_search_cache`, `quota_usage`) are skipped. | Size and portability. |
| 2026-11-09 | Onboarding is a `/welcome` route shown while settings `onboarded` is false; libraries with courses are marked onboarded automatically. | Existing users never see it. |
| 2026-11-09 | CI builds are **unsigned** (no Apple notarisation or Windows code signing yet). Tests run on Linux and macOS; Windows is build-only for now. Windows x64 ffmpeg is pinned to the gyan.dev 9.0.2 essentials build (hash recorded in `fetch-binaries.ts`). | Signing needs certificates. |
| 2026-11-09 | Providers other than MIT OCW use **one generic command trio** (`provider_search/preview/import`) and a shared `build::Plan` writer; MIT OCW keeps its own commands but now shares the progress type, relocation and PDF indexing. Provider HTML (OCW included) goes through an **ammonia allow-list sanitiser** (MathML allowed, Content MathML dropped) instead of the regex one. | Less code per provider; safer HTML. |
| 2026-11-09 | Images in imported pages are **stored locally** and referenced as `lectern-media:<path under media>`; `get_material_html` turns that into the current loopback URL (its token changes every launch). Images that fail or exceed 5 MB become their alt text, so pages never load remote content. The app CSP's `img-src` gained `http://127.0.0.1:*` (stored images) and `https://assets.openstax.org` (book covers in search). | Offline pages; no remote requests. |
| 2026-11-09 | LibreTexts imports use the official **per-page PDF ZIP**, not page HTML: the spike found the "pages" download is one PDF per page, and the page HTML API (`/@api`) is disallowed by robots.txt. This is about 7× the size of the single book PDF (86 MB vs 12 MB for the sampled book), but every page becomes its own reading lesson with exact boundaries, page anchors and search, with no schema change. | Exact structure without scraping. |
| 2026-11-09 | NPTEL's license is shown as **CC BY-NC-SA (as stated by NPTEL)**: the site footer says BY-NC-SA (earlier research noted BY-SA). NPTEL's newer English subtitles are Google Drive file ids, fetched via `drive.google.com/uc?export=download`; older ones are `gs://` paths, fetched from storage.googleapis.com. | Accuracy. |
| 2026-11-09 | **Khan Academy dropped** from Phase 7: its public API returns 410 and its terms forbid scraping. The YouTube tab notes that Khan's playlists import there. | No legitimate data source. |
| 2026-11-09 | Releases: pushing a `v*` tag runs `release.yml` (version check → tests → 4-platform build) and uploads the installers to a **draft** GitHub release that is published by hand. macOS builds are **ad-hoc signed** (`bundle.macOS.signingIdentity: "-"`) and **not notarised** (no Apple Developer account), so users open them with right-click → Open or `xattr -dr com.apple.quarantine`. CI runs on `master`; the Intel macOS runner is `macos-15-intel`. | No signing certificates. |
