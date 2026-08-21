//! Storage contracts for immutable telemetry blobs.
//!
//! High-rate samples belong in blob storage, not individual database rows. The
//! metadata repository and concrete filesystem/SQLite implementation can evolve
//! independently behind this boundary.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Opaque identity of a committed telemetry blob.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct BlobId(String);

impl BlobId {
    /// Returns the stable identifier representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Opaque identity of a blob that has not been committed.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct PendingBlobId(String);

/// Portable path relative to a storage root.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct RelativeBlobPath(String);

impl RelativeBlobPath {
    /// Validates and creates a normalized portable path.
    ///
    /// # Errors
    ///
    /// Rejects empty, absolute, platform-specific, or traversing paths.
    pub fn parse(value: impl Into<String>) -> Result<Self, PathError> {
        let value = value.into();
        if value.is_empty() {
            return Err(PathError::Empty);
        }
        if value.starts_with('/') || value.contains('\\') {
            return Err(PathError::NotPortableRelative);
        }
        if value
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
        {
            return Err(PathError::InvalidComponent);
        }
        if value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
        {
            return Err(PathError::InvalidCharacter);
        }
        Ok(Self(value))
    }

    /// Returns the normalized relative representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Invalid blob path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathError {
    Empty,
    NotPortableRelative,
    InvalidComponent,
    InvalidCharacter,
}

/// Physical representation of telemetry bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BlobFormat {
    ArrowIpc,
    TraceFixture,
}

/// Information required to atomically commit a completed blob.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlobCommit {
    pub path: RelativeBlobPath,
    pub format: BlobFormat,
    pub schema_version: u32,
    pub sample_count: u64,
    /// Optional expected digest supplied by an importer or capture pipeline.
    pub expected_sha256: Option<[u8; 32]>,
}

/// Persisted metadata for an immutable telemetry blob.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BlobMetadata {
    pub id: BlobId,
    pub path: RelativeBlobPath,
    pub format: BlobFormat,
    pub schema_version: u32,
    pub byte_length: u64,
    pub sample_count: u64,
    pub sha256: [u8; 32],
}

/// Storage operations needed by capture and replay pipelines.
pub trait TelemetryBlobStore {
    /// Begins an uncommitted blob that is invisible to normal readers.
    ///
    /// # Errors
    ///
    /// Returns a backend error if staging cannot begin.
    fn begin(&mut self) -> Result<PendingBlobId, StorageError>;

    /// Appends a bounded chunk to a pending blob.
    ///
    /// # Errors
    ///
    /// Returns a backend error or [`StorageError::PendingNotFound`].
    fn append(&mut self, pending: &PendingBlobId, bytes: &[u8]) -> Result<(), StorageError>;

    /// Atomically makes a finalized blob and its metadata visible.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing handle, path collision, invalid checksum,
    /// oversized data, or backend failure. Failed commits remain pending so callers
    /// can inspect or explicitly abort them.
    fn commit(
        &mut self,
        pending: &PendingBlobId,
        commit: BlobCommit,
    ) -> Result<BlobMetadata, StorageError>;

    /// Removes an uncommitted blob. This operation is idempotent.
    ///
    /// # Errors
    ///
    /// Returns a backend error if cleanup fails.
    fn abort(&mut self, pending: &PendingBlobId) -> Result<(), StorageError>;

    /// Reads a committed blob for replay or export.
    ///
    /// Concrete filesystem implementations should expose streaming readers in
    /// addition to this baseline contract when telemetry sizes require it.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::BlobNotFound`] or a backend error.
    fn read(&self, id: &BlobId) -> Result<Vec<u8>, StorageError>;
}

/// Storage failure with recoverable cases represented explicitly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StorageError {
    PendingNotFound,
    BlobNotFound,
    PathAlreadyExists,
    ChecksumMismatch,
    BlobTooLarge,
    Backend(String),
}

/// Dependency-free storage fixture used by tests and early replay development.
#[derive(Clone, Debug)]
pub struct InMemoryBlobStore {
    next_id: u64,
    max_blob_bytes: usize,
    pending: BTreeMap<PendingBlobId, Vec<u8>>,
    blobs: BTreeMap<BlobId, (BlobMetadata, Vec<u8>)>,
    paths: BTreeMap<RelativeBlobPath, BlobId>,
}

impl InMemoryBlobStore {
    /// Creates a fixture store with a hard resource limit per blob.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::BlobTooLarge`] when the configured limit is zero.
    pub fn new(max_blob_bytes: usize) -> Result<Self, StorageError> {
        if max_blob_bytes == 0 {
            return Err(StorageError::BlobTooLarge);
        }
        Ok(Self {
            next_id: 1,
            max_blob_bytes,
            pending: BTreeMap::new(),
            blobs: BTreeMap::new(),
            paths: BTreeMap::new(),
        })
    }

    /// Returns committed metadata without exposing pending writes.
    pub fn metadata(&self, id: &BlobId) -> Option<&BlobMetadata> {
        self.blobs.get(id).map(|(metadata, _)| metadata)
    }

    /// Returns the number of staged blobs, useful for reconciliation tests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
}

