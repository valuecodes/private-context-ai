-- Reasoning text from thinking models, shown collapsed and never sent back
-- to the model, and how long the model thought before answering.
ALTER TABLE messages ADD COLUMN thinking TEXT;
ALTER TABLE messages ADD COLUMN thinking_ms INTEGER;
