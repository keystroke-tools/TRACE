import type { RecordedSessionSummary } from "../../data-source";

export interface RankedDrivingItem {
	key: string;
	label: string;
	detail?: string;
	sessionCount: number;
	lapCount: number;
}

export interface ComboSessionPoint {
	sessionId: string;
	startedAt: string;
	bestLapNs: number;
}

export interface CombinationStat {
	key: string;
	simulatorId: string;
	simulatorName: string;
	track: string;
	car: string;
	sessionCount: number;
	lapCount: number;
	bestLapNs: number;
	bestSessionId: string;
	bestStartedAt: string;
	bestDriver?: string | null;
	points: ComboSessionPoint[];
}

export interface DrivingStats {
	sessionCount: number;
	lapCount: number;
	drivingTimeNs: number;
	combinationCount: number;
	simulators: RankedDrivingItem[];
	tracks: RankedDrivingItem[];
	cars: RankedDrivingItem[];
	combinations: CombinationStat[];
}

export type StatsTimeRange = "all" | "30" | "90" | "365";

export interface StatsSessionFilters {
	simulatorId?: string;
	sessionType?: string;
	timeRange?: StatsTimeRange;
	focusKind?: "car" | "track";
	focusKey?: string;
}

function isDrivenSession(session: RecordedSessionSummary) {
	const source = session.source.toLocaleLowerCase();
	return !source.includes("import") && !source.includes("replay");
}

export function filterStatsSessions(sessions: RecordedSessionSummary[], filters: StatsSessionFilters, nowMs = Date.now()) {
	const cutoffDays = filters.timeRange === "all" || filters.timeRange == null ? null : Number(filters.timeRange);
	const cutoffMs = cutoffDays == null ? null : nowMs - cutoffDays * 86_400_000;
	return sessions.filter((session) => {
		if (!isDrivenSession(session)) return false;
		if (filters.simulatorId && session.simulatorId !== filters.simulatorId) return false;
		if (filters.sessionType && session.sessionType !== filters.sessionType) return false;
		if (cutoffMs != null) {
			const startedAt = new Date(session.startedAt).getTime();
			if (!Number.isFinite(startedAt) || startedAt < cutoffMs) return false;
		}
		if (filters.focusKind && filters.focusKey) {
			const value = filters.focusKind === "car" ? session.car : session.track;
			if (`${session.simulatorId}\u0000${value}` !== filters.focusKey) return false;
		}
		return true;
	});
}

function completedLaps(session: RecordedSessionSummary) {
	return session.laps.filter((lap) => typeof lap.durationNs === "number" && lap.durationNs > 0);
}

function sectorCount(lap: RecordedSessionSummary["laps"][number]) {
	return new Set(lap.sectors.map((sector) => sector.index)).size;
}

function expectedSectorCounts(sessions: RecordedSessionSummary[]) {
	const frequencies = new Map<string, Map<number, number>>();
	for (const session of sessions) {
		const key = [session.simulatorId, session.track, session.car].join("\u0000");
		const counts = frequencies.get(key) ?? new Map<number, number>();
		for (const lap of completedLaps(session)) {
			if (lap.validity === "invalid") continue;
			const count = sectorCount(lap);
			if (count > 0) counts.set(count, (counts.get(count) ?? 0) + 1);
		}
		frequencies.set(key, counts);
	}
	return new Map(
		[...frequencies].flatMap(([key, counts]) => {
			const expected = [...counts].sort((left, right) => right[1] - left[1] || right[0] - left[0])[0]?.[0];
			return expected == null ? [] : [[key, expected] as const];
		}),
	);
}

function paceLaps(session: RecordedSessionSummary, expectedSectors?: number) {
	return completedLaps(session).filter((lap) => lap.validity !== "invalid" && (expectedSectors == null || sectorCount(lap) === expectedSectors));
}

function rank(
	sessions: RecordedSessionSummary[],
	identity: (session: RecordedSessionSummary) => { key: string; label: string; detail?: string },
): RankedDrivingItem[] {
	const values = new Map<string, RankedDrivingItem>();
	for (const session of sessions) {
		const item = identity(session);
		const current = values.get(item.key) ?? { ...item, sessionCount: 0, lapCount: 0 };
		current.sessionCount += 1;
		current.lapCount += completedLaps(session).length;
		values.set(item.key, current);
	}
	return [...values.values()].sort(
		(left, right) => right.lapCount - left.lapCount || right.sessionCount - left.sessionCount || left.label.localeCompare(right.label),
	);
}

export function calculateDrivingStats(allSessions: RecordedSessionSummary[]): DrivingStats {
	const sessions = allSessions.filter(isDrivenSession);
	const sectorCounts = expectedSectorCounts(sessions);
	const combinations = new Map<string, CombinationStat>();
	let lapCount = 0;
	let drivingTimeNs = 0;

	for (const session of sessions) {
		const completed = completedLaps(session);
		const key = [session.simulatorId, session.track, session.car].join("\u0000");
		const laps = paceLaps(session, sectorCounts.get(key));
		lapCount += completed.length;
		drivingTimeNs += completed.reduce((total, lap) => total + (lap.durationNs ?? 0), 0);
		if (laps.length === 0) continue;
		const bestLapNs = Math.min(...laps.map((lap) => lap.durationNs as number));
		const current = combinations.get(key);
		if (!current) {
			combinations.set(key, {
				key,
				simulatorId: session.simulatorId,
				simulatorName: session.simulatorName,
				track: session.track,
				car: session.car,
				sessionCount: 1,
				lapCount: completed.length,
				bestLapNs,
				bestSessionId: session.id,
				bestStartedAt: session.startedAt,
				bestDriver: session.driver,
				points: [{ sessionId: session.id, startedAt: session.startedAt, bestLapNs }],
			});
			continue;
		}
		current.sessionCount += 1;
		current.lapCount += completed.length;
		current.points.push({ sessionId: session.id, startedAt: session.startedAt, bestLapNs });
		if (bestLapNs < current.bestLapNs) {
			current.bestLapNs = bestLapNs;
			current.bestSessionId = session.id;
			current.bestStartedAt = session.startedAt;
			current.bestDriver = session.driver;
		}
	}

	const combinationList = [...combinations.values()]
		.map((combination) => ({
			...combination,
			points: combination.points.sort((left, right) => new Date(left.startedAt).getTime() - new Date(right.startedAt).getTime()),
		}))
		.sort((left, right) => right.lapCount - left.lapCount || left.track.localeCompare(right.track));

	return {
		sessionCount: sessions.length,
		lapCount,
		drivingTimeNs,
		combinationCount: combinationList.length,
		simulators: rank(sessions, (session) => ({ key: session.simulatorId, label: session.simulatorName })),
		tracks: rank(sessions, (session) => ({ key: `${session.simulatorId}\u0000${session.track}`, label: session.track, detail: session.simulatorName })),
		cars: rank(sessions, (session) => ({ key: `${session.simulatorId}\u0000${session.car}`, label: session.car, detail: session.simulatorName })),
		combinations: combinationList,
	};
}
