//! Storage contracts for immutable telemetry blobs.
//!
//! High-rate samples belong in blob storage, not individual database rows. The
//! metadata repository and concrete filesystem/SQLite implementation can evolve
//! independently behind this boundary.

use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod ipc;
pub mod metadata;

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
        let id = BlobId(format!("blob-{}", hex_digest(&sha256)));
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

/// Filesystem-backed immutable blob storage for desktop capture.
///
/// Pending and committed files share one root, allowing publication with a
/// same-filesystem hard link that never overwrites an existing destination. The
/// in-memory identifier index is reconstructed from committed files on open.
#[derive(Debug)]
pub struct FileBlobStore {
    root: PathBuf,
    staging: PathBuf,
    quarantine: PathBuf,
    max_blob_bytes: u64,
    next_pending: u64,
    pending: BTreeMap<PendingBlobId, PathBuf>,
    paths: BTreeMap<BlobId, RelativeBlobPath>,
}

/// Owned bounded writer for one filesystem staging file.
#[derive(Debug)]
pub struct FileBlobWriter {
    file: File,
    pending: PendingBlobId,
    length: u64,
    max_blob_bytes: u64,
}

impl FileBlobWriter {
    /// Returns the pending identity after a higher-level encoder is finished.
    pub fn into_pending(self) -> PendingBlobId {
        self.pending
    }
}

impl Write for FileBlobWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let appended = u64::try_from(bytes.len()).map_err(std::io::Error::other)?;
        if self.length.saturating_add(appended) > self.max_blob_bytes {
            return Err(std::io::Error::other(
                "TRACE telemetry blob size limit exceeded",
            ));
        }
        let written = self.file.write(bytes)?;
        self.length = self
            .length
            .saturating_add(u64::try_from(written).map_err(std::io::Error::other)?);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl FileBlobStore {
    /// Opens a dedicated blob root and indexes existing committed files.
    ///
    /// # Errors
    ///
    /// Returns a backend error if directories or committed files cannot be read,
    /// and [`StorageError::BlobTooLarge`] for a zero byte limit.
    pub fn open(root: &Path, max_blob_bytes: u64) -> Result<Self, StorageError> {
        if max_blob_bytes == 0 {
            return Err(StorageError::BlobTooLarge);
        }
        fs::create_dir_all(root).map_err(backend)?;
        let staging = root.join(".pending");
        let quarantine = root.join(".orphaned");
        fs::create_dir_all(&staging).map_err(backend)?;
        fs::create_dir_all(&quarantine).map_err(backend)?;
        let mut paths = BTreeMap::new();
        index_committed(root, root, [&staging, &quarantine], &mut paths)?;
        Ok(Self {
            root: root.to_path_buf(),
            staging,
            quarantine,
            max_blob_bytes,
            next_pending: 1,
            pending: BTreeMap::new(),
            paths,
        })
    }

    /// Returns unfinished staging files left by an interrupted process.
    ///
    /// # Errors
    ///
    /// Returns a backend error when the staging directory cannot be read.
    pub fn orphaned_pending_files(&self) -> Result<Vec<PathBuf>, StorageError> {
        let mut paths = fs::read_dir(&self.staging)
            .map_err(backend)?
            .map(|entry| entry.map(|value| value.path()).map_err(backend))
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort();
        Ok(paths)
    }

    /// Begins an owned staging writer suitable for incremental encoders.
    ///
    /// # Errors
    ///
    /// Returns a storage error when staging cannot begin or reopen for append.
    pub fn begin_writer(&mut self) -> Result<FileBlobWriter, StorageError> {
        let pending = self.begin()?;
        let path = self
            .pending
            .get(&pending)
            .ok_or(StorageError::PendingNotFound)?;
        let file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(backend)?;
        Ok(FileBlobWriter {
            file,
            pending,
            length: 0,
            max_blob_bytes: self.max_blob_bytes,
        })
    }

    /// Quarantines files that cannot currently be reached from metadata.
    ///
    /// Both committed-but-unreferenced blobs and interrupted pending files are moved
    /// beneath `.orphaned` without deleting their bytes. This should run before new
    /// capture begins, while the store has no active pending handles.
    ///
    /// # Errors
    ///
    /// Returns a backend or collision error without overwriting quarantine data.
    pub fn reconcile(
        &mut self,
        referenced: &std::collections::BTreeSet<RelativeBlobPath>,
    ) -> Result<ReconciliationReport, StorageError> {
        if !self.pending.is_empty() {
            return Err(StorageError::Backend(
                "reconciliation requires an idle blob store".into(),
            ));
        }
        let mut report = ReconciliationReport::default();
        let unreferenced = self
            .paths
            .iter()
            .filter(|(_, path)| !referenced.contains(*path))
            .map(|(id, path)| (id.clone(), path.clone()))
            .collect::<Vec<_>>();
        for (id, relative) in unreferenced {
            let source = self.root.join(relative.as_str());
            let destination = self.quarantine.join("committed").join(relative.as_str());
            quarantine_file(&source, &destination)?;
            self.paths.remove(&id);
            report.committed.push(relative);
        }
        for path in self.orphaned_pending_files()? {
            let name = path
                .file_name()
                .ok_or_else(|| StorageError::Backend("pending file has no name".into()))?;
            let destination = self.quarantine.join("pending").join(name);
            quarantine_file(&path, &destination)?;
            report.pending.push(destination);
        }
        Ok(report)
    }
}

