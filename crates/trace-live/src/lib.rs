//! Simulator- and transport-independent live telemetry encoding.

use trace_protocol::{
    ChannelColumn, Envelope, Hello, LiveStatus, PROTOCOL_VERSION, Payload, ProtocolError,
    ProtocolLimits, SessionEnd, SessionState, TelemetryBatch, WireUnit,
};

/// Default spectator publishing interval: 20 Hz.
pub const LIVE_SAMPLE_INTERVAL_NS: u64 = 50_000_000;

/// Canonical values needed by the protocol-v1 live telemetry projection.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiveTelemetrySample {
    pub elapsed_ns: u64,
    pub throttle: Option<f32>,
    pub brake: Option<f32>,
    pub clutch: Option<f32>,
    pub steering_angle_rad: Option<f32>,
    pub speed_mps: Option<f32>,
    pub engine_rpm: Option<f32>,
    pub gear: Option<i16>,
    pub fuel_litres: Option<f32>,
    pub lap_position: Option<f32>,
    pub lap_time_s: Option<f32>,
    pub sector_index: Option<u32>,
    pub position_x_m: Option<f64>,
    pub position_z_m: Option<f64>,
    pub ambient_temperature_c: Option<f32>,
    pub track_temperature_c: Option<f32>,
}

/// One encoded message and its delay from the start of replay broadcasting.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledEnvelope {
    pub due_ns: u64,
    pub envelope: Envelope,
}

/// Stateful encoder for an active capture stream.
pub struct LiveStreamEncoder {
    session_id: String,
    sequence: u64,
    last_sample_elapsed_ns: Option<u64>,
}

impl LiveStreamEncoder {
    /// Starts a stream and returns its introduction messages.
    ///
    /// # Errors
    ///
    /// Returns a protocol error when the session metadata or identifier is invalid.
    pub fn start(
        session_id: impl Into<String>,
        mut state: SessionState,
        sent_at_unix_ms: i64,
    ) -> Result<(Self, Vec<Envelope>), ProtocolError> {
        let session_id = session_id.into();
        state.status = LiveStatus::Live;
        let mut encoder = Self {
            session_id,
            sequence: 0,
            last_sample_elapsed_ns: None,
        };
        let messages = vec![
            encoder.envelope(
                sent_at_unix_ms,
                Payload::Hello(Hello {
                    publisher_version: env!("CARGO_PKG_VERSION").to_owned(),
                    source: "active-capture".to_owned(),
                }),
            )?,
            encoder.envelope(sent_at_unix_ms, Payload::SessionState(state))?,
        ];
        Ok((encoder, messages))
    }

    /// Encodes a sample when the 20 Hz live interval has elapsed.
    ///
    /// # Errors
    ///
    /// Returns a protocol error when the projected telemetry is invalid.
    pub fn sample(
        &mut self,
        sample: &LiveTelemetrySample,
        sent_at_unix_ms: i64,
    ) -> Result<Option<Envelope>, ProtocolError> {
        if self
            .last_sample_elapsed_ns
            .is_some_and(|last| sample.elapsed_ns < last.saturating_add(LIVE_SAMPLE_INTERVAL_NS))
        {
            return Ok(None);
        }
        self.last_sample_elapsed_ns = Some(sample.elapsed_ns);
        self.envelope(
            sent_at_unix_ms,
            Payload::TelemetryBatch(telemetry_batch(sample, sample.elapsed_ns)),
        )
        .map(Some)
    }

    /// Ends the stream with a terminal message.
    ///
    /// # Errors
    ///
    /// Returns a protocol error when the terminal payload is invalid.
    pub fn end(
        &mut self,
        reason: impl Into<String>,
        sent_at_unix_ms: i64,
    ) -> Result<Envelope, ProtocolError> {
        self.envelope(
            sent_at_unix_ms,
            Payload::End(SessionEnd {
                reason: reason.into(),
            }),
        )
    }

    fn envelope(
        &mut self,
        sent_at_unix_ms: i64,
        payload: Payload,
    ) -> Result<Envelope, ProtocolError> {
        let envelope = Envelope {
            protocol_version: PROTOCOL_VERSION,
            message_id: format!("live_{}", self.sequence),
            session_id: self.session_id.clone(),
            sequence: self.sequence,
            sent_at_unix_ms,
            payload,
        };
        envelope.validate(ProtocolLimits::default())?;
        self.sequence = self.sequence.saturating_add(1);
        Ok(envelope)
    }
}

/// Failure to build a valid replay stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayEncodeError {
    Empty,
    NonIncreasingTime,
    InvalidProtocol(ProtocolError),
}

