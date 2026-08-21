//! `SQLite` metadata store and forward-only schema migration.

use std::{collections::BTreeSet, path::Path};

use rusqlite::{Connection, params, types::Type};
use serde::{Deserialize, Serialize};

use crate::{BlobMetadata, RelativeBlobPath};

const SCHEMA_VERSION: u32 = 1;
const MIGRATION_1: &str = include_str!("../migrations/0001_initial.sql");

/// `SQLite` metadata connection configured for TRACE invariants.
pub struct MetadataStore {
    connection: Connection,
}

/// Stable identities and metadata required to begin a locally captured session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewSession {
    pub id: String,
    pub simulator_id: String,
    pub simulator_key: String,
    pub simulator_version: Option<String>,
    pub track_id: Option<String>,
    pub source_track_id: Option<String>,
    pub layout_id: Option<String>,
    pub track_display_name: Option<String>,
    pub car_id: Option<String>,
    pub source_car_id: Option<String>,
    pub car_display_name: Option<String>,
    pub started_at: String,
    pub session_type: Option<String>,
    pub source_kind: String,
}

/// One completed lap to commit with a session telemetry blob.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewLap {
    pub id: String,
    pub lap_index: u32,
    pub started_offset_ns: Option<u64>,
    pub duration_ns: Option<u64>,
    pub validity: String,
    pub validity_reason: Option<String>,
    pub sample_start: u64,
    pub sample_count: u64,
    pub is_personal_best: bool,
}

/// Display-safe lap metadata returned to the desktop application.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LapSummary {
    pub index: u32,
    pub duration_ns: Option<u64>,
    pub validity: String,
    pub is_personal_best: bool,
}

/// Display-safe locally persisted session metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub track: Option<String>,
    pub car: Option<String>,
    pub session_type: Option<String>,
    pub started_at: String,
    pub source_kind: String,
    pub laps: Vec<LapSummary>,
}

