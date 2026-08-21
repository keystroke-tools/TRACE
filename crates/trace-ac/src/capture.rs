use trace_domain::{ElapsedNanoseconds, FrameSequence, SessionSeed, TelemetryFrame};
use trace_windows_shmem::{MappingError, NamedMapping};

use crate::{AcPageError, map_frame, map_session, pages};

const PHYSICS_MAPPING: &str = "acpmf_physics";
const GRAPHICS_MAPPING: &str = "acpmf_graphics";
const STATIC_MAPPING: &str = "acpmf_static";
const DEFAULT_STABILITY_ATTEMPTS: usize = 3;

/// Whether Assetto Corsa shared memory can be opened on this host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcAvailability {
    Available,
    NotRunning,
    UnsupportedPlatform,
}

/// Failure while detecting or snapshotting Assetto Corsa telemetry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AcCaptureError {
    Mapping(MappingError),
    InvalidPage(AcPageError),
    UnstablePacket { page: &'static str, attempts: usize },
}

impl From<MappingError> for AcCaptureError {
    fn from(value: MappingError) -> Self {
        Self::Mapping(value)
    }
}

impl From<AcPageError> for AcCaptureError {
    fn from(value: AcPageError) -> Self {
        Self::InvalidPage(value)
    }
}

/// One owned, packet-stable snapshot of the three vanilla AC pages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcSnapshot {
    physics: Vec<u8>,
    graphics: Vec<u8>,
    static_page: Vec<u8>,
}

impl AcSnapshot {
    /// Converts changing pages into one canonical telemetry frame.
    ///
    /// # Errors
    ///
    /// Returns an error when a captured page does not contain the validated prefix.
    pub fn map_frame(
        &self,
        sequence: FrameSequence,
        elapsed: ElapsedNanoseconds,
    ) -> Result<TelemetryFrame, AcCaptureError> {
        Ok(map_frame(&self.physics, &self.graphics, sequence, elapsed)?)
    }

    /// Extracts canonical session identity and environment from the static page.
    ///
    /// # Errors
    ///
    /// Returns an error when the captured static page lacks the validated prefix.
    pub fn map_session(
        &self,
    ) -> Result<(SessionSeed, Option<trace_domain::EnvironmentState>), AcCaptureError> {
        Ok(map_session(&self.static_page)?)
    }
}

/// Open read-only handles to vanilla Assetto Corsa shared-memory pages.
pub struct AcSharedMemory {
    physics: NamedMapping,
    graphics: NamedMapping,
    static_page: NamedMapping,
    stability_attempts: usize,
}

impl AcSharedMemory {
    /// Checks for the physics mapping without retaining a connection.
    ///
    /// # Errors
    ///
    /// Returns an acquisition error when Windows reports a failure other than a
    /// missing mapping or when an invalid mapping configuration is encountered.
    pub fn detect() -> Result<AcAvailability, AcCaptureError> {
        match NamedMapping::open(PHYSICS_MAPPING, pages::PHYSICS_PREFIX_LENGTH) {
            Ok(_) => Ok(AcAvailability::Available),
            Err(MappingError::NotFound) => Ok(AcAvailability::NotRunning),
            Err(MappingError::UnsupportedPlatform) => Ok(AcAvailability::UnsupportedPlatform),
            Err(error) => Err(error.into()),
        }
    }

    /// Opens all required vanilla Assetto Corsa mappings.
    ///
    /// # Errors
    ///
    /// Returns a mapping error if any required page cannot be opened.
    pub fn open() -> Result<Self, AcCaptureError> {
        Ok(Self {
            physics: NamedMapping::open(PHYSICS_MAPPING, pages::PHYSICS_PREFIX_LENGTH)?,
            graphics: NamedMapping::open(GRAPHICS_MAPPING, pages::GRAPHICS_PREFIX_LENGTH)?,
            static_page: NamedMapping::open(STATIC_MAPPING, pages::STATIC_PREFIX_LENGTH)?,
            stability_attempts: DEFAULT_STABILITY_ATTEMPTS,
        })
    }

