import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { createRoot } from "react-dom/client";

type Point = {
	elapsed_seconds?: number | null;
	speed_kph?: number | null;
	throttle?: number | null;
	brake?: number | null;
	engine_rpm?: number | null;
	position_x_m?: number | null;
	position_z_m?: number | null;
};

type Telemetry = { points: Point[] };
type Lap = { id: string; index: number; duration_ns: number | null };
type Detail = { summary: { title: string | null; driver_name: string | null } };
type Entry = { detail: Detail; lap: Lap; telemetry: Telemetry };

type Sample = {
	reference_speed_kph?: number | null;
	analysed_speed_kph?: number | null;
	reference_throttle?: number | null;
	analysed_throttle?: number | null;
	reference_brake?: number | null;
	analysed_brake?: number | null;
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

type Series = { values: Array<number | null | undefined>; color: string; maximum?: number };
type Trace = { points: Point[]; color: string; width: number };

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

function canvasContext(canvas: HTMLCanvasElement) {
	const rectangle = canvas.getBoundingClientRect();
	const ratio = window.devicePixelRatio || 1;
	canvas.width = Math.max(1, Math.round(rectangle.width * ratio));
	canvas.height = Math.max(1, Math.round(rectangle.height * ratio));
	const context = canvas.getContext("2d");
	if (!context) return null;
	context.scale(ratio, ratio);
	context.lineJoin = "round";
	context.lineCap = "round";
	return { context, width: rectangle.width, height: rectangle.height };
}

function drawTrack(canvas: HTMLCanvasElement, traces: Trace[]) {
	const surface = canvasContext(canvas);
	if (!surface) return;
	const { context, width, height } = surface;
	const positionedTraces = traces.map((trace) => ({
		...trace,
		points: trace.points.filter((point) => Number.isFinite(point.position_x_m) && Number.isFinite(point.position_z_m)),
	}));
	const positions = positionedTraces.flatMap((trace) => trace.points);
	if (positions.length < 2) {
		context.fillStyle = "#777";
		context.font = "12px system-ui";
		context.textAlign = "center";
		context.fillText("TRACK POSITION NOT RECORDED", width / 2, height / 2);
		return;
	}
	const xs = positions.map((point) => point.position_x_m as number);
	const zs = positions.map((point) => point.position_z_m as number);
	const minX = Math.min(...xs);
	const maxX = Math.max(...xs);
	const minZ = Math.min(...zs);
	const maxZ = Math.max(...zs);
	const padding = 24;
	const scale = Math.min((width - padding * 2) / Math.max(maxX - minX, 1), (height - padding * 2) / Math.max(maxZ - minZ, 1));
	const offsetX = (width - (maxX - minX) * scale) / 2;
	const offsetZ = (height - (maxZ - minZ) * scale) / 2;
	for (const trace of positionedTraces) {
		context.strokeStyle = trace.color;
		context.lineWidth = trace.width;
		context.beginPath();
		trace.points.forEach((point, index) => {
			const x = offsetX + ((point.position_x_m as number) - minX) * scale;
			const y = offsetZ + ((point.position_z_m as number) - minZ) * scale;
			if (index === 0) context.moveTo(x, y);
			else context.lineTo(x, y);
		});
		context.stroke();
	}
}

function drawSeries(canvas: HTMLCanvasElement, series: Series[], zeroCentered = false) {
	const surface = canvasContext(canvas);
	if (!surface) return;
	const { context, width, height } = surface;
	const padding = 12;
	context.strokeStyle = "#292929";
	context.lineWidth = 1;
	for (let line = 1; line < 4; line += 1) {
		const y = padding + ((height - padding * 2) * line) / 4;
		context.beginPath();
		context.moveTo(0, y);
		context.lineTo(width, y);
		context.stroke();
	}
	const allValues = series.flatMap((item) => item.values.filter((value): value is number => Number.isFinite(value)));
	const absoluteMaximum = Math.max(0.01, ...allValues.map(Math.abs));
	const sharedMaximum = zeroCentered ? absoluteMaximum : Math.max(1, ...allValues);
	if (zeroCentered) {
		context.strokeStyle = "#555";
		context.beginPath();
		context.moveTo(0, height / 2);
		context.lineTo(width, height / 2);
		context.stroke();
	}
	for (const item of series) {
		const maximum = item.maximum || sharedMaximum;
		context.strokeStyle = item.color;
		context.lineWidth = 2;
		context.beginPath();
		let started = false;
		item.values.forEach((value, index) => {
			if (!Number.isFinite(value)) return;
			const x = item.values.length === 1 ? 0 : (index / (item.values.length - 1)) * width;
			const normalized = zeroCentered ? ((value as number) + maximum) / (maximum * 2) : Math.max(0, value as number) / maximum;
			const y = height - padding - normalized * (height - padding * 2);
			if (started) context.lineTo(x, y);
			else context.moveTo(x, y);
			started = true;
		});
		context.stroke();
	}
}

function useCanvas(draw: (canvas: HTMLCanvasElement) => void, dependencies: unknown[]): RefObject<HTMLCanvasElement | null> {
	const canvas = useRef<HTMLCanvasElement>(null);
	useLayoutEffect(() => {
		const element = canvas.current;
		if (!element) return;
		const redraw = () => draw(element);
		redraw();
		const observer = new ResizeObserver(redraw);
		observer.observe(element);
		return () => observer.disconnect();
		// The caller supplies the exact drawing dependencies.
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, dependencies);
	return canvas;
}

function TrackCanvas({ traces }: { traces: Trace[] }) {
	const canvas = useCanvas((element) => drawTrack(element, traces), [traces]);
	return <canvas ref={canvas} />;
}

function SeriesCanvas({ series, zeroCentered = false }: { series: Series[]; zeroCentered?: boolean }) {
	const canvas = useCanvas((element) => drawSeries(element, series, zeroCentered), [series, zeroCentered]);
	return <canvas ref={canvas} />;
}

function TelemetryWorkspace() {
	const snapshot = useWorkspace();
	if (!snapshot?.lap || !snapshot.telemetry)
		return <div className="telemetry-empty">{snapshot?.message || "Select a recorded lap to review its telemetry."}</div>;
	const { lap, telemetry, comparison } = snapshot;
	const maximumSpeed = Math.max(0, ...telemetry.points.map((point) => point.speed_kph || 0));
	const maximumRpm = Math.max(0, ...telemetry.points.map((point) => point.engine_rpm || 0));
	const traces: Trace[] = comparison
		? [
				{ points: comparison.analysed.telemetry.points, color: "#c9ff00", width: 2 },
				{ points: comparison.reference.telemetry.points, color: "#b49cff", width: 3 },
			]
		: [{ points: telemetry.points, color: "#d5d5d5", width: 3 }];
	const speed: Series[] = comparison
		? [
				{ values: comparison.samples.map((sample) => sample.analysed_speed_kph), color: "#65a9ff" },
				{ values: comparison.samples.map((sample) => sample.reference_speed_kph), color: "#b49cff" },
			]
		: [{ values: telemetry.points.map((point) => point.speed_kph), color: "#65a9ff" }];
	const throttle: Series[] = comparison
		? [
				{ values: comparison.samples.map((sample) => sample.analysed_throttle), color: "#c9ff00", maximum: 1 },
				{ values: comparison.samples.map((sample) => sample.reference_throttle), color: "#b49cff", maximum: 1 },
			]
		: [{ values: telemetry.points.map((point) => point.throttle), color: "#c9ff00", maximum: 1 }];
	const brake: Series[] = comparison
		? [
				{ values: comparison.samples.map((sample) => sample.analysed_brake), color: "#ef6969", maximum: 1 },
				{ values: comparison.samples.map((sample) => sample.reference_brake), color: "#b49cff", maximum: 1 },
			]
		: [{ values: telemetry.points.map((point) => point.brake), color: "#ef6969", maximum: 1 }];

	return (
		<>
			<div className="telemetry-head">
				<div>
					<span>LAP {lap.index}</span>
					<strong>{formatDuration(lap.duration_ns)}</strong>
				</div>
				<div className="telemetry-stats">
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
			<div className="visual-grid">
				<div className="visual-card map-card">
					<div className="visual-label">DRIVING LINE</div>
					<TrackCanvas traces={traces} />
				</div>
				<div className="visual-card">
					<div className="visual-label">SPEED</div>
					<SeriesCanvas series={speed} />
				</div>
				<div className="visual-card">
					<div className="visual-label">THROTTLE</div>
					<SeriesCanvas series={throttle} />
				</div>
				<div className="visual-card">
					<div className="visual-label">BRAKE</div>
					<SeriesCanvas series={brake} />
				</div>
				{comparison && (
					<div className="visual-card delta-card">
						<div className="visual-label">TIME DELTA · ANALYSED VS REFERENCE</div>
						<SeriesCanvas series={[{ values: comparison.samples.map((sample) => sample.delta_seconds), color: "#e8e8e8" }]} zeroCentered />
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
