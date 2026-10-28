-- The file tool calls of a reply as JSON (`[{tool, target, hits, error}]`),
-- shown under it and never sent back to the model; NULL when it made none.
ALTER TABLE messages ADD COLUMN file_lookups TEXT;
