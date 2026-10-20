CREATE TABLE conversations (
  id TEXT PRIMARY KEY, title TEXT NOT NULL, model TEXT NOT NULL,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  role TEXT NOT NULL CHECK (role IN ('system','user','assistant')),
  content TEXT NOT NULL, created_at TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending','complete','cancelled','error')),
  prompt_tokens INTEGER, completion_tokens INTEGER, duration_ms INTEGER);
CREATE INDEX messages_conversation_idx ON messages(conversation_id, created_at);
CREATE TABLE audit_events (
  id INTEGER PRIMARY KEY, at TEXT NOT NULL, kind TEXT NOT NULL,
  host TEXT, endpoint TEXT, model TEXT, status TEXT NOT NULL,
  prompt_tokens INTEGER, completion_tokens INTEGER, duration_ms INTEGER,
  detail TEXT);
CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
