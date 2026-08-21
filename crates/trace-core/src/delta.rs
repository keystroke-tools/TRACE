//! Distance-aligned lap delta calculation.

use serde::{Deserialize, Serialize};

use crate::distance::{
    DistanceSample, DistanceSeries, InterpolationError, InterpolationMethod, SeriesError,
};

/// Elapsed lap time as a validated function of distance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ElapsedTimeSeries {
    series: DistanceSeries,
}

impl ElapsedTimeSeries {
    /// Validates an elapsed-time series.
    ///
    /// Distances must be strictly increasing and elapsed seconds must be finite,
    /// non-negative, and non-decreasing.
    ///
    /// # Errors
    ///
    /// Returns [`ElapsedTimeError`] when either axis violates those invariants.
    pub fn new(samples: Vec<DistanceSample>) -> Result<Self, ElapsedTimeError> {
        let series = DistanceSeries::new(samples).map_err(ElapsedTimeError::Distance)?;
        for (index, sample) in series.samples().iter().enumerate() {
            if sample.value < 0.0 {
                return Err(ElapsedTimeError::NegativeElapsedTime { index });
            }
            if index > 0 && sample.value < series.samples()[index - 1].value {
                return Err(ElapsedTimeError::DecreasingElapsedTime { index });
            }
        }
        Ok(Self { series })
    }

    /// Returns the validated distance/time observations.
    pub fn samples(&self) -> &[DistanceSample] {
        self.series.samples()
    }
}

/// Invalid elapsed-time curve.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElapsedTimeError {
    Distance(SeriesError),
    NegativeElapsedTime { index: usize },
    DecreasingElapsedTime { index: usize },
}

/// Delta at one requested lap distance. Positive values mean the comparison lap
/// is behind the reference lap.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeltaSample {
    pub distance_m: f64,
    pub delta_s: Option<f64>,
}

/// A distance-aligned cumulative lap delta trace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeltaTrace {
    pub samples: Vec<DeltaSample>,
    pub baseline_s: f64,
    pub valid_samples: usize,
}

/// Computes `comparison_time(distance) - reference_time(distance)`.
///
/// The first jointly valid delta is removed as a baseline so independently
/// captured clock origins do not create a false whole-lap offset. Gaps remain
/// unavailable and authoritative values are not smoothed.
///
/// # Errors
///
/// Returns [`DeltaError`] when the requested grid is empty, non-monotonic, or
/// cannot be interpolated with the supplied maximum gap.
pub fn calculate_delta(
    reference: &ElapsedTimeSeries,
    comparison: &ElapsedTimeSeries,
    distances_m: &[f64],
    max_gap_m: f64,
) -> Result<DeltaTrace, DeltaError> {
    validate_grid(distances_m)?;
    let reference_times = reference
        .series
        .interpolate(distances_m, InterpolationMethod::Linear, max_gap_m)
        .map_err(DeltaError::Interpolation)?;
    let comparison_times = comparison
        .series
        .interpolate(distances_m, InterpolationMethod::Linear, max_gap_m)
        .map_err(DeltaError::Interpolation)?;

    let baseline_s = reference_times
        .iter()
        .zip(&comparison_times)
        .find_map(|(reference, comparison)| {
            (*reference)
                .zip(*comparison)
                .map(|(reference, comparison)| comparison - reference)
        })
        .ok_or(DeltaError::NoCommonSamples)?;

    let samples: Vec<_> = distances_m
        .iter()
        .zip(reference_times.iter().zip(comparison_times))
        .map(|(distance_m, (reference, comparison))| DeltaSample {
            distance_m: *distance_m,
            delta_s: reference
                .zip(comparison)
                .map(|(reference, comparison)| comparison - reference - baseline_s),
        })
        .collect();
    let valid_samples = samples
        .iter()
        .filter(|sample| sample.delta_s.is_some())
        .count();

    Ok(DeltaTrace {
        samples,
        baseline_s,
        valid_samples,
    })
}

fn validate_grid(distances_m: &[f64]) -> Result<(), DeltaError> {
    if distances_m.is_empty() {
        return Err(DeltaError::EmptyGrid);
    }
    for (index, distance) in distances_m.iter().enumerate() {
        if !distance.is_finite() || *distance < 0.0 {
            return Err(DeltaError::InvalidGridDistance { index });
        }
        if index > 0 && *distance <= distances_m[index - 1] {
            return Err(DeltaError::NonIncreasingGrid { index });
        }
    }
    Ok(())
}

/// Failure to calculate a trustworthy delta trace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeltaError {
    EmptyGrid,
    InvalidGridDistance { index: usize },
    NonIncreasingGrid { index: usize },
    Interpolation(InterpolationError),
    NoCommonSamples,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time_series(samples: &[(f64, f64)]) -> ElapsedTimeSeries {
        ElapsedTimeSeries::new(
            samples
                .iter()
                .map(|(distance_m, elapsed_s)| DistanceSample {
                    distance_m: *distance_m,
                    value: *elapsed_s,
                })
                .collect(),
        )
        .expect("valid elapsed time series")
    }

    #[test]
    fn elapsed_time_must_be_monotonic() {
        assert_eq!(
            ElapsedTimeSeries::new(vec![
                DistanceSample {
                    distance_m: 0.0,
                    value: 1.0,
                },
                DistanceSample {
                    distance_m: 1.0,
                    value: 0.9,
                },
            ]),
            Err(ElapsedTimeError::DecreasingElapsedTime { index: 1 })
        );
    }

    #[test]
    fn slower_comparison_produces_positive_analytic_delta() {
        // Reference travels at 10 m/s; comparison travels at 8 m/s.
        let reference = time_series(&[(0.0, 0.0), (100.0, 10.0)]);
        let comparison = time_series(&[(0.0, 0.0), (40.0, 5.0), (100.0, 12.5)]);
        let trace = calculate_delta(&reference, &comparison, &[0.0, 20.0, 40.0, 100.0], 100.0)
            .expect("valid delta");

        assert_eq!(trace.valid_samples, 4);
        assert!(trace.baseline_s.abs() < f64::EPSILON);
        assert!((trace.samples[3].delta_s.expect("valid endpoint") - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn baseline_removes_independent_capture_clock_offset() {
        let reference = time_series(&[(0.0, 0.0), (100.0, 10.0)]);
        let comparison = time_series(&[(0.0, 3.0), (100.0, 13.0)]);
        let trace =
            calculate_delta(&reference, &comparison, &[0.0, 100.0], 100.0).expect("valid delta");

        assert!((trace.baseline_s - 3.0).abs() < f64::EPSILON);
        assert!(trace.samples.iter().all(|sample| {
            sample
                .delta_s
                .is_some_and(|delta| delta.abs() < f64::EPSILON)
        }));
    }

    #[test]
    fn gaps_propagate_as_unavailable_delta_samples() {
        let reference = time_series(&[(0.0, 0.0), (10.0, 1.0), (100.0, 10.0)]);
        let comparison = time_series(&[(0.0, 0.0), (10.0, 1.0), (100.0, 10.0)]);
        let trace = calculate_delta(&reference, &comparison, &[0.0, 50.0, 100.0], 20.0)
            .expect("valid delta with a gap");

        assert_eq!(trace.valid_samples, 2);
        assert_eq!(trace.samples[1].delta_s, None);
    }
}
