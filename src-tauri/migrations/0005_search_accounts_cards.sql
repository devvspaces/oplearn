-- Phase 5: YouTube search cache and Data API quota accounting.
CREATE TABLE yt_search_cache (
  key TEXT PRIMARY KEY,
  json TEXT NOT NULL,
  fetched_at TEXT NOT NULL
);

-- Units spent per Pacific-time day (the Data API quota resets at midnight PT).
CREATE TABLE quota_usage (
  day TEXT PRIMARY KEY,
  units INTEGER NOT NULL DEFAULT 0
);

-- Flashcards: review history for stats and the "reviewed today" count.
ALTER TABLE flashcards ADD COLUMN last_reviewed_at TEXT;
CREATE INDEX idx_flashcards_lesson ON flashcards(lesson_id);
