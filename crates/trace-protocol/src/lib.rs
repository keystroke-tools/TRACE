//! Explicit, versioned wire models for TRACE services.
//!
//! These data-transfer objects deliberately do not mirror simulator structures or
//! canonical in-memory Rust types. Encoding and transport are separate concerns.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Current wire protocol version.
pub const PROTOCOL_VERSION: u16 = 1;

/// One-time installation credential returned to a desktop publisher.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InstallationCredentials {
    pub installation_id: String,
    pub publishing_token: String,
}

/// Metadata submitted when an unlisted live session is created.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreateLiveSessionRequest {
    pub session: SessionState,
}

/// Identifiers and routes returned to the desktop publisher.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreateLiveSessionResponse {
    pub session_id: String,
    pub publish_websocket_url: String,
    pub spectator_url: String,
}

/// Public metadata and retained sequence bounds for one live session.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LiveSessionSummary {
    pub session_id: String,
    pub session: SessionState,
    pub oldest_sequence: Option<u64>,
    pub newest_sequence: Option<u64>,
}

/// Resource and shape limits applied before accepting a message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProtocolLimits {
    pub max_samples_per_batch: usize,
    pub max_channels_per_batch: usize,
    pub max_text_bytes: usize,
}

impl Default for ProtocolLimits {
    fn default() -> Self {
        Self {
            max_samples_per_batch: 512,
            max_channels_per_batch: 64,
            max_text_bytes: 256,
        }
    }
}

/// Top-level ordered protocol message.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub protocol_version: u16,
    pub message_id: String,
    pub session_id: String,
    pub sequence: u64,
    pub sent_at_unix_ms: i64,
    pub payload: Payload,
}

impl Envelope {
    /// Validates version, identifiers, payload shape, and resource limits.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] for unsupported versions or malformed payloads.
    pub fn validate(&self, limits: ProtocolLimits) -> Result<(), ProtocolError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedVersion {
                found: self.protocol_version,
            });
        }
        validate_identifier(&self.message_id, 1, 64, IdentifierKind::Message)?;
        validate_identifier(&self.session_id, 16, 64, IdentifierKind::Session)?;
        self.payload.validate(limits)
    }
}

/// Payload variants understood by a v1 server or spectator.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Payload {
    Hello(Hello),
    SessionState(SessionState),
    TrackGeometry(TrackGeometry),
    TelemetryBatch(TelemetryBatch),
    LapEvent(LapEvent),
    Heartbeat,
    End(SessionEnd),
}

impl Payload {
    fn validate(&self, limits: ProtocolLimits) -> Result<(), ProtocolError> {
        match self {
            Self::Hello(hello) => {
                validate_text(&hello.publisher_version, limits.max_text_bytes)?;
                validate_text(&hello.source, limits.max_text_bytes)
            }
            Self::SessionState(state) => state.validate(limits),
            Self::TrackGeometry(geometry) => geometry.validate(),
            Self::TelemetryBatch(batch) => batch.validate(limits),
            Self::LapEvent(event) => event.validate(),
            Self::Heartbeat => Ok(()),
            Self::End(end) => validate_text(&end.reason, limits.max_text_bytes),
        }
    }
}

/// Publisher introduction. It contains TRACE source identity, never an AC struct.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub publisher_version: String,
    pub source: String,
}

/// Current session metadata suitable for spectators.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionState {
    pub driver_name: Option<String>,
    pub simulator: String,
    pub simulator_name: Option<String>,
    pub simulator_mark: Option<String>,
    pub car: Option<String>,
    pub track: Option<String>,
    pub layout: Option<String>,
    pub session_type: Option<String>,
    pub status: LiveStatus,
}

impl SessionState {
    fn validate(&self, limits: ProtocolLimits) -> Result<(), ProtocolError> {
        validate_text(&self.simulator, limits.max_text_bytes)?;
        for value in [
            self.driver_name.as_deref(),
            self.simulator_name.as_deref(),
            self.simulator_mark.as_deref(),
            self.car.as_deref(),
            self.track.as_deref(),
            self.layout.as_deref(),
            self.session_type.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            validate_text(value, limits.max_text_bytes)?;
        }
        Ok(())
    }
}

/// Simulator-provided world-space track geometry aligned with telemetry positions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackGeometry {
    pub centre_line: Vec<TrackPoint>,
    pub left_boundary: Vec<TrackPoint>,
    pub right_boundary: Vec<TrackPoint>,
}

