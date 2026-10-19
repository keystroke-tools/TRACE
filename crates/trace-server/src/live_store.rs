use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use rusqlite::{Connection, params};
use trace_protocol::{Envelope, SessionState};

#[derive(Clone)]
pub(crate) struct LiveStore {
    connection: Arc<Mutex<Connection>>,
    retention: Duration,
}

pub(crate) struct StoredInstallation {
    pub(crate) installation_id: String,
    pub(crate) token_hash: [u8; 32],
}

pub(crate) struct StoredSession {
    pub(crate) session_id: String,
    pub(crate) owner_id: String,
    pub(crate) state: SessionState,
    pub(crate) last_sequence: Option<u64>,
    pub(crate) updated_at_unix_ms: i64,
    pub(crate) publisher_token_hash: Option<[u8; 32]>,
    pub(crate) messages: Vec<(Envelope, i64)>,
}

#[derive(Debug)]
pub(crate) enum LiveStoreError {
    Io,
    Database,
    InvalidData,
}

impl LiveStore {
    pub(crate) fn open(root: &Path, retention: Duration) -> Result<Self, LiveStoreError> {
        fs::create_dir_all(root).map_err(|_| LiveStoreError::Io)?;
        let connection =
            Connection::open(root.join("live.sqlite")).map_err(|_| LiveStoreError::Database)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS live_installations (
                     installation_id TEXT PRIMARY KEY,
                     token_hash BLOB NOT NULL,
                     created_at_unix_ms INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS live_sessions (
                     session_id TEXT PRIMARY KEY,
                     owner_id TEXT NOT NULL REFERENCES live_installations(installation_id),
                     state_json TEXT NOT NULL,
                     last_sequence INTEGER,
                     publisher_token_hash BLOB,
                     updated_at_unix_ms INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS live_messages (
                     session_id TEXT NOT NULL REFERENCES live_sessions(session_id) ON DELETE CASCADE,
                     sequence INTEGER NOT NULL,
                     received_at_unix_ms INTEGER NOT NULL,
                     envelope_json TEXT NOT NULL,
                     PRIMARY KEY (session_id, sequence)
                 );
                 CREATE INDEX IF NOT EXISTS live_messages_received
                     ON live_messages(received_at_unix_ms);",
            )
            .map_err(|_| LiveStoreError::Database)?;
        ensure_publisher_token_column(&connection)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            retention,
        })
    }

    pub(crate) fn load(
        &self,
        now_unix_ms: i64,
    ) -> Result<(Vec<StoredInstallation>, Vec<StoredSession>), LiveStoreError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| LiveStoreError::Database)?;
        let transaction = connection
            .transaction()
            .map_err(|_| LiveStoreError::Database)?;
        prune(&transaction, cutoff(now_unix_ms, self.retention))?;

        let installations = {
            let mut statement = transaction
                .prepare("SELECT installation_id, token_hash FROM live_installations")
                .map_err(|_| LiveStoreError::Database)?;
            let rows = statement
                .query_map([], |row| {
                    let hash = row.get::<_, Vec<u8>>(1)?;
                    Ok((row.get::<_, String>(0)?, hash))
                })
                .map_err(|_| LiveStoreError::Database)?;
            rows.map(|row| {
                let (installation_id, hash) = row.map_err(|_| LiveStoreError::Database)?;
                let token_hash = hash.try_into().map_err(|_| LiveStoreError::InvalidData)?;
                Ok(StoredInstallation {
                    installation_id,
                    token_hash,
                })
            })
            .collect::<Result<Vec<_>, LiveStoreError>>()?
        };

        let sessions = {
            let mut statement = transaction
                .prepare(
                    "SELECT session_id, owner_id, state_json, last_sequence, updated_at_unix_ms,
                            publisher_token_hash
                     FROM live_sessions ORDER BY updated_at_unix_ms",
                )
                .map_err(|_| LiveStoreError::Database)?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<Vec<u8>>>(5)?,
                    ))
                })
                .map_err(|_| LiveStoreError::Database)?;
            rows.map(|row| {
                let (
                    session_id,
                    owner_id,
                    state_json,
                    last_sequence,
                    updated_at_unix_ms,
                    publisher_token_hash,
                ) = row.map_err(|_| LiveStoreError::Database)?;
                let state =
                    serde_json::from_str(&state_json).map_err(|_| LiveStoreError::InvalidData)?;
                let last_sequence = last_sequence
                    .map(|value| u64::try_from(value).map_err(|_| LiveStoreError::InvalidData))
                    .transpose()?;
                let messages = load_messages(&transaction, &session_id)?;
                let publisher_token_hash = publisher_token_hash
                    .map(|hash| hash.try_into().map_err(|_| LiveStoreError::InvalidData))
                    .transpose()?;
                Ok(StoredSession {
                    session_id,
                    owner_id,
                    state,
                    last_sequence,
                    updated_at_unix_ms,
                    publisher_token_hash,
                    messages,
                })
            })
            .collect::<Result<Vec<_>, LiveStoreError>>()?
        };
        transaction.commit().map_err(|_| LiveStoreError::Database)?;
        Ok((installations, sessions))
    }

    pub(crate) fn save_installation(
        &self,
        installation_id: &str,
        token_hash: &[u8; 32],
        now_unix_ms: i64,
    ) -> Result<(), LiveStoreError> {
        self.connection
            .lock()
            .map_err(|_| LiveStoreError::Database)?
            .execute(
                "INSERT INTO live_installations
                 (installation_id, token_hash, created_at_unix_ms) VALUES (?1, ?2, ?3)",
                params![installation_id, token_hash.as_slice(), now_unix_ms],
            )
            .map_err(|_| LiveStoreError::Database)?;
        Ok(())
    }

    pub(crate) fn save_session(
        &self,
        session_id: &str,
        owner_id: &str,
        state: &SessionState,
        publisher_token_hash: &[u8; 32],
        now_unix_ms: i64,
    ) -> Result<(), LiveStoreError> {
        let state_json = serde_json::to_string(state).map_err(|_| LiveStoreError::InvalidData)?;
        self.connection
            .lock()
            .map_err(|_| LiveStoreError::Database)?
            .execute(
                "INSERT INTO live_sessions
                 (session_id, owner_id, state_json, last_sequence, publisher_token_hash,
                  updated_at_unix_ms)
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5)",
                params![
                    session_id,
                    owner_id,
                    state_json,
                    publisher_token_hash.as_slice(),
                    now_unix_ms
                ],
            )
            .map_err(|_| LiveStoreError::Database)?;
        Ok(())
    }

    pub(crate) fn save_message(
        &self,
        session_id: &str,
        state: &SessionState,
        message: &Envelope,
        received_at_unix_ms: i64,
        revoke_publisher: bool,
    ) -> Result<(), LiveStoreError> {
        let state_json = serde_json::to_string(state).map_err(|_| LiveStoreError::InvalidData)?;
        let envelope_json =
            serde_json::to_string(message).map_err(|_| LiveStoreError::InvalidData)?;
        let sequence = i64::try_from(message.sequence).map_err(|_| LiveStoreError::InvalidData)?;
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| LiveStoreError::Database)?;
        let transaction = connection
            .transaction()
            .map_err(|_| LiveStoreError::Database)?;
        transaction
            .execute(
                "INSERT INTO live_messages
                 (session_id, sequence, received_at_unix_ms, envelope_json)
                 VALUES (?1, ?2, ?3, ?4)",
                params![session_id, sequence, received_at_unix_ms, envelope_json],
            )
            .map_err(|_| LiveStoreError::Database)?;
        transaction
            .execute(
                "UPDATE live_sessions SET state_json = ?2, last_sequence = ?3,
                 updated_at_unix_ms = ?4,
                 publisher_token_hash = CASE WHEN ?5 THEN NULL ELSE publisher_token_hash END
                 WHERE session_id = ?1",
                params![
                    session_id,
                    state_json,
                    sequence,
                    received_at_unix_ms,
                    revoke_publisher
                ],
            )
            .map_err(|_| LiveStoreError::Database)?;
        prune(&transaction, cutoff(received_at_unix_ms, self.retention))?;
        transaction.commit().map_err(|_| LiveStoreError::Database)?;
        Ok(())
    }

    pub(crate) fn set_publisher_token_hash(
        &self,
        session_id: &str,
        publisher_token_hash: Option<&[u8; 32]>,
    ) -> Result<(), LiveStoreError> {
        self.connection
            .lock()
            .map_err(|_| LiveStoreError::Database)?
            .execute(
                "UPDATE live_sessions SET publisher_token_hash = ?2 WHERE session_id = ?1",
                params![session_id, publisher_token_hash.map(<[u8; 32]>::as_slice)],
            )
            .map_err(|_| LiveStoreError::Database)?;
        Ok(())
    }

    pub(crate) fn delete_session(&self, session_id: &str) -> Result<(), LiveStoreError> {
        self.connection
            .lock()
            .map_err(|_| LiveStoreError::Database)?
            .execute(
                "DELETE FROM live_sessions WHERE session_id = ?1",
                [session_id],
            )
            .map_err(|_| LiveStoreError::Database)?;
        Ok(())
    }

    pub(crate) fn delete_installation(&self, installation_id: &str) -> Result<(), LiveStoreError> {
        self.connection
            .lock()
            .map_err(|_| LiveStoreError::Database)?
            .execute(
                "DELETE FROM live_installations WHERE installation_id = ?1",
                [installation_id],
            )
            .map_err(|_| LiveStoreError::Database)?;
        Ok(())
    }
}

