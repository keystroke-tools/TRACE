//! Simulator-, storage-, and UI-agnostic telemetry types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Monotonic sequence number assigned by an adapter.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct FrameSequence(pub u64);

/// Time elapsed from the start of a telemetry stream.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ElapsedNanoseconds(pub u64);

/// An opaque simulator identifier such as `assetto-corsa`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct SimulatorId(String);

impl SimulatorId {
    /// Creates an identifier after validating its portable representation.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError`] when `value` is empty or contains unsupported
    /// characters.
    pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }

    /// Returns the portable string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A stable canonical channel identifier.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ChannelId(String);

impl ChannelId {
    /// Creates a channel identifier, for example `vehicle.speed`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError`] when `value` is empty or contains unsupported
    /// characters.
    pub fn parse(value: impl Into<String>) -> Result<Self, IdentifierError> {
        let value = value.into();
        validate_identifier(&value)?;
        Ok(Self(value))
    }

    /// Returns the portable string representation.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_identifier(value: &str) -> Result<(), IdentifierError> {
    if value.is_empty() {
        return Err(IdentifierError::Empty);
    }
    if value
        .bytes()
        .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)))
    {
        return Err(IdentifierError::InvalidCharacter);
    }
    Ok(())
}

/// Failure to construct a portable identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentifierError {
    /// The identifier was empty.
    Empty,
    /// The identifier contained a character outside `[a-z0-9-_.]`.
    InvalidCharacter,
}

/// Unit attached to a channel value.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Unit {
    Ratio,
    Metre,
    MetresPerSecond,
    MetresPerSecondSquared,
    Radian,
    RevolutionsPerMinute,
    Pascal,
    DegreeCelsius,
    Litre,
    Second,
    Unitless,
}

/// Availability of a channel for a source or session.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ChannelAvailability {
    Available,
    Intermittent,
    Unsupported,
    Unknown,
}

/// How a channel entered the canonical stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ValueProvenance {
    Measured,
    SimulatorDerived,
    TraceDerived,
}

/// Description and capability state of one channel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelDescriptor {
    pub id: ChannelId,
    pub unit: Unit,
    pub availability: ChannelAvailability,
    pub provenance: ValueProvenance,
    pub source_field: Option<String>,
}

/// Discoverable channels keyed by stable channel identifier.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelCapabilities {
    channels: BTreeMap<ChannelId, ChannelDescriptor>,
}

impl ChannelCapabilities {
    /// Adds or replaces a descriptor.
    pub fn insert(&mut self, descriptor: ChannelDescriptor) -> Option<ChannelDescriptor> {
        self.channels.insert(descriptor.id.clone(), descriptor)
    }

    /// Finds a channel descriptor.
    pub fn get(&self, id: &ChannelId) -> Option<&ChannelDescriptor> {
        self.channels.get(id)
    }

    /// Iterates in stable identifier order.
    pub fn iter(&self) -> impl Iterator<Item = (&ChannelId, &ChannelDescriptor)> {
        self.channels.iter()
    }
}

/// Coordinate frame for vector-valued telemetry.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CoordinateFrame {
    SourceWorld,
    TraceWorld,
    Vehicle,
}

/// A three-dimensional vector whose frame is explicit.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vector3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub frame: CoordinateFrame,
}

/// Driver-controlled inputs for one sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DriverInputs {
    pub throttle: Option<f32>,
    pub brake: Option<f32>,
    pub clutch: Option<f32>,
    pub steering_angle_rad: Option<f32>,
}

/// Canonical gear state, independent of simulator encoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Gear {
    Reverse,
    Neutral,
    Forward(u8),
    Unknown(i16),
}

/// Vehicle-wide state for one sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VehicleState {
    pub speed_mps: Option<f32>,
    pub engine_rpm: Option<f32>,
    pub gear: Option<Gear>,
    pub fuel_litres: Option<f32>,
}

/// Position and motion state for one sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MotionState {
    pub position_m: Option<Vector3>,
    pub velocity_mps: Option<Vector3>,
    pub acceleration_mps2: Option<Vector3>,
}

/// Fixed wheel location, normalized by the adapter.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum WheelCorner {
    FrontLeft,
    FrontRight,
    RearLeft,
    RearRight,
}

/// State associated with one wheel and tyre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WheelState {
    pub angular_speed_rad_s: Option<f32>,
    pub tyre_pressure_pa: Option<f32>,
    pub tyre_core_temperature_c: Option<f32>,
    pub suspension_travel_m: Option<f32>,
}

/// Four-corner wheel state.
pub type WheelStates = BTreeMap<WheelCorner, WheelState>;

/// Simulator-reported lap observations. None are assumed authoritative without
/// later lap processing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LapObservation {
    pub completed_laps: Option<u32>,
    pub normalized_position: Option<f32>,
    pub current_lap_time_ns: Option<u64>,
    pub simulator_distance_m: Option<f64>,
    pub current_sector_index: Option<u32>,
    pub last_sector_time_ns: Option<u64>,
}

/// Environmental state sampled or scoped to the current session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentState {
    pub ambient_temperature_c: Option<f32>,
    pub track_temperature_c: Option<f32>,
    pub track_grip: Option<f32>,
}

/// One canonical sample produced by any adapter.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TelemetryFrame {
    pub sequence: FrameSequence,
    pub elapsed: ElapsedNanoseconds,
    pub lap: LapObservation,
    pub inputs: DriverInputs,
    pub vehicle: VehicleState,
    pub motion: MotionState,
    pub wheels: WheelStates,
    pub environment: Option<EnvironmentState>,
}

/// Identity and version of a telemetry source.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceDescriptor {
    pub simulator: SimulatorId,
    pub adapter_version: String,
    pub simulator_version: Option<String>,
    pub kind: SourceKind,
}

/// How telemetry entered TRACE before canonical mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    NativeCapture,
    SimulatorReplay,
    Imported,
}

/// Metadata known when a source starts or changes session.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionSeed {
    pub source_session_id: Option<String>,
    pub car_id: Option<String>,
    pub track_id: Option<String>,
    pub layout_id: Option<String>,
    pub session_type: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_identifiers_are_validated() {
        assert_eq!(
            SimulatorId::parse("Assetto Corsa"),
            Err(IdentifierError::InvalidCharacter)
        );
        assert_eq!(ChannelId::parse(""), Err(IdentifierError::Empty));
        assert_eq!(
            ChannelId::parse("vehicle.speed")
                .expect("valid channel")
                .as_str(),
            "vehicle.speed"
        );
    }

    #[test]
    fn capabilities_replace_a_channel_without_duplicates() {
        let id = ChannelId::parse("vehicle.speed").expect("valid channel");
        let mut capabilities = ChannelCapabilities::default();
        let descriptor = |availability| ChannelDescriptor {
            id: id.clone(),
            unit: Unit::MetresPerSecond,
            availability,
            provenance: ValueProvenance::Measured,
            source_field: Some("speedKmh".into()),
        };

        assert!(
            capabilities
                .insert(descriptor(ChannelAvailability::Unknown))
                .is_none()
        );
        assert!(
            capabilities
                .insert(descriptor(ChannelAvailability::Available))
                .is_some()
        );
        assert_eq!(capabilities.iter().count(), 1);
        assert_eq!(
            capabilities.get(&id).map(|entry| entry.availability),
            Some(ChannelAvailability::Available)
        );
    }
}
