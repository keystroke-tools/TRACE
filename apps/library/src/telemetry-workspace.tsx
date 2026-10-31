import { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
	ComparisonChart,
	ComparisonAnalysisPanel,
	ComparisonTelemetryHud,
	LapTelemetryHud,
	SectorPicker,
	TrackMap,
	channelColours,
	comparisonSeries,
	comparisonAnalysis,
	comparisonSectorDeltas,
	deltaRange,
	filterSamplesByTelemetryWindow,
	filterSamplesBySector,
	formatSessionType,
	formatGear,
	telemetryPlaybackTimeline,
	nextTelemetryWindow,
	panTelemetryWindow,
	singleSeries,
	steeringAngleRange,
	type LapComparisonSample,
	type TelemetryWindow,
} from "@trace/ui";

type Point = {
	elapsed_seconds?: number | null;
	sector_index?: number | null;
	lap_position?: number | null;
	speed_kph?: number | null;
	throttle?: number | null;
	brake?: number | null;
	engine_rpm?: number | null;
	steering_angle_rad?: number | null;
	gear?: number | null;
	position_x_m?: number | null;
	position_z_m?: number | null;
	tc_active?: boolean | null;
	abs_active?: boolean | null;
	rear_flap_active?: boolean | null;
};

type Telemetry = { points: Point[] };
type Lap = { id: string; index: number; duration_ns: number | null; sectors?: Array<{ index: number; duration_ns: number | null }> };
type ElectronicSetting = {
	key: string;
	label: string;
	start: number;
	end: number;
	minimum: number;
	maximum: number;
	available_maximum?: number | null;
	changes: number;
};
type LapMetrics = {
	lap_id: string;
	lap_index: number;
	fuel_start_litres?: number | null;
	fuel_end_litres?: number | null;
	fuel_used_litres?: number | null;
	fuel_capacity_litres?: number | null;
	max_speed_kmh?: number | null;
	tyre_wear_start: Array<number | null>;
	tyre_wear_end: Array<number | null>;
	tyre_wear_minimum: Array<number | null>;
	tyre_flat_observed: boolean[];
	wheel_detached_observed: boolean[];
	dent_severity_maximum: number[];
	overheating_observed: boolean;
	body_part_detached_observed: boolean;
	impact_count: number;
	maximum_impact_magnitude?: number | null;
	virtual_energy_start?: number | null;
	virtual_energy_end?: number | null;
	virtual_energy_used?: number | null;
	electronics?: {
		settings: ElectronicSetting[];
		tc_active_percent?: number | null;
		abs_active_percent?: number | null;
		rear_flap_active_percent?: number | null;
	} | null;
};
type Detail = {
	summary: {
		title: string | null;
		driver_name: string | null;
		track?: string | null;
		car?: string | null;
		session_type?: string | null;
	};
	lap_metrics?: LapMetrics[];
};
type Entry = { detail: Detail; lap: Lap; telemetry: Telemetry };