impl TrackGeometry {
    fn validate(&self) -> Result<(), ProtocolError> {
        const MAX_POINTS: usize = 4_096;
        let point_count = self.centre_line.len();
        if !(3..=MAX_POINTS).contains(&point_count)
            || self.left_boundary.len() != point_count
            || self.right_boundary.len() != point_count
        {
            return Err(ProtocolError::InvalidTrackGeometry);
        }
        if self
            .centre_line
            .iter()
            .chain(&self.left_boundary)
            .chain(&self.right_boundary)
            .any(|point| !point.x_m.is_finite() || !point.z_m.is_finite())
        {
            return Err(ProtocolError::InvalidTrackGeometry);
        }
        Ok(())
    }
}

/// One horizontal world-space track point in metres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrackPoint {
    pub x_m: f32,
    pub z_m: f32,
}

/// Explicit live lifecycle shown to spectators.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveStatus {
    Live,
    Paused,
    Reconnecting,
    Ended,
}

/// A bounded columnar telemetry batch, normally sent around 20 Hz.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryBatch {
    pub base_elapsed_ns: u64,
    /// Strictly increasing offsets from `base_elapsed_ns`.
    pub offsets_ns: Vec<u32>,
    pub channels: Vec<ChannelColumn>,
}

impl TelemetryBatch {
    fn validate(&self, limits: ProtocolLimits) -> Result<(), ProtocolError> {
        if self.offsets_ns.is_empty() {
            return Err(ProtocolError::EmptyTelemetryBatch);
        }
        if self.offsets_ns.len() > limits.max_samples_per_batch {
            return Err(ProtocolError::TooManySamples);
        }
        if self.channels.is_empty() {
            return Err(ProtocolError::NoChannels);
        }
        if self.channels.len() > limits.max_channels_per_batch {
            return Err(ProtocolError::TooManyChannels);
        }
        if self.offsets_ns.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ProtocolError::NonIncreasingOffsets);
        }

        let mut channel_ids = BTreeSet::new();
        for channel in &self.channels {
            validate_channel_id(&channel.id)?;
            if !channel_ids.insert(&channel.id) {
                return Err(ProtocolError::DuplicateChannel);
            }
            if channel.values.len() != self.offsets_ns.len() {
                return Err(ProtocolError::ColumnLengthMismatch);
            }
            if channel
                .values
                .iter()
                .flatten()
                .any(|value| !value.is_finite())
            {
                return Err(ProtocolError::NonFiniteValue);
            }
        }
        Ok(())
    }
}

/// One nullable telemetry channel aligned with batch offsets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelColumn {
    pub id: String,
    pub unit: WireUnit,
    pub values: Vec<Option<f32>>,
}

/// Stable unit vocabulary used on the wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireUnit {
    Ratio,
    Metre,
    MetresPerSecond,
    Radian,
    RevolutionsPerMinute,
    Pascal,
    DegreeCelsius,
    Litre,
    Second,
    Unitless,
}

/// Discrete completed/invalid lap update.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LapEvent {
    pub lap_index: u32,
    pub duration_s: Option<f64>,
    pub validity: LapValidity,
}

impl LapEvent {
    fn validate(&self) -> Result<(), ProtocolError> {
        if self
            .duration_s
            .is_some_and(|duration| !duration.is_finite() || duration < 0.0)
        {
            return Err(ProtocolError::InvalidLapDuration);
        }
        Ok(())
    }
}

/// Conservative lap validity state.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LapValidity {
    Valid,
    Invalid,
    Unknown,
}

/// Explicit session end message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionEnd {
    pub reason: String,
}

/// Rejected wire data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    UnsupportedVersion { found: u16 },
    InvalidMessageId,
    InvalidSessionId,
    InvalidText,
    EmptyTelemetryBatch,
    TooManySamples,
    NoChannels,
    TooManyChannels,
    NonIncreasingOffsets,
    InvalidChannelId,
    DuplicateChannel,
    ColumnLengthMismatch,
    NonFiniteValue,
    InvalidTrackGeometry,
    InvalidLapDuration,
}

#[derive(Clone, Copy)]
enum IdentifierKind {
    Message,
    Session,
}

fn validate_identifier(
    value: &str,
    min_length: usize,
    max_length: usize,
    kind: IdentifierKind,
) -> Result<(), ProtocolError> {
    let valid = (min_length..=max_length).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if valid {
        Ok(())
    } else {
        Err(match kind {
            IdentifierKind::Message => ProtocolError::InvalidMessageId,
            IdentifierKind::Session => ProtocolError::InvalidSessionId,
        })
    }
}

