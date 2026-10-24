-- Ollama's own timings of a finished chat or memory request, for per-model
-- speed stats. Older rows keep NULL and are left out of the stats.
ALTER TABLE audit_events ADD COLUMN eval_ms INTEGER;
ALTER TABLE audit_events ADD COLUMN prompt_eval_ms INTEGER;
ALTER TABLE audit_events ADD COLUMN load_ms INTEGER;
