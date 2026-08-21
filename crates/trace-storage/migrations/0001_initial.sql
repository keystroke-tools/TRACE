CREATE TABLE simulators (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    version TEXT
);

CREATE TABLE tracks (
    id TEXT PRIMARY KEY,
    simulator_id TEXT NOT NULL REFERENCES simulators(id),
    source_track_id TEXT NOT NULL,
    layout_id TEXT NOT NULL DEFAULT '',
    display_name TEXT NOT NULL,
    UNIQUE (simulator_id, source_track_id, layout_id)
);

CREATE TABLE cars (
    id TEXT PRIMARY KEY,
    simulator_id TEXT NOT NULL REFERENCES simulators(id),
    source_car_id TEXT NOT NULL,
    display_name TEXT NOT NULL,
    UNIQUE (simulator_id, source_car_id)
);

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    simulator_id TEXT NOT NULL REFERENCES simulators(id),
    track_id TEXT REFERENCES tracks(id),
    car_id TEXT REFERENCES cars(id),
    started_at TEXT NOT NULL,
    ended_at TEXT,
    session_type TEXT,
    source_kind TEXT NOT NULL,
    source_metadata_json TEXT NOT NULL DEFAULT '{}',
    conditions_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE telemetry_blobs (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL UNIQUE,
    format TEXT NOT NULL,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    sample_count INTEGER NOT NULL CHECK (sample_count >= 0),
    sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
    created_at TEXT NOT NULL
);

CREATE TABLE laps (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    lap_index INTEGER NOT NULL CHECK (lap_index >= 0),
    started_offset_ns INTEGER,
    duration_ns INTEGER,
    validity TEXT NOT NULL,
    validity_reason TEXT,
    telemetry_blob_id TEXT REFERENCES telemetry_blobs(id),
    sample_start INTEGER,
    sample_count INTEGER,
    distance_m REAL,
    is_personal_best INTEGER NOT NULL DEFAULT 0 CHECK (is_personal_best IN (0, 1)),
    UNIQUE (session_id, lap_index)
);

CREATE TABLE track_geometries (
    id TEXT PRIMARY KEY,
    track_id TEXT NOT NULL REFERENCES tracks(id) ON DELETE CASCADE,
    blob_path TEXT NOT NULL,
    source TEXT NOT NULL,
    confidence REAL NOT NULL CHECK (confidence BETWEEN 0.0 AND 1.0),
    algorithm_version INTEGER NOT NULL,
    sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
    created_at TEXT NOT NULL
);

CREATE TABLE setup_snapshots (
    id TEXT PRIMARY KEY,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    source TEXT NOT NULL,
    source_hash BLOB,
    captured_at TEXT NOT NULL,
    normalized_json TEXT NOT NULL,
    original_blob_path TEXT
);

CREATE TABLE setup_revisions (
    id TEXT PRIMARY KEY,
    setup_snapshot_id TEXT NOT NULL REFERENCES setup_snapshots(id) ON DELETE CASCADE,
    parent_id TEXT REFERENCES setup_revisions(id),
    label TEXT,
    notes TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE analysis_cache (
    id TEXT PRIMARY KEY,
    reference_lap_id TEXT NOT NULL REFERENCES laps(id) ON DELETE CASCADE,
    comparison_lap_id TEXT NOT NULL REFERENCES laps(id) ON DELETE CASCADE,
    algorithm_version INTEGER NOT NULL,
    input_hash BLOB NOT NULL,
    result_blob_path TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (reference_lap_id, comparison_lap_id, algorithm_version, input_hash)
);

CREATE TABLE favourites (
    lap_id TEXT PRIMARY KEY REFERENCES laps(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL
);

CREATE INDEX sessions_started_at_idx ON sessions(started_at DESC);
CREATE INDEX sessions_track_car_idx ON sessions(track_id, car_id);
CREATE INDEX laps_session_idx ON laps(session_id, lap_index);
CREATE INDEX telemetry_blobs_session_idx ON telemetry_blobs(session_id);