/// Recoverable files isolated by a reconciliation pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReconciliationReport {
    pub committed: Vec<RelativeBlobPath>,
    pub pending: Vec<PathBuf>,
}

impl TelemetryBlobStore for FileBlobStore {
    fn begin(&mut self) -> Result<PendingBlobId, StorageError> {
        loop {
            let id = PendingBlobId(format!(
                "pending-{}-{}",
                std::process::id(),
                self.next_pending
            ));
            self.next_pending = self
                .next_pending
                .checked_add(1)
                .ok_or_else(|| StorageError::Backend("pending identifier exhausted".into()))?;
            let path = self.staging.join(&id.0);
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => {
                    self.pending.insert(id.clone(), path);
                    return Ok(id);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(backend(error)),
            }
        }
    }

    fn append(&mut self, pending: &PendingBlobId, bytes: &[u8]) -> Result<(), StorageError> {
        let path = self
            .pending
            .get(pending)
            .ok_or(StorageError::PendingNotFound)?;
        let current = fs::metadata(path).map_err(backend)?.len();
        let appended = u64::try_from(bytes.len()).map_err(|_| StorageError::BlobTooLarge)?;
        if current.saturating_add(appended) > self.max_blob_bytes {
            return Err(StorageError::BlobTooLarge);
        }
        let mut file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(backend)?;
        file.write_all(bytes).map_err(backend)
    }

    fn commit(
        &mut self,
        pending: &PendingBlobId,
        commit: BlobCommit,
    ) -> Result<BlobMetadata, StorageError> {
        let staged = self
            .pending
            .get(pending)
            .ok_or(StorageError::PendingNotFound)?;
        let (byte_length, sha256) = digest_file(staged)?;
        if commit
            .expected_sha256
            .is_some_and(|expected| expected != sha256)
        {
            return Err(StorageError::ChecksumMismatch);
        }
        let destination = self.root.join(commit.path.as_str());
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(backend)?;
        }
        File::open(staged)
            .and_then(|file| file.sync_all())
            .map_err(backend)?;
        match fs::hard_link(staged, &destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(StorageError::PathAlreadyExists);
            }
            Err(error) => return Err(backend(error)),
        }
        // Publication has succeeded at this point. A staging cleanup failure must
        // not turn that durable commit into a retry that reports a path collision;
        // the leftover remains visible to orphan reconciliation.
        let _ = fs::remove_file(staged);
        self.pending.remove(pending);
        let id = BlobId(format!("blob-{}", hex_digest(&sha256)));
        self.paths.insert(id.clone(), commit.path.clone());
        Ok(BlobMetadata {
            id,
            path: commit.path,
            format: commit.format,
            schema_version: commit.schema_version,
            byte_length,
            sample_count: commit.sample_count,
            sha256,
        })
    }

    fn abort(&mut self, pending: &PendingBlobId) -> Result<(), StorageError> {
        let Some(path) = self.pending.remove(pending) else {
            return Ok(());
        };
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(backend(error)),
        }
    }

    fn read(&self, id: &BlobId) -> Result<Vec<u8>, StorageError> {
        let relative = self.paths.get(id).ok_or(StorageError::BlobNotFound)?;
        fs::read(self.root.join(relative.as_str())).map_err(backend)
    }
}

fn index_committed(
    root: &Path,
    directory: &Path,
    excluded: [&Path; 2],
    paths: &mut BTreeMap<BlobId, RelativeBlobPath>,
) -> Result<(), StorageError> {
    for entry in fs::read_dir(directory).map_err(backend)? {
        let entry = entry.map_err(backend)?;
        let path = entry.path();
        if excluded.contains(&path.as_path()) {
            continue;
        }
        let file_type = entry.file_type().map_err(backend)?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            index_committed(root, &path, excluded, paths)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| StorageError::Backend(error.to_string()))?
                .to_str()
                .ok_or_else(|| StorageError::Backend("blob path is not UTF-8".into()))?
                .replace('\\', "/");
            let relative = RelativeBlobPath::parse(relative)
                .map_err(|error| StorageError::Backend(format!("invalid blob path: {error:?}")))?;
            let (_, digest) = digest_file(&path)?;
            paths.insert(BlobId(format!("blob-{}", hex_digest(&digest))), relative);
        }
    }
    Ok(())
}

fn quarantine_file(source: &Path, destination: &Path) -> Result<(), StorageError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(backend)?;
    }
    match fs::hard_link(source, destination) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(StorageError::PathAlreadyExists);
        }
        Err(error) => return Err(backend(error)),
    }
    fs::remove_file(source).map_err(backend)
}

