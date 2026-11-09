-- Page anchors on PDFs: a note or card can point at (material, page). `annotations.page` exists since 0006.
ALTER TABLE annotations ADD COLUMN material_id TEXT REFERENCES materials(id) ON DELETE SET NULL;
ALTER TABLE flashcards ADD COLUMN source_material_id TEXT REFERENCES materials(id) ON DELETE SET NULL;
ALTER TABLE flashcards ADD COLUMN source_page INTEGER;

-- 'custom' once an AI plan is applied: rebalancing keeps the plan's order instead of the outline's.
ALTER TABLE courses ADD COLUMN plan_order TEXT NOT NULL DEFAULT 'outline' CHECK (plan_order IN ('outline','custom'));
