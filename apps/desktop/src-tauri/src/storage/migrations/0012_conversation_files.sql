-- The per-chat Files switch: a project chat's system message carries passages
-- of the project's files. On for every existing conversation.
ALTER TABLE conversations ADD COLUMN files INTEGER NOT NULL DEFAULT 1;
