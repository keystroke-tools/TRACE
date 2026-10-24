import { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
	ComparisonChart,
	TrackMap,
	channelColours,
	comparisonSeries,
	deltaRange,
	filterSamplesByTelemetryWindow,
	formatGear,
	telemetryPlaybackSpeeds,
	telemetryPlaybackTimeline,
	nextTelemetryWindow,
	singleSeries,
	steeringAngleRange,
	type LapComparisonSample,
	type TelemetryWindow,
} from "@trace/ui";

type Point = {
	elapsed_seconds?: number | null;
	lap_position?: number | null;
	speed_kph?: number | null;
	throttle?: number | null;
	brake?: number | null;
	engine_rpm?: number | null;
	steering_angle_rad?: number | null;
	gear?: number | null;
	position_x_m?: number | null;
	position_z_m?: number | null;
};

type Telemetry = { points: Point[] };
type Lap = { id: string; index: number; duration_ns: number | null };
type Detail = { summary: { title: string | null; driver_name: string | null } };
type Entry = { detail: Detail; lap: Lap; telemetry: Telemetry };

type Sample = {
	progress: number;
	reference_position_x_m?: number | null;
	reference_position_z_m?: number | null;
	analysed_position_x_m?: number | null;
	analysed_position_z_m?: number | null;
	reference_speed_kph?: number | null;
	analysed_speed_kph?: number | null;
	reference_throttle?: number | null;
	analysed_throttle?: number | null;
	reference_brake?: number | null;
	analysed_brake?: number | null;
	reference_elapsed_seconds?: number | null;
	analysed_elapsed_seconds?: number | null;
	delta_seconds?: number | null;
};

type Comparison = {
	reference: Entry;
	analysed: Entry;
	samples: Sample[];
	alignment: "distance" | "time";
};

type WorkspaceSnapshot = {
	lap: Lap | null;
	telemetry: Telemetry | null;
	comparison: Comparison | null;
	message: string;
};

declare global {
	interface Window {
		traceTelemetrySnapshot?: WorkspaceSnapshot | null;
	}
}

function useWorkspace() {
	const [snapshot, setSnapshot] = useState<WorkspaceSnapshot | null>(window.traceTelemetrySnapshot || null);
	useEffect(() => {
		const receive = (event: Event) => setSnapshot((event as CustomEvent<WorkspaceSnapshot | null>).detail);
		window.addEventListener("trace:telemetry-load", receive);
		return () => window.removeEventListener("trace:telemetry-load", receive);
	}, []);
	return snapshot;
}