/// Filesystem location and sample range needed to read one recorded lap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LapTelemetryLocator {
    pub blob_path: RelativeBlobPath,
    pub sample_start: u64,
    pub sample_count: u64,
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

    /// Creates a session and its simulator/track/car identities atomically.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] for inconsistent optional identity fields,
    /// duplicate sessions, or any `SQLite` failure.
    pub fn create_session(&mut self, session: &NewSession) -> Result<(), MetadataError> {
        validate_session(session)?;
        let transaction = self.connection.transaction().map_err(MetadataError::from)?;
        transaction
            .execute(
                "INSERT INTO simulators (id, key, version) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET version = excluded.version",
                params![
                    session.simulator_id,
                    session.simulator_key,
                    session.simulator_version
                ],
            )
            .map_err(MetadataError::from)?;
        if let (Some(id), Some(source_id), Some(display_name)) = (
            &session.track_id,
            &session.source_track_id,
            &session.track_display_name,
        ) {
            transaction
                .execute(
                    "INSERT INTO tracks
                     (id, simulator_id, source_track_id, layout_id, display_name)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name",
                    params![
                        id,
                        session.simulator_id,
                        source_id,
                        session.layout_id.as_deref().unwrap_or(""),
                        display_name
                    ],
                )
                .map_err(MetadataError::from)?;
        }
        if let (Some(id), Some(source_id), Some(display_name)) = (
            &session.car_id,
            &session.source_car_id,
            &session.car_display_name,
        ) {
            transaction
                .execute(
                    "INSERT INTO cars (id, simulator_id, source_car_id, display_name)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(id) DO UPDATE SET display_name = excluded.display_name",
                    params![id, session.simulator_id, source_id, display_name],
                )
                .map_err(MetadataError::from)?;
        }
        transaction
            .execute(
                "INSERT INTO sessions
                 (id, simulator_id, track_id, car_id, started_at, session_type, source_kind)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    session.id,
                    session.simulator_id,
                    session.track_id,
                    session.car_id,
                    session.started_at,
                    session.session_type,
                    session.source_kind
                ],
            )
            .map_err(MetadataError::from)?;
        transaction.commit().map_err(MetadataError::from)
    }

    /// Atomically records a committed telemetry blob, its laps, and session end.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] for integer overflow, invalid lap ranges, duplicate
    /// records, a missing session, or any `SQLite` failure.
    pub fn complete_session(
        &mut self,
        session_id: &str,
        ended_at: &str,
        blob: &BlobMetadata,
        laps: &[NewLap],
    ) -> Result<(), MetadataError> {
        validate_laps(laps, blob.sample_count)?;
        let transaction = self.connection.transaction().map_err(MetadataError::from)?;
        transaction
            .execute(
                "INSERT INTO telemetry_blobs
                 (id, session_id, relative_path, format, schema_version, byte_length,
                  sample_count, sha256, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    blob.id.as_str(),
                    session_id,
                    blob.path.as_str(),
                    blob_format(blob),
                    i64::from(blob.schema_version),
                    to_i64(blob.byte_length)?,
                    to_i64(blob.sample_count)?,
                    blob.sha256.as_slice(),
                    ended_at
                ],
            )
            .map_err(MetadataError::from)?;
        for lap in laps {
            transaction
                .execute(
                    "INSERT INTO laps
                     (id, session_id, lap_index, started_offset_ns, duration_ns,
                      validity, validity_reason, telemetry_blob_id, sample_start,
                      sample_count, is_personal_best)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        lap.id,
                        session_id,
                        i64::from(lap.lap_index),
                        optional_i64(lap.started_offset_ns)?,
                        optional_i64(lap.duration_ns)?,
                        lap.validity,
                        lap.validity_reason,
                        blob.id.as_str(),
                        to_i64(lap.sample_start)?,
                        to_i64(lap.sample_count)?,
                        lap.is_personal_best
                    ],
                )
                .map_err(MetadataError::from)?;
        }
        let changed = transaction
            .execute(
                "UPDATE sessions SET ended_at = ?1 WHERE id = ?2 AND ended_at IS NULL",
                params![ended_at, session_id],
            )
            .map_err(MetadataError::from)?;
        if changed != 1 {
            return Err(MetadataError::SessionNotOpen);
        }
        transaction.commit().map_err(MetadataError::from)
    }

    /// Returns newest sessions and their laps for the local session browser.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] when `limit` is zero, values cannot be represented,
    /// or `SQLite` cannot execute the bounded query.
    pub fn recent_sessions(&self, limit: usize) -> Result<Vec<SessionSummary>, MetadataError> {
        if limit == 0 {
            return Err(MetadataError::InvalidRecord(
                "recent session limit must be greater than zero".into(),
            ));
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT s.id, t.display_name, c.display_name, s.session_type,
                        s.started_at, s.source_kind
                 FROM sessions s
                 LEFT JOIN tracks t ON t.id = s.track_id
                 LEFT JOIN cars c ON c.id = s.car_id
                 ORDER BY s.started_at DESC, s.id
                 LIMIT ?1",
            )
            .map_err(MetadataError::from)?;
        let rows = statement
            .query_map(
                [to_i64(
                    u64::try_from(limit).map_err(|_| MetadataError::IntegerOverflow)?,
                )?],
                |row| {
                    Ok(SessionSummary {
                        id: row.get(0)?,
                        track: row.get(1)?,
                        car: row.get(2)?,
                        session_type: row.get(3)?,
                        started_at: row.get(4)?,
                        source_kind: row.get(5)?,
                        laps: Vec::new(),
                    })
                },
            )
            .map_err(MetadataError::from)?;
        let mut sessions: Vec<_> = rows
            .collect::<Result<_, _>>()
            .map_err(MetadataError::from)?;
        drop(statement);
        let mut lap_statement = self
            .connection
            .prepare(
                "SELECT lap_index, duration_ns, validity, is_personal_best
                 FROM laps WHERE session_id = ?1 ORDER BY lap_index",
            )
            .map_err(MetadataError::from)?;
        for session in &mut sessions {
            session.laps = lap_statement
                .query_map([&session.id], |row| {
                    let index: u32 = row.get(0)?;
                    let duration = optional_row_u64(row, 1)?;
                    Ok(LapSummary {
                        index,
                        duration_ns: duration,
                        validity: row.get(2)?,
                        is_personal_best: row.get(3)?,
                    })
                })
                .map_err(MetadataError::from)?
                .collect::<Result<_, _>>()
                .map_err(MetadataError::from)?;
        }
        Ok(sessions)
    }

    /// Returns every filesystem blob path referenced by metadata.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] for a malformed stored path or `SQLite` query failure.
    pub fn referenced_blob_paths(&self) -> Result<BTreeSet<RelativeBlobPath>, MetadataError> {
        let mut statement = self
            .connection
            .prepare("SELECT relative_path FROM telemetry_blobs ORDER BY relative_path")
            .map_err(MetadataError::from)?;
        let paths = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(MetadataError::from)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(MetadataError::from)?;
        paths
            .into_iter()
            .map(|path| {
                RelativeBlobPath::parse(path).map_err(|error| {
                    MetadataError::InvalidRecord(format!(
                        "stored telemetry blob path is invalid: {error:?}"
                    ))
                })
            })
            .collect()
    }

    /// Finds the immutable blob and exact sample range for one lap.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError::RecordNotFound`] when the lap has no complete
    /// telemetry location, or another metadata error for malformed values/query failure.
    pub fn lap_telemetry(&self, lap_id: &str) -> Result<LapTelemetryLocator, MetadataError> {
        let result = self.connection.query_row(
            "SELECT b.relative_path, l.sample_start, l.sample_count
             FROM laps l
             JOIN telemetry_blobs b ON b.id = l.telemetry_blob_id
             WHERE l.id = ?1 AND l.sample_start IS NOT NULL AND l.sample_count IS NOT NULL",
            [lap_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        );
        let (path, sample_start, sample_count) = match result {
            Ok(value) => value,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Err(MetadataError::RecordNotFound),
            Err(error) => return Err(MetadataError::from(error)),
        };
        Ok(LapTelemetryLocator {
            blob_path: RelativeBlobPath::parse(path).map_err(|error| {
                MetadataError::InvalidRecord(format!("stored lap blob path is invalid: {error:?}"))
            })?,
            sample_start: u64::try_from(sample_start)
                .map_err(|_| MetadataError::IntegerOverflow)?,
            sample_count: u64::try_from(sample_count)
                .map_err(|_| MetadataError::IntegerOverflow)?,
        })
    }
}

