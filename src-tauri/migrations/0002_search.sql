-- Phase 2: full-text search over lessons and notebooks, transcript bookkeeping.

-- Self-contained FTS5 tables (lessons/notebooks have text ULID keys, so external-content
-- tables keyed by rowid would be fragile across VACUUM).
CREATE VIRTUAL TABLE lessons_fts USING fts5(lesson_id UNINDEXED, title, description, tokenize='porter unicode61');
CREATE TRIGGER lessons_fts_ai AFTER INSERT ON lessons BEGIN
  INSERT INTO lessons_fts(lesson_id, title, description) VALUES (new.id, new.title, coalesce(new.description, ''));
END;
CREATE TRIGGER lessons_fts_ad AFTER DELETE ON lessons BEGIN
  DELETE FROM lessons_fts WHERE lesson_id = old.id;
END;
CREATE TRIGGER lessons_fts_au AFTER UPDATE OF title, description ON lessons BEGIN
  DELETE FROM lessons_fts WHERE lesson_id = old.id;
  INSERT INTO lessons_fts(lesson_id, title, description) VALUES (new.id, new.title, coalesce(new.description, ''));
END;
INSERT INTO lessons_fts(lesson_id, title, description) SELECT id, title, coalesce(description, '') FROM lessons;

CREATE VIRTUAL TABLE notebooks_fts USING fts5(notebook_id UNINDEXED, body_md, tokenize='porter unicode61');
CREATE TRIGGER notebooks_fts_ai AFTER INSERT ON notebooks BEGIN
  INSERT INTO notebooks_fts(notebook_id, body_md) VALUES (new.id, new.body_md);
END;
CREATE TRIGGER notebooks_fts_ad AFTER DELETE ON notebooks BEGIN
  DELETE FROM notebooks_fts WHERE notebook_id = old.id;
END;
CREATE TRIGGER notebooks_fts_au AFTER UPDATE OF body_md ON notebooks BEGIN
  DELETE FROM notebooks_fts WHERE notebook_id = old.id;
  INSERT INTO notebooks_fts(notebook_id, body_md) VALUES (new.id, new.body_md);
END;
INSERT INTO notebooks_fts(notebook_id, body_md) SELECT id, body_md FROM notebooks;

-- Which transcript a lesson has: 'manual' | 'auto' | 'none' (checked, nothing available).
ALTER TABLE lessons ADD COLUMN transcript_source TEXT;
ALTER TABLE lessons ADD COLUMN transcript_lang TEXT;
ALTER TABLE lessons ADD COLUMN transcript_fetched_at TEXT;