fn validate_channel_id(value: &str) -> Result<(), ProtocolError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        });
    if valid {
        Ok(())
    } else {
        Err(ProtocolError::InvalidChannelId)
    }
}

fn validate_text(value: &str, max_bytes: usize) -> Result<(), ProtocolError> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        Err(ProtocolError::InvalidText)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(payload: Payload) -> Envelope {
        Envelope {
            protocol_version: PROTOCOL_VERSION,
            message_id: "message_1".into(),
            session_id: "K8c2Fx_valid_session".into(),
            sequence: 1,
            sent_at_unix_ms: 1_700_000_000_000,
            payload,
        }
    }

    fn batch() -> TelemetryBatch {
        TelemetryBatch {
            base_elapsed_ns: 10_000,
            offsets_ns: vec![0, 50_000_000],
            channels: vec![ChannelColumn {
                id: "vehicle.speed".into(),
                unit: WireUnit::MetresPerSecond,
                values: vec![Some(20.0), Some(21.0)],
            }],
        }
    }

    #[test]
    fn accepts_a_bounded_canonical_batch() {
        assert_eq!(
            envelope(Payload::TelemetryBatch(batch())).validate(ProtocolLimits::default()),
            Ok(())
        );
    }

    #[test]
    fn validates_aligned_finite_track_geometry() {
        let points = vec![
            TrackPoint { x_m: 0.0, z_m: 0.0 },
            TrackPoint { x_m: 1.0, z_m: 0.0 },
            TrackPoint { x_m: 1.0, z_m: 1.0 },
        ];
        let geometry = TrackGeometry {
            centre_line: points.clone(),
            left_boundary: points.clone(),
            right_boundary: points,
        };
        assert_eq!(
            envelope(Payload::TrackGeometry(geometry.clone())).validate(ProtocolLimits::default()),
            Ok(())
        );
        let mut invalid = geometry;
        invalid.right_boundary.pop();
        assert_eq!(
            envelope(Payload::TrackGeometry(invalid)).validate(ProtocolLimits::default()),
            Err(ProtocolError::InvalidTrackGeometry)
        );
    }

    #[test]
    fn rejects_unknown_versions_and_guessable_session_ids() {
        let mut message = envelope(Payload::Heartbeat);
        message.protocol_version = PROTOCOL_VERSION + 1;
        assert_eq!(
            message.validate(ProtocolLimits::default()),
            Err(ProtocolError::UnsupportedVersion { found: 2 })
        );

        message.protocol_version = PROTOCOL_VERSION;
        message.session_id = "short".into();
        assert_eq!(
            message.validate(ProtocolLimits::default()),
            Err(ProtocolError::InvalidSessionId)
        );
    }

    #[test]
    fn rejects_misaligned_duplicate_and_non_finite_channels() {
        let mut telemetry = batch();
        telemetry.channels[0].values.pop();
        assert_eq!(
            envelope(Payload::TelemetryBatch(telemetry)).validate(ProtocolLimits::default()),
            Err(ProtocolError::ColumnLengthMismatch)
        );

        let mut telemetry = batch();
        telemetry.channels.push(telemetry.channels[0].clone());
        assert_eq!(
            envelope(Payload::TelemetryBatch(telemetry)).validate(ProtocolLimits::default()),
            Err(ProtocolError::DuplicateChannel)
        );

        let mut telemetry = batch();
        telemetry.channels[0].values[1] = Some(f32::NAN);
        assert_eq!(
            envelope(Payload::TelemetryBatch(telemetry)).validate(ProtocolLimits::default()),
            Err(ProtocolError::NonFiniteValue)
        );
    }

    #[test]
    fn rejects_oversized_or_non_monotonic_batches() {
        let limits = ProtocolLimits {
            max_samples_per_batch: 1,
            ..ProtocolLimits::default()
        };
        assert_eq!(
            envelope(Payload::TelemetryBatch(batch())).validate(limits),
            Err(ProtocolError::TooManySamples)
        );

        let mut telemetry = batch();
        telemetry.offsets_ns = vec![10, 10];
        assert_eq!(
            envelope(Payload::TelemetryBatch(telemetry)).validate(ProtocolLimits::default()),
            Err(ProtocolError::NonIncreasingOffsets)
        );
    }
}
