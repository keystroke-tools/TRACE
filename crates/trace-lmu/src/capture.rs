use trace_windows_shmem::{MappingError, SpinLockedMapping};

use crate::{LmuPageError, LmuSnapshot, SHARED_MEMORY_SIZE};

const DATA_MAPPING: &str = "LMU_Data";
const LOCK_MAPPING: &str = "LMU_SharedMemoryLockData";
const LOCK_WAKE_EVENT: &str = "LMU_SharedMemoryLockEvent";
const DEFAULT_LOCK_SPINS: u32 = 4_000;

/// Whether LMU's official shared-memory objects are available on this host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LmuAvailability {
    Available,
    NotRunning,
    UnsupportedPlatform,
}

/// Failure while opening or decoding LMU shared memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LmuCaptureError {
    Mapping(MappingError),
    InvalidPage(LmuPageError),
}

impl From<MappingError> for LmuCaptureError {
    fn from(value: MappingError) -> Self {
        Self::Mapping(value)
    }
}

impl From<LmuPageError> for LmuCaptureError {
    fn from(value: LmuPageError) -> Self {
        Self::InvalidPage(value)
    }
}

/// Production connection to LMU's built-in, synchronized shared-memory feed.
pub struct LmuSharedMemory {
    mapping: SpinLockedMapping,
}

impl LmuSharedMemory {
    /// Checks whether LMU's shared-memory feed can be opened.
    ///
    /// # Errors
    ///
    /// Returns a typed error for failures other than the simulator not running or
    /// the current platform not supporting Windows mappings.
    pub fn detect() -> Result<LmuAvailability, LmuCaptureError> {
        match Self::open() {
            Ok(_) => Ok(LmuAvailability::Available),
            Err(LmuCaptureError::Mapping(MappingError::NotFound)) => {
                Ok(LmuAvailability::NotRunning)
            }
            Err(LmuCaptureError::Mapping(MappingError::UnsupportedPlatform)) => {
                Ok(LmuAvailability::UnsupportedPlatform)
            }
            Err(error) => Err(error),
        }
    }

    /// Opens LMU's data mapping and official shared synchronization objects.
    ///
    /// # Errors
    ///
    /// Returns a mapping error when LMU is not running or an expected object is
    /// unavailable.
    pub fn open() -> Result<Self, LmuCaptureError> {
        Ok(Self {
            mapping: SpinLockedMapping::open(
                DATA_MAPPING,
                SHARED_MEMORY_SIZE,
                LOCK_MAPPING,
                LOCK_WAKE_EVENT,
            )?,
        })
    }

    /// Copies and validates one coherent LMU update under the official lock.
    ///
    /// # Errors
    ///
    /// Returns an error when the lock is temporarily busy or the snapshot does not
    /// match the validated V01 layout.
    pub fn snapshot(&self) -> Result<LmuSnapshot, LmuCaptureError> {
        Ok(LmuSnapshot::from_bytes(
            self.mapping.copy_owned(DEFAULT_LOCK_SPINS)?,
        )?)
    }
}