impl TelemetryBlobStore for InMemoryBlobStore {
    fn begin(&mut self) -> Result<PendingBlobId, StorageError> {
        let id = PendingBlobId(format!("pending-{}", self.next_id));
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| StorageError::Backend("in-memory blob identifier exhausted".into()))?;
        self.pending.insert(id.clone(), Vec::new());
        Ok(id)
    }

    fn append(&mut self, pending: &PendingBlobId, bytes: &[u8]) -> Result<(), StorageError> {
        let target = self
            .pending
            .get_mut(pending)
            .ok_or(StorageError::PendingNotFound)?;
        if target.len().saturating_add(bytes.len()) > self.max_blob_bytes {
            return Err(StorageError::BlobTooLarge);
        }
        target.extend_from_slice(bytes);
        Ok(())
    }

    fn commit(
        &mut self,
        pending: &PendingBlobId,
        commit: BlobCommit,
    ) -> Result<BlobMetadata, StorageError> {
        if self.paths.contains_key(&commit.path) {
            return Err(StorageError::PathAlreadyExists);
        }
        let bytes = self
            .pending
            .get(pending)
            .ok_or(StorageError::PendingNotFound)?;
        let sha256: [u8; 32] = Sha256::digest(bytes).into();
        if commit
            .expected_sha256
            .is_some_and(|expected| expected != sha256)
        {
            return Err(StorageError::ChecksumMismatch);
        }
        let byte_length = u64::try_from(bytes.len()).map_err(|_| StorageError::BlobTooLarge)?;
        let id = BlobId(pending.0.replacen("pending-", "blob-", 1));
        let metadata = BlobMetadata {
            id: id.clone(),
            path: commit.path.clone(),
            format: commit.format,
            schema_version: commit.schema_version,
            byte_length,
            sample_count: commit.sample_count,
            sha256,
        };
        let bytes = self
            .pending
            .remove(pending)
            .ok_or(StorageError::PendingNotFound)?;
        self.paths.insert(commit.path, id.clone());
        self.blobs.insert(id, (metadata.clone(), bytes));
        Ok(metadata)
    }

    fn abort(&mut self, pending: &PendingBlobId) -> Result<(), StorageError> {
        self.pending.remove(pending);
        Ok(())
    }

    fn read(&self, id: &BlobId) -> Result<Vec<u8>, StorageError> {
        self.blobs
            .get(id)
            .map(|(_, bytes)| bytes.clone())
            .ok_or(StorageError::BlobNotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(path: &str) -> BlobCommit {
        BlobCommit {
            path: RelativeBlobPath::parse(path).expect("valid path"),
            format: BlobFormat::TraceFixture,
            schema_version: 1,
            sample_count: 2,
            expected_sha256: None,
        }
    }

    #[test]
    fn paths_reject_traversal_and_platform_specific_separators() {
        assert_eq!(
            RelativeBlobPath::parse("../secret"),
            Err(PathError::InvalidComponent)
        );
        assert_eq!(
            RelativeBlobPath::parse("laps\\one.arrow"),
            Err(PathError::NotPortableRelative)
        );
        assert_eq!(
            RelativeBlobPath::parse("laps/one.arrow")
                .expect("portable path")
                .as_str(),
            "laps/one.arrow"
        );
    }

    #[test]
    fn pending_bytes_are_invisible_until_atomic_commit() {
        let mut store = InMemoryBlobStore::new(1024).expect("valid store");
        let pending = store.begin().expect("begin");
        store.append(&pending, b"telemetry").expect("append");
        assert_eq!(store.pending_count(), 1);

        let metadata = store
            .commit(&pending, commit("laps/one.trace-fixture"))
            .expect("commit");
        assert_eq!(store.pending_count(), 0);
        assert_eq!(metadata.byte_length, 9);
        assert_eq!(store.read(&metadata.id).expect("read"), b"telemetry");
    }

    #[test]
    fn failed_checksum_keeps_pending_blob_for_reconciliation() {
        let mut store = InMemoryBlobStore::new(1024).expect("valid store");
        let pending = store.begin().expect("begin");
        store.append(&pending, b"telemetry").expect("append");
        let mut details = commit("laps/one.trace-fixture");
        details.expected_sha256 = Some([0; 32]);

        assert_eq!(
            store.commit(&pending, details),
            Err(StorageError::ChecksumMismatch)
        );
        assert_eq!(store.pending_count(), 1);
        store.abort(&pending).expect("abort");
        assert_eq!(store.pending_count(), 0);
    }

    #[test]
    fn blob_size_and_path_collision_are_bounded() {
        let mut store = InMemoryBlobStore::new(4).expect("valid store");
        let oversized = store.begin().expect("begin oversized");
        assert_eq!(
            store.append(&oversized, b"12345"),
            Err(StorageError::BlobTooLarge)
        );

        let first = store.begin().expect("begin first");
        store.append(&first, b"1234").expect("append first");
        store
            .commit(&first, commit("laps/shared.trace-fixture"))
            .expect("commit first");

        let second = store.begin().expect("begin second");
        assert_eq!(
            store.commit(&second, commit("laps/shared.trace-fixture")),
            Err(StorageError::PathAlreadyExists)
        );
    }
}
