import { useEffect, useMemo, useState } from "react";
import { telemetryDataSource, type LapComparisonSample, type LapTrace, type RecordedSessionSummary } from "../../data-source";
import { PageIntro } from "../../components/layout";
import { channelColours, ComparisonChart, formatGear, singleSeries, steeringInputRange } from "../telemetry/ComparisonChart";
import { FloatingTrackMap, TrackMap, useTrackMapPip } from "../telemetry/TrackMap";
import { filterSamplesBySector, filterSamplesByTelemetryWindow, nextTelemetryWindow, type TelemetryWindow } from "../telemetry/telemetry-window";
import { SectorPicker, TelemetryHud } from "../compare/ComparePage";

export function LapVisualizer({ session, lapIndex }: { session: RecordedSessionSummary; lapIndex: number }) {
	const [trace, setTrace] = useState<LapTrace | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [cursorIndex, setCursorIndex] = useState<number | null>(null);
	const [sector, setSector] = useState<number | null>(null);
	const [telemetryWindow, setTelemetryWindow] = useState<TelemetryWindow | null>(null);
	const [mapZoomLinked, setMapZoomLinked] = useState(false);
	const mapPip = useTrackMapPip(trace != null);

	useEffect(() => {
		let active = true;
		setTrace(null);
		setError(null);
		setTelemetryWindow(null);
		void telemetryDataSource
			.visualizeSessionLap(session.id, lapIndex)
			.then((value) => {
				if (active) setTrace(value);
			})
			.catch((reason) => {
				if (active) setError(reason instanceof Error ? reason.message : String(reason));
			});
		return () => {
			active = false;
		};
	}, [lapIndex, session.id]);

	const chartSamples = useMemo<LapComparisonSample[]>(
		() =>
			trace?.samples.map((sample) => ({
				distanceM: sample.distanceM,
				sectorIndex: sample.sectorIndex,
				referenceSpeedKmh: sample.speedKmh,
				referenceThrottlePercent: sample.throttlePercent,
				referenceBrakePercent: sample.brakePercent,
				referenceSteeringPercent: sample.steeringPercent,
				referenceRpm: sample.rpm,
				referenceGear: sample.gear,
				referencePositionXM: sample.positionXM,
				referencePositionZM: sample.positionZM,
				referenceAirTemperatureC: sample.airTemperatureC,
				referenceTrackTemperatureC: sample.trackTemperatureC,
			})) ?? [],
		[trace],
	);
	const baseSamples = filterSamplesBySector(chartSamples, sector);
	const samples = filterSamplesByTelemetryWindow(baseSamples, telemetryWindow);
	const cursor = cursorIndex == null ? null : (samples[cursorIndex] ?? null);
	const zoomTelemetry = (anchorM: number, direction: "in" | "out") => {
		setTelemetryWindow((current) => nextTelemetryWindow(baseSamples, current, anchorM, direction));
		setCursorIndex(null);
	};

	return (
		<>
			<PageIntro
				index="02"
				eyebrow="LAP VISUALIZER"
				title={`LAP ${lapIndex} · ${trace?.lapTime ?? session.laps.find((lap) => lap.index === lapIndex)?.time ?? "—"}`}
				description={`${session.track} · ${session.car}. Inspect the recorded driving inputs and line on one synchronized distance axis.`}
			/>
			{!trace && !error && (
				<div className="mt-7 border border-trace-divider bg-trace-surface p-8 font-mono text-[12px] text-trace-dim">PREPARING LAP TELEMETRY…</div>
			)}
			{error && (
				<div className="mt-7 border border-trace-warning/50 bg-trace-warning/10 p-5 text-[13px] text-trace-warning">
					<strong>Lap visualization unavailable.</strong> {error}
				</div>
			)}
			{trace && (
				<div className="mt-7">
					<div className="flex items-center justify-between border border-trace-divider bg-trace-surface px-5 py-3">
						<SectorPicker
							samples={chartSamples}
							value={sector}
							onChange={(value) => {
								setSector(value);
								setTelemetryWindow(null);
								setCursorIndex(null);
							}}
						/>
						<div className="flex gap-6 font-mono text-[12px] text-trace-muted">
							{telemetryWindow && (
								<button
									type="button"
									onClick={() => {
										setTelemetryWindow(null);
										setCursorIndex(null);
									}}
									className="font-bold text-trace-accent hover:text-trace-text"
								>
									{Math.round(telemetryWindow.startM)}–{Math.round(telemetryWindow.endM)} M · RESET ZOOM
								</button>
							)}
							<span>{Math.round(cursor?.distanceM ?? trace.lapLengthM).toLocaleString()} M</span>
							<span>
								GEAR <strong className="text-trace-text">{formatGear(cursor?.referenceGear)}</strong>
							</span>
						</div>
					</div>
					<div className="mt-3 pb-32">
						<div className="grid grid-cols-[minmax(0,3fr)_minmax(380px,1fr)] gap-3">
							<div className="grid gap-3">
								<ComparisonChart
									label="SPEED"
									unit="km/h"
									samples={samples}
									cursorIndex={cursorIndex}
									onCursor={setCursorIndex}
									onZoom={zoomTelemetry}
									series={singleSeries("referenceSpeedKmh", channelColours.speed)}
								/>
								<ComparisonChart
									label="GEAR"
									unit=""
									samples={samples}
									cursorIndex={cursorIndex}
									onCursor={setCursorIndex}
									onZoom={zoomTelemetry}
									fixedRange={[-1, 8]}
									formatValue={(value) => formatGear(value)}
									series={singleSeries("referenceGear", channelColours.gear)}
								/>
							</div>
							<div ref={mapPip.anchor} className="min-w-0">
								<TrackMap
									samples={samples}
									cursorIndex={cursorIndex}
									trackMap={trace.trackMap}
									height={508}
									focusSelection={sector != null || telemetryWindow != null}
									rangeLabel={
										sector != null
											? `SECTOR ${sector}`
											: telemetryWindow
												? `${Math.round(telemetryWindow.startM)}–${Math.round(telemetryWindow.endM)} M`
												: undefined
									}
									onRangeZoom={zoomTelemetry}
									rangeZoomLinked={mapZoomLinked}
									onRangeZoomLinked={setMapZoomLinked}
								/>
							</div>
						</div>
						<div className="mt-3 grid gap-3">
							<ComparisonChart
								label="THROTTLE"
								unit="%"
								samples={samples}
								cursorIndex={cursorIndex}
								onCursor={setCursorIndex}
								onZoom={zoomTelemetry}
								fixedRange={[0, 100]}
								series={singleSeries("referenceThrottlePercent", channelColours.throttle)}
							/>
							<ComparisonChart
								label="BRAKE"
								unit="%"
								samples={samples}
								cursorIndex={cursorIndex}
								onCursor={setCursorIndex}
								onZoom={zoomTelemetry}
								fixedRange={[0, 100]}
								series={singleSeries("referenceBrakePercent", channelColours.brake)}
							/>
							<ComparisonChart
								label="STEERING INPUT"
								unit="%"
								samples={samples}
								cursorIndex={cursorIndex}
								onCursor={setCursorIndex}
								onZoom={zoomTelemetry}
								fixedRange={steeringInputRange(samples)}
								zeroLine
								series={singleSeries("referenceSteeringPercent", channelColours.steering)}
							/>
						</div>
					</div>
					{mapPip.visible && (
						<FloatingTrackMap
							samples={samples}
							cursorIndex={cursorIndex}
							trackMap={trace.trackMap}
							focusSelection={sector != null || telemetryWindow != null}
							rangeLabel={
								sector != null
									? `SECTOR ${sector}`
									: telemetryWindow
										? `${Math.round(telemetryWindow.startM)}–${Math.round(telemetryWindow.endM)} M`
										: undefined
							}
							onRangeZoom={zoomTelemetry}
							rangeZoomLinked={mapZoomLinked}
							onRangeZoomLinked={setMapZoomLinked}
							onDismiss={mapPip.dismiss}
						/>
					)}
					<TelemetryHud session={session} lapIndex={lapIndex} samples={samples} cursorIndex={cursorIndex} onSeek={setCursorIndex} />
				</div>
			)}
		</>
	);
}
