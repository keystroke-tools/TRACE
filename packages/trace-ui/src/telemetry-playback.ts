import type { LapComparisonSample } from "./telemetry-chart";

export const telemetryPlaybackSpeeds = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 4] as const;

function finiteElapsed(sample: LapComparisonSample) {
	const elapsed = sample.comparisonElapsedSeconds ?? sample.referenceElapsedSeconds;
	return elapsed != null && Number.isFinite(elapsed) ? elapsed : null;
}

export function telemetryPlaybackTimeline(samples: LapComparisonSample[]) {
	let elapsed = finiteElapsed(samples[0]) ?? 0;
	return samples.map((sample, index) => {
		const reported = finiteElapsed(sample);
		if (reported != null && (index === 0 || reported > elapsed)) {
			elapsed = reported;
		} else if (index > 0) {
			const previous = samples[index - 1];
			const distance = Math.max(0, sample.distanceM - previous.distanceM);
			const speedKmh = sample.comparisonSpeedKmh ?? sample.referenceSpeedKmh;
			const estimatedStep = speedKmh != null && speedKmh > 1 ? distance / (speedKmh / 3.6) : 1 / 60;
			elapsed += Math.min(0.25, Math.max(1 / 240, estimatedStep));
		}
		return elapsed;
	});
}

export function nearestTelemetryPlaybackIndex(timeline: number[], target: number) {
	if (target <= (timeline[0] ?? 0)) return 0;
	if (target >= (timeline.at(-1) ?? 0)) return Math.max(0, timeline.length - 1);
	let low = 0;
	let high = timeline.length - 1;
	while (low < high) {
		const middle = Math.floor((low + high) / 2);
		if (timeline[middle] < target) low = middle + 1;
		else high = middle;
	}
	if (low === 0) return 0;
	return target - timeline[low - 1] <= timeline[low] - target ? low - 1 : low;
}
