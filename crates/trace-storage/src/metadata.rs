//! `SQLite` metadata store and forward-only schema migration.

use std::{collections::BTreeSet, path::Path};

use rusqlite::{Connection, OptionalExtension, params, types::Type};
use serde::{Deserialize, Serialize};

use crate::{BlobMetadata, RelativeBlobPath};

const SCHEMA_VERSION: u32 = 7;
const MIGRATION_1: &str = include_str!("../migrations/0001_initial.sql");
const MIGRATION_2: &str = include_str!("../migrations/0002_lap_sectors.sql");
const MIGRATION_3: &str = include_str!("../migrations/0003_session_details.sql");
const MIGRATION_4: &str = include_str!("../migrations/0004_session_attribution.sql");
const MIGRATION_5: &str = include_str!("../migrations/0005_lap_track_limits.sql");
const MIGRATION_6: &str = include_str!("../migrations/0006_simulator_install_paths.sql");
const MIGRATION_7: &str = include_str!("../migrations/0007_profile_and_saved_comparisons.sql");

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
    pub conditions: SessionConditions,
}

/// Session-scoped conditions captured from simulator configuration when live
/// telemetry does not publish reliable values.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionConditions {
    pub ambient_temperature_c: Option<String>,
    pub road_temperature_c: Option<String>,
    pub weather_name: Option<String>,
    pub track_grip_percent: Option<u8>,
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
    pub max_tyres_out: Option<u8>,
    pub sample_start: u64,
    pub sample_count: u64,
    pub is_personal_best: bool,
    pub sectors: Vec<NewSector>,
}

/// One sector time to commit with a completed lap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NewSector {
    pub index: u32,
    pub duration_ns: u64,
}

/// Display-safe lap metadata returned to the desktop application.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LapSummary {
    pub id: String,
    pub index: u32,
    pub duration_ns: Option<u64>,
    pub validity: String,
    pub validity_reason: Option<String>,
    pub max_tyres_out: Option<u8>,
    pub is_personal_best: bool,
    pub sectors: Vec<SectorSummary>,
}

/// Display-safe sector timing returned to the desktop application.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SectorSummary {
    pub index: u32,
    pub duration_ns: u64,
}

/// Display-safe locally persisted session metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub simulator_key: String,
    pub source_track_id: Option<String>,
    pub layout_id: Option<String>,
    pub source_car_id: Option<String>,
    pub user_title: Option<String>,
    pub user_driver: Option<String>,
    pub ownership: String,
    pub tags: Vec<String>,
    pub track: Option<String>,
    pub car: Option<String>,
    pub session_type: Option<String>,
    pub started_at: String,
    pub source_kind: String,
    #[serde(default)]
    pub conditions: SessionConditions,
    pub exportable: bool,
    pub laps: Vec<LapSummary>,
}

/// Filesystem location and sample range needed to read one recorded lap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LapTelemetryLocator {
    pub blob_path: RelativeBlobPath,
    pub sample_start: u64,
    pub sample_count: u64,
}

/// Filesystem location and sample count needed to export one recorded session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionTelemetryLocator {
    pub blob_path: RelativeBlobPath,
    pub sample_count: u64,
}

