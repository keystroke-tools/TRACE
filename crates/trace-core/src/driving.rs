//! Conservative lap-level observations derived from repeated corner evidence.

use serde::{Deserialize, Serialize};

use crate::{
    analysis::{
        AlgorithmIdentity, AnalysisAvailability, AnalysisResult, ComparisonContext, Confidence,
        Derivation, MetricEvidence, UncertaintyReason,
    },
    corners::{CornerComparison, CornerComparisonSample, CornerPhase},
};
use trace_domain::{ChannelId, Unit};

const ALGORITHM_VERSION: u32 = 1;
const MINIMUM_PATTERN_CORNERS: usize = 2;
const HIGH_CONFIDENCE_CORNERS: usize = 3;
const DISTANCE_DIFFERENCE_M: f64 = 5.0;
const MINIMUM_SPEED_DIFFERENCE_KMH: f64 = 3.0;
const STEERING_DERIVATIVE_THRESHOLD: f64 = 2.5;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrivingObservationKind {
    BrakingEarlier,
    BrakingLater,
    BrakeReleaseEarlier,
    BrakeReleaseLater,
    LowerMinimumSpeed,
    LaterThrottle,
    EntryLoss,
    MidCornerLoss,
    ExitLoss,
    MoreSteeringCorrections,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationConfidenceTier {
    High,
    Low,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationUnit {
    Metre,
    KilometresPerHour,
    Second,
    Count,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrivingObservation {
    pub kind: DrivingObservationKind,
    pub tier: ObservationConfidenceTier,
    pub confidence: Confidence,
    pub corner_indices: Vec<u32>,
    pub eligible_corner_count: u32,
    pub mean_difference: f64,
    pub unit: ObservationUnit,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrivingAnalysis {
    pub observations: Vec<DrivingObservation>,
}

pub fn analyze_driving_comparison(
    corner_comparison: Option<&CornerComparison>,
    samples: &[CornerComparisonSample],
    context: ComparisonContext,
) -> AnalysisResult<DrivingAnalysis> {
    let algorithm = AlgorithmIdentity {
        key: "trace.driving-observations".into(),
        version: ALGORITHM_VERSION,
    };
    let Some(corner_comparison) = corner_comparison else {
        return AnalysisResult {
            schema_version: 1,
            algorithm,
            availability: AnalysisAvailability::InsufficientSamples,
            value: None,
            evidence: Vec::new(),
            confidence: confidence(0.0),
            uncertainty: vec![UncertaintyReason::Other(
                "corner analysis unavailable".into(),
            )],
            context,
        };
    };
    if corner_comparison.corners.is_empty() {
        return AnalysisResult {
            schema_version: 1,
            algorithm,
            availability: AnalysisAvailability::InsufficientSamples,
            value: None,
            evidence: Vec::new(),
            confidence: confidence(0.0),
            uncertainty: vec![UncertaintyReason::SparseSamples],
            context,
        };
    }

    let corners = &corner_comparison.corners;
    let mut observations = Vec::new();
    add_signed_pattern(
        &mut observations,
        corners.iter().filter_map(|corner| {
            corner
                .metrics
                .comparison_braking_point_m
                .zip(corner.metrics.reference_braking_point_m)
                .map(|(analysed, reference)| (corner.index, analysed - reference))
        }),
        DrivingObservationKind::BrakingLater,
        DrivingObservationKind::BrakingEarlier,
        DISTANCE_DIFFERENCE_M,
        ObservationUnit::Metre,
    );
    add_signed_pattern(
        &mut observations,
        corners.iter().filter_map(|corner| {
            corner
                .metrics
                .comparison_brake_release_point_m
                .zip(corner.metrics.reference_brake_release_point_m)
                .map(|(analysed, reference)| (corner.index, analysed - reference))
        }),
        DrivingObservationKind::BrakeReleaseLater,
        DrivingObservationKind::BrakeReleaseEarlier,
        DISTANCE_DIFFERENCE_M,
        ObservationUnit::Metre,
    );
    add_one_sided_pattern(
        &mut observations,
        corners.iter().filter_map(|corner| {
            corner
                .metrics
                .comparison_minimum_speed_kmh
                .zip(corner.metrics.reference_minimum_speed_kmh)
                .map(|(analysed, reference)| (corner.index, analysed - reference))
        }),
        DrivingObservationKind::LowerMinimumSpeed,
        |difference| difference <= -MINIMUM_SPEED_DIFFERENCE_KMH,
        ObservationUnit::KilometresPerHour,
    );
    add_one_sided_pattern(
        &mut observations,
        corners.iter().filter_map(|corner| {
            corner
                .metrics
                .comparison_throttle_point_m
                .zip(corner.metrics.reference_throttle_point_m)
                .map(|(analysed, reference)| (corner.index, analysed - reference))
        }),
        DrivingObservationKind::LaterThrottle,
        |difference| difference >= DISTANCE_DIFFERENCE_M,
        ObservationUnit::Metre,
    );
    add_dominant_loss(&mut observations, corner_comparison);
    add_steering_corrections(&mut observations, corner_comparison, samples);

    finalize_analysis(algorithm, observations, context)
}

fn finalize_analysis(
    algorithm: AlgorithmIdentity,
    mut observations: Vec<DrivingObservation>,
    context: ComparisonContext,
) -> AnalysisResult<DrivingAnalysis> {
    observations.sort_by(|left, right| {
        tier_rank(left.tier)
            .cmp(&tier_rank(right.tier))
            .then_with(|| right.confidence.get().total_cmp(&left.confidence.get()))
    });
    let overall_confidence = f64::from(
        observations
            .iter()
            .map(|observation| observation.confidence.get())
            .max_by(f32::total_cmp)
            .unwrap_or(0.5),
    );
    let evidence = observations
        .iter()
        .map(|observation| MetricEvidence {
            key: format!("driving.{:?}", observation.kind).to_lowercase(),
            value: observation.mean_difference,
            unit: evidence_unit(observation.unit),
            derivation: Derivation::HeuristicClassification,
            source_channels: source_channels(observation.kind),
            distance_range_m: None,
            uncertainty: None,
        })
        .collect();

    AnalysisResult {
        schema_version: 1,
        algorithm,
        availability: AnalysisAvailability::Available,
        value: Some(DrivingAnalysis { observations }),
        evidence,
        confidence: confidence(overall_confidence),
        uncertainty: vec![UncertaintyReason::Other(
            "rule-based observations do not establish driver or vehicle causality".into(),
        )],
        context,
    }
}

fn add_signed_pattern(
    observations: &mut Vec<DrivingObservation>,
    values: impl Iterator<Item = (u32, f64)>,
    positive_kind: DrivingObservationKind,
    negative_kind: DrivingObservationKind,
    threshold: f64,
    unit: ObservationUnit,
) {
    let values = values.collect::<Vec<_>>();
    let positive = values
        .iter()
        .copied()
        .filter(|(_, difference)| *difference >= threshold)
        .collect::<Vec<_>>();
    let negative = values
        .iter()
        .copied()
        .filter(|(_, difference)| *difference <= -threshold)
        .collect::<Vec<_>>();
    let (kind, supporting) = if positive.len() >= negative.len() {
        (positive_kind, positive)
    } else {
        (negative_kind, negative)
    };
    push_pattern(observations, kind, &supporting, values.len(), unit);
}

fn add_one_sided_pattern(
    observations: &mut Vec<DrivingObservation>,
    values: impl Iterator<Item = (u32, f64)>,
    kind: DrivingObservationKind,
    supports: impl Fn(f64) -> bool,
    unit: ObservationUnit,
) {
    let values = values.collect::<Vec<_>>();
    let supporting = values
        .iter()
        .copied()
        .filter(|(_, difference)| supports(*difference))
        .collect::<Vec<_>>();
    push_pattern(observations, kind, &supporting, values.len(), unit);
}

fn push_pattern(
    observations: &mut Vec<DrivingObservation>,
    kind: DrivingObservationKind,
    supporting: &[(u32, f64)],
    eligible_count: usize,
    unit: ObservationUnit,
) {
    if supporting.len() < MINIMUM_PATTERN_CORNERS || eligible_count == 0 {
        return;
    }
    let coverage = ratio(supporting.len(), eligible_count);
    if coverage < 0.4 {
        return;
    }
    let tier = if supporting.len() >= HIGH_CONFIDENCE_CORNERS && coverage >= 0.6 {
        ObservationConfidenceTier::High
    } else {
        ObservationConfidenceTier::Low
    };
    let score = if tier == ObservationConfidenceTier::High {
        (0.72 + coverage * 0.22).min(0.94)
    } else {
        (0.5 + coverage * 0.18).min(0.68)
    };
    observations.push(DrivingObservation {
        kind,
        tier,
        confidence: confidence(score),
        corner_indices: supporting.iter().map(|(index, _)| *index).collect(),
        eligible_corner_count: u32::try_from(eligible_count).unwrap_or(u32::MAX),
        mean_difference: mean_values(supporting),
        unit,
    });
}

fn add_dominant_loss(observations: &mut Vec<DrivingObservation>, comparison: &CornerComparison) {
    let losing = comparison
        .corners
        .iter()
        .filter(|corner| corner.total_loss_seconds.is_some_and(|loss| loss > 0.005))
        .collect::<Vec<_>>();
    if losing.len() < MINIMUM_PATTERN_CORNERS {
        return;
    }
    let phases = [
        (CornerPhase::Entry, DrivingObservationKind::EntryLoss),
        (CornerPhase::Mid, DrivingObservationKind::MidCornerLoss),
        (CornerPhase::Exit, DrivingObservationKind::ExitLoss),
    ];
    let totals = phases.map(|(phase, kind)| {
        let values = losing
            .iter()
            .filter_map(|corner| {
                corner
                    .phases
                    .iter()
                    .find(|value| value.phase == phase)
                    .and_then(|value| value.loss_seconds)
                    .filter(|loss| *loss > 0.0)
                    .map(|loss| (corner.index, loss))
            })
            .collect::<Vec<_>>();
        (kind, values)
    });
    let Some((kind, values)) = totals
        .into_iter()
        .max_by(|left, right| sum_values(&left.1).total_cmp(&sum_values(&right.1)))
    else {
        return;
    };
    if values.len() < MINIMUM_PATTERN_CORNERS {
        return;
    }
    let dominant_loss = sum_values(&values);
    let all_positive_loss = losing
        .iter()
        .flat_map(|corner| corner.phases.iter())
        .filter_map(|phase| phase.loss_seconds)
        .filter(|loss| *loss > 0.0)
        .sum::<f64>();
    if dominant_loss < 0.1 || all_positive_loss <= 0.0 || dominant_loss / all_positive_loss < 0.5 {
        return;
    }
    let coverage = ratio(values.len(), losing.len());
    let tier = if values.len() >= HIGH_CONFIDENCE_CORNERS && coverage >= 0.6 {
        ObservationConfidenceTier::High
    } else {
        ObservationConfidenceTier::Low
    };
    observations.push(DrivingObservation {
        kind,
        tier,
        confidence: confidence(if tier == ObservationConfidenceTier::High {
            (0.78 + dominant_loss / all_positive_loss * 0.16).min(0.94)
        } else {
            0.62
        }),
        corner_indices: values.iter().map(|(index, _)| *index).collect(),
        eligible_corner_count: u32::try_from(losing.len()).unwrap_or(u32::MAX),
        mean_difference: dominant_loss,
        unit: ObservationUnit::Second,
    });
}

fn add_steering_corrections(
    observations: &mut Vec<DrivingObservation>,
    comparison: &CornerComparison,
    samples: &[CornerComparisonSample],
) {
    let differences = comparison
        .corners
        .iter()
        .filter_map(|corner| {
            let start = samples
                .iter()
                .position(|sample| sample.distance_m >= corner.start_distance_m)?;
            let end = samples
                .iter()
                .rposition(|sample| sample.distance_m <= corner.end_distance_m)?;
            let reference = steering_reversals(&samples[start..=end], true)?;
            let analysed = steering_reversals(&samples[start..=end], false)?;
            (analysed > reference).then_some((corner.index, f64::from(analysed - reference)))
        })
        .collect::<Vec<_>>();
    let total = differences.iter().map(|(_, value)| *value).sum::<f64>();
    if differences.len() < 2 || total < 4.0 {
        return;
    }
    observations.push(DrivingObservation {
        kind: DrivingObservationKind::MoreSteeringCorrections,
        tier: ObservationConfidenceTier::Low,
        confidence: confidence(0.58),
        corner_indices: differences.iter().map(|(index, _)| *index).collect(),
        eligible_corner_count: u32::try_from(comparison.corners.len()).unwrap_or(u32::MAX),
        mean_difference: mean_values(&differences),
        unit: ObservationUnit::Count,
    });
}

fn steering_reversals(samples: &[CornerComparisonSample], reference: bool) -> Option<u32> {
    let values = samples
        .iter()
        .filter_map(|sample| {
            if reference {
                sample.reference_steering_percent
            } else {
                sample.comparison_steering_percent
            }
        })
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    if values.len() < 3 {
        return None;
    }
    let mut previous_direction = 0_i8;
    let mut reversals = 0_u32;
    for pair in values.windows(2) {
        let difference = pair[1] - pair[0];
        if difference.abs() < STEERING_DERIVATIVE_THRESHOLD {
            continue;
        }
        let direction = if difference > 0.0 { 1 } else { -1 };
        if previous_direction != 0 && direction != previous_direction {
            reversals = reversals.saturating_add(1);
        }
        previous_direction = direction;
    }
    Some(reversals)
}

fn sum_values(values: &[(u32, f64)]) -> f64 {
    values.iter().map(|(_, value)| *value).sum()
}

fn mean_values(values: &[(u32, f64)]) -> f64 {
    sum_values(values) / count_as_f64(values.len())
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    count_as_f64(numerator) / count_as_f64(denominator)
}

#[allow(clippy::cast_precision_loss)]
fn count_as_f64(value: usize) -> f64 {
    value as f64
}

const fn tier_rank(tier: ObservationConfidenceTier) -> u8 {
    match tier {
        ObservationConfidenceTier::High => 0,
        ObservationConfidenceTier::Low => 1,
    }
}

const fn evidence_unit(unit: ObservationUnit) -> Unit {
    match unit {
        ObservationUnit::Metre => Unit::Metre,
        ObservationUnit::KilometresPerHour => Unit::KilometresPerHour,
        ObservationUnit::Second => Unit::Second,
        ObservationUnit::Count => Unit::Count,
    }
}

fn source_channels(kind: DrivingObservationKind) -> Vec<ChannelId> {
    match kind {
        DrivingObservationKind::BrakingEarlier
        | DrivingObservationKind::BrakingLater
        | DrivingObservationKind::BrakeReleaseEarlier
        | DrivingObservationKind::BrakeReleaseLater => channels(&["inputs.brake", "lap.distance"]),
        DrivingObservationKind::LowerMinimumSpeed => channels(&["vehicle.speed", "lap.distance"]),
        DrivingObservationKind::LaterThrottle => channels(&["inputs.throttle", "lap.distance"]),
        DrivingObservationKind::EntryLoss
        | DrivingObservationKind::MidCornerLoss
        | DrivingObservationKind::ExitLoss => channels(&["lap.elapsed_time", "lap.distance"]),
        DrivingObservationKind::MoreSteeringCorrections => {
            channels(&["inputs.steering", "lap.distance"])
        }
    }
}

fn channels(values: &[&str]) -> Vec<ChannelId> {
    values.iter().map(|value| channel(value)).collect()
}

fn channel(value: &str) -> ChannelId {
    ChannelId::parse(value).expect("hard-coded canonical channel id must be valid")
}

#[allow(clippy::cast_possible_truncation)]
fn confidence(value: f64) -> Confidence {
    Confidence::new(value as f32).expect("calculated confidence must be within zero and one")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corners::{CornerAnalysis, CornerMetrics, CornerPhaseAnalysis};

    fn context() -> ComparisonContext {
        ComparisonContext {
            same_simulator: true,
            same_car: true,
            same_track_layout: true,
            setup_differs: None,
            conditions_differ: None,
        }
    }

    fn corner(
        index: u32,
        braking_difference: f64,
        throttle_difference: f64,
        exit_loss: f64,
    ) -> CornerAnalysis {
        let start = f64::from(index - 1) * 200.0;
        CornerAnalysis {
            index,
            label: format!("T{index}"),
            start_distance_m: start,
            apex_distance_m: start + 100.0,
            end_distance_m: start + 180.0,
            total_loss_seconds: Some(exit_loss + 0.02),
            phases: vec![
                CornerPhaseAnalysis {
                    phase: CornerPhase::Entry,
                    start_distance_m: start,
                    end_distance_m: start + 70.0,
                    loss_seconds: Some(0.01),
                },
                CornerPhaseAnalysis {
                    phase: CornerPhase::Mid,
                    start_distance_m: start + 70.0,
                    end_distance_m: start + 110.0,
                    loss_seconds: Some(0.01),
                },
                CornerPhaseAnalysis {
                    phase: CornerPhase::Exit,
                    start_distance_m: start + 110.0,
                    end_distance_m: start + 180.0,
                    loss_seconds: Some(exit_loss),
                },
            ],
            metrics: CornerMetrics {
                reference_braking_point_m: Some(start + 20.0),
                comparison_braking_point_m: Some(start + 20.0 + braking_difference),
                reference_brake_release_point_m: Some(start + 75.0),
                comparison_brake_release_point_m: Some(start + 75.0 + braking_difference),
                reference_peak_brake_percent: Some(70.0),
                comparison_peak_brake_percent: Some(75.0),
                reference_minimum_speed_kmh: Some(100.0),
                comparison_minimum_speed_kmh: Some(96.0),
                reference_throttle_point_m: Some(start + 120.0),
                comparison_throttle_point_m: Some(start + 120.0 + throttle_difference),
            },
        }
    }

    #[test]
    fn repeated_patterns_are_high_confidence_and_isolated_values_are_ignored() {
        let comparison = CornerComparison {
            corners: vec![
                corner(1, 10.0, 15.0, 0.12),
                corner(2, 12.0, 20.0, 0.14),
                corner(3, 8.0, 18.0, 0.16),
                corner(4, -2.0, 0.0, 0.01),
            ],
        };
        let result = analyze_driving_comparison(Some(&comparison), &[], context());
        let observations = result.value.expect("available").observations;
        assert!(
            observations
                .iter()
                .any(|value| value.kind == DrivingObservationKind::BrakingLater
                    && value.tier == ObservationConfidenceTier::High)
        );
        assert!(
            observations
                .iter()
                .any(|value| value.kind == DrivingObservationKind::LaterThrottle
                    && value.tier == ObservationConfidenceTier::High)
        );
        assert!(
            observations
                .iter()
                .any(|value| value.kind == DrivingObservationKind::ExitLoss)
        );
    }

    #[test]
    fn two_corner_pattern_remains_low_confidence() {
        let comparison = CornerComparison {
            corners: vec![
                corner(1, -10.0, 0.0, 0.03),
                corner(2, -8.0, 0.0, 0.03),
                corner(3, 0.0, 0.0, 0.03),
            ],
        };
        let result = analyze_driving_comparison(Some(&comparison), &[], context());
        let observation = result
            .value
            .expect("available")
            .observations
            .into_iter()
            .find(|value| value.kind == DrivingObservationKind::BrakingEarlier)
            .expect("earlier braking observation");
        assert_eq!(observation.tier, ObservationConfidenceTier::Low);
    }

    #[test]
    fn absent_corner_analysis_is_unavailable() {
        let result = analyze_driving_comparison(None, &[], context());
        assert_eq!(
            result.availability,
            AnalysisAvailability::InsufficientSamples
        );
        assert!(result.value.is_none());
    }
}
