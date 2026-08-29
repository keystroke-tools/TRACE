//! Deterministic corner detection and distance-domain loss decomposition.

use serde::{Deserialize, Serialize};
use trace_domain::{ChannelId, Unit};

use crate::analysis::{
    AlgorithmIdentity, AnalysisAvailability, AnalysisResult, ComparisonContext, Confidence,
    Derivation, MetricEvidence, UncertaintyReason,
};

const ALGORITHM_VERSION: u32 = 3;
const MINIMUM_SAMPLES: usize = 12;
const BRAKE_ACTIVE_PERCENT: f64 = 5.0;
const STEERING_ACTIVE_PERCENT: f64 = 5.0;
const PATH_TURN_ACTIVE_RADIANS: f64 = 0.045;
const MAXIMUM_INACTIVE_GAP_M: f64 = 30.0;
const MINIMUM_CORNER_SPAN_M: f64 = 20.0;
const METRIC_BRAKE_PERCENT: f64 = 10.0;
const METRIC_THROTTLE_PERCENT: f64 = 20.0;
/// Maximum distance searched before the reference-defined corner region. The previous
/// detected corner remains a hard boundary even when it is closer than this limit.
const MAXIMUM_BRAKING_LOOKBACK_M: f64 = 300.0;
/// Joins active brake samples across a short release or a sparse distance-grid sample.
const MAXIMUM_BRAKE_GAP_M: f64 = 15.0;
/// Keeps a compound range from comparing minimum-speed points from different bends.
const MAXIMUM_APEX_OFFSET_M: f64 = 75.0;
/// A larger discrepancy is more likely to be two different braking events than a
/// meaningful same-corner comparison. Unavailable is safer than a false coaching cue.
const MAXIMUM_BRAKING_POINT_DIFFERENCE_M: f64 = 125.0;
const MINIMUM_APEX_SEPARATION_M: f64 = 50.0;
const MINIMUM_SPEED_RECOVERY_KMH: f64 = 10.0;

/// One distance-aligned pair of lap observations used by corner analysis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CornerComparisonSample {
    pub distance_m: f64,
    pub delta_s: Option<f64>,
    pub reference_speed_kmh: Option<f64>,
    pub comparison_speed_kmh: Option<f64>,
    pub reference_throttle_percent: Option<f64>,
    pub comparison_throttle_percent: Option<f64>,
    pub reference_brake_percent: Option<f64>,
    pub comparison_brake_percent: Option<f64>,
    pub reference_steering_percent: Option<f64>,
    pub comparison_steering_percent: Option<f64>,
    pub reference_position_x_m: Option<f64>,
    pub reference_position_z_m: Option<f64>,
    pub comparison_position_x_m: Option<f64>,
    pub comparison_position_z_m: Option<f64>,
}

/// The telemetry-defined part of a corner.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerPhase {
    Entry,
    Mid,
    Exit,
}

/// Time gained or lost through one contiguous corner phase.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CornerPhaseAnalysis {
    pub phase: CornerPhase,
    pub start_distance_m: f64,
    pub end_distance_m: f64,
    /// Positive means the comparison lost time to the reference.
    pub loss_seconds: Option<f64>,
}

/// Driver-facing metrics retained as explicit reference/comparison facts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CornerMetrics {
    pub reference_braking_point_m: Option<f64>,
    pub comparison_braking_point_m: Option<f64>,
    pub reference_brake_release_point_m: Option<f64>,
    pub comparison_brake_release_point_m: Option<f64>,
    pub reference_peak_brake_percent: Option<f64>,
    pub comparison_peak_brake_percent: Option<f64>,
    pub reference_minimum_speed_kmh: Option<f64>,
    pub comparison_minimum_speed_kmh: Option<f64>,
    pub reference_throttle_point_m: Option<f64>,
    pub comparison_throttle_point_m: Option<f64>,
}

/// Stable distance range and deterministic comparison for one detected corner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CornerAnalysis {
    pub index: u32,
    pub label: String,
    pub start_distance_m: f64,
    pub apex_distance_m: f64,
    pub end_distance_m: f64,
    /// Positive means the comparison lost time to the reference.
    pub total_loss_seconds: Option<f64>,
    pub phases: Vec<CornerPhaseAnalysis>,
    pub metrics: CornerMetrics,
}

