//! Machine-consumable conventions shared by deterministic analyses.

use serde::{Deserialize, Serialize};
use trace_domain::{ChannelId, Unit};

/// Versioned algorithm identity for cache invalidation and diagnostics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AlgorithmIdentity {
    pub key: String,
    pub version: u32,
}

/// Whether an analysis could produce a result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AnalysisAvailability {
    Available,
    UnsupportedChannels(Vec<ChannelId>),
    InsufficientSamples,
    InvalidRange,
    IncomparableInputs,
}

/// Origin of a numerical fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Derivation {
    Measured,
    DeterministicDerived,
    HeuristicClassification,
}

/// Typed numerical evidence supporting an analysis result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricEvidence {
    pub key: String,
    pub value: f64,
    pub unit: Unit,
    pub derivation: Derivation,
    pub source_channels: Vec<ChannelId>,
    pub distance_range_m: Option<(f64, f64)>,
    pub uncertainty: Option<UncertaintyBounds>,
}

/// Symmetric or asymmetric bounds around a reported metric.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct UncertaintyBounds {
    pub lower: f64,
    pub upper: f64,
}

/// Explicit reason confidence was reduced or a result was unavailable.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum UncertaintyReason {
    MissingChannel(ChannelId),
    IntermittentChannel(ChannelId),
    SparseSamples,
    LargeTelemetryGap,
    DifferentSetup,
    DifferentConditions,
    SourceSemanticsUncertain,
    Other(String),
}

/// Calibrated confidence in the inclusive range 0..=1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Confidence(f32);

impl Confidence {
    /// Validates a confidence score.
    ///
    /// # Errors
    ///
    /// Returns [`ConfidenceError`] for non-finite values or values outside 0..=1.
    pub fn new(value: f32) -> Result<Self, ConfidenceError> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(ConfidenceError);
        }
        Ok(Self(value))
    }

    /// Returns the validated score.
    pub fn get(self) -> f32 {
        self.0
    }
}

/// Invalid confidence score.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfidenceError;

/// Setup and conditions that qualify a lap comparison.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComparisonContext {
    pub same_simulator: bool,
    pub same_car: bool,
    pub same_track_layout: bool,
    pub setup_differs: Option<bool>,
    pub conditions_differ: Option<bool>,
}

/// Standard envelope for every structured analysis payload.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnalysisResult<T> {
    pub schema_version: u32,
    pub algorithm: AlgorithmIdentity,
    pub availability: AnalysisAvailability,
    pub value: Option<T>,
    pub evidence: Vec<MetricEvidence>,
    pub confidence: Confidence,
    pub uncertainty: Vec<UncertaintyReason>,
    pub context: ComparisonContext,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_rejects_false_precision_outside_probability_range() {
        assert_eq!(Confidence::new(-0.1), Err(ConfidenceError));
        assert_eq!(Confidence::new(1.1), Err(ConfidenceError));
        assert_eq!(Confidence::new(f32::NAN), Err(ConfidenceError));
        assert!(
            (Confidence::new(0.87).expect("valid confidence").get() - 0.87).abs() < f32::EPSILON
        );
    }
}
