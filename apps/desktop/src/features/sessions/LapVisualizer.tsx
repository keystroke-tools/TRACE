import { useEffect, useMemo, useState } from "react";
import { SectorPicker } from "@trace/ui";
import { telemetryDataSource, type LapComparisonSample, type LapTrace, type RecordedSessionSummary } from "../../data-source";
import { PageIntro } from "../../components/layout";
import { channelColours, ComparisonChart, formatGear, singleSeries, steeringAngleRange } from "../telemetry/ComparisonChart";
import { FloatingTrackMap, TrackMap, useTrackMapPip } from "../telemetry/TrackMap";
import { filterSamplesBySector, filterSamplesByTelemetryWindow, nextTelemetryWindow, type TelemetryWindow } from "../telemetry/telemetry-window";
import { TelemetryHud } from "../compare/ComparePage";
import { ConsistencyChart } from "./ConsistencyChart";

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
				referenceSteeringDegrees: sample.steeringDegrees,
				referenceRpm: sample.rpm,
				referenceGear: sample.gear,
				referencePositionXM: sample.positionXM,
				referencePositionZM: sample.positionZM,
				referenceAirTemperatureC: sample.airTemperatureC,
				referenceTrackTemperatureC: sample.trackTemperatureC,
				referenceTyreLoadFrontLeftN: sample.tyreLoadFrontLeftN,
				referenceTyreLoadFrontRightN: sample.tyreLoadFrontRightN,
				referenceTyreLoadRearLeftN: sample.tyreLoadRearLeftN,
				referenceTyreLoadRearRightN: sample.tyreLoadRearRightN,
				referenceTyreSlipFrontLeft: sample.tyreSlipFrontLeft,
				referenceTyreSlipFrontRight: sample.tyreSlipFrontRight,
				referenceTyreSlipRearLeft: sample.tyreSlipRearLeft,
				referenceTyreSlipRearRight: sample.tyreSlipRearRight,
			})) ?? [],
		[trace],
	);
	const baseSamples = filterSamplesBySector(chartSamples, sector);
	const samples = filterSamplesByTelemetryWindow(baseSamples, telemetryWindow);
	const cursor = cursorIndex == null ? null : (samples[cursorIndex] ?? null);
	const hasTyreLoad = chartSamples.some((sample) =>
		[sample.referenceTyreLoadFrontLeftN, sample.referenceTyreLoadFrontRightN, sample.referenceTyreLoadRearLeftN, sample.referenceTyreLoadRearRightN].some(
			(value) => value != null,
		),
	);
	const hasTyreSlip = chartSamples.some((sample) =>
		[sample.referenceTyreSlipFrontLeft, sample.referenceTyreSlipFrontRight, sample.referenceTyreSlipRearLeft, sample.referenceTyreSlipRearRight].some(
			(value) => value != null,
		),
	);
	const tyreColours = ["#45d6e8", "#5394ff", "#ffb84d", "#d276ff"];
	const tyreLabels = ["FL", "FR", "RL", "RR"];
	const tyreLoadReaders = [
		(sample: LapComparisonSample) => sample.referenceTyreLoadFrontLeftN,
		(sample: LapComparisonSample) => sample.referenceTyreLoadFrontRightN,
		(sample: LapComparisonSample) => sample.referenceTyreLoadRearLeftN,
		(sample: LapComparisonSample) => sample.referenceTyreLoadRearRightN,
	];
	const tyreSlipReaders = [
		(sample: LapComparisonSample) => sample.referenceTyreSlipFrontLeft,
		(sample: LapComparisonSample) => sample.referenceTyreSlipFrontRight,
		(sample: LapComparisonSample) => sample.referenceTyreSlipRearLeft,
		(sample: LapComparisonSample) => sample.referenceTyreSlipRearRight,
	];
	const tyreSlipUnit = trace?.tyreSlipKind === "angle" ? "°" : trace?.tyreSlipKind === "ratio" ? "%" : "";
	const tyreSlipLabel =
		trace?.tyreSlipKind === "angle"
			? `TYRE SLIP ANGLE · PEAK ${trace.maximumAbsTyreSlip?.toFixed(1) ?? "—"}°`
			: trace?.tyreSlipKind === "ratio"
				? `TYRE SLIP RATIO · PEAK ${trace.maximumAbsTyreSlip?.toFixed(1) ?? "—"}%`
				: "AC WHEEL SLIP · SOURCE VALUE";
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
					<div className="mb-3">
						<ConsistencyChart series={[{ label: "This session", session }]} />
					</div>
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
								label="STEERING ANGLE"
								unit="°"
								samples={samples}
								cursorIndex={cursorIndex}
								onCursor={setCursorIndex}
								onZoom={zoomTelemetry}
								fixedRange={steeringAngleRange(samples)}
								zeroLine
								series={singleSeries("referenceSteeringDegrees", channelColours.steering)}
							/>
							{(hasTyreLoad || hasTyreSlip) && (
								<div className="grid gap-3 2xl:grid-cols-2">
									{hasTyreLoad && (
										<ComparisonChart
											label="TYRE LOAD"
											unit="N"
											samples={samples}
											cursorIndex={cursorIndex}
											onCursor={setCursorIndex}
											onZoom={zoomTelemetry}
											series={tyreLoadReaders.map((value, index) => ({ label: tyreLabels[index], colour: tyreColours[index], value }))}
										/>
									)}
									{hasTyreSlip && (
										<ComparisonChart
											label={tyreSlipLabel}
											unit={tyreSlipUnit}
											samples={samples}
											cursorIndex={cursorIndex}
											onCursor={setCursorIndex}
											onZoom={zoomTelemetry}
											zeroLine
											series={tyreSlipReaders.map((value, index) => ({ label: tyreLabels[index], colour: tyreColours[index], value }))}
										/>
									)}
								</div>
							)}
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
