use trace_domain::{
    ElapsedNanoseconds, FrameSequence, NativeTelemetrySample, SessionSeed, TelemetryFrame,
};
use trace_windows_shmem::{MappingError, NamedMapping};

use crate::{AcPageError, map_frame, map_session, pages};

const PHYSICS_MAPPING: &str = "acpmf_physics";
const GRAPHICS_MAPPING: &str = "acpmf_graphics";
const STATIC_MAPPING: &str = "acpmf_static";
const DEFAULT_STABILITY_ATTEMPTS: usize = 3;
const NATIVE_PAYLOAD_MAGIC: &[u8; 4] = b"ACSM";
const NATIVE_PAYLOAD_VERSION: u16 = 1;
const NATIVE_PAYLOAD_HEADER_LENGTH: usize = 20;

/// Decoder identifier for TRACE's lossless vanilla AC shared-memory envelope.
pub const AC_NATIVE_SCHEMA: &str = "assetto-corsa.shared-memory/1";

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
    InvalidNativePayload,
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

/// Borrowed pages decoded from one lossless AC-native telemetry payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcNativePages<'a> {
    pub physics: &'a [u8],
    pub graphics: &'a [u8],
    pub static_page: &'a [u8],
}

/// Packet-stable page prefixes suitable for a checked-in regression fixture.
///
/// Physics and graphics contain no player identity fields in TRACE's validated
/// prefixes. The static page is reconstructed from decoded version, car, track, and
/// temperature values so player names and unsupported bytes are never exported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcRedactedFixture {
    pub physics: Vec<u8>,
    pub graphics: Vec<u8>,
    pub static_page: Vec<u8>,
    pub shared_memory_version: Option<String>,
    pub assetto_corsa_version: Option<String>,
    pub car_model: Option<String>,
    pub track: Option<String>,
}

impl AcSnapshot {
    /// Creates an owned snapshot from captured page bytes after validating prefixes.
    ///
    /// This constructor supports version-labelled fixtures and alternate acquisition
    /// hosts without exposing mapped memory.
    ///
    /// # Errors
    ///
    /// Returns an error when any page lacks the currently validated vanilla prefix.
    pub fn from_pages(
        physics: Vec<u8>,
        graphics: Vec<u8>,
        static_page: Vec<u8>,
    ) -> Result<Self, AcCaptureError> {
        pages::PhysicsPage::parse(&physics)?;
        pages::GraphicsPage::parse(&graphics)?;
        pages::StaticPage::parse(&static_page)?;
        Ok(Self {
            physics,
            graphics,
            static_page,
        })
    }

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
        let mut frame = map_frame(&self.physics, &self.graphics, sequence, elapsed)?;
        frame.native = Some(Box::new(NativeTelemetrySample {
            schema: AC_NATIVE_SCHEMA.into(),
            payload: self.native_payload(),
            float_fields: pages::native_float_fields(
                &self.physics,
                &self.graphics,
                &self.static_page,
            ),
            integer_fields: pages::native_integer_fields(
                &self.physics,
                &self.graphics,
                &self.static_page,
            ),
            text_fields: pages::native_text_fields(&self.graphics, &self.static_page),
        }));
        Ok(frame)
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

    pub(crate) fn status(&self) -> i32 {
        pages::GraphicsPage::parse(&self.graphics)
            .expect("snapshot pages validated at acquisition")
            .status()
    }

    pub(crate) fn packet_signature(&self) -> (i32, i32) {
        let physics = pages::PhysicsPage::parse(&self.physics)
            .expect("snapshot pages validated at acquisition")
            .packet_id();
        let graphics = pages::GraphicsPage::parse(&self.graphics)
            .expect("snapshot pages validated at acquisition")
            .packet_id();
        (physics, graphics)
    }

    pub(crate) fn versions(&self) -> Result<(Option<String>, Option<String>), AcCaptureError> {
        let page = pages::StaticPage::parse(&self.static_page)?;
        Ok((page.shared_memory_version(), page.assetto_corsa_version()))
    }

    fn native_payload(&self) -> Vec<u8> {
        let mut payload = Vec::with_capacity(
            NATIVE_PAYLOAD_HEADER_LENGTH
                + self.physics.len()
                + self.graphics.len()
                + self.static_page.len(),
        );
        payload.extend_from_slice(NATIVE_PAYLOAD_MAGIC);
        payload.extend_from_slice(&NATIVE_PAYLOAD_VERSION.to_le_bytes());
        payload.extend_from_slice(&0_u16.to_le_bytes());
        for length in [
            self.physics.len(),
            self.graphics.len(),
            self.static_page.len(),
        ] {
            payload.extend_from_slice(&u32::try_from(length).unwrap_or(u32::MAX).to_le_bytes());
        }
        payload.extend_from_slice(&self.physics);
        payload.extend_from_slice(&self.graphics);
        payload.extend_from_slice(&self.static_page);
        payload
    }

