-- Flashcards in global search (text-keyed, so a self-contained FTS table like lessons_fts).
CREATE VIRTUAL TABLE flashcards_fts USING fts5(card_id UNINDEXED, front_md, back_md, tokenize='porter unicode61');
CREATE TRIGGER flashcards_fts_ai AFTER INSERT ON flashcards BEGIN
  INSERT INTO flashcards_fts(card_id, front_md, back_md) VALUES (new.id, new.front_md, new.back_md);
END;
CREATE TRIGGER flashcards_fts_ad AFTER DELETE ON flashcards BEGIN
  DELETE FROM flashcards_fts WHERE card_id = old.id;
END;
CREATE TRIGGER flashcards_fts_au AFTER UPDATE OF front_md, back_md ON flashcards BEGIN
  DELETE FROM flashcards_fts WHERE card_id = old.id;
  INSERT INTO flashcards_fts(card_id, front_md, back_md) VALUES (new.id, new.front_md, new.back_md);
END;
INSERT INTO flashcards_fts(card_id, front_md, back_md) SELECT id, front_md, back_md FROM flashcards;

-- AI usage ledger (cost tracking and the monthly budget).
CREATE TABLE ai_usage (
  id TEXT PRIMARY KEY,
  at TEXT NOT NULL,
  feature TEXT NOT NULL,
  model TEXT NOT NULL,
  course_id TEXT REFERENCES courses(id) ON DELETE SET NULL,
  lesson_id TEXT REFERENCES lessons(id) ON DELETE SET NULL,
  input_tokens INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_usd REAL NOT NULL DEFAULT 0,
  ok INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_ai_usage_at ON ai_usage(at);

-- Quiz banks.
CREATE TABLE quiz_questions (
  id TEXT PRIMARY KEY,
  course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
  lesson_id TEXT REFERENCES lessons(id) ON DELETE SET NULL,
  kind TEXT NOT NULL CHECK (kind IN ('mcq','short')),
  prompt_md TEXT NOT NULL,
  choices_json TEXT,
  answer_index INTEGER,
  answer_md TEXT NOT NULL DEFAULT '',
  explanation_md TEXT NOT NULL DEFAULT '',
  hint_kind TEXT NOT NULL DEFAULT 'none' CHECK (hint_kind IN ('video','pdf','none')),
  hint_sec REAL,
  hint_material_id TEXT,
  hint_page INTEGER,
  difficulty TEXT,
  ai_generated INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE INDEX idx_quiz_course ON quiz_questions(course_id);
CREATE INDEX idx_quiz_lesson ON quiz_questions(lesson_id);
CREATE TABLE quiz_attempts (
  id TEXT PRIMARY KEY,
  question_id TEXT NOT NULL REFERENCES quiz_questions(id) ON DELETE CASCADE,
  correct INTEGER NOT NULL,
  answered_at TEXT NOT NULL
);
CREATE INDEX idx_quiz_attempts_q ON quiz_attempts(question_id);

-- Providers (YouTube, MIT OpenCourseWare, …).
ALTER TABLE courses ADD COLUMN provider TEXT NOT NULL DEFAULT 'youtube';
ALTER TABLE courses ADD COLUMN provider_ref TEXT;
CREATE TABLE course_meta (
  course_id TEXT PRIMARY KEY REFERENCES courses(id) ON DELETE CASCADE,
  json TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

-- Course materials: PDFs and pages imported from a provider (local copy + online source).
CREATE TABLE materials (
  id TEXT PRIMARY KEY,
  course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
  lesson_id TEXT REFERENCES lessons(id) ON DELETE SET NULL,
  solution_of TEXT REFERENCES materials(id) ON DELETE SET NULL,
  provider_uid TEXT,
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  section TEXT,
  description TEXT,
  source_url TEXT,
  file_url TEXT,
  local_path TEXT,
  file_type TEXT,
  file_size INTEGER,
  page_count INTEGER,
  html TEXT,
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);
CREATE INDEX idx_materials_course ON materials(course_id, kind);
CREATE INDEX idx_materials_lesson ON materials(lesson_id);
CREATE UNIQUE INDEX idx_materials_uid ON materials(course_id, provider_uid);
CREATE VIRTUAL TABLE material_pages_fts USING fts5(material_id UNINDEXED, page UNINDEXED, text, tokenize='porter unicode61');

-- Page anchors for notes on documents (start_sec stays NULL).
ALTER TABLE annotations ADD COLUMN page INTEGER;
