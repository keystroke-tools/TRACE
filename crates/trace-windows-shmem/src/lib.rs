//! Minimal owned-snapshot boundary around Windows named shared memory.
//!
//! This is the only TRACE crate permitted to contain unsafe code. Consumers never
//! receive a mapped pointer or borrowed view; every read is a volatile copy into
//! owned bytes because the simulator can modify the mapping concurrently.

#[cfg(windows)]
mod windows;

#[cfg(windows)]
pub use windows::{NamedMapping, SpinLockedMapping};

/// Failure to open or copy a named shared-memory mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MappingError {
    InvalidName,
    InvalidSize,
    NotFound,
    OpenFailed(u32),
    ViewFailed(u32),
    SynchronizationOpenFailed(u32),
    SynchronizationBusy,
    UnsupportedPlatform,
}

#[cfg(not(windows))]
/// Platform stub that keeps acquisition code testable on non-Windows hosts.
pub struct NamedMapping;

#[cfg(not(windows))]
/// Platform stub for a mapping protected by a simulator-owned spin lock.
pub struct SpinLockedMapping;

#[cfg(not(windows))]
impl NamedMapping {
    /// Reports that named Windows mappings are unavailable on this platform.
    ///
    /// # Errors
    ///
    /// Always returns [`MappingError::UnsupportedPlatform`].
    pub fn open(_name: &str, _size: usize) -> Result<Self, MappingError> {
        Err(MappingError::UnsupportedPlatform)
    }

    /// Reports that named Windows mappings are unavailable on this platform.
    ///
    /// # Errors
    ///
    /// Always returns [`MappingError::UnsupportedPlatform`].
    pub fn copy_owned(&self) -> Result<Vec<u8>, MappingError> {
        Err(MappingError::UnsupportedPlatform)
    }

    /// Reports that named Windows mappings are unavailable on this platform.
    ///
    /// # Errors
    ///
    /// Always returns [`MappingError::UnsupportedPlatform`].
    pub fn read_i32(&self, _offset: usize) -> Result<i32, MappingError> {
        Err(MappingError::UnsupportedPlatform)
    }
}

#[cfg(not(windows))]
impl SpinLockedMapping {
    /// Reports that named Windows mappings are unavailable on this platform.
    ///
    /// # Errors
    ///
    /// Always returns [`MappingError::UnsupportedPlatform`].
    pub fn open(
        _mapping_name: &str,
        _size: usize,
        _lock_mapping_name: &str,
        _wake_event_name: &str,
    ) -> Result<Self, MappingError> {
        Err(MappingError::UnsupportedPlatform)
    }

    /// Reports that named Windows mappings are unavailable on this platform.
    ///
    /// # Errors
    ///
    /// Always returns [`MappingError::UnsupportedPlatform`].
    pub fn copy_owned(&self, _max_spins: u32) -> Result<Vec<u8>, MappingError> {
        Err(MappingError::UnsupportedPlatform)
    }
}
