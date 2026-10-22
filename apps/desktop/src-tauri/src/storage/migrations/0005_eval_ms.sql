-- How long Ollama spent generating a reply's tokens, for its tokens/s.
-- Older replies keep NULL and show an estimate from their total duration.
ALTER TABLE messages ADD COLUMN eval_ms INTEGER;