/// Detected corners in lap order. Ranking is a presentation concern.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CornerComparison {
    pub corners: Vec<CornerAnalysis>,
}

/// Detects corners from reference-lap controls and trajectory, then decomposes the
/// cumulative comparison delta into entry, mid and exit ranges.
///
/// Detection never invents missing telemetry. The value is unavailable when speed
/// or all usable direction/braking signals are absent.
#[allow(clippy::too_many_lines)]
pub fn analyze_corner_comparison(
    samples: &[CornerComparisonSample],
    context: ComparisonContext,
) -> AnalysisResult<CornerComparison> {
    let algorithm = AlgorithmIdentity {
        key: "trace.corner-comparison".into(),
        version: ALGORITHM_VERSION,
    };
    let unavailable = |availability, uncertainty| AnalysisResult {
        schema_version: 1,
        algorithm: algorithm.clone(),
        availability,
        value: None,
        evidence: Vec::new(),
        confidence: confidence(0.0),
        uncertainty,
        context: context.clone(),
    };

    if samples.len() < MINIMUM_SAMPLES {
        return unavailable(
            AnalysisAvailability::InsufficientSamples,
            vec![UncertaintyReason::SparseSamples],
        );
    }
    if !has_valid_distances(samples) {
        return unavailable(AnalysisAvailability::InvalidRange, Vec::new());
    }
    if !samples
        .iter()
        .any(|sample| sample.reference_speed_kmh.is_some())
    {
        return unavailable(
            AnalysisAvailability::UnsupportedChannels(vec![channel("vehicle.speed")]),
            vec![UncertaintyReason::MissingChannel(channel("vehicle.speed"))],
        );
    }

    let has_brake = samples
        .iter()
        .any(|sample| sample.reference_brake_percent.is_some());
    let has_steering = samples
        .iter()
        .any(|sample| sample.reference_steering_percent.is_some());
    let has_path = samples
        .iter()
        .any(|sample| reference_position(*sample).is_some());
    if !has_brake && !has_steering && !has_path {
        let channels = vec![
            channel("inputs.brake"),
            channel("inputs.steering"),
            channel("motion.position"),
        ];
        return unavailable(
            AnalysisAvailability::UnsupportedChannels(channels.clone()),
            channels
                .into_iter()
                .map(UncertaintyReason::MissingChannel)
                .collect(),
        );
    }

    let active = (0..samples.len())
        .map(|index| {
            samples[index]
                .reference_brake_percent
                .is_some_and(|value| value >= BRAKE_ACTIVE_PERCENT)
                || samples[index]
                    .reference_steering_percent
                    .is_some_and(|value| value.abs() >= STEERING_ACTIVE_PERCENT)
                || path_turn_angle(samples, index)
                    .is_some_and(|value| value.abs() >= PATH_TURN_ACTIVE_RADIANS)
        })
        .collect::<Vec<_>>();
    let ranges = split_compound_ranges(samples, &active_ranges(samples, &active));
    let corners = ranges
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, (start, end))| {
            let lower_bound = index
                .checked_sub(1)
                .and_then(|previous| ranges.get(previous))
                .map_or(0, |(_, previous_end)| {
                    previous_end.saturating_add(1).min(start)
                });
            let distance_floor = samples[start].distance_m - MAXIMUM_BRAKING_LOOKBACK_M;
            let braking_search_start = (lower_bound..=start)
                .find(|sample_index| samples[*sample_index].distance_m >= distance_floor)
                .unwrap_or(lower_bound);
            build_corner(samples, index, start, end, braking_search_start)
        })
        .collect::<Vec<_>>();

    let evidence = corners
        .iter()
        .filter_map(|corner| {
            corner.total_loss_seconds.map(|value| MetricEvidence {
                key: format!("corner.{}.time_loss", corner.index),
                value,
                unit: Unit::Second,
                derivation: Derivation::DeterministicDerived,
                source_channels: vec![channel("lap.elapsed_time"), channel("lap.distance")],
                distance_range_m: Some((corner.start_distance_m, corner.end_distance_m)),
                uncertainty: None,
            })
        })
        .collect();
    let mut uncertainty = Vec::new();
    if !has_brake {
        uncertainty.push(UncertaintyReason::MissingChannel(channel("inputs.brake")));
    }
    if !samples
        .iter()
        .any(|sample| sample.reference_throttle_percent.is_some())
    {
        uncertainty.push(UncertaintyReason::MissingChannel(channel(
            "inputs.throttle",
        )));
    }

    AnalysisResult {
        schema_version: 1,
        algorithm,
        availability: AnalysisAvailability::Available,
        value: Some(CornerComparison { corners }),
        evidence,
        confidence: confidence(if has_path && has_brake { 0.82 } else { 0.68 }),
        uncertainty,
        context,
    }
}

