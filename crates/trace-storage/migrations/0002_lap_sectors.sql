CREATE TABLE lap_sectors (
    lap_id TEXT NOT NULL REFERENCES laps(id) ON DELETE CASCADE,
    sector_index INTEGER NOT NULL CHECK (sector_index > 0),
    duration_ns INTEGER NOT NULL CHECK (duration_ns > 0),
    PRIMARY KEY (lap_id, sector_index)
);

CREATE INDEX lap_sectors_lap_idx ON lap_sectors(lap_id, sector_index);
