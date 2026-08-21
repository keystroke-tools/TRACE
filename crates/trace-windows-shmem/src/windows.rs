use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, GetLastError, HANDLE};
use windows_sys::Win32::System::Memory::{
    FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile,
};

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
