//! Self-contained, versioned TRACE session exchange packages.

use std::io::{self, Read, Seek, SeekFrom, Write};

use serde::{Deserialize, Serialize};

use crate::{
    ipc::{IpcError, compact_for_sharing},
    metadata::{LapSummary, NewLap, NewSector, NewSession, SessionSummary},
};

pub const PACKAGE_VERSION: u32 = 1;
pub const MAX_MANIFEST_BYTES: u32 = 1024 * 1024;
const MAGIC: &[u8; 8] = b"TRACEPKG";
const HEADER_LENGTH: u64 = 24;
const HEADER_BYTES: usize = 24;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionPackageManifest {
    pub format_version: u32,
    pub telemetry_schema_version: u32,
    pub session: SessionSummary,
    pub laps: Vec<SessionPackageLap>,
    pub sample_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SessionPackageLap {
    pub summary: LapSummary,
    pub sample_start: u64,
    pub sample_count: u64,
}

#[derive(Debug)]
pub enum PackageError {
    Io(io::Error),
    Json(serde_json::Error),
    Ipc(IpcError),
    Invalid(&'static str),
    UnsupportedVersion(u32),
}

impl From<io::Error> for PackageError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for PackageError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<IpcError> for PackageError {
    fn from(value: IpcError) -> Self {
        Self::Ipc(value)
    }
}

/// Writes a manifest and telemetry stream as one session package.
///
/// # Errors
///
/// Returns [`PackageError`] for inconsistent metadata, an oversized manifest, JSON
/// encoding failure, or an output/read error.
pub fn write_package<W: Write, R: Read>(
    mut destination: W,
    mut telemetry: R,
    manifest: &SessionPackageManifest,
) -> Result<u64, PackageError> {
    write_package_header(&mut destination, manifest)?;
    io::copy(&mut telemetry, &mut destination).map_err(PackageError::from)
}

/// Writes a compact, shareable package while retaining every canonical channel and
/// the source-native values used by TRACE's current analysis features.
///
/// # Errors
///
/// Returns [`PackageError`] for invalid metadata, unsupported Arrow telemetry, a
/// sample-count mismatch, or an output/read error.
pub fn write_compact_package<W: Write, R: Read + Seek>(
    mut destination: W,
    telemetry: R,
    manifest: &SessionPackageManifest,
) -> Result<u64, PackageError> {
    write_package_header(&mut destination, manifest)?;
    let samples = compact_for_sharing(telemetry, &mut destination)?;
    if samples != manifest.sample_count {
        return Err(PackageError::Invalid(
            "session sample count does not match telemetry",
        ));
    }
    Ok(samples)
}

fn write_package_header<W: Write>(
    destination: &mut W,
    manifest: &SessionPackageManifest,
) -> Result<(), PackageError> {
    validate_manifest(manifest, manifest.sample_count)?;
    let metadata = serde_json::to_vec(manifest)?;
    let metadata_length = u32::try_from(metadata.len())
        .map_err(|_| PackageError::Invalid("session metadata is too large"))?;
    if metadata_length > MAX_MANIFEST_BYTES {
        return Err(PackageError::Invalid("session metadata is too large"));
    }
    destination.write_all(MAGIC)?;
    destination.write_all(&PACKAGE_VERSION.to_le_bytes())?;
    destination.write_all(&metadata_length.to_le_bytes())?;
    destination.write_all(&manifest.sample_count.to_le_bytes())?;
    destination.write_all(&metadata)?;
    Ok(())
}

/// Parses and bounds a session package while leaving telemetry available as a stream.
///
/// # Errors
///
/// Returns [`PackageError`] for malformed or unsupported headers, invalid metadata,
/// resource-limit violations, JSON decoding failure, or input seek/read errors.
pub fn read_package<R: Read + Seek>(
    mut source: R,
    total_length: u64,
    maximum_telemetry_bytes: u64,
) -> Result<SessionPackage<R>, PackageError> {
    let mut header = [0_u8; HEADER_BYTES];
    source.read_exact(&mut header)?;
    if &header[..8] != MAGIC {
        return Err(PackageError::Invalid("not a TRACE session package"));
    }
    let version = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
    if version != PACKAGE_VERSION {
        return Err(PackageError::UnsupportedVersion(version));
    }
    let metadata_length = u32::from_le_bytes([header[12], header[13], header[14], header[15]]);
    if metadata_length == 0 || metadata_length > MAX_MANIFEST_BYTES {
        return Err(PackageError::Invalid("invalid session metadata size"));
    }
    let declared_samples = u64::from_le_bytes([
        header[16], header[17], header[18], header[19], header[20], header[21], header[22],
        header[23],
    ]);
    let metadata_size = usize::try_from(metadata_length)
        .map_err(|_| PackageError::Invalid("session metadata size is not representable"))?;
    let mut metadata = vec![0; metadata_size];
    source.read_exact(&mut metadata)?;
    let manifest: SessionPackageManifest = serde_json::from_slice(&metadata)?;
    validate_manifest(&manifest, declared_samples)?;
    let telemetry_start = HEADER_LENGTH + u64::from(metadata_length);
    let telemetry_length = total_length
        .checked_sub(telemetry_start)
        .filter(|length| *length > 0 && *length <= maximum_telemetry_bytes)
        .ok_or(PackageError::Invalid("telemetry is empty or too large"))?;
    Ok(SessionPackage {
        manifest,
        telemetry: SegmentReader::new(source, telemetry_start, telemetry_length)?,
    })
}

/// Converts portable package metadata into fresh local records. Imported ownership is
/// deliberately applied separately by the caller as `other`.
pub fn imported_records(
    session_id: &str,
    manifest: &SessionPackageManifest,
) -> (NewSession, Vec<NewLap>) {
    let session = &manifest.session;
    let new_session = NewSession {
        id: session_id.into(),
        simulator_id: format!("sim-{}", session.simulator_key),
        simulator_key: session.simulator_key.clone(),
        simulator_version: None,
        track_id: session
            .source_track_id
            .as_deref()
            .map(|source| format!("track-{}", hex_identity(source))),
        source_track_id: session.source_track_id.clone(),
        layout_id: session.layout_id.clone(),
        track_display_name: session.track.clone(),
        car_id: session
            .source_car_id
            .as_deref()
            .map(|source| format!("car-{}", hex_identity(source))),
        source_car_id: session.source_car_id.clone(),
        car_display_name: session.car.clone(),
        started_at: session.started_at.clone(),
        session_type: session.session_type.clone(),
        source_kind: "imported".into(),
        conditions: session.conditions.clone(),
    };
    let laps = manifest
        .laps
        .iter()
        .map(|lap| NewLap {
            id: format!("{session_id}-lap-{}", lap.summary.index),
            lap_index: lap.summary.index,
            started_offset_ns: None,
            duration_ns: lap.summary.duration_ns,
            validity: lap.summary.validity.clone(),
            validity_reason: lap.summary.validity_reason.clone(),
            max_tyres_out: lap.summary.max_tyres_out,
            sample_start: lap.sample_start,
            sample_count: lap.sample_count,
            is_personal_best: lap.summary.is_personal_best,
            sectors: lap
                .summary
                .sectors
                .iter()
                .map(|sector| NewSector {
                    index: sector.index,
                    duration_ns: sector.duration_ns,
                })
                .collect(),
        })
        .collect();
    (new_session, laps)
}

fn hex_identity(value: &str) -> String {
    use std::fmt::Write as _;
    value.as_bytes().iter().fold(
        String::with_capacity(value.len() * 2),
        |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
            output
        },
    )
}

fn validate_manifest(
    manifest: &SessionPackageManifest,
    declared_samples: u64,
) -> Result<(), PackageError> {
    if manifest.format_version != PACKAGE_VERSION
        || manifest.sample_count == 0
        || manifest.sample_count != declared_samples
        || manifest.telemetry_schema_version == 0
    {
        return Err(PackageError::Invalid("inconsistent session metadata"));
    }
    if manifest.laps.iter().any(|lap| {
        lap.sample_count == 0
            || lap
                .sample_start
                .checked_add(lap.sample_count)
                .is_none_or(|end| end > manifest.sample_count)
    }) {
        return Err(PackageError::Invalid("invalid lap telemetry range"));
    }
    Ok(())
}

pub struct SessionPackage<R> {
    pub manifest: SessionPackageManifest,
    pub telemetry: SegmentReader<R>,
}

pub struct SegmentReader<R> {
    source: R,
    start: u64,
    length: u64,
    position: u64,
}

impl<R: Seek> SegmentReader<R> {
    fn new(mut source: R, start: u64, length: u64) -> Result<Self, io::Error> {
        source.seek(SeekFrom::Start(start))?;
        Ok(Self {
            source,
            start,
            length,
            position: 0,
        })
    }
}

impl<R: Read> Read for SegmentReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let buffer_length = u64::try_from(buffer.len()).unwrap_or(u64::MAX);
        let allowed = usize::try_from(self.length.saturating_sub(self.position).min(buffer_length))
            .unwrap_or(buffer.len());
        let read = self.source.read(&mut buffer[..allowed])?;
        self.position = self
            .position
            .saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
        Ok(read)
    }
}