/// Encodes recorded canonical samples as a rebased, approximately 20 Hz live stream.
///
/// The returned stream begins with `hello` and live `session_state` messages, includes
/// the final telemetry sample even when it falls between rate boundaries, and ends
/// with an explicit terminal message. `broadcast_unix_ms` is the wall-clock timestamp
/// assigned to the first message; later timestamps follow replay time.
///
/// # Errors
///
/// Returns [`ReplayEncodeError`] for empty/non-monotonic input or invalid protocol data.
pub fn encode_recorded_session(
    session_id: &str,
    mut state: SessionState,
    samples: &[LiveTelemetrySample],
    broadcast_unix_ms: i64,
) -> Result<Vec<ScheduledEnvelope>, ReplayEncodeError> {
    let first = samples.first().ok_or(ReplayEncodeError::Empty)?;
    if samples
        .windows(2)
        .any(|pair| pair[0].elapsed_ns >= pair[1].elapsed_ns)
    {
        return Err(ReplayEncodeError::NonIncreasingTime);
    }

    state.status = LiveStatus::Live;
    let mut sequence = 0_u64;
    let mut scheduled = Vec::with_capacity(samples.len().min(2_402));
    push_message(
        &mut scheduled,
        session_id,
        &mut sequence,
        0,
        broadcast_unix_ms,
        Payload::Hello(Hello {
            publisher_version: env!("CARGO_PKG_VERSION").to_owned(),
            source: "recorded-session".to_owned(),
        }),
    )?;
    push_message(
        &mut scheduled,
        session_id,
        &mut sequence,
        0,
        broadcast_unix_ms,
        Payload::SessionState(state),
    )?;

    let mut last_selected = None;
    let mut next_due_ns = 0_u64;
    for (index, sample) in samples.iter().enumerate() {
        let due_ns = sample.elapsed_ns - first.elapsed_ns;
        let final_sample = index + 1 == samples.len();
        if due_ns < next_due_ns && !final_sample {
            continue;
        }
        if last_selected == Some(index) {
            continue;
        }
        push_message(
            &mut scheduled,
            session_id,
            &mut sequence,
            due_ns,
            rebased_timestamp(broadcast_unix_ms, due_ns),
            Payload::TelemetryBatch(telemetry_batch(sample, due_ns)),
        )?;
        last_selected = Some(index);
        next_due_ns = due_ns.saturating_add(LIVE_SAMPLE_INTERVAL_NS);
    }

    let final_due_ns = samples
        .last()
        .map_or(0, |sample| sample.elapsed_ns - first.elapsed_ns);
    push_message(
        &mut scheduled,
        session_id,
        &mut sequence,
        final_due_ns,
        rebased_timestamp(broadcast_unix_ms, final_due_ns),
        Payload::End(SessionEnd {
            reason: "recorded session playback completed".to_owned(),
        }),
    )?;
    Ok(scheduled)
}

fn push_message(
    messages: &mut Vec<ScheduledEnvelope>,
    session_id: &str,
    sequence: &mut u64,
    due_ns: u64,
    sent_at_unix_ms: i64,
    payload: Payload,
) -> Result<(), ReplayEncodeError> {
    let envelope = Envelope {
        protocol_version: PROTOCOL_VERSION,
        message_id: format!("replay_{sequence}"),
        session_id: session_id.to_owned(),
        sequence: *sequence,
        sent_at_unix_ms,
        payload,
    };
    envelope
        .validate(ProtocolLimits::default())
        .map_err(ReplayEncodeError::InvalidProtocol)?;
    messages.push(ScheduledEnvelope { due_ns, envelope });
    *sequence = sequence.saturating_add(1);
    Ok(())
}

fn telemetry_batch(sample: &LiveTelemetrySample, elapsed_ns: u64) -> TelemetryBatch {
    let channels = vec![
        channel("driver.throttle", WireUnit::Ratio, sample.throttle),
        channel("driver.brake", WireUnit::Ratio, sample.brake),
        channel("driver.clutch", WireUnit::Ratio, sample.clutch),
        channel(
            "driver.steering_angle",
            WireUnit::Radian,
            sample.steering_angle_rad,
        ),
        channel("vehicle.speed", WireUnit::MetresPerSecond, sample.speed_mps),
        channel(
            "vehicle.engine_rpm",
            WireUnit::RevolutionsPerMinute,
            sample.engine_rpm,
        ),
        channel(
            "vehicle.gear",
            WireUnit::Unitless,
            sample.gear.map(f32::from),
        ),
        channel("vehicle.fuel", WireUnit::Litre, sample.fuel_litres),
        channel(
            "lap.normalized_position",
            WireUnit::Ratio,
            sample.lap_position,
        ),
        channel("lap.elapsed", WireUnit::Second, sample.lap_time_s),
        channel(
            "lap.sector_index",
            WireUnit::Unitless,
            sample
                .sector_index
                .and_then(|value| u16::try_from(value).ok())
                .map(f32::from),
        ),
        channel(
            "motion.position.x",
            WireUnit::Metre,
            sample.position_x_m.and_then(narrow_f64),
        ),
        channel(
            "motion.position.z",
            WireUnit::Metre,
            sample.position_z_m.and_then(narrow_f64),
        ),
        channel(
            "environment.ambient_temperature",
            WireUnit::DegreeCelsius,
            sample.ambient_temperature_c,
        ),
        channel(
            "environment.track_temperature",
            WireUnit::DegreeCelsius,
            sample.track_temperature_c,
        ),
    ];
    TelemetryBatch {
        base_elapsed_ns: elapsed_ns,
        offsets_ns: vec![0],
        channels,
    }
}

