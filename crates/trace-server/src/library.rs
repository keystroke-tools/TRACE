use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use trace_storage::{
    ipc::{
        LapElectronicsMetrics, LapSettingMetrics, LapTelemetryMetrics, TelemetryColumns,
        TelemetrySampleRange, read_columns_range, read_lap_metrics_ranges,
    },
    metadata::{LapSummary, SectorSummary, SessionConditions},
    package::{SessionPackageManifest, read_package},
};
use uuid::Uuid;

use crate::shares::{package_filename, validate_package};

#[derive(Clone)]
pub(crate) struct LibraryStore {
    root: PathBuf,
    connection: Arc<Mutex<Connection>>,
    maximum_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LibrarySessionSummary {
    pub(crate) session_id: String,
    pub(crate) title: Option<String>,
    pub(crate) driver_name: Option<String>,
    pub(crate) simulator: String,
    pub(crate) track: Option<String>,
    pub(crate) layout: Option<String>,
    pub(crate) car: Option<String>,
    pub(crate) session_type: Option<String>,
    pub(crate) started_at: String,
    pub(crate) tags: Vec<String>,
    pub(crate) lap_count: u32,
    pub(crate) size_bytes: u64,
    pub(crate) created_at_unix_ms: i64,
    pub(crate) share_id: Option<String>,
}

pub(crate) struct StoredLibrarySession {
    pub(crate) summary: LibrarySessionSummary,
    pub(crate) path: PathBuf,
    pub(crate) download_name: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LibrarySessionDetail {
    pub(crate) summary: LibrarySessionSummary,
    pub(crate) source_track_id: Option<String>,
    pub(crate) source_car_id: Option<String>,
    pub(crate) source_kind: String,
    pub(crate) conditions: SessionConditions,
    pub(crate) laps: Vec<LapSummary>,
    pub(crate) lap_metrics: Vec<LibraryLapMetrics>,
    pub(crate) setup_name: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LibraryLapMetrics {
    pub(crate) lap_id: String,
    pub(crate) lap_index: u32,
    pub(crate) fuel_start_litres: Option<f32>,
    pub(crate) fuel_end_litres: Option<f32>,
    pub(crate) fuel_used_litres: Option<f32>,
    pub(crate) fuel_capacity_litres: Option<f32>,
    pub(crate) max_speed_kmh: Option<f32>,
    pub(crate) tyre_wear_start: [Option<f32>; 4],
    pub(crate) tyre_wear_end: [Option<f32>; 4],
    pub(crate) tyre_wear_minimum: [Option<f32>; 4],
    pub(crate) tyre_flat_observed: [bool; 4],
    pub(crate) wheel_detached_observed: [bool; 4],
    pub(crate) dent_severity_maximum: [u8; 8],
    pub(crate) overheating_observed: bool,
    pub(crate) body_part_detached_observed: bool,
    pub(crate) impact_count: u32,
    pub(crate) maximum_impact_magnitude: Option<f32>,
    pub(crate) virtual_energy_start: Option<f32>,
    pub(crate) virtual_energy_end: Option<f32>,
    pub(crate) virtual_energy_used: Option<f32>,
    pub(crate) electronics: Option<LibraryLapElectronics>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LibraryLapElectronics {
    pub(crate) settings: Vec<LibraryElectronicSetting>,
    pub(crate) tc_active_percent: Option<f32>,
    pub(crate) abs_active_percent: Option<f32>,
    pub(crate) rear_flap_active_percent: Option<f32>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LibraryElectronicSetting {
    pub(crate) key: &'static str,
    pub(crate) label: &'static str,
    pub(crate) start: u8,
    pub(crate) end: u8,
    pub(crate) minimum: u8,
    pub(crate) maximum: u8,
    pub(crate) available_maximum: Option<u8>,
    pub(crate) changes: u32,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct LibraryLapTelemetry {
    pub(crate) lap_id: String,
    pub(crate) source_samples: u64,
    pub(crate) returned_samples: usize,
    pub(crate) points: Vec<LibraryTelemetryPoint>,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct LibraryTelemetryPoint {
    pub(crate) elapsed_seconds: f64,
    pub(crate) sector_index: Option<u32>,
    pub(crate) lap_position: Option<f32>,
    pub(crate) throttle: Option<f32>,
    pub(crate) brake: Option<f32>,
    pub(crate) clutch: Option<f32>,
    pub(crate) speed_kph: Option<f32>,
    pub(crate) engine_rpm: Option<f32>,
    pub(crate) steering_angle_rad: Option<f32>,
    pub(crate) gear: Option<i16>,
    pub(crate) position_x_m: Option<f64>,
    pub(crate) position_z_m: Option<f64>,
    pub(crate) tc_active: Option<bool>,
    pub(crate) abs_active: Option<bool>,
    pub(crate) rear_flap_active: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(crate) struct LibraryCleanupSummary {
    pub(crate) session_count: u64,
    pub(crate) size_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum LibraryStoreError {
    Database,
    Invalid,
    NotFound,
    TooLarge,
    Unavailable,
}

fn library_lap_metrics(summary: &LapSummary, metrics: &LapTelemetryMetrics) -> LibraryLapMetrics {
    let fuel_used_litres = resource_used(
        metrics.fuel_start_litres,
        metrics.fuel_end_litres,
        metrics.fuel_maximum_litres,
    );
    let virtual_energy_used = resource_used(
        metrics.virtual_energy_start,
        metrics.virtual_energy_end,
        metrics.virtual_energy_maximum,
    );
    LibraryLapMetrics {
        lap_id: summary.id.clone(),
        lap_index: summary.index,
        fuel_start_litres: metrics.fuel_start_litres,
        fuel_end_litres: metrics.fuel_end_litres,
        fuel_used_litres,
        fuel_capacity_litres: metrics.fuel_capacity_litres,
        max_speed_kmh: metrics.max_speed_mps.map(|value| value * 3.6),
        tyre_wear_start: metrics.tyre_wear_start,
        tyre_wear_end: metrics.tyre_wear_end,
        tyre_wear_minimum: metrics.tyre_wear_minimum,
        tyre_flat_observed: metrics.tyre_flat_observed,
        wheel_detached_observed: metrics.wheel_detached_observed,
        dent_severity_maximum: metrics.dent_severity_maximum,
        overheating_observed: metrics.overheating_observed,
        body_part_detached_observed: metrics.body_part_detached_observed,
        impact_count: metrics.impact_count,
        maximum_impact_magnitude: metrics.maximum_impact_magnitude,
        virtual_energy_start: metrics.virtual_energy_start,
        virtual_energy_end: metrics.virtual_energy_end,
        virtual_energy_used,
        electronics: library_lap_electronics(metrics.electronics),
    }
}

fn resource_used(
    start: Option<f32>,
    end: Option<f32>,
    observed_maximum: Option<f32>,
) -> Option<f32> {
    start
        .zip(end)
        .filter(|(start, end)| start >= end)
        .filter(|(start, _)| {
            observed_maximum.is_none_or(|maximum| maximum <= *start + f32::EPSILON * 8.0)
        })
        .map(|(start, end)| start - end)
}

fn library_lap_electronics(electronics: LapElectronicsMetrics) -> Option<LibraryLapElectronics> {
    let settings = [
        ("tc", "TC", electronics.tc),
        ("tc_slip", "TC SLIP", electronics.tc_slip),
        ("tc_cut", "TC CUT", electronics.tc_cut),
        ("abs", "ABS", electronics.abs),
        ("motor_map", "MOTOR", electronics.motor_map),
        ("brake_migration", "MIGRATION", electronics.brake_migration),
    ]
    .into_iter()
    .filter_map(|(key, label, setting)| library_electronic_setting(key, label, setting))
    .collect::<Vec<_>>();
    let percentage = |samples: u64| {
        (electronics.observed_samples > 0).then(|| {
            #[allow(clippy::cast_precision_loss)]
            let percent = samples as f32 / electronics.observed_samples as f32 * 100.0;
            percent.clamp(0.0, 100.0)
        })
    };
    let result = LibraryLapElectronics {
        settings,
        tc_active_percent: percentage(electronics.tc_active_samples),
        abs_active_percent: percentage(electronics.abs_active_samples),
        rear_flap_active_percent: percentage(electronics.rear_flap_active_samples),
    };
    (!result.settings.is_empty()
        || electronics.tc_active_samples > 0
        || electronics.abs_active_samples > 0
        || electronics.rear_flap_active_samples > 0)
        .then_some(result)
}

fn library_electronic_setting(
    key: &'static str,
    label: &'static str,
    setting: LapSettingMetrics,
) -> Option<LibraryElectronicSetting> {
    let supported = setting.available_maximum.is_some_and(|maximum| maximum > 0)
        || setting.minimum.is_some_and(|value| value > 0)
        || setting.maximum.is_some_and(|value| value > 0);
    let (start, end, minimum, maximum) = (
        setting.start?,
        setting.end?,
        setting.minimum?,
        setting.maximum?,
    );
    supported.then_some(LibraryElectronicSetting {
        key,
        label,
        start,
        end,
        minimum,
        maximum,
        available_maximum: setting.available_maximum,
        changes: setting.changes,
    })
}

impl LibraryStore {
    pub(crate) fn open(root: &Path, maximum_bytes: u64) -> Result<Self, LibraryStoreError> {
        let library_root = root.join("library");
        fs::create_dir_all(&library_root).map_err(|_| LibraryStoreError::Unavailable)?;
        let connection =
            Connection::open(root.join("live.sqlite")).map_err(|_| LibraryStoreError::Database)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS web_library_sessions (
                     session_id TEXT PRIMARY KEY,
                     account_id TEXT NOT NULL REFERENCES web_accounts(account_id) ON DELETE CASCADE,
                     title TEXT,
                     driver_name TEXT,
                     simulator TEXT NOT NULL,
                     track TEXT,
                     layout TEXT,
                     car TEXT,
                     session_type TEXT,
                     started_at TEXT NOT NULL,
                     tags_json TEXT NOT NULL,
                     lap_count INTEGER NOT NULL,
                     size_bytes INTEGER NOT NULL,
                     download_name TEXT NOT NULL,
                     content_sha256 BLOB,
                     created_at_unix_ms INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS web_library_sessions_owner_created
                     ON web_library_sessions(account_id, created_at_unix_ms DESC);
                 CREATE TABLE IF NOT EXISTS web_library_shares (
                     share_id TEXT PRIMARY KEY,
                     session_id TEXT NOT NULL UNIQUE
                         REFERENCES web_library_sessions(session_id) ON DELETE CASCADE,
                     account_id TEXT NOT NULL
                         REFERENCES web_accounts(account_id) ON DELETE CASCADE,
                     created_at_unix_ms INTEGER NOT NULL
                 );",
            )
            .map_err(|_| LibraryStoreError::Database)?;
        let has_content_digest = connection
            .prepare("PRAGMA table_info(web_library_sessions)")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(1))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|_| LibraryStoreError::Database)?
            .iter()
            .any(|column| column == "content_sha256");
        if !has_content_digest {
            connection
                .execute(
                    "ALTER TABLE web_library_sessions ADD COLUMN content_sha256 BLOB",
                    [],
                )
                .map_err(|_| LibraryStoreError::Database)?;
        }
        connection
            .execute(
                "CREATE UNIQUE INDEX IF NOT EXISTS web_library_sessions_owner_content
                 ON web_library_sessions(account_id, content_sha256)
                 WHERE content_sha256 IS NOT NULL",
                [],
            )
            .map_err(|_| LibraryStoreError::Database)?;
        Ok(Self {
            root: library_root,
            connection: Arc::new(Mutex::new(connection)),
            maximum_bytes,
        })
    }

    pub(crate) fn upload(
        &self,
        account_id: &str,
        contents: &[u8],
        now_unix_ms: i64,
    ) -> Result<LibrarySessionSummary, LibraryStoreError> {
        let size_bytes = u64::try_from(contents.len()).map_err(|_| LibraryStoreError::TooLarge)?;
        if size_bytes == 0 || size_bytes > self.maximum_bytes {
            return Err(LibraryStoreError::TooLarge);
        }
        let manifest =
            validate_package(contents, self.maximum_bytes).map_err(|error| match error {
                crate::shares::ShareStoreError::TooLarge => LibraryStoreError::TooLarge,
                crate::shares::ShareStoreError::Invalid => LibraryStoreError::Invalid,
                crate::shares::ShareStoreError::NotFound => LibraryStoreError::NotFound,
                crate::shares::ShareStoreError::Unavailable => LibraryStoreError::Unavailable,
            })?;
        let content_sha256: [u8; 32] = Sha256::digest(contents).into();
        if let Some(existing) = self.find_by_content_digest(account_id, &content_sha256)? {
            return Ok(existing);
        }
        let session_id = Uuid::new_v4().simple().to_string();
        let owner_root = self.root.join(account_id);
        fs::create_dir_all(&owner_root).map_err(|_| LibraryStoreError::Unavailable)?;
        let temporary = owner_root.join(format!("{session_id}.upload"));
        let destination = owner_root.join(format!("{session_id}.trace"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| LibraryStoreError::Unavailable)?;
        if file
            .write_all(contents)
            .and_then(|()| file.sync_all())
            .and_then(|()| fs::rename(&temporary, &destination))
            .is_err()
        {
            let _ = fs::remove_file(&temporary);
            return Err(LibraryStoreError::Unavailable);
        }

        let summary = summary_from_manifest(&session_id, &manifest, size_bytes, now_unix_ms);
        let tags_json =
            serde_json::to_string(&summary.tags).map_err(|_| LibraryStoreError::Invalid)?;
        let lap_count = i64::from(summary.lap_count);
        let size = i64::try_from(size_bytes).map_err(|_| LibraryStoreError::TooLarge)?;
        let download_name = package_filename(&manifest);
        let inserted = self
            .connection
            .lock()
            .map_err(|_| LibraryStoreError::Database)?
            .execute(
                "INSERT INTO web_library_sessions
                 (session_id, account_id, title, driver_name, simulator, track, layout, car,
                  session_type, started_at, tags_json, lap_count, size_bytes, download_name,
                  created_at_unix_ms, content_sha256)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    summary.session_id,
                    account_id,
                    summary.title,
                    summary.driver_name,
                    summary.simulator,
                    summary.track,
                    summary.layout,
                    summary.car,
                    summary.session_type,
                    summary.started_at,
                    tags_json,
                    lap_count,
                    size,
                    download_name,
                    summary.created_at_unix_ms,
                    content_sha256.as_slice()
                ],
            );
        if inserted.is_err() {
            let _ = fs::remove_file(destination);
            if let Some(existing) = self.find_by_content_digest(account_id, &content_sha256)? {
                return Ok(existing);
            }
            return Err(LibraryStoreError::Database);
        }
        Ok(summary)
    }

    fn find_by_content_digest(
        &self,
        account_id: &str,
        content_sha256: &[u8; 32],
    ) -> Result<Option<LibrarySessionSummary>, LibraryStoreError> {
        self.connection
            .lock()
            .map_err(|_| LibraryStoreError::Database)?
            .query_row(
                "SELECT session_id, title, driver_name, simulator, track, layout, car,
                        session_type, started_at, tags_json, lap_count, size_bytes,
                        created_at_unix_ms,
                        (SELECT share_id FROM web_library_shares
                         WHERE web_library_shares.session_id = web_library_sessions.session_id)
                 FROM web_library_sessions
                 WHERE account_id = ?1 AND content_sha256 = ?2",
                params![account_id, content_sha256.as_slice()],
                summary_from_row,
            )
            .optional()
            .map_err(|_| LibraryStoreError::Database)
    }

    pub(crate) fn list(
        &self,
        account_id: &str,
    ) -> Result<Vec<LibrarySessionSummary>, LibraryStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| LibraryStoreError::Database)?;
        let mut statement = connection
            .prepare(
                "SELECT session_id, title, driver_name, simulator, track, layout, car,
                        session_type, started_at, tags_json, lap_count, size_bytes,
                        created_at_unix_ms,
                        (SELECT share_id FROM web_library_shares
                         WHERE web_library_shares.session_id = web_library_sessions.session_id)
                 FROM web_library_sessions WHERE account_id = ?1
                 ORDER BY created_at_unix_ms DESC, session_id DESC",
            )
            .map_err(|_| LibraryStoreError::Database)?;
        let rows = statement
            .query_map([account_id], summary_from_row)
            .map_err(|_| LibraryStoreError::Database)?;
        rows.map(|row| row.map_err(|_| LibraryStoreError::Database))
            .collect()
    }

    pub(crate) fn find(
        &self,
        account_id: &str,
        session_id: &str,
    ) -> Result<StoredLibrarySession, LibraryStoreError> {
        if !valid_identifier(session_id) {
            return Err(LibraryStoreError::NotFound);
        }
        let stored = self
            .connection
            .lock()
            .map_err(|_| LibraryStoreError::Database)?
            .query_row(
                "SELECT session_id, title, driver_name, simulator, track, layout, car,
                        session_type, started_at, tags_json, lap_count, size_bytes,
                        created_at_unix_ms,
                        (SELECT share_id FROM web_library_shares
                         WHERE web_library_shares.session_id = web_library_sessions.session_id),
                        download_name
                 FROM web_library_sessions WHERE account_id = ?1 AND session_id = ?2",
                [account_id, session_id],
                |row| Ok((summary_from_row(row)?, row.get::<_, String>(14)?)),
            )
            .optional()
            .map_err(|_| LibraryStoreError::Database)?
            .ok_or(LibraryStoreError::NotFound)?;
        let path = self
            .root
            .join(account_id)
            .join(format!("{session_id}.trace"));
        if !path.is_file() {
            return Err(LibraryStoreError::Unavailable);
        }
        Ok(StoredLibrarySession {
            summary: stored.0,
            path,
            download_name: stored.1,
        })
    }

    pub(crate) fn delete(
        &self,
        account_id: &str,
        session_id: &str,
    ) -> Result<(), LibraryStoreError> {
        let stored = self.find(account_id, session_id)?;
        let deleting = stored.path.with_extension("deleting");
        fs::rename(&stored.path, &deleting).map_err(|_| LibraryStoreError::Unavailable)?;
        let deleted = self
            .connection
            .lock()
            .map_err(|_| LibraryStoreError::Database)?
            .execute(
                "DELETE FROM web_library_sessions WHERE account_id = ?1 AND session_id = ?2",
                [account_id, session_id],
            );
        if deleted != Ok(1) {
            let _ = fs::rename(&deleting, &stored.path);
            return Err(LibraryStoreError::Database);
        }
        fs::remove_file(deleting).map_err(|_| LibraryStoreError::Unavailable)
    }

    pub(crate) fn update_metadata(
        &self,
        account_id: &str,
        session_id: &str,
        title: Option<&str>,
        driver_name: Option<&str>,
        tags: &[String],
    ) -> Result<LibrarySessionSummary, LibraryStoreError> {
        if !valid_identifier(session_id) {
            return Err(LibraryStoreError::NotFound);
        }
        let title = normalized_metadata_value(title, 120)?;
        let driver_name = normalized_metadata_value(driver_name, 80)?;
        let tags = normalized_tags(tags)?;
        let tags_json = serde_json::to_string(&tags).map_err(|_| LibraryStoreError::Invalid)?;
        let updated = self
            .connection
            .lock()
            .map_err(|_| LibraryStoreError::Database)?
            .execute(
                "UPDATE web_library_sessions
                 SET title = ?3, driver_name = ?4, tags_json = ?5
                 WHERE account_id = ?1 AND session_id = ?2",
                params![account_id, session_id, title, driver_name, tags_json],
            )
            .map_err(|_| LibraryStoreError::Database)?;
        if updated != 1 {
            return Err(LibraryStoreError::NotFound);
        }
        self.find(account_id, session_id)
            .map(|stored| stored.summary)
    }

    pub(crate) fn preview_retention(
        &self,
        account_id: Option<&str>,
        now_unix_ms: i64,
        retention_days: u32,
        maximum_sessions_per_account: u32,
    ) -> Result<LibraryCleanupSummary, LibraryStoreError> {
        let candidates = self.retention_candidates(
            account_id,
            now_unix_ms,
            retention_days,
            maximum_sessions_per_account,
            None,
        )?;
        Ok(LibraryCleanupSummary {
            session_count: u64::try_from(candidates.len()).unwrap_or(u64::MAX),
            size_bytes: candidates
                .iter()
                .fold(0_u64, |total, candidate| total.saturating_add(candidate.2)),
        })
    }

    pub(crate) fn enforce_retention(
        &self,
        account_id: Option<&str>,
        now_unix_ms: i64,
        retention_days: u32,
        maximum_sessions_per_account: u32,
        protected_session_id: Option<&str>,
    ) -> Result<LibraryCleanupSummary, LibraryStoreError> {
        let candidates = self.retention_candidates(
            account_id,
            now_unix_ms,
            retention_days,
            maximum_sessions_per_account,
            protected_session_id,
        )?;
        let mut cleanup = LibraryCleanupSummary::default();
        for (owner_id, session_id, size_bytes) in candidates {
            self.delete(&owner_id, &session_id)?;
            cleanup.session_count = cleanup.session_count.saturating_add(1);
            cleanup.size_bytes = cleanup.size_bytes.saturating_add(size_bytes);
        }
        Ok(cleanup)
    }

    fn retention_candidates(
        &self,
        account_id: Option<&str>,
        now_unix_ms: i64,
        retention_days: u32,
        maximum_sessions_per_account: u32,
        protected_session_id: Option<&str>,
    ) -> Result<Vec<(String, String, u64)>, LibraryStoreError> {
        if retention_days == 0 && maximum_sessions_per_account == 0 {
            return Ok(Vec::new());
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| LibraryStoreError::Database)?;
        let mut statement = connection
            .prepare(
                "SELECT account_id, session_id, size_bytes, created_at_unix_ms
                 FROM web_library_sessions
                 ORDER BY account_id, created_at_unix_ms DESC, session_id DESC",
            )
            .map_err(|_| LibraryStoreError::Database)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|_| LibraryStoreError::Database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| LibraryStoreError::Database)?;
        let age_ms = i64::from(retention_days).saturating_mul(24 * 60 * 60 * 1_000);
        let cutoff = now_unix_ms.saturating_sub(age_ms);
        let mut owner_counts = HashMap::<String, u32>::new();
        let mut selected = HashSet::new();
        let mut candidates = Vec::new();
        let mut rows = rows;
        rows.sort_by(|left, right| {
            left.0.cmp(&right.0).then_with(|| {
                let left_protected = protected_session_id == Some(left.1.as_str());
                let right_protected = protected_session_id == Some(right.1.as_str());
                right_protected
                    .cmp(&left_protected)
                    .then_with(|| right.3.cmp(&left.3))
                    .then_with(|| right.1.cmp(&left.1))
            })
        });
        for (owner_id, session_id, size_bytes, created_at_unix_ms) in rows {
            if account_id.is_some_and(|expected| expected != owner_id) {
                continue;
            }
            let count = owner_counts.entry(owner_id.clone()).or_default();
            *count = count.saturating_add(1);
            let expired_by_age = retention_days > 0 && created_at_unix_ms < cutoff;
            let exceeds_count =
                maximum_sessions_per_account > 0 && *count > maximum_sessions_per_account;
            if (expired_by_age || exceeds_count) && selected.insert(session_id.clone()) {
                candidates.push((
                    owner_id,
                    session_id,
                    u64::try_from(size_bytes).unwrap_or_default(),
                ));
            }
        }
        Ok(candidates)
    }

    pub(crate) fn detail(
        &self,
        account_id: &str,
        session_id: &str,
    ) -> Result<LibrarySessionDetail, LibraryStoreError> {
        let stored = self.find(account_id, session_id)?;
        let package = open_package(&stored.path, self.maximum_bytes)?;
        let setup_name = package
            .manifest
            .confirmed_setup
            .as_ref()
            .map(|setup| setup.name.clone());
        let metric_laps = package
            .manifest
            .laps
            .iter()
            .filter(|lap| lap.sample_count > 0)
            .collect::<Vec<_>>();
        let metric_ranges = metric_laps
            .iter()
            .map(|lap| TelemetrySampleRange {
                sample_start: lap.sample_start,
                sample_count: lap.sample_count,
            })
            .collect::<Vec<_>>();
        let lap_metrics = if metric_ranges.is_empty() {
            Vec::new()
        } else {
            read_lap_metrics_ranges(package.telemetry, &metric_ranges)
                .map(|metrics| {
                    metric_laps
                        .into_iter()
                        .zip(metrics)
                        .map(|(lap, metrics)| library_lap_metrics(&lap.summary, &metrics))
                        .collect()
                })
                .unwrap_or_default()
        };
        Ok(LibrarySessionDetail {
            summary: stored.summary,
            source_track_id: package.manifest.session.source_track_id.clone(),
            source_car_id: package.manifest.session.source_car_id.clone(),
            source_kind: package.manifest.session.source_kind.clone(),
            conditions: package.manifest.session.conditions.clone(),
            laps: package
                .manifest
                .laps
                .iter()
                .map(|lap| lap.summary.clone())
                .collect(),
            lap_metrics,
            setup_name,
        })
    }

    pub(crate) fn lap_telemetry(
        &self,
        account_id: &str,
        session_id: &str,
        lap_id: &str,
        maximum_points: usize,
    ) -> Result<LibraryLapTelemetry, LibraryStoreError> {
        let stored = self.find(account_id, session_id)?;
        let package = open_package(&stored.path, self.maximum_bytes)?;
        let lap = package
            .manifest
            .laps
            .iter()
            .find(|lap| lap.summary.id == lap_id)
            .ok_or(LibraryStoreError::NotFound)?;
        let sample_start = lap.sample_start;
        let sample_count = lap.sample_count;
        let lap_id = lap.summary.id.clone();
        let columns = read_columns_range(package.telemetry, sample_start, sample_count)
            .map_err(|_| LibraryStoreError::Invalid)?;
        let indices = sampled_indices(columns.len(), maximum_points);
        let elapsed_start = columns.elapsed_ns.first().copied().unwrap_or_default();
        let points = indices
            .into_iter()
            .map(|index| telemetry_point(&columns, index, elapsed_start, &lap.summary.sectors))
            .collect::<Vec<_>>();
        Ok(LibraryLapTelemetry {
            lap_id,
            source_samples: sample_count,
            returned_samples: points.len(),
            points,
        })
    }

    pub(crate) fn share(
        &self,
        account_id: &str,
        session_id: &str,
        now_unix_ms: i64,
    ) -> Result<(String, LibrarySessionSummary), LibraryStoreError> {
        let summary = self.find(account_id, session_id)?.summary;
        let connection = self
            .connection
            .lock()
            .map_err(|_| LibraryStoreError::Database)?;
        if let Some(share_id) = connection
            .query_row(
                "SELECT share_id FROM web_library_shares
                 WHERE account_id = ?1 AND session_id = ?2",
                [account_id, session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| LibraryStoreError::Database)?
        {
            return Ok((share_id, summary));
        }
        let share_id = Uuid::new_v4().simple().to_string();
        connection
            .execute(
                "INSERT INTO web_library_shares
                 (share_id, session_id, account_id, created_at_unix_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![share_id, session_id, account_id, now_unix_ms],
            )
            .map_err(|_| LibraryStoreError::Database)?;
        Ok((share_id, summary))
    }

    pub(crate) fn revoke_share(
        &self,
        account_id: &str,
        session_id: &str,
    ) -> Result<(), LibraryStoreError> {
        self.find(account_id, session_id)?;
        self.connection
            .lock()
            .map_err(|_| LibraryStoreError::Database)?
            .execute(
                "DELETE FROM web_library_shares WHERE account_id = ?1 AND session_id = ?2",
                [account_id, session_id],
            )
            .map_err(|_| LibraryStoreError::Database)?;
        Ok(())
    }

    pub(crate) fn public_detail(
        &self,
        share_id: &str,
    ) -> Result<LibrarySessionDetail, LibraryStoreError> {
        let (account_id, session_id) = self.public_owner(share_id)?;
        self.detail(&account_id, &session_id)
    }

    pub(crate) fn public_lap_telemetry(
        &self,
        share_id: &str,
        lap_id: &str,
        maximum_points: usize,
    ) -> Result<LibraryLapTelemetry, LibraryStoreError> {
        let (account_id, session_id) = self.public_owner(share_id)?;
        self.lap_telemetry(&account_id, &session_id, lap_id, maximum_points)
    }

    pub(crate) fn public_session(
        &self,
        share_id: &str,
    ) -> Result<StoredLibrarySession, LibraryStoreError> {
        let (account_id, session_id) = self.public_owner(share_id)?;
        self.find(&account_id, &session_id)
    }

    fn public_owner(&self, share_id: &str) -> Result<(String, String), LibraryStoreError> {
        if !valid_identifier(share_id) {
            return Err(LibraryStoreError::NotFound);
        }
        self.connection
            .lock()
            .map_err(|_| LibraryStoreError::Database)?
            .query_row(
                "SELECT account_id, session_id FROM web_library_shares WHERE share_id = ?1",
                [share_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|_| LibraryStoreError::Database)?
            .ok_or(LibraryStoreError::NotFound)
    }
}

fn normalized_metadata_value(
    value: Option<&str>,
    maximum_characters: usize,
) -> Result<Option<String>, LibraryStoreError> {
    let value = value.map(str::trim).filter(|value| !value.is_empty());
    if value.is_some_and(|value| {
        value.chars().count() > maximum_characters || value.chars().any(char::is_control)
    }) {
        return Err(LibraryStoreError::Invalid);
    }
    Ok(value.map(str::to_owned))
}

fn normalized_tags(tags: &[String]) -> Result<Vec<String>, LibraryStoreError> {
    if tags.len() > 12 {
        return Err(LibraryStoreError::Invalid);
    }
    let mut normalized = Vec::with_capacity(tags.len());
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        if tag.chars().count() > 40 || tag.chars().any(char::is_control) {
            return Err(LibraryStoreError::Invalid);
        }
        if !normalized.iter().any(|existing| existing == tag) {
            normalized.push(tag.to_owned());
        }
    }
    Ok(normalized)
}

fn open_package(
    path: &Path,
    maximum_bytes: u64,
) -> Result<trace_storage::package::SessionPackage<File>, LibraryStoreError> {
    let file = File::open(path).map_err(|_| LibraryStoreError::Unavailable)?;
    let length = file
        .metadata()
        .map_err(|_| LibraryStoreError::Unavailable)?
        .len();
    read_package(file, length, maximum_bytes).map_err(|_| LibraryStoreError::Invalid)
}

fn sampled_indices(sample_count: usize, maximum_points: usize) -> Vec<usize> {
    let maximum_points = maximum_points.max(2);
    if sample_count <= maximum_points {
        return (0..sample_count).collect();
    }
    let last = sample_count.saturating_sub(1);
    let denominator = maximum_points.saturating_sub(1);
    (0..maximum_points)
        .map(|index| index.saturating_mul(last) / denominator)
        .collect()
}

fn telemetry_point(
    columns: &TelemetryColumns,
    index: usize,
    elapsed_start: u64,
    sectors: &[SectorSummary],
) -> LibraryTelemetryPoint {
    let elapsed_ns = columns.elapsed_ns[index].saturating_sub(elapsed_start);
    let mut sector_end_ns = 0_u64;
    let sector_index = sectors
        .iter()
        .find(|sector| {
            sector_end_ns = sector_end_ns.saturating_add(sector.duration_ns);
            elapsed_ns < sector_end_ns
        })
        .or_else(|| sectors.last())
        .map(|sector| sector.index);
    LibraryTelemetryPoint {
        elapsed_seconds: std::time::Duration::from_nanos(elapsed_ns).as_secs_f64(),
        sector_index,
        lap_position: columns.lap_position[index],
        throttle: columns.throttle[index],
        brake: columns.brake[index],
        clutch: columns.clutch[index],
        speed_kph: columns.speed_mps[index].map(|speed| speed * 3.6),
        engine_rpm: columns.engine_rpm[index],
        steering_angle_rad: columns.steering_angle_rad[index],
        gear: columns.gear_value[index],
        position_x_m: columns.position_x_m[index],
        position_z_m: columns.position_z_m[index],
        tc_active: columns.tc_active[index],
        abs_active: columns.abs_active[index],
        rear_flap_active: columns.rear_flap_active[index],
    }
}

fn summary_from_manifest(
    session_id: &str,
    manifest: &SessionPackageManifest,
    size_bytes: u64,
    created_at_unix_ms: i64,
) -> LibrarySessionSummary {
    LibrarySessionSummary {
        session_id: session_id.to_owned(),
        title: manifest.session.user_title.clone(),
        driver_name: manifest.session.user_driver.clone(),
        simulator: manifest.session.simulator_key.clone(),
        track: manifest.session.track.clone(),
        layout: manifest.session.layout_id.clone(),
        car: manifest.session.car.clone(),
        session_type: manifest.session.session_type.clone(),
        started_at: manifest.session.started_at.clone(),
        tags: manifest.session.tags.clone(),
        lap_count: u32::try_from(manifest.laps.len()).unwrap_or(u32::MAX),
        size_bytes,
        created_at_unix_ms,
        share_id: None,
    }
}

fn summary_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LibrarySessionSummary> {
    let tags_json = row.get::<_, String>(9)?;
    let tags = serde_json::from_str(&tags_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(error))
    })?;
    let lap_count = u32::try_from(row.get::<_, i64>(10)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            10,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    let size_bytes = u64::try_from(row.get::<_, i64>(11)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            11,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })?;
    Ok(LibrarySessionSummary {
        session_id: row.get(0)?,
        title: row.get(1)?,
        driver_name: row.get(2)?,
        simulator: row.get(3)?,
        track: row.get(4)?,
        layout: row.get(5)?,
        car: row.get(6)?,
        session_type: row.get(7)?,
        started_at: row.get(8)?,
        tags,
        lap_count,
        size_bytes,
        created_at_unix_ms: row.get(12)?,
        share_id: row.get(13)?,
    })
}

fn valid_identifier(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
