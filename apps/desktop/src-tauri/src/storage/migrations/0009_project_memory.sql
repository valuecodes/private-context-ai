-- Project memory: one model-written summary per saved chat in a project,
-- sent to the project's other chats. `through_message_id` and
-- `covered_count` are the checkpoint: the history rows up to that message,
-- and how many there were. `revision` is bumped on every write, so a summary
-- that ran while the entry changed is discarded.
CREATE TABLE project_memories (
  conversation_id TEXT PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
  project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  content TEXT NOT NULL,
  through_message_id TEXT NOT NULL,
  covered_count INTEGER NOT NULL,
  user_edited INTEGER NOT NULL DEFAULT 0,
  rebuilt INTEGER NOT NULL DEFAULT 0,
  truncated INTEGER NOT NULL DEFAULT 0,
  model TEXT,
  revision INTEGER NOT NULL DEFAULT 0,
  updated_at TEXT NOT NULL);
CREATE INDEX project_memories_project_idx ON project_memories(project_id);
ALTER TABLE projects ADD COLUMN auto_memory INTEGER NOT NULL DEFAULT 1;
