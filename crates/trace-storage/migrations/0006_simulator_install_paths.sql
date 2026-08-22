CREATE TABLE simulator_install_paths (
    simulator_key TEXT PRIMARY KEY,
    custom_path TEXT NOT NULL CHECK (length(custom_path) BETWEEN 1 AND 1024)
);
