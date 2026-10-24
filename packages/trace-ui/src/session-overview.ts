export interface SessionOverviewSector {
	index: number;
	durationNs: number | null;
}

export interface SessionOverviewLap {
	id: string;
	index: number;
	durationNs: number | null;
	invalid: boolean;
	sectors: SessionOverviewSector[];
}

export interface SessionOverview {
	sectorIndices: number[];
	bestLapId: string | null;
	bestLapDurationNs: number | null;
	theoreticalBestDurationNs: number | null;
	validTimedLapCount: number;
}

export function sessionOverview(laps: SessionOverviewLap[]): SessionOverview {
	const sectorIndices = [...new Set(laps.flatMap((lap) => lap.sectors.map((sector) => sector.index)))].sort((left, right) => left - right);
	const validTimedLaps = laps.filter((lap) => !lap.invalid && lap.durationNs != null && Number.isFinite(lap.durationNs) && lap.durationNs > 0);
	const bestLap = validTimedLaps.reduce<SessionOverviewLap | null>(
		(best, lap) => (!best || (lap.durationNs ?? Number.POSITIVE_INFINITY) < (best.durationNs ?? Number.POSITIVE_INFINITY) ? lap : best),
		null,
	);
	let theoreticalBestDurationNs: number | null = sectorIndices.length > 0 ? 0 : null;
	for (const index of sectorIndices) {
		const durations = laps
			.filter((lap) => !lap.invalid)
			.flatMap((lap) => lap.sectors)
			.filter((sector) => sector.index === index && sector.durationNs != null && Number.isFinite(sector.durationNs) && sector.durationNs > 0)
			.map((sector) => sector.durationNs as number);
		if (durations.length === 0) {
			theoreticalBestDurationNs = null;
			break;
		}
		theoreticalBestDurationNs = (theoreticalBestDurationNs ?? 0) + Math.min(...durations);
	}
	return {
		sectorIndices,
		bestLapId: bestLap?.id ?? null,
		bestLapDurationNs: bestLap?.durationNs ?? null,
		theoreticalBestDurationNs,
		validTimedLapCount: validTimedLaps.length,
	};
}

export function sectorPerformance(laps: SessionOverviewLap[], lap: SessionOverviewLap, sectorIndex: number) {
	const sector = lap.sectors.find((candidate) => candidate.index === sectorIndex);
	if (!sector || sector.durationNs == null || lap.invalid) return "grey" as const;
	const comparable = laps
		.filter((candidate) => !candidate.invalid)
		.flatMap((candidate) => candidate.sectors.filter((item) => item.index === sectorIndex && item.durationNs != null));
	const sessionBest = Math.min(...comparable.map((candidate) => candidate.durationNs as number));
	if (sector.durationNs === sessionBest) return "purple" as const;
	const priorBest = Math.min(
		...laps
			.filter((candidate) => candidate.index < lap.index && !candidate.invalid)
			.flatMap((candidate) => candidate.sectors.filter((item) => item.index === sectorIndex && item.durationNs != null))
			.map((candidate) => candidate.durationNs as number),
	);
	return sector.durationNs < priorBest ? ("green" as const) : ("yellow" as const);
}

export function formatLapDurationNs(nanoseconds: number | null) {
	if (nanoseconds == null || !Number.isFinite(nanoseconds)) return "—";
	const milliseconds = Math.floor(nanoseconds / 1_000_000);
	const minutes = Math.floor(milliseconds / 60_000);
	const seconds = Math.floor((milliseconds % 60_000) / 1_000);
	return `${minutes}:${String(seconds).padStart(2, "0")}.${String(milliseconds % 1_000).padStart(3, "0")}`;
}
