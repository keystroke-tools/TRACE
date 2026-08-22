//! Durable completion of an in-memory recording.
//!
//! Blob bytes are committed before their `SQLite` reference. A failure in the second
//! step returns the committed blob metadata for explicit orphan reconciliation.

use trace_storage::{
    BlobCommit, BlobFormat, BlobMetadata, FileBlobStore, FileBlobWriter, RelativeBlobPath,
    StorageError, TelemetryBlobStore,
    ipc::{IpcError, TELEMETRY_SCHEMA_VERSION, TelemetryIpcWriter, encode_frames},
    metadata::{MetadataError, MetadataStore, NewLap},
};

use crate::RecordedSession;

/// Host-assigned identities and timestamps needed to persist a completed recording.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletionDescriptor {
    pub session_id: String,
    pub ended_at: String,
    pub blob_path: RelativeBlobPath,
    pub lap_id_prefix: String,
}

/// A committed recording and its durable metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedRecording {
    pub blob: BlobMetadata,
    pub lap_count: usize,
}

/// Failure while encoding or committing a completed recording.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PersistenceError {
    EmptyRecording,
    InvalidDescriptor,
    Encode(IpcError),
    Blob(StorageError),
    /// The blob is durable but `SQLite` did not accept its reference.
    OrphanedBlob {
        blob: Box<BlobMetadata>,
        metadata_error: MetadataError,
    },
}

/// Encodes, commits, and indexes a completed recording in recovery-safe order.
///
/// The session row must already have been created when capture began. Append errors
/// abort the pending blob. A failed blob commit remains pending according to the
/// store contract. A failed metadata transaction returns the committed blob details
/// so callers can queue reconciliation.
///
/// # Errors
///
/// Returns [`PersistenceError`] for empty recordings, invalid host identities,
/// Arrow encoding errors, blob-store failures, or metadata transaction failures.
pub fn persist_recording<S: TelemetryBlobStore>(
    blobs: &mut S,
    metadata: &mut MetadataStore,
    recording: &RecordedSession,
    descriptor: &CompletionDescriptor,
) -> Result<PersistedRecording, PersistenceError> {
    if recording.frames.is_empty() {
        return Err(PersistenceError::EmptyRecording);
    }
    if descriptor.session_id.is_empty()
        || descriptor.ended_at.is_empty()
        || descriptor.lap_id_prefix.is_empty()
    {
        return Err(PersistenceError::InvalidDescriptor);
    }

    let bytes = encode_frames(&recording.frames).map_err(PersistenceError::Encode)?;
    let sample_count =
        u64::try_from(recording.frames.len()).map_err(|_| PersistenceError::InvalidDescriptor)?;
    let pending = blobs.begin().map_err(PersistenceError::Blob)?;
    if let Err(error) = blobs.append(&pending, &bytes) {
        let _ = blobs.abort(&pending);
        return Err(PersistenceError::Blob(error));
    }
    let blob = blobs
        .commit(
            &pending,
            BlobCommit {
                path: descriptor.blob_path.clone(),
                format: BlobFormat::ArrowIpc,
                schema_version: TELEMETRY_SCHEMA_VERSION,
                sample_count,
                expected_sha256: None,
            },
        )
        .map_err(PersistenceError::Blob)?;

    index_recording(metadata, recording, descriptor, blob)
}

/// Finalizes an incremental Arrow encoder and indexes its already-staged recording.
///
/// # Errors
///
/// Returns a persistence error for Arrow finalization, sample-count disagreement,
/// blob publication, or metadata completion failure.
pub fn persist_streamed_recording(
    blobs: &mut FileBlobStore,
    metadata: &mut MetadataStore,
    recording: &RecordedSession,
    descriptor: &CompletionDescriptor,
    writer: TelemetryIpcWriter<FileBlobWriter>,
) -> Result<PersistedRecording, PersistenceError> {
    if descriptor.session_id.is_empty()
        || descriptor.ended_at.is_empty()
        || descriptor.lap_id_prefix.is_empty()
    {
        return Err(PersistenceError::InvalidDescriptor);
    }
    let (writer, sample_count) = writer.finish().map_err(PersistenceError::Encode)?;
    if sample_count != recording.sample_count {
        return Err(PersistenceError::InvalidDescriptor);
    }
    let blob = blobs
        .commit(
            &writer.into_pending(),
            BlobCommit {
                path: descriptor.blob_path.clone(),
                format: BlobFormat::ArrowIpc,
                schema_version: TELEMETRY_SCHEMA_VERSION,
                sample_count,
                expected_sha256: None,
            },
        )
        .map_err(PersistenceError::Blob)?;
    index_recording(metadata, recording, descriptor, blob)
}

