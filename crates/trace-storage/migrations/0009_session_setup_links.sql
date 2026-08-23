CREATE TABLE session_setup_links (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    setup_id TEXT NOT NULL REFERENCES setup_library(id) ON DELETE CASCADE,
    relationship TEXT NOT NULL CHECK (relationship IN ('user_confirmed', 'package_confirmed')),
    confirmed_at TEXT NOT NULL
);

CREATE INDEX session_setup_links_setup_idx
    ON session_setup_links(setup_id, session_id);
