-- Per-conversation settings, applied from the next send. Thinking is opt-in:
-- every existing conversation starts with it off.
ALTER TABLE conversations ADD COLUMN system_prompt TEXT NOT NULL DEFAULT '';
ALTER TABLE conversations ADD COLUMN temperature REAL;
ALTER TABLE conversations ADD COLUMN num_ctx INTEGER;
ALTER TABLE conversations ADD COLUMN think TEXT NOT NULL DEFAULT 'off';
