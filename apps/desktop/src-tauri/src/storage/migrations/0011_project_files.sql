-- Project files: copies of text files the user added to a project, split into
-- chunks of about 40 lines for full-text search. Deleting a project deletes
-- its files, and deleting a file its chunks; the triggers keep the external
-- content FTS index in step, cascades included.
CREATE TABLE project_files (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  content TEXT NOT NULL,
  bytes INTEGER NOT NULL,
  line_count INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (project_id, name));
CREATE TABLE file_chunks (
  id INTEGER PRIMARY KEY,
  file_id TEXT NOT NULL REFERENCES project_files(id) ON DELETE CASCADE,
  start_line INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  text TEXT NOT NULL);
CREATE INDEX file_chunks_file_idx ON file_chunks(file_id);
CREATE VIRTUAL TABLE file_chunks_fts USING fts5(
  text, content='file_chunks', content_rowid='id',
  tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER file_chunks_ai AFTER INSERT ON file_chunks BEGIN
  INSERT INTO file_chunks_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER file_chunks_ad AFTER DELETE ON file_chunks BEGIN
  INSERT INTO file_chunks_fts(file_chunks_fts, rowid, text)
    VALUES ('delete', old.id, old.text);
END;