fn index_recording(
    metadata: &mut MetadataStore,
    recording: &RecordedSession,
    descriptor: &CompletionDescriptor,
    blob: BlobMetadata,
) -> Result<PersistedRecording, PersistenceError> {
    let fastest_lap = recording
        .laps
        .iter()
        .filter(|lap| !lap.partial)
        .filter_map(|lap| lap.duration_ns)
        .min();
    let laps = recording
        .laps
        .iter()
        .map(|lap| NewLap {
            id: format!("{}-{}", descriptor.lap_id_prefix, lap.lap_index),
            lap_index: lap.lap_index,
            started_offset_ns: Some(lap.started_offset_ns),
            duration_ns: lap.duration_ns,
            validity: if lap.partial { "invalid" } else { "unknown" }.into(),
            validity_reason: Some(lap_validity_reason(lap)),
            max_tyres_out: lap.max_tyres_out,
            sample_start: lap.sample_start,
            sample_count: lap.sample_count,
            is_personal_best: lap
                .duration_ns
                .is_some_and(|time| Some(time) == fastest_lap),
            sectors: lap
                .sectors
                .iter()
                .map(|sector| trace_storage::metadata::NewSector {
                    index: sector.index,
                    duration_ns: sector.duration_ns,
                })
                .collect(),
        })
        .collect::<Vec<_>>();

    metadata
        .complete_session(&descriptor.session_id, &descriptor.ended_at, &blob, &laps)
        .map_err(|metadata_error| PersistenceError::OrphanedBlob {
            blob: Box::new(blob.clone()),
            metadata_error,
        })?;
    Ok(PersistedRecording {
        blob,
        lap_count: laps.len(),
    })
}

