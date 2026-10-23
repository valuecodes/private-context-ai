-- The per-chat Charts switch, off for every existing conversation, and the
-- `render_chart` calls of a reply as JSON (`[{offset, spec, error}]`); NULL
-- when a reply has no chart.
ALTER TABLE conversations ADD COLUMN charts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE messages ADD COLUMN charts TEXT;
