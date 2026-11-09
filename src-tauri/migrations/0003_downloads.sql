-- Phase 3: download queue bookkeeping.
ALTER TABLE downloads ADD COLUMN bytes_done INTEGER;
ALTER TABLE downloads ADD COLUMN speed_bps REAL;
ALTER TABLE downloads ADD COLUMN eta_sec INTEGER;
ALTER TABLE downloads ADD COLUMN next_retry_at TEXT;
ALTER TABLE downloads ADD COLUMN started_at TEXT;
ALTER TABLE downloads ADD COLUMN finished_at TEXT;
-- One download row per lesson.
CREATE UNIQUE INDEX idx_downloads_lesson ON downloads(lesson_id);