fn digest_file(path: &Path) -> Result<(u64, [u8; 32]), StorageError> {
    let mut file = File::open(path).map_err(backend)?;
    let mut hasher = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file.read(&mut buffer).map_err(backend)?;
        if read == 0 {
            break;
        }
        length = length
            .checked_add(u64::try_from(read).map_err(|_| StorageError::BlobTooLarge)?)
            .ok_or(StorageError::BlobTooLarge)?;
        hasher.update(&buffer[..read]);
    }
    Ok((length, hasher.finalize().into()))
}

fn hex_digest(value: &[u8; 32]) -> String {
    use std::fmt::Write as _;

    value
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
            output
        })
}

#[allow(clippy::needless_pass_by_value)] // Matches map_err's owned error callback.
fn backend(error: std::io::Error) -> StorageError {
    StorageError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TemporaryRoot(PathBuf);

    impl TemporaryRoot {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("trace-storage-test-{}-{nonce}", std::process::id()));
            fs::create_dir(&path).expect("temporary root");
            Self(path)
        }
    }

    impl Drop for TemporaryRoot {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove temporary root");
        }
    }

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

    #[test]
    fn filesystem_store_publishes_and_reopens_committed_blobs() {
        let root = TemporaryRoot::new();
        let metadata = {
            let mut store = FileBlobStore::open(&root.0, 1024).expect("file store");
            let pending = store.begin().expect("begin");
            store.append(&pending, b"telemetry").expect("append");
            let metadata = store
                .commit(&pending, commit("sessions/one.trace-fixture"))
                .expect("commit");
            assert!(store.orphaned_pending_files().expect("orphans").is_empty());
            assert_eq!(store.read(&metadata.id).expect("read"), b"telemetry");
            metadata
        };

        let reopened = FileBlobStore::open(&root.0, 1024).expect("reopened");
        assert_eq!(
            reopened.read(&metadata.id).expect("reopened read"),
            b"telemetry"
        );
    }

    #[test]
    fn filesystem_store_preserves_failed_staging_for_reconciliation() {
        let root = TemporaryRoot::new();
        let mut store = FileBlobStore::open(&root.0, 1024).expect("file store");
        let pending = store.begin().expect("begin");
        store.append(&pending, b"telemetry").expect("append");
        let mut details = commit("sessions/one.trace-fixture");
        details.expected_sha256 = Some([0; 32]);

        assert_eq!(
            store.commit(&pending, details),
            Err(StorageError::ChecksumMismatch)
        );
        assert_eq!(store.orphaned_pending_files().expect("orphans").len(), 1);
    }

    #[test]
    fn filesystem_store_never_overwrites_a_committed_path() {
        let root = TemporaryRoot::new();
        let mut store = FileBlobStore::open(&root.0, 1024).expect("file store");
        let first = store.begin().expect("first");
        store.append(&first, b"first").expect("append first");
        store
            .commit(&first, commit("sessions/shared.trace-fixture"))
            .expect("commit first");
        let second = store.begin().expect("second");
        store.append(&second, b"second").expect("append second");

        assert_eq!(
            store.commit(&second, commit("sessions/shared.trace-fixture")),
            Err(StorageError::PathAlreadyExists)
        );
        assert_eq!(
            fs::read(root.0.join("sessions/shared.trace-fixture")).expect("original"),
            b"first"
        );
    }

    #[test]
    fn reconciliation_quarantines_unreferenced_and_interrupted_files() {
        let root = TemporaryRoot::new();
        let mut store = FileBlobStore::open(&root.0, 1024).expect("file store");
        let kept = store.begin().expect("kept");
        store.append(&kept, b"kept").expect("append kept");
        let kept = store
            .commit(&kept, commit("sessions/kept.trace-fixture"))
            .expect("commit kept");
        let orphan = store.begin().expect("orphan");
        store.append(&orphan, b"orphan").expect("append orphan");
        store
            .commit(&orphan, commit("sessions/orphan.trace-fixture"))
            .expect("commit orphan");
        let interrupted = store.begin().expect("interrupted");
        store
            .append(&interrupted, b"interrupted")
            .expect("append interrupted");
        drop(store);

        let mut reopened = FileBlobStore::open(&root.0, 1024).expect("reopened");
        let report = reopened
            .reconcile(&BTreeSet::from([kept.path.clone()]))
            .expect("reconciled");
        assert_eq!(
            report.committed,
            vec![RelativeBlobPath::parse("sessions/orphan.trace-fixture").expect("path")]
        );
        assert_eq!(report.pending.len(), 1);
        assert!(reopened.read(&kept.id).is_ok());
        assert!(!root.0.join("sessions/orphan.trace-fixture").exists());
        assert!(
            root.0
                .join(".orphaned/committed/sessions/orphan.trace-fixture")
                .exists()
        );
        assert!(report.pending[0].exists());
    }
}
