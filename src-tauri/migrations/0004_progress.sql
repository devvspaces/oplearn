-- Downloads: the resolution actually downloaded (from ffprobe), e.g. '720p'.
ALTER TABLE lessons ADD COLUMN local_quality TEXT;

-- Phase 4: progress and discipline.
CREATE INDEX idx_sessions_lesson ON study_sessions(lesson_id);
CREATE INDEX idx_plan_course ON plan_items(course_id);
CREATE UNIQUE INDEX idx_plan_lesson ON plan_items(lesson_id);
CREATE INDEX idx_lessons_completed ON lessons(completed_at);
