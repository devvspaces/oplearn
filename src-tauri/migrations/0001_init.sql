-- Lectern initial schema (SPEC.md §8). IDs are ULIDs; timestamps are ISO 8601 UTC.

CREATE TABLE courses (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  slug TEXT NOT NULL UNIQUE,
  description TEXT,
  instructor TEXT,
  source_url TEXT,
  youtube_playlist_id TEXT,
  cover_path TEXT,
  tags_json TEXT NOT NULL DEFAULT '[]',
  mode TEXT NOT NULL DEFAULT 'stream' CHECK (mode IN ('stream','download')),
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','archived','completed')),
  target_finish_date TEXT,
  pace_type TEXT CHECK (pace_type IN ('date','lessons_per_week','minutes_per_week')),
  pace_value REAL,
  playback_rate REAL NOT NULL DEFAULT 1.0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_courses_playlist ON courses(youtube_playlist_id);

CREATE TABLE modules (
  id TEXT PRIMARY KEY,
  course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  position INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_modules_course ON modules(course_id, position);

CREATE TABLE lessons (
  id TEXT PRIMARY KEY,
  module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  type TEXT NOT NULL CHECK (type IN ('video','reading','lab','quiz')),
  title TEXT NOT NULL,
  description TEXT,
  url TEXT,
  youtube_video_id TEXT,
  channel_title TEXT,
  license TEXT CHECK (license IN ('youtube','creativeCommon')),
  duration_sec INTEGER,
  local_video_path TEXT,
  thumbnail_path TEXT,
  mode_override TEXT CHECK (mode_override IN ('stream','download')),
  completed_at TEXT,
  due_date TEXT,
  checklist_json TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_lessons_module ON lessons(module_id, position);
CREATE INDEX idx_lessons_video ON lessons(youtube_video_id);

CREATE TABLE chapters (
  id TEXT PRIMARY KEY,
  lesson_id TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
  start_sec REAL NOT NULL,
  end_sec REAL,
  title TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('youtube','user')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_chapters_lesson ON chapters(lesson_id, start_sec);

CREATE TABLE transcript_cues (
  id INTEGER PRIMARY KEY, -- integer rowid so FTS5 external content can reference it
  lesson_id TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
  lang TEXT NOT NULL,
  start_sec REAL NOT NULL,
  end_sec REAL NOT NULL,
  text TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_cues_lesson ON transcript_cues(lesson_id, lang, start_sec);

CREATE VIRTUAL TABLE transcript_fts USING fts5(
  text, content='transcript_cues', content_rowid='id', tokenize='porter unicode61'
);
CREATE TRIGGER transcript_cues_ai AFTER INSERT ON transcript_cues BEGIN
  INSERT INTO transcript_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER transcript_cues_ad AFTER DELETE ON transcript_cues BEGIN
  INSERT INTO transcript_fts(transcript_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
CREATE TRIGGER transcript_cues_au AFTER UPDATE ON transcript_cues BEGIN
  INSERT INTO transcript_fts(transcript_fts, rowid, text) VALUES ('delete', old.id, old.text);
  INSERT INTO transcript_fts(rowid, text) VALUES (new.id, new.text);
END;

CREATE TABLE watch_progress (
  lesson_id TEXT PRIMARY KEY REFERENCES lessons(id) ON DELETE CASCADE,
  last_position_sec REAL NOT NULL DEFAULT 0,
  watched_intervals_json TEXT NOT NULL DEFAULT '[]',
  watched_ratio REAL NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE annotations (
  id TEXT PRIMARY KEY,
  rid INTEGER NOT NULL UNIQUE, -- stable integer key for FTS5 external content
  lesson_id TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
  type TEXT NOT NULL CHECK (type IN ('note','bookmark','question','todo','reference')),
  start_sec REAL,
  end_sec REAL,
  title TEXT,
  body_md TEXT,
  status TEXT,
  priority TEXT,
  due_date TEXT,
  url TEXT,
  ref_type TEXT,
  answer_md TEXT,
  tags_json TEXT NOT NULL DEFAULT '[]',
  ai_generated INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_annotations_lesson ON annotations(lesson_id, start_sec);
CREATE INDEX idx_annotations_type ON annotations(type, status);

CREATE VIRTUAL TABLE annotations_fts USING fts5(
  title, body_md, answer_md, content='annotations', content_rowid='rid', tokenize='porter unicode61'
);
CREATE TRIGGER annotations_ai AFTER INSERT ON annotations BEGIN
  INSERT INTO annotations_fts(rowid, title, body_md, answer_md)
  VALUES (new.rid, new.title, new.body_md, new.answer_md);
END;
CREATE TRIGGER annotations_ad AFTER DELETE ON annotations BEGIN
  INSERT INTO annotations_fts(annotations_fts, rowid, title, body_md, answer_md)
  VALUES ('delete', old.rid, old.title, old.body_md, old.answer_md);
END;
CREATE TRIGGER annotations_au AFTER UPDATE ON annotations BEGIN
  INSERT INTO annotations_fts(annotations_fts, rowid, title, body_md, answer_md)
  VALUES ('delete', old.rid, old.title, old.body_md, old.answer_md);
  INSERT INTO annotations_fts(rowid, title, body_md, answer_md)
  VALUES (new.rid, new.title, new.body_md, new.answer_md);
END;

CREATE TABLE attachments (
  id TEXT PRIMARY KEY,
  annotation_id TEXT NOT NULL REFERENCES annotations(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('image','file')),
  path TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE notebooks (
  id TEXT PRIMARY KEY,
  scope TEXT NOT NULL CHECK (scope IN ('lesson','course')),
  scope_id TEXT NOT NULL,
  body_md TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (scope, scope_id)
);

CREATE TABLE resources (
  id TEXT PRIMARY KEY,
  course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
  title TEXT NOT NULL,
  url TEXT NOT NULL,
  kind TEXT NOT NULL CHECK (kind IN ('syllabus','site','repo','paper','other')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE study_sessions (
  id TEXT PRIMARY KEY,
  lesson_id TEXT REFERENCES lessons(id) ON DELETE SET NULL,
  started_at TEXT NOT NULL,
  ended_at TEXT,
  active_seconds INTEGER NOT NULL DEFAULT 0,
  source TEXT NOT NULL CHECK (source IN ('playback','notes','review','timer')),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_sessions_started ON study_sessions(started_at);

CREATE TABLE plan_items (
  id TEXT PRIMARY KEY,
  course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
  lesson_id TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
  planned_date TEXT NOT NULL,
  done INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_plan_date ON plan_items(planned_date);

CREATE TABLE flashcards (
  id TEXT PRIMARY KEY,
  lesson_id TEXT REFERENCES lessons(id) ON DELETE SET NULL,
  source_sec REAL,
  front_md TEXT NOT NULL,
  back_md TEXT NOT NULL,
  ease REAL NOT NULL DEFAULT 2.5,
  interval_days INTEGER NOT NULL DEFAULT 0,
  repetitions INTEGER NOT NULL DEFAULT 0,
  due_at TEXT NOT NULL,
  ai_generated INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_flashcards_due ON flashcards(due_at);

CREATE TABLE downloads (
  id TEXT PRIMARY KEY,
  lesson_id TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
  status TEXT NOT NULL CHECK (status IN ('queued','running','paused','done','error')),
  quality TEXT NOT NULL,
  progress REAL NOT NULL DEFAULT 0,
  bytes_total INTEGER,
  error_code TEXT,
  error_message TEXT,
  attempts INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_downloads_status ON downloads(status);

CREATE TABLE settings (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
