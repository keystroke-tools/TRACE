ALTER TABLE sessions ADD COLUMN user_driver TEXT;
ALTER TABLE sessions ADD COLUMN ownership TEXT NOT NULL DEFAULT 'unknown'
    CHECK (ownership IN ('mine', 'other', 'unknown'));