function formatDuration(nanoseconds: number | null) {
	if (nanoseconds == null) return "—";
	const milliseconds = Math.round(nanoseconds / 1_000_000);
	const minutes = Math.floor(milliseconds / 60_000);
	const seconds = Math.floor((milliseconds % 60_000) / 1000);
	return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds % 1000).padStart(3, "0")}`;
}

function comparisonName(entry: Entry) {
	return entry.detail.summary.driver_name || entry.detail.summary.title || `Lap ${entry.lap.index}`;
}

function ComparisonSummary() {
	const snapshot = useWorkspace();
	const comparison = snapshot?.comparison;
	if (!comparison) return null;
	const { reference, analysed, alignment } = comparison;
	const deltaSeconds = ((analysed.lap.duration_ns || 0) - (reference.lap.duration_ns || 0)) / 1_000_000_000;
	return (
		<div className="comparison-summary">
			<div className="comparison-driver reference">
				<small>REFERENCE · FASTER</small>
				<strong>{comparisonName(reference)}</strong>
				<span>
					Lap {reference.lap.index} · {formatDuration(reference.lap.duration_ns)}
				</span>
			</div>
			<div className="comparison-delta">
				<small>LAP DELTA</small>
				<strong>+{deltaSeconds.toFixed(3)}</strong>
				<span>{alignment === "distance" ? "DISTANCE ALIGNED" : "NORMALISED TIME · DISTANCE UNAVAILABLE"}</span>
			</div>
			<div className="comparison-driver analysed">
				<small>ANALYSED LAP</small>
				<strong>{comparisonName(analysed)}</strong>
				<span>
					Lap {analysed.lap.index} · {formatDuration(analysed.lap.duration_ns)}
				</span>
			</div>
			<button className="quiet" type="button" onClick={() => window.dispatchEvent(new Event("trace:comparison-clear"))}>
				Close comparison
			</button>
		</div>
	);
}

function percentage(value?: number | null) {
	if (value == null || !Number.isFinite(value)) return value;
	return Math.abs(value) <= 1.001 ? value * 100 : value;
}

function lapSamples(points: Point[]): LapComparisonSample[] {
	const last = Math.max(1, points.length - 1);
	return points.map((point, index) => ({
		distanceM: Number.isFinite(point.lap_position) ? (point.lap_position as number) * 100 : (index / last) * 100,
		referenceElapsedSeconds: point.elapsed_seconds,
		referenceSpeedKmh: point.speed_kph,
		referenceThrottlePercent: percentage(point.throttle),
		referenceBrakePercent: percentage(point.brake),
		referenceSteeringDegrees: point.steering_angle_rad == null ? null : (point.steering_angle_rad * 180) / Math.PI,
		referenceRpm: point.engine_rpm,
		referenceGear: point.gear,
		referencePositionXM: point.position_x_m,
		referencePositionZM: point.position_z_m,
	}));
}

function alignedSamples(samples: Sample[]): LapComparisonSample[] {
	return samples.map((sample) => ({
		distanceM: sample.progress * 100,
		deltaSeconds: sample.delta_seconds,
		referenceElapsedSeconds: sample.reference_elapsed_seconds,
		comparisonElapsedSeconds: sample.analysed_elapsed_seconds,
		referenceSpeedKmh: sample.reference_speed_kph,
		comparisonSpeedKmh: sample.analysed_speed_kph,
		referenceThrottlePercent: percentage(sample.reference_throttle),
		comparisonThrottlePercent: percentage(sample.analysed_throttle),
		referenceBrakePercent: percentage(sample.reference_brake),
		comparisonBrakePercent: percentage(sample.analysed_brake),
		referencePositionXM: sample.reference_position_x_m,
		referencePositionZM: sample.reference_position_z_m,
		comparisonPositionXM: sample.analysed_position_x_m,
		comparisonPositionZM: sample.analysed_position_z_m,
	}));
}

function TelemetryWorkspace() {
	const snapshot = useWorkspace();
	const [cursorIndex, setCursorIndex] = useState<number | null>(null);
	const [telemetryWindow, setTelemetryWindow] = useState<TelemetryWindow | null>(null);
	const [playing, setPlaying] = useState(false);
	const [playbackSpeed, setPlaybackSpeed] = useState(1);
	const playbackStartIndex = useRef(0);
	const playbackSpeedRef = useRef(1);
	const baseSamples = useMemo(
		() => (snapshot?.comparison ? alignedSamples(snapshot.comparison.samples) : snapshot?.telemetry ? lapSamples(snapshot.telemetry.points) : []),
		[snapshot?.comparison, snapshot?.telemetry],
	);
	const samples = useMemo(() => filterSamplesByTelemetryWindow(baseSamples, telemetryWindow), [baseSamples, telemetryWindow]);
	useEffect(() => {
		setCursorIndex(null);
		setTelemetryWindow(null);
		setPlaying(false);
	}, [snapshot?.lap?.id, snapshot?.comparison]);
	useEffect(() => {
		if (!playing) return;
		if (samples.length < 2) {
			setPlaying(false);
			return;
		}
		const timeline = telemetryPlaybackTimeline(samples);
		let index = Math.min(playbackStartIndex.current, samples.length - 1);
		let playbackTime = timeline[index] ?? 0;
		let previous = performance.now();
		let animationFrame = 0;
		const tick = (now: number) => {
			playbackTime += ((now - previous) / 1_000) * playbackSpeedRef.current;
			previous = now;
			while (index < samples.length - 1 && timeline[index + 1] <= playbackTime) index += 1;
			setCursorIndex(index);
			if (index >= samples.length - 1) {
				setPlaying(false);
				return;
			}
			animationFrame = requestAnimationFrame(tick);
		};
		animationFrame = requestAnimationFrame(tick);
		return () => cancelAnimationFrame(animationFrame);
	}, [playing, samples]);
	if (!snapshot?.lap || !snapshot.telemetry)
		return <div className="telemetry-empty">{snapshot?.message || "Select a recorded lap to review its telemetry."}</div>;
	const { lap, telemetry, comparison } = snapshot;
	const maximumSpeed = Math.max(0, ...telemetry.points.map((point) => point.speed_kph || 0));
	const maximumRpm = Math.max(0, ...telemetry.points.map((point) => point.engine_rpm || 0));
	const zoomTelemetry = (anchor: number, direction: "in" | "out") => {
		setPlaying(false);
		setTelemetryWindow((current) => nextTelemetryWindow(baseSamples, current, anchor, direction, 2));
		setCursorIndex(null);
	};
	const seek = (index: number | null) => {
		setPlaying(false);
		setCursorIndex(index);
	};
	const togglePlayback = () => {
		if (playing) {
			setPlaying(false);
			return;
		}
		const current = Math.min(Math.max(cursorIndex ?? 0, 0), Math.max(samples.length - 1, 0));
		const start = current >= samples.length - 1 ? 0 : current;
		playbackStartIndex.current = start;
		setCursorIndex(start);
		setPlaying(samples.length > 1);
	};
	const changePlaybackSpeed = (speed: number) => {
		playbackSpeedRef.current = speed;
		setPlaybackSpeed(speed);
	};
	const comparisonIsFaster = false;
	const speedSeries = comparison
		? comparisonSeries("referenceSpeedKmh", "comparisonSpeedKmh", channelColours.speed, comparisonIsFaster)
		: singleSeries("referenceSpeedKmh", channelColours.speed);
	const throttleSeries = comparison
		? comparisonSeries("referenceThrottlePercent", "comparisonThrottlePercent", channelColours.throttle, comparisonIsFaster)
		: singleSeries("referenceThrottlePercent", channelColours.throttle);
	const brakeSeries = comparison
		? comparisonSeries("referenceBrakePercent", "comparisonBrakePercent", channelColours.brake, comparisonIsFaster)
		: singleSeries("referenceBrakePercent", channelColours.brake);

	return (
		<>
			<div className="telemetry-head">
				<div>
					<span>LAP {lap.index}</span>
					<strong>{formatDuration(lap.duration_ns)}</strong>
				</div>
				<div className="telemetry-stats">
					{telemetryWindow && (
						<button
							type="button"
							onClick={() => {
								setPlaying(false);
								setTelemetryWindow(null);
								setCursorIndex(null);
							}}
							className="telemetry-reset-zoom"
						>
							{Math.round(telemetryWindow.startM)}–{Math.round(telemetryWindow.endM)}% · RESET ZOOM
						</button>
					)}
					<span>
						<strong>{Math.round(maximumSpeed)} km/h</strong>
						<small>MAX SPEED</small>
					</span>
					<span>
						<strong>{Math.round(maximumRpm)} rpm</strong>
						<small>MAX RPM</small>
					</span>
				</div>
			</div>
			<div className="telemetry-playback">
				<button
					type="button"
					onClick={togglePlayback}
					disabled={samples.length < 2}
					aria-label={playing ? "Pause telemetry playback" : "Play telemetry playback"}
				>
					{playing ? (
						<svg viewBox="0 0 12 12" aria-hidden="true">
							<path d="M2 1h3v10H2zm5 0h3v10H7z" />
						</svg>
					) : (
						<svg viewBox="0 0 12 12" aria-hidden="true">
							<path d="m3 1 8 5-8 5z" />
						</svg>
					)}
				</button>
				<select value={playbackSpeed} onChange={(event) => changePlaybackSpeed(Number(event.target.value))} aria-label="Telemetry playback speed">
					{telemetryPlaybackSpeeds.map((speed) => (
						<option value={speed} key={speed}>
							{speed}×
						</option>
					))}
				</select>
				<span>{Math.round(samples[0]?.distanceM ?? 0)}%</span>
				<input
					type="range"
					min="0"
					max={Math.max(samples.length - 1, 0)}
					step="1"
					value={Math.min(Math.max(cursorIndex ?? 0, 0), Math.max(samples.length - 1, 0))}
					disabled={samples.length < 2}
					onChange={(event) => seek(Number(event.target.value))}
					aria-label="Seek through lap distance"
				/>
				<span>{Math.round(samples.at(-1)?.distanceM ?? 0)}%</span>
			</div>
			<div className="visual-grid">
				<div className="web-track-map">
					<TrackMap
						samples={samples}
						cursorIndex={cursorIndex}
						comparison={Boolean(comparison)}
						height={360}
						onRangeZoom={zoomTelemetry}
						rangeZoomLinked={false}
					/>
				</div>
				<ComparisonChart
					label="SPEED"
					unit="km/h"
					samples={samples}
					series={speedSeries}
					cursorIndex={cursorIndex}
					onCursor={seek}
					onZoom={zoomTelemetry}
					distanceUnit="%"
				/>
				<div className="telemetry-chart-wide">
					<ComparisonChart
						label="THROTTLE"
						unit="%"
						samples={samples}
						series={throttleSeries}
						cursorIndex={cursorIndex}
						onCursor={seek}
						onZoom={zoomTelemetry}
						fixedRange={[0, 100]}
						distanceUnit="%"
					/>
				</div>
				<div className="telemetry-chart-wide">
					<ComparisonChart
						label="BRAKE"
						unit="%"
						samples={samples}
						series={brakeSeries}
						cursorIndex={cursorIndex}
						onCursor={seek}
						onZoom={zoomTelemetry}
						fixedRange={[0, 100]}
						distanceUnit="%"
					/>
				</div>
				{!comparison && (
					<>
						<ComparisonChart
							label="STEERING ANGLE"
							unit="°"
							samples={samples}
							series={singleSeries("referenceSteeringDegrees", channelColours.steering)}
							cursorIndex={cursorIndex}
							onCursor={seek}
							onZoom={zoomTelemetry}
							fixedRange={steeringAngleRange(samples)}
							zeroLine
							distanceUnit="%"
						/>
						<ComparisonChart
							label="GEAR"
							unit=""
							samples={samples}
							series={singleSeries("referenceGear", channelColours.gear)}
							cursorIndex={cursorIndex}
							onCursor={seek}
							onZoom={zoomTelemetry}
							fixedRange={[-1, 8]}
							formatValue={(value) => formatGear(value)}
							distanceUnit="%"
						/>
					</>
				)}
				{comparison && (
					<div className="telemetry-chart-wide">
						<ComparisonChart
							label="TIME DELTA · ANALYSED VS REFERENCE"
							unit="s"
							samples={samples}
							series={singleSeries("deltaSeconds", channelColours.delta)}
							cursorIndex={cursorIndex}
							onCursor={seek}
							onZoom={zoomTelemetry}
							fixedRange={deltaRange(samples)}
							zeroLine
							distanceUnit="%"
						/>
					</div>
				)}
			</div>
		</>
	);
}

const summaryRoot = document.querySelector("#comparison-summary-root");
if (summaryRoot) createRoot(summaryRoot).render(<ComparisonSummary />);
const workspaceRoot = document.querySelector("#telemetry-workspace-root");
if (workspaceRoot) createRoot(workspaceRoot).render(<TelemetryWorkspace />);

export { ComparisonSummary, TelemetryWorkspace };