fn has_valid_distances(samples: &[CornerComparisonSample]) -> bool {
    samples.iter().enumerate().all(|(index, sample)| {
        sample.distance_m.is_finite()
            && sample.distance_m >= 0.0
            && (index == 0 || sample.distance_m > samples[index - 1].distance_m)
    })
}

fn active_ranges(samples: &[CornerComparisonSample], active: &[bool]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = None;
    let mut last_active = 0;
    for (index, is_active) in active.iter().copied().enumerate() {
        if !is_active {
            continue;
        }
        if let Some(current_start) = start {
            if samples[index].distance_m - samples[last_active].distance_m > MAXIMUM_INACTIVE_GAP_M
            {
                push_range(samples, &mut ranges, current_start, last_active);
                start = Some(index);
            }
        } else {
            start = Some(index);
        }
        last_active = index;
    }
    if let Some(current_start) = start {
        push_range(samples, &mut ranges, current_start, last_active);
    }
    ranges
}

fn push_range(
    samples: &[CornerComparisonSample],
    ranges: &mut Vec<(usize, usize)>,
    start: usize,
    end: usize,
) {
    if samples[end].distance_m - samples[start].distance_m >= MINIMUM_CORNER_SPAN_M {
        ranges.push((start, end));
    }
}

/// Splits a continuously active steering/path range when the car reaches two distinct
/// speed minima with a meaningful speed recovery between them. The intervening maximum
/// becomes the boundary, so the next corner's braking search cannot leak backwards.
fn split_compound_ranges(
    samples: &[CornerComparisonSample],
    ranges: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    ranges
        .iter()
        .flat_map(|(start, end)| {
            let apexes = local_speed_minima(samples, *start, *end);
            if apexes.len() < 2 {
                return vec![(*start, *end)];
            }
            let mut boundaries = Vec::new();
            let mut previous_apex = apexes[0];
            for apex in apexes.into_iter().skip(1) {
                let separated = samples[apex].distance_m - samples[previous_apex].distance_m
                    >= MINIMUM_APEX_SEPARATION_M;
                let recovery = maximum_speed_index(samples, previous_apex, apex);
                let recovered = recovery.is_some_and(|recovery_index| {
                    let recovery_speed = samples[recovery_index].reference_speed_kmh;
                    let previous_speed = samples[previous_apex].reference_speed_kmh;
                    let next_speed = samples[apex].reference_speed_kmh;
                    recovery_speed
                        .zip(previous_speed.zip(next_speed))
                        .is_some_and(|(recovery_speed, (previous_speed, next_speed))| {
                            recovery_speed - previous_speed.max(next_speed)
                                >= MINIMUM_SPEED_RECOVERY_KMH
                        })
                });
                if separated && recovered {
                    if let Some(boundary) = recovery {
                        boundaries.push(boundary);
                    }
                    previous_apex = apex;
                } else if samples[apex].reference_speed_kmh.unwrap_or(f64::INFINITY)
                    < samples[previous_apex]
                        .reference_speed_kmh
                        .unwrap_or(f64::INFINITY)
                {
                    previous_apex = apex;
                }
            }
            if boundaries.is_empty() {
                return vec![(*start, *end)];
            }
            let mut split = Vec::with_capacity(boundaries.len() + 1);
            let mut range_start = *start;
            for boundary in boundaries {
                if samples[boundary].distance_m - samples[range_start].distance_m
                    >= MINIMUM_CORNER_SPAN_M
                {
                    split.push((range_start, boundary));
                    range_start = boundary.saturating_add(1).min(*end);
                }
            }
            if samples[*end].distance_m - samples[range_start].distance_m >= MINIMUM_CORNER_SPAN_M {
                split.push((range_start, *end));
            }
            if split.is_empty() {
                vec![(*start, *end)]
            } else {
                split
            }
        })
        .collect()
}