/// Metadata database failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataError {
    Sqlite(String),
    NewerSchema { found: u32, supported: u32 },
    InvalidRecord(String),
    IntegerOverflow,
    SessionNotOpen,
    RecordNotFound,
}

fn validate_session(session: &NewSession) -> Result<(), MetadataError> {
    if session.id.is_empty()
        || session.simulator_id.is_empty()
        || session.simulator_key.is_empty()
        || session.started_at.is_empty()
        || session.source_kind.is_empty()
    {
        return Err(MetadataError::InvalidRecord(
            "session required fields must be non-empty".into(),
        ));
    }
    let track_fields = [
        session.track_id.is_some(),
        session.source_track_id.is_some(),
        session.track_display_name.is_some(),
    ];
    let car_fields = [
        session.car_id.is_some(),
        session.source_car_id.is_some(),
        session.car_display_name.is_some(),
    ];
    if (!track_fields.iter().all(|value| *value) && track_fields.iter().any(|value| *value))
        || (!car_fields.iter().all(|value| *value) && car_fields.iter().any(|value| *value))
    {
        return Err(MetadataError::InvalidRecord(
            "track and car identity fields must be complete or absent".into(),
        ));
    }
    Ok(())
}

fn validate_laps(laps: &[NewLap], samples: u64) -> Result<(), MetadataError> {
    for lap in laps {
        if lap.id.is_empty() || lap.validity.is_empty() {
            return Err(MetadataError::InvalidRecord(
                "lap id and validity must be non-empty".into(),
            ));
        }
        let end = lap
            .sample_start
            .checked_add(lap.sample_count)
            .ok_or(MetadataError::IntegerOverflow)?;
        if end > samples {
            return Err(MetadataError::InvalidRecord(
                "lap sample range exceeds telemetry blob".into(),
            ));
        }
    }
    Ok(())
}

fn blob_format(blob: &BlobMetadata) -> &'static str {
    match blob.format {
        crate::BlobFormat::ArrowIpc => "arrow_ipc",
        crate::BlobFormat::TraceFixture => "trace_fixture",
    }
}

