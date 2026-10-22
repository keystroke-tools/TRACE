use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use trace_storage::package::SessionPackageManifest;
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
}

pub(crate) struct StoredLibrarySession {
    pub(crate) summary: LibrarySessionSummary,
    pub(crate) path: PathBuf,
    pub(crate) download_name: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum LibraryStoreError {
    Database,
    Invalid,
    NotFound,
    TooLarge,
    Unavailable,
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
                     created_at_unix_ms INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS web_library_sessions_owner_created
                     ON web_library_sessions(account_id, created_at_unix_ms DESC);",
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
                  created_at_unix_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
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
                    summary.created_at_unix_ms
                ],
            );
        if inserted.is_err() {
            let _ = fs::remove_file(destination);
            return Err(LibraryStoreError::Database);
        }
        Ok(summary)
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
                        created_at_unix_ms
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
                        created_at_unix_ms, download_name
                 FROM web_library_sessions WHERE account_id = ?1 AND session_id = ?2",
                [account_id, session_id],
                |row| Ok((summary_from_row(row)?, row.get::<_, String>(13)?)),
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
    })
}

fn valid_identifier(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