impl<R: Seek> Seek for SegmentReader<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let next = match position {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(value) => i128::from(self.position) + i128::from(value),
            SeekFrom::End(value) => i128::from(self.length) + i128::from(value),
        };
        if next < 0 || next > i128::from(self.length) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek outside package telemetry",
            ));
        }
        self.position = u64::try_from(next).expect("validated segment position");
        self.source
            .seek(SeekFrom::Start(self.start + self.position))?;
        Ok(self.position)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use trace_domain::{ElapsedNanoseconds, FrameSequence, TelemetryFrame};

    use super::*;
    use crate::{
        BlobCommit, BlobFormat, InMemoryBlobStore, RelativeBlobPath, TelemetryBlobStore,
        ipc::{TELEMETRY_SCHEMA_VERSION, encode_frames, read_columns_range, sample_count},
        metadata::{MetadataStore, SectorSummary, SessionSummary},
    };

    #[test]
    #[allow(clippy::too_many_lines)]
    fn session_package_round_trip_preserves_metadata_laps_and_telemetry() {
        let telemetry = encode_frames(
            &(0..3)
                .map(|sequence| TelemetryFrame {
                    sequence: FrameSequence(sequence),
                    elapsed: ElapsedNanoseconds(sequence * 10),
                    ..TelemetryFrame::default()
                })
                .collect::<Vec<_>>(),
        )
        .expect("telemetry");
        let manifest = SessionPackageManifest {
            format_version: PACKAGE_VERSION,
            telemetry_schema_version: TELEMETRY_SCHEMA_VERSION,
            session: SessionSummary {
                id: "friend-session".into(),
                simulator_key: "assetto-corsa".into(),
                source_track_id: Some("ks_silverstone".into()),
                layout_id: Some("gp".into()),
                source_car_id: Some("ks_mazda_mx5_cup".into()),
                user_title: Some("Alex practice".into()),
                user_driver: Some("Alex".into()),
                ownership: "mine".into(),
                tags: vec!["shared".into()],
                track: Some("Silverstone".into()),
                car: Some("Mazda MX-5 Cup".into()),
                session_type: Some("practice".into()),
                started_at: "2026-08-22T12:00:00Z".into(),
                source_kind: "native_capture".into(),
                conditions: crate::metadata::SessionConditions {
                    ambient_temperature_c: Some("15".into()),
                    road_temperature_c: Some("12".into()),
                    weather_name: Some("2_light_fog".into()),
                    track_grip_percent: Some(95),
                },
                exportable: true,
                laps: Vec::new(),
            },
            laps: vec![SessionPackageLap {
                summary: LapSummary {
                    id: "lap-1".into(),
                    index: 1,
                    duration_ns: Some(90_000_000_000),
                    validity: "valid".into(),
                    validity_reason: None,
                    max_tyres_out: Some(0),
                    is_personal_best: true,
                    sectors: vec![SectorSummary {
                        index: 1,
                        duration_ns: 30_000_000_000,
                    }],
                },
                sample_start: 0,
                sample_count: 3,
            }],
            sample_count: 3,
        };
        let mut legacy_manifest = serde_json::to_value(&manifest).expect("legacy manifest");
        legacy_manifest["session"]
            .as_object_mut()
            .expect("session object")
            .remove("conditions");
        let legacy_manifest: SessionPackageManifest =
            serde_json::from_value(legacy_manifest).expect("legacy package metadata");
        assert_eq!(
            legacy_manifest.session.conditions,
            crate::metadata::SessionConditions::default()
        );

        let mut package_bytes = Vec::new();
        write_compact_package(&mut package_bytes, Cursor::new(&telemetry), &manifest)
            .expect("package");

        let package = read_package(
            Cursor::new(&package_bytes),
            u64::try_from(package_bytes.len()).expect("length"),
            1024 * 1024,
        )
        .expect("read package");
        assert_eq!(package.manifest, manifest);
        assert_eq!(
            sample_count(package.telemetry).expect("sample count"),
            manifest.sample_count
        );

        let package = read_package(
            Cursor::new(&package_bytes),
            u64::try_from(package_bytes.len()).expect("length"),
            1024 * 1024,
        )
        .expect("read package again");
        let projected = read_columns_range(package.telemetry, 0, 3).expect("projection");
        assert_eq!(projected.sequence, vec![0, 1, 2]);

        let mut package = read_package(
            Cursor::new(&package_bytes),
            u64::try_from(package_bytes.len()).expect("length"),
            1024 * 1024,
        )
        .expect("import package");
        let (session, laps) = imported_records("imported-1", &package.manifest);
        let mut metadata = MetadataStore::open_in_memory().expect("metadata");
        metadata.create_session(&session).expect("create session");
        let mut telemetry_bytes = Vec::new();
        package
            .telemetry
            .read_to_end(&mut telemetry_bytes)
            .expect("read payload");
        let mut blobs = InMemoryBlobStore::new(1024 * 1024).expect("blobs");
        let pending = blobs.begin().expect("pending");
        blobs.append(&pending, &telemetry_bytes).expect("append");
        let blob = blobs
            .commit(
                &pending,
                BlobCommit {
                    path: RelativeBlobPath::parse("sessions/imported-1.arrow").expect("path"),
                    format: BlobFormat::ArrowIpc,
                    schema_version: package.manifest.telemetry_schema_version,
                    sample_count: package.manifest.sample_count,
                    expected_sha256: None,
                },
            )
            .expect("commit");
        metadata
            .complete_session(
                "imported-1",
                &package.manifest.session.started_at,
                &blob,
                &laps,
            )
            .expect("complete");
        metadata
            .update_session_details(
                "imported-1",
                package.manifest.session.user_title.as_deref(),
                package.manifest.session.user_driver.as_deref(),
                "other",
                &package.manifest.session.tags,
            )
            .expect("details");
        let imported = metadata.recent_sessions(1).expect("sessions").remove(0);
        assert_eq!(imported.source_kind, "imported");
        assert_eq!(imported.ownership, "other");
        assert_eq!(imported.user_driver.as_deref(), Some("Alex"));
        assert_eq!(imported.tags, vec!["shared"]);
        assert_eq!(imported.laps[0].sectors[0].duration_ns, 30_000_000_000);
    }
}
