-- The model that wrote each reply; NULL for replies saved before it was
-- recorded, and for prompts.
ALTER TABLE messages ADD COLUMN model TEXT;
-- Set on a compared prompt and its candidate replies until one is picked or
-- the comparison is discarded; NULL otherwise.
ALTER TABLE messages ADD COLUMN compare_group TEXT;
