CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE saved_comparisons (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
    reference_lap_id TEXT NOT NULL REFERENCES laps(id) ON DELETE CASCADE,
    analysed_lap_id TEXT NOT NULL REFERENCES laps(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    CHECK (reference_lap_id != analysed_lap_id)
);

CREATE INDEX saved_comparisons_created_at_idx
    ON saved_comparisons(created_at DESC);
