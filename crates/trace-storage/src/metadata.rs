//! `SQLite` metadata store and forward-only schema migration.

use std::path::Path;

use rusqlite::Connection;

const SCHEMA_VERSION: u32 = 1;
const MIGRATION_1: &str = include_str!("../migrations/0001_initial.sql");

/// `SQLite` metadata connection configured for TRACE invariants.
pub struct MetadataStore {
    connection: Connection,
}

impl MetadataStore {
    /// Opens or creates a metadata database and migrates it to the current schema.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] when `SQLite` cannot open, configure, or migrate the
    /// database, or when the file was created by a newer TRACE version.
    pub fn open(path: &Path) -> Result<Self, MetadataError> {
        let connection = Connection::open(path).map_err(MetadataError::from)?;
        Self::configure_and_migrate(connection)
    }

    /// Creates a migrated in-memory store for tests and ephemeral workflows.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] if `SQLite` setup or migration fails.
    pub fn open_in_memory() -> Result<Self, MetadataError> {
        let connection = Connection::open_in_memory().map_err(MetadataError::from)?;
        Self::configure_and_migrate(connection)
    }

    fn configure_and_migrate(mut connection: Connection) -> Result<Self, MetadataError> {
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(MetadataError::from)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(MetadataError::from)?;

        let current: u32 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(MetadataError::from)?;
        if current > SCHEMA_VERSION {
            return Err(MetadataError::NewerSchema {
                found: current,
                supported: SCHEMA_VERSION,
            });
        }
        if current == 0 {
            let transaction = connection.transaction().map_err(MetadataError::from)?;
            transaction
                .execute_batch(MIGRATION_1)
                .map_err(MetadataError::from)?;
            transaction
                .pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(MetadataError::from)?;
            transaction.commit().map_err(MetadataError::from)?;
        }

        Ok(Self { connection })
    }

    /// Returns the schema version stored by `SQLite`.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] if the pragma cannot be queried.
    pub fn schema_version(&self) -> Result<u32, MetadataError> {
        self.connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(MetadataError::from)
    }
}

/// Metadata database failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataError {
    Sqlite(String),
    NewerSchema { found: u32, supported: u32 },
}

impl From<rusqlite::Error> for MetadataError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_creates_expected_metadata_tables_only() {
        let store = MetadataStore::open_in_memory().expect("migrated store");
        assert_eq!(store.schema_version().expect("schema version"), 1);

        let mut statement = store
            .connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .expect("table query");
        let tables: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .expect("table rows")
            .collect::<Result<_, _>>()
            .expect("table names");

        assert!(tables.contains(&"sessions".to_owned()));
        assert!(tables.contains(&"laps".to_owned()));
        assert!(tables.contains(&"telemetry_blobs".to_owned()));
        assert!(!tables.contains(&"telemetry_samples".to_owned()));
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let store = MetadataStore::open_in_memory().expect("migrated store");
        let result = store.connection.execute(
            "INSERT INTO laps (id, session_id, lap_index, validity) VALUES ('lap', 'missing', 1, 'unknown')",
            [],
        );
        assert!(result.is_err());
    }
}
