use std::{
    hint, ptr,
    sync::atomic::{AtomicI32, Ordering},
};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE};
use windows_sys::Win32::System::Memory::{
    FILE_MAP_ALL_ACCESS, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
    OpenFileMappingW, UnmapViewOfFile,
};
use windows_sys::Win32::System::Threading::{EVENT_MODIFY_STATE, OpenEventW, SetEvent};

use crate::MappingError;

/// Read-only named mapping whose contents can only be copied into owned bytes.
pub struct NamedMapping {
    handle: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    size: usize,
}

impl NamedMapping {
    /// Opens an existing Windows named mapping and maps exactly `size` bytes.
    ///
    /// # Errors
    ///
    /// Returns a typed error for invalid input, a missing mapping, or a Win32 open
    /// or view failure.
    pub fn open(name: &str, size: usize) -> Result<Self, MappingError> {
        if name.is_empty() || name.encode_utf16().any(|unit| unit == 0) {
            return Err(MappingError::InvalidName);
        }
        if size == 0 {
            return Err(MappingError::InvalidSize);
        }

        let wide_name: Vec<_> = name.encode_utf16().chain([0]).collect();
        // SAFETY: `wide_name` is NUL-terminated and remains alive for the call. The
        // returned handle is checked and owned by this value.
        let handle = unsafe { OpenFileMappingW(FILE_MAP_READ, 0, wide_name.as_ptr()) };
        if handle.is_null() {
            // SAFETY: `GetLastError` has no preconditions and immediately follows
            // the failed Win32 call whose error it reports.
            let code = unsafe { GetLastError() };
            return if code == ERROR_FILE_NOT_FOUND {
                Err(MappingError::NotFound)
            } else {
                Err(MappingError::OpenFailed(code))
            };
        }

        // SAFETY: `handle` is a valid read-only mapping handle. The returned view
        // is checked and later released by `Drop`.
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, size) };
        if view.Value.is_null() {
            // SAFETY: as above, this directly follows the failed mapping call.
            let code = unsafe { GetLastError() };
            // SAFETY: `handle` is valid and has not been closed.
            unsafe { CloseHandle(handle) };
            return Err(MappingError::ViewFailed(code));
        }

        Ok(Self { handle, view, size })
    }

    /// Volatile-copies the mapped range into newly owned bytes.
    ///
    /// The copy can contain mixed simulator packets. Consumers must verify the
    /// producer's packet identifier before and after calling this method.
    ///
    /// # Errors
    ///
    /// This established mapping has no recoverable copy failure; the result remains
    /// fallible so platform implementations share one acquisition contract.
    pub fn copy_owned(&self) -> Result<Vec<u8>, MappingError> {
        let source = self.view.Value.cast::<u8>();
        let mut bytes = Vec::with_capacity(self.size);
        for offset in 0..self.size {
            // SAFETY: the view covers `self.size` readable bytes for the lifetime of
            // `self`; volatile reads prevent the compiler from assuming immutability.
            bytes.push(unsafe { ptr::read_volatile(source.add(offset)) });
        }
        Ok(bytes)
    }

    /// Reads one little-endian packet identifier using volatile byte reads.
    ///
    /// # Errors
    ///
    /// Returns [`MappingError::InvalidSize`] when four bytes do not fit at `offset`.
    pub fn read_i32(&self, offset: usize) -> Result<i32, MappingError> {
        if offset.checked_add(4).is_none_or(|end| end > self.size) {
            return Err(MappingError::InvalidSize);
        }
        let source = self.view.Value.cast::<u8>();
        let mut bytes = [0_u8; 4];
        for (index, byte) in bytes.iter_mut().enumerate() {
            // SAFETY: the bounds check above proves all four addresses lie in the
            // mapped view; volatile reads are required for concurrently updated data.
            *byte = unsafe { ptr::read_volatile(source.add(offset + index)) };
        }
        Ok(i32::from_le_bytes(bytes))
    }
}

impl Drop for NamedMapping {
    fn drop(&mut self) {
        // SAFETY: both resources were acquired by `open`, are owned by `self`, and
        // are released exactly once here, view before handle.
        unsafe {
            UnmapViewOfFile(self.view);
            CloseHandle(self.handle);
        }
    }
}

/// Read-only mapping copied while holding a simulator-owned shared spin lock.
///
/// This follows the synchronization contract used by LMU's official shared-memory
/// SDK while keeping all mapped pointers inside this audited crate.
pub struct SpinLockedMapping {
    mapping: NamedMapping,
    lock_handle: HANDLE,
    lock_view: MEMORY_MAPPED_VIEW_ADDRESS,
    wake_event: HANDLE,
}

