-- Projects: a name and instructions every chat in the project is sent. A
-- conversation outside any project has a NULL `project_id`; deleting a
-- project deletes its conversations and, by their cascade, their messages.
CREATE TABLE projects (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, instructions TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
ALTER TABLE conversations ADD COLUMN project_id TEXT
  REFERENCES projects(id) ON DELETE CASCADE;
CREATE INDEX conversations_project_idx ON conversations(project_id);