    /// Copies a packet-stable owned snapshot of all required pages.
    ///
    /// # Errors
    ///
    /// Returns a mapping error or [`AcCaptureError::UnstablePacket`] when a changing
    /// page does not stabilize within the bounded retry count.
    pub fn snapshot(&mut self) -> Result<AcSnapshot, AcCaptureError> {
        let physics = stable_copy(&mut self.physics, PHYSICS_MAPPING, self.stability_attempts)?;
        let graphics = stable_copy(
            &mut self.graphics,
            GRAPHICS_MAPPING,
            self.stability_attempts,
        )?;
        let static_page = self.static_page.copy_owned()?;
        Ok(AcSnapshot {
            physics,
            graphics,
            static_page,
        })
    }
}

trait ChangingPage {
    fn packet_id(&mut self) -> Result<i32, MappingError>;
    fn copy_owned(&mut self) -> Result<Vec<u8>, MappingError>;
}

impl ChangingPage for NamedMapping {
    fn packet_id(&mut self) -> Result<i32, MappingError> {
        self.read_i32(0)
    }

    fn copy_owned(&mut self) -> Result<Vec<u8>, MappingError> {
        NamedMapping::copy_owned(self)
    }
}

fn stable_copy(
    page: &mut impl ChangingPage,
    page_name: &'static str,
    attempts: usize,
) -> Result<Vec<u8>, AcCaptureError> {
    for _ in 0..attempts {
        let before = page.packet_id()?;
        let bytes = page.copy_owned()?;
        let after = page.packet_id()?;
        let copied = bytes
            .get(..4)
            .and_then(|value| <[u8; 4]>::try_from(value).ok())
            .map(i32::from_le_bytes);
        if before == after && copied == Some(before) {
            return Ok(bytes);
        }
    }
    Err(AcCaptureError::UnstablePacket {
        page: page_name,
        attempts,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct ScriptedPage {
        packet_ids: VecDeque<i32>,
        copies: VecDeque<Vec<u8>>,
    }

    impl ChangingPage for ScriptedPage {
        fn packet_id(&mut self) -> Result<i32, MappingError> {
            Ok(self.packet_ids.pop_front().expect("scripted packet id"))
        }

        fn copy_owned(&mut self) -> Result<Vec<u8>, MappingError> {
            Ok(self.copies.pop_front().expect("scripted page copy"))
        }
    }

    fn page(packet_id: i32) -> Vec<u8> {
        let mut bytes = vec![0; 8];
        bytes[..4].copy_from_slice(&packet_id.to_le_bytes());
        bytes
    }

    #[test]
    fn retries_a_torn_packet_then_accepts_a_stable_copy() {
        let mut source = ScriptedPage {
            packet_ids: VecDeque::from([1, 2, 2, 2]),
            copies: VecDeque::from([page(1), page(2)]),
        };

        assert_eq!(
            stable_copy(&mut source, PHYSICS_MAPPING, 3).expect("stable retry"),
            page(2)
        );
    }

    #[test]
    fn rejects_a_copy_whose_embedded_packet_id_does_not_match() {
        let mut source = ScriptedPage {
            packet_ids: VecDeque::from([4, 4]),
            copies: VecDeque::from([page(3)]),
        };

        assert_eq!(
            stable_copy(&mut source, GRAPHICS_MAPPING, 1),
            Err(AcCaptureError::UnstablePacket {
                page: GRAPHICS_MAPPING,
                attempts: 1,
            })
        );
    }

    #[test]
    fn detection_is_explicitly_unsupported_off_windows() {
        #[cfg(not(windows))]
        assert_eq!(
            AcSharedMemory::detect(),
            Ok(AcAvailability::UnsupportedPlatform)
        );
    }
}