impl SpinLockedMapping {
    /// Opens an existing data mapping, lock mapping, and wake event.
    ///
    /// # Errors
    ///
    /// Returns a typed mapping or synchronization error when the producer is not
    /// running or its synchronization objects cannot be opened.
    pub fn open(
        mapping_name: &str,
        size: usize,
        lock_mapping_name: &str,
        wake_event_name: &str,
    ) -> Result<Self, MappingError> {
        let mapping = NamedMapping::open(mapping_name, size)?;
        let lock_name = wide_name(lock_mapping_name)?;
        // SAFETY: the name is NUL-terminated and lives for the duration of the call.
        let lock_handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, lock_name.as_ptr()) };
        if lock_handle.is_null() {
            // SAFETY: this immediately follows the failed Win32 call.
            return Err(MappingError::SynchronizationOpenFailed(unsafe {
                GetLastError()
            }));
        }
        // SAFETY: the handle grants full access and the official lock mapping is two
        // consecutive, four-byte-aligned LONG values.
        let lock_view = unsafe { MapViewOfFile(lock_handle, FILE_MAP_ALL_ACCESS, 0, 0, 8) };
        if lock_view.Value.is_null() {
            // SAFETY: this immediately follows the failed mapping call.
            let code = unsafe { GetLastError() };
            // SAFETY: the handle is valid and still owned here.
            unsafe { CloseHandle(lock_handle) };
            return Err(MappingError::SynchronizationOpenFailed(code));
        }
        let event_name = match wide_name(wake_event_name) {
            Ok(name) => name,
            Err(error) => {
                // SAFETY: both resources are valid and owned here.
                unsafe {
                    UnmapViewOfFile(lock_view);
                    CloseHandle(lock_handle);
                }
                return Err(error);
            }
        };
        // SAFETY: the name is NUL-terminated and the requested right is sufficient
        // for waking existing waiters when releasing the lock.
        let wake_event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr()) };
        if wake_event.is_null() {
            // SAFETY: this immediately follows the failed Win32 call.
            let code = unsafe { GetLastError() };
            // SAFETY: both resources are valid and owned here.
            unsafe {
                UnmapViewOfFile(lock_view);
                CloseHandle(lock_handle);
            }
            return Err(MappingError::SynchronizationOpenFailed(code));
        }
        Ok(Self {
            mapping,
            lock_handle,
            lock_view,
            wake_event,
        })
    }

    /// Copies one coherent snapshot after a bounded, non-blocking lock attempt.
    ///
    /// # Errors
    ///
    /// Returns [`MappingError::SynchronizationBusy`] when the producer retains the
    /// lock for all `max_spins` attempts.
    pub fn copy_owned(&self, max_spins: u32) -> Result<Vec<u8>, MappingError> {
        let waiters = self.lock_view.Value.cast::<AtomicI32>();
        // SAFETY: the mapped layout is `{ LONG waiters; LONG busy; }`, packed at its
        // natural four-byte alignment. A Windows mapping is page aligned.
        let busy = unsafe { &*waiters.add(1) };
        let acquired = (0..max_spins).any(|_| {
            if busy
                .compare_exchange(0, 1, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
            {
                true
            } else {
                hint::spin_loop();
                false
            }
        });
        if !acquired {
            return Err(MappingError::SynchronizationBusy);
        }
        let guard = SharedLockGuard {
            // SAFETY: the first LONG in the validated lock mapping is `waiters`.
            waiters: unsafe { &*waiters },
            busy,
            wake_event: self.wake_event,
        };
        let bytes = self.mapping.copy_owned();
        drop(guard);
        bytes
    }
}

impl Drop for SpinLockedMapping {
    fn drop(&mut self) {
        // SAFETY: all resources are owned by `self` and released exactly once. The
        // data mapping drops separately after these synchronization resources.
        unsafe {
            CloseHandle(self.wake_event);
            UnmapViewOfFile(self.lock_view);
            CloseHandle(self.lock_handle);
        }
    }
}

struct SharedLockGuard<'a> {
    waiters: &'a AtomicI32,
    busy: &'a AtomicI32,
    wake_event: HANDLE,
}

impl Drop for SharedLockGuard<'_> {
    fn drop(&mut self) {
        self.busy.store(0, Ordering::SeqCst);
        if self.waiters.load(Ordering::SeqCst) > 0 {
            // SAFETY: `wake_event` remains valid for the guard's lifetime. Failure to
            // wake is not recoverable here, but the lock itself has been released.
            unsafe { SetEvent(self.wake_event) };
        }
    }
}

fn wide_name(name: &str) -> Result<Vec<u16>, MappingError> {
    if name.is_empty() || name.encode_utf16().any(|unit| unit == 0) {
        return Err(MappingError::InvalidName);
    }
    Ok(name.encode_utf16().chain([0]).collect())
}
