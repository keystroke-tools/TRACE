ALTER TABLE laps ADD COLUMN max_tyres_out INTEGER
    CHECK (max_tyres_out IS NULL OR max_tyres_out BETWEEN 0 AND 4);