/// Persisted shortcut to a compatible pair of laps.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedComparison {
    pub id: String,
    pub name: String,
    pub reference_session_id: String,
    pub reference_lap_index: u32,
    pub reference_duration_ns: u64,
    pub reference_started_at: String,
    pub analysed_session_id: String,
    pub analysed_lap_index: u32,
    pub analysed_duration_ns: u64,
    pub analysed_started_at: String,
    pub simulator_key: String,
    pub track: String,
    pub car: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SavedLapIdentity {
    lap_id: String,
    duration_ns: u64,
    simulator_key: String,
    track_id: String,
    car_id: String,
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
        if current < 1 {
            let transaction = connection.transaction().map_err(MetadataError::from)?;
            transaction
                .execute_batch(MIGRATION_1)
                .map_err(MetadataError::from)?;
            transaction
                .pragma_update(None, "user_version", 1)
                .map_err(MetadataError::from)?;
            transaction.commit().map_err(MetadataError::from)?;
        }
        if current < 2 {
            let transaction = connection.transaction().map_err(MetadataError::from)?;
            transaction
                .execute_batch(MIGRATION_2)
                .map_err(MetadataError::from)?;
            transaction
                .pragma_update(None, "user_version", 2)
                .map_err(MetadataError::from)?;
            transaction.commit().map_err(MetadataError::from)?;
        }
        if current < 3 {
            let transaction = connection.transaction().map_err(MetadataError::from)?;
            transaction
                .execute_batch(MIGRATION_3)
                .map_err(MetadataError::from)?;
            transaction
                .pragma_update(None, "user_version", 3)
                .map_err(MetadataError::from)?;
            transaction.commit().map_err(MetadataError::from)?;
        }
        if current < 4 {
            let transaction = connection.transaction().map_err(MetadataError::from)?;
            transaction
                .execute_batch(MIGRATION_4)
                .map_err(MetadataError::from)?;
            transaction
                .pragma_update(None, "user_version", 4)
                .map_err(MetadataError::from)?;
            transaction.commit().map_err(MetadataError::from)?;
        }
        if current < 5 {
            let transaction = connection.transaction().map_err(MetadataError::from)?;
            transaction
                .execute_batch(MIGRATION_5)
                .map_err(MetadataError::from)?;
            transaction
                .pragma_update(None, "user_version", 5)
                .map_err(MetadataError::from)?;
            transaction.commit().map_err(MetadataError::from)?;
        }
        if current < 6 {
            let transaction = connection.transaction().map_err(MetadataError::from)?;
            transaction
                .execute_batch(MIGRATION_6)
                .map_err(MetadataError::from)?;
            transaction
                .pragma_update(None, "user_version", 6)
                .map_err(MetadataError::from)?;
            transaction.commit().map_err(MetadataError::from)?;
        }
        if current < 7 {
            let transaction = connection.transaction().map_err(MetadataError::from)?;
            transaction
                .execute_batch(MIGRATION_7)
                .map_err(MetadataError::from)?;
            transaction
                .pragma_update(None, "user_version", 7)
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

    /// Returns the local driver identity attached to self-owned captures and exports.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] when `SQLite` cannot read the setting.
    pub fn driver_profile_name(&self) -> Result<Option<String>, MetadataError> {
        self.connection
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'driver_profile_name'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(MetadataError::from)
    }

    /// Sets or clears the local driver identity.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] for an invalid name or when `SQLite` cannot persist it.
    pub fn set_driver_profile_name(&mut self, name: Option<&str>) -> Result<(), MetadataError> {
        match name {
            Some(value)
                if !value.trim().is_empty()
                    && value.chars().count() <= 80
                    && !value.chars().any(char::is_control) =>
            {
                self.connection
                    .execute(
                        "INSERT INTO app_settings (key, value) VALUES ('driver_profile_name', ?1)
                         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                        [value.trim()],
                    )
                    .map_err(MetadataError::from)?;
            }
            Some(_) => {
                return Err(MetadataError::InvalidRecord(
                    "driver name must contain 1–80 printable characters".into(),
                ));
            }
            None => {
                self.connection
                    .execute(
                        "DELETE FROM app_settings WHERE key = 'driver_profile_name'",
                        [],
                    )
                    .map_err(MetadataError::from)?;
            }
        }
        Ok(())
    }

    /// Saves a compatible lap pair, normalising the faster lap as Reference.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] when either lap is unavailable or incompatible, the
    /// record is invalid, or `SQLite` cannot persist it.
    #[allow(clippy::too_many_arguments)]
    pub fn save_comparison(
        &mut self,
        id: &str,
        name: &str,
        reference_session_id: &str,
        reference_lap_index: u32,
        analysed_session_id: &str,
        analysed_lap_index: u32,
        created_at: &str,
    ) -> Result<(), MetadataError> {
        if id.is_empty()
            || name.trim().is_empty()
            || name.chars().count() > 80
            || name.chars().any(char::is_control)
            || created_at.is_empty()
        {
            return Err(MetadataError::InvalidRecord(
                "invalid saved comparison".into(),
            ));
        }
        let reference = self.saved_lap_identity(reference_session_id, reference_lap_index)?;
        let analysed = self.saved_lap_identity(analysed_session_id, analysed_lap_index)?;
        if reference.lap_id == analysed.lap_id
            || reference.simulator_key != analysed.simulator_key
            || reference.track_id != analysed.track_id
            || reference.car_id != analysed.car_id
        {
            return Err(MetadataError::InvalidRecord(
                "saved comparison laps must use the same simulator, track, and car".into(),
            ));
        }
        let (reference, analysed) = if reference.duration_ns <= analysed.duration_ns {
            (reference, analysed)
        } else {
            (analysed, reference)
        };
        self.connection
            .execute(
                "INSERT INTO saved_comparisons
                 (id, name, reference_lap_id, analysed_lap_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id,
                    name.trim(),
                    reference.lap_id,
                    analysed.lap_id,
                    created_at
                ],
            )
            .map_err(MetadataError::from)?;
        Ok(())
    }

    /// Lists saved lap pairs newest first.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] when `SQLite` cannot read the saved lap pairs.
    pub fn saved_comparisons(&self) -> Result<Vec<SavedComparison>, MetadataError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT sc.id, sc.name,
                        rs.id, rl.lap_index, rl.duration_ns,
                        ass.id, al.lap_index, al.duration_ns,
                        sim.key, t.display_name, c.display_name,
                        rs.started_at, ass.started_at, sc.created_at
                 FROM saved_comparisons sc
                 JOIN laps rl ON rl.id = sc.reference_lap_id
                 JOIN sessions rs ON rs.id = rl.session_id
                 JOIN laps al ON al.id = sc.analysed_lap_id
                 JOIN sessions ass ON ass.id = al.session_id
                 JOIN simulators sim ON sim.id = rs.simulator_id
                 JOIN tracks t ON t.id = rs.track_id
                 JOIN cars c ON c.id = rs.car_id
                 ORDER BY sc.created_at DESC, sc.id DESC",
            )
            .map_err(MetadataError::from)?;
        let rows = statement
            .query_map([], |row| {
                Ok(SavedComparison {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    reference_session_id: row.get(2)?,
                    reference_lap_index: row_u32(row, 3)?,
                    reference_duration_ns: row_u64(row, 4)?,
                    analysed_session_id: row.get(5)?,
                    analysed_lap_index: row_u32(row, 6)?,
                    analysed_duration_ns: row_u64(row, 7)?,
                    simulator_key: row.get(8)?,
                    track: row.get(9)?,
                    car: row.get(10)?,
                    reference_started_at: row.get(11)?,
                    analysed_started_at: row.get(12)?,
                    created_at: row.get(13)?,
                })
            })
            .map_err(MetadataError::from)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(MetadataError::from)
    }

    /// Deletes one saved lap pair.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] when the record does not exist or `SQLite` cannot delete it.
    pub fn delete_saved_comparison(&mut self, id: &str) -> Result<(), MetadataError> {
        let changed = self
            .connection
            .execute("DELETE FROM saved_comparisons WHERE id = ?1", [id])
            .map_err(MetadataError::from)?;
        if changed == 0 {
            return Err(MetadataError::RecordNotFound);
        }
        Ok(())
    }

    /// Renames one saved lap pair without changing either referenced lap.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError::InvalidRecord`] for an invalid name,
    /// [`MetadataError::RecordNotFound`] for an unknown id, or a `SQLite` error.
    pub fn rename_saved_comparison(&mut self, id: &str, name: &str) -> Result<(), MetadataError> {
        if id.is_empty()
            || name.trim().is_empty()
            || name.chars().count() > 80
            || name.chars().any(char::is_control)
        {
            return Err(MetadataError::InvalidRecord(
                "invalid saved comparison name".into(),
            ));
        }
        let changed = self
            .connection
            .execute(
                "UPDATE saved_comparisons SET name = ?2 WHERE id = ?1",
                params![id, name.trim()],
            )
            .map_err(MetadataError::from)?;
        if changed == 0 {
            return Err(MetadataError::RecordNotFound);
        }
        Ok(())
    }

    fn saved_lap_identity(
        &self,
        session_id: &str,
        lap_index: u32,
    ) -> Result<SavedLapIdentity, MetadataError> {
        self.connection
            .query_row(
                "SELECT l.id, l.duration_ns, sim.key, t.id, c.id
                 FROM laps l
                 JOIN sessions s ON s.id = l.session_id
                 JOIN simulators sim ON sim.id = s.simulator_id
                 JOIN tracks t ON t.id = s.track_id
                 JOIN cars c ON c.id = s.car_id
                 WHERE s.id = ?1 AND l.lap_index = ?2
                   AND l.duration_ns IS NOT NULL AND l.validity != 'invalid'",
                params![session_id, lap_index],
                |row| {
                    Ok(SavedLapIdentity {
                        lap_id: row.get(0)?,
                        duration_ns: row_u64(row, 1)?,
                        simulator_key: row.get(2)?,
                        track_id: row.get(3)?,
                        car_id: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(MetadataError::from)?
            .ok_or(MetadataError::RecordNotFound)
    }

    /// Returns the user-configured install root for one simulator.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] when `SQLite` cannot read the setting.
    pub fn simulator_install_path(
        &self,
        simulator_key: &str,
    ) -> Result<Option<String>, MetadataError> {
        self.connection
            .query_row(
                "SELECT custom_path FROM simulator_install_paths WHERE simulator_key = ?1",
                [simulator_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(MetadataError::from)
    }

    /// Sets or clears a simulator install-root override.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] for an invalid key/path or `SQLite` write failure.
    pub fn set_simulator_install_path(
        &mut self,
        simulator_key: &str,
        custom_path: Option<&str>,
    ) -> Result<(), MetadataError> {
        if simulator_key.is_empty()
            || simulator_key.len() > 80
            || !simulator_key.chars().all(|character| {
                character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
            })
        {
            return Err(MetadataError::InvalidRecord("invalid simulator key".into()));
        }
        match custom_path {
            Some(path) if !path.is_empty() && path.chars().count() <= 1_024 => {
                self.connection
                    .execute(
                        "INSERT INTO simulator_install_paths (simulator_key, custom_path)
                         VALUES (?1, ?2)
                         ON CONFLICT(simulator_key) DO UPDATE SET custom_path = excluded.custom_path",
                        params![simulator_key, path],
                    )
                    .map_err(MetadataError::from)?;
            }
            Some(_) => {
                return Err(MetadataError::InvalidRecord(
                    "invalid simulator install path".into(),
                ));
            }
            None => {
                self.connection
                    .execute(
                        "DELETE FROM simulator_install_paths WHERE simulator_key = ?1",
                        [simulator_key],
                    )
                    .map_err(MetadataError::from)?;
            }
        }
        Ok(())
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
                 (id, simulator_id, track_id, car_id, started_at, session_type, source_kind,
                  conditions_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    session.id,
                    session.simulator_id,
                    session.track_id,
                    session.car_id,
                    session.started_at,
                    session.session_type,
                    session.source_kind,
                    serde_json::to_string(&session.conditions).map_err(|error| {
                        MetadataError::InvalidRecord(format!("invalid session conditions: {error}"))
                    })?
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
                      sample_count, is_personal_best, max_tyres_out)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
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
                        lap.is_personal_best,
                        lap.max_tyres_out
                    ],
                )
                .map_err(MetadataError::from)?;
            for sector in &lap.sectors {
                transaction
                    .execute(
                        "INSERT INTO lap_sectors (lap_id, sector_index, duration_ns)
                         VALUES (?1, ?2, ?3)",
                        params![lap.id, i64::from(sector.index), to_i64(sector.duration_ns)?],
                    )
                    .map_err(MetadataError::from)?;
            }
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
    #[allow(clippy::too_many_lines)]
    pub fn recent_sessions(&self, limit: usize) -> Result<Vec<SessionSummary>, MetadataError> {
        if limit == 0 {
            return Err(MetadataError::InvalidRecord(
                "recent session limit must be greater than zero".into(),
            ));
        }
        let mut statement = self
            .connection
            .prepare(
                "SELECT s.id, sim.key, s.user_title, s.user_driver, s.ownership,
                        t.display_name, c.display_name, s.session_type, s.started_at, s.source_kind,
                        EXISTS(SELECT 1 FROM telemetry_blobs b WHERE b.session_id = s.id),
                        t.source_track_id, t.layout_id, c.source_car_id, s.conditions_json
                 FROM sessions s JOIN simulators sim ON sim.id = s.simulator_id
                 LEFT JOIN tracks t ON t.id = s.track_id
                 LEFT JOIN cars c ON c.id = s.car_id
                 ORDER BY s.started_at DESC, s.id
                 LIMIT ?1",
            )
            .map_err(MetadataError::from)?;
        let limit = i64::try_from(limit).map_err(|_| MetadataError::IntegerOverflow)?;
        let rows = statement
            .query_map([limit], |row| {
                Ok(SessionSummary {
                    id: row.get(0)?,
                    simulator_key: row.get(1)?,
                    user_title: row.get(2)?,
                    user_driver: row.get(3)?,
                    ownership: row.get(4)?,
                    tags: Vec::new(),
                    track: row.get(5)?,
                    car: row.get(6)?,
                    session_type: row.get(7)?,
                    started_at: row.get(8)?,
                    source_kind: row.get(9)?,
                    exportable: row.get(10)?,
                    source_track_id: row.get(11)?,
                    layout_id: row.get(12)?,
                    source_car_id: row.get(13)?,
                    conditions: serde_json::from_str(&row.get::<_, String>(14)?).map_err(
                        |error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                14,
                                Type::Text,
                                Box::new(error),
                            )
                        },
                    )?,
                    laps: Vec::new(),
                })
            })
            .map_err(MetadataError::from)?;
        let mut sessions: Vec<_> = rows
            .collect::<Result<_, _>>()
            .map_err(MetadataError::from)?;
        drop(statement);
        let mut lap_statement = self
            .connection
            .prepare(
                "SELECT id, lap_index, duration_ns, validity, validity_reason, is_personal_best, max_tyres_out FROM laps
                 WHERE session_id = ?1 ORDER BY lap_index",
            )
            .map_err(MetadataError::from)?;
        for session in &mut sessions {
            session.tags = query_session_tags(&self.connection, &session.id)?;
            let laps: Vec<(String, LapSummary)> = lap_statement
                .query_map([&session.id], |row| {
                    let id: String = row.get(0)?;
                    let index: u32 = row.get(1)?;
                    let duration = optional_row_u64(row, 2)?;
                    Ok((
                        id.clone(),
                        LapSummary {
                            id: id.clone(),
                            index,
                            duration_ns: duration,
                            validity: row.get(3)?,
                            validity_reason: row.get(4)?,
                            is_personal_best: row.get(5)?,
                            max_tyres_out: row.get(6)?,
                            sectors: Vec::new(),
                        },
                    ))
                })
                .map_err(MetadataError::from)?
                .collect::<Result<_, _>>()
                .map_err(MetadataError::from)?;
            let mut sector_statement = self
                .connection
                .prepare(
                    "SELECT sector_index, duration_ns FROM lap_sectors
                     WHERE lap_id = ?1 ORDER BY sector_index",
                )
                .map_err(MetadataError::from)?;
            session.laps = laps
                .into_iter()
                .map(|(lap_id, mut lap)| {
                    lap.sectors = sector_statement
                        .query_map([lap_id], |row| {
                            Ok(SectorSummary {
                                index: row.get(0)?,
                                duration_ns: row_u64(row, 1)?,
                            })
                        })
                        .map_err(MetadataError::from)?
                        .collect::<Result<_, _>>()
                        .map_err(MetadataError::from)?;
                    Ok(lap)
                })
                .collect::<Result<_, MetadataError>>()?;
        }
        Ok(sessions)
    }

    /// Replaces the user-managed title and tags for a recorded session atomically.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError`] when the session is missing, details exceed bounded
    /// limits, or `SQLite` cannot commit the update.
    pub fn update_session_details(
        &mut self,
        session_id: &str,
        user_title: Option<&str>,
        user_driver: Option<&str>,
        ownership: &str,
        tags: &[String],
    ) -> Result<(), MetadataError> {
        validate_session_details(user_title, user_driver, ownership, tags)?;
        let transaction = self.connection.transaction().map_err(MetadataError::from)?;
        let changed = transaction
            .execute(
                "UPDATE sessions SET user_title = ?1, user_driver = ?2, ownership = ?3
                 WHERE id = ?4",
                params![user_title, user_driver, ownership, session_id],
            )
            .map_err(MetadataError::from)?;
        if changed != 1 {
            return Err(MetadataError::RecordNotFound);
        }
        transaction
            .execute(
                "DELETE FROM session_tags WHERE session_id = ?1",
                [session_id],
            )
            .map_err(MetadataError::from)?;
        for tag in tags {
            transaction
                .execute(
                    "INSERT INTO session_tags (session_id, tag) VALUES (?1, ?2)",
                    params![session_id, tag],
                )
                .map_err(MetadataError::from)?;
        }
        transaction.commit().map_err(MetadataError::from)
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

    /// Finds the immutable telemetry blob for one completed session.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError::RecordNotFound`] when the session is unknown or has
    /// not completed, and another metadata error for malformed values/query failure.
    pub fn session_telemetry(
        &self,
        session_id: &str,
    ) -> Result<SessionTelemetryLocator, MetadataError> {
        let result = self.connection.query_row(
            "SELECT b.relative_path, b.sample_count
             FROM telemetry_blobs b
             WHERE b.session_id = ?1",
            [session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        );
        let (path, sample_count) = match result {
            Ok(value) => value,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Err(MetadataError::RecordNotFound),
            Err(error) => return Err(MetadataError::from(error)),
        };
        Ok(SessionTelemetryLocator {
            blob_path: RelativeBlobPath::parse(path).map_err(|error| {
                MetadataError::InvalidRecord(format!(
                    "stored session blob path is invalid: {error:?}"
                ))
            })?,
            sample_count: u64::try_from(sample_count)
                .map_err(|_| MetadataError::IntegerOverflow)?,
        })
    }

    /// Deletes one session and all metadata owned by it.
    ///
    /// The returned path identifies the immutable telemetry file that the caller
    /// must remove from blob storage after the transaction commits. Callers are
    /// responsible for protecting a session that is still actively being written.
    ///
    /// # Errors
    ///
    /// Returns [`MetadataError::RecordNotFound`] for an unknown session or a metadata
    /// error when the stored blob path or transaction is invalid.
    pub fn delete_session(
        &mut self,
        session_id: &str,
    ) -> Result<Option<RelativeBlobPath>, MetadataError> {
        let transaction = self.connection.transaction().map_err(MetadataError::from)?;
        let path = transaction
            .query_row(
                "SELECT relative_path FROM telemetry_blobs WHERE session_id = ?1",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(MetadataError::from)?
            .map(RelativeBlobPath::parse)
            .transpose()
            .map_err(|error| {
                MetadataError::InvalidRecord(format!(
                    "stored session blob path is invalid: {error:?}"
                ))
            })?;

        let changed = transaction
            .execute("DELETE FROM sessions WHERE id = ?1", [session_id])
            .map_err(MetadataError::from)?;
        if changed != 1 {
            return Err(MetadataError::RecordNotFound);
        }
        transaction.commit().map_err(MetadataError::from)?;
        Ok(path)
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

fn query_session_tags(
    connection: &Connection,
    session_id: &str,
) -> Result<Vec<String>, MetadataError> {
    let mut statement = connection
        .prepare("SELECT tag FROM session_tags WHERE session_id = ?1 ORDER BY tag COLLATE NOCASE")
        .map_err(MetadataError::from)?;
    statement
        .query_map([session_id], |row| row.get(0))
        .map_err(MetadataError::from)?
        .collect::<Result<_, _>>()
        .map_err(MetadataError::from)
}

fn validate_session_details(
    user_title: Option<&str>,
    user_driver: Option<&str>,
    ownership: &str,
    tags: &[String],
) -> Result<(), MetadataError> {
    if user_title.is_some_and(|title| {
        title.trim().is_empty() || title.chars().count() > 80 || title.chars().any(char::is_control)
    }) {
        return Err(MetadataError::InvalidRecord(
            "session title must contain 1–80 printable characters".into(),
        ));
    }
    if user_driver.is_some_and(|driver| {
        driver.trim().is_empty()
            || driver.chars().count() > 80
            || driver.chars().any(char::is_control)
    }) {
        return Err(MetadataError::InvalidRecord(
            "driver name must contain 1–80 printable characters".into(),
        ));
    }
    if !matches!(ownership, "mine" | "other" | "unknown") {
        return Err(MetadataError::InvalidRecord(
            "session ownership must be mine, other, or unknown".into(),
        ));
    }
    if tags.len() > 12 {
        return Err(MetadataError::InvalidRecord(
            "a session may have at most 12 tags".into(),
        ));
    }
    let mut unique = BTreeSet::new();
    for tag in tags {
        if tag.trim() != tag
            || tag.is_empty()
            || tag.chars().count() > 32
            || tag.chars().any(char::is_control)
            || !unique.insert(tag.to_lowercase())
        {
            return Err(MetadataError::InvalidRecord(
                "tags must be unique printable values of 1–32 characters".into(),
            ));
        }
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
        if lap
            .sectors
            .iter()
            .any(|sector| sector.index == 0 || sector.duration_ns == 0)
        {
            return Err(MetadataError::InvalidRecord(
                "sector index and duration must be greater than zero".into(),
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

fn row_u64(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
    })
}

fn row_u32(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u32> {
    let value: i64 = row.get(index)?;
    u32::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(index, Type::Integer, Box::new(error))
    })
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
            conditions: SessionConditions::default(),
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
        assert_eq!(store.schema_version().expect("schema version"), 7);

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
        assert!(tables.contains(&"lap_sectors".to_owned()));
        assert!(tables.contains(&"session_tags".to_owned()));
        assert!(tables.contains(&"telemetry_blobs".to_owned()));
        assert!(tables.contains(&"simulator_install_paths".to_owned()));
        assert!(tables.contains(&"app_settings".to_owned()));
        assert!(tables.contains(&"saved_comparisons".to_owned()));
        assert!(!tables.contains(&"telemetry_samples".to_owned()));
    }

    #[test]
    fn simulator_install_path_can_be_set_replaced_and_cleared() {
        let mut store = MetadataStore::open_in_memory().expect("metadata");
        assert_eq!(
            store.simulator_install_path("assetto-corsa").expect("path"),
            None
        );

        store
            .set_simulator_install_path("assetto-corsa", Some("C:\\Games\\assettocorsa"))
            .expect("set path");
        assert_eq!(
            store.simulator_install_path("assetto-corsa").expect("path"),
            Some("C:\\Games\\assettocorsa".into())
        );

        store
            .set_simulator_install_path("assetto-corsa", None)
            .expect("clear path");
        assert_eq!(
            store.simulator_install_path("assetto-corsa").expect("path"),
            None
        );
    }

    #[test]
    fn version_one_database_migrates_without_losing_existing_rows() {
        let connection = Connection::open_in_memory().expect("database");
        connection
            .execute_batch(MIGRATION_1)
            .expect("version one schema");
        connection
            .pragma_update(None, "user_version", 1)
            .expect("version one marker");
        connection
            .execute(
                "INSERT INTO simulators (id, key) VALUES ('sim-ac', 'assetto-corsa')",
                [],
            )
            .expect("existing row");

        let store = MetadataStore::configure_and_migrate(connection).expect("migration");

        assert_eq!(store.schema_version().expect("schema version"), 7);
        let simulator_count: u32 = store
            .connection
            .query_row("SELECT COUNT(*) FROM simulators", [], |row| row.get(0))
            .expect("simulator count");
        assert_eq!(simulator_count, 1);
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
    fn driver_profile_can_be_set_and_cleared() {
        let mut store = MetadataStore::open_in_memory().expect("migrated store");
        assert_eq!(store.driver_profile_name().expect("profile"), None);

        store
            .set_driver_profile_name(Some("  Alex Driver  "))
            .expect("set profile");
        assert_eq!(
            store.driver_profile_name().expect("profile"),
            Some("Alex Driver".into())
        );

        store.set_driver_profile_name(None).expect("clear profile");
        assert_eq!(store.driver_profile_name().expect("profile"), None);
    }

    #[test]
    fn saved_comparison_normalises_the_faster_lap_as_reference() {
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
                        id: "lap-fast".into(),
                        lap_index: 1,
                        started_offset_ns: Some(0),
                        duration_ns: Some(90_000_000_000),
                        validity: "valid".into(),
                        validity_reason: None,
                        max_tyres_out: Some(0),
                        sample_start: 0,
                        sample_count: 100,
                        is_personal_best: true,
                        sectors: Vec::new(),
                    },
                    NewLap {
                        id: "lap-slow".into(),
                        lap_index: 2,
                        started_offset_ns: Some(90_000_000_000),
                        duration_ns: Some(92_000_000_000),
                        validity: "valid".into(),
                        validity_reason: None,
                        max_tyres_out: Some(0),
                        sample_start: 100,
                        sample_count: 100,
                        is_personal_best: false,
                        sectors: Vec::new(),
                    },
                ],
            )
            .expect("completed session");

        store
            .save_comparison(
                "saved-1",
                "Race setup",
                "session-1",
                2,
                "session-1",
                1,
                "2026-08-21T16:00:00Z",
            )
            .expect("save comparison");

        let saved = store.saved_comparisons().expect("saved comparisons");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].reference_lap_index, 1);
        assert_eq!(saved[0].analysed_lap_index, 2);
        assert_eq!(saved[0].name, "Race setup");
        assert_eq!(saved[0].reference_started_at, "2026-08-21T14:32:00Z");
        assert_eq!(saved[0].analysed_started_at, "2026-08-21T14:32:00Z");

        store
            .rename_saved_comparison("saved-1", "Qualifying benchmark")
            .expect("rename comparison");
        assert_eq!(
            store.saved_comparisons().expect("renamed comparisons")[0].name,
            "Qualifying benchmark"
        );

        store
            .delete_saved_comparison("saved-1")
            .expect("delete comparison");
        assert!(
            store
                .saved_comparisons()
                .expect("saved comparisons")
                .is_empty()
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
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
                        max_tyres_out: Some(0),
                        sample_start: 0,
                        sample_count: 100,
                        is_personal_best: true,
                        sectors: vec![
                            NewSector {
                                index: 1,
                                duration_ns: 36_370_000_000,
                            },
                            NewSector {
                                index: 2,
                                duration_ns: 37_201_000_000,
                            },
                        ],
                    },
                    NewLap {
                        id: "lap-2".into(),
                        lap_index: 2,
                        started_offset_ns: Some(110_906_000_000),
                        duration_ns: None,
                        validity: "invalid".into(),
                        validity_reason: Some("session ended".into()),
                        max_tyres_out: None,
                        sample_start: 100,
                        sample_count: 100,
                        is_personal_best: false,
                        sectors: Vec::new(),
                    },
                ],
            )
            .expect("completed session");
        store
            .update_session_details(
                "session-1",
                Some("League qualifying run"),
                Some("Alex Driver"),
                "other",
                &["league".into(), "wet".into()],
            )
            .expect("session details");

        let summaries = store.recent_sessions(10).expect("session summaries");
        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries[0].user_title.as_deref(),
            Some("League qualifying run")
        );
        assert_eq!(summaries[0].tags, vec!["league", "wet"]);
        assert_eq!(summaries[0].user_driver.as_deref(), Some("Alex Driver"));
        assert_eq!(summaries[0].ownership, "other");
        assert_eq!(summaries[0].track.as_deref(), Some("Mugello"));
        assert!(summaries[0].exportable);
        assert_eq!(summaries[0].laps.len(), 2);
        assert_eq!(summaries[0].laps[0].duration_ns, Some(110_906_000_000));
        assert_eq!(summaries[0].laps[0].max_tyres_out, Some(0));
        assert!(summaries[0].laps[0].is_personal_best);
        assert_eq!(
            summaries[0].laps[0].sectors,
            vec![
                SectorSummary {
                    index: 1,
                    duration_ns: 36_370_000_000,
                },
                SectorSummary {
                    index: 2,
                    duration_ns: 37_201_000_000,
                },
            ]
        );
        assert_eq!(
            summaries[0].laps[1].validity_reason.as_deref(),
            Some("session ended")
        );
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
            store
                .session_telemetry("session-1")
                .expect("session telemetry"),
            SessionTelemetryLocator {
                blob_path: blob().path,
                sample_count: 200,
            }
        );
        assert_eq!(
            store.lap_telemetry("missing"),
            Err(MetadataError::RecordNotFound)
        );
        assert_eq!(
            store.session_telemetry("missing"),
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
                max_tyres_out: None,
                sample_start: 150,
                sample_count: 100,
                is_personal_best: false,
                sectors: Vec::new(),
            }],
        );
        assert!(matches!(error, Err(MetadataError::InvalidRecord(_))));
        let sessions = store.recent_sessions(10).expect("sessions");
        assert!(sessions[0].laps.is_empty());
        assert!(!sessions[0].exportable);
    }

    #[test]
    fn deleting_a_completed_session_cascades_metadata_and_returns_its_blob() {
        let mut store = MetadataStore::open_in_memory().expect("migrated store");
        store
            .create_session(&session("session-1"))
            .expect("session");
        store
            .complete_session("session-1", "2026-08-21T15:00:00Z", &blob(), &[])
            .expect("completed session");

        let deleted = store.delete_session("session-1").expect("delete session");

        assert_eq!(deleted, Some(blob().path));
        assert!(store.recent_sessions(10).expect("sessions").is_empty());
        assert!(
            store
                .referenced_blob_paths()
                .expect("blob paths")
                .is_empty()
        );
        assert_eq!(
            store.delete_session("session-1"),
            Err(MetadataError::RecordNotFound)
        );
    }

    #[test]
    fn deleting_an_incomplete_session_cleans_up_its_metadata() {
        let mut store = MetadataStore::open_in_memory().expect("migrated store");
        store
            .create_session(&session("session-1"))
            .expect("session");

        assert_eq!(store.delete_session("session-1"), Ok(None));
        assert!(store.recent_sessions(10).expect("sessions").is_empty());
    }
}