type Sample = {
	progress: number;
	reference_sector_index?: number | null;
	analysed_sector_index?: number | null;
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
	reference_engine_rpm?: number | null;
	analysed_engine_rpm?: number | null;
	reference_steering_angle_rad?: number | null;
	analysed_steering_angle_rad?: number | null;
	reference_gear?: number | null;
	analysed_gear?: number | null;
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
	detail: Detail | null;
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

function percentage(value?: number | null) {
	if (value == null || !Number.isFinite(value)) return value;
	return Math.abs(value) <= 1.001 ? value * 100 : value;
}

function lapSamples(points: Point[]): LapComparisonSample[] {
	const last = Math.max(1, points.length - 1);
	return points.map((point, index) => ({
		distanceM: Number.isFinite(point.lap_position) ? (point.lap_position as number) * 100 : (index / last) * 100,
		sectorIndex: point.sector_index,
		referenceElapsedSeconds: point.elapsed_seconds,
		referenceSpeedKmh: point.speed_kph,
		referenceThrottlePercent: percentage(point.throttle),
		referenceBrakePercent: percentage(point.brake),
		referenceSteeringDegrees: point.steering_angle_rad == null ? null : (point.steering_angle_rad * 180) / Math.PI,
		referenceRpm: point.engine_rpm,
		referenceGear: point.gear,
		referencePositionXM: point.position_x_m,
		referencePositionZM: point.position_z_m,
		referenceTcActive: point.tc_active,
		referenceAbsActive: point.abs_active,
		referenceRearFlapActive: point.rear_flap_active,
	}));
}

function alignedSamples(samples: Sample[]): LapComparisonSample[] {
	return samples.map((sample) => ({
		distanceM: sample.progress * 100,
		sectorIndex:
			sample.reference_sector_index == null && sample.analysed_sector_index == null
				? null
				: Math.round(sample.reference_sector_index ?? sample.analysed_sector_index ?? 0),
		deltaSeconds: sample.delta_seconds,
		referenceElapsedSeconds: sample.reference_elapsed_seconds,
		comparisonElapsedSeconds: sample.analysed_elapsed_seconds,
		referenceSpeedKmh: sample.reference_speed_kph,
		comparisonSpeedKmh: sample.analysed_speed_kph,
		referenceThrottlePercent: percentage(sample.reference_throttle),
		comparisonThrottlePercent: percentage(sample.analysed_throttle),
		referenceBrakePercent: percentage(sample.reference_brake),
		comparisonBrakePercent: percentage(sample.analysed_brake),
		referenceRpm: sample.reference_engine_rpm,
		comparisonRpm: sample.analysed_engine_rpm,
		referenceSteeringDegrees: sample.reference_steering_angle_rad == null ? null : (sample.reference_steering_angle_rad * 180) / Math.PI,
		comparisonSteeringDegrees: sample.analysed_steering_angle_rad == null ? null : (sample.analysed_steering_angle_rad * 180) / Math.PI,
		referenceGear: sample.reference_gear,
		comparisonGear: sample.analysed_gear,
		referencePositionXM: sample.reference_position_x_m,
		referencePositionZM: sample.reference_position_z_m,
		comparisonPositionXM: sample.analysed_position_x_m,
		comparisonPositionZM: sample.analysed_position_z_m,
	}));
}

function median(values: number[]) {
	if (!values.length) return null;
	const sorted = [...values].sort((left, right) => left - right);
	const middle = Math.floor(sorted.length / 2);
	return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

function finite(value?: number | null): value is number {
	return value != null && Number.isFinite(value);
}

function tyreTone(remaining: number) {
	if (remaining >= 75) return "healthy";
	if (remaining >= 50) return "used";
	if (remaining >= 25) return "worn";
	return "critical";
}

function LapMetricsSummary({ metric, allMetrics }: { metric?: LapMetrics; allMetrics: LapMetrics[] }) {
	if (!metric) return null;
	const tyreLabels = ["FL", "FR", "RL", "RR"];
	const hasTyres = metric.tyre_wear_end.some(finite);
	const fuelPerLap = median(
		allMetrics
			.map((item) => item.fuel_used_litres)
			.filter(finite)
			.filter((value) => value > 0),
	);
	const energyScale = [metric.virtual_energy_start, metric.virtual_energy_end].filter(finite).every((value) => value <= 1.001) ? 100 : 1;
	const energyPerLap = median(
		allMetrics
			.map((item) => item.virtual_energy_used)
			.filter(finite)
			.filter((value) => value > 0),
	);
	const damagedZones = metric.dent_severity_maximum.filter((value) => value > 0).length;
	const failures = [
		metric.overheating_observed ? "OVERHEAT" : null,
		metric.body_part_detached_observed ? "PART LOST" : null,
		metric.tyre_flat_observed.some(Boolean) ? "PUNCTURE" : null,
		metric.wheel_detached_observed.some(Boolean) ? "WHEEL LOST" : null,
	].filter((value): value is string => Boolean(value));
	const electronics = metric.electronics;
	const hasFuel = finite(metric.fuel_end_litres) || finite(metric.fuel_used_litres);
	const hasEnergy = finite(metric.virtual_energy_end) || finite(metric.virtual_energy_used);
	const hasDamage = damagedZones > 0 || metric.impact_count > 0 || failures.length > 0;
	if (!hasTyres && !hasFuel && !hasEnergy && !electronics && !hasDamage) return null;
	const fuelPercent =
		finite(metric.fuel_end_litres) && finite(metric.fuel_capacity_litres) && metric.fuel_capacity_litres > 0
			? Math.min(100, Math.max(0, (metric.fuel_end_litres / metric.fuel_capacity_litres) * 100))
			: null;
	return (
		<section className="lap-metrics" aria-label={`Lap ${metric.lap_index} vehicle state`}>
			<header>
				<strong>LAP STATE</strong>
				<span>Recorded at the end of lap {metric.lap_index}</span>
			</header>
			<div className="lap-metrics-grid">
				{hasTyres && (
					<div className="lap-metric-block tyre-state">
						<small>TYRE LIFE</small>
						<div className="tyre-state-grid">
							{tyreLabels.map((label, index) => {
								const value = metric.tyre_wear_end[index];
								const remaining = finite(value) ? Math.min(100, Math.max(0, value)) : null;
								const failed = metric.tyre_flat_observed[index] || metric.wheel_detached_observed[index];
								return (
									<span className={`tyre-state-wheel ${failed ? "failed" : remaining == null ? "unknown" : tyreTone(remaining)}`} key={label}>
										<b>{failed ? "!" : remaining == null ? "—" : `${Math.round(remaining)}%`}</b>
										<em>{label}</em>
									</span>
								);
							})}
						</div>
					</div>
				)}
				{hasFuel && (
					<div className="lap-metric-block resource-state">
						<small>FUEL</small>
						<strong>{finite(metric.fuel_end_litres) ? `${metric.fuel_end_litres.toFixed(1)} L` : "—"}</strong>
						<span>
							{finite(metric.fuel_used_litres) ? `${metric.fuel_used_litres.toFixed(2)} L used` : "Usage unavailable"}
							{fuelPerLap != null ? ` · ${fuelPerLap.toFixed(2)} L/lap median` : ""}
						</span>
						{fuelPercent != null && (
							<i aria-hidden="true">
								<b style={{ width: `${fuelPercent}%` }} />
							</i>
						)}
					</div>
				)}
				{hasEnergy && (
					<div className="lap-metric-block resource-state">
						<small>VIRTUAL ENERGY</small>
						<strong>
							{finite(metric.virtual_energy_end)
								? `${(metric.virtual_energy_end * energyScale).toFixed(1)}${energyScale === 100 ? "%" : ""}`
								: "—"}
						</strong>
						<span>
							{finite(metric.virtual_energy_used)
								? `${(metric.virtual_energy_used * energyScale).toFixed(1)}${energyScale === 100 ? "%" : ""} used`
								: "Usage unavailable"}
							{energyPerLap != null ? ` · ${(energyPerLap * energyScale).toFixed(1)}${energyScale === 100 ? "%" : ""}/lap median` : ""}
						</span>
					</div>
				)}
				{electronics && (
					<div className="lap-metric-block electronics-state">
						<small>ELECTRONICS</small>
						<div>
							{electronics.settings.map((setting) => (
								<span key={setting.key}>
									{setting.label} {setting.changes ? `${setting.start}→${setting.end}` : setting.start}
								</span>
							))}
							{finite(electronics.tc_active_percent) && electronics.tc_active_percent >= 0.1 && (
								<span>TC {Math.round(electronics.tc_active_percent)}%</span>
							)}
							{finite(electronics.abs_active_percent) && electronics.abs_active_percent >= 0.1 && (
								<span>ABS {Math.round(electronics.abs_active_percent)}%</span>
							)}
							{finite(electronics.rear_flap_active_percent) && electronics.rear_flap_active_percent >= 0.1 && (
								<span>DRS {Math.round(electronics.rear_flap_active_percent)}%</span>
							)}
						</div>
					</div>
				)}
				{hasDamage && (
					<div className="lap-metric-block damage-state">
						<small>DAMAGE</small>
						<div>
							{damagedZones > 0 && <span>BODY {damagedZones}/8</span>}
							{metric.impact_count > 0 && <span>IMPACT ×{metric.impact_count}</span>}
							{failures.map((failure) => (
								<span key={failure}>{failure}</span>
							))}
						</div>
					</div>
				)}
			</div>
		</section>
	);
}

function TelemetryWorkspace() {
	const snapshot = useWorkspace();
	const [cursorIndex, setCursorIndex] = useState<number | null>(null);
	const [sector, setSector] = useState<number | null>(null);
	const [analysisSelection, setAnalysisSelection] = useState<string | null>(null);
	const [telemetryWindow, setTelemetryWindow] = useState<TelemetryWindow | null>(null);
	const [playing, setPlaying] = useState(false);
	const [playbackSpeed, setPlaybackSpeed] = useState(1);
	const playbackStartIndex = useRef(0);
	const playbackSpeedRef = useRef(1);
	const baseSamples = useMemo(
		() => (snapshot?.comparison ? alignedSamples(snapshot.comparison.samples) : snapshot?.telemetry ? lapSamples(snapshot.telemetry.points) : []),
		[snapshot?.comparison, snapshot?.telemetry],
	);
	const sectorSamples = useMemo(() => filterSamplesBySector(baseSamples, sector), [baseSamples, sector]);
	const samples = useMemo(() => filterSamplesByTelemetryWindow(sectorSamples, telemetryWindow), [sectorSamples, telemetryWindow]);
	useEffect(() => {
		setCursorIndex(null);
		setSector(null);
		setAnalysisSelection(null);
		setTelemetryWindow(null);
		setPlaying(false);
	}, [snapshot?.lap?.id, snapshot?.comparison]);
	useEffect(() => {
		const review = document.querySelector("#review-view");
		review?.classList.toggle("comparison-active", Boolean(snapshot?.comparison));
		return () => review?.classList.remove("comparison-active");
	}, [snapshot?.comparison]);
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
	const { detail, lap, telemetry, comparison } = snapshot;
	const lapMetric = detail?.lap_metrics?.find((metric) => metric.lap_id === lap.id);
	const maximumSpeed = Math.max(0, ...telemetry.points.map((point) => point.speed_kph || 0));
	const maximumRpm = Math.max(0, ...telemetry.points.map((point) => point.engine_rpm || 0));
	const zoomTelemetry = (anchor: number, direction: "in" | "out") => {
		setPlaying(false);
		setTelemetryWindow((current) => nextTelemetryWindow(sectorSamples, current, anchor, direction, 2));
		setCursorIndex(null);
	};
	const panTelemetry = (delta: number) => {
		setPlaying(false);
		setTelemetryWindow((current) => panTelemetryWindow(sectorSamples, current, delta));
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
	const sectorDeltas = comparison
		? comparisonSectorDeltas(
				(comparison.reference.lap.sectors || []).map((item) => ({ index: item.index, durationNs: item.duration_ns })),
				(comparison.analysed.lap.sectors || []).map((item) => ({ index: item.index, durationNs: item.duration_ns })),
				baseSamples,
			)
		: [];
	const analysisSegments = comparison ? comparisonAnalysis(baseSamples) : [];
	const speedSeries = comparison
		? comparisonSeries("referenceSpeedKmh", "comparisonSpeedKmh", channelColours.speed, comparisonIsFaster)
		: singleSeries("referenceSpeedKmh", channelColours.speed);
	const throttleSeries = comparison
		? comparisonSeries("referenceThrottlePercent", "comparisonThrottlePercent", channelColours.throttle, comparisonIsFaster)
		: singleSeries("referenceThrottlePercent", channelColours.throttle);
	if (!comparison && baseSamples.some((sample) => sample.referenceTcActive)) {
		throttleSeries.push({
			label: "TC ACTIVE",
			colour: channelColours.intervention,
			strokeWidth: 3.5,
			value: (sample) => (sample.referenceTcActive ? sample.referenceThrottlePercent : null),
		});
	}
	const brakeSeries = comparison
		? comparisonSeries("referenceBrakePercent", "comparisonBrakePercent", channelColours.brake, comparisonIsFaster)
		: singleSeries("referenceBrakePercent", channelColours.brake);
	if (!comparison && baseSamples.some((sample) => sample.referenceAbsActive)) {
		brakeSeries.push({
			label: "ABS ACTIVE",
			colour: channelColours.intervention,
			strokeWidth: 3.5,
			value: (sample) => (sample.referenceAbsActive ? sample.referenceBrakePercent : null),
		});
	}
	const steeringSeries = comparison
		? comparisonSeries("referenceSteeringDegrees", "comparisonSteeringDegrees", channelColours.steering, comparisonIsFaster)
		: singleSeries("referenceSteeringDegrees", channelColours.steering);
	const rpmSeries = comparison
		? comparisonSeries("referenceRpm", "comparisonRpm", channelColours.rpm, comparisonIsFaster)
		: singleSeries("referenceRpm", channelColours.rpm);
	const gearSeries = comparison
		? comparisonSeries("referenceGear", "comparisonGear", channelColours.gear, comparisonIsFaster)
		: singleSeries("referenceGear", channelColours.gear);

	return (
		<>
			{!comparison && (
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
			)}
			<div className={`comparison-review-layout ${comparison ? "with-analysis" : "single"}`}>
				{comparison && (
					<div className="comparison-analysis-column">
						<ComparisonAnalysisPanel
							segments={analysisSegments}
							selectedId={analysisSelection}
							onSelect={(segment) => {
								setPlaying(false);
								setAnalysisSelection(segment?.id ?? null);
								setCursorIndex(null);
								if (!segment) {
									setSector(null);
									setTelemetryWindow(null);
								} else if (segment.sectorIndex != null) {
									setSector(segment.sectorIndex);
									setTelemetryWindow(null);
								} else {
									setSector(null);
									setTelemetryWindow({ startM: segment.startM, endM: segment.endM });
								}
							}}
						/>
					</div>
				)}
				<div className="comparison-telemetry-content">
					{!comparison && (
						<div className="telemetry-range">
							<SectorPicker
								samples={baseSamples}
								value={sector}
								onChange={(value) => {
									setPlaying(false);
									setSector(value);
									setTelemetryWindow(null);
									setCursorIndex(null);
								}}
							/>
						</div>
					)}
					{comparison ? (
						<ComparisonTelemetryHud
							samples={samples}
							cursorIndex={cursorIndex}
							onSeek={(index) => seek(index)}
							referenceLabel={comparisonName(comparison.reference)}
							comparisonLabel={comparisonName(comparison.analysed)}
							trackCarLabel={[comparison.reference.detail.summary.track, comparison.reference.detail.summary.car].filter(Boolean).join(" · ")}
							sectorDeltas={sectorDeltas}
							sector={sector}
							onSector={(value) => {
								setPlaying(false);
								setSector(value);
								setTelemetryWindow(null);
								setCursorIndex(null);
							}}
							onSwap={() => window.dispatchEvent(new Event("trace:comparison-swap"))}
							onClose={() => window.dispatchEvent(new Event("trace:comparison-clear"))}
							layout="embedded"
							distanceUnit="%"
							playing={playing}
							onTogglePlayback={togglePlayback}
							playbackSpeed={playbackSpeed}
							onPlaybackSpeed={changePlaybackSpeed}
						/>
					) : (
						<LapTelemetryHud
							identity={[detail?.summary.track, detail?.summary.car].filter(Boolean).join(" · ") || detail?.summary.title || "Recorded lap"}
							secondary={`LAP ${lap.index} · ${formatSessionType(detail?.summary.session_type)}`}
							samples={samples}
							cursorIndex={cursorIndex}
							onSeek={(index) => seek(index)}
							layout="embedded"
							distanceUnit="%"
							playing={playing}
							onTogglePlayback={togglePlayback}
							playbackSpeed={playbackSpeed}
							onPlaybackSpeed={changePlaybackSpeed}
						/>
					)}
					{!comparison && <LapMetricsSummary metric={lapMetric} allMetrics={detail?.lap_metrics || []} />}
					<div className="telemetry-primary-grid">
						<div className="telemetry-primary-charts">
							<ComparisonChart
								label="SPEED"
								unit="km/h"
								samples={samples}
								series={speedSeries}
								cursorIndex={cursorIndex}
								onCursor={seek}
								onZoom={zoomTelemetry}
								onPan={panTelemetry}
								distanceUnit="%"
							/>
							{comparison && (
								<ComparisonChart
									label="TIME DELTA · ANALYSED VS REFERENCE"
									unit="s"
									samples={samples}
									series={singleSeries("deltaSeconds", channelColours.delta)}
									cursorIndex={cursorIndex}
									onCursor={seek}
									onZoom={zoomTelemetry}
									onPan={panTelemetry}
									fixedRange={deltaRange(samples)}
									zeroLine
									distanceUnit="%"
								/>
							)}
						</div>
						<div className="web-track-map">
							<TrackMap
								samples={samples}
								cursorIndex={cursorIndex}
								comparison={Boolean(comparison)}
								height={comparison ? 512 : 268}
								focusSelection={sector != null || telemetryWindow != null}
								rangeLabel={sector != null ? `SECTOR ${sector}` : undefined}
								onRangeZoom={zoomTelemetry}
								rangeZoomLinked={false}
							/>
						</div>
					</div>
					<div className="visual-grid telemetry-secondary-grid">
						<div className="telemetry-chart-wide">
							<ComparisonChart
								label="THROTTLE"
								unit="%"
								samples={samples}
								series={throttleSeries}
								cursorIndex={cursorIndex}
								onCursor={seek}
								onZoom={zoomTelemetry}
								onPan={panTelemetry}
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
								onPan={panTelemetry}
								fixedRange={[0, 100]}
								distanceUnit="%"
							/>
						</div>
						<div className="telemetry-chart-wide">
							<ComparisonChart
								label="STEERING ANGLE"
								unit="°"
								samples={samples}
								series={steeringSeries}
								cursorIndex={cursorIndex}
								onCursor={seek}
								onZoom={zoomTelemetry}
								onPan={panTelemetry}
								fixedRange={steeringAngleRange(samples)}
								zeroLine
								distanceUnit="%"
							/>
						</div>
						<ComparisonChart
							label="ENGINE SPEED"
							unit="rpm"
							samples={samples}
							series={rpmSeries}
							cursorIndex={cursorIndex}
							onCursor={seek}
							onZoom={zoomTelemetry}
							onPan={panTelemetry}
							distanceUnit="%"
						/>
						<ComparisonChart
							label="GEAR"
							unit=""
							samples={samples}
							series={gearSeries}
							cursorIndex={cursorIndex}
							onCursor={seek}
							onZoom={zoomTelemetry}
							onPan={panTelemetry}
							fixedRange={[-1, 8]}
							formatValue={(value) => formatGear(value)}
							distanceUnit="%"
						/>
					</div>
				</div>
			</div>
		</>
	);
}

const workspaceRoot = document.querySelector("#telemetry-workspace-root");
if (workspaceRoot) createRoot(workspaceRoot).render(<TelemetryWorkspace />);

export { TelemetryWorkspace };