fn lap_validity_reason(lap: &crate::RecordedLap) -> String {
    if lap.partial {
        return "capture began after lap start; partial lap or outlap".into();
    }
    match lap.max_tyres_out {
        Some(0) => {
            "no tyres-out excursion observed; simulator does not expose final lap validity".into()
        }
        Some(count) => format!(
            "track-limit evidence: up to {count} tyres outside the track; simulator does not expose final lap validity"
        ),
        None => "simulator track-limit evidence unavailable".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use trace_adapter::DisconnectReason;
    use trace_domain::{
        ElapsedNanoseconds, FrameSequence, SimulatorId, SourceDescriptor, SourceKind,
        TelemetryFrame,
    };
    use trace_storage::{
        FileBlobStore, InMemoryBlobStore,
        ipc::{TelemetryIpcWriter, decode_columns},
        metadata::{MetadataStore, NewSession},
    };

    use super::*;
    use crate::{RecordedLap, RecordingEndReason};

    fn session(id: &str) -> NewSession {
        NewSession {
            id: id.into(),
            simulator_id: "sim-fixture".into(),
            simulator_key: "fixture".into(),
            simulator_version: None,
            track_id: None,
            source_track_id: None,
            layout_id: None,
            track_display_name: None,
            car_id: None,
            source_car_id: None,
            car_display_name: None,
            started_at: "2026-08-21T20:00:00Z".into(),
            session_type: Some("practice".into()),
            source_kind: "replay".into(),
        }
    }

    fn recording() -> RecordedSession {
        RecordedSession {
            source: SourceDescriptor {
                simulator: SimulatorId::parse("fixture").expect("simulator"),
                adapter_version: "1".into(),
                simulator_version: None,
                kind: SourceKind::SimulatorReplay,
            },
            seed: trace_domain::SessionSeed::default(),
            frames: (0..3)
                .map(|index| TelemetryFrame {
                    sequence: FrameSequence(index),
                    elapsed: ElapsedNanoseconds(index * 100),
                    ..TelemetryFrame::default()
                })
                .collect(),
            sample_count: 3,
            laps: vec![RecordedLap {
                lap_index: 1,
                started_offset_ns: 0,
                duration_ns: Some(200),
                sample_start: 0,
                sample_count: 3,
                partial: false,
                max_tyres_out: None,
                sectors: vec![
                    crate::RecordedSector {
                        index: 1,
                        duration_ns: 70,
                    },
                    crate::RecordedSector {
                        index: 2,
                        duration_ns: 65,
                    },
                ],
            }],
            end_reason: RecordingEndReason::Disconnected(DisconnectReason::SessionEnded),
        }
    }

    fn descriptor(session_id: &str, path: &str) -> CompletionDescriptor {
        CompletionDescriptor {
            session_id: session_id.into(),
            ended_at: "2026-08-21T20:05:00Z".into(),
            blob_path: RelativeBlobPath::parse(path).expect("path"),
            lap_id_prefix: format!("{session_id}-lap"),
        }
    }

    #[test]
    fn commits_arrow_before_indexing_session_and_laps() {
        let mut metadata = MetadataStore::open_in_memory().expect("metadata");
        metadata
            .create_session(&session("session-1"))
            .expect("session");
        let mut blobs = InMemoryBlobStore::new(1_000_000).expect("blobs");

        let persisted = persist_recording(
            &mut blobs,
            &mut metadata,
            &recording(),
            &descriptor("session-1", "sessions/session-1.arrow"),
        )
        .expect("persisted");

        assert_eq!(persisted.lap_count, 1);
        let bytes = blobs.read(&persisted.blob.id).expect("committed bytes");
        assert_eq!(decode_columns(&bytes).expect("Arrow").len(), 3);
        let summaries = metadata.recent_sessions(10).expect("summaries");
        assert_eq!(summaries[0].laps.len(), 1);
        assert_eq!(summaries[0].laps[0].validity, "unknown");
        assert!(summaries[0].laps[0].is_personal_best);
        assert_eq!(summaries[0].laps[0].sectors.len(), 2);
        assert_eq!(summaries[0].laps[0].sectors[1].duration_ns, 65);
        assert_eq!(
            summaries[0].laps[0].validity_reason.as_deref(),
            Some("simulator track-limit evidence unavailable")
        );
    }

    #[test]
    fn persists_the_initial_observation_as_an_invalid_partial_lap() {
        let mut metadata = MetadataStore::open_in_memory().expect("metadata");
        metadata
            .create_session(&session("session-partial"))
            .expect("session");
        let mut blobs = InMemoryBlobStore::new(1_000_000).expect("blobs");
        let mut partial = recording();
        partial.laps[0].duration_ns = None;
        partial.laps[0].partial = true;

        persist_recording(
            &mut blobs,
            &mut metadata,
            &partial,
            &descriptor("session-partial", "sessions/session-partial.arrow"),
        )
        .expect("persisted");

        let summaries = metadata.recent_sessions(10).expect("summaries");
        assert_eq!(summaries[0].laps[0].validity, "invalid");
        assert_eq!(summaries[0].laps[0].duration_ns, None);
        assert_eq!(
            summaries[0].laps[0].validity_reason.as_deref(),
            Some("capture began after lap start; partial lap or outlap")
        );
    }

    #[test]
    fn returns_committed_blob_when_metadata_cannot_reference_it() {
        let mut metadata = MetadataStore::open_in_memory().expect("metadata");
        let mut blobs = InMemoryBlobStore::new(1_000_000).expect("blobs");

        let error = persist_recording(
            &mut blobs,
            &mut metadata,
            &recording(),
            &descriptor("missing-session", "sessions/orphan.arrow"),
        )
        .expect_err("orphan expected");
        let PersistenceError::OrphanedBlob { blob, .. } = error else {
            panic!("expected committed orphan");
        };
        assert!(blobs.read(&blob.id).is_ok());
        assert_eq!(blobs.pending_count(), 0);
    }

    #[test]
    fn rejects_empty_recordings_without_staging_a_blob() {
        let mut empty = recording();
        empty.frames.clear();
        let mut metadata = MetadataStore::open_in_memory().expect("metadata");
        let mut blobs = InMemoryBlobStore::new(1_000_000).expect("blobs");

        assert_eq!(
            persist_recording(
                &mut blobs,
                &mut metadata,
                &empty,
                &descriptor("session", "sessions/empty.arrow"),
            ),
            Err(PersistenceError::EmptyRecording)
        );
        assert_eq!(blobs.pending_count(), 0);
    }

    #[test]
    fn streamed_recording_commits_multiple_bounded_arrow_batches() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "trace-stream-persistence-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&root).expect("root");
        let mut metadata = MetadataStore::open_in_memory().expect("metadata");
        metadata
            .create_session(&session("session-stream"))
            .expect("session");
        let mut blobs = FileBlobStore::open(&root, 1_000_000).expect("blobs");
        let source = recording();
        let mut writer =
            TelemetryIpcWriter::new(blobs.begin_writer().expect("staging"), 2).expect("writer");
        for frame in source.frames.iter().cloned() {
            writer.push(frame).expect("push");
        }
        let mut streamed = source;
        streamed.frames.clear();
        let persisted = persist_streamed_recording(
            &mut blobs,
            &mut metadata,
            &streamed,
            &descriptor("session-stream", "sessions/stream.arrow"),
            writer,
        )
        .expect("persisted");
        assert_eq!(
            decode_columns(&blobs.read(&persisted.blob.id).expect("read"))
                .expect("decode")
                .len(),
            3
        );
        drop(blobs);
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
