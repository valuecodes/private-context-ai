-- Regenerate and edit-and-resend: the rows a running turn replaces, marked
-- with that turn's pending assistant id. Deleted when its reply completes,
-- unmarked again when it fails or is stopped.
ALTER TABLE messages ADD COLUMN superseded_by TEXT;