    /// Produces page prefixes for regression testing without exporting personal data.
    ///
    /// # Errors
    ///
    /// Returns a capture error if the validated static prefix can no longer be mapped.
    pub fn redacted_fixture(&self) -> Result<AcRedactedFixture, AcCaptureError> {
        let (session, environment) = self.map_session()?;
        let (shared_memory_version, assetto_corsa_version) = self.versions()?;
        let static_page = pages::redacted_static_page(
            shared_memory_version.as_deref(),
            assetto_corsa_version.as_deref(),
            session.car_id.as_deref(),
            session.track_id.as_deref(),
            environment.and_then(|value| value.ambient_temperature_c),
            environment.and_then(|value| value.track_temperature_c),
        );
        Ok(AcRedactedFixture {
            physics: self.physics.clone(),
            graphics: self.graphics.clone(),
            static_page,
            shared_memory_version,
            assetto_corsa_version,
            car_model: session.car_id,
            track: session.track_id,
        })
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
            physics: NamedMapping::open(PHYSICS_MAPPING, pages::PHYSICS_PAGE_LENGTH)?,
            graphics: NamedMapping::open(GRAPHICS_MAPPING, pages::GRAPHICS_PAGE_LENGTH)?,
            static_page: NamedMapping::open(STATIC_MAPPING, pages::STATIC_PAGE_LENGTH)?,
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
        AcSnapshot::from_pages(physics, graphics, static_page)
    }
}

/// Decodes a lossless AC-native envelope without assigning semantics to page bytes.
///
/// # Errors
///
/// Returns [`AcCaptureError::InvalidNativePayload`] for an unknown, truncated, or
/// length-inconsistent envelope.
pub fn decode_native_payload(payload: &[u8]) -> Result<AcNativePages<'_>, AcCaptureError> {
    if payload.len() < NATIVE_PAYLOAD_HEADER_LENGTH
        || payload.get(..4) != Some(NATIVE_PAYLOAD_MAGIC)
        || read_u16(payload, 4) != Some(NATIVE_PAYLOAD_VERSION)
        || read_u16(payload, 6) != Some(0)
    {
        return Err(AcCaptureError::InvalidNativePayload);
    }
    let physics_length = read_u32(payload, 8).ok_or(AcCaptureError::InvalidNativePayload)? as usize;
    let graphics_length =
        read_u32(payload, 12).ok_or(AcCaptureError::InvalidNativePayload)? as usize;
    let static_length = read_u32(payload, 16).ok_or(AcCaptureError::InvalidNativePayload)? as usize;
    let physics_end = NATIVE_PAYLOAD_HEADER_LENGTH
        .checked_add(physics_length)
        .ok_or(AcCaptureError::InvalidNativePayload)?;
    let graphics_end = physics_end
        .checked_add(graphics_length)
        .ok_or(AcCaptureError::InvalidNativePayload)?;
    let static_end = graphics_end
        .checked_add(static_length)
        .ok_or(AcCaptureError::InvalidNativePayload)?;
    if static_end != payload.len() {
        return Err(AcCaptureError::InvalidNativePayload);
    }
    Ok(AcNativePages {
        physics: &payload[NATIVE_PAYLOAD_HEADER_LENGTH..physics_end],
        graphics: &payload[physics_end..graphics_end],
        static_page: &payload[graphics_end..static_end],
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    bytes
        .get(offset..offset + 2)?
        .try_into()
        .ok()
        .map(u16::from_le_bytes)
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    bytes
        .get(offset..offset + 4)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
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

    #[test]
    fn redacted_fixture_zeros_unsupported_static_regions() {
        let physics = vec![0; pages::PHYSICS_PREFIX_LENGTH];
        let graphics = vec![0; pages::GRAPHICS_PREFIX_LENGTH];
        let static_page = vec![0x41; pages::STATIC_PREFIX_LENGTH];
        let snapshot =
            AcSnapshot::from_pages(physics, graphics, static_page).expect("valid prefixes");

        let fixture = snapshot.redacted_fixture().expect("redacted fixture");
        assert!(fixture.static_page[200..456].iter().all(|byte| *byte == 0));
        assert_eq!(fixture.static_page.len(), pages::STATIC_PREFIX_LENGTH);
    }
}
