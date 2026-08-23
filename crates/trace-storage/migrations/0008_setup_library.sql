CREATE TABLE setup_library (
    id TEXT PRIMARY KEY,
    simulator_key TEXT NOT NULL,
    source_car_id TEXT NOT NULL,
    source_track_id TEXT NOT NULL,
    layout_id TEXT NOT NULL DEFAULT '',
    name TEXT NOT NULL,
    installed_path TEXT NOT NULL,
    source_archive TEXT,
    content_sha256 BLOB NOT NULL CHECK (length(content_sha256) = 32),
    imported_at TEXT NOT NULL,
    UNIQUE (simulator_key, installed_path)
);

CREATE INDEX setup_library_compatibility_idx
    ON setup_library(simulator_key, source_track_id, layout_id, source_car_id, imported_at DESC);
