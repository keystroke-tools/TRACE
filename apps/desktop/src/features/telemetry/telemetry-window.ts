import type { LapComparisonSample } from "../../data-source";

export function filterSamplesBySector(samples: LapComparisonSample[], sector: number | null) {
	return sector == null ? samples : samples.filter((sample) => sample.sectorIndex === sector);
}

export function filterSamplesByDistance(samples: LapComparisonSample[], startDistanceM: number, endDistanceM: number) {
	return samples.filter((sample) => sample.distanceM >= startDistanceM && sample.distanceM <= endDistanceM);
}

export type TelemetryWindow = { startM: number; endM: number };

export function filterSamplesByTelemetryWindow(samples: LapComparisonSample[], window: TelemetryWindow | null) {
	if (window == null) return samples;
	return samples.filter((sample) => sample.distanceM >= window.startM && sample.distanceM <= window.endM);
}

export function nextTelemetryWindow(
	samples: LapComparisonSample[],
	current: TelemetryWindow | null,
	anchorM: number,
	direction: "in" | "out",
): TelemetryWindow | null {
	const baseStart = samples[0]?.distanceM;
	const baseEnd = samples.at(-1)?.distanceM;
	if (baseStart == null || baseEnd == null || baseEnd <= baseStart) return current;
	const start = current?.startM ?? baseStart;
	const end = current?.endM ?? baseEnd;
	const span = end - start;
	const baseSpan = baseEnd - baseStart;
	const minimumSpan = Math.max(50, baseSpan / 64);
	const nextSpan = Math.min(baseSpan, Math.max(minimumSpan, span * (direction === "in" ? 0.7 : 1 / 0.7)));
	if (nextSpan >= baseSpan * 0.995) return null;
	const anchor = Math.min(end, Math.max(start, anchorM));
	const ratio = span <= 0 ? 0.5 : (anchor - start) / span;
	let nextStart = anchor - nextSpan * ratio;
	let nextEnd = nextStart + nextSpan;
	if (nextStart < baseStart) {
		nextStart = baseStart;
		nextEnd = baseStart + nextSpan;
	}
	if (nextEnd > baseEnd) {
		nextEnd = baseEnd;
		nextStart = baseEnd - nextSpan;
	}
	return { startM: nextStart, endM: nextEnd };
}