fn local_speed_minima(samples: &[CornerComparisonSample], start: usize, end: usize) -> Vec<usize> {
    if end.saturating_sub(start) < 2 {
        return Vec::new();
    }
    (start.saturating_add(1)..end)
        .filter(|index| {
            let Some(speed) = samples[*index].reference_speed_kmh else {
                return false;
            };
            samples[*index - 1]
                .reference_speed_kmh
                .zip(samples[*index + 1].reference_speed_kmh)
                .is_some_and(|(before, after)| speed <= before && speed < after)
        })
        .collect()
}

fn maximum_speed_index(
    samples: &[CornerComparisonSample],
    start: usize,
    end: usize,
) -> Option<usize> {
    (start..=end)
        .filter_map(|index| {
            samples[index]
                .reference_speed_kmh
                .filter(|speed| speed.is_finite())
                .map(|speed| (index, speed))
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
}

fn build_corner(
    samples: &[CornerComparisonSample],
    zero_based_index: usize,
    start: usize,
    end: usize,
    braking_search_start: usize,
) -> Option<CornerAnalysis> {
    let apex = minimum_speed_index(samples, start, end, true)?;
    let mut entry_end = (start..=apex)
        .rev()
        .find(|index| {
            samples[*index]
                .reference_brake_percent
                .is_some_and(|value| value >= BRAKE_ACTIVE_PERCENT)
        })
        .unwrap_or_else(|| apex.saturating_sub(1).max(start));
    let mut exit_start = (apex..=end)
        .find(|index| {
            samples[*index]
                .reference_throttle_percent
                .is_some_and(|value| value >= METRIC_THROTTLE_PERCENT)
        })
        .unwrap_or((apex + 1).min(end));
    if entry_end >= exit_start {
        entry_end = apex.saturating_sub(1).max(start);
        exit_start = (apex + 1).min(end);
    }

    let phases = vec![
        phase(samples, CornerPhase::Entry, start, entry_end),
        phase(samples, CornerPhase::Mid, entry_end, exit_start),
        phase(samples, CornerPhase::Exit, exit_start, end),
    ];
    let total_loss_seconds = loss_between(samples, start, end);
    let comparison_apex = minimum_speed_index_near(
        samples,
        start,
        end,
        false,
        samples[apex].distance_m,
        MAXIMUM_APEX_OFFSET_M,
    );
    let reference_braking_zone =
        braking_zone(samples, braking_search_start, apex, end, None, |sample| {
            sample.reference_brake_percent
        });
    let comparison_braking_zone = braking_zone(
        samples,
        braking_search_start,
        comparison_apex.unwrap_or(apex),
        end,
        reference_braking_zone.map(|zone| zone.start_distance_m),
        |sample| sample.comparison_brake_percent,
    );
    let metrics = CornerMetrics {
        reference_braking_point_m: reference_braking_zone.map(|zone| zone.start_distance_m),
        comparison_braking_point_m: comparison_braking_zone.map(|zone| zone.start_distance_m),
        reference_brake_release_point_m: reference_braking_zone.map(|zone| zone.release_distance_m),
        comparison_brake_release_point_m: comparison_braking_zone
            .map(|zone| zone.release_distance_m),
        reference_peak_brake_percent: reference_braking_zone.map(|zone| zone.peak_percent),
        comparison_peak_brake_percent: comparison_braking_zone.map(|zone| zone.peak_percent),
        reference_minimum_speed_kmh: samples[apex].reference_speed_kmh,
        comparison_minimum_speed_kmh: comparison_apex
            .and_then(|index| samples[index].comparison_speed_kmh),
        reference_throttle_point_m: threshold_point(
            samples,
            apex,
            end,
            |sample| sample.reference_throttle_percent,
            METRIC_THROTTLE_PERCENT,
        ),
        comparison_throttle_point_m: comparison_apex.and_then(|comparison_apex| {
            threshold_point(
                samples,
                comparison_apex,
                end,
                |sample| sample.comparison_throttle_percent,
                METRIC_THROTTLE_PERCENT,
            )
        }),
    };
    let index = u32::try_from(zero_based_index + 1).ok()?;
    Some(CornerAnalysis {
        index,
        label: format!("T{index}"),
        start_distance_m: samples[start].distance_m,
        apex_distance_m: samples[apex].distance_m,
        end_distance_m: samples[end].distance_m,
        total_loss_seconds,
        phases,
        metrics,
    })
}

fn phase(
    samples: &[CornerComparisonSample],
    phase: CornerPhase,
    start: usize,
    end: usize,
) -> CornerPhaseAnalysis {
    CornerPhaseAnalysis {
        phase,
        start_distance_m: samples[start].distance_m,
        end_distance_m: samples[end].distance_m,
        loss_seconds: loss_between(samples, start, end),
    }
}

fn loss_between(samples: &[CornerComparisonSample], start: usize, end: usize) -> Option<f64> {
    samples[start]
        .delta_s
        .zip(samples[end].delta_s)
        .map(|(start_delta, end_delta)| end_delta - start_delta)
}

fn minimum_speed_index(
    samples: &[CornerComparisonSample],
    start: usize,
    end: usize,
    reference: bool,
) -> Option<usize> {
    (start..=end)
        .filter_map(|index| {
            let speed = if reference {
                samples[index].reference_speed_kmh
            } else {
                samples[index].comparison_speed_kmh
            }?;
            speed.is_finite().then_some((index, speed))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
}

fn minimum_speed_index_near(
    samples: &[CornerComparisonSample],
    start: usize,
    end: usize,
    reference: bool,
    anchor_distance_m: f64,
    maximum_offset_m: f64,
) -> Option<usize> {
    (start..=end)
        .filter(|index| (samples[*index].distance_m - anchor_distance_m).abs() <= maximum_offset_m)
        .filter_map(|index| {
            let speed = if reference {
                samples[index].reference_speed_kmh
            } else {
                samples[index].comparison_speed_kmh
            }?;
            speed.is_finite().then_some((index, speed))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(index, _)| index)
}

fn threshold_point(
    samples: &[CornerComparisonSample],
    start: usize,
    end: usize,
    value: impl Fn(CornerComparisonSample) -> Option<f64>,
    threshold: f64,
) -> Option<f64> {
    samples[start..=end]
        .iter()
        .copied()
        .find(|sample| value(*sample).is_some_and(|value| value >= threshold))
        .map(|sample| sample.distance_m)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BrakingZone {
    start_distance_m: f64,
    release_distance_m: f64,
    peak_percent: f64,
}

fn braking_zone(
    samples: &[CornerComparisonSample],
    start: usize,
    apex: usize,
    end: usize,
    preferred_start_distance_m: Option<f64>,
    value: impl Fn(CornerComparisonSample) -> Option<f64>,
) -> Option<BrakingZone> {
    let mut zones = Vec::new();
    let mut current: Option<(usize, usize, f64)> = None;
    for index in start..=end {
        let Some(pressure) = value(samples[index])
            .filter(|pressure| pressure.is_finite() && *pressure >= METRIC_BRAKE_PERCENT)
        else {
            continue;
        };
        if let Some((zone_start, last_active, peak)) = current {
            if samples[index].distance_m - samples[last_active].distance_m <= MAXIMUM_BRAKE_GAP_M {
                current = Some((zone_start, index, peak.max(pressure)));
            } else {
                zones.push((zone_start, last_active, peak));
                current = Some((index, index, pressure));
            }
        } else {
            current = Some((index, index, pressure));
        }
    }
    if let Some(zone) = current {
        zones.push(zone);
    }
    zones
        .into_iter()
        .filter(|(zone_start, _, _)| *zone_start <= apex)
        .filter(|(zone_start, _, _)| {
            preferred_start_distance_m.is_none_or(|preferred| {
                (samples[*zone_start].distance_m - preferred).abs()
                    <= MAXIMUM_BRAKING_POINT_DIFFERENCE_M
            })
        })
        .min_by(|left, right| {
            let left_distance = preferred_start_distance_m.map_or_else(
                || (samples[left.1].distance_m - samples[apex].distance_m).abs(),
                |preferred| (samples[left.0].distance_m - preferred).abs(),
            );
            let right_distance = preferred_start_distance_m.map_or_else(
                || (samples[right.1].distance_m - samples[apex].distance_m).abs(),
                |preferred| (samples[right.0].distance_m - preferred).abs(),
            );
            left_distance.total_cmp(&right_distance)
        })
        .map(|(zone_start, release, peak_percent)| BrakingZone {
            start_distance_m: samples[zone_start].distance_m,
            release_distance_m: samples[release].distance_m,
            peak_percent,
        })
}

fn path_turn_angle(samples: &[CornerComparisonSample], index: usize) -> Option<f64> {
    const STRIDE: usize = 2;
    if index < STRIDE || index + STRIDE >= samples.len() {
        return None;
    }
    let before = reference_position(samples[index - STRIDE])?;
    let centre = reference_position(samples[index])?;
    let after = reference_position(samples[index + STRIDE])?;
    let incoming = (centre.0 - before.0, centre.1 - before.1);
    let outgoing = (after.0 - centre.0, after.1 - centre.1);
    let incoming_length = incoming.0.hypot(incoming.1);
    let outgoing_length = outgoing.0.hypot(outgoing.1);
    if incoming_length < 0.5 || outgoing_length < 0.5 {
        return None;
    }
    let cross = incoming.0 * outgoing.1 - incoming.1 * outgoing.0;
    let dot = incoming.0 * outgoing.0 + incoming.1 * outgoing.1;
    Some(cross.atan2(dot))
}

fn reference_position(sample: CornerComparisonSample) -> Option<(f64, f64)> {
    sample
        .reference_position_x_m
        .zip(sample.reference_position_z_m)
        .filter(|(x, z)| x.is_finite() && z.is_finite())
}

fn channel(value: &str) -> ChannelId {
    ChannelId::parse(value).expect("hard-coded canonical channel id must be valid")
}

fn confidence(value: f32) -> Confidence {
    Confidence::new(value).expect("hard-coded confidence must be within zero and one")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ComparisonContext {
        ComparisonContext {
            same_simulator: true,
            same_car: true,
            same_track_layout: true,
            setup_differs: None,
            conditions_differ: None,
        }
    }

    fn synthetic_corner() -> Vec<CornerComparisonSample> {
        (0..=40)
            .map(|index| {
                let distance_m = f64::from(index) * 5.0;
                let in_corner = (10..=30).contains(&index);
                let reference_speed = if in_corner {
                    150.0 - (10.0 - (f64::from(index) - 20.0).abs()) * 5.0
                } else {
                    150.0
                };
                let comparison_speed = reference_speed - if in_corner { 5.0 } else { 0.0 };
                let delta_s = if index < 10 {
                    0.0
                } else if index <= 30 {
                    f64::from(index - 10) * 0.01
                } else {
                    0.2
                };
                let angle = f64::from(index) / 40.0 * std::f64::consts::FRAC_PI_2;
                CornerComparisonSample {
                    distance_m,
                    delta_s: Some(delta_s),
                    reference_speed_kmh: Some(reference_speed),
                    comparison_speed_kmh: Some(comparison_speed),
                    reference_throttle_percent: Some(if index >= 22 { 100.0 } else { 0.0 }),
                    comparison_throttle_percent: Some(if index >= 25 { 100.0 } else { 0.0 }),
                    reference_brake_percent: Some(if (10..=18).contains(&index) {
                        70.0
                    } else {
                        0.0
                    }),
                    comparison_brake_percent: Some(if (12..=20).contains(&index) {
                        75.0
                    } else {
                        0.0
                    }),
                    reference_steering_percent: Some(if in_corner { 25.0 } else { 0.0 }),
                    comparison_steering_percent: Some(if in_corner { 27.0 } else { 0.0 }),
                    reference_position_x_m: Some(angle.cos() * 100.0),
                    reference_position_z_m: Some(angle.sin() * 100.0),
                    comparison_position_x_m: Some(angle.cos() * 101.0),
                    comparison_position_z_m: Some(angle.sin() * 101.0),
                }
            })
            .collect()
    }

    #[test]
    fn detects_a_corner_and_keeps_phase_losses_coherent() {
        let result = analyze_corner_comparison(&synthetic_corner(), context());
        assert_eq!(result.algorithm.version, 3);
        assert_eq!(result.availability, AnalysisAvailability::Available);
        let value = result.value.expect("available analysis");
        assert_eq!(value.corners.len(), 1);
        let corner = &value.corners[0];
        assert_eq!(corner.label, "T1");
        let phase_total = corner
            .phases
            .iter()
            .map(|phase| phase.loss_seconds.expect("phase delta"))
            .sum::<f64>();
        assert!((phase_total - corner.total_loss_seconds.expect("corner delta")).abs() < 1e-9);
        assert_eq!(corner.metrics.reference_braking_point_m, Some(50.0));
        assert_eq!(corner.metrics.comparison_braking_point_m, Some(60.0));
        assert_eq!(corner.metrics.reference_brake_release_point_m, Some(90.0));
        assert_eq!(corner.metrics.comparison_brake_release_point_m, Some(100.0));
        assert_eq!(corner.metrics.reference_peak_brake_percent, Some(70.0));
        assert_eq!(corner.metrics.comparison_peak_brake_percent, Some(75.0));
        assert_eq!(corner.metrics.reference_throttle_point_m, Some(110.0));
        assert_eq!(corner.metrics.comparison_throttle_point_m, Some(125.0));
    }

    #[test]
    fn comparison_braking_can_begin_before_the_reference_corner_range() {
        let mut samples = synthetic_corner();
        for (index, sample) in samples.iter_mut().enumerate() {
            sample.comparison_brake_percent =
                Some(if (6..=18).contains(&index) { 64.0 } else { 0.0 });
        }
        let result = analyze_corner_comparison(&samples, context());
        let corner = &result.value.expect("available analysis").corners[0];
        assert_eq!(corner.metrics.comparison_braking_point_m, Some(30.0));
        assert_eq!(corner.metrics.comparison_brake_release_point_m, Some(90.0));
        assert_eq!(corner.metrics.comparison_peak_brake_percent, Some(64.0));
    }

    #[test]
    fn brief_pressure_gap_does_not_split_a_braking_zone() {
        let mut samples = synthetic_corner();
        for (index, sample) in samples.iter_mut().enumerate() {
            sample.comparison_brake_percent = Some(if (6..=18).contains(&index) && index != 10 {
                if index == 14 { 82.0 } else { 64.0 }
            } else {
                0.0
            });
        }
        let result = analyze_corner_comparison(&samples, context());
        let corner = &result.value.expect("available analysis").corners[0];
        assert_eq!(corner.metrics.comparison_braking_point_m, Some(30.0));
        assert_eq!(corner.metrics.comparison_brake_release_point_m, Some(90.0));
        assert_eq!(corner.metrics.comparison_peak_brake_percent, Some(82.0));
    }

    #[test]
    fn nearest_pre_apex_zone_wins_over_an_earlier_brake_application() {
        let mut samples = synthetic_corner();
        for (index, sample) in samples.iter_mut().enumerate() {
            sample.comparison_brake_percent = Some(if (1..=3).contains(&index) {
                40.0
            } else if (12..=18).contains(&index) {
                70.0
            } else {
                0.0
            });
        }
        let result = analyze_corner_comparison(&samples, context());
        let corner = &result.value.expect("available analysis").corners[0];
        assert_eq!(corner.metrics.comparison_braking_point_m, Some(60.0));
        assert_eq!(corner.metrics.comparison_brake_release_point_m, Some(90.0));
    }

    #[test]
    fn continuous_steering_range_splits_two_distinct_corners() {
        let samples = (0..=90)
            .map(|index| {
                let reference_speed = (70.0 + (f64::from(index) - 20.0).abs() * 4.0)
                    .min(90.0 + (f64::from(index) - 60.0).abs() * 4.0)
                    .min(150.0);
                let comparison_speed = (80.0 + (f64::from(index) - 20.0).abs() * 4.0)
                    .min(60.0 + (f64::from(index) - 60.0).abs() * 4.0)
                    .min(150.0);
                CornerComparisonSample {
                    distance_m: f64::from(index) * 5.0,
                    delta_s: Some(f64::from(index) * 0.002),
                    reference_speed_kmh: Some(reference_speed),
                    comparison_speed_kmh: Some(comparison_speed),
                    reference_throttle_percent: Some(if index > 62 { 100.0 } else { 0.0 }),
                    comparison_throttle_percent: Some(if index > 62 { 100.0 } else { 0.0 }),
                    reference_brake_percent: Some(
                        if (10..=18).contains(&index) || (50..=58).contains(&index) {
                            70.0
                        } else {
                            0.0
                        },
                    ),
                    comparison_brake_percent: Some(
                        if (12..=20).contains(&index) || (52..=60).contains(&index) {
                            75.0
                        } else {
                            0.0
                        },
                    ),
                    reference_steering_percent: Some(if (10..=70).contains(&index) {
                        25.0
                    } else {
                        0.0
                    }),
                    comparison_steering_percent: Some(if (10..=70).contains(&index) {
                        25.0
                    } else {
                        0.0
                    }),
                    reference_position_x_m: None,
                    reference_position_z_m: None,
                    comparison_position_x_m: None,
                    comparison_position_z_m: None,
                }
            })
            .collect::<Vec<_>>();
        let result = analyze_corner_comparison(&samples, context());
        let corners = result.value.expect("available analysis").corners;
        assert_eq!(corners.len(), 2);
        assert!((corners[0].apex_distance_m - 100.0).abs() < f64::EPSILON);
        assert_eq!(corners[0].metrics.reference_braking_point_m, Some(50.0));
        assert_eq!(corners[0].metrics.comparison_braking_point_m, Some(60.0));
        assert!((corners[1].apex_distance_m - 300.0).abs() < f64::EPSILON);
        assert_eq!(corners[1].metrics.reference_braking_point_m, Some(250.0));
        assert_eq!(corners[1].metrics.comparison_braking_point_m, Some(260.0));
    }

    #[test]
    fn missing_driver_brake_input_does_not_invent_a_zone() {
        let mut samples = synthetic_corner();
        for sample in &mut samples {
            sample.comparison_brake_percent = None;
        }
        let result = analyze_corner_comparison(&samples, context());
        let corner = &result.value.expect("available analysis").corners[0];
        assert_eq!(corner.metrics.comparison_braking_point_m, None);
        assert_eq!(corner.metrics.comparison_brake_release_point_m, None);
        assert_eq!(corner.metrics.comparison_peak_brake_percent, None);
    }

    #[test]
    fn preceding_corner_braking_is_not_attached_to_the_next_corner() {
        let samples = (0..=80)
            .map(|index| {
                let first_corner = (10..=25).contains(&index);
                let second_corner = (45..=60).contains(&index);
                let nearest_apex = if first_corner { 18 } else { 52 };
                let speed = if first_corner || second_corner {
                    150.0
                        - (10.0 - (f64::from(index) - f64::from(nearest_apex)).abs()).max(0.0) * 5.0
                } else {
                    150.0
                };
                CornerComparisonSample {
                    distance_m: f64::from(index) * 5.0,
                    delta_s: Some(f64::from(index) * 0.001),
                    reference_speed_kmh: Some(speed),
                    comparison_speed_kmh: Some(speed - 2.0),
                    reference_throttle_percent: Some(if index > nearest_apex {
                        100.0
                    } else {
                        0.0
                    }),
                    comparison_throttle_percent: Some(if index > nearest_apex {
                        100.0
                    } else {
                        0.0
                    }),
                    reference_brake_percent: Some(
                        if (8..=14).contains(&index) || (43..=49).contains(&index) {
                            70.0
                        } else {
                            0.0
                        },
                    ),
                    comparison_brake_percent: Some(if (12..=16).contains(&index) {
                        60.0
                    } else {
                        0.0
                    }),
                    reference_steering_percent: Some(if first_corner || second_corner {
                        25.0
                    } else {
                        0.0
                    }),
                    comparison_steering_percent: Some(if first_corner || second_corner {
                        25.0
                    } else {
                        0.0
                    }),
                    reference_position_x_m: None,
                    reference_position_z_m: None,
                    comparison_position_x_m: None,
                    comparison_position_z_m: None,
                }
            })
            .collect::<Vec<_>>();
        let result = analyze_corner_comparison(&samples, context());
        let corners = result.value.expect("available analysis").corners;
        assert_eq!(corners.len(), 2);
        assert_eq!(corners[0].metrics.comparison_braking_point_m, Some(60.0));
        assert_eq!(corners[1].metrics.comparison_braking_point_m, None);
    }

    #[test]
    fn missing_corner_signals_are_reported_instead_of_guessed() {
        let mut samples = synthetic_corner();
        for sample in &mut samples {
            sample.reference_brake_percent = None;
            sample.reference_steering_percent = None;
            sample.reference_position_x_m = None;
            sample.reference_position_z_m = None;
        }
        let result = analyze_corner_comparison(&samples, context());
        assert!(matches!(
            result.availability,
            AnalysisAvailability::UnsupportedChannels(_)
        ));
        assert!(result.value.is_none());
    }

    #[test]
    fn invalid_distance_order_is_rejected() {
        let mut samples = synthetic_corner();
        samples[8].distance_m = samples[7].distance_m;
        let result = analyze_corner_comparison(&samples, context());
        assert_eq!(result.availability, AnalysisAvailability::InvalidRange);
    }
}