fn ensure_publisher_token_column(connection: &Connection) -> Result<(), LiveStoreError> {
    let exists = connection
        .query_row(
            "SELECT EXISTS(
                SELECT 1 FROM pragma_table_info('live_sessions')
                WHERE name = 'publisher_token_hash'
            )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|_| LiveStoreError::Database)?;
    if !exists {
        connection
            .execute(
                "ALTER TABLE live_sessions ADD COLUMN publisher_token_hash BLOB",
                [],
            )
            .map_err(|_| LiveStoreError::Database)?;
    }
    Ok(())
}

fn load_messages(
    connection: &Connection,
    session_id: &str,
) -> Result<Vec<(Envelope, i64)>, LiveStoreError> {
    let mut statement = connection
        .prepare(
            "SELECT envelope_json, received_at_unix_ms FROM live_messages
             WHERE session_id = ?1 ORDER BY sequence",
        )
        .map_err(|_| LiveStoreError::Database)?;
    let rows = statement
        .query_map([session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|_| LiveStoreError::Database)?;
    rows.map(|row| {
        let (json, received_at) = row.map_err(|_| LiveStoreError::Database)?;
        let envelope = serde_json::from_str(&json).map_err(|_| LiveStoreError::InvalidData)?;
        Ok((envelope, received_at))
    })
    .collect()
}

fn prune(connection: &Connection, cutoff_unix_ms: i64) -> Result<(), LiveStoreError> {
    connection
        .execute(
            "DELETE FROM live_messages WHERE received_at_unix_ms < ?1",
            [cutoff_unix_ms],
        )
        .map_err(|_| LiveStoreError::Database)?;
    connection
        .execute(
            "DELETE FROM live_sessions WHERE updated_at_unix_ms < ?1",
            [cutoff_unix_ms],
        )
        .map_err(|_| LiveStoreError::Database)?;
    Ok(())
}

fn cutoff(now_unix_ms: i64, retention: Duration) -> i64 {
    let retention_ms = i64::try_from(retention.as_millis()).unwrap_or(i64::MAX);
    now_unix_ms.saturating_sub(retention_ms)
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use trace_protocol::{Hello, LiveStatus, PROTOCOL_VERSION, Payload};

    use super::*;

    #[test]
    fn expired_live_history_is_removed_on_load() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store =
            LiveStore::open(directory.path(), Duration::from_millis(10)).expect("live store");
        let token_hash = [7_u8; 32];
        store
            .save_installation("installation", &token_hash, 1_000)
            .expect("installation");
        let state = SessionState {
            driver_name: Some("Driver".to_owned()),
            simulator: "assetto-corsa".to_owned(),
            simulator_name: Some("Assetto Corsa".to_owned()),
            simulator_mark: Some("AC".to_owned()),
            car: Some("car".to_owned()),
            track: Some("track".to_owned()),
            layout: None,
            session_type: Some("race".to_owned()),
            status: LiveStatus::Live,
        };
        store
            .save_session("session", "installation", &state, &[9_u8; 32], 1_000)
            .expect("session");
        let envelope = Envelope {
            protocol_version: PROTOCOL_VERSION,
            message_id: "message".to_owned(),
            session_id: "session".to_owned(),
            sequence: 1,
            sent_at_unix_ms: 1_000,
            payload: Payload::Hello(Hello {
                publisher_version: "test".to_owned(),
                source: "test".to_owned(),
            }),
        };
        store
            .save_message("session", &state, &envelope, 1_000, false)
            .expect("message");

        let (installations, sessions) = store.load(1_011).expect("reload");
        assert_eq!(installations.len(), 1);
        assert!(sessions.is_empty());
    }

    #[test]
    fn existing_database_gains_the_session_publisher_column() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("live.sqlite");
        let connection = Connection::open(&path).expect("legacy database");
        connection
            .execute_batch(
                "CREATE TABLE live_sessions (
                    session_id TEXT PRIMARY KEY,
                    owner_id TEXT NOT NULL,
                    state_json TEXT NOT NULL,
                    last_sequence INTEGER,
                    updated_at_unix_ms INTEGER NOT NULL
                );",
            )
            .expect("legacy schema");
        drop(connection);

        let store = LiveStore::open(directory.path(), Duration::from_secs(600))
            .expect("migrated live store");
        let connection = store.connection.lock().expect("connection");
        let exists = connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM pragma_table_info('live_sessions')
                    WHERE name = 'publisher_token_hash'
                )",
                [],
                |row| row.get::<_, bool>(0),
            )
            .expect("column query");
        assert!(exists);
    }
}