fn optional_i64(value: Option<u64>) -> Result<Option<i64>, MetadataError> {
    value.map(to_i64).transpose()
}

fn to_i64(value: u64) -> Result<i64, MetadataError> {
    i64::try_from(value).map_err(|_| MetadataError::IntegerOverflow)
}

fn optional_row_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            u64::try_from(value).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
            })
        })
        .transpose()
}

impl From<rusqlite::Error> for MetadataError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BlobFormat, BlobId, RelativeBlobPath};

    fn session(id: &str) -> NewSession {
        NewSession {
            id: id.into(),
            simulator_id: "sim-ac".into(),
            simulator_key: "assetto-corsa".into(),
            simulator_version: Some("1.16.4".into()),
            track_id: Some("track-mugello".into()),
            source_track_id: Some("mugello".into()),
            layout_id: None,
            track_display_name: Some("Mugello".into()),
            car_id: Some("car-tatuus".into()),
            source_car_id: Some("tatuusfa1".into()),
            car_display_name: Some("Tatuus FA01".into()),
            started_at: "2026-08-21T14:32:00Z".into(),
            session_type: Some("practice".into()),
            source_kind: "native_capture".into(),
        }
    }

    fn blob() -> BlobMetadata {
        BlobMetadata {
            id: BlobId("blob-session-1".into()),
            path: RelativeBlobPath::parse("sessions/session-1.arrow").expect("valid path"),
            format: BlobFormat::ArrowIpc,
            schema_version: 1,
            byte_length: 1_024,
            sample_count: 200,
            sha256: [7; 32],
        }
    }

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

    #[test]
    fn session_completion_is_returned_as_a_browser_summary() {
        let mut store = MetadataStore::open_in_memory().expect("migrated store");
        store
            .create_session(&session("session-1"))
            .expect("session");
        store
            .complete_session(
                "session-1",
                "2026-08-21T15:00:00Z",
                &blob(),
                &[
                    NewLap {
                        id: "lap-1".into(),
                        lap_index: 1,
                        started_offset_ns: Some(0),
                        duration_ns: Some(110_906_000_000),
                        validity: "valid".into(),
                        validity_reason: None,
                        sample_start: 0,
                        sample_count: 100,
                        is_personal_best: true,
                    },
                    NewLap {
                        id: "lap-2".into(),
                        lap_index: 2,
                        started_offset_ns: Some(110_906_000_000),
                        duration_ns: None,
                        validity: "invalid".into(),
                        validity_reason: Some("session ended".into()),
                        sample_start: 100,
                        sample_count: 100,
                        is_personal_best: false,
                    },
                ],
            )
            .expect("completed session");

        let summaries = store.recent_sessions(10).expect("session summaries");
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].track.as_deref(), Some("Mugello"));
        assert_eq!(summaries[0].laps.len(), 2);
        assert_eq!(summaries[0].laps[0].duration_ns, Some(110_906_000_000));
        assert!(summaries[0].laps[0].is_personal_best);
        assert_eq!(
            store.referenced_blob_paths().expect("blob paths"),
            BTreeSet::from([blob().path])
        );
        assert_eq!(
            store.lap_telemetry("lap-2").expect("lap telemetry"),
            LapTelemetryLocator {
                blob_path: blob().path,
                sample_start: 100,
                sample_count: 100,
            }
        );
        assert_eq!(
            store.lap_telemetry("missing"),
            Err(MetadataError::RecordNotFound)
        );
    }

    #[test]
    fn invalid_lap_ranges_do_not_partially_complete_a_session() {
        let mut store = MetadataStore::open_in_memory().expect("migrated store");
        store
            .create_session(&session("session-1"))
            .expect("session");
        let error = store.complete_session(
            "session-1",
            "2026-08-21T15:00:00Z",
            &blob(),
            &[NewLap {
                id: "lap-1".into(),
                lap_index: 1,
                started_offset_ns: None,
                duration_ns: None,
                validity: "unknown".into(),
                validity_reason: None,
                sample_start: 150,
                sample_count: 100,
                is_personal_best: false,
            }],
        );
        assert!(matches!(error, Err(MetadataError::InvalidRecord(_))));
        assert!(
            store.recent_sessions(10).expect("sessions")[0]
                .laps
                .is_empty()
        );
    }
}
