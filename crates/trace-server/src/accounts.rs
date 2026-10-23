use std::{
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const SESSION_LIFETIME_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

#[derive(Clone)]
pub(crate) struct AccountStore {
    connection: Arc<Mutex<Connection>>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AccountSummary {
    pub(crate) account_id: String,
    pub(crate) username: String,
    pub(crate) display_name: String,
    pub(crate) administrator: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AccountTokenSummary {
    pub(crate) token_id: String,
    pub(crate) label: String,
    pub(crate) token_prefix: String,
    pub(crate) created_at_unix_ms: i64,
    pub(crate) last_used_at_unix_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct CreatedAccountToken {
    #[serde(flatten)]
    pub(crate) summary: AccountTokenSummary,
    pub(crate) token: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum AccountStoreError {
    Database,
    InvalidCredentials,
    UsernameTaken,
}

impl AccountStore {
    pub(crate) fn open(root: &Path) -> Result<Self, AccountStoreError> {
        fs::create_dir_all(root).map_err(|_| AccountStoreError::Database)?;
        let connection =
            Connection::open(root.join("live.sqlite")).map_err(|_| AccountStoreError::Database)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS web_accounts (
                     account_id TEXT PRIMARY KEY,
                     username TEXT NOT NULL UNIQUE COLLATE NOCASE,
                     display_name TEXT NOT NULL,
                     password_hash TEXT NOT NULL,
                     administrator INTEGER NOT NULL CHECK (administrator IN (0, 1)),
                     created_at_unix_ms INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS web_account_sessions (
                     token_hash BLOB PRIMARY KEY,
                     account_id TEXT NOT NULL REFERENCES web_accounts(account_id) ON DELETE CASCADE,
                     created_at_unix_ms INTEGER NOT NULL,
                     expires_at_unix_ms INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS web_account_sessions_expiry
                     ON web_account_sessions(expires_at_unix_ms);
                 CREATE TABLE IF NOT EXISTS web_account_tokens (
                     token_id TEXT PRIMARY KEY,
                     token_hash BLOB NOT NULL UNIQUE,
                     account_id TEXT NOT NULL REFERENCES web_accounts(account_id) ON DELETE CASCADE,
                     label TEXT NOT NULL,
                     token_prefix TEXT NOT NULL,
                     created_at_unix_ms INTEGER NOT NULL,
                     last_used_at_unix_ms INTEGER
                 );
                 CREATE INDEX IF NOT EXISTS web_account_tokens_account
                     ON web_account_tokens(account_id, created_at_unix_ms DESC);",
            )
            .map_err(|_| AccountStoreError::Database)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub(crate) fn register(
        &self,
        username: &str,
        display_name: &str,
        password: &str,
        now_unix_ms: i64,
    ) -> Result<AccountSummary, AccountStoreError> {
        let password_hash = hash_password(password)?;
        let account_id = Uuid::new_v4().simple().to_string();
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| AccountStoreError::Database)?;
        let transaction = connection
            .transaction()
            .map_err(|_| AccountStoreError::Database)?;
        let administrator = transaction
            .query_row("SELECT COUNT(*) = 0 FROM web_accounts", [], |row| {
                row.get::<_, bool>(0)
            })
            .map_err(|_| AccountStoreError::Database)?;
        let result = transaction.execute(
            "INSERT INTO web_accounts
             (account_id, username, display_name, password_hash, administrator, created_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account_id,
                username,
                display_name,
                password_hash,
                administrator,
                now_unix_ms
            ],
        );
        if let Err(error) = result {
            return if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                Err(AccountStoreError::UsernameTaken)
            } else {
                Err(AccountStoreError::Database)
            };
        }
        transaction
            .commit()
            .map_err(|_| AccountStoreError::Database)?;
        Ok(AccountSummary {
            account_id,
            username: username.to_owned(),
            display_name: display_name.to_owned(),
            administrator,
        })
    }

    pub(crate) fn login(
        &self,
        username: &str,
        password: &str,
        now_unix_ms: i64,
    ) -> Result<(AccountSummary, String), AccountStoreError> {
        let stored = self
            .connection
            .lock()
            .map_err(|_| AccountStoreError::Database)?
            .query_row(
                "SELECT account_id, username, display_name, password_hash, administrator
                 FROM web_accounts WHERE username = ?1 COLLATE NOCASE",
                [username],
                |row| {
                    Ok((
                        AccountSummary {
                            account_id: row.get(0)?,
                            username: row.get(1)?,
                            display_name: row.get(2)?,
                            administrator: row.get(4)?,
                        },
                        row.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|_| AccountStoreError::Database)?;
        let Some((account, password_hash)) = stored else {
            consume_password_work(password);
            return Err(AccountStoreError::InvalidCredentials);
        };
        let parsed_hash =
            PasswordHash::new(&password_hash).map_err(|_| AccountStoreError::Database)?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .map_err(|_| AccountStoreError::InvalidCredentials)?;

        let token = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let token_hash = hash_token(&token);
        let expires_at_unix_ms = now_unix_ms.saturating_add(SESSION_LIFETIME_MS);
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| AccountStoreError::Database)?;
        let transaction = connection
            .transaction()
            .map_err(|_| AccountStoreError::Database)?;
        transaction
            .execute(
                "DELETE FROM web_account_sessions WHERE expires_at_unix_ms <= ?1",
                [now_unix_ms],
            )
            .map_err(|_| AccountStoreError::Database)?;
        transaction
            .execute(
                "INSERT INTO web_account_sessions
                 (token_hash, account_id, created_at_unix_ms, expires_at_unix_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    token_hash.as_slice(),
                    account.account_id,
                    now_unix_ms,
                    expires_at_unix_ms
                ],
            )
            .map_err(|_| AccountStoreError::Database)?;
        transaction
            .commit()
            .map_err(|_| AccountStoreError::Database)?;
        Ok((account, token))
    }

    pub(crate) fn find_session(
        &self,
        token: &str,
        now_unix_ms: i64,
    ) -> Result<AccountSummary, AccountStoreError> {
        self.connection
            .lock()
            .map_err(|_| AccountStoreError::Database)?
            .query_row(
                "SELECT account_id, username, display_name, administrator
                 FROM web_accounts
                 JOIN web_account_sessions USING (account_id)
                 WHERE token_hash = ?1 AND expires_at_unix_ms > ?2",
                params![hash_token(token).as_slice(), now_unix_ms],
                |row| {
                    Ok(AccountSummary {
                        account_id: row.get(0)?,
                        username: row.get(1)?,
                        display_name: row.get(2)?,
                        administrator: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(|_| AccountStoreError::Database)?
            .ok_or(AccountStoreError::InvalidCredentials)
    }

    pub(crate) fn logout(&self, token: &str) -> Result<(), AccountStoreError> {
        self.connection
            .lock()
            .map_err(|_| AccountStoreError::Database)?
            .execute(
                "DELETE FROM web_account_sessions WHERE token_hash = ?1",
                [hash_token(token).as_slice()],
            )
            .map_err(|_| AccountStoreError::Database)?;
        Ok(())
    }

    pub(crate) fn create_api_token(
        &self,
        account_id: &str,
        label: &str,
        now_unix_ms: i64,
    ) -> Result<CreatedAccountToken, AccountStoreError> {
        let token_id = Uuid::new_v4().simple().to_string();
        let token = format!(
            "trace_pat_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let summary = AccountTokenSummary {
            token_id,
            label: label.to_owned(),
            token_prefix: token.chars().take(18).collect(),
            created_at_unix_ms: now_unix_ms,
            last_used_at_unix_ms: None,
        };
        self.connection
            .lock()
            .map_err(|_| AccountStoreError::Database)?
            .execute(
                "INSERT INTO web_account_tokens
                 (token_id, token_hash, account_id, label, token_prefix, created_at_unix_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    summary.token_id,
                    hash_token(&token).as_slice(),
                    account_id,
                    summary.label,
                    summary.token_prefix,
                    now_unix_ms
                ],
            )
            .map_err(|_| AccountStoreError::Database)?;
        Ok(CreatedAccountToken { summary, token })
    }

    pub(crate) fn list_api_tokens(
        &self,
        account_id: &str,
    ) -> Result<Vec<AccountTokenSummary>, AccountStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| AccountStoreError::Database)?;
        let mut statement = connection
            .prepare(
                "SELECT token_id, label, token_prefix, created_at_unix_ms, last_used_at_unix_ms
                 FROM web_account_tokens WHERE account_id = ?1 ORDER BY created_at_unix_ms DESC",
            )
            .map_err(|_| AccountStoreError::Database)?;
        statement
            .query_map([account_id], |row| {
                Ok(AccountTokenSummary {
                    token_id: row.get(0)?,
                    label: row.get(1)?,
                    token_prefix: row.get(2)?,
                    created_at_unix_ms: row.get(3)?,
                    last_used_at_unix_ms: row.get(4)?,
                })
            })
            .map_err(|_| AccountStoreError::Database)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| AccountStoreError::Database)
    }

    pub(crate) fn revoke_api_token(
        &self,
        account_id: &str,
        token_id: &str,
    ) -> Result<(), AccountStoreError> {
        self.connection
            .lock()
            .map_err(|_| AccountStoreError::Database)?
            .execute(
                "DELETE FROM web_account_tokens WHERE account_id = ?1 AND token_id = ?2",
                params![account_id, token_id],
            )
            .map_err(|_| AccountStoreError::Database)?;
        Ok(())
    }

    pub(crate) fn find_api_token(
        &self,
        token: &str,
        now_unix_ms: i64,
    ) -> Result<AccountSummary, AccountStoreError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| AccountStoreError::Database)?;
        let account = connection
            .query_row(
                "SELECT account_id, username, display_name, administrator
                 FROM web_accounts JOIN web_account_tokens USING (account_id)
                 WHERE token_hash = ?1",
                [hash_token(token).as_slice()],
                |row| {
                    Ok(AccountSummary {
                        account_id: row.get(0)?,
                        username: row.get(1)?,
                        display_name: row.get(2)?,
                        administrator: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(|_| AccountStoreError::Database)?
            .ok_or(AccountStoreError::InvalidCredentials)?;
        connection
            .execute(
                "UPDATE web_account_tokens SET last_used_at_unix_ms = ?1 WHERE token_hash = ?2",
                params![now_unix_ms, hash_token(token).as_slice()],
            )
            .map_err(|_| AccountStoreError::Database)?;
        Ok(account)
    }
}

fn hash_password(password: &str) -> Result<String, AccountStoreError> {
    let salt = SaltString::encode_b64(Uuid::new_v4().as_bytes())
        .map_err(|_| AccountStoreError::Database)?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AccountStoreError::Database)
}

fn consume_password_work(password: &str) {
    let Ok(salt) = SaltString::encode_b64(b"trace-auth-miss") else {
        return;
    };
    let _ = Argon2::default().hash_password(password.as_bytes(), &salt);
}

fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_are_local_persistent_and_sessions_can_be_revoked() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let store = AccountStore::open(directory.path()).expect("account store");
        let account = store
            .register("driver", "Local Driver", "correct horse battery", 1_000)
            .expect("account");
        assert!(account.administrator);
        assert!(matches!(
            store.register("DRIVER", "Duplicate", "another safe password", 2_000),
            Err(AccountStoreError::UsernameTaken)
        ));
        assert!(matches!(
            store.login("driver", "wrong password", 2_000),
            Err(AccountStoreError::InvalidCredentials)
        ));
        let (logged_in, token) = store
            .login("DRIVER", "correct horse battery", 2_000)
            .expect("login");
        assert_eq!(logged_in.account_id, account.account_id);
        assert_eq!(
            store
                .find_session(&token, 3_000)
                .expect("active session")
                .username,
            "driver"
        );
        store.logout(&token).expect("logout");
        assert!(matches!(
            store.find_session(&token, 3_000),
            Err(AccountStoreError::InvalidCredentials)
        ));

        let reopened = AccountStore::open(directory.path()).expect("reopened account store");
        reopened
            .login("driver", "correct horse battery", 4_000)
            .expect("persistent account");

        let created = reopened
            .create_api_token(&account.account_id, "TRACE desktop", 5_000)
            .expect("API token");
        assert!(created.token.starts_with("trace_pat_"));
        assert_eq!(
            reopened.list_api_tokens(&account.account_id).unwrap().len(),
            1
        );
        assert_eq!(
            reopened
                .find_api_token(&created.token, 6_000)
                .unwrap()
                .account_id,
            account.account_id
        );
        reopened
            .revoke_api_token(&account.account_id, &created.summary.token_id)
            .expect("revoke API token");
        assert!(matches!(
            reopened.find_api_token(&created.token, 7_000),
            Err(AccountStoreError::InvalidCredentials)
        ));
    }
}
