ALTER TABLE sessions ADD COLUMN user_title TEXT;

CREATE TABLE session_tags (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    tag TEXT NOT NULL COLLATE NOCASE,
    PRIMARY KEY (session_id, tag)
);

CREATE INDEX session_tags_tag_idx ON session_tags(tag, session_id);
