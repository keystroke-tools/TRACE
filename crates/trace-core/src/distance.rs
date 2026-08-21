//! Validation and interpolation in the lap-distance domain.

use serde::{Deserialize, Serialize};

/// A value observed at a lap distance in metres.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DistanceSample {
    pub distance_m: f64,
    pub value: f64,
}

/// Samples proven to have finite values and strictly increasing distance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DistanceSeries {
    samples: Vec<DistanceSample>,
}

impl DistanceSeries {
    /// Validates samples for use by distance-domain algorithms.
    ///
    /// # Errors
    ///
    /// Returns [`SeriesError`] for empty, non-finite, negative-distance, or
    /// non-monotonic input.
    pub fn new(samples: Vec<DistanceSample>) -> Result<Self, SeriesError> {
        if samples.is_empty() {
            return Err(SeriesError::Empty);
        }

        for (index, sample) in samples.iter().enumerate() {
            if !sample.distance_m.is_finite() || !sample.value.is_finite() {
                return Err(SeriesError::NonFinite { index });
            }
            if sample.distance_m < 0.0 {
                return Err(SeriesError::NegativeDistance { index });
            }
            if index > 0 && sample.distance_m <= samples[index - 1].distance_m {
                return Err(SeriesError::NonIncreasingDistance { index });
            }
        }

        Ok(Self { samples })
    }

    /// Returns the validated samples.
    pub fn samples(&self) -> &[DistanceSample] {
        &self.samples
    }

    /// Interpolates at each requested distance using the selected method.
    ///
    /// Values outside the source range, or across a source interval larger than
    /// `max_gap_m`, are returned as `None` rather than fabricated.
    ///
    /// # Errors
    ///
    /// Returns [`InterpolationError`] for invalid query distances or gap limits.
    pub fn interpolate(
        &self,
        distances_m: &[f64],
        method: InterpolationMethod,
        max_gap_m: f64,
    ) -> Result<Vec<Option<f64>>, InterpolationError> {
        if !max_gap_m.is_finite() || max_gap_m <= 0.0 {
            return Err(InterpolationError::InvalidMaximumGap);
        }
        if distances_m.iter().any(|distance| !distance.is_finite()) {
            return Err(InterpolationError::NonFiniteQuery);
        }

        Ok(distances_m
            .iter()
            .map(|distance| self.interpolate_one(*distance, method, max_gap_m))
            .collect())
    }

    fn interpolate_one(
        &self,
        distance_m: f64,
        method: InterpolationMethod,
        max_gap_m: f64,
    ) -> Option<f64> {
        let insertion = self
            .samples
            .partition_point(|sample| sample.distance_m < distance_m);

        if let Some(exact) = self.samples.get(insertion)
            && exact.distance_m.total_cmp(&distance_m).is_eq()
        {
            return Some(exact.value);
        }
        if insertion == 0 || insertion == self.samples.len() {
            return None;
        }

        let before = self.samples[insertion - 1];
        let after = self.samples[insertion];
        let interval_m = after.distance_m - before.distance_m;
        if interval_m > max_gap_m {
            return None;
        }

        match method {
            InterpolationMethod::Linear => {
                let ratio = (distance_m - before.distance_m) / interval_m;
                Some(before.value + ratio * (after.value - before.value))
            }
            InterpolationMethod::HoldPrevious => Some(before.value),
        }
    }
}

/// Interpolation appropriate to the semantics of a channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum InterpolationMethod {
    /// Piecewise linear interpolation for continuous values.
    Linear,
    /// Zero-order hold for discrete state such as gear.
    HoldPrevious,
}

/// Invalid distance series.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SeriesError {
    Empty,
    NonFinite { index: usize },
    NegativeDistance { index: usize },
    NonIncreasingDistance { index: usize },
}

/// Invalid interpolation request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InterpolationError {
    InvalidMaximumGap,
    NonFiniteQuery,
}

/// Creates an inclusive, uniform distance grid with the exact lap end appended.
///
/// # Errors
///
/// Returns [`GridError`] when lap length or spacing is non-finite or non-positive.
pub fn uniform_grid(lap_length_m: f64, spacing_m: f64) -> Result<Vec<f64>, GridError> {
    const MAX_GRID_POINTS: f64 = 10_000_000.0;

    if !lap_length_m.is_finite() || lap_length_m <= 0.0 {
        return Err(GridError::InvalidLapLength);
    }
    if !spacing_m.is_finite() || spacing_m <= 0.0 {
        return Err(GridError::InvalidSpacing);
    }
    if (lap_length_m / spacing_m).ceil() + 1.0 > MAX_GRID_POINTS {
        return Err(GridError::TooManyPoints);
    }

    let mut distances = vec![0.0];
    let mut distance_m = spacing_m;
    while distance_m < lap_length_m {
        distances.push(distance_m);
        let next = distance_m + spacing_m;
        if next <= distance_m {
            return Err(GridError::SpacingBelowPrecision);
        }
        distance_m = next;
    }
    if distances
        .last()
        .is_none_or(|last| last.total_cmp(&lap_length_m).is_ne())
    {
        distances.push(lap_length_m);
    }

    Ok(distances)
}

/// Invalid uniform distance grid configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridError {
    InvalidLapLength,
    InvalidSpacing,
    TooManyPoints,
    SpacingBelowPrecision,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(distance_m: f64, value: f64) -> DistanceSample {
        DistanceSample { distance_m, value }
    }

    #[test]
    fn series_rejects_duplicate_and_non_finite_samples() {
        assert_eq!(DistanceSeries::new(vec![]), Err(SeriesError::Empty));
        assert_eq!(
            DistanceSeries::new(vec![sample(0.0, 1.0), sample(0.0, 2.0)]),
            Err(SeriesError::NonIncreasingDistance { index: 1 })
        );
        assert_eq!(
            DistanceSeries::new(vec![sample(0.0, f64::NAN)]),
            Err(SeriesError::NonFinite { index: 0 })
        );
    }

    #[test]
    fn linear_interpolation_matches_known_line() {
        let series =
            DistanceSeries::new(vec![sample(0.0, 10.0), sample(10.0, 30.0)]).expect("valid series");
        let values = series
            .interpolate(&[0.0, 2.5, 10.0], InterpolationMethod::Linear, 10.0)
            .expect("valid interpolation");

        assert_eq!(values, vec![Some(10.0), Some(15.0), Some(30.0)]);
    }

    #[test]
    fn discrete_interpolation_holds_previous_value() {
        let series =
            DistanceSeries::new(vec![sample(0.0, 2.0), sample(10.0, 3.0)]).expect("valid series");
        let values = series
            .interpolate(&[4.0, 10.0], InterpolationMethod::HoldPrevious, 10.0)
            .expect("valid interpolation");

        assert_eq!(values, vec![Some(2.0), Some(3.0)]);
    }

    #[test]
    fn interpolation_does_not_bridge_gaps_or_extrapolate() {
        let series =
            DistanceSeries::new(vec![sample(5.0, 10.0), sample(15.0, 30.0)]).expect("valid series");
        let values = series
            .interpolate(&[0.0, 10.0, 20.0], InterpolationMethod::Linear, 5.0)
            .expect("valid interpolation");

        assert_eq!(values, vec![None, None, None]);
    }

    #[test]
    fn grid_includes_exact_endpoint_without_overshooting() {
        assert_eq!(
            uniform_grid(10.0, 4.0).expect("valid grid"),
            vec![0.0, 4.0, 8.0, 10.0]
        );
        assert_eq!(
            uniform_grid(10.0, 5.0).expect("valid grid"),
            vec![0.0, 5.0, 10.0]
        );
    }
}