fn channel(id: &str, unit: WireUnit, value: Option<f32>) -> ChannelColumn {
    ChannelColumn {
        id: id.to_owned(),
        unit,
        values: vec![value.filter(|number| number.is_finite())],
    }
}

#[allow(clippy::cast_possible_truncation)]
fn narrow_f64(value: f64) -> Option<f32> {
    if value.is_finite() && (f64::from(f32::MIN)..=f64::from(f32::MAX)).contains(&value) {
        Some(value as f32)
    } else {
        None
    }
}

fn rebased_timestamp(start_unix_ms: i64, due_ns: u64) -> i64 {
    let elapsed_ms = i64::try_from(due_ns / 1_000_000).unwrap_or(i64::MAX);
    start_unix_ms.saturating_add(elapsed_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SessionState {
        SessionState {
            driver_name: Some("Ismail".to_owned()),
            simulator: "assetto-corsa".to_owned(),
            car: Some("ks_mazda_mx5_cup".to_owned()),
            track: Some("zandvoort".to_owned()),
            layout: None,
            session_type: Some("hotlap".to_owned()),
            status: LiveStatus::Paused,
        }
    }

    fn sample(elapsed_ns: u64, throttle: f32) -> LiveTelemetrySample {
        LiveTelemetrySample {
            elapsed_ns,
            throttle: Some(throttle),
            speed_mps: Some(30.0),
            ..LiveTelemetrySample::default()
        }
    }

    #[test]
    fn downsamples_to_twenty_hertz_and_keeps_the_final_sample() {
        let samples = [
            sample(1_000_000_000, 0.0),
            sample(1_010_000_000, 0.2),
            sample(1_050_000_000, 0.5),
            sample(1_099_000_000, 0.8),
        ];
        let messages = encode_recorded_session(
            "0123456789abcdef0123456789abcdef",
            state(),
            &samples,
            1_700_000_000_000,
        )
        .expect("replay");

        assert_eq!(messages.len(), 6);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.due_ns)
                .collect::<Vec<_>>(),
            vec![0, 0, 0, 50_000_000, 99_000_000, 99_000_000]
        );
        assert!(matches!(messages[0].envelope.payload, Payload::Hello(_)));
        assert!(matches!(
            messages[1].envelope.payload,
            Payload::SessionState(_)
        ));
        assert!(matches!(
            messages.last().map(|value| &value.envelope.payload),
            Some(Payload::End(_))
        ));
    }

    #[test]
    fn rebases_wall_time_and_preserves_missing_values() {
        let samples = [sample(4_000_000_000, 0.5), sample(4_050_000_000, f32::NAN)];
        let messages =
            encode_recorded_session("0123456789abcdef0123456789abcdef", state(), &samples, 1_000)
                .expect("replay");
        assert_eq!(messages[3].envelope.sent_at_unix_ms, 1_050);
        let Payload::TelemetryBatch(batch) = &messages[3].envelope.payload else {
            panic!("expected telemetry");
        };
        assert_eq!(batch.channels[0].values, vec![None]);
    }

    #[test]
    fn rejects_empty_and_non_monotonic_recordings() {
        assert_eq!(
            encode_recorded_session("0123456789abcdef0123456789abcdef", state(), &[], 0,),
            Err(ReplayEncodeError::Empty)
        );
        assert_eq!(
            encode_recorded_session(
                "0123456789abcdef0123456789abcdef",
                state(),
                &[sample(10, 0.0), sample(10, 1.0)],
                0,
            ),
            Err(ReplayEncodeError::NonIncreasingTime)
        );
    }

    #[test]
    fn active_streams_are_ordered_and_rate_limited() {
        let (mut encoder, introduction) =
            LiveStreamEncoder::start("0123456789abcdef0123456789abcdef", state(), 1_000)
                .expect("active stream");
        assert_eq!(introduction.len(), 2);
        assert_eq!(introduction[0].sequence, 0);
        assert_eq!(introduction[1].sequence, 1);

        let first = sample(0, 0.4);
        assert_eq!(
            encoder
                .sample(&first, 1_000)
                .expect("first sample")
                .expect("published")
                .sequence,
            2
        );
        assert!(
            encoder
                .sample(&sample(10_000_000, 0.5), 1_010)
                .expect("limited sample")
                .is_none()
        );
        assert_eq!(
            encoder
                .sample(&sample(LIVE_SAMPLE_INTERVAL_NS, 0.6), 1_050)
                .expect("next sample")
                .expect("published")
                .sequence,
            3
        );
        assert_eq!(
            encoder.end("capture ended", 1_100).expect("end").sequence,
            4
        );
    }
}
