use std::{
    fs::{self, File, OpenOptions},
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use trace_protocol::SharedSessionSummary;
use trace_storage::{
    ipc::sample_count,
    package::{SessionPackageManifest, read_package},
};
use uuid::Uuid;

const PACKAGE_EXTENSION: &str = "trace";

#[derive(Clone, Debug)]
pub(crate) struct ShareStore {
    root: PathBuf,
    public_base_url: String,
    ttl: Duration,
    maximum_bytes: u64,
}

#[derive(Debug)]
pub(crate) struct StoredShare {
    pub(crate) summary: SharedSessionSummary,
    pub(crate) path: PathBuf,
    pub(crate) download_name: String,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ShareStoreError {
    Invalid,
    NotFound,
    TooLarge,
    Unavailable,
}

impl ShareStore {
    pub(crate) fn new(
        root: PathBuf,
        public_base_url: String,
        ttl: Duration,
        maximum_bytes: u64,
    ) -> Self {
        Self {
            root,
            public_base_url,
            ttl,
            maximum_bytes,
        }
    }

    pub(crate) async fn publish(
        &self,
        contents: Vec<u8>,
    ) -> Result<SharedSessionSummary, ShareStoreError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.publish_blocking(&contents))
            .await
            .map_err(|_| ShareStoreError::Unavailable)?
    }

    pub(crate) async fn find(&self, share_id: String) -> Result<StoredShare, ShareStoreError> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.find_blocking(&share_id))
            .await
            .map_err(|_| ShareStoreError::Unavailable)?
    }

    fn publish_blocking(&self, contents: &[u8]) -> Result<SharedSessionSummary, ShareStoreError> {
        let length = u64::try_from(contents.len()).map_err(|_| ShareStoreError::TooLarge)?;
        if length == 0 || length > self.maximum_bytes {
            return Err(ShareStoreError::TooLarge);
        }
        let manifest = validate_package(contents, self.maximum_bytes)?;
        fs::create_dir_all(&self.root).map_err(|_| ShareStoreError::Unavailable)?;
        self.remove_expired();

        for _ in 0..4 {
            let share_id = Uuid::new_v4().simple().to_string();
            let temporary = self.root.join(format!("{share_id}.upload"));
            let destination = self.package_path(&share_id);
            let mut file = match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(ShareStoreError::Unavailable),
            };
            let result = file
                .write_all(contents)
                .and_then(|()| file.sync_all())
                .and_then(|()| fs::rename(&temporary, &destination));
            if result.is_err() {
                let _ = fs::remove_file(&temporary);
                return Err(ShareStoreError::Unavailable);
            }
            return Ok(self.summary(&share_id, length, &manifest, SystemTime::now()));
        }
        Err(ShareStoreError::Unavailable)
    }

    fn find_blocking(&self, share_id: &str) -> Result<StoredShare, ShareStoreError> {
        if !valid_identifier(share_id) {
            return Err(ShareStoreError::NotFound);
        }
        let path = self.package_path(share_id);
        let metadata = path.metadata().map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ShareStoreError::NotFound
            } else {
                ShareStoreError::Unavailable
            }
        })?;
        let modified = metadata
            .modified()
            .map_err(|_| ShareStoreError::Unavailable)?;
        if modified
            .checked_add(self.ttl)
            .is_none_or(|expires| expires <= SystemTime::now())
        {
            let _ = fs::remove_file(&path);
            return Err(ShareStoreError::NotFound);
        }
        let manifest = inspect_package(&path, metadata.len(), self.maximum_bytes)?;
        let summary = self.summary(share_id, metadata.len(), &manifest, modified);
        let download_name = package_filename(&manifest);
        Ok(StoredShare {
            summary,
            path,
            download_name,
        })
    }

    fn summary(
        &self,
        share_id: &str,
        size_bytes: u64,
        manifest: &SessionPackageManifest,
        created_at: SystemTime,
    ) -> SharedSessionSummary {
        let share_url = format!("{}/s/{share_id}", self.public_base_url);
        SharedSessionSummary {
            share_id: share_id.to_owned(),
            download_url: format!(
                "{}/api/v1/shared-sessions/{share_id}/download",
                self.public_base_url
            ),
            share_url,
            expires_at_unix_ms: unix_timestamp_ms(created_at.checked_add(self.ttl)),
            size_bytes,
            driver_name: manifest.session.user_driver.clone(),
            simulator: manifest.session.simulator_key.clone(),
            car: manifest
                .session
                .car
                .clone()
                .unwrap_or_else(|| "Unknown car".to_owned()),
            track: manifest
                .session
                .track
                .clone()
                .unwrap_or_else(|| "Unknown track".to_owned()),
            session_type: manifest
                .session
                .session_type
                .clone()
                .unwrap_or_else(|| "Session".to_owned()),
            lap_count: u32::try_from(manifest.laps.len()).unwrap_or(u32::MAX),
        }
    }

    fn package_path(&self, share_id: &str) -> PathBuf {
        self.root.join(format!("{share_id}.{PACKAGE_EXTENSION}"))
    }

    fn remove_expired(&self) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let path = entry.path();
            let is_package =
                path.extension().and_then(|value| value.to_str()) == Some(PACKAGE_EXTENSION);
            let expired = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.checked_add(self.ttl))
                .is_some_and(|expires| expires <= now);
            if is_package && expired {
                let _ = fs::remove_file(path);
            }
        }
    }
}

pub(crate) fn validate_package(
    contents: &[u8],
    maximum_bytes: u64,
) -> Result<SessionPackageManifest, ShareStoreError> {
    let length = u64::try_from(contents.len()).map_err(|_| ShareStoreError::TooLarge)?;
    let package = read_package(Cursor::new(contents), length, maximum_bytes)
        .map_err(|_| ShareStoreError::Invalid)?;
    let manifest = package.manifest;
    let mut telemetry = package.telemetry;
    let actual_samples = sample_count(&mut telemetry).map_err(|_| ShareStoreError::Invalid)?;
    if actual_samples != manifest.sample_count {
        return Err(ShareStoreError::Invalid);
    }
    Ok(manifest)
}

fn inspect_package(
    path: &Path,
    length: u64,
    maximum_bytes: u64,
) -> Result<SessionPackageManifest, ShareStoreError> {
    let file = File::open(path).map_err(|_| ShareStoreError::Unavailable)?;
    read_package(file, length, maximum_bytes)
        .map(|package| package.manifest)
        .map_err(|_| ShareStoreError::Invalid)
}

fn valid_identifier(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn package_filename(manifest: &SessionPackageManifest) -> String {
    let parts = [
        manifest.session.user_driver.as_deref(),
        manifest.session.track.as_deref(),
        manifest.session.car.as_deref(),
    ];
    let stem = parts
        .into_iter()
        .flatten()
        .map(filename_component)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!(
        "{}.trace",
        if stem.is_empty() {
            "trace-session"
        } else {
            &stem
        }
    )
}

fn filename_component(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .take(8)
        .collect::<Vec<_>>()
        .join("-")
}

fn unix_timestamp_ms(time: Option<SystemTime>) -> i64 {
    let millis = time
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_millis());
    i64::try_from(millis).unwrap_or(i64::MAX)
}
