CREATE TABLE setup_identity_mappings (
    simulator_key TEXT NOT NULL,
    setup_car_id TEXT NOT NULL,
    setup_track_id TEXT NOT NULL,
    setup_layout_id TEXT NOT NULL DEFAULT '',
    session_car_id TEXT NOT NULL,
    session_track_id TEXT NOT NULL,
    session_layout_id TEXT NOT NULL DEFAULT '',
    confirmed_at TEXT NOT NULL,
    PRIMARY KEY (simulator_key, setup_car_id, setup_track_id, setup_layout_id)
);

CREATE INDEX setup_identity_mappings_session_idx
    ON setup_identity_mappings(
        simulator_key,
        session_track_id,
        session_layout_id,
        session_car_id
    );
